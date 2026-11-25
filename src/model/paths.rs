use std::path::PathBuf;

/// CLI 持久化目录布局。
///
/// byk 只碰两处：家目录（`~/.byk`）与别名目录（`~/.byk/alias`）。
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

    /// `sys@config` 的目标文件：CLI Home 根部的 `config.json`。
    ///
    /// 由 `root_dir` 推导而不是单独存一个字段，省得两处各写一遍、
    /// 将来改了目录却只改到一半。
    pub fn config_path(&self) -> PathBuf {
        self.root_dir.join("config.json")
    }

    /// `sys@tap` 管理的仓库目录：CLI Home 下的 `tap/`。
    ///
    /// 布局是 `<tap>/<user>/<repo>/`，与 `clone --tap` 扫描本地索引的两层结构一致。
    pub fn tap_dir(&self) -> PathBuf {
        self.root_dir.join("tap")
    }

    /// `sys@tap` 的账本：`tap/index.json`。
    ///
    /// 只有 `sys@tap` 读写它。放在 `tap/` 根部是安全的：扫描器按
    /// `<tap>/<user>/<repo>/byk.tap.json` 两层找索引，`index.json` 含点号，
    /// 不会被当成用户名。
    pub fn tap_index_path(&self) -> PathBuf {
        self.tap_dir().join("index.json")
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
        assert!(!layout.config_path().exists());
    }

    #[test]
    fn config_sits_in_the_cli_home() {
        let layout = PathLayout::with_name("fcbyk_test_paths3");
        let home = dirs::home_dir().unwrap();

        assert_eq!(
            layout.config_path(),
            home.join(".fcbyk_test_paths3").join("config.json")
        );
    }

    #[test]
    fn tap_lives_next_to_the_alias_directory() {
        let layout = PathLayout::with_name("fcbyk_test_paths4");
        let home = dirs::home_dir().unwrap();
        let root = home.join(".fcbyk_test_paths4");

        assert_eq!(layout.tap_dir(), root.join("tap"));
        assert_eq!(layout.tap_index_path(), root.join("tap").join("index.json"));
    }
}
