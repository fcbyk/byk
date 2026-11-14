//! 文件扫描：扫描目录下的 *.byk.json 文件并解析为 AliasFile。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::core::config::{
    AliasFile, Diagnostic, FILE_DIR_VAR, Level, Mode, VarTable, build_file_key, describe_issue,
    filter_invalid_keys, is_valid_private, parse_priority, parse_private, preprocess_vars,
    validate_filename,
};
use crate::utils::path::{canonical, dir_of, expand_home};

/// 一次目录扫描的结果：成功解析的文件、诊断、以及实际检查过的候选文件。
#[derive(Debug, Default)]
pub(crate) struct ScanResult {
    pub files: Vec<AliasFile>,
    pub diagnostics: Vec<Diagnostic>,
    pub seen: Vec<PathBuf>,
}

enum CandidateKind {
    File,
    Symlink,
}

/// 目录项是否是别名文件候选：普通文件，或指向文件的符号链接。
///
/// 断链也是候选：名字合法、地址成立，只是内容不可用，交给诊断报告。
fn candidate_kind(path: &Path) -> Option<CandidateKind> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() {
        Some(CandidateKind::Symlink)
    } else if metadata.is_file() {
        Some(CandidateKind::File)
    } else {
        None
    }
}

fn symlink_problem(path: &Path) -> Option<String> {
    match fs::metadata(path) {
        Ok(target) if target.is_file() => None,
        Ok(_) => Some("symlink target is not a regular file".to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let target = fs::read_link(path)
                .map(|target| target.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            Some(format!("broken symlink: target does not exist: {}", target))
        }
        Err(e) => Some(format!("cannot read symlink target: {}", e)),
    }
}

pub(crate) fn scan_alias_files(dir: &Path, is_global: bool, vars: &VarTable) -> ScanResult {
    // 本地文件默认参与合并，且优先级高于全局（1 > 0）——进目录就能 `byk <key>`。
    // 只想给本文件用（仅 @file.key 可达）时写 `$private: true`。
    let default_priority = default_priority(is_global);
    let mut result = ScanResult::default();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return result,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(stem) = file_name.strip_suffix(".byk.json") else {
            continue;
        };
        let Some(kind) = candidate_kind(&path) else {
            continue;
        };

        result.seen.push(path.clone());

        if !validate_filename(stem) {
            result.diagnostics.push(Diagnostic::error(
                &path,
                format!(
                    "invalid file name {:?}; use letters, digits, \"-\", \"_\", or CJK characters",
                    file_name
                ),
            ));
            continue;
        }

        if let CandidateKind::Symlink = kind
            && let Some(reason) = symlink_problem(&path)
        {
            result.diagnostics.push(Diagnostic::error(&path, reason));
            continue;
        }

        if let Some(alias_file) = parse_alias_file(
            &path,
            is_global,
            default_priority,
            vars,
            &mut result.diagnostics,
        ) {
            result.files.push(alias_file);
        }
    }

    result
}

/// 直接加载单个文件（按文件名 stem），不扫描目录。
///
/// 精确执行只需要目标文件，跳过其余文件的读取、解析与合并。解析仍走
/// [`parse_alias_file`]，链接文件、`$var` / `${file_dir}`、继承的
/// `$cwd` / `$mode` / `$paths` 都与扫描路径一致。
pub(crate) fn load_alias_file(
    dir: &Path,
    is_global: bool,
    stem: &str,
    vars: &VarTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<AliasFile> {
    let path = dir.join(format!("{}.byk.json", stem));
    parse_alias_file(
        &path,
        is_global,
        default_priority(is_global),
        vars,
        diagnostics,
    )
}

fn default_priority(is_global: bool) -> i32 {
    if is_global { 0 } else { 1 }
}

/// 只枚举目录里的别名文件，不读取内容。
///
/// 与 [`scan_alias_files`] 的区别：这里只做文件名层面的检查，JSON 损坏、
/// 链接目标缺失之类的文件照样返回。`--paths` 需要展示“文件在哪”，
/// 内容是否可用交给 `--check`。返回 `(file key, path)`，按 key 排序。
pub(crate) fn list_alias_files(dir: &Path, is_global: bool) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let stem = name.strip_suffix(".byk.json")?;
            if !validate_filename(stem) {
                return None;
            }
            match candidate_kind(&path)? {
                CandidateKind::File => {}
                CandidateKind::Symlink => {
                    // 断链仍是一个可寻址的目录项；指向目录的链接才排除。
                    if fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
                        return None;
                    }
                }
            }
            Some((build_file_key(stem, is_global), path))
        })
        .collect();

    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// 读取一个别名文件：注入它自己的 `file_dir`，完成变量替换与告警。
