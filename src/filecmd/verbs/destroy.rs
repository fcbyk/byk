//! `..destroy` 的入口参数解析。

use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;

pub(crate) use imp::Outcome;

pub(crate) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<Outcome, String> {
    imp::apply(scope, stem, parse_args(args)?, layout)
}

fn parse_args(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--yes" => Ok(true),
        _ => Err("destroy accepts only --yes".to_string()),
    }
}

mod imp {
    //! `..destroy`：删除别名文件。

    use std::fs;
    use std::io;
    use std::path::PathBuf;

    use crate::filecmd::io::path::{not_found_message, resolve_target};
    use crate::filecmd::io::summary;
    use crate::filecmd::io::summary::ContentSummary;
    use crate::filecmd::prompt;
    use crate::filecmd::prompt::Confirmation;
    use crate::filecmd::spec::FileScope;
    use crate::model::PathLayout;

    #[derive(Debug)]
    pub(crate) enum Outcome {
        Removed { path: PathBuf, symlink: bool },
        Cancelled,
        Mismatch,
    }

    pub(crate) fn apply(
        scope: FileScope,
        stem: &str,
        yes: bool,
        layout: &PathLayout,
    ) -> Result<Outcome, String> {
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
            prompt::require_tty(&target, "remove")?;

            let (title, note) = if symlink {
                (
                    "This will remove the symlink:",
                    "(the link target will not be touched)".to_string(),
                )
            } else {
                let note = match summary::summarize(&target) {
                    ContentSummary::Empty => summary::aliases_label(0),
                    ContentSummary::Aliases(count) => summary::aliases_label(count),
                    ContentSummary::Link => {
                        "(link file; aliases are stored in its target)".to_string()
                    }
                    ContentSummary::Unreadable => "(alias count unavailable)".to_string(),
                };
                ("This will permanently remove:", note)
            };
            match prompt::confirm(&target, stem, title, Some(&note))? {
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

    #[cfg(test)]
    mod tests {
        use super::*;

        fn test_layout(temp: &tempfile::TempDir) -> PathLayout {
            PathLayout {
                root_dir: temp.path().to_path_buf(),
                alias_dir: temp.path().join(".byk").join("alias"),
            }
        }

        #[test]
        fn run_with_yes_removes_file() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            fs::create_dir_all(&layout.alias_dir).unwrap();
            let path = layout.alias_dir.join("ops.byk.json");
            fs::write(&path, "{}").unwrap();

            let outcome = apply(FileScope::Global, "ops", true, &layout).unwrap();

            assert!(matches!(outcome, Outcome::Removed { symlink: false, .. }));
            assert!(!path.exists());
        }

        #[test]
        fn run_missing_file_is_an_error() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);

            let err = apply(FileScope::Global, "ops", true, &layout).unwrap_err();

            assert!(err.starts_with("File not found:"));
        }

        #[test]
        fn run_refuses_directory() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            let path = layout.alias_dir.join("ops.byk.json");
            fs::create_dir_all(&path).unwrap();

            let err = apply(FileScope::Global, "ops", true, &layout).unwrap_err();

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

            let outcome = apply(FileScope::Global, "ops", true, &layout).unwrap();

            assert!(matches!(outcome, Outcome::Removed { symlink: true, .. }));
            assert!(fs::symlink_metadata(&link).is_err());
            assert!(real.exists());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes() -> Vec<String> {
        vec!["--yes".to_string()]
    }

    #[test]
    fn default_is_interactive() {
        assert_eq!(parse_args(&[]), Ok(false));
    }

    #[test]
    fn accepts_yes() {
        assert_eq!(parse_args(&yes()), Ok(true));
    }

    #[test]
    fn rejects_other_arguments() {
        assert!(parse_args(&["--force".to_string()]).is_err());
        assert!(parse_args(&["--yes".to_string(), "extra".to_string()]).is_err());
    }
}
