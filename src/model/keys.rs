//! 别名文件 key 的构建规则。

/// 构建文件 key：全局目录加 `@@`，当前目录加 `@`。
pub(crate) fn build_file_key(stem: &str, is_global: bool) -> String {
    let prefix = if is_global { "@@" } else { "@" };
    if stem.is_empty() {
        prefix.to_string()
    } else {
        format!("{}{}", prefix, stem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_key_local_empty_stem() {
        assert_eq!(build_file_key("", false), "@");
    }

    #[test]
    fn build_key_global_empty_stem() {
        assert_eq!(build_file_key("", true), "@@");
    }

    #[test]
    fn build_key_local_with_stem() {
        assert_eq!(build_file_key("release", false), "@release");
    }

    #[test]
    fn build_key_global_with_stem() {
        assert_eq!(build_file_key("release", true), "@@release");
    }
}