///
/// 返回 `(文档, 文件路径)`。链接文件和链接目标都走这里，所以两者
/// 的 `${file_dir}` 都指向各自所在目录。
fn load_document(
    path: &Path,
    vars: &VarTable,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(serde_json::Value, PathBuf), String> {
    let content = fs::read_to_string(path).map_err(|e| format!("cannot read file: {}", e))?;
    let mut data: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON: {}", e))?;

    // 内置表是全局共用的，file_dir 逐文件不同：克隆一份局部表再注入
    let mut file_vars = vars.clone();
    file_vars.insert(FILE_DIR_VAR.to_string(), dir_of(path));

    // 变量替换：内置变量与文件 $var 合成一张表，扫描期一次替换完成
    for issue in preprocess_vars(&mut data, &file_vars) {
        push_doc_diagnostic(
            diagnostics,
            Level::Warning,
            owner,
            path,
            describe_issue(&issue),
        );
    }

    warn_invalid_modes(&data, path, owner, diagnostics);

    Ok((data, path.to_path_buf()))
}

/// 记录一条归属于扫描文件的诊断；链接目标的内部问题补上目标路径。
fn push_doc_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    level: Level,
    owner: &Path,
    doc: &Path,
    message: String,
) {
    let message = if doc != owner {
        format!("{} (in {})", message, doc.display())
    } else {
        message
    };
    diagnostics.push(Diagnostic {
        level,
        path: owner.to_path_buf(),
        message,
    });
}

