//! 别名 key 的公共校验：点号分段、保留前缀与 `$` 元数据。

/// 校验文件命令使用的别名 key，失败时返回带提示的消息。
pub(super) fn validate(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("Alias key must not be empty".to_string());
    }
    if key.starts_with('-') {
        return Err(
            "Alias key must not start with '-'\nHint: place the alias key before options"
                .to_string(),
        );
    }
    for segment in key.split('.') {
        if segment.is_empty() {
            return Err(format!("Invalid alias key: {:?} has an empty segment", key));
        }
        if segment.contains('@') {
            return Err(format!("Invalid alias key: {:?} contains `@`", key));
        }
        if segment.starts_with('$') {
            return Err(format!(
                "Invalid alias key: {:?} is a metadata field\nHint: remove metadata with `..set <key> --unset`",
                segment
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_dotted_keys() {
        assert!(validate("build").is_ok());
        assert!(validate("web.deploy").is_ok());
        assert!(validate("a$b").is_ok());
    }

    #[test]
    fn rejects_empty_key() {
        assert_eq!(validate("").unwrap_err(), "Alias key must not be empty");
    }

    #[test]
    fn rejects_leading_dash() {
        let err = validate("-build").unwrap_err();
        assert!(err.starts_with("Alias key must not start with '-'"));
        assert!(err.contains("before options"));
    }

    #[test]
    fn rejects_empty_segment() {
        assert!(validate("a..b").unwrap_err().contains("empty segment"));
        assert!(validate(".a").unwrap_err().contains("empty segment"));
        assert!(validate("a.").unwrap_err().contains("empty segment"));
    }

    #[test]
    fn rejects_at_sign() {
        assert!(validate("a@b").unwrap_err().contains("contains `@`"));
    }

    #[test]
    fn rejects_metadata_fields_with_unset_hint() {
        let err = validate("web.$cwd").unwrap_err();
        assert!(err.contains("is a metadata field"));
        assert!(err.contains("--unset"));
    }
}
