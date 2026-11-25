//! 别名文件 key 的构建规则。
//!
//! key 文本只有本模块写；其余代码判断作用域一律走字段或 `AliasFile::is_global`。

use super::scope::Scope;

/// 构建文件 key：`local@<stem>` / `global@<stem>`，空 stem 为 `local@` / `global@`。
pub(crate) fn build_file_key(stem: &str, is_global: bool) -> String {
    let prefix = if is_global {
        Scope::Global.token()
    } else {
        Scope::Local.token()
    };
    format!("{}{}", prefix, stem)
}

/// key 是否指向匿名文件。
///
/// 依赖 `stem` 不含 `@`：`local@x` 的 `x` 不可能是空串之外的前缀形态。
pub(crate) fn is_anonymous_key(key: &str) -> bool {
    key == Scope::Local.token() || key == Scope::Global.token()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_key_local_empty_stem() {
        assert_eq!(build_file_key("", false), "local@");
    }

    #[test]
    fn build_key_global_empty_stem() {
        assert_eq!(build_file_key("", true), "global@");
    }

    #[test]
    fn build_key_local_with_stem() {
        assert_eq!(build_file_key("release", false), "local@release");
    }

    #[test]
    fn build_key_global_with_stem() {
        assert_eq!(build_file_key("release", true), "global@release");
    }

    #[test]
    fn detects_anonymous_keys() {
        assert!(is_anonymous_key("local@"));
        assert!(is_anonymous_key("global@"));
        assert!(!is_anonymous_key("local@ops"));
        assert!(!is_anonymous_key("global@ops"));
    }
}
