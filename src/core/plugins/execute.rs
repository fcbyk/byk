/// 插件命令执行引擎。
///
/// 支持多种执行类型，通过 `PluginExecutor` trait 实现可扩展架构。
/// 当前内置：py-module、py-script、py-bin。

use std::path::Path;
use std::process::{Command, exit};

use super::state::get_python_executable;
use super::types::{CmdState, VENV_BIN};

// ---------------------------------------------------------------------------
// 执行器 trait
// ---------------------------------------------------------------------------

/// 插件执行器：每种命令类型（py-module / py-script / ...）对应一个实现。
pub trait PluginExecutor {
    /// 返回此执行器支持的命令类型字符串。
    fn cmd_type(&self) -> &'static str;

    /// 执行插件命令。
    ///
    /// - `entry`：入口点（模块路径、脚本文件名或 bin 名）
    /// - `args`：用户传入的额外参数
    /// - `plugins_dir`：plugins 目录路径
    /// - `venv_dir`：venv 根目录路径
    /// - `python_exe`：Python 解释器路径
    fn execute(
        &self,
        entry: &str,
        args: &[String],
        plugins_dir: &Path,
        venv_dir: &Path,
        python_exe: &str,
    ) -> std::process::ExitStatus;
}

// ---------------------------------------------------------------------------
// 内置执行器
// ---------------------------------------------------------------------------

/// Python 模块执行器（`python -m <module> <args>`）。
pub struct PyModuleExecutor;

impl PluginExecutor for PyModuleExecutor {
    fn cmd_type(&self) -> &'static str {
        "py-module"
    }

    fn execute(
        &self,
        entry: &str,
        args: &[String],
        _plugins_dir: &Path,
        _venv_dir: &Path,
        python_exe: &str,
    ) -> std::process::ExitStatus {
        Command::new(python_exe)
            .arg("-m")
            .arg(entry)
            .args(args)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Failed to start Python runtime: {}", e);
                exit(1);
            })
    }
}

/// Python 脚本执行器（`python <script> <args>`）。
pub struct PyScriptExecutor;

impl PluginExecutor for PyScriptExecutor {
    fn cmd_type(&self) -> &'static str {
        "py-script"
    }

    fn execute(
        &self,
        entry: &str,
        args: &[String],
        plugins_dir: &Path,
        _venv_dir: &Path,
        python_exe: &str,
    ) -> std::process::ExitStatus {
        let script_path = plugins_dir.join("scripts").join(entry);
        Command::new(python_exe)
            .arg(script_path)
            .args(args)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Failed to start Python runtime: {}", e);
                exit(1);
            })
    }
}

/// Python bin 执行器（直接执行 venv/bin/ 下的控制台脚本）。
///
/// 适用于通过 pip 安装的 whl 包，其 `[project.scripts]` 声明的
/// 入口点会被 pip 自动生成到 venv/bin/ 目录下。
pub struct PyBinExecutor;

impl PluginExecutor for PyBinExecutor {
    fn cmd_type(&self) -> &'static str {
        "py-bin"
    }

    fn execute(
        &self,
        entry: &str,
        args: &[String],
        _plugins_dir: &Path,
        venv_dir: &Path,
        _python_exe: &str,
    ) -> std::process::ExitStatus {
        let bin_path = venv_dir.join(VENV_BIN).join(entry);
        Command::new(bin_path)
            .args(args)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Failed to start plugin binary: {}", e);
                exit(1);
            })
    }
}

// ---------------------------------------------------------------------------
// 执行器注册表
// ---------------------------------------------------------------------------

/// 返回所有已注册的执行器。
///
/// 新增执行类型时在此函数中追加即可。
fn executors() -> Vec<Box<dyn PluginExecutor>> {
    vec![
        Box::new(PyModuleExecutor),
        Box::new(PyScriptExecutor),
        Box::new(PyBinExecutor),
    ]
}

// ---------------------------------------------------------------------------
// 公开入口
// ---------------------------------------------------------------------------

/// 将插件命令转发给对应的执行器。
///
/// 根据 `cmd_type` 查找匹配的执行器并执行。
/// 若找不到匹配的执行器，fallback 到 py-module。
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

    let executors = executors();
    let executor = executors
        .iter()
        .find(|e| e.cmd_type() == cmd.cmd_type.as_str());

    let status = match executor {
        Some(e) => e.execute(&cmd.entry, cmd_args, plugins_dir, venv_dir, &python_exe),
        None => {
            // fallback: 默认使用 py-module
            PyModuleExecutor.execute(&cmd.entry, cmd_args, plugins_dir, venv_dir, &python_exe)
        }
    };

    exit(status.code().unwrap_or(1));
}