/// 根是数组时跟随链接指针，返回 `(目标文档, 目标路径)`。
///
/// 语义是「放弃自己的身份」：`@file` 前缀、key、默认优先级都留在链接文件上，
/// 内容与相对路径基准则完全来自目标。所以返回的路径会替换掉 `AliasFile.path`，
/// 让 `$cwd` / `$paths` / `$var` 的相对基准都落到目标目录。
///
/// 链深只允许一层：目标是数组（又一个链接文件）直接报错——链接本可以一步指到位，
/// 多一层只是隐藏真实文件的位置。
fn follow_link(
    data: serde_json::Value,
    link_path: &Path,
    vars: &VarTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(serde_json::Value, PathBuf)> {
    let serde_json::Value::Array(items) = &data else {
        return Some((data, link_path.to_path_buf()));
    };

    let Some(first) = items.first() else {
        diagnostics.push(Diagnostic::error(
            link_path,
            "link file lists no path; file skipped",
        ));
        return None;
    };

    if items.len() > 1 {
        diagnostics.push(Diagnostic::warning(
            link_path,
            format!("only the first of {} link paths is used", items.len()),
        ));
    }

    let Some(spec) = first.as_str() else {
        diagnostics.push(Diagnostic::error(link_path, "link path must be a string"));
        return None;
    };

    let target = resolve_link_path(spec, link_path);
    let (target_data, target_path) = match load_document(&target, vars, link_path, diagnostics) {
        Ok(loaded) => loaded,
        Err(reason) => {
            diagnostics.push(Diagnostic::error(
                link_path,
                format!("cannot load link target {}: {}", target.display(), reason),
            ));
            return None;
        }
    };

    if target_data.is_array() {
        diagnostics.push(Diagnostic::error(
            link_path,
            format!(
                "link chains are limited to one level; target is another link file: {}",
                target.display()
            ),
        ));
        return None;
    }

    if !target_data.is_object() {
        diagnostics.push(Diagnostic::error(
            link_path,
            format!("link target must be a JSON object: {}", target.display()),
        ));
        return None;
    }

    Some((target_data, target_path))
}

/// 链接路径：`~` 展开，相对路径按**链接文件所在目录**解析（与调用目录无关）。
fn resolve_link_path(spec: &str, link_path: &Path) -> PathBuf {
    let expanded = expand_home(spec);
    let joined = if expanded.is_relative() {
        link_path
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
/// 根是对象时按别名文件处理；根是数组时视为**单链接指针**，转去解析它指向的
/// 文件（见 `follow_link`）。
fn parse_alias_file(
    path: &Path,
    is_global: bool,
    default_priority: i32,
    vars: &VarTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<AliasFile> {
    let name = path.file_name()?.to_str()?;

    let suffix = ".byk.json";
    let stem = name.strip_suffix(suffix)?;

    if !validate_filename(stem) {
        return None;
    }

    // 读链接文件本身，再顺着指针（如果有）拿到真正的别名文档
    let (mut data, source_path) = {
        let (link_data, link_path) = match load_document(path, vars, path, diagnostics) {
            Ok(loaded) => loaded,
            Err(reason) => {
                diagnostics.push(Diagnostic::error(path, reason));
                return None;
            }
        };
        follow_link(link_data, &link_path, vars, diagnostics)?
    };

    let owner = path;
    let doc = source_path.as_path();

    let Some(obj) = data.as_object_mut() else {
        diagnostics.push(Diagnostic::error(
            owner,
            "root must be a JSON object (or a link array)",
        ));
        return None;
    };

    let priority = match obj.remove("$priority") {
        None => default_priority,
        Some(raw) if raw.is_number() => parse_priority(&raw, default_priority),
        Some(_) => {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                "$priority must be an integer; using the default".to_string(),
            );
            default_priority
        }
    };

    // $var 为文件级变量声明（已在变量替换阶段生效），移除避免泄漏为别名
    obj.remove("$var");

    let inherited_cwd = match obj.remove("$cwd") {
        None => None,
        Some(serde_json::Value::String(cwd)) => Some(cwd),
        Some(_) => {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                "$cwd must be a string; ignored".to_string(),
            );
            None
        }
    };

    let inherited_mode = obj.remove("$mode").map(|v| Mode::from_json(&v));

    let inherited_private = match obj.remove("$private") {
        None => false,
        Some(raw) if is_valid_private(&raw) => parse_private(&raw),
        Some(raw) => {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!("$private must be a boolean; {} is treated as false", raw),
            );
            false
        }
    };

    let inherited_paths = match obj.remove("$paths") {
        None => Vec::new(),
        Some(serde_json::Value::Array(items)) => {
            let total = items.len();
            let paths: Vec<String> = items
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect();
            let dropped = total - paths.len();
            if dropped > 0 {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Warning,
                    owner,
                    doc,
                    format!(
                        "$paths contains {} non-string {}; ignored",
                        dropped,
                        if dropped == 1 { "entry" } else { "entries" }
                    ),
                );
            }
            paths
        }
        Some(_) => {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                "$paths must be an array of strings; ignored".to_string(),
            );
            Vec::new()
        }
    };

    validate_aliases(obj, "", doc, owner, diagnostics);

    let filtered = filter_invalid_keys(obj);

    Some(AliasFile {
        key: build_file_key(stem, is_global),
        priority,
        aliases: filtered,
        path: source_path,
        inherited_cwd,
        inherited_mode,
        inherited_private,
        inherited_paths,
    })
}

/// 校验别名树里的字段类型；这些错误会被后续解析静默丢弃，所以必须在这里报告。
///
/// 在 `filter_invalid_keys` 之前调用，否则错误类型已经被过滤掉，无法诊断。
fn validate_aliases(
    object: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    doc: &Path,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (key, value) in object {
        if key.starts_with('$') {
            continue;
        }
        let serde_json::Value::Object(inner) = value else {
            continue;
        };
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        let is_alias = inner.contains_key("$cmd");

        if is_alias {
            if let Some(cmd) = inner.get("$cmd")
                && !cmd.is_string()
            {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Error,
                    owner,
                    doc,
                    format!("alias \"{}\": $cmd must be a string; alias ignored", path),
                );
            }
            if let Some(cwd) = inner.get("$cwd")
                && !cwd.is_string()
            {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Error,
                    owner,
                    doc,
                    format!("alias \"{}\": $cwd must be a string; alias ignored", path),
                );
            }
            if let Some(description) = inner.get("$description")
                && !description.is_string()
            {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Error,
                    owner,
                    doc,
                    format!(
                        "alias \"{}\": $description must be a string; alias ignored",
                        path
                    ),
                );
            }
        } else if let Some(cwd) = inner.get("$cwd")
            && !cwd.is_string()
        {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!("group \"{}\": $cwd must be a string; ignored", path),
            );
        }

        if let Some(private) = inner.get("$private")
            && !is_valid_private(private)
        {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!(
                    "\"{}\": $private must be a boolean; {} is treated as false",
                    path, private
                ),
            );
        }

        validate_aliases(inner, &path, doc, owner, diagnostics);
    }
}

