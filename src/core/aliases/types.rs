/// 别名模块数据结构。
///
/// 定义别名扫描、合并、解析全流程使用的核心类型。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// 别名值类型（叶子节点）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AliasValue {
    Str(String),
    /// 对象格式：{ "$cmd": "...", "$cwd": "...", "$interactive": true }
    Meta {
        #[serde(rename = "$cmd")]
        cmd: String,
        #[serde(rename = "$cwd")]
        #[serde(default)]
        cwd: Option<String>,
        #[serde(rename = "$interactive")]
        #[serde(default)]
        interactive: Option<bool>,
    },
}

/// 别名定义（解析后的可执行形式）。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AliasDefinition {
    pub command: String,
    pub cwd: Option<String>,
    pub interactive: bool,
}

/// 单个别名文件的数据结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasFile {
    /// 文件 key: "@release" | "@@release" | "@" | "@@"
    pub key: String,
    /// 优先级
    pub priority: i32,
    /// 过滤后的原始 JSON（已去除 $priority/$cwd/$interactive/$paths，已过滤非法 key）
    pub aliases: serde_json::Map<String, serde_json::Value>,
    /// 配置文件完整路径
    pub path: PathBuf,
    /// 文件级 $cwd，所有子别名自动继承（除非自行指定）
    pub inherited_cwd: Option<String>,
    /// 文件级 $interactive，所有子别名自动继承（除非自行指定）
    pub inherited_interactive: Option<bool>,
    /// 文件级 $paths，需要前置到 PATH 环境变量的目录列表
    pub inherited_paths: Vec<String>,
}

/// 合并后叶子节点，包含来源信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ResolvedAlias {
    pub value: AliasValue,
    /// 来源文件 key，如 "@release"
    pub source: String,
    /// 来源配置文件所在目录（用于解析别名中的相对路径）
    pub source_path: Option<PathBuf>,
    /// 来源文件的 $paths，需要前置到 PATH 环境变量的目录列表
    #[serde(default)]
    pub paths: Vec<String>,
}

/// 合并配置树节点。每个节点可以同时拥有别名和子节点。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergedNode {
    pub alias: Option<ResolvedAlias>,
    pub children: HashMap<String, MergedNode>,
}

/// 合并后的别名配置树（顶层为 key → MergedNode 映射）。
pub type MergedConfig = HashMap<String, MergedNode>;
