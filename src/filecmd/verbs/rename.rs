//! `rename` 的入口参数解析。

use std::path::PathBuf;

use crate::filecmd::spec::{FileScope, invalid_stem_message};
use crate::model::PathLayout;
use crate::resolve::validate_filename;

pub(crate) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(PathBuf, PathBuf), String> {
    imp::apply(scope, stem, parse_args(args)?, layout)
}

fn parse_args(args: &[String]) -> Result<&str, String> {
    let [new_stem] = args else {
        return Err("rename takes exactly one argument".to_string());
    };
    if !validate_filename(new_stem) {
        return Err(invalid_stem_message(new_stem));
    }
    Ok(new_stem)
}

mod imp {
    //! `rename`：重命名别名文件。

    use std::fs;
    use std::io;
    use std::path::PathBuf;

    use crate::filecmd::io::path::{not_found_message, resolve_target};
    use crate::filecmd::spec::FileScope;
    use crate::model::PathLayout;

    pub(crate) fn apply(
        scope: FileScope,
        stem: &str,
        new_stem: &str,
        layout: &PathLayout,
    ) -> Result<(PathBuf, PathBuf), String> {
        let from = resolve_target(scope, stem, layout)?;
        let metadata = match fs::symlink_metadata(&from) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(not_found_message(scope, stem, &from, layout, "rename"));
            }
            Err(e) => return Err(format!("Failed to inspect {}: {}", from.display(), e)),
        };
        if metadata.file_type().is_dir() {
            return Err(format!("Refusing to rename directory: {}", from.display()));
        }

        let to = resolve_target(scope, new_stem, layout)?;
        if from == to {
            return Err(format!(
                "Source and target are the same: {}",
                from.display()
            ));
        }
        if fs::symlink_metadata(&to).is_ok() {
            return Err(format!("Target already exists: {}", to.display()));
        }

        fs::rename(&from, &to).map_err(|e| {
            format!(
                "Failed to rename {} to {}: {}",
                from.display(),
                to.display(),
                e
            )
        })?;

        Ok((from, to))
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

        fn prepare(layout: &PathLayout, stem: &str) -> PathBuf {
            fs::create_dir_all(&layout.alias_dir).unwrap();
            let path = layout.alias_dir.join(format!("{}.byk.json", stem));
            fs::write(&path, "{}").unwrap();
            path
        }

        #[test]
        fn renames_file() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            let from = prepare(&layout, "ops");

            let (renamed_from, renamed_to) =
                apply(FileScope::Global, "ops", "new", &layout).unwrap();

            assert_eq!(renamed_from, from);
            assert_eq!(renamed_to, layout.alias_dir.join("new.byk.json"));
            assert!(fs::symlink_metadata(&from).is_err());
            assert!(renamed_to.exists());
        }

        #[test]
        fn renames_to_anonymous() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            prepare(&layout, "ops");

            let (_, to) = apply(FileScope::Global, "ops", "", &layout).unwrap();

            assert_eq!(to, layout.alias_dir.join(".byk.json"));
            assert!(to.exists());
        }

        #[test]
        fn rejects_missing_source() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);

            let err = apply(FileScope::Global, "ops", "new", &layout).unwrap_err();

            assert!(err.starts_with("File not found:"));
        }

        #[test]
        fn rejects_existing_target() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            let from = prepare(&layout, "ops");
            let to = prepare(&layout, "new");

            let err = apply(FileScope::Global, "ops", "new", &layout).unwrap_err();

            assert!(err.starts_with("Target already exists:"));
            assert!(from.exists());
            assert!(to.exists());
        }

        #[test]
        fn rejects_same_source_and_target() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            prepare(&layout, "ops");

            let err = apply(FileScope::Global, "ops", "ops", &layout).unwrap_err();

            assert!(err.starts_with("Source and target are the same:"));
        }

        #[test]
        fn rejects_directory_source() {
            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            let path = layout.alias_dir.join("ops.byk.json");
            fs::create_dir_all(&path).unwrap();

            let err = apply(FileScope::Global, "ops", "new", &layout).unwrap_err();

            assert!(err.starts_with("Refusing to rename directory:"));
            assert!(path.is_dir());
        }

        #[cfg(unix)]
        #[test]
        fn renames_symlink_itself() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            fs::create_dir_all(&layout.alias_dir).unwrap();
            let real = temp.path().join("real.byk.json");
            fs::write(&real, "{}").unwrap();
            let link = layout.alias_dir.join("ops.byk.json");
            symlink(&real, &link).unwrap();

            let (_, to) = apply(FileScope::Global, "ops", "new", &layout).unwrap();

            assert!(fs::symlink_metadata(&link).is_err());
            assert!(fs::symlink_metadata(&to).unwrap().file_type().is_symlink());
            assert_eq!(fs::read_link(&to).unwrap(), real);
            assert!(real.exists());
        }

        #[cfg(unix)]
        #[test]
        fn rejects_dangling_symlink_target() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().unwrap();
            let layout = test_layout(&temp);
            let from = prepare(&layout, "ops");
            let to = layout.alias_dir.join("new.byk.json");
            symlink(temp.path().join("gone.byk.json"), &to).unwrap();

            let err = apply(FileScope::Global, "ops", "new", &layout).unwrap_err();

            assert!(err.starts_with("Target already exists:"));
            assert!(from.exists());
            assert!(to.is_symlink());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn accepts_one_name() {
        assert_eq!(parse_args(&args(&["new"])).unwrap(), "new");
    }

    #[test]
    fn rejects_missing_and_extra_arguments() {
        assert_eq!(
            parse_args(&args(&[])).unwrap_err(),
            "rename takes exactly one argument"
        );
        assert_eq!(
            parse_args(&args(&["a", "b"])).unwrap_err(),
            "rename takes exactly one argument"
        );
    }

    #[test]
    fn rejects_invalid_filename() {
        assert!(parse_args(&args(&["a.b"])).is_err());
    }
}
