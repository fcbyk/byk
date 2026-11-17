//! 依赖目录的卸载、改名、删除与 stale 提示。

use std::fs;
use std::path::Path;

/// 删除整个依赖目录；局部目录删完后收掉空壳。
pub(crate) fn remove_all(deps_dir: &Path, is_global: bool) -> Result<(), String> {
    fs::remove_dir_all(deps_dir)
        .map_err(|e| format!("Failed to remove {}: {}", deps_dir.display(), e))?;
    if !is_global {
        prune_local_root(deps_dir);
    }
    Ok(())
}

/// 局部依赖删完后收掉空壳：`.byk/deps` 里只剩我们的 `.gitignore` 时，
/// 连它和空的 `.byk` 一起删，别把空目录留给项目。
fn prune_local_root(deps_dir: &Path) {
    let Some(root) = deps_dir.parent() else {
        return;
    };
    let only_gitignore = fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .all(|entry| entry.file_name() == std::ffi::OsStr::new(".gitignore"))
        })
        .unwrap_or(false);
    if !only_gitignore {
        return;
    }

    let _ = fs::remove_file(root.join(".gitignore"));
    if fs::remove_dir(root).is_err() {
        return;
    }
    if let Some(byk) = root.parent() {
        let _ = fs::remove_dir(byk);
    }
}

/// `..rename` 成功后把依赖目录一起改名；目录不存在时静默跳过。
pub(crate) fn rename_dir(from: &Path, to: &Path) {
    if !from.is_dir() {
        return;
    }
    if let Err(e) = fs::rename(from, to) {
        eprintln!(
            "Warning: cannot move the dependency directory {}: {}",
            from.display(),
            e
        );
        return;
    }
    eprintln!("Moved {} to {}", from.display(), to.display());
}

/// `..destroy` 成功后删除依赖目录；目录不存在时静默跳过。
pub(crate) fn remove_dir(dir: &Path, is_global: bool) {
    if !dir.is_dir() {
        return;
    }
    if let Err(e) = fs::remove_dir_all(dir) {
        eprintln!(
            "Warning: cannot remove the dependency directory {}: {}",
            dir.display(),
            e
        );
        return;
    }
    if !is_global {
        prune_local_root(dir);
    }
    eprintln!("Removed {}", dir.display());
}

/// `..link` 改指向时提醒旧的依赖目录还在。
pub(crate) fn warn_stale_dir(dir: &Path, key: &str) {
    if dir.is_dir() {
        eprintln!(
            "Warning: this file still has a dependency directory at {};\nHint: run `{}..deps uninstall` first if the new target needs different assets",
            dir.display(),
            key
        );
    }
}
