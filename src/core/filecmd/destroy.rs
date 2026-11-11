//! `..destroy`：删除别名文件。

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde_json::Value;

use super::{FileScope, not_found_message, resolve_target};
use crate::core::paths::PathLayout;

#[derive(Debug)]
pub(super) enum Outcome {
    Removed { path: PathBuf, symlink: bool },
    Cancelled,
    Mismatch,
}

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<Outcome, String> {
    let yes = parse_args(args)?;
    let target = resolve_target(scope, stem, layout)?;

    let metadata = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(not_found_message(scope, stem, &target, layout, "destroy"));
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", target.display(), e)),
    };

    if metadata.file_type().is_dir() {
        return Err(format!(
            "Refusing to remove directory: {}",
            target.display()
        ));
    }

    let symlink = metadata.file_type().is_symlink();

    if !yes {
        if !io::stderr().is_terminal() {
            return Err(format!(
                "Refusing to remove {} without confirmation; pass --yes",
                target.display()
            ));
        }

        let count = if symlink {
            None
        } else {
            Some(count_aliases(&target))
        };
        match confirm(&target, stem, count, symlink)? {
            Confirmation::Confirmed => {}
            Confirmation::Cancelled => return Ok(Outcome::Cancelled),
            Confirmation::Mismatch => return Ok(Outcome::Mismatch),
        }
    }

    fs::remove_file(&target)
        .map_err(|e| format!("Failed to remove {}: {}", target.display(), e))?;

    Ok(Outcome::Removed {
        path: target,
        symlink,
    })
}

