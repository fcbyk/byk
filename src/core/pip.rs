//! venv / pip 共享原语。
//!
//! 由 `$pip` / `$pip-keep` 安装（`byk add pip` / `byk add <file>` / 远程分发）复用。

use std::io::{self, Write};
use std::process::{Command, exit};

use colored::Colorize;

use super::download::ensure_dir;
use super::paths::PathLayout;

// ---------------------------------------------------------------------------
// venv 平台常量
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

/// 是否 uv 管理模式（py-venv/pyproject.toml 存在）。
fn is_uv_mode(layout: &PathLayout) -> bool {
    layout.py_venv_dir.join("pyproject.toml").is_file()
}

/// 安装 Python 包（自动检测 py-v / uv 模式）。
pub(crate) fn install_python_package(pkg: &str, layout: &PathLayout) {
    if is_uv_mode(layout) {
        if !tool_exists("uv") {
            eprintln!(
                "{} this environment is managed by uv, but uv was not found.",
                "Error:".red(),
            );
            eprintln!(
                "  Reinstall uv, or delete ~/.byk/py-venv/ to let byk rebuild it with {}.",
                default_python_name()
            );
            exit(1);
        }
        let args = vec!["add", pkg];
        let status = Command::new("uv")
            .args(&args)
            .current_dir(&layout.py_venv_dir)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!(
                    "{} uv add failed with exit code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1),
                );
                exit(1);
            }
            Err(e) => {
                eprintln!("{} Failed to run uv: {}", "Error:".red(), e);
                exit(1);
            }
        }
    } else {
        let pip = layout.venv_dir.join(VENV_BIN).join("pip");
        let args = vec!["install", pkg];
        let status = Command::new(&pip).args(&args).status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!(
                    "{} pip install failed with exit code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1),
                );
                exit(1);
            }
            Err(e) => {
                eprintln!("{} Failed to run pip: {}", "Error:".red(), e);
                exit(1);
            }
        }
    }
}

/// 安装一批 pip 包到 byk 托管 venv（按需创建，含一次性确认）。
///
/// `byk add pip` 与插件安装（`byk add <file>` / 远程）共用。
pub(crate) fn install_packages(packages: &[String], layout: &PathLayout, subject: &str) {
    ensure_python_env(layout, subject);
    for pkg in packages {
        println!(
            "{}",
            format!("Installing pip package: {}", pkg).dimmed()
        );
        install_python_package(pkg, layout);
        println!("{} {}", "+".green(), pkg.bold());
    }
}

// ---------------------------------------------------------------------------
// Python venv 初始化
// ---------------------------------------------------------------------------

/// venv 创建工具。
///
/// 优先级：uv 优于系统 Python——uv 不依赖系统 Python（可自行管理
/// 解释器），venv 创建和包安装也都更快。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PyTool {
    /// uv（`uv venv` + `uv init`）
    Uv,
    /// 系统 Python（具体的解释器命令，如 python3 / python / py）
    Python(&'static str),
}

impl PyTool {
    /// 提示文案中展示的工具名。
    fn label(&self) -> &'static str {
        match self {
            PyTool::Uv => "uv",
            PyTool::Python(exe) => exe,
        }
    }
}

/// 根据探测结果选择 venv 创建工具（纯函数，便于测试）。
fn choose_py_tool(has_uv: bool, has_python: bool) -> Option<PyTool> {
    if has_uv {
        Some(PyTool::Uv)
    } else if has_python {
        Some(PyTool::Python(default_python_name()))
    } else {
        None
    }
}

/// 探测 PATH 中可用的 venv 创建工具。
fn detect_py_tool() -> Option<PyTool> {
    let has_python = python_candidates().iter().any(|name| tool_exists(name));
    choose_py_tool(tool_exists("uv"), has_python)
}

/// 系统 Python 的候选可执行名（按优先级）。
fn python_candidates() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec!["python", "py"]
    }
    #[cfg(not(windows))]
    {
        vec!["python3", "python"]
    }
}

/// 默认系统 Python 可执行名（提示文案与 venv 创建共用）。
fn default_python_name() -> &'static str {
    python_candidates()[0]
}

/// 无 Python 环境时提示文案中的解释器名。
fn python_hint() -> &'static str {
    default_python_name()
}

/// 探测可执行文件是否在 PATH 中且可运行。
fn tool_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// 按平台打印 Python / uv 的安装指引。
fn print_python_install_hints() {
    #[cfg(target_os = "macos")]
    {
        eprintln!("  {}", "brew install uv                 # recommended".dimmed());
        eprintln!("  {}", "brew install python".dimmed());
    }
    #[cfg(target_os = "linux")]
    {
        eprintln!(
            "  {}",
            "curl -LsSf https://astral.sh/uv/install.sh | sh".dimmed()
        );
        eprintln!(
            "  {}",
            "sudo apt install python3-venv    # Debian/Ubuntu".dimmed()
        );
    }
    #[cfg(target_os = "windows")]
    {
        eprintln!("  {}", "winget install astral-sh.uv".dimmed());
        eprintln!("  {}", "winget install Python.Python.3.13".dimmed());
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        eprintln!("  {}", "https://docs.astral.sh/uv/".dimmed());
        eprintln!("  {}", "https://www.python.org/downloads/".dimmed());
    }
}

