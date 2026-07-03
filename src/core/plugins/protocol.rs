//! 插件协议层数据结构。
//!
//! 直接映射 byk.json 的 JSON 形状，保持清晰、扁平的嵌套结构。
//! 协议层的目标：好写、好读、好扩展。
//! 不直接用于执行——先转换为 execution 层的 InstallPlan。

use std::collections::HashMap;

use colored::Colorize;
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
    /// "$pip": 全局共享 pip 依赖（不属于任何插件，卸载时保留）
    pub global_pip: Vec<String>,
    /// 插件列表（过滤掉 $ 开头的 key）
    pub plugins: HashMap<String, PluginDef>,
}

// ---------------------------------------------------------------------------
// 单个插件定义
// ---------------------------------------------------------------------------

/// 单个插件的协议定义。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginDef {
    /// pip 依赖（属于本插件，卸载时会一起删除）
    #[serde(default)]
    pub pip: Option<Vec<String>>,

    /// 下载区块（脚本 + 二进制）
    #[serde(default)]
    pub downloads: Option<DownloadSection>,

    /// 语法糖：扁平写法，等价于 downloads.scripts
    #[serde(rename = "download-scripts", default)]
    pub download_scripts: Option<HashMap<String, String>>,

    /// 语法糖：扁平写法，等价于 downloads.bin
    #[serde(rename = "download-bin", default)]
    pub download_bin: Option<HashMap<String, BinSource>>,

    /// 语法糖：扁平写法，等价于 downloads.workdir
    #[serde(rename = "download-workdir", default)]
    pub download_workdir: Option<WorkdirValue>,

    /// 语法糖：扁平写法，等价于 downloads.alias
    #[serde(rename = "download-alias", default)]
    pub download_alias: Option<HashMap<String, String>>,

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

impl PluginDef {
    /// 将 `download-scripts` / `download-bin` / `download-workdir` / `download-alias` 语法糖合并到 `downloads`。
    ///
    /// 冲突规则：同时定义语法糖和 `downloads.*` 时报错退出。无冲突则合并，互不干扰。
    pub fn normalize(&mut self) {
        if let Some(ds) = self.download_scripts.take() {
            if let Some(ref dl) = self.downloads
                && dl.scripts.is_some()
            {
                eprintln!(
                    "{} 'download-scripts' and 'downloads.scripts' cannot both be defined",
                    "Error:".red(),
                );
                std::process::exit(1);
            }
            self.downloads
                .get_or_insert(DownloadSection {
                    scripts: None,
                    bin: None,
                    workdir: None,
                    alias: None,
                })
                .scripts = Some(ds);
        }

        if let Some(db) = self.download_bin.take() {
            if let Some(ref dl) = self.downloads
                && dl.bin.is_some()
            {
                eprintln!(
                    "{} 'download-bin' and 'downloads.bin' cannot both be defined",
                    "Error:".red(),
                );
                std::process::exit(1);
            }
            self.downloads
                .get_or_insert(DownloadSection {
                    scripts: None,
                    bin: None,
                    workdir: None,
                    alias: None,
                })
                .bin = Some(db);
        }

        if let Some(dw) = self.download_workdir.take() {
            if let Some(ref dl) = self.downloads
                && dl.workdir.is_some()
            {
                eprintln!(
                    "{} 'download-workdir' and 'downloads.workdir' cannot both be defined",
                    "Error:".red(),
                );
                std::process::exit(1);
            }
            self.downloads
                .get_or_insert(DownloadSection {
                    scripts: None,
                    bin: None,
                    workdir: None,
                    alias: None,
                })
                .workdir = Some(dw);
        }

        if let Some(da) = self.download_alias.take() {
            if let Some(ref dl) = self.downloads
                && dl.alias.is_some()
            {
                eprintln!(
                    "{} 'download-alias' and 'downloads.alias' cannot both be defined",
                    "Error:".red(),
                );
                std::process::exit(1);
            }
            self.downloads
                .get_or_insert(DownloadSection {
                    scripts: None,
                    bin: None,
                    workdir: None,
                    alias: None,
                })
                .alias = Some(da);
        }
    }
}

// ---------------------------------------------------------------------------
// 下载区块
// ---------------------------------------------------------------------------

/// 下载区块：统一下载到 plugins/ 目录的文件。
/// URL 字符串支持 `[tar] ` 前缀标记解压行为。
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadSection {
    /// 下载到 plugins/scripts/
    /// key = 文件名，value = 来源（URL 或相对路径，支持 `[tar]` 前缀）
    #[serde(default)]
    pub scripts: Option<HashMap<String, String>>,

    /// 下载到 plugins/bin/
    /// key = 文件名/目录名，value = 平台映射（URL 支持 `[tar]` 前缀）
    #[serde(default)]
    pub bin: Option<HashMap<String, BinSource>>,

    /// 下载到当前工作目录
    /// 字符串 = 单文件，对象 = 目录树（URL 支持 `[tar]` 前缀）
    #[serde(default)]
    pub workdir: Option<WorkdirValue>,

    /// 下载到 ~/.byk/alias/
    /// key = 文件名，value = 来源（URL 或相对路径，支持 `[tar]` 前缀）
    #[serde(default)]
    pub alias: Option<HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// 工作目录下载
// ---------------------------------------------------------------------------

/// 工作目录下载的取值：字符串（单文件）或对象（目录树）。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WorkdirValue {
    /// 单文件 URL 字符串 → 下载到当前工作目录
    Single(String),
    /// 目录树对象 → 下载到当前工作目录 / <$name>/
    Tree(WorkdirTree),
}

