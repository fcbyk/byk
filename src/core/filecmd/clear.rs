//! `..clear`：把当前文件重置为空对象。

use std::fs;
use std::io;
use std::path::PathBuf;

use super::init::EMPTY_ALIAS_FILE;
use super::prompt::{self, Confirmation};
use super::summary::{self, ContentSummary};
use super::{FileScope, not_found_message, resolve_target};
use crate::core::paths::PathLayout;

#[derive(Debug)]
pub(super) enum Outcome {
    Cleared(PathBuf),
    AlreadyEmpty(PathBuf),
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
            return Err(not_found_message(scope, stem, &target, layout, "clear"));
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", target.display(), e)),
    };

    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Refusing to clear symlink: {}\nHint: use ..destroy to remove the link",
            target.display()
        ));
    }
    if metadata.file_type().is_dir() {
        return Err(format!("Refusing to clear directory: {}", target.display()));
    }

    let content = summary::summarize(&target);
    if content == ContentSummary::Empty {
        return Ok(Outcome::AlreadyEmpty(target));
    }

    if !yes {
        prompt::require_tty(&target, "clear")?;

        let note = match content {
            ContentSummary::Empty => unreachable!("handled above"),
            ContentSummary::Aliases(count) => summary::aliases_label(count),
            ContentSummary::Link => "(link file; its target will not be touched)".to_string(),
            ContentSummary::Unreadable => "(content cannot be summarized)".to_string(),
        };
        match prompt::confirm(&target, stem, "This will clear:", Some(&note))? {
            Confirmation::Confirmed => {}
            Confirmation::Cancelled => return Ok(Outcome::Cancelled),
            Confirmation::Mismatch => return Ok(Outcome::Mismatch),
        }
    }

    fs::write(&target, EMPTY_ALIAS_FILE)
        .map_err(|e| format!("Failed to clear {}: {}", target.display(), e))?;

    Ok(Outcome::Cleared(target))
}

fn parse_args(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--yes" => Ok(true),
        _ => Err("clear accepts only --yes".to_string()),
    }
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

    fn prepare(layout: &PathLayout, content: &str) -> PathBuf {
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let path = layout.alias_dir.join("ops.byk.json");
        fs::write(&path, content).unwrap();
        path
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
    fn run_clears_object() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = prepare(&layout, r#"{"build": "echo build"}"#);

        let outcome = run(FileScope::Global, "ops", &yes(), &layout).unwrap();

        assert!(matches!(outcome, Outcome::Cleared(cleared) if cleared == path));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn run_clears_link_without_touching_target() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let target = temp.path().join("real.byk.json");
        fs::write(&target, "{\"hi\": \"echo hi\"}").unwrap();
        let path = prepare(&layout, &format!("[\"{}\"]", target.display()));

        let outcome = run(FileScope::Global, "ops", &yes(), &layout).unwrap();

        assert!(matches!(outcome, Outcome::Cleared(cleared) if cleared == path));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "{\"hi\": \"echo hi\"}"
        );
    }

    #[test]
    fn run_already_empty_is_a_noop() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = prepare(&layout, "{}\n");

        let outcome = run(FileScope::Global, "ops", &yes(), &layout).unwrap();

        assert!(matches!(outcome, Outcome::AlreadyEmpty(empty) if empty == path));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn run_clears_invalid_json_with_yes() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = prepare(&layout, "{not json");

        let outcome = run(FileScope::Global, "ops", &yes(), &layout).unwrap();

        assert!(matches!(outcome, Outcome::Cleared(cleared) if cleared == path));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
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

        assert!(err.starts_with("Refusing to clear directory:"));
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn run_refuses_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let real = temp.path().join("real.byk.json");
        fs::write(&real, "{\"hi\": \"echo hi\"}").unwrap();
        let link = layout.alias_dir.join("ops.byk.json");
        symlink(&real, &link).unwrap();

        let err = run(FileScope::Global, "ops", &yes(), &layout).unwrap_err();

        assert!(err.starts_with("Refusing to clear symlink:"));
        assert_eq!(fs::read_to_string(&real).unwrap(), "{\"hi\": \"echo hi\"}");
    }
}
