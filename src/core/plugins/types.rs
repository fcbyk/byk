//! 插件系统数据结构。
//!
//! 三层架构：
//! - protocol 层：映射 byk.json 的 JSON 形状（见 protocol.rs）
//! - execution 层：InstallPlan / FileOp / BinOp 等，扁平无歧义
//! - state 层：CmdState / PkgState，持久化到 plugins/ 目录
//!
//! 持久化文件：
//! - plugins.cmd.json：命令路由（热路径，每次执行读）
//! - plugins.pkg.json：包追踪（冷路径，install/uninstall 时读写）

use std::collections::HashMap;
use std::path::PathBuf;

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
// 第三层：State — plugins.cmd.json
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
// 第三层：State — plugins.pkg.json
// ---------------------------------------------------------------------------

/// 包状态（持久化到 plugins/plugins.pkg.json）。
/// 插件 key → 包信息映射。
pub type PkgState = HashMap<String, PkgEntry>;

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
    /// 脚本文件名列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
    /// 二进制文件名列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<String>,
    /// bin-tar 解压出的所有文件/目录列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins_tar: Vec<String>,
    /// 工作目录下载的文件/目录名列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workdir: Vec<String>,
    /// 该插件注册的命令名列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

// ---------------------------------------------------------------------------
// 第二层：Execution — 安装计划（扁平、无歧义）
// ---------------------------------------------------------------------------

/// 安装计划：从协议层转换而来，供执行器消费。
pub struct InstallPlan {
    /// 全局共享 pip 包（先安装，永不随插件卸载）
    pub global_pip: Vec<String>,
    /// 当前插件的执行计划
    pub plugin: ResolvedPlugin,
}

/// 已解析的单插件执行计划。
#[allow(dead_code)]
pub struct ResolvedPlugin {
    /// 插件 key
    pub key: String,
    /// 来源标签（如 "user/repo"），None = 本地
    pub source: Option<String>,
    /// pip 安装列表（属于本插件，卸载时删除）
    pub pip_packages: Vec<String>,
    /// 脚本文件操作列表
    pub scripts: Vec<FileOp>,
    /// 二进制操作列表（已按 tar 字段分流）
    pub bins: Vec<BinOp>,
    /// 工作目录下载操作列表
    pub workdir: Vec<WorkdirOp>,
    /// 命令注册列表（command 和 commands 已合并）
    pub commands: Vec<CommandReg>,
}

// ---------------------------------------------------------------------------
// 文件 / 二进制操作
// ---------------------------------------------------------------------------

/// 文件操作：脚本或直下二进制共用。
pub struct FileOp {
    /// 目标文件名
    pub filename: String,
    /// 已解析的来源（变量替换 + URL/路径判断已完成）
    pub src: ResolvedSrc,
}

/// 二进制操作：在转换时已按 $tar 标记区分为两类。
pub enum BinOp {
    /// 直接下载二进制文件，chmod +x
    Download {
        filename: String,
        src: ResolvedSrc,
    },
    /// 下载 tar.gz/zip，解压到 plugins/bin/<dir_name>/
    Extract {
        dir_name: String,
        src: ResolvedSrc,
    },
}

/// 已解析的资源来源（变量替换 / ref 路径解析已完成）。
pub enum ResolvedSrc {
    Url(String),
    LocalPath(PathBuf),
}

// ---------------------------------------------------------------------------
// 工作目录下载操作
// ---------------------------------------------------------------------------

/// 工作目录下载操作：下载到当前 CLI 运行目录。
pub enum WorkdirOp {
    /// 单文件：下载到当前工作目录，文件名从 URL 提取
    SingleFile {
        src: ResolvedSrc,
    },
    /// 目录树：下载到当前工作目录 / <dir_name>/
    Tree {
        dir_name: String,
        files: Vec<WorkdirFile>,
    },
}

/// 目录树中的单个文件。
pub struct WorkdirFile {
    /// 相对于 dir_name 的路径
    pub rel_path: String,
    /// 下载来源
    pub src: ResolvedSrc,
}

// ---------------------------------------------------------------------------
// 命令注册
// ---------------------------------------------------------------------------

/// 待注册的命令（command 和 commands 合并后的统一形式）。
pub struct CommandReg {
    pub name: String,
    pub cmd_type: String,
    pub entry: String,
    pub desc: String,
}