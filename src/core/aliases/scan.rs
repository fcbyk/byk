//! 文件扫描：扫描目录下的 *.byk.json 文件并解析为 AliasFile。

use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;

use super::parse::{build_file_key, filter_invalid_keys, parse_priority, validate_filename};
use super::types::{AliasFile, Mode};
use crate::core::vars::{FILE_DIR_VAR, VarIssue, VarTable, preprocess_vars};
use crate::utils::path::{canonical, dir_of, expand_home};

pub(crate) fn scan_alias_files(dir: &Path, is_global: bool, vars: &VarTable) -> Vec<AliasFile> {
    // 本地文件默认参与合并，且优先级高于全局（1 > 0）——进目录就能 `byk <key>`。
    // 只想给本文件用（仅 @file.key 可达）时显式写 `$priority: -1`。
    let default_priority = if is_global { 0 } else { 1 };
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
        if let Some(alias_file) = parse_alias_file(&path, is_global, default_priority, vars) {
            files.push(alias_file);
        }
    }

    files
}

/// 读取一个别名文件：注入它自己的 `file_dir`，完成变量替换与告警。
///
/// 返回 `(文档, 文件路径)`。索引文件和被导入的目标文件都走这里，所以两者
/// 的 `${file_dir}` 都指向各自所在目录。
fn load_document(path: &Path, vars: &VarTable) -> Option<(serde_json::Value, PathBuf)> {
    let content = fs::read_to_string(path).ok()?;
    let mut data: serde_json::Value = serde_json::from_str(&content).ok()?;

    // 内置表是全局共用的，file_dir 逐文件不同：克隆一份局部表再注入
    let mut file_vars = vars.clone();
    file_vars.insert(FILE_DIR_VAR.to_string(), dir_of(path));

    // 变量替换：内置变量与文件 $var 合成一张表，扫描期一次替换完成
    for issue in preprocess_vars(&mut data, &file_vars) {
        report_var_issue(&issue, path);
    }

    warn_invalid_modes(&data, path);
    warn_retired_vars(&data, path);

    Some((data, path.to_path_buf()))
}

fn report_var_issue(issue: &VarIssue, path: &Path) {
    let detail = match issue {
        VarIssue::InvalidName(n) => format!(
            "variable '${{{}}}' has an invalid name (expected [A-Za-z_][A-Za-z0-9_]*), ignored",
            n
        ),
        VarIssue::Reserved(n) => format!(
            "variable '${{{}}}' collides with an alias placeholder, declaration ignored",
            n
        ),
        VarIssue::SelfReference(n) => format!(
            "variable '${{{}}}' references itself and cannot be resolved, left as-is",
            n
        ),
    };
    eprintln!("{} {} in {}", "Warning:".yellow(), detail, path.display());
}

/// 根是数组时跟随单导入指针，返回 `(目标文档, 目标路径)`。
///
/// 语义是「放弃自己的身份」：`@file` 前缀、key、默认优先级都留在索引文件上，
/// 内容与相对路径基准则完全来自目标。所以返回的路径会替换掉 `AliasFile.path`，
/// 让 `$cwd` / `$paths` / `$var` 的相对基准都落到目标目录。
///
/// 链深只允许一层：目标是数组（又一个导入文件）直接报错——索引本可以一步指到位，
/// 多一层只是隐藏真实文件的位置。
fn follow_single_import(
    data: serde_json::Value,
    index_path: &Path,
    vars: &VarTable,
) -> Option<(serde_json::Value, PathBuf)> {
    let serde_json::Value::Array(items) = &data else {
        return Some((data, index_path.to_path_buf()));
    };

    let Some(first) = items.first() else {
        eprintln!(
            "{} import file lists no path, {} is skipped",
            "Warning:".yellow(),
            index_path.display()
        );
        return None;
    };

    if items.len() > 1 {
        eprintln!(
            "{} only the first of {} import paths is used in {}",
            "Warning:".yellow(),
            items.len(),
            index_path.display()
        );
    }

    let Some(spec) = first.as_str() else {
        eprintln!(
            "{} import path must be a string in {}",
            "Warning:".yellow(),
            index_path.display()
        );
        return None;
    };

    let target = resolve_import_path(spec, index_path);
    let (target_data, target_path) = match load_document(&target, vars) {
        Some(loaded) => loaded,
        None => {
            eprintln!(
                "{} cannot load import target {} (from {})",
                "Error:".red(),
                target.display(),
                index_path.display()
            );
            return None;
        }
    };

    if target_data.is_array() {
        eprintln!(
            "{} import chains are limited to one level, {} points at another import file",
            "Error:".red(),
            index_path.display()
        );
        return None;
    }

    if !target_data.is_object() {
        eprintln!(
            "{} import target must be a JSON object: {}",
            "Error:".red(),
            target.display()
        );
        return None;
    }

    Some((target_data, target_path))
}

/// 导入路径：`~` 展开，相对路径按**索引文件所在目录**解析（与调用目录无关）。
fn resolve_import_path(spec: &str, index_path: &Path) -> PathBuf {
    let expanded = expand_home(spec);
    let joined = if expanded.is_relative() {
        index_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(expanded)
    } else {
        expanded
    };
    // 规范化，让 ${file_dir} 和 $cwd 看到的是干净路径而不是一串 ../
    canonical(&joined)
}

