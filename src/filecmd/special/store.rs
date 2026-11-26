//! 特殊文件共用的 JSON 读写。
//!
//! 三条硬规则对 `config.json` 与 `tap/index.json` 一致：文件不存在是正常状态；读不懂就
//! 退回空对象并给一行警告（配置坏了连 `byk -h` 都打不开，那正是最需要帮助的时候）；
//! 写入原子，并保留读到的全部键。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// 读出顶层对象；第二个返回值是需要提示给用户的一行警告。
pub(crate) fn read_object(path: &Path) -> (Map<String, Value>, Option<String>) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Map::new(), None),
        Err(e) => {
            return (
                Map::new(),
                Some(format!(
                    "Warning: failed to read {}: {}; using defaults",
                    path.display(),
                    e
                )),
            );
        }
    };

    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(object)) => (object, None),
        Ok(_) => (
            Map::new(),
            Some(format!(
                "Warning: {} is not a JSON object; using defaults",
                path.display()
            )),
        ),
        Err(e) => (
            Map::new(),
            Some(format!(
                "Warning: failed to parse {}: {}; using defaults",
                path.display(),
                e
            )),
        ),
    }
}

/// 原子写入：先写临时文件再改名，中途失败不会留下半截 JSON。
pub(crate) fn write_object(path: &Path, object: &Map<String, Value>) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    let mut text = serde_json::to_string_pretty(object)
        .map_err(|e| format!("Failed to render {}: {}", path.display(), e))?;
    text.push('\n');

    let temp = temp_path(path);
    fs::write(&temp, text).map_err(|e| format!("Failed to write {}: {}", temp.display(), e))?;

    // Windows 的 rename 不能覆盖已存在的目标；配置文件丢了最多回默认值，
    // 不值得为它引入一次 Win32 调用，先删再改名即可。
    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Failed to write {}: {}", path.display(), e)
    })
}

/// 同目录下的临时文件名：改名是原子的前提是两者在同一个文件系统上。
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let (map, warning) = read_object(&temp.path().join("config.json"));

        assert!(map.is_empty());
        assert!(warning.is_none());
    }

    #[test]
    fn broken_json_falls_back_with_a_warning() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, "{not json").unwrap();

        let (map, warning) = read_object(&path);

        assert!(map.is_empty());
        assert!(warning.unwrap().starts_with("Warning: failed to parse"));
    }

    #[test]
    fn non_object_falls_back_with_a_warning() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, "[1, 2]").unwrap();

        let (map, warning) = read_object(&path);

        assert!(map.is_empty());
        assert!(warning.unwrap().contains("is not a JSON object"));
    }

    #[test]
    fn reads_an_object_as_is() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, r#"{"banner":"never"}"#).unwrap();

        let (map, warning) = read_object(&path);

        assert!(warning.is_none());
        assert_eq!(map.get("banner").and_then(Value::as_str), Some("never"));
    }

    #[test]
    fn write_creates_the_parent_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".byk").join("config.json");

        write_object(&path, &object(json!({"banner": "never"}))).unwrap();

        assert!(path.exists());
    }

    #[test]
    fn write_round_trips_and_keeps_unknown_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, r#"{"banner":"auto","future.key":"keep me"}"#).unwrap();

        let (mut map, _) = read_object(&path);
        map.insert("banner".into(), json!("never"));
        write_object(&path, &map).unwrap();

        let (after, warning) = read_object(&path);
        assert!(warning.is_none());
        assert_eq!(after.get("banner").and_then(Value::as_str), Some("never"));
        assert_eq!(
            after.get("future.key").and_then(Value::as_str),
            Some("keep me")
        );
    }

    #[test]
    fn write_leaves_no_temp_file_behind() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");

        write_object(&path, &object(json!({"banner": "always"}))).unwrap();

        assert!(!temp_path(&path).exists());
    }

    #[test]
    fn written_file_ends_with_a_newline() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");

        write_object(&path, &object(json!({"banner": "always"}))).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("}\n"), "{text:?}");
    }
}
