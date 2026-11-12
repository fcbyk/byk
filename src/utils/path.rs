//! 路径工具函数。

use std::path::{Path, PathBuf};

/// 展开开头的 `~` 为用户主目录，其余原样。
pub fn expand_home(raw: &str) -> PathBuf {
    match raw.strip_prefix('~') {
        Some(rest) => {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
            PathBuf::from(format!("{}{}", home.to_string_lossy(), rest))
        }
        None => PathBuf::from(raw),
    }
}

/// 规范化路径：消除 `..`、解符号链接，并剥掉 Windows 的 `\\?\` 前缀。
///
/// 失败时原样返回——路径不存在时调用方通常紧接着会读文件，让读取去报错更准确。
pub fn canonical(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(resolved) => strip_extended_path_prefix(resolved),
        Err(_) => path.to_path_buf(),
    }
}

fn strip_extended_path_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
}

/// 文件所在目录的字符串形式；无父目录（或父目录是空串）时退回 `.`。
pub fn dir_of(path: &Path) -> String {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_string_lossy().into_owned(),
        _ => ".".to_string(),
    }
}

/// 展示用路径：家目录下替换为 `~`，分隔符统一为 `/`。
pub fn display_path(path: &Path) -> String {
    let rendered = if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        format!("~/{}", rest.display())
    } else {
        path.display().to_string()
    };
    rendered.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_home_leaves_plain_path() {
        assert_eq!(expand_home("/tmp/x"), PathBuf::from("/tmp/x"));
        assert_eq!(expand_home("./rel"), PathBuf::from("./rel"));
    }

    #[test]
    fn expand_home_expands_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/x"), home.join("x"));
    }

    #[test]
    fn dir_of_returns_parent() {
        assert_eq!(dir_of(Path::new("/a/b/c.json")), "/a/b");
        assert_eq!(dir_of(Path::new("c.json")), ".");
    }

    #[test]
    fn display_path_shortens_home() {
        let path = dirs::home_dir().unwrap().join("x");
        assert_eq!(display_path(&path), "~/x");
    }

    #[test]
    fn display_path_normalizes_separators() {
        assert_eq!(display_path(Path::new("plain/path")), "plain/path");
    }
}
