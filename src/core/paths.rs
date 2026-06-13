use std::path::PathBuf;

/// CLI 持久化目录布局。
pub struct PathLayout {
    pub root_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub alias_dir: PathBuf,
    pub node_pkgs_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// ~/.byk/ 目录是否已存在（用于零配置分叉）
    pub home_exists: bool,
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
        let home_exists = root_dir.is_dir();

        let logs_dir = root_dir.join("logs");
        let alias_dir = root_dir.join("alias");
        let node_pkgs_dir = root_dir.join("node-pkgs");
        let cache_dir = root_dir.join("cache");

        // 子目录不在此处创建，由各子系统按需创建：
        // - cache/  → json_io::write_json 内部 create_dir_all
        // - alias/  → 用户手动放入文件，scan 缺失时天然空
        // - logs/   → 暂未使用，后续日志模块自行创建

        PathLayout {
            root_dir,
            logs_dir,
            alias_dir,
            node_pkgs_dir,
            cache_dir,
            home_exists,
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

        // home_exists=false → 不创建任何目录
        assert!(!layout.root_dir.exists());
        assert!(!layout.logs_dir.exists());
        assert!(!layout.alias_dir.exists());
        assert!(!layout.cache_dir.exists());
        assert!(!layout.node_pkgs_dir.exists());
        assert!(!layout.home_exists);
    }
}
