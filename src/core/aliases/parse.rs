//! 原始 JSON → 结构化别名数据的转换：文件名校验、优先级解析、key 过滤、值类型转换。

use super::types::{AliasDefinition, AliasValue};


/// 文件名 stem 是否合法：空串（匿名文件）或仅含字母、数字、`-`、`_`、中文。
pub(crate) fn validate_filename(stem: &str) -> bool {
    if stem.is_empty() {
        return true;
    }
    if stem.contains('.') {
        return false;
    }
    stem.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || is_cjk_char(c))
}

/// 判断字符是否为 CJK 统一表意文字（U+4E00-U+9FFF）。
fn is_cjk_char(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// 构建文件 key：全局目录加 `@@`，当前目录加 `@`。
pub(crate) fn build_file_key(stem: &str, is_global: bool) -> String {
    let prefix = if is_global { "@@" } else { "@" };
    if stem.is_empty() {
        prefix.to_string()
    } else {
        format!("{}{}", prefix, stem)
    }
}


/// 解析 $priority 字段，失败时返回默认值。
///
/// 规则：
/// - 整数：直接使用（负数不参与优先级合并）
/// - 浮点数 → 向下取整
/// - bool、字符串、null 等 → 默认值
pub(crate) fn parse_priority(raw: &serde_json::Value, default: i32) -> i32 {
    match raw {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i as i32
            } else if let Some(f) = n.as_f64() {
                f as i32
            } else {
                default
            }
        }
        _ => default,
    }
}


/// 递归过滤：丢掉 key 含 `@`/`.` 的条目与非法的值类型。
///
/// 含 `$` 前缀 key 的 object 是别名元数据，整体保留；其余 object 视为分组继续递归。
/// array / number / bool / null 静默丢弃。
pub(crate) fn filter_invalid_keys(
    data: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut result = serde_json::Map::new();
    for (key, val) in data {
        if key.contains('@') || key.contains('.') {
            continue;
        }
        match val {
            serde_json::Value::Object(inner) => {
                if has_meta_key(inner) {
                    result.insert(key.clone(), val.clone());
                } else {
                    let filtered = filter_invalid_keys(inner);
                    if !filtered.is_empty() {
                        result.insert(key.clone(), serde_json::Value::Object(filtered));
                    }
                }
            }
            serde_json::Value::String(_) | serde_json::Value::Bool(_) => {
                result.insert(key.clone(), val.clone());
            }
            _ => {} // 其他类型（Number, Array, Null）静默丢弃
        }
    }
    result
}

/// object 是否含 `$cmd`（叶子节点）；只有 `$cwd`/`$interactive` 的是分组。
fn has_meta_key(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    obj.contains_key("$cmd")
}


/// string 或含 `$` 前缀 key 的 object 都是叶子。
pub(crate) fn is_alias_value(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::String(_) => true,
        serde_json::Value::Object(obj) => has_meta_key(obj),
        _ => false,
    }
}

pub(crate) fn to_alias_value(val: &serde_json::Value) -> Option<AliasValue> {
    match val {
        serde_json::Value::String(s) => Some(AliasValue::Str(s.clone())),
        serde_json::Value::Object(_) => serde_json::from_value(val.clone()).ok(),
        _ => None,
    }
}