fn parse_args(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--yes" => Ok(true),
        _ => Err("destroy accepts only --yes".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AliasCount {
    Known(usize),
    Link,
    Unknown,
}

impl AliasCount {
    fn label(self) -> String {
        match self {
            Self::Known(1) => "(1 alias)".to_string(),
            Self::Known(n) => format!("({} aliases)", n),
            Self::Link => "(link file; aliases are stored in its target)".to_string(),
            Self::Unknown => "(alias count unavailable)".to_string(),
        }
    }
}

/// 只统计文件自身定义的别名；链接文件不跟随目标。
fn count_aliases(path: &Path) -> AliasCount {
    let Ok(text) = fs::read_to_string(path) else {
        return AliasCount::Unknown;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return AliasCount::Unknown;
    };

    match &value {
        Value::Array(_) => AliasCount::Link,
        Value::Object(map) => AliasCount::Known(count_object(map)),
        _ => AliasCount::Known(0),
    }
}

fn count_object(map: &serde_json::Map<String, Value>) -> usize {
    match map.get("$cmd") {
        Some(Value::String(_)) => 1 + count_children(map),
        Some(_) => 0,
        None => count_children(map),
    }
}

fn count_children(map: &serde_json::Map<String, Value>) -> usize {
    map.iter()
        .filter(|(key, _)| !key.starts_with('$') && !key.contains('@') && !key.contains('.'))
        .map(|(_, value)| count_value(value))
        .sum()
}

fn count_value(value: &Value) -> usize {
    match value {
        Value::String(_) => 1,
        Value::Object(map) => count_object(map),
        _ => 0,
    }
}

enum Confirmation {
    Confirmed,
    Cancelled,
    Mismatch,
}

fn confirm(
    path: &Path,
    stem: &str,
    count: Option<AliasCount>,
    symlink: bool,
) -> Result<Confirmation, String> {
    eprintln!();
    if symlink {
        eprintln!("{}", "This will remove the symlink:".yellow());
        eprintln!("  {}", path.display().to_string().dimmed());
        eprintln!("  {}", "(the link target will not be touched)".dimmed());
    } else {
        eprintln!("{}", "This will permanently remove:".yellow());
        eprintln!("  {}", path.display().to_string().dimmed());
        if let Some(count) = count {
            eprintln!("  {}", count.label().dimmed());
        }
    }
    eprintln!();

    let expected = confirm_token(stem);
    eprint!("  {} {}: ", "Type".dimmed(), expected.yellow());
    io::stderr()
        .flush()
        .map_err(|e| format!("Failed to write confirmation prompt: {}", e))?;

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => {
            eprintln!();
            Ok(Confirmation::Cancelled)
        }
        Ok(_) if input.trim() == expected => Ok(Confirmation::Confirmed),
        Ok(_) => {
            eprintln!();
            Ok(Confirmation::Mismatch)
        }
        Err(e) => {
            eprintln!();
            Err(format!("Failed to read confirmation: {}", e))
        }
    }
}

/// 匿名文件没有 stem，改为输入完整文件名确认。
fn confirm_token(stem: &str) -> &str {
    if stem.is_empty() { ".byk.json" } else { stem }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join(".byk").join("alias"),
        }
    }

    fn yes() -> Vec<String> {
        vec!["--yes".to_string()]
    }

    #[test]
    fn parse_args_default_is_interactive() {
        assert_eq!(parse_args(&[]), Ok(false));
    }

    #[test]
    fn parse_args_accepts_yes() {
        assert_eq!(parse_args(&yes()), Ok(true));
    }

    #[test]
    fn parse_args_rejects_other_arguments() {
        assert!(parse_args(&["--force".to_string()]).is_err());
        assert!(parse_args(&["--yes".to_string(), "extra".to_string()]).is_err());
    }

    #[test]
    fn confirm_token_uses_stem_or_anonymous_name() {
        assert_eq!(confirm_token("ops"), "ops");
        assert_eq!(confirm_token(""), ".byk.json");
    }

    #[test]
    fn count_aliases_flat_and_nested() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");
        fs::write(
            &path,
            r#"{
                "build": "echo build",
                "group": {"one": "echo one", "two": {"$cmd": "echo two"}},
                "count": 42,
                "flag": true,
                "a.b": "ignored",
                "$priority": 1
            }"#,
        )
        .unwrap();

        assert_eq!(count_aliases(&path), AliasCount::Known(3));
    }

    #[test]
    fn count_aliases_meta_node_with_children() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");
        fs::write(
            &path,
            r#"{"dev": {"$cmd": "echo dev", "inner": "echo inner"}}"#,
        )
        .unwrap();

        assert_eq!(count_aliases(&path), AliasCount::Known(2));
    }

    #[test]
    fn count_aliases_link_file_is_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");
        fs::write(&path, r#"["shared.byk.json"]"#).unwrap();

        assert_eq!(count_aliases(&path), AliasCount::Link);
    }

    #[test]
    fn count_aliases_invalid_json_is_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");
        fs::write(&path, "{not json").unwrap();

        assert_eq!(count_aliases(&path), AliasCount::Unknown);
    }

    #[test]
    fn alias_count_labels() {
        assert_eq!(AliasCount::Known(0).label(), "(0 aliases)");
        assert_eq!(AliasCount::Known(1).label(), "(1 alias)");
        assert_eq!(AliasCount::Known(3).label(), "(3 aliases)");
        assert!(AliasCount::Link.label().contains("link file"));
        assert!(AliasCount::Unknown.label().contains("unavailable"));
    }

    #[test]
    fn run_with_yes_removes_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let path = layout.alias_dir.join("ops.byk.json");
        fs::write(&path, "{}").unwrap();

        let outcome = run(FileScope::Global, "ops", &yes(), &layout).unwrap();

        assert!(matches!(outcome, Outcome::Removed { symlink: false, .. }));
        assert!(!path.exists());
    }

    #[test]
    fn run_missing_file_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(FileScope::Global, "ops", &yes(), &layout).unwrap_err();

        assert!(err.starts_with("File not found:"));
    }

    #[test]
    fn run_refuses_directory() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = layout.alias_dir.join("ops.byk.json");
        fs::create_dir_all(&path).unwrap();

        let err = run(FileScope::Global, "ops", &yes(), &layout).unwrap_err();

        assert!(err.contains("Refusing to remove directory"));
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn run_removes_symlink_only() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let real = temp.path().join("real.byk.json");
        fs::write(&real, "{}").unwrap();
        let link = layout.alias_dir.join("ops.byk.json");
        symlink(&real, &link).unwrap();

        let outcome = run(FileScope::Global, "ops", &yes(), &layout).unwrap();

        assert!(matches!(outcome, Outcome::Removed { symlink: true, .. }));
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(real.exists());
    }
}