/// venv 是否已就绪（pip 或 python 可执行文件存在即可；
/// uv 创建的 venv 不含 pip，因此二者满足其一）。
fn venv_is_ready(layout: &PathLayout) -> bool {
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
    let py_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    pip.is_file() || py_exe.is_file()
}

/// 确保 Python venv 可用（不存在则探测工具、一次性确认、创建）。
///
/// venv 已就绪时直接返回。`subject` 为触发原因的描述，
/// 如 `plugin "hello"` / `byk add pip`。
///
/// 用户不需要理解“虚拟环境管理器选型”这个概念——
/// 探测到 uv 就用 uv（快、不依赖系统 Python），否则用系统 Python。
pub(crate) fn ensure_python_env(layout: &PathLayout, subject: &str) {
    if venv_is_ready(layout) {
        return;
    }
    match detect_py_tool() {
        Some(tool) => {
            println!();
            println!(
                "{} {} needs a Python virtual environment.",
                "!".yellow(),
                subject.bold()
            );
            println!(
                "  byk will create {} using {} and install its dependencies.",
                "~/.byk/py-venv".dimmed(),
                tool.label().bold()
            );
            println!();
            print!("Continue? [Y/n] ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                eprintln!();
                eprintln!(
                    "{} cancelled: cannot read confirmation from stdin.",
                    "Error:".red()
                );
                exit(1);
            }
            let answer = input.trim().to_lowercase();
            if answer == "n" || answer == "no" {
                println!("{}", "Cancelled.".dimmed());
                exit(1);
            }
            init_py(layout, tool);
            if !venv_is_ready(layout) {
                exit(1);
            }
        }
        None => {
            eprintln!(
                "{} {} needs a Python environment, but neither {} nor {} was found.",
                "Error:".red(),
                subject,
                "uv".bold(),
                python_hint().bold(),
            );
            eprintln!();
            eprintln!("Install one of them, then retry:");
            print_python_install_hints();
            exit(1);
        }
    }
}

/// 初始化 Python 虚拟环境。
///
/// 创建 ~/.byk/py-venv/.venv/（不存在时）；uv 模式额外创建 pyproject.toml。
fn init_py(layout: &PathLayout, tool: PyTool) {
    let venv_dir = &layout.venv_dir;
    let py_venv_dir = &layout.py_venv_dir;
    let pyproject_toml = py_venv_dir.join("pyproject.toml");

    // ensure common dirs
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(&layout.alias_dir, "alias");
    ensure_dir(&layout.cache_dir, "cache");
    ensure_dir(&layout.plugins_dir, "plugins");

    // ① 创建 venv（不存在时）
    if venv_dir.exists() {
        println!("{}", "venv already exists, skipping creation.".dimmed());
    } else {
        println!("{}", "Creating Python virtual environment...".dimmed());
        let status = match tool {
            PyTool::Uv => Command::new("uv")
                .args(["venv", &venv_dir.to_string_lossy()])
                .status(),
            PyTool::Python(exe) => Command::new(exe)
                .args(["-m", "venv", &venv_dir.to_string_lossy()])
                .status(),
        };
        match status {
            Ok(s) if s.success() => {
                println!("  {} venv/ {}", "+".green(), "(created)".dimmed());
            }
            Ok(s) => {
                eprintln!(
                    "{} venv creation failed with code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1)
                );
                #[cfg(target_os = "linux")]
                if matches!(tool, PyTool::Python(_)) {
                    eprintln!(
                        "  {}",
                        "hint: Debian/Ubuntu may need: sudo apt install python3-venv".dimmed()
                    );
                }
                return;
            }
            Err(e) => {
                eprintln!("{} Failed to create venv: {}", "Error:".red(), e);
                return;
            }
        }
    }

    // ② uv 模式：创建 pyproject.toml
    if matches!(tool, PyTool::Uv) && !pyproject_toml.is_file() {
        println!("{}", "Initializing uv project...".dimmed());
        let status = Command::new("uv")
            .args(["init", "--name", "byk", "--no-readme", "--no-pin-python"])
            .current_dir(py_venv_dir)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("  {} pyproject.toml {}", "+".green(), "(created)".dimmed());
            }
            Ok(s) => {
                eprintln!(
                    "{} uv init failed with code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1)
                );
                return;
            }
            Err(e) => {
                eprintln!("{} Failed to run uv init: {}", "Error:".red(), e);
                return;
            }
        }
    }

    println!();
    println!(
        "{} {}",
        "Python environment ready.".green(),
        format!("({})", tool.label()).dimmed()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_py_tool_prefers_uv() {
        assert_eq!(choose_py_tool(true, false), Some(PyTool::Uv));
    }

    #[test]
    fn choose_py_tool_falls_back_to_python() {
        assert_eq!(
            choose_py_tool(false, true),
            Some(PyTool::Python(default_python_name()))
        );
    }

    #[test]
    fn choose_py_tool_none_when_neither() {
        assert_eq!(choose_py_tool(false, false), None);
    }
}