/// 工作目录下载的目录树结构。
#[derive(Debug, Clone, Deserialize)]
pub struct WorkdirTree {
    /// 目录名，默认 "downloads"
    #[serde(rename = "$name", default)]
    pub name: Option<String>,

    /// 目录内容（$ 前缀的 key 自动跳过）
    #[serde(flatten)]
    pub entries: HashMap<String, WorkdirEntry>,
}

/// 目录树中的条目：叶子（URL 字符串）或子目录（嵌套对象）。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WorkdirEntry {
    /// 叶子节点：URL 字符串
    File(String),
    /// 子目录：嵌套的 key-value 映射
    Dir(HashMap<String, WorkdirEntry>),
}

// ---------------------------------------------------------------------------
// 二进制来源
// ---------------------------------------------------------------------------

/// 二进制来源：按平台区分，URL 字符串内 `[tar]` 前缀标记解压。
#[derive(Debug, Clone, Deserialize)]
pub struct BinSource {
    /// 平台 → URL 映射，如 "darwin-arm64" → "https://..."
    #[serde(flatten)]
    pub urls: HashMap<String, String>,
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

    let global_pip = raw
        .get("$pip")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
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
        global_pip,
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
        assert!(registry.global_pip.is_empty());
        assert_eq!(registry.plugins.len(), 1);
        let def = registry.plugins.get("plugin").unwrap();
        assert_eq!(def.pip.as_ref().unwrap(), &vec!["requests".to_string()]);
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
    fn parse_global_pip() {
        let raw = to_map(r#"{
            "$pip": ["shared-lib"],
            "p1": {"pip": ["p1-lib"]}
        }"#);
        let registry = parse_registry(&raw);
        assert_eq!(registry.global_pip, vec!["shared-lib".to_string()]);
    }