/// `$mode` 只认三个值；写错时告警并回落 `normal`。
///
/// 必须在这里喊一声：把它拼成 `"interactiv"` 会让一条本该确认的危险命令
/// 直接执行，是「配置写了没生效」里后果最重的一种。
///
/// 只走 object：数组在 `filter_invalid_keys` 里本来就会被整段丢掉，
/// 里面的 `$mode` 属于死配置，不值得为它再递归一层。
fn warn_invalid_modes(
    value: &serde_json::Value,
    doc: &Path,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };

    if let Some(raw) = obj.get("$mode")
        && !matches!(
            raw.as_str(),
            Some("normal") | Some("interactive") | Some("silent")
        )
    {
        push_doc_diagnostic(
            diagnostics,
            Level::Warning,
            owner,
            doc,
            format!(
                "$mode must be one of normal|interactive|silent, got {} (treated as normal)",
                raw
            ),
        );
    }

    for child in obj.values() {
        warn_invalid_modes(child, doc, owner, diagnostics);
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
            inherited_private: false,
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
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].inherited_paths, vec!["./scripts", "~/tools/bin"]);
    }

    #[test]
    fn paths_empty_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"build": "make build"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
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
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].inherited_paths, vec!["./good", "./also-good"]);
    }

    #[test]
    fn paths_empty_when_not_array() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"$paths": "not-an-array", "build": "echo"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
        assert_eq!(files.len(), 1);
        assert!(files[0].inherited_paths.is_empty());
    }

    #[test]
    fn private_extracted_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"$private": true, "build": "make"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
        assert_eq!(files.len(), 1);
        assert!(files[0].inherited_private);
        assert!(!files[0].aliases.contains_key("$private"));
    }

    #[test]
    fn private_defaults_to_false() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"build": "make"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
        assert!(!files[0].inherited_private);
    }

    #[test]
    fn paths_not_leaked_into_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        std::fs::write(&file, r#"{"$paths": ["./bin"], "build": "make"}"#).unwrap();
        let files = scan_alias_files(dir.path(), false, &VarTable::new()).files;
        assert_eq!(files.len(), 1);
        assert!(!files[0].aliases.contains_key("$paths"));
    }

    #[test]
    fn list_alias_files_lists_valid_names_without_reading_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.byk.json"), "not json").unwrap();
        std::fs::write(dir.path().join("a.byk.json"), "{}").unwrap();
        std::fs::write(dir.path().join(".byk.json"), "{}").unwrap();
        std::fs::write(dir.path().join("bad.name.byk.json"), "{}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("nested.byk.json")).unwrap();

        let files = list_alias_files(dir.path(), false);
        assert_eq!(
            files
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["@", "@a", "@b"]
        );
    }

    #[test]
    fn list_alias_files_uses_global_prefix_and_missing_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ops.byk.json"), "{}").unwrap();

        let files = list_alias_files(dir.path(), true);
        assert_eq!(files[0].0, "@@ops");
        assert!(list_alias_files(Path::new("/nonexistent/byk-test-dir"), true).is_empty());
    }

    #[test]
    fn scan_reports_invalid_file_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.name.byk.json"), "{}").unwrap();

        let result = scan_alias_files(dir.path(), false, &VarTable::new());

        assert_eq!(result.seen.len(), 1);
        assert!(result.files.is_empty());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.level == Level::Error && diagnostic.message.contains("invalid file name")
        }));
    }

    #[cfg(unix)]
    #[test]
    fn scan_reports_and_lists_broken_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        symlink(
            dir.path().join("missing.byk.json"),
            dir.path().join("ops.byk.json"),
        )
        .unwrap();

        let result = scan_alias_files(dir.path(), false, &VarTable::new());
        assert_eq!(result.seen.len(), 1);
        assert!(result.files.is_empty());
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.level == Level::Error && diagnostic.message.contains("broken symlink")
        }));

        let files = list_alias_files(dir.path(), false);
        assert_eq!(files[0].0, "@ops");
    }
}
