/// 插件状态加载与持久化。
///
/// 从 plugins.cmd.json 和 plugins.pkg.json 读写插件状态。

use std::collections::HashMap;
use std::path::Path;

use super::types::*;
use crate::utils::json_io;

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