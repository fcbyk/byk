//! 别名文件内容摘要：供确认提示与清空判断使用。

use std::fs;
use std::path::Path;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContentSummary {
    Empty,
    Aliases(usize),
    Link,
    Unreadable,
}

pub(super) fn summarize(path: &Path) -> ContentSummary {
    let Ok(text) = fs::read_to_string(path) else {
        return ContentSummary::Unreadable;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return ContentSummary::Unreadable;
    };

    match &value {
        Value::Array(_) => ContentSummary::Link,
        Value::Object(map) if map.is_empty() => ContentSummary::Empty,
        Value::Object(map) => ContentSummary::Aliases(count_object(map)),
        _ => ContentSummary::Unreadable,
    }
}

pub(super) fn aliases_label(count: usize) -> String {
    if count == 1 {
        "(1 alias)".to_string()
    } else {
        format!("({} aliases)", count)
    }
}

fn count_object(map: &serde_json::Map<String, Value>) -> usize {
    match map.get("$cmd") {
        Some(Value::String(_)) => 1 + count_children(map),
        Some(_) => 0,
        None => count_children(map),
    }
}

fn count_children(map: &serde_json::Map<String, Value>) -> usize {
    map.iter()
        .filter(|(key, _)| !key.starts_with('$') && !key.contains('@') && !key.contains('.'))
        .map(|(_, value)| count_value(value))
        .sum()
}

fn count_value(value: &Value) -> usize {
    match value {
        Value::String(_) => 1,
        Value::Object(map) => count_object(map),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(temp: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = temp.path().join("ops.byk.json");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn summary_empty_object() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(&temp, "{}\n");

        assert_eq!(summarize(&path), ContentSummary::Empty);
    }

    #[test]
    fn summary_flat_and_nested_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            &temp,
            r#"{
                "build": "echo build",
                "group": {"one": "echo one", "two": {"$cmd": "echo two"}},
                "count": 42,
                "flag": true,
                "a.b": "ignored",
                "$priority": 1
            }"#,
        );

        assert_eq!(summarize(&path), ContentSummary::Aliases(3));
    }

    #[test]
    fn summary_meta_node_with_children() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(
            &temp,
            r#"{"dev": {"$cmd": "echo dev", "inner": "echo inner"}}"#,
        );

        assert_eq!(summarize(&path), ContentSummary::Aliases(2));
    }

    #[test]
    fn summary_link_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(&temp, r#"["shared.byk.json"]"#);

        assert_eq!(summarize(&path), ContentSummary::Link);
    }

    #[test]
    fn summary_invalid_json_is_unreadable() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(&temp, "{not json");

        assert_eq!(summarize(&path), ContentSummary::Unreadable);
    }

    #[test]
    fn summary_scalar_is_unreadable() {
        let temp = tempfile::tempdir().unwrap();
        let path = write(&temp, "42");

        assert_eq!(summarize(&path), ContentSummary::Unreadable);
    }

    #[test]
    fn alias_labels() {
        assert_eq!(aliases_label(0), "(0 aliases)");
        assert_eq!(aliases_label(1), "(1 alias)");
        assert_eq!(aliases_label(3), "(3 aliases)");
    }
}
