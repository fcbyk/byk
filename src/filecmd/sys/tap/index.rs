//! `tap/index.json` 的读取。
//!
//! 文件形态：`{ "<user>/<repo>": { "origin": "github", "ref": "HEAD" } }`。
//!
//! 只放推导不出来的东西——目录位置由键决定，不记绝对路径，换机器回家目录
//! 也不会让账本变成谎话。写入走 [`super::store`]，与 `config.json` 同一套原子写。

use serde_json::{Map, Value};

/// 账本里的一行。
pub(crate) struct Entry {
    pub(crate) key: String,
    pub(crate) origin: String,
    pub(crate) reference: String,
}

/// 按字母序列出全部条目。
///
/// 值读不懂也照样列出来（字段显示 `?`），否则用户既看不见也删不掉它。
pub(crate) fn entries(object: &Map<String, Value>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = object
        .iter()
        .map(|(key, value)| Entry {
            key: key.clone(),
            origin: field(value, "origin").unwrap_or("?").to_string(),
            reference: field(value, "ref").unwrap_or("?").to_string(),
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    entries
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn reads_origin_and_ref() {
        let entries = entries(&object(serde_json::json!({
            "aaa/bb": { "origin": "github", "ref": "v1" }
        })));

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "aaa/bb");
        assert_eq!(entries[0].origin, "github");
        assert_eq!(entries[0].reference, "v1");
    }

    #[test]
    fn keeps_entries_it_cannot_read() {
        let entries = entries(&object(serde_json::json!({
            "aaa/bb": "github",
            "ccc/dd": { "ref": 1 }
        })));

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].origin, "?");
        assert_eq!(entries[0].reference, "?");
        assert_eq!(entries[1].origin, "?");
        assert_eq!(entries[1].reference, "?");
    }

    #[test]
    fn sorts_by_key() {
        let entries = entries(&object(serde_json::json!({
            "ccc/dd": {},
            "aaa/bb": {}
        })));

        let keys: Vec<&str> = entries.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys, ["aaa/bb", "ccc/dd"]);
    }
}
