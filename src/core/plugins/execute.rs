//! 插件命令执行引擎。
//!
//! 支持多种执行类型，通过 `PluginExecutor` trait 实现可扩展架构。
//! 当前内置：python-m、python、bin、node。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use colored::Colorize;

use super::state::get_python_executable;
use super::types::CmdState;

// ---------------------------------------------------------------------------
// 执行器 trait
// ---------------------------------------------------------------------------

/// 插件执行器：每种命令类型（python-m / python / ...）对应一个实现。
pub trait PluginExecutor {
    /// 返回此执行器支持的命令类型字符串。
    fn cmd_type(&self) -> &'static str;

    /// 执行插件命令。
    ///
    /// - `cmd_name`：注册的命令名（bin 目录形态下用它作为可执行文件名）
    /// - `entry`：入口点（模块路径、脚本文件名或已解析的绝对路径）
    /// - `args`：用户传入的额外参数
    /// - `plugins_dir`：plugins 目录路径
    /// - `venv_dir`：venv 根目录路径
    /// - `root_dir`：byk 根目录路径（`~/.byk`）
    /// - `python_exe`：Python 解释器路径
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        cmd_name: &str,
        entry: &str,
        args: &[String],
        plugins_dir: &Path,
        venv_dir: &Path,
        root_dir: &Path,
        python_exe: &str,
    ) -> std::process::ExitStatus;
}

// ---------------------------------------------------------------------------
// 内置执行器
// ---------------------------------------------------------------------------

/// Python 模块执行器（`python -m <module> <args>`）。
pub struct PythonMExecutor;

impl PluginExecutor for PythonMExecutor {
    fn cmd_type(&self) -> &'static str {
        "python-m"
    }

    fn execute(
        &self,
        _cmd_name: &str,
        entry: &str,
        args: &[String],
        _plugins_dir: &Path,
        _venv_dir: &Path,
        _root_dir: &Path,
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

/// Python 文件执行器（`python plugins/<entry> <args>`）。
pub struct PythonExecutor;

impl PluginExecutor for PythonExecutor {
    fn cmd_type(&self) -> &'static str {
        "python"
    }

    fn execute(
        &self,
        _cmd_name: &str,
        entry: &str,
        args: &[String],
        plugins_dir: &Path,
        _venv_dir: &Path,
        _root_dir: &Path,
        python_exe: &str,
    ) -> std::process::ExitStatus {
        let script_path = plugins_dir.join(entry);
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

/// 通用二进制执行器（`type: "bin"`）。
///
/// entry 为安装期解析好的绝对路径（`$this` / `$venv` token 已替换），
/// 支持两种形态：
/// - 目录：将该目录前置到子进程 PATH，按命令名查找可执行文件
///   （如 `"$venv"` → venv bin 目录 + 命令名，覆盖原 pip-bin 场景）
/// - 文件：直接执行该文件，并将其所在目录前置到子进程 PATH，
///   使被启动的程序能按名字找到同目录的兄弟可执行文件
pub struct BinExecutor;

impl PluginExecutor for BinExecutor {
    fn cmd_type(&self) -> &'static str {
        "bin"
    }

    fn execute(
        &self,
        cmd_name: &str,
        entry: &str,
        args: &[String],
        _plugins_dir: &Path,
        _venv_dir: &Path,
        _root_dir: &Path,
        _python_exe: &str,
    ) -> std::process::ExitStatus {
        let entry_path = Path::new(entry);

        let (exe_path, bin_dir) = if entry_path.is_dir() {
            // 目录形态：按命令名在目录内查找
            let exe = entry_path.join(cmd_name);
            if !exe.is_file() {
                eprintln!(
                    "{} '{}' not found in bin directory {}",
                    "Error:".red(),
                    cmd_name,
                    entry_path.display()
                );
                exit(1);
            }
            (exe, entry_path.to_path_buf())
        } else {
            // 文件形态：直接执行，前置其所在目录
            if !entry_path.is_file() {
                eprintln!(
                    "{} bin entry not found: {}",
                    "Error:".red(),
                    entry_path.display()
                );
                exit(1);
            }
            let dir = entry_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            (entry_path.to_path_buf(), dir)
        };

        let mut cmd = Command::new(&exe_path);
        cmd.args(args);
        cmd.env("PATH", prepend_path(&bin_dir));
        cmd.status().unwrap_or_else(|e| {
            eprintln!("Failed to start binary: {}", e);
            exit(1);
        })
    }
}

/// Node.js 脚本执行器（`node plugins/<entry> <args>`）。
///
/// 适用于零依赖的单文件 JS 脚本。node 从 PATH 解析，
/// 由用户自行管理，与 byk 的 Python venv 无关。
///
/// 向子进程注入 byk 上下文环境变量（见 `build_node_env`），
/// 使 JS 插件无需硬编码 `~/.byk` 路径。
pub struct NodeExecutor;

impl PluginExecutor for NodeExecutor {
    fn cmd_type(&self) -> &'static str {
        "node"
    }

    fn execute(
        &self,
        _cmd_name: &str,
        entry: &str,
        args: &[String],
        plugins_dir: &Path,
        _venv_dir: &Path,
        root_dir: &Path,
        _python_exe: &str,
    ) -> std::process::ExitStatus {
        let script_path = plugins_dir.join(entry);
        let mut cmd = Command::new("node");
        cmd.arg(script_path);
        cmd.args(args);
        for (key, value) in build_node_env(root_dir, plugins_dir) {
            cmd.env(key, value);
        }
        cmd.status().unwrap_or_else(|e| {
            eprintln!("Failed to start Node.js runtime: {}", e);
            eprintln!("  Make sure `node` is installed and available in PATH.");
            exit(1);
        })
    }
}

