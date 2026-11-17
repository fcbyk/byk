//! 通用 JSON I/O：读取、pretty 序列化与写入。

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// 读取并解析一个 JSON 文件。
pub fn read_value(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&text).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// pretty 序列化，不带结尾换行。
pub fn pretty<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| format!("Failed to encode JSON: {}", e))
}

/// pretty 序列化，并补一个结尾换行。
pub fn pretty_line<T: Serialize>(value: &T) -> Result<String, String> {
    let mut text = pretty(value)?;
    text.push('\n');
    Ok(text)
}

/// 原样写入文本。
pub fn write_text(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// pretty 序列化并写入文件。
pub fn write_value<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write_text(path, &pretty_line(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pretty_has_no_trailing_newline() {
        let text = pretty(&json!({"a": 1})).unwrap();
        assert_eq!(text, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn pretty_line_adds_one_newline() {
        let text = pretty_line(&json!({"a": 1})).unwrap();
        assert_eq!(text, "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn write_value_roundtrips() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("x.json");
        let value = json!({"a": [1, 2], "b": "x"});

        write_value(&path, &value).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.ends_with('\n'));
        assert_eq!(read_value(&path).unwrap(), value);
    }

    #[test]
    fn read_value_reports_the_path_on_invalid_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("x.json");
        fs::write(&path, "{not json").unwrap();

        let error = read_value(&path).unwrap_err();
        assert!(error.contains(&path.display().to_string()), "{error}");
    }
}
