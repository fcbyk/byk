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

    /// 单个命令注册（命令名 = 插件 key）
    #[serde(default)]
    pub command: Option<CommandDef>,

    /// 多个命令注册
    #[serde(default)]
    pub commands: Option<HashMap<String, CommandDef>>,
}

// ---------------------------------------------------------------------------
// 下载区块
// ---------------------------------------------------------------------------

/// 下载区块：统一下载到 plugins/ 目录的文件。
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadSection {
    /// 下载到 plugins/scripts/
    /// key = 文件名，value = 来源（URL 或相对路径）
    #[serde(default)]
    pub scripts: Option<HashMap<String, String>>,

    /// 下载到 plugins/bin/
    /// key = 文件名/目录名，value = 平台映射
    #[serde(default)]
    pub bin: Option<HashMap<String, BinSource>>,
}

// ---------------------------------------------------------------------------
// 二进制来源
// ---------------------------------------------------------------------------

/// 二进制来源：按平台区分，$tar 标记控制下载后行为。
#[derive(Debug, Clone, Deserialize)]
pub struct BinSource {
    /// "$tar": 标记为 tar.gz/zip，解压而非直接下载。
    /// 默认 false = 直接下载单文件 + chmod +x。
    #[serde(rename = "$tar", default)]
    pub tar: bool,

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
    /// 命令类型："py-module" | "py-script" | "py-bin" | "bin"
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
                            "$tar": true,
                            "darwin-arm64": "https://example.com/app.tar.gz"
                        }
                    }
                }
            }
        }"#);
        let registry = parse_registry(&raw);
        let def = registry.plugins.get("app").unwrap();
        let dl = def.downloads.as_ref().unwrap();
        let bin_src = dl.bin.as_ref().unwrap().get("app").unwrap();
        assert!(bin_src.tar);
        assert_eq!(
            bin_src.urls.get("darwin-arm64").unwrap(),
            "https://example.com/app.tar.gz"
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
        assert!(!bin_src.tar);
    }

    #[test]
    fn parse_with_command_and_commands() {
        let raw = to_map(r#"{
            "app": {
                "command": {"type": "bin", "entry": "app/app", "desc": "main"},
                "commands": {
                    "sub": {"type": "py-script", "entry": "sub.py", "desc": "sub"}
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
        assert_eq!(sub.cmd_type, "py-script");
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
}