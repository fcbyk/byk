//! 设置项注册表：键名、默认值、候选与说明。
//!
//! 这些键由 byk 自己消费，所以必须有注册表：手写成 `baner` 时，
//! 自由 JSON 只会静默失效，事后极难查；注册表能当场报错，也能自己当文档。

/// 一个设置项。v1 全是枚举型字符串，值域由 `choices` 定。
pub(crate) struct SettingInfo {
    pub(crate) key: &'static str,
    pub(crate) default: &'static str,
    pub(crate) choices: &'static [&'static str],
    pub(crate) description: &'static str,
}

impl SettingInfo {
    pub(crate) fn accepts(&self, value: &str) -> bool {
        self.choices.contains(&value)
    }
}

/// 顺序即表格、帮助与错误提示的展示顺序。
pub(crate) const SETTINGS: &[SettingInfo] = &[SettingInfo {
    key: "banner",
    default: "auto",
    choices: &["auto", "always", "never"],
    description: "Show the banner in help output",
}];

pub(crate) fn find(key: &str) -> Option<&'static SettingInfo> {
    SETTINGS.iter().find(|info| info.key == key)
}

pub(crate) fn names() -> Vec<&'static str> {
    SETTINGS.iter().map(|info| info.key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_registered_keys() {
        let info = find("banner").expect("banner is registered");
        assert_eq!(info.default, "auto");
        assert_eq!(info.choices, &["auto", "always", "never"]);
    }

    #[test]
    fn rejects_unregistered_keys() {
        assert!(find("baner").is_none());
        assert!(find("").is_none());
        // 键名含点号是位置式的残留写法，注册表里不该有
        assert!(find("ui.banner").is_none());
    }

    #[test]
    fn accepts_only_listed_values() {
        let info = find("banner").unwrap();
        assert!(info.accepts("auto"));
        assert!(info.accepts("always"));
        assert!(info.accepts("never"));
        assert!(!info.accepts("nvr"));
        assert!(!info.accepts("AUTO"));
        assert!(!info.accepts(""));
    }

    #[test]
    fn names_follow_registry_order() {
        assert_eq!(names(), vec!["banner"]);
    }
}
