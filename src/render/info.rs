/// CLI 信息格式化渲染（--info 选项）。

use colored::Colorize;
use std::env;
use std::fs;
use std::process::Command;

use crate::core::paths::PathLayout;
use crate::core::plugins;

/// 渲染 CLI 完整信息：banner + 基础信息 + Node + Python。
pub fn render_all(layout: &PathLayout) {
    crate::render::banner::render();
    render_general(layout);
    render_node(layout);
    render_py(layout);
}

// ---------------------------------------------------------------------------
// 基础信息
// ---------------------------------------------------------------------------

fn render_general(layout: &PathLayout) {
    // 目录路径
    print_path("CLI Home", &layout.root_dir);
    print_path("Alias Directory", &layout.alias_dir);
    print_path("Logs Directory", &layout.logs_dir);

    // 缓存状态
    let cache_dir = &layout.cache_dir;
    let cache_initialized = if cache_dir.exists() {
        let entries = fs::read_dir(cache_dir).ok();
        entries
            .map(|e| e.filter_map(|en| en.ok()).count() > 0)
            .unwrap_or(false)
    } else {
        false
    };
    if cache_initialized {
        println!("{}:  {}", "Cache".yellow(), "enabled".green());
    } else {
        println!("{}", "Cache not initialized.".yellow());
        println!("  {}", "$ byk init cache".dimmed());
    }

    println!("{}", "-".repeat(29).dimmed());

    // 补全状态
    if let Some(shell_name) = detect_shell() {
        let (_rc_file, configured) = check_completion(&shell_name);
        if configured {
            println!("{}:  {}", "Completion".yellow(), "enabled".green());
        } else {
            println!("{}", "Completion not configured.".yellow());
            println!("  {}", "$ byk init comp".dimmed());
        }
    }
}

// ---------------------------------------------------------------------------
// Node 信息
// ---------------------------------------------------------------------------

fn render_node(layout: &PathLayout) {
    println!("{}", "-".repeat(29).dimmed());

    let node_dir = &layout.node_pkgs_dir;
    let alias_path = layout.alias_dir.join("node.byk.json");
    let cache_path = layout.cache_dir.join("node-pkg.json");

    let initialized = node_dir.exists() && alias_path.exists() && cache_path.exists();

    if !initialized {
        println!("{}", "Node package support not initialized.".yellow());
        println!("  {}   {}", "$ byk init npm".dimmed(), "(node-pkgs)".dimmed());
        println!("  {}   {}", "$ byk init pnpm".dimmed(), "(node-pkgs)".dimmed());
    } else {
        print_path("Node Packages", node_dir);
    }
}

// ---------------------------------------------------------------------------
// Python 信息
// ---------------------------------------------------------------------------

/// 渲染 Python 环境信息。
pub fn render_py(layout: &PathLayout) {
    println!("{}", "-".repeat(29).dimmed());

    let python_exe = plugins::get_python_executable(&layout.cache_dir);
    let cache_file = layout.cache_dir.join("app.json");
    let is_env = env::var("BYK_PYTHON").is_ok();

    print_path("Python venv", &layout.venv_dir);

    // 未初始化检测：无 BYK_PYTHON 且 app.json 不存在
    if !is_env && !cache_file.exists() {
        println!("{}", "Python plugin system not initialized.".yellow());
        println!("  {}   {}", "$ byk init py".dimmed(), "(global)".dimmed());
        println!("  {}   {}", "$ byk init py-v".dimmed(), "(venv, recommended)".dimmed());
        println!();
        return;
    }

    // Python 解释器路径
    println!("{}: {}", "Python".yellow(), python_exe);

    // 尝试获取 Python 版本
    if let Ok(output) = Command::new(&python_exe)
        .args(["--version"])
        .output()
    {
        let version = String::from_utf8_lossy(&output.stdout);
        let version = version.trim();
        if !version.is_empty() {
            println!("{}: {}", "Version".yellow(), version);
        }
    }

    // 缓存文件路径
    println!("{}: {}", "Cache".yellow(), cache_file.display());

    // 来源提示
    if is_env {
        println!("{}:  {}", "Source".yellow(), "BYK_PYTHON env var".dimmed());
    } else {
        println!("{}:  {}", "Source".yellow(), "Cache file (app.json)".dimmed());
    }
    println!();
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 打印路径，仅路径存在时显示。
fn print_path(label: &str, path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    println!("{}: {}", label.yellow(), path.display());
}

/// 从 $SHELL 检测当前 shell。
fn detect_shell() -> Option<&'static str> {
    let shell = env::var("SHELL").unwrap_or_default();
    if shell.ends_with("/zsh") {
        Some("zsh")
    } else if shell.ends_with("/bash") {
        Some("bash")
    } else {
        None
    }
}

/// 检查 shell 补全是否已配置。
fn check_completion(shell_name: &str) -> (&'static str, bool) {
    let rc_filename = if shell_name == "zsh" { ".zshrc" } else { ".bashrc" };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (rc_filename, false),
    };

    let rc_path = home.join(rc_filename);
    let content = fs::read_to_string(&rc_path).unwrap_or_default();

    let line = format!(
        "if command -v byk >/dev/null 2>&1; then source <(byk completion {}); fi",
        shell_name
    );

    (rc_filename, content.contains(&line))
}
