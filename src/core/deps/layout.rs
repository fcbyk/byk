//! 依赖目录的布局与名称。

use std::path::{Path, PathBuf};

use crate::core::config::build_file_key;

/// 某个别名文件的依赖目录。
///
/// 两种作用域同一套形状，只有根不同：
///
/// - 全局：`~/.byk/deps/@@<stem>`
/// - 局部：`<文件所在目录>/.byk/deps/@<stem>`
///
/// 局部依赖跟着项目走：删项目即删依赖，不需要哈希目录，也不需要额外的映射表
/// ——目录本身就是身份。
pub(crate) fn deps_dir(stem: &str, is_global: bool, file_dir: &Path, byk_root: &Path) -> PathBuf {
    let key = build_file_key(stem, is_global);
    if is_global {
        byk_root.join("deps").join(key)
    } else {
        file_dir.join(".byk").join("deps").join(key)
    }
}

/// 计算某文件应注入的 `deps_dir` 变量值。
pub(crate) fn deps_dir_var(
    stem: &str,
    is_global: bool,
    file_dir: &Path,
    byk_root: &Path,
) -> String {
    deps_dir(stem, is_global, file_dir, byk_root)
        .to_string_lossy()
        .into_owned()
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
            deps_dir_var("ops", true, &project, &root),
            "/home/u/.byk/deps/@@ops"
        );
        assert_eq!(
            deps_dir_var("ops", false, &project, &root),
            "/work/app/.byk/deps/@ops"
        );
        // 匿名文件：key 就是前缀本身
        assert_eq!(
            deps_dir("", false, &project, &root),
            PathBuf::from("/work/app/.byk/deps/@")
        );
        assert_eq!(
            deps_dir("", true, &project, &root),
            PathBuf::from("/home/u/.byk/deps/@@")
        );
    }
}
