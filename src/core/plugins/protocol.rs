//! 插件协议层数据结构。
//!
//! 直接映射 byk.json 的 JSON 形状，保持清晰、扁平的嵌套结构。
//! 协议层的目标：好写、好读、好扩展。
//! 不直接用于执行——先转换为 execution 层的 InstallPlan。

use std::collections::HashMap;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// 注册表根对象
// ---------------------------------------------------------------------------

/// byk.json 解析后的完整注册表。
#[allow(dead_code)]
pub struct Registry {
    /// "$default": 默认插件 key
    pub default: Option<String>,
    /// "$var": 全局变量模板
    pub variables: HashMap<String, String>,
    /// 插件列表（过滤掉 $ 开头的 key）
    pub plugins: HashMap<String, PluginDef>,
}

// ---------------------------------------------------------------------------
// 单个插件定义
// ---------------------------------------------------------------------------

/// 下载区块：裸 URL 字符串 或 文件名→URL 映射。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DownloadsSection {
    /// 裸 URL 字符串 → 文件名从 URL 自动提取
    BareUrl(String),
    /// 文件名/目录名 → 下载条目
    Map(HashMap<String, DownloadValue>),
}

/// 单个插件的协议定义。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginDef {
    /// pip 依赖（属于本插件，卸载时会一起删除）
    /// 支持单字符串或字符串数组，单值自动包装为 `Some(vec![...])`
    #[serde(default, deserialize_with = "deserialize_pip_one_or_many")]
    pub pip: Option<Vec<String>>,

    /// pip-keep 依赖（属于本插件，但卸载时保留，不会被 pip uninstall）
    /// 结构与 pip 完全相同，支持单字符串或字符串数组
    #[serde(rename = "pip-keep", default, deserialize_with = "deserialize_pip_one_or_many")]
    pub pip_keep: Option<Vec<String>>,

    /// 下载到 plugins/{plugin_key}/ 的文件
    #[serde(default)]
    pub downloads: Option<DownloadsSection>,

    /// 下载到当前工作目录
    #[serde(rename = "download-to-workdir", default)]
    pub download_to_workdir: Option<DownloadsSection>,

    /// 下载到 ~/.byk/alias/
    #[serde(rename = "download-to-alias", default)]
    pub download_to_alias: Option<DownloadsSection>,

    /// 单个命令注册（命令名 = 插件 key）
    #[serde(default)]
    pub command: Option<CommandDef>,

    /// 多个命令注册
    #[serde(default)]
    pub commands: Option<HashMap<String, CommandDef>>,

    /// 别名部署：安装时写入 *.byk.json 文件
    /// key = "@filename"（当前目录）或 "@@filename"（~/.byk/alias/）
    /// value = 别名 key-value 定义
    #[serde(default)]
    pub alias: Option<HashMap<String, serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// 下载条目（统一支持单文件 URL 和目录树）
// ---------------------------------------------------------------------------

/// 下载条目的值：URL 字符串或目录树。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DownloadValue {
    /// 单文件 URL（支持 `[tar]` / `[exe]` 前缀）
    Url(String),
    /// 目录树
    Tree(HashMap<String, DownloadEntry>),
}

/// 目录树中的条目：叶子（URL）或子目录。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DownloadEntry {
    /// 叶子节点：URL 字符串
    File(String),
    /// 子目录：嵌套的 key-value 映射
    Dir(HashMap<String, DownloadEntry>),
}

// ---------------------------------------------------------------------------
// 命令定义（command / commands 共用）
// ---------------------------------------------------------------------------

/// 命令注册定义。
#[derive(Debug, Clone, Deserialize)]
pub struct CommandDef {
    /// 命令类型："python-m" | "python" | "pip-bin" | "bin"
    #[serde(rename = "type")]
    pub cmd_type: String,

    /// 入口点路径
    pub entry: String,

    /// 描述文本
    #[serde(default)]
    pub desc: String,
}

// ---------------------------------------------------------------------------
// 自定义反序列化器
// ---------------------------------------------------------------------------

/// 反序列化 `pip` 字段：接受单字符串或字符串数组，统一返回 `Option<Vec<String>>`。
/// - 字段不存在 → `None`
/// - 单字符串 `"pkg"` → `Some(vec!["pkg"])`
/// - 数组 `["a", "b"]` → `Some(vec!["a", "b"])`
/// - 空数组 `[]` → `None`
fn deserialize_pip_one_or_many<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(vec![s])),
        serde_json::Value::Array(arr) => {
            let strings: Result<Vec<String>, _> = arr
                .into_iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => Ok(s),
                    _ => Err(serde::de::Error::custom(
                        "expected string in pip array",
                    )),
                })
                .collect();
            strings.map(|v| if v.is_empty() { None } else { Some(v) })
        }
        _ => Err(serde::de::Error::custom(
            "expected a string or array of strings for pip field",
        )),
    }
}