    #[test]
    fn parse_bin_with_tar() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "bin": {
                        "app": {
                            "darwin-arm64": "[tar] https://example.com/app.tar.gz"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let bin_src = dl.bin.as_ref().unwrap().get("app").unwrap();
        assert_eq!(
            bin_src.urls.get("darwin-arm64").unwrap(),
            "[tar] https://example.com/app.tar.gz"
        );
    }

    #[test]
    fn parse_bin_direct_download() {
        let raw = to_map(r#"{
            "tool": {
                "downloads": {
                    "bin": {
                        "tool": {
                            "darwin-arm64": "https://example.com/tool"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("tool").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let bin_src = dl.bin.as_ref().unwrap().get("tool").unwrap();
        assert_eq!(
            bin_src.urls.get("darwin-arm64").unwrap(),
            "https://example.com/tool"
        );
    }

    #[test]
    fn parse_with_command_and_commands() {
        let raw = to_map(r#"{
            "app": {
                "command": {"type": "bin", "entry": "app/app", "desc": "main"},
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
        assert_eq!(cmd.entry, "app/app");

        let cmds = def.commands.as_ref().unwrap();
        let sub = cmds.get("sub").unwrap();
        assert_eq!(sub.cmd_type, "python");
    }

    #[test]
    fn parse_download_scripts() {
        let raw = to_map(r#"{
            "pys": {
                "downloads": {
                    "scripts": {
                        "hello.py": "plugins/hello.py"
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("pys").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let scripts = dl.scripts.as_ref().unwrap();
        assert_eq!(scripts.get("hello.py").unwrap(), "plugins/hello.py");
    }

    #[test]
    fn parse_download_scripts_sugar() {
        let raw = to_map(r#"{
            "pys": {
                "download-scripts": {
                    "hello.py": "plugins/hello.py"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("pys").unwrap();
        assert!(def.download_scripts.is_some());
        assert!(def.downloads.is_none());
        let scripts = def.download_scripts.as_ref().unwrap();
        assert_eq!(scripts.get("hello.py").unwrap(), "plugins/hello.py");
    }

    #[test]
    fn parse_download_bin_sugar() {
        let raw = to_map(r#"{
            "tool": {
                "download-bin": {
                    "tool": {
                        "darwin-arm64": "[tar] https://example.com/tool.tar.gz"
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("tool").unwrap();
        assert!(def.download_bin.is_some());
        assert!(def.downloads.is_none());
        let bin = def.download_bin.as_ref().unwrap().get("tool").unwrap();
        assert_eq!(
            bin.urls.get("darwin-arm64").unwrap(),
            "[tar] https://example.com/tool.tar.gz"
        );
    }

    #[test]
    fn normalize_download_scripts_sugar() {
        let raw = to_map(r#"{
            "pys": {
                "download-scripts": {
                    "hello.py": "plugins/hello.py"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let mut def = registry.plugins.get("pys").unwrap().clone();
        def.normalize();
        assert!(def.download_scripts.is_none());
        let dl = def.downloads.as_ref().unwrap();
        let scripts = dl.scripts.as_ref().unwrap();
        assert_eq!(scripts.get("hello.py").unwrap(), "plugins/hello.py");
    }

    #[test]
    fn normalize_download_bin_sugar() {
        let raw = to_map(r#"{
            "tool": {
                "download-bin": {
                    "tool": {
                        "darwin-arm64": "https://example.com/tool"
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let mut def = registry.plugins.get("tool").unwrap().clone();
        def.normalize();
        assert!(def.download_bin.is_none());
        let dl = def.downloads.as_ref().unwrap();
        let bin = dl.bin.as_ref().unwrap().get("tool").unwrap();
        assert_eq!(
            bin.urls.get("darwin-arm64").unwrap(),
            "https://example.com/tool"
        );
    }

    #[test]
    fn normalize_mix_sugar_and_downloads() {
        let raw = to_map(r#"{
            "pys": {
                "download-scripts": {
                    "hello.py": "plugins/hello.py"
                },
                "downloads": {
                    "bin": {
                        "tool": {
                            "darwin-arm64": "https://example.com/tool"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let mut def = registry.plugins.get("pys").unwrap().clone();
        def.normalize();
        assert!(def.download_scripts.is_none());
        let dl = def.downloads.as_ref().unwrap();
        assert!(dl.scripts.is_some());
        assert!(dl.bin.is_some());
        assert_eq!(
            dl.scripts.as_ref().unwrap().get("hello.py").unwrap(),
            "plugins/hello.py"
        );
        assert_eq!(
            dl.bin.as_ref().unwrap().get("tool").unwrap().urls.get("darwin-arm64").unwrap(),
            "https://example.com/tool"
        );
    }

    // ==================== workdir ====================

    #[test]
    fn parse_workdir_single_url() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "workdir": "https://example.com/config.json"
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let wv = dl.workdir.as_ref().unwrap();
        match wv {
            WorkdirValue::Single(url) => assert_eq!(url, "https://example.com/config.json"),
            _ => panic!("expected Single"),
        }
    }

    #[test]
    fn parse_workdir_tree_with_name() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "workdir": {
                        "$name": "myconfig",
                        "README.md": "https://example.com/readme",
                        "db": {
                            "seed.sql": "https://example.com/seed.sql"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let wv = dl.workdir.as_ref().unwrap();
        match wv {
            WorkdirValue::Tree(tree) => {
                assert_eq!(tree.name.as_deref(), Some("myconfig"));
                assert_eq!(tree.entries.len(), 2);
                match tree.entries.get("README.md").unwrap() {
                    WorkdirEntry::File(url) => assert_eq!(url, "https://example.com/readme"),
                    _ => panic!("expected File"),
                }
                match tree.entries.get("db").unwrap() {
                    WorkdirEntry::Dir(sub) => {
                        match sub.get("seed.sql").unwrap() {
                            WorkdirEntry::File(url) => assert_eq!(url, "https://example.com/seed.sql"),
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
    fn parse_workdir_tree_default_name() {
        let raw = to_map(r#"{
            "app": {
                "downloads": {
                    "workdir": {
                        "hello.txt": "https://example.com/hello.txt"
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let wv = dl.workdir.as_ref().unwrap();
        match wv {
            WorkdirValue::Tree(tree) => {
                assert_eq!(tree.name, None);
            }
            _ => panic!("expected Tree"),
        }
    }

    #[test]
    fn parse_download_workdir_sugar() {
        let raw = to_map(r#"{
            "app": {
                "download-workdir": "https://example.com/file.txt"
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        assert!(def.download_workdir.is_some());
        assert!(def.downloads.is_none());
        match def.download_workdir.as_ref().unwrap() {
            WorkdirValue::Single(url) => assert_eq!(url, "https://example.com/file.txt"),
            _ => panic!("expected Single"),
        }
    }

    #[test]
    fn normalize_download_workdir_sugar() {
        let raw = to_map(r#"{
            "app": {
                "download-workdir": "https://example.com/file.txt"
            }
        }"#);
        let registry = parse_registry(&raw);
        let mut def = registry.plugins.get("app").unwrap().clone();
        def.normalize();
        assert!(def.download_workdir.is_none());
        let dl = def.downloads.as_ref().unwrap();
        let wv = dl.workdir.as_ref().unwrap();
        match wv {
            WorkdirValue::Single(url) => assert_eq!(url, "https://example.com/file.txt"),
            _ => panic!("expected Single"),
        }
    }
}