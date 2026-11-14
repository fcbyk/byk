//! 精确执行语法：@file.key 和 @@file.key。

use std::io;
use std::path::PathBuf;

use super::scan::load_alias_file;
use crate::core::config::{
    AliasFile, Diagnostic, Level, Mode, ResolvedAlias, apply_inherited, build_builtin_table,
    is_alias_value, to_alias_value, validate_filename,
};
use crate::core::paths::PathLayout;

/// 解析 `@file.key` / `@@file.key`，返回 (file_key, alias_key)。
///
/// `@.build` 的 file_key 是 `@`（匿名文件）；无点号或 key 为空时返回 None，走普通查找。
pub fn parse_exact_syntax(input: &str) -> Option<(String, String)> {
    let (prefix, rest) = if let Some(rest) = input.strip_prefix("@@") {
        ("@@", rest)
    } else {
        ("@", input.strip_prefix('@')?)
    };

    let dot_idx = rest.find('.')?;
    let file_name = &rest[..dot_idx];
    let alias_key = &rest[dot_idx + 1..];

    if alias_key.is_empty() {
        return None;
    }

    Some((format!("{}{}", prefix, file_name), alias_key.to_string()))
}

/// 精确执行结果。
pub enum ExactLookup {
    Found {
        resolved: ResolvedAlias,
        display_source: String,
    },
    FileNotFound,
    /// 文件存在，但读取、解析或链接目标加载失败。
    FileUnavailable {
        reason: String,
    },
    KeyNotFound,
    /// `$` 开头的路径段是保留字段或给其他程序的自定义字段，永远不是别名。
    ReservedField {
        key: String,
    },
}

/// 目标文件加载失败的原因。
pub enum LoadFailure {
    NotFound,
    Failed(String),
}

