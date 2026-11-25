//! 依赖目录的布局与名称。

use std::path::{Path, PathBuf};

/// 某个别名文件的依赖目录：全局是 `~/.byk/deps/global@<stem>`，局部是
/// `<文件所在目录>/.byk/deps/local@<stem>`；匿名文件用空前缀（`local@` / `global@`）。
///
/// 目录名就是文件 key：带作用域前缀，所以家目录里的本地文件
/// （`~/.byk/deps/local@ops`）不会和全局文件（`~/.byk/deps/global@ops`）重合，
/// 最终一段也不会是裸 stem（避开 Windows 的保留设备名）。
///
/// 局部依赖跟着项目走，删项目即删依赖——目录本身就是身份，不需要哈希目录或映射表。
pub(crate) fn deps_dir(key: &str, is_global: bool, file_dir: &Path, byk_root: &Path) -> PathBuf {
    if is_global {
        byk_root.join("deps").join(key)
    } else {
        file_dir.join(".byk").join("deps").join(key)
    }
}

/// 计算某文件应注入的 `deps_dir` 变量值。
pub(crate) fn deps_dir_var(key: &str, is_global: bool, file_dir: &Path, byk_root: &Path) -> String {
    deps_dir(key, is_global, file_dir, byk_root)
        .to_string_lossy()
        .into_owned()
}

/// pip 虚拟环境在专属目录里的目录名（隐藏目录，避开文件同步的落点空间）。
pub(crate) const VENV_DIR: &str = ".venv";

/// npm 包装进专属目录下的 `node_modules`（无需虚拟环境：node_modules 本身就是目录级隔离）。
pub(crate) const NODE_MODULES: &str = "node_modules";

/// venv 里可执行文件所在子目录：Unix 是 `bin`，Windows 是 `Scripts`。
pub(crate) fn venv_bin_dir() -> &'static str {
    #[cfg(windows)]
    {
        "Scripts"
    }
    #[cfg(not(windows))]
    {
        "bin"
    }
}

/// venv 的 `bin`（/`Scripts`）目录，用于前置到 PATH。
pub(crate) fn venv_bin(deps_dir: &Path) -> PathBuf {
    deps_dir.join(VENV_DIR).join(venv_bin_dir())
}

/// 专属目录下 npm 可执行脚本所在目录（node_modules/.bin），全平台同名。
pub(crate) fn node_modules_bin(deps_dir: &Path) -> PathBuf {
    deps_dir.join(NODE_MODULES).join(".bin")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn deps_dir_mirrors_the_file_key_under_each_root() {
        let root = PathBuf::from("/home/u/.byk");
        let project = PathBuf::from("/work/app");

        assert_eq!(
            PathBuf::from(deps_dir_var("global@ops", true, &project, &root)),
            root.join("deps").join("global@ops")
        );
        assert_eq!(
            PathBuf::from(deps_dir_var("local@ops", false, &project, &root)),
            project.join(".byk").join("deps").join("local@ops")
        );
        // 匿名文件：key 就是前缀本身
        assert_eq!(
            deps_dir("local@", false, &project, &root),
            project.join(".byk").join("deps").join("local@")
        );
        assert_eq!(
            deps_dir("global@", true, &project, &root),
            root.join("deps").join("global@")
        );
    }
}