pub fn to_alias_definition(value: &AliasValue) -> Option<AliasDefinition> {
    match value {
        AliasValue::Str(command) => Some(AliasDefinition {
            command: command.clone(),
            cwd: None,
            interactive: false,
            description: None,
        }),
        AliasValue::Meta {
            cmd,
            cwd,
            interactive,
            description,
            ..
        } => Some(AliasDefinition {
            command: cmd.clone(),
            cwd: cwd.clone(),
            interactive: interactive.unwrap_or(false),
            description: description.clone(),
        }),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;


    #[test]
    fn validate_empty_stem() {
        assert!(validate_filename(""));
    }

    #[test]
    fn validate_ascii_alphanumeric() {
        assert!(validate_filename("release"));
        assert!(validate_filename("build123"));
    }

    #[test]
    fn validate_with_hyphen() {
        assert!(validate_filename("my-alias"));
    }

    #[test]
    fn validate_with_underscore() {
        assert!(validate_filename("my_alias"));
    }

    #[test]
    fn validate_cjk_chars() {
        assert!(validate_filename("部署"));
        assert!(validate_filename("测试文件"));
    }

    #[test]
    fn validate_rejects_dot() {
        assert!(!validate_filename("file.name"));
        assert!(!validate_filename("a.b"));
    }

    #[test]
    fn validate_rejects_at() {
        assert!(!validate_filename("@release"));
    }

    #[test]
    fn validate_rejects_special_chars() {
        assert!(!validate_filename("hello!"));
        assert!(!validate_filename("path/to"));
        assert!(!validate_filename("name space"));
    }


    #[test]
    fn cjk_char_in_range() {
        assert!(is_cjk_char('中'));  // U+4E2D
        assert!(is_cjk_char('文'));  // U+6587
    }

    #[test]
    fn cjk_char_boundary() {
        assert!(is_cjk_char('\u{4e00}')); // 起始
        assert!(is_cjk_char('\u{9fff}')); // 结束
    }

    #[test]
    fn cjk_char_out_of_range() {
        assert!(!is_cjk_char('a'));
        assert!(!is_cjk_char('1'));
        assert!(!is_cjk_char('-'));
        assert!(!is_cjk_char('\u{3fff}')); // CJK 扩展前
    }


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


    #[test]
    fn priority_integer() {
        assert_eq!(parse_priority(&json!(42), 10), 42);
        assert_eq!(parse_priority(&json!(-5), 10), -5);
    }

    #[test]
    fn priority_float_truncated() {
        assert_eq!(parse_priority(&json!(3.7), 10), 3);
        assert_eq!(parse_priority(&json!(-2.5), 10), -2);
    }

    #[test]
    fn priority_non_number_fallback() {
        assert_eq!(parse_priority(&json!("high"), 10), 10);
        assert_eq!(parse_priority(&json!(true), 10), 10);
        assert_eq!(parse_priority(&json!(null), 10), 10);
        assert_eq!(parse_priority(&json!([]), 10), 10);
    }

    #[test]
    fn has_meta_key_true() {
        let obj = json!({"$cmd": "echo hello"}).as_object().unwrap().clone();
        assert!(has_meta_key(&obj));
    }

    #[test]
    fn has_meta_key_false() {
        let obj = json!({"name": "test"}).as_object().unwrap().clone();
        assert!(!has_meta_key(&obj));
    }

    #[test]
    fn has_meta_key_empty() {
        let obj = serde_json::Map::new();
        assert!(!has_meta_key(&obj));
    }


    #[test]
    fn filter_empty_object() {
        let input = serde_json::Map::new();
        assert!(filter_invalid_keys(&input).is_empty());
    }

    #[test]
    fn filter_rejects_at_in_key() {
        let data = json!({"k@ey": "val", "ok": "yes"}).as_object().unwrap().clone();
        let result = filter_invalid_keys(&data);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("ok"));
    }

    #[test]
    fn filter_rejects_dot_in_key() {
        let data = json!({"k.ey": "val", "ok": "yes"}).as_object().unwrap().clone();
        let result = filter_invalid_keys(&data);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("ok"));
    }

    #[test]
    fn filter_keeps_string_value() {
        let data = json!({"cmd": "echo hello"}).as_object().unwrap().clone();
        let result = filter_invalid_keys(&data);
        assert_eq!(result.len(), 1);
        assert_eq!(result["cmd"], json!("echo hello"));
    }

    #[test]
    fn filter_keeps_bool_value() {
        let data = json!({"flag": true}).as_object().unwrap().clone();
        let result = filter_invalid_keys(&data);
        assert_eq!(result.len(), 1);
        assert_eq!(result["flag"], json!(true));
    }

    #[test]
    fn filter_drops_number() {
        let data = json!({"count": 42}).as_object().unwrap().clone();
        assert!(filter_invalid_keys(&data).is_empty());
    }

    #[test]
    fn filter_drops_null() {
        let data = json!({"n": null}).as_object().unwrap().clone();
        assert!(filter_invalid_keys(&data).is_empty());
    }

    #[test]
    fn filter_drops_array() {
        let data = json!({"items": [1, 2]}).as_object().unwrap().clone();
        assert!(filter_invalid_keys(&data).is_empty());
    }

    #[test]
    fn filter_preserves_meta_object() {
        let data = json!({"deploy": {"$cmd": "deploy.sh", "$cwd": "/app"}})
            .as_object()
            .unwrap()
            .clone();
        let result = filter_invalid_keys(&data);
        assert_eq!(result.len(), 1);
        assert!(result["deploy"].as_object().unwrap().contains_key("$cmd"));
    }

    #[test]
    fn filter_recurses_nested_object() {
        let data = json!({"group": {"inner": "value"}})
            .as_object()
            .unwrap()
            .clone();
        let result = filter_invalid_keys(&data);
        assert_eq!(result.len(), 1);
        let group = result["group"].as_object().unwrap();
        assert_eq!(group["inner"], json!("value"));
    }

    #[test]
    fn filter_drops_empty_nested_after_filtering() {
        // 嵌套对象中所有值都被过滤掉 → 整个节点消失
        let data = json!({"group": {"count": 42}})
            .as_object()
            .unwrap()
            .clone();
        assert!(filter_invalid_keys(&data).is_empty());
    }


    #[test]
    fn alias_value_string() {
        assert!(is_alias_value(&json!("echo hello")));
    }

    #[test]
    fn alias_value_meta_object() {
        assert!(is_alias_value(&json!({"$cmd": "build.sh"})));
    }

    #[test]
    fn alias_value_plain_object_is_not_leaf() {
        assert!(!is_alias_value(&json!({"name": "test"})));
    }

    #[test]
    fn alias_value_number_is_not_leaf() {
        assert!(!is_alias_value(&json!(42)));
    }

    #[test]
    fn alias_value_null_is_not_leaf() {
        assert!(!is_alias_value(&json!(null)));
    }


    #[test]
    fn to_alias_from_string() {
        let result = to_alias_value(&json!("echo hi"));
        assert!(matches!(result, Some(AliasValue::Str(s)) if s == "echo hi"));
    }

    #[test]
    fn to_alias_from_meta() {
        let result = to_alias_value(&json!({"$cmd": "npm run build", "$cwd": "/proj"}));
        match result {
            Some(AliasValue::Meta { cmd, cwd, .. }) => {
                assert_eq!(cmd, "npm run build");
                assert_eq!(cwd, Some("/proj".into()));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn to_alias_from_invalid() {
        assert!(to_alias_value(&json!(42)).is_none());
        assert!(to_alias_value(&json!(null)).is_none());
    }


    #[test]
    fn definition_from_str() {
        let av = AliasValue::Str("echo hello".into());
        let def = to_alias_definition(&av).unwrap();
        assert_eq!(def.command, "echo hello");
        assert_eq!(def.cwd, None);
        assert!(!def.interactive);
    }

    #[test]
    fn definition_from_meta() {
        let av = AliasValue::Meta {
            cmd: "build".into(),
            cwd: Some("/app".into()),
            interactive: Some(true),
            description: None,
        };
        let def = to_alias_definition(&av).unwrap();
        assert_eq!(def.command, "build");
        assert_eq!(def.cwd, Some("/app".into()));
        assert!(def.interactive);
    }

    #[test]
    fn definition_from_meta_defaults() {
        let av = AliasValue::Meta {
            cmd: "run".into(),
            cwd: None,
            interactive: None,
            description: None,
        };
        let def = to_alias_definition(&av).unwrap();
        assert!(!def.interactive);
        assert_eq!(def.cwd, None);
    }
}