/// 构造 node 执行器注入子进程的环境变量（纯函数，便于测试）。
///
/// 让 JS 插件无需硬编码 `~/.byk` 路径：
/// - `BYK_HOME`：byk 根目录（插件在此定位 node-pkgs 等数据目录）
/// - `BYK_PLUGINS_DIR`：plugins 目录（定位 plugins.cmd.json / plugins.pkg.json）
fn build_node_env(root_dir: &Path, plugins_dir: &Path) -> Vec<(String, String)> {
    vec![
        (
            "BYK_HOME".to_string(),
            root_dir.to_string_lossy().into_owned(),
        ),
        (
            "BYK_PLUGINS_DIR".to_string(),
            plugins_dir.to_string_lossy().into_owned(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// PATH 前置（bin 目录注入子进程环境）
// ---------------------------------------------------------------------------

/// 将 `dir` 前置到当前 PATH，返回新的 PATH 值。
///
/// 与别名系统的 `build_alias_env` 保持同一套分隔符处理。
fn prepend_path(dir: &Path) -> OsString {
    let existing = std::env::var_os("PATH").unwrap_or_default();
    build_prepended_path(dir, existing.as_os_str())
}

/// 纯函数：把 `dir` 拼到 `existing` PATH 之前（便于测试）。
fn build_prepended_path(dir: &Path, existing: &OsStr) -> OsString {
    #[cfg(windows)]
    let sep = ";";
    #[cfg(not(windows))]
    let sep = ":";
    let mut result = dir.as_os_str().to_os_string();
    result.push(sep);
    result.push(existing);
    result
}

// ---------------------------------------------------------------------------
// 执行器注册表
// ---------------------------------------------------------------------------

/// 返回所有已注册的执行器。
///
/// 新增执行类型时在此函数中追加即可。
fn executors() -> Vec<Box<dyn PluginExecutor>> {
    vec![
        Box::new(PythonMExecutor),
        Box::new(PythonExecutor),
        Box::new(BinExecutor),
        Box::new(NodeExecutor),
    ]
}

// ---------------------------------------------------------------------------
// 公开入口
// ---------------------------------------------------------------------------

/// 该命令类型是否依赖 Python venv。
///
/// 仅 `bin` / `node` 类型与 Python 生态无关（分别直接执行
/// plugins/ 下的二进制和 node 脚本，node 从 PATH 解析）；
/// 其余类型（含未知类型，会 fallback 到 python-m）都需要解释器。
fn requires_python(cmd_type: &str) -> bool {
    !matches!(cmd_type, "bin" | "node")
}

/// 将插件命令转发给对应的执行器。
///
/// 根据 `cmd_type` 查找匹配的执行器并执行。
/// 若找不到匹配的执行器，fallback 到 python-m。
pub fn execute_plugin_command(
    cmd_name: &str,
    cmd_args: &[String],
    plugins_dir: &Path,
    venv_dir: &Path,
    root_dir: &Path,
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

    // venv 缺失时（如用户手动删除 ~/.byk/py-venv 后），依赖 Python 的
    // 命令类型给出友好提示；`bin` / `node` 类型不依赖 Python 环境，正常放行。
    if requires_python(&cmd.cmd_type) && !Path::new(&python_exe).is_file() {
        eprintln!(
            "{} command '{}' requires the Python environment, but the venv is missing.",
            "Error:".red(),
            cmd_name
        );
        eprintln!("  Reinstall a Python plugin (e.g. {}) to recreate it.", "`byk add <user/repo>`".bold());
        exit(1);
    }

    let executors = executors();
    let executor = executors
        .iter()
        .find(|e| e.cmd_type() == cmd.cmd_type.as_str());

    let status = match executor {
        Some(e) => e.execute(
            cmd_name,
            &cmd.entry,
            cmd_args,
            plugins_dir,
            venv_dir,
            root_dir,
            &python_exe,
        ),
        None => {
            // fallback: 默认使用 python-m
            PythonMExecutor.execute(
                cmd_name,
                &cmd.entry,
                cmd_args,
                plugins_dir,
                venv_dir,
                root_dir,
                &python_exe,
            )
        }
    };

    exit(status.code().unwrap_or(1));
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== cmd_type ====================

    #[test]
    fn python_m_executor_type() {
        assert_eq!(PythonMExecutor.cmd_type(), "python-m");
    }

    #[test]
    fn python_executor_type() {
        assert_eq!(PythonExecutor.cmd_type(), "python");
    }

    #[test]
    fn bin_executor_type() {
        assert_eq!(BinExecutor.cmd_type(), "bin");
    }

    #[test]
    fn node_executor_type() {
        assert_eq!(NodeExecutor.cmd_type(), "node");
    }

    // ==================== executors ====================

    #[test]
    fn executors_has_four_builtin() {
        let exes = executors();
        assert_eq!(exes.len(), 4);
    }

    #[test]
    fn executors_contains_all_types() {
        let exes = executors();
        let types: Vec<&str> = exes.iter().map(|e| e.cmd_type()).collect();
        assert!(types.contains(&"python-m"));
        assert!(types.contains(&"python"));
        assert!(types.contains(&"bin"));
        assert!(types.contains(&"node"));
        // pip-bin 已合并进 bin（$venv 目录形态）
        assert!(!types.contains(&"pip-bin"));
    }

    // ==================== prepend_path ====================

    #[test]
    fn build_prepended_path_puts_dir_first() {
        let result = build_prepended_path(Path::new("/opt/tool/bin"), OsStr::new("/usr/bin:/bin"));
        let s = result.to_string_lossy();
        #[cfg(not(windows))]
        assert_eq!(s, "/opt/tool/bin:/usr/bin:/bin");
        #[cfg(windows)]
        assert_eq!(s, "/opt/tool/bin;/usr/bin:/bin");
    }

    #[test]
    fn build_prepended_path_handles_empty_existing() {
        let result = build_prepended_path(Path::new("/opt/tool/bin"), OsStr::new(""));
        let s = result.to_string_lossy();
        #[cfg(not(windows))]
        assert_eq!(s, "/opt/tool/bin:");
        #[cfg(windows)]
        assert_eq!(s, "/opt/tool/bin;");
    }

    // ==================== requires_python ====================

    #[test]
    fn requires_python_bin_is_independent() {
        // 回归测试：bin 类型与 Python 环境无关，
        // venv 被删除后命令仍可执行（含 $venv 目录/文件形态）
        assert!(!requires_python("bin"));
    }

    #[test]
    fn requires_python_node_is_independent() {
        // node 类型只依赖 PATH 中的 node，与 Python venv 无关
        assert!(!requires_python("node"));
    }

    #[test]
    fn requires_python_python_types() {
        assert!(requires_python("python-m"));
        assert!(requires_python("python"));
    }

    #[test]
    fn requires_python_unknown_type_fallback() {
        // 未知类型会 fallback 到 python-m，视为需要 Python
        assert!(requires_python("unknown-type"));
    }

    // ==================== build_node_env ====================

    fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn build_node_env_contains_home_and_plugins_dir() {
        let env = build_node_env(Path::new("/home/u/.byk"), Path::new("/home/u/.byk/plugins"));
        assert_eq!(env.len(), 2);
        assert_eq!(env_value(&env, "BYK_HOME"), Some("/home/u/.byk"));
        assert_eq!(
            env_value(&env, "BYK_PLUGINS_DIR"),
            Some("/home/u/.byk/plugins")
        );
    }

    #[test]
    fn build_node_env_no_extra_variables() {
        // 只注入约定的两个变量，避免污染插件进程环境
        let env = build_node_env(Path::new("/root/.byk"), Path::new("/root/.byk/plugins"));
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["BYK_HOME", "BYK_PLUGINS_DIR"]);
    }
}