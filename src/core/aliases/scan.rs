//! 文件扫描：扫描目录下的 *.byk.json 文件并解析为 AliasFile。

use std::fs;
use std::path::Path;

use super::parse::{build_file_key, filter_invalid_keys, parse_priority, validate_filename};
use super::types::AliasFile;

// ---------------------------------------------------------------------------
// 文件扫描与解析
// ---------------------------------------------------------------------------

/// 扫描目录下所有 *.byk.json，返回 AliasFile 列表。
pub(crate) fn scan_alias_files(dir: &Path, is_global: bool) -> Vec<AliasFile> {
    let default_priority = if is_global { 0 } else { 10 };
    let mut files: Vec<AliasFile> = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !file_name.ends_with(".byk.json") {
            continue;
        }
        if let Some(alias_file) = parse_alias_file(&path, is_global, default_priority) {
            files.push(alias_file);
        }
    }

    files
}

/// 解析单个 .byk.json 文件为 AliasFile，失败返回 None。
fn parse_alias_file(path: &Path, is_global: bool, default_priority: i32) -> Option<AliasFile> {
    let name = path.file_name()?.to_str()?;

    // 提取 stem（去掉 .byk.json 后缀）
    let suffix = ".byk.json";
    let stem = name.strip_suffix(suffix)?;

    if !validate_filename(stem) {
        return None;
    }

    let content = fs::read_to_string(path).ok()?;
    let mut data: serde_json::Value = serde_json::from_str(&content).ok()?;

    let obj = data.as_object_mut()?;

    // 提取 $priority
    let priority_raw = obj.remove("$priority");
    let priority =
        priority_raw
            .as_ref()
            .map_or(default_priority, |v| parse_priority(v, default_priority));

    // 提取文件级 $cwd（所有子别名默认继承）
    let inherited_cwd = obj
        .remove("$cwd")
        .and_then(|v| v.as_str().map(String::from));

    // 提取文件级 $interactive（所有子别名默认继承）
    let inherited_interactive = obj.remove("$interactive").and_then(|v| v.as_bool());

    // 提取文件级 $paths（需要前置到 PATH 的目录列表）
    let inherited_paths = obj
        .remove("$paths")
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // 过滤非法 key
    let filtered = filter_invalid_keys(obj);

    Some(AliasFile {
        key: build_file_key(stem, is_global),
        priority,
        aliases: filtered,
        path: path.to_path_buf(),
        inherited_cwd,
        inherited_interactive,
        inherited_paths,
    })
}

// ---------------------------------------------------------------------------
// 排序
// ---------------------------------------------------------------------------

/// 原地排序 AliasFile 列表：低优先级在前，高优先级在后。
///
/// 同优先级时匿名文件（@ 或 @@）排在前，非匿名按 key 字母序。
pub(crate) fn sort_alias_files(files: &mut [AliasFile]) {
    files.sort_by(|a, b| {
        let a_is_anon = matches!(a.key.as_str(), "@" | "@@");
        let b_is_anon = matches!(b.key.as_str(), "@" | "@@");
        a.priority
            .cmp(&b.priority)
            .then_with(|| a_is_anon.cmp(&b_is_anon))
            .then_with(|| a.key.cmp(&b.key))
    });
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_file(key: &str, priority: i32) -> AliasFile {
        AliasFile {
            key: key.into(),
            priority,
            aliases: serde_json::Map::new(),
            path: PathBuf::from(format!("/fake/{}.byk.json", key.trim_start_matches('@'))),
            inherited_cwd: None,
            inherited_interactive: None,
            inherited_paths: Vec::new(),
        }
    }

    // ==================== sort_alias_files ====================

    #[test]
    fn sort_by_priority_low_first() {
        let mut files = vec![
            make_file("@high", 100),
            make_file("@low", 0),
            make_file("@mid", 50),
        ];
        sort_alias_files(&mut files);
        assert_eq!(files[0].priority, 0);
        assert_eq!(files[1].priority, 50);
        assert_eq!(files[2].priority, 100);
    }

    #[test]
    fn sort_same_priority_anonymous_first() {
        let mut files = vec![
            make_file("@named", 10),
            make_file("@", 10),
            make_file("@@", 10),
            make_file("@other", 10),
        ];
        sort_alias_files(&mut files);
        // 非匿名文件排在前面（false.cmp(&true) == Less）
        // 匿名文件（@, @@）排在后面
        assert!(matches!(files[2].key.as_str(), "@" | "@@"));
        assert!(matches!(files[3].key.as_str(), "@" | "@@"));
    }

    #[test]
    fn sort_same_priority_non_anonymous_alphabetical() {
        let mut files = vec![
            make_file("@zzz", 10),
            make_file("@aaa", 10),
            make_file("@mmm", 10),
        ];
        sort_alias_files(&mut files);
        assert_eq!(files[0].key, "@aaa");
        assert_eq!(files[1].key, "@mmm");
        assert_eq!(files[2].key, "@zzz");
    }

    #[test]
    fn sort_empty() {
        let mut files: Vec<AliasFile> = vec![];
        sort_alias_files(&mut files);
        assert!(files.is_empty());
    }

    #[test]
    fn sort_single() {
        let mut files = vec![make_file("@only", 5)];
        sort_alias_files(&mut files);
        assert_eq!(files[0].key, "@only");
    }

    // ==================== $paths 提取 ====================

    #[test]
    fn paths_extracted_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(
            &file,
            r#"{"$paths": ["./scripts", "~/tools/bin"], "build": "make build"}"#,
        )
        .unwrap();
        let files = scan_alias_files(dir.path(), false);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].inherited_paths, vec!["./scripts", "~/tools/bin"]);
    }

    #[test]
    fn paths_empty_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"build": "make build"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false);
        assert_eq!(files.len(), 1);
        assert!(files[0].inherited_paths.is_empty());
    }

    #[test]
    fn paths_filters_non_string_elements() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(
            &file,
            r#"{"$paths": ["./good", 42, null, "./also-good"], "build": "echo"}"#,
        )
        .unwrap();
        let files = scan_alias_files(dir.path(), false);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].inherited_paths, vec!["./good", "./also-good"]);
    }

    #[test]
    fn paths_empty_when_not_array() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(
            &file,
            r#"{"$paths": "not-an-array", "build": "echo"}"#,
        )
        .unwrap();
        let files = scan_alias_files(dir.path(), false);
        assert_eq!(files.len(), 1);
        assert!(files[0].inherited_paths.is_empty());
    }

    #[test]
    fn paths_not_leaked_into_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(
            &file,
            r#"{"$paths": ["./bin"], "build": "make"}"#,
        )
        .unwrap();
        let files = scan_alias_files(dir.path(), false);
        assert_eq!(files.len(), 1);
        assert!(!files[0].aliases.contains_key("$paths"));
    }
}
