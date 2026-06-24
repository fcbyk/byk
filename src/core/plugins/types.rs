//! 插件系统数据结构。
//!
//! 插件通过 `byk add` 安装，持久化到 plugins/ 目录。
//! - plugins.cmd.json：命令路由（热路径，每次执行读）
//! - plugins.pkg.json：包追踪（冷路径，install/uninstall 时读写）

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 平台常量
// ---------------------------------------------------------------------------

/// venv 内 bin 目录名。
#[cfg(windows)]
pub const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
pub const VENV_BIN: &str = "bin";

/// venv 内 Python 可执行文件名。
#[cfg(windows)]
pub const PYTHON_BIN: &str = "python.exe";
#[cfg(not(windows))]
pub const PYTHON_BIN: &str = "python";

// ---------------------------------------------------------------------------
// 数据结构 — plugins.cmd.json
// ---------------------------------------------------------------------------

/// 单个插件命令的缓存条目。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginCommand {
    /// 命令类型（"py-module" | "py-script" | ...）
    #[serde(rename = "type")]
    pub cmd_type: String,
    /// 入口点（py-module: 模块路径, py-script: 脚本文件名）
    #[serde(rename = "entry")]
    pub entry: String,
    /// 命令描述
    #[serde(rename = "desc")]
    pub desc: String,
}

/// 命令状态（持久化到 plugins/plugins.cmd.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdState {
    /// 已安装插件的命令列表
    pub commands: HashMap<String, PluginCommand>,
    /// Python 解释器路径（venv 内的 python）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python_executable: Option<String>,
}

// ---------------------------------------------------------------------------
// 数据结构 — plugins.pkg.json
// ---------------------------------------------------------------------------

/// 包状态（持久化到 plugins/plugins.pkg.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgState {
    /// 插件 key → 包信息映射
    pub packages: HashMap<String, PkgEntry>,
}

/// 单个插件的包条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgEntry {
    /// 来源仓库：None = 本地安装，Some("user/repo") = 远程仓库
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// pip 安装列表（包名 / URL / 版本约束），卸载插件时自动 pip uninstall
    /// URL 包需使用 "name @ url" 格式才能卸载，纯 URL 静默跳过
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pip: Option<Vec<String>>,
    /// pip 保留列表，安装后不随插件卸载（共享依赖）
    #[serde(default, rename = "pip-keep", skip_serializing_if = "Option::is_none")]
    pub pip_keep: Option<Vec<String>>,
    /// 下载信息（脚本文件等）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<DownloadInfo>,
    /// 该插件注册的命令名列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

/// 下载信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfo {
    /// 脚本文件名列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
}