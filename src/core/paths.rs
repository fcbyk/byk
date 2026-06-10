use std::path::PathBuf;

/// CLI 持久化目录布局。
pub struct PathLayout {
    pub root_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub alias_dir: PathBuf,
    pub node_pkgs_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl PathLayout {
    /// 使用默认 app_name "byk" 构建。
    pub fn new() -> Self {
        Self::with_name("byk")
    }

    /// 使用自定义 app_name 构建目录布局，自动创建子目录。
    pub fn with_name(app_name: &str) -> Self {
        let home = dirs::home_dir()
            .unwrap_or_else(|| {
                eprintln!("无法获取用户主目录，回退到当前目录");
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            });
        let root_dir = home.join(format!(".{}", app_name));

        let logs_dir = root_dir.join("logs");
        let alias_dir = root_dir.join("alias");
        let node_pkgs_dir = root_dir.join("node-pkgs");
        let cache_dir = root_dir.join("cache");

        for d in [&logs_dir, &alias_dir, &node_pkgs_dir, &cache_dir] {
            std::fs::create_dir_all(d).unwrap_or_else(|e| {
                eprintln!("无法创建持久化目录 {}: {}", d.display(), e);
            });
        }

        PathLayout {
            root_dir,
            logs_dir,
            alias_dir,
            node_pkgs_dir,
            cache_dir,
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_layout_with_name_structure() {
        let layout = PathLayout::with_name("fcbyk_test_paths");
        let home = dirs::home_dir().unwrap();

        assert_eq!(layout.root_dir, home.join(".fcbyk_test_paths"));
        assert_eq!(layout.logs_dir, home.join(".fcbyk_test_paths").join("logs"));
        assert_eq!(
            layout.alias_dir,
            home.join(".fcbyk_test_paths").join("alias")
        );
        assert_eq!(
            layout.node_pkgs_dir,
            home.join(".fcbyk_test_paths").join("node-pkgs")
        );
        assert_eq!(
            layout.cache_dir,
            home.join(".fcbyk_test_paths").join("cache")
        );
    }

    #[test]
    fn path_layout_creates_directories() {
        let layout = PathLayout::with_name("fcbyk_test_paths2");

        assert!(layout.root_dir.exists());
        assert!(layout.logs_dir.exists());
        assert!(layout.alias_dir.exists());
        assert!(layout.node_pkgs_dir.exists());
        assert!(layout.cache_dir.exists());

        // 清理
        let _ = fs::remove_dir_all(&layout.root_dir);
    }
}