// ---------------------------------------------------------------------------
// 解析入口
// ---------------------------------------------------------------------------

/// 从 preprocess_registry 返回的原始 HashMap 解析为 Registry。
///
/// 变量替换已在 preprocess_registry 阶段完成，此函数只做结构反序列化。
pub fn parse_registry(raw: &HashMap<String, serde_json::Value>) -> Registry {
    let default = raw
        .get("$default")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let variables = raw
        .get("$var")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut plugins = HashMap::new();
    for (key, value) in raw {
        if key.starts_with('$') {
            continue;
        }
        match serde_json::from_value::<PluginDef>(value.clone()) {
            Ok(def) => {
                plugins.insert(key.clone(), def);
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to parse plugin \"{}\": {}",
                    key,
                    e,
                );
            }
        }
    }

    Registry {
        default,
        variables,
        plugins,
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn to_map(json: &str) -> HashMap<String, serde_json::Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parse_minimal_registry() {
        let raw = to_map(r#"{"plugin": {"pip": ["requests"]}}"#);
        let registry = parse_registry(&raw);
        assert_eq!(registry.default, None);
        assert!(registry.variables.is_empty());
        assert_eq!(registry.plugins.len(), 1);
        let def = registry.plugins.get("plugin").unwrap();
        assert_eq!(def.pip.as_ref().unwrap(), &vec!["requests".to_string()]);
    }

    #[test]
    fn parse_pip_single_string() {
        let raw = to_map(r#"{"plugin": {"pip": "requests"}}"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("plugin").unwrap();
        assert_eq!(def.pip.as_ref().unwrap(), &vec!["requests".to_string()]);
    }

    #[test]
    fn parse_pip_empty_array_is_none() {
        let raw = to_map(r#"{"plugin": {"pip": []}}"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("plugin").unwrap();
        assert!(def.pip.is_none());
    }

    #[test]
    fn parse_with_default_and_var() {
        let raw = to_map(r#"{
            "$default": "main",
            "$var": {"url": "https://example.com"},
            "main": {"pip": ["pkg"]}
        }"#);
        let registry = parse_registry(&raw);
        assert_eq!(registry.default, Some("main".to_string()));
        assert_eq!(registry.variables.get("url").unwrap(), "https://example.com");
    }

    #[test]
    fn parse_pip_keep_single_string() {
        let raw = to_map(r#"{
            "p1": {"pip": "p1-lib", "pip-keep": "shared-lib"}
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("p1").unwrap();
        assert_eq!(def.pip.as_ref().unwrap(), &vec!["p1-lib".to_string()]);
        assert_eq!(def.pip_keep.as_ref().unwrap(), &vec!["shared-lib".to_string()]);
    }

    #[test]
    fn parse_pip_keep_array() {
        let raw = to_map(r#"{
            "p1": {"pip": ["p1-lib"], "pip-keep": ["shared-lib"]}
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("p1").unwrap();
        assert_eq!(def.pip.as_ref().unwrap(), &vec!["p1-lib".to_string()]);
        assert_eq!(def.pip_keep.as_ref().unwrap(), &vec!["shared-lib".to_string()]);
    }

    #[test]
    fn parse_downloads_with_tar() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "app.tar.gz": "[tar] https://example.com/app.tar.gz"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        match dl {
            DownloadsSection::Map(m) => {
                match m.get("app.tar.gz").unwrap() {
                    DownloadValue::Url(s) => assert_eq!(s, "[tar] https://example.com/app.tar.gz"),
                    _ => panic!("expected Url"),
                }
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn parse_downloads_bare_url() {
        let raw = to_map(r#"{
            "tool": {
                "downloads": "[exe] https://example.com/tool"
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("tool").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        match dl {
            DownloadsSection::BareUrl(s) => assert_eq!(s, "[exe] https://example.com/tool"),
            _ => panic!("expected BareUrl"),
        }
    }

    #[test]
    fn parse_downloads_with_exe() {
        let raw = to_map(r#"{
            "tool": {
                "downloads": {
                    "mytool": "[exe] https://example.com/tool"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("tool").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        match dl {
            DownloadsSection::Map(m) => {
                match m.get("mytool").unwrap() {
                    DownloadValue::Url(s) => assert_eq!(s, "[exe] https://example.com/tool"),
                    _ => panic!("expected Url"),
                }
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn parse_downloads_plain_url() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "script.py": "https://example.com/script.py"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        match dl {
            DownloadsSection::Map(m) => {
                match m.get("script.py").unwrap() {
                    DownloadValue::Url(s) => assert_eq!(s, "https://example.com/script.py"),
                    _ => panic!("expected Url"),
                }
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn parse_downloads_with_directory_tree() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "mylib": {
                        "main.py": "https://example.com/main.py",
                        "sub": {
                            "util.py": "https://example.com/util.py"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let entries = match dl {
            DownloadsSection::Map(m) => m,
            _ => panic!("expected Map"),
        };
        match entries.get("mylib").unwrap() {
            DownloadValue::Tree(entries) => {
                assert_eq!(entries.len(), 2);
                match entries.get("main.py").unwrap() {
                    DownloadEntry::File(s) => assert_eq!(s, "https://example.com/main.py"),
                    _ => panic!("expected File"),
                }
                match entries.get("sub").unwrap() {
                    DownloadEntry::Dir(sub) => {
                        match sub.get("util.py").unwrap() {
                            DownloadEntry::File(s) => assert_eq!(s, "https://example.com/util.py"),
                            _ => panic!("expected File"),
                        }
                    }
                    _ => panic!("expected Dir"),
                }
            }
            _ => panic!("expected Tree"),
        }
    }

    #[test]
    fn parse_download_to_workdir() {
        let raw = to_map(r#"{
            "app": {
                "download-to-workdir": {
                    "config.json": "https://example.com/config.json"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let wd = def.download_to_workdir.as_ref().unwrap();
        match wd {
            DownloadsSection::Map(m) => {
                match m.get("config.json").unwrap() {
                    DownloadValue::Url(s) => assert_eq!(s, "https://example.com/config.json"),
                    _ => panic!("expected Url"),
                }
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn parse_download_to_workdir_bare_url() {
        let raw = to_map(r#"{
            "app": {
                "download-to-workdir": "https://example.com/file.json"
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let wd = def.download_to_workdir.as_ref().unwrap();
        match wd {
            DownloadsSection::BareUrl(s) => assert_eq!(s, "https://example.com/file.json"),
            _ => panic!("expected BareUrl"),
        }
    }

    #[test]
    fn parse_download_to_workdir_with_tree() {
        let raw = to_map(r#"{
            "app": {
                "download-to-workdir": {
                    "myconfig": {
                        "$name": "configs",
                        "settings.json": "https://example.com/settings.json",
                        "db": {
                            "seed.sql": "https://example.com/seed.sql"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let wd = def.download_to_workdir.as_ref().unwrap();
        match wd {
            DownloadsSection::Map(m) => {
                match m.get("myconfig").unwrap() {
                    DownloadValue::Tree(entries) => {
                        assert_eq!(entries.len(), 3); // $name + settings.json + db
                        match entries.get("$name").unwrap() {
                            DownloadEntry::File(s) => assert_eq!(s, "configs"),
                            _ => panic!("expected $name as File"),
                        }
                    }
                    _ => panic!("expected Tree"),
                }
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn parse_download_to_alias() {
        let raw = to_map(r#"{
            "app": {
                "download-to-alias": {
                    "myalias": "https://example.com/alias.py"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let al = def.download_to_alias.as_ref().unwrap();
        match al {
            DownloadsSection::Map(m) => {
                match m.get("myalias").unwrap() {
                    DownloadValue::Url(s) => assert_eq!(s, "https://example.com/alias.py"),
                    _ => panic!("expected Url"),
                }
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn parse_with_command_and_commands() {
        let raw = to_map(r#"{
            "app": {
                "command": {"type": "bin", "entry": "app", "desc": "main"},
                "commands": {
                    "sub": {"type": "python", "entry": "sub.py", "desc": "sub"}
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        assert!(def.command.is_some());
        assert!(def.commands.is_some());
        let cmd = def.command.as_ref().unwrap();
        assert_eq!(cmd.cmd_type, "bin");
        assert_eq!(cmd.entry, "app");

        let cmds = def.commands.as_ref().unwrap();
        let sub = cmds.get("sub").unwrap();
        assert_eq!(sub.cmd_type, "python");
    }
}