//! 别名文件的结构化数据类型。
//!
//! 扫描、合并、解析与执行共用这些类型。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// 执行方式，三态互斥。
///
/// 交互要求说话（模板 / 占位符提示 / Enter 确认），静默要求闭嘴，两者天然互斥。
/// 做成一个属性而不是两个布尔，是为了让「交互 + 静默」这种无意义组合不可表达，
/// 也避免「文件级交互 + 别名级静默」时用户还得额外写一个 false 才能生效。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// 普通：打印 byk 自己的提示行，然后直接执行
    #[default]
    Normal,
    /// 交互：逐项提示占位符，确认后执行（照常打印提示行）
    Interactive,
    /// 静默：不打印 byk 自己的提示行，stdout 只剩子进程输出
    Silent,
}

impl Mode {
    /// 从 JSON 值解析：只认 `"normal"` / `"interactive"` / `"silent"`，
    /// 其余一律回落 `Normal`。非法值由扫描阶段另行告警，这里保持完备——
    /// 免得一个拼写错误把整个别名从命名空间里弄消失。
    pub fn from_json(raw: &serde_json::Value) -> Mode {
        match raw.as_str() {
            Some("interactive") => Mode::Interactive,
            Some("silent") => Mode::Silent,
            _ => Mode::Normal,
        }
    }

    /// 序列化用的名字，与配置文件里写的一致。
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Interactive => "interactive",
            Mode::Silent => "silent",
        }
    }
}

impl<'de> Deserialize<'de> for Mode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(deserializer)?;
        Ok(Mode::from_json(&raw))
    }
}

/// 别名值类型（叶子节点）。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AliasValue {
    Str(String),
    /// 对象格式：{ "$cmd": "...", "$cwd": "...", "$mode": "silent", "$description": "...", "$recursive": true }
    Meta {
        #[serde(rename = "$cmd")]
        cmd: String,
        #[serde(rename = "$cwd")]
        #[serde(default)]
        cwd: Option<String>,
        #[serde(rename = "$mode")]
        #[serde(default)]
        mode: Option<Mode>,
        #[serde(rename = "$description")]
        #[serde(default)]
        description: Option<String>,
        /// 允许该别名在调用链上重复进入，用于刻意自调用（如 `FOO=1 byk build`）
        #[serde(rename = "$recursive")]
        #[serde(default, deserialize_with = "deserialize_recursive")]
        recursive: Option<bool>,
    },
}

/// `$recursive` 的宽容解析：布尔为准，`"true"` / `1` 之类也认，其余按假处理。
///
/// 与 `$mode` 同一套思路——字段写错不该让整条别名从命名空间里静默消失，
/// 那比把 `$recursive` 当成 `false` 难查得多。
fn deserialize_recursive<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = serde_json::Value::deserialize(deserializer)?;
    Ok(Some(match raw {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::String(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => false,
    }))
}

/// 别名定义（解析后的可执行形式）。
#[derive(Debug, Clone)]
pub struct AliasDefinition {
    pub command: String,
    pub cwd: Option<String>,
    /// 执行方式，默认 `Mode::Normal`
    pub mode: Mode,
    pub description: Option<String>,
    /// 是否放行调用链上的重复进入（`$recursive`）
    pub recursive: bool,
}

/// 单个别名文件的数据结构。
#[derive(Debug, Clone)]
pub struct AliasFile {
    /// 文件 key: "@release" | "@@release" | "@" | "@@"
    pub key: String,
    pub priority: i32,
    /// 过滤后的原始 JSON（已去除 $priority/$cwd/$mode/$private/$paths/$var，已过滤非法 key）
    pub aliases: serde_json::Map<String, serde_json::Value>,
    /// 文件路径
    pub path: PathBuf,
    /// 文件级 $cwd，所有子别名自动继承（除非自行指定）
    pub inherited_cwd: Option<String>,
    /// 文件级 $mode，所有子别名自动继承（除非自行指定）
    pub inherited_mode: Option<Mode>,
    /// 文件级 $private：为真时不进入合并空间，只能精确调用
    pub inherited_private: bool,
    /// 文件级 $paths，需要前置到 PATH 环境变量的目录列表
    pub inherited_paths: Vec<String>,
}

/// 合并后叶子节点，包含来源信息。
#[derive(Debug, Clone)]
pub struct ResolvedAlias {
    pub value: AliasValue,
    /// 来源文件 key，如 "@release"
    pub source: String,
    /// 来源配置文件路径（文件本身，非目录）
    pub source_path: Option<PathBuf>,
    /// 来源文件的 $paths，需要前置到 PATH 环境变量的目录列表
    pub paths: Vec<String>,
}

/// 合并配置树节点。每个节点可以同时拥有别名和子节点。
#[derive(Debug, Clone, Default)]
pub struct MergedNode {
    pub alias: Option<ResolvedAlias>,
    pub children: HashMap<String, MergedNode>,
}

/// 合并后的别名配置树（顶层为 key → MergedNode 映射）。
pub type MergedConfig = HashMap<String, MergedNode>;
