/// 精确执行语法：@file.key 和 @@file.key。

use super::merge::apply_inherited;
use super::parse::{is_alias_value, to_alias_value};
use super::types::{AliasFile, ResolvedAlias};

// ---------------------------------------------------------------------------
// 精确执行语法
// ---------------------------------------------------------------------------

/// 解析精确执行语法。
///
/// Returns (file_key, alias_key) 或 None。
///
/// @release.build  → ("@release", "build")
/// @@release.build → ("@@release", "build")
/// @.build         → ("@", "build")
/// @@.build        → ("@@", "build")
/// 无 . 或 alias_key 为空 → None（走普通查找）
#[allow(dead_code)]
pub fn parse_exact_syntax(input: &str) -> Option<(String, String)> {
    let (prefix, rest) = if input.starts_with("@@") {
        ("@@", &input[2..])
    } else if input.starts_with('@') {
        ("@", &input[1..])
    } else {
        return None;
    };

    let dot_idx = rest.find('.')?;
    let file_name = &rest[..dot_idx];
    let alias_key = &rest[dot_idx + 1..];

    if alias_key.is_empty() {
        return None;
    }

    Some((format!("{}{}", prefix, file_name), alias_key.to_string()))
}

/// 在 files 数组中按 file_key 精确查找别名。
///
/// Returns (ResolvedAlias, display_source) 或 None。
/// display_source 如 "@release.group.sub"。
#[allow(dead_code)]
pub fn lookup_exact_alias(
    files: &[AliasFile],
    file_key: &str,
    alias_key: &str,
) -> Option<(ResolvedAlias, String)> {
    for f in files {
        if f.key != file_key {
            continue;
        }
        let parts: Vec<&str> = alias_key.split('.').collect();
        let mut current: &serde_json::Value = &serde_json::Value::Object(f.aliases.clone());
        // 沿路径遍历时累积分组级 $cwd / $interactive，子级覆盖父级
        let mut group_cwd: Option<&str> = None;
        let mut group_interactive: Option<bool> = None;
        for (i, part) in parts.iter().enumerate() {
            let obj = current.as_object()?;
            // 累积分组级继承属性（文件根级的 $cwd/$interactive 已被 parse_alias_file 移除，此处不影响）
            if let Some(c) = obj.get("$cwd").and_then(|v| v.as_str()) {
                group_cwd = Some(c);
            }
            if let Some(v) = obj.get("$interactive") {
                if let Some(b) = v.as_bool() {
                    group_interactive = Some(b);
                }
            }
            current = obj.get(*part)?;
            if i < parts.len() - 1 && !current.is_object() {
                return None;
            }
        }
        if is_alias_value(current) {
            let value = to_alias_value(current)?;
            let effective_cwd = group_cwd.or(f.inherited_cwd.as_deref());
            let effective_interactive = group_interactive.or(f.inherited_interactive);
            let value = apply_inherited(value, effective_cwd, effective_interactive);
            let display_source = format!("{}.{}", file_key, alias_key);
            let source_path = f.path.parent().map(|p| p.to_path_buf());
            return Some((
                ResolvedAlias {
                    value,
                    source: file_key.to_string(),
                    source_path,
                },
                display_source,
            ));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== parse_exact_syntax ====================

    #[test]
    fn parse_at_file_dot_key() {
        assert_eq!(
            parse_exact_syntax("@release.build"),
            Some(("@release".into(), "build".into()))
        );
    }

    #[test]
    fn parse_double_at_file_dot_key() {
        assert_eq!(
            parse_exact_syntax("@@release.build"),
            Some(("@@release".into(), "build".into()))
        );
    }

    #[test]
    fn parse_at_dot_key() {
        assert_eq!(
            parse_exact_syntax("@.build"),
            Some(("@".into(), "build".into()))
        );
    }

    #[test]
    fn parse_double_at_dot_key() {
        assert_eq!(
            parse_exact_syntax("@@.build"),
            Some(("@@".into(), "build".into()))
        );
    }

    #[test]
    fn parse_at_only_no_dot() {
        assert_eq!(parse_exact_syntax("@release"), None);
    }

    #[test]
    fn parse_no_at_prefix() {
        assert_eq!(parse_exact_syntax("release.build"), None);
        assert_eq!(parse_exact_syntax("normal"), None);
    }

    #[test]
    fn parse_at_dot_empty_key() {
        assert_eq!(parse_exact_syntax("@release."), None);
        assert_eq!(parse_exact_syntax("@."), None);
    }

    #[test]
    fn parse_dot_in_file_name() {
        // file_name 部分不含 dot（第一个 dot 即分隔符），多 dot 后面的归 alias_key
        assert_eq!(
            parse_exact_syntax("@release.build.prod"),
            Some(("@release".into(), "build.prod".into()))
        );
    }

    #[test]
    fn parse_with_hyphens() {
        assert_eq!(
            parse_exact_syntax("@my-file.run-test"),
            Some(("@my-file".into(), "run-test".into()))
        );
    }
}