/// 按 `@file` / `@@file` 加载并解析目标文件，不扫描目录、不建合并树。
///
/// `@` 指向当前目录，`@@` 指向全局别名目录；文件 key 与扫描阶段的命名规则一致。
/// 文件名不合法时视作文件不存在，避免 `@../x` 这类路径穿越。
pub fn load_file_by_key(layout: &PathLayout, file_key: &str) -> Result<AliasFile, LoadFailure> {
    let Some((is_global, stem)) = split_file_key(file_key) else {
        return Err(LoadFailure::NotFound);
    };
    if !validate_filename(stem) {
        return Err(LoadFailure::NotFound);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dir = if is_global {
        layout.alias_dir.clone()
    } else {
        cwd.clone()
    };
    let path = dir.join(format!("{}.byk.json", stem));

    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(LoadFailure::NotFound),
        Err(e) => {
            return Err(LoadFailure::Failed(format!(
                "cannot inspect {}: {}",
                path.display(),
                e
            )));
        }
    };

    if metadata.file_type().is_symlink() {
        match std::fs::metadata(&path) {
            Ok(target) if target.is_file() => {}
            Ok(_) => {
                return Err(LoadFailure::Failed(format!(
                    "symlink target is not a regular file: {}",
                    path.display()
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let target = std::fs::read_link(&path)
                    .map(|target| target.display().to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                return Err(LoadFailure::Failed(format!(
                    "broken symlink: target does not exist: {}",
                    target
                )));
            }
            Err(e) => {
                return Err(LoadFailure::Failed(format!(
                    "cannot read symlink target: {}",
                    e
                )));
            }
        }
    } else if !metadata.is_file() {
        return Err(LoadFailure::Failed(format!(
            "not a regular file: {}",
            path.display()
        )));
    }

    let vars = build_builtin_table(&layout.root_dir, &cwd);
    let mut diagnostics = Vec::new();
    match load_alias_file(&dir, is_global, stem, &vars, &mut diagnostics) {
        Some(file) => Ok(file),
        None => Err(LoadFailure::Failed(failure_reason(&diagnostics))),
    }
}

/// 解析目标文件来响应 `@file.key` / `@@file.key`。
pub fn resolve_exact_alias(layout: &PathLayout, file_key: &str, alias_key: &str) -> ExactLookup {
    let file = match load_file_by_key(layout, file_key) {
        Ok(file) => file,
        Err(LoadFailure::NotFound) => return ExactLookup::FileNotFound,
        Err(LoadFailure::Failed(reason)) => return ExactLookup::FileUnavailable { reason },
    };

    if alias_key.split('.').any(|part| part.starts_with('$')) {
        return ExactLookup::ReservedField {
            key: alias_key.to_string(),
        };
    }

    match lookup_exact_alias(std::slice::from_ref(&file), file_key, alias_key) {
        Some((resolved, display_source)) => ExactLookup::Found {
            resolved,
            display_source,
        },
        None => ExactLookup::KeyNotFound,
    }
}

fn failure_reason(diagnostics: &[Diagnostic]) -> String {
    let errors: Vec<&str> = diagnostics
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.as_str())
        .collect();
    if !errors.is_empty() {
        return errors.join("; ");
    }
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    if messages.is_empty() {
        "cannot load alias file".to_string()
    } else {
        messages.join("; ")
    }
}

fn split_file_key(file_key: &str) -> Option<(bool, &str)> {
    if let Some(stem) = file_key.strip_prefix("@@") {
        Some((true, stem))
    } else {
        file_key.strip_prefix('@').map(|stem| (false, stem))
    }
}

/// 按 file_key 在文件列表里查找别名，返回 (别名, 展示用来源串)。
pub fn lookup_exact_alias(
    files: &[AliasFile],
    file_key: &str,
    alias_key: &str,
) -> Option<(ResolvedAlias, String)> {
    for f in files {
        if f.key != file_key {
            continue;
        }
        let parts: Vec<&str> = alias_key.split('.').collect();
        let mut current: &serde_json::Value = &serde_json::Value::Object(f.aliases.clone());
        // 沿路径遍历时累积分组级 $cwd / $mode，子级覆盖父级
        let mut group_cwd: Option<&str> = None;
        let mut group_mode: Option<Mode> = None;
        for (i, part) in parts.iter().enumerate() {
            let obj = current.as_object()?;
            // 累积分组级继承属性（文件根级的 $cwd/$mode 已被 parse_alias_file 移除，此处不影响）
            if let Some(c) = obj.get("$cwd").and_then(|v| v.as_str()) {
                group_cwd = Some(c);
            }
            if let Some(v) = obj.get("$mode") {
                group_mode = Some(Mode::from_json(v));
            }
            current = obj.get(*part)?;
            if i < parts.len() - 1 && !current.is_object() {
                return None;
            }
        }
        if is_alias_value(current) {
            let value = to_alias_value(current)?;
            let effective_cwd = group_cwd.or(f.inherited_cwd.as_deref());
            let effective_mode = group_mode.or(f.inherited_mode);
            let value = apply_inherited(value, effective_cwd, effective_mode);
            let display_source = format!("{}.{}", file_key, alias_key);
            let source_path = Some(f.path.clone());
            return Some((
                ResolvedAlias {
                    value,
                    source: file_key.to_string(),
                    source_path,
                    paths: f.inherited_paths.clone(),
                },
                display_source,
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::AliasValue;
    use std::fs;

    fn temp_layout() -> (tempfile::TempDir, PathLayout) {
        let tmp = tempfile::tempdir().unwrap();
        let alias_dir = tmp.path().join("alias");
        fs::create_dir_all(&alias_dir).unwrap();
        let layout = PathLayout {
            root_dir: tmp.path().to_path_buf(),
            alias_dir,
        };
        (tmp, layout)
    }

    #[test]
    fn resolve_global_exact_alias() {
        let (_tmp, layout) = temp_layout();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build": "make build"}"#,
        )
        .unwrap();

        let ExactLookup::Found {
            resolved,
            display_source,
        } = resolve_exact_alias(&layout, "@@ops", "build")
        else {
            panic!("expected the alias to resolve");
        };
        assert_eq!(resolved.source, "@@ops");
        assert_eq!(
            resolved.source_path.as_deref(),
            Some(layout.alias_dir.join("ops.byk.json").as_path())
        );
        assert_eq!(display_source, "@@ops.build");
        let AliasValue::Str(command) = resolved.value else {
            panic!("expected a string command");
        };
        assert_eq!(command, "make build");
    }

    #[test]
    fn resolve_exact_bypasses_private() {
        let (_tmp, layout) = temp_layout();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"$private": true, "deploy": "echo private"}"#,
        )
        .unwrap();

        let ExactLookup::Found { resolved, .. } = resolve_exact_alias(&layout, "@@ops", "deploy")
        else {
            panic!("exact calls must reach private files");
        };
        let AliasValue::Str(command) = resolved.value else {
            panic!("expected a string command");
        };
        assert_eq!(command, "echo private");
    }

    #[test]
    fn resolve_exact_missing_file() {
        let (_tmp, layout) = temp_layout();
        assert!(matches!(
            resolve_exact_alias(&layout, "@@nope", "build"),
            ExactLookup::FileNotFound
        ));
    }

    #[test]
    fn resolve_exact_missing_key() {
        let (_tmp, layout) = temp_layout();
        fs::write(layout.alias_dir.join("ops.byk.json"), r#"{"other": "x"}"#).unwrap();
        assert!(matches!(
            resolve_exact_alias(&layout, "@@ops", "build"),
            ExactLookup::KeyNotFound
        ));
    }

    #[test]
    fn resolve_exact_reports_unloadable_file() {
        let (_tmp, layout) = temp_layout();
        fs::write(layout.alias_dir.join("broken.byk.json"), "{not json").unwrap();
        match resolve_exact_alias(&layout, "@@broken", "build") {
            ExactLookup::FileUnavailable { reason } => {
                assert!(
                    reason.contains("invalid JSON"),
                    "unexpected reason: {reason}"
                );
            }
            _ => panic!("expected FileUnavailable"),
        }
    }

    #[test]
    fn resolve_exact_rejects_reserved_fields() {
        let (_tmp, layout) = temp_layout();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":{"$cmd":"echo hi","$description":"d"}}"#,
        )
        .unwrap();

        for key in ["build.$cmd", "build.$description", "$custom"] {
            assert!(
                matches!(
                    resolve_exact_alias(&layout, "@@ops", key),
                    ExactLookup::ReservedField { .. }
                ),
                "expected {key} to be rejected"
            );
        }
    }

    #[test]
    fn resolve_exact_rejects_path_traversal() {
        let (_tmp, layout) = temp_layout();
        assert!(matches!(
            resolve_exact_alias(&layout, "@@../evil", "build"),
            ExactLookup::FileNotFound
        ));
    }

    #[test]
    fn parse_at_file_dot_key() {
        assert_eq!(
            parse_exact_syntax("@release.build"),
            Some(("@release".into(), "build".into()))
        );
    }

    #[test]
    fn parse_double_at_file_dot_key() {
        assert_eq!(
            parse_exact_syntax("@@release.build"),
            Some(("@@release".into(), "build".into()))
        );
    }

    #[test]
    fn parse_at_dot_key() {
        assert_eq!(
            parse_exact_syntax("@.build"),
            Some(("@".into(), "build".into()))
        );
    }

    #[test]
    fn parse_double_at_dot_key() {
        assert_eq!(
            parse_exact_syntax("@@.build"),
            Some(("@@".into(), "build".into()))
        );
    }

    #[test]
    fn parse_at_only_no_dot() {
        assert_eq!(parse_exact_syntax("@release"), None);
    }

    #[test]
    fn parse_no_at_prefix() {
        assert_eq!(parse_exact_syntax("release.build"), None);
        assert_eq!(parse_exact_syntax("normal"), None);
    }

    #[test]
    fn parse_at_dot_empty_key() {
        assert_eq!(parse_exact_syntax("@release."), None);
        assert_eq!(parse_exact_syntax("@."), None);
    }

    #[test]
    fn parse_dot_in_file_name() {
        // file_name 部分不含 dot（第一个 dot 即分隔符），多 dot 后面的归 alias_key
        assert_eq!(
            parse_exact_syntax("@release.build.prod"),
            Some(("@release".into(), "build.prod".into()))
        );
    }

    #[test]
    fn parse_with_hyphens() {
        assert_eq!(
            parse_exact_syntax("@my-file.run-test"),
            Some(("@my-file".into(), "run-test".into()))
        );
    }
}
