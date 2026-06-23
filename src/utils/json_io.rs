//! 统一的 JSON 读写工具。
//!
//! 提供原子写入（临时文件 + rename）和反序列化读取能力。

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// 读取并反序列化 JSON 文件，文件不存在或解析失败返回 None。
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    if !path.is_file() {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 序列化并写入 JSON 文件，使用原子写入（临时文件 + rename）。
pub fn write_json<T: Serialize>(path: &Path, data: &T) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json_str = match serde_json::to_string_pretty(data) {
        Ok(s) => s,
        Err(_) => return,
    };
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, &json_str).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::fs;
    use std::path::PathBuf;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("fcbyk_test");
        let _ = fs::create_dir_all(&dir);
        dir.join(name)
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Person {
        name: String,
        age: u32,
    }

    // ==================== read_json ====================

    #[test]
    fn read_json_returns_none_for_nonexistent_file() {
        let result: Option<Person> = read_json(&temp_file("nonexistent.json"));
        assert!(result.is_none());
    }

    #[test]
    fn read_json_returns_none_for_invalid_json() {
        let path = temp_file("invalid.json");
        fs::write(&path, "not valid json").unwrap();
        let result: Option<Person> = read_json(&path);
        assert!(result.is_none());
    }

    #[test]
    fn read_json_returns_none_for_wrong_structure() {
        let path = temp_file("wrong_struct.json");
        fs::write(&path, r#"{"foo": "bar"}"#).unwrap();
        let result: Option<Person> = read_json(&path);
        assert!(result.is_none());
    }

    #[test]
    fn read_json_round_trip() {
        let path = temp_file("roundtrip.json");
        let person = Person {
            name: "Alice".into(),
            age: 30,
        };
        write_json(&path, &person);

        let result: Option<Person> = read_json(&path);
        assert_eq!(result, Some(person));
    }

    // ==================== write_json ====================

    #[test]
    fn write_json_creates_parent_dirs() {
        let path = std::env::temp_dir()
            .join("fcbyk_test")
            .join("nested")
            .join("sub")
            .join("data.json");
        // 确保父目录不存在
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        let person = Person {
            name: "Bob".into(),
            age: 25,
        };
        write_json(&path, &person);
        assert!(path.is_file());
    }

    #[test]
    fn write_json_produces_readable_file() {
        let path = temp_file("readable.json");
        let person = Person {
            name: "Carol".into(),
            age: 42,
        };
        write_json(&path, &person);

        let content = fs::read_to_string(&path).unwrap();
        let parsed: Person = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed, person);
    }

    #[test]
    fn write_json_overwrites_existing() {
        let path = temp_file("overwrite.json");
        let first = Person {
            name: "Old".into(),
            age: 1,
        };
        write_json(&path, &first);

        let second = Person {
            name: "New".into(),
            age: 2,
        };
        write_json(&path, &second);

        let result: Option<Person> = read_json(&path);
        assert_eq!(result, Some(second));
    }

    #[test]
    fn write_json_empty_vec() {
        let path = temp_file("empty_vec.json");
        let data: Vec<String> = vec![];
        write_json(&path, &data);

        let result: Option<Vec<String>> = read_json(&path);
        assert_eq!(result, Some(data));
    }

    #[test]
    fn write_json_special_characters() {
        let path = temp_file("special.json");
        let person = Person {
            name: "名字\n包含\\特殊\t字符".into(),
            age: 100,
        };
        write_json(&path, &person);

        let result: Option<Person> = read_json(&path);
        assert_eq!(result, Some(person));
    }
}
