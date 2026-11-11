//! `..init`：创建新的空别名文件。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{FileScope, target_path};
use crate::core::paths::PathLayout;

/// 新建内容：空 JSON 对象加一个结尾换行。
const EMPTY_ALIAS_FILE: &[u8] = b"{}\n";

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<PathBuf, String> {
    if !args.is_empty() {
        return Err("init takes no arguments".to_string());
    }

    let base = match scope {
        FileScope::Local => std::env::current_dir()
            .map_err(|e| format!("Failed to resolve current directory: {}", e))?,
        FileScope::Global => {
            fs::create_dir_all(&layout.alias_dir).map_err(|e| {
                format!(
                    "Failed to create directory {}: {}",
                    layout.alias_dir.display(),
                    e
                )
            })?;
            layout.alias_dir.clone()
        }
    };

    let target = target_path(&base, stem);
    create(&target)?;
    Ok(target)
}

fn create(path: &Path) -> Result<(), String> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!("File already exists: {}", path.display()));
        }
        Err(e) => {
            return Err(format!("Failed to create {}: {}", path.display(), e));
        }
    };

    if let Err(e) = file.write_all(EMPTY_ALIAS_FILE) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("Failed to write {}: {}", path.display(), e));
    }

    Ok(())
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
    fn run_creates_dirs_and_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let path = run(FileScope::Global, "ops", &[], &layout).unwrap();

        assert_eq!(path, layout.alias_dir.join("ops.byk.json"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn run_anonymous_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let path = run(FileScope::Global, "", &[], &layout).unwrap();

        assert_eq!(path, layout.alias_dir.join(".byk.json"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn run_rejects_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(FileScope::Global, "ops", &["extra".to_string()], &layout).unwrap_err();

        assert_eq!(err, "init takes no arguments");
        assert!(!layout.alias_dir.join("ops.byk.json").exists());
    }

    #[test]
    fn create_never_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");
        fs::write(&path, "{\"keep\": true}").unwrap();

        let err = create(&path).unwrap_err();

        assert_eq!(err, format!("File already exists: {}", path.display()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"keep\": true}");
    }

    #[test]
    fn create_writes_empty_object() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");

        create(&path).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }
}
