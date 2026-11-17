use std::path::PathBuf;

/// CLI 持久化目录布局。
///
/// byk 只读两处：家目录（`~/.byk`）与别名目录（`~/.byk/alias`）。
pub struct PathLayout {
    pub root_dir: PathBuf,
    pub alias_dir: PathBuf,
}

impl PathLayout {
    pub fn new() -> Self {
        Self::with_name("byk")
    }

    /// 不创建任何目录：别名目录缺失时扫描结果天然为空。
    pub fn with_name(app_name: &str) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| {
            eprintln!("无法获取用户主目录，回退到当前目录");
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        });
        let root_dir = home.join(format!(".{}", app_name));
        let alias_dir = root_dir.join("alias");

        PathLayout {
            root_dir,
            alias_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_layout_with_name_structure() {
        let layout = PathLayout::with_name("fcbyk_test_paths");
        let home = dirs::home_dir().unwrap();

        assert_eq!(layout.root_dir, home.join(".fcbyk_test_paths"));
        assert_eq!(
            layout.alias_dir,
            home.join(".fcbyk_test_paths").join("alias")
        );
    }

    #[test]
    fn path_layout_creates_no_directories() {
        let layout = PathLayout::with_name("fcbyk_test_paths2");

        assert!(!layout.root_dir.exists());
        assert!(!layout.alias_dir.exists());
    }
}
