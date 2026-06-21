/// 插件状态管理。
///
/// 插件通过 `byk add` 安装，持久化到 plugins/ 目录。
/// - plugins.cmd.json：命令路由（热路径，每次执行读）
/// - plugins.pkg.json：包追踪（冷路径，install/uninstall 时读写）

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, exit};

use serde::{Deserialize, Serialize};

use crate::utils::json_io;

// ---------------------------------------------------------------------------
// 平台常量
// ---------------------------------------------------------------------------

/// venv 内 bin 目录名。
#[cfg(windows)]
const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
const VENV_BIN: &str = "bin";

/// venv 内 Python 可执行文件名。
#[cfg(windows)]
const PYTHON_BIN: &str = "python.exe";
#[cfg(not(windows))]
const PYTHON_BIN: &str = "python";

// ---------------------------------------------------------------------------
// 数据结构 — plugins.cmd.json
// ---------------------------------------------------------------------------

/// 单个插件命令的缓存条目。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginCommand {
    /// 行为类型（"py-m" | "py-f" | ...）
    pub behavior: String,
    /// 行为操作对象（py-m: 模块路径, py-f: 脚本文件名）
    pub target: String,
    /// 命令描述
    pub description: String,
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
    /// py-m 行为信息
    #[serde(rename = "py-m", default, skip_serializing_if = "Option::is_none")]
    pub py_m: Option<PyMInfo>,
    /// py-f 行为信息
    #[serde(rename = "py-f", default, skip_serializing_if = "Option::is_none")]
    pub py_f: Option<PyFInfo>,
}

/// py-m 行为信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyMInfo {
    /// pip 包名
    pub name: String,
    /// 该行为注册的命令名列表
    pub commands: Vec<String>,
}

/// py-f 行为信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyFInfo {
    /// 脚本文件名列表
    pub scripts: Vec<String>,
    /// 该行为注册的命令名列表
    pub commands: Vec<String>,
    /// pip 依赖列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

// ---------------------------------------------------------------------------
// 空状态
// ---------------------------------------------------------------------------

/// 构造空命令状态。
pub fn empty_cmd_state() -> CmdState {
    CmdState {
        commands: HashMap::new(),
        python_executable: None,
    }
}

/// 构造空包状态。
pub fn empty_pkg_state() -> PkgState {
    PkgState {
        packages: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Python 解释器路径
// ---------------------------------------------------------------------------

/// 获取 Python 解释器路径。
///
/// 优先级：
/// 1. plugins.cmd.json 中的 `python_executable`
/// 2. 如果 venv 存在 → `venv/bin/python`
pub(crate) fn get_python_executable(plugins_dir: &Path, venv_dir: &Path) -> String {
    let cmd_file = plugins_dir.join("plugins.cmd.json");
    if let Some(data) = json_io::read_json::<CmdState>(&cmd_file) {
        if let Some(exe) = data.python_executable {
            return exe;
        }
    }

    let venv_python = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    venv_python.to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// 状态加载
// ---------------------------------------------------------------------------

/// 读取命令状态（从 plugins.cmd.json）。
///
/// - venv 不存在 → 返回空状态
/// - 无状态文件 → 返回空状态
/// - 有状态文件 → 直接返回
pub fn load_plugin_state(plugins_dir: &Path, venv_dir: &Path) -> CmdState {
    if !venv_dir.is_dir() {
        return empty_cmd_state();
    }

    let cmd_file = plugins_dir.join("plugins.cmd.json");
    json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state)
}

/// 读取包状态（从 plugins.pkg.json）。
pub fn load_pkg_state(plugins_dir: &Path) -> PkgState {
    let pkg_file = plugins_dir.join("plugins.pkg.json");
    json_io::read_json(&pkg_file).unwrap_or_else(empty_pkg_state)
}

// ---------------------------------------------------------------------------
// 命令执行
// ---------------------------------------------------------------------------

/// 将插件命令转发给 Python 执行。
///
/// - py-m：`python -m <target> <args>`
/// - py-f：`python <scripts_dir>/<target> <args>`
pub fn execute_plugin_command(
    cmd_name: &str,
    cmd_args: &[String],
    plugins_dir: &Path,
    venv_dir: &Path,
    cmd_state: &CmdState,
) {
    let python_exe = get_python_executable(plugins_dir, venv_dir);

    let cmd = match cmd_state.commands.get(cmd_name) {
        Some(c) => c,
        None => {
            eprintln!(
                "Internal error: command '{}' not found in plugin cache",
                cmd_name
            );
            exit(1);
        }
    };

    let status = match cmd.behavior.as_str() {
        "py-f" => {
            let script_path = plugins_dir.join("scripts").join(&cmd.target);
            Command::new(&python_exe)
                .arg(script_path)
                .args(cmd_args)
                .status()
        }
        _ => {
            // py-m（默认）
            Command::new(&python_exe)
                .arg("-m")
                .arg(&cmd.target)
                .args(cmd_args)
                .status()
        }
    };

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to start Python runtime: {}", e);
            exit(1);
        }
    }
}