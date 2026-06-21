/// 插件状态管理。
///
/// 插件通过 `byk add` 安装，持久化到 plugins/pip.json。
/// 执行时直接调用 `python -m <模块>` 透传参数。

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
// 数据结构
// ---------------------------------------------------------------------------

/// 单个插件命令的缓存条目。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginCommand {
    /// 目标路径（py-m: 模块路径, py-f: 脚本路径, py-b: 二进制名）
    pub module: String,
    /// 命令描述
    pub description: String,
    /// 行为类型（"py-m" | "py-f" | "py-b" | ...）
    #[serde(default)]
    pub behavior: Option<String>,
}

/// 单个插件的包信息（install 时写入）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageInfo {
    /// pip 包名（来自 byk.json 的 py-m.pip）
    pub name: String,
    /// 该插件注册的命令名列表
    pub commands: Vec<String>,
    /// 来源仓库：None = 本地安装（--file / -e），Some("user/repo") = 远程仓库
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// 安装行为类型（如 "py-m"、"py-f"，未来扩展 js-* 等）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<String>,
}

/// 插件状态（持久化到 plugins/pip.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    /// 已安装插件的命令列表
    pub commands: HashMap<String, PluginCommand>,
    /// Python 解释器路径（venv 内的 python）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python_executable: Option<String>,
    /// 插件 key → 包信息映射
    #[serde(default)]
    pub packages: HashMap<String, PackageInfo>,
}

// ---------------------------------------------------------------------------
// 空状态
// ---------------------------------------------------------------------------

/// 构造空插件状态（venv 不存在时使用）。
pub fn empty_plugin_state() -> PluginState {
    PluginState {
        commands: HashMap::new(),
        python_executable: None,
        packages: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Python 解释器路径
// ---------------------------------------------------------------------------

/// 获取 Python 解释器路径。
///
/// 优先级：
/// 1. 状态文件（pip.json）中的 `python_executable`
/// 2. 如果 venv 存在 → `venv/bin/python`
pub(crate) fn get_python_executable(plugins_dir: &Path, venv_dir: &Path) -> String {
    let state_file = plugins_dir.join("pip.json");
    if let Some(data) = json_io::read_json::<PluginState>(&state_file) {
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

/// 读取插件状态（从 pip.json 直接读取，不做扫描或过期检测）。
///
/// - venv 不存在 → 返回空状态
/// - 无状态文件 → 返回空状态
/// - 有状态文件 → 直接返回
pub fn load_plugin_state(plugins_dir: &Path, venv_dir: &Path) -> PluginState {
    if !venv_dir.is_dir() {
        return empty_plugin_state();
    }

    let state_file = plugins_dir.join("pip.json");
    json_io::read_json(&state_file).unwrap_or_else(empty_plugin_state)
}

// ---------------------------------------------------------------------------
// 命令执行
// ---------------------------------------------------------------------------

/// 将插件命令转发给 Python 执行。
///
/// py-m 行为通过 `python -m <module> <args>` 调用。
pub fn execute_plugin_command(
    cmd_name: &str,
    cmd_args: &[String],
    plugins_dir: &Path,
    venv_dir: &Path,
    plugin_state: &PluginState,
) {
    let python_exe = get_python_executable(plugins_dir, venv_dir);

    let module = match plugin_state.commands.get(cmd_name) {
        Some(cmd) => &cmd.module,
        None => {
            eprintln!(
                "Internal error: command '{}' not found in plugin cache",
                cmd_name
            );
            exit(1);
        }
    };

    let status = Command::new(&python_exe)
        .arg("-m")
        .arg(module)
        .args(cmd_args)
        .status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to start Python runtime: {}", e);
            exit(1);
        }
    }
}