/// 解析单个文件；文件名非法、读取失败或 JSON 损坏时静默返回 None。
///
/// 根是对象时按别名文件处理；根是数组时视为**单导入指针**，转去解析它指向的
/// 文件（见 `follow_single_import`）。
fn parse_alias_file(
    path: &Path,
    is_global: bool,
    default_priority: i32,
    vars: &VarTable,
) -> Option<AliasFile> {
    let name = path.file_name()?.to_str()?;

    let suffix = ".byk.json";
    let stem = name.strip_suffix(suffix)?;

    if !validate_filename(stem) {
        return None;
    }

    // 读索引文件本身，再顺着指针（如果有）拿到真正的别名文档
    let (mut data, source_path) = {
        let (index_data, index_path) = load_document(path, vars)?;
        follow_single_import(index_data, &index_path, vars)?
    };

    let obj = data.as_object_mut()?;

    let priority_raw = obj.remove("$priority");
    let priority = priority_raw
        .as_ref()
        .map_or(default_priority, |v| parse_priority(v, default_priority));

    // $var 为文件级变量声明（已在变量替换阶段生效），移除避免泄漏为别名
    obj.remove("$var");

    let inherited_cwd = obj
        .remove("$cwd")
        .and_then(|v| v.as_str().map(String::from));

    let inherited_mode = obj.remove("$mode").map(|v| Mode::from_json(&v));

    let inherited_paths = obj
        .remove("$paths")
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let filtered = filter_invalid_keys(obj);

    Some(AliasFile {
        key: build_file_key(stem, is_global),
        priority,
        aliases: filtered,
        path: source_path,
        inherited_cwd,
        inherited_mode,
        inherited_paths,
    })
}

/// `$mode` 只认三个值；写错时告警并回落 `normal`。
///
/// 必须在这里喊一声：把它拼成 `"interactiv"` 会让一条本该确认的危险命令
/// 直接执行，是「配置写了没生效」里后果最重的一种。
///
/// 只走 object：数组在 `filter_invalid_keys` 里本来就会被整段丢掉，
/// 里面的 `$mode` 属于死配置，不值得为它再递归一层。
fn warn_invalid_modes(value: &serde_json::Value, path: &Path) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };

    if let Some(raw) = obj.get("$mode")
        && !matches!(
            raw.as_str(),
            Some("normal") | Some("interactive") | Some("silent")
        )
    {
        eprintln!(
            "{} $mode must be one of normal|interactive|silent, got {} in {} (treated as normal)",
            "Warning:".yellow(),
            raw,
            path.display()
        );
    }

    for child in obj.values() {
        warn_invalid_modes(child, path);
    }
}

/// 已退役的内置变量（名字, 替代写法）。
///
/// 退役后它们不再命中变量表，会原样透传给 shell，而 shell 把未定义变量展开成**空串**：
/// `cd ${workdir}/x` 静默变成 `cd /x`。这是最不该悄悄发生的一类错误，所以扫描期喊一声。
const RETIRED_VARS: &[(&str, &str)] = &[
    ("${byk}", "${byk_dir}"),
    ("${workdir}", "the shell's $PWD, or ${file_dir}"),
];

fn warn_retired_vars(value: &serde_json::Value, path: &Path) {
    let mut hits: Vec<&'static str> = Vec::new();
    collect_retired(value, &mut hits);
    for &(retired, replacement) in RETIRED_VARS {
        if hits.contains(&retired) {
            eprintln!(
                "{} {} is no longer a built-in variable, use {} in {}",
                "Warning:".yellow(),
                retired,
                replacement,
                path.display()
            );
        }
    }
}

fn collect_retired(value: &serde_json::Value, hits: &mut Vec<&'static str>) {
    match value {
        serde_json::Value::String(s) => {
            for &(retired, _) in RETIRED_VARS {
                if s.contains(retired) && !hits.contains(&retired) {
                    hits.push(retired);
                }
            }
        }
        serde_json::Value::Object(obj) => obj.values().for_each(|v| collect_retired(v, hits)),
        serde_json::Value::Array(arr) => arr.iter().for_each(|v| collect_retired(v, hits)),
        _ => {}
    }
}

/// 原地排序 AliasFile 列表：低优先级在前，高优先级在后。
///
/// 同优先级时非匿名文件按 key 字母序，匿名文件（`@` 或 `@@`）排在最后——
/// 因为后面的覆盖前面的，所以同目录里 `.byk.json` 的别名最后生效。
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
            inherited_mode: None,
            inherited_paths: Vec::new(),
        }
    }

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

    #[test]
    fn paths_extracted_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(
            &file,
            r#"{"$paths": ["./scripts", "~/tools/bin"], "build": "make build"}"#,
        )
        .unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].inherited_paths, vec!["./scripts", "~/tools/bin"]);
    }

    #[test]
    fn paths_empty_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"build": "make build"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new());
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
        let files = scan_alias_files(dir.path(), false, &VarTable::new());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].inherited_paths, vec!["./good", "./also-good"]);
    }

    #[test]
    fn paths_empty_when_not_array() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"$paths": "not-an-array", "build": "echo"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new());
        assert_eq!(files.len(), 1);
        assert!(files[0].inherited_paths.is_empty());
    }

    #[test]
    fn paths_not_leaked_into_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"$paths": ["./bin"], "build": "make"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new());
        assert_eq!(files.len(), 1);
        assert!(!files[0].aliases.contains_key("$paths"));
    }
}
