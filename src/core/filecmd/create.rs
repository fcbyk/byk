//! 新建别名文件的共用流程：准备目标目录并拒绝覆盖。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{FileScope, target_path};
use crate::core::paths::PathLayout;

pub(super) fn destination(
    scope: FileScope,
    stem: &str,
    layout: &PathLayout,
) -> Result<PathBuf, String> {
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
    Ok(target_path(&base, stem))
}

pub(super) fn ensure_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(format!("File already exists: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to inspect {}: {}", path.display(), e)),
    }
}

pub(super) fn create(path: &Path, content: &[u8]) -> Result<(), String> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!("File already exists: {}", path.display()));
        }
        Err(e) => return Err(format!("Failed to create {}: {}", path.display(), e)),
    };

    if let Err(e) = file.write_all(content) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("Failed to write {}: {}", path.display(), e));
    }

    Ok(())
}
