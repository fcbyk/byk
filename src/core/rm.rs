/// --rm 选项逻辑。
///
/// 删除 --init 创建的持久化数据（venv、缓存、别名等），
/// 并提供 byk 包卸载指引。

use colored::Colorize;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

use super::paths::PathLayout;

// ---------------------------------------------------------------------------
// --rm 帮助
// ---------------------------------------------------------------------------

/// 渲染 --rm 帮助信息（无子参数时显示）。
pub fn render_rm_help() {
    let title = "byk --rm <feature>";
    println!("{}", title.bold());
    println!();
    println!(
        "  {:<8} {}",
        "py".yellow(),
        "Remove Python plugin cache (keep byk packages)".dimmed()
    );
    println!(
        "  {:<8} {}",
        "py-v".yellow(),
        "Remove venv, aliases, and plugin cache".dimmed()
    );
    println!(
        "  {:<8} {}",
        "npm".yellow(),
        "Remove node-pkgs, aliases, and cache".dimmed()
    );
    println!(
        "  {:<8} {}",
        "pnpm".yellow(),
        "Remove node-pkgs, aliases, and cache".dimmed()
    );
}

// ---------------------------------------------------------------------------
// --rm py
// ---------------------------------------------------------------------------

/// 删除全局 Python 插件缓存。
///
/// 删除 cache/app.json，检索所有 byk 开头的 pip 包，
/// 提供一键卸载命令。
pub fn rm_py(layout: &PathLayout) {
    let cache_path = layout.cache_dir.join("app.json");

    // 检索 byk 包（无条件）
    let py_exe = crate::core::plugins::get_python_executable(&layout.cache_dir);
    let byk_packages = find_byk_packages(&py_exe);

    // 删除缓存（存在时）
    if cache_path.exists() {
        let _ = fs::remove_file(&cache_path);
        println!("  {} cache/app.json {}", "-".red(), "(removed)".dimmed());
    } else {
        println!("{}", "Plugin cache not found, skipped.".dimmed());
    }

    // 提供 byk 包卸载命令
    if let Some(cmd) = byk_packages {
        println!();
        println!(
            "{} {}",
            "byk-related packages detected:".yellow(),
            "(copy to uninstall all)".dimmed()
        );
        println!("  {}", cmd.white());
    } else {
        println!(
            "  {}",
            "No byk packages found.".dimmed()
        );
    }
}

// ---------------------------------------------------------------------------
// --rm py-v
// ---------------------------------------------------------------------------

/// 删除 Python venv 环境及所有关联数据。
///
/// 删除 ~/.byk/venv/、alias/py.byk.json、cache/app.json。
/// 检索 venv 中 byk 开头的包，提供卸载命令。
pub fn rm_py_v(layout: &PathLayout) {
    let venv_dir = &layout.venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");
    let cache_path = layout.cache_dir.join("app.json");

    if !venv_dir.exists() && !alias_path.exists() && !cache_path.exists() {
        println!(
            "{}",
            "Python venv not found. Nothing to remove.".dimmed()
        );
        return;
    }

    // 现有数据提示
    println!();
    println!("{}", "This will remove:".yellow());
    if venv_dir.exists() {
        println!("  {}", venv_dir.display().to_string().dimmed());
    }
    if alias_path.exists() {
        println!("  {}", alias_path.display().to_string().dimmed());
    }
    if cache_path.exists() {
        println!("  {}", cache_path.display().to_string().dimmed());
    }
    println!();

    let confirm_text = "py-v".to_string();
    print!("  {} {}: ", "Type".dimmed(), confirm_text.yellow());
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        println!();
        println!("  {}", "Cancelled.".dimmed());
        return;
    }
    if input.trim() != confirm_text {
        println!();
        println!(
            "{}",
            "Confirmation does not match. Cancelled.".dimmed()
        );
        return;
    }

    // 检索 byk 包（删除 venv 之前）
    #[cfg(windows)]
    let bin_dir = "Scripts";
    #[cfg(not(windows))]
    let bin_dir = "bin";

    let byk_packages = if venv_dir.exists() {
        let py = venv_dir
            .join(bin_dir)
            .join(if cfg!(windows) { "python.exe" } else { "python" });
        find_byk_packages(&py.to_string_lossy())
    } else {
        None
    };

    // 删除
    if venv_dir.exists() {
        let _ = fs::remove_dir_all(venv_dir);
        println!("  {} venv/ {}", "-".red(), "(removed)".dimmed());
    }
    if alias_path.exists() {
        let _ = fs::remove_file(&alias_path);
        println!(
            "  {} alias/py.byk.json {}",
            "-".red(),
            "(removed)".dimmed()
        );
    }
    if cache_path.exists() {
        let _ = fs::remove_file(&cache_path);
        println!(
            "  {} cache/app.json {}",
            "-".red(),
            "(removed)".dimmed()
        );
    }

    println!();
    println!("{}", "Python venv removed.".green());

    // 提供 byk 包卸载命令
    if let Some(cmd) = byk_packages {
        println!();
        println!(
            "{} {}",
            "byk-related packages detected:".yellow(),
            "(copy to uninstall all)".dimmed()
        );
        println!("  {}", cmd.white());
    }
}

// ---------------------------------------------------------------------------
// --rm npm / --rm pnpm
// ---------------------------------------------------------------------------

/// 删除 NPM node-pkgs 环境。
pub fn rm_npm(layout: &PathLayout) {
    rm_node_pkgs(layout, "npm");
}

/// 删除 PNPM node-pkgs 环境。
pub fn rm_pnpm(layout: &PathLayout) {
    rm_node_pkgs(layout, "pnpm");
}

fn rm_node_pkgs(layout: &PathLayout, pm: &str) {
    let node_pkgs_dir = &layout.node_pkgs_dir;
    let alias_path = layout.alias_dir.join("node.byk.json");
    let cache_path = layout.cache_dir.join("node-pkg.json");

    if !node_pkgs_dir.exists() && !alias_path.exists() && !cache_path.exists() {
        println!(
            "{} {}",
            "Node package data not found.".dimmed(),
            format!("({})", pm).dimmed()
        );
        return;
    }

    println!();
    println!("{} ({})", "This will remove:".yellow(), pm.dimmed());
    if node_pkgs_dir.exists() {
        println!("  {}", node_pkgs_dir.display().to_string().dimmed());
    }
    if alias_path.exists() {
        println!("  {}", alias_path.display().to_string().dimmed());
    }
    if cache_path.exists() {
        println!("  {}", cache_path.display().to_string().dimmed());
    }
    println!();

    let confirm_text = "node-pkgs".to_string();
    print!("  {} {}: ", "Type".dimmed(), confirm_text.yellow());
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        println!();
        println!("  {}", "Cancelled.".dimmed());
        return;
    }
    if input.trim() != confirm_text {
        println!();
        println!(
            "{}",
            "Confirmation does not match. Cancelled.".dimmed()
        );
        return;
    }

    if node_pkgs_dir.exists() {
        let _ = fs::remove_dir_all(node_pkgs_dir);
        println!(
            "  {} node-pkgs/ {}",
            "-".red(),
            "(removed)".dimmed()
        );
    }
    if alias_path.exists() {
        let _ = fs::remove_file(&alias_path);
        println!(
            "  {} alias/node.byk.json {}",
            "-".red(),
            "(removed)".dimmed()
        );
    }
    if cache_path.exists() {
        let _ = fs::remove_file(&cache_path);
        println!(
            "  {} cache/node-pkg.json {}",
            "-".red(),
            "(removed)".dimmed()
        );
    }

    println!();
    println!(
        "{} ({})",
        "Node packages removed.".green(),
        pm.dimmed()
    );
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 查找所有 byk 开头的 pip 包，返回一键卸载命令字符串。
fn find_byk_packages(python_exe: &str) -> Option<String> {
    let output = Command::new(python_exe)
        .args(["-m", "pip", "list", "--format=json"])
        .output()
        .ok()?;

    let packages: Vec<String> = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()?
        .as_array()?
        .iter()
        .filter_map(|p| p.get("name")?.as_str().map(String::from))
        .filter(|name| name.starts_with("byk"))
        .collect();

    if packages.is_empty() {
        None
    } else {
        Some(format!("pip uninstall {}", packages.join(" ")))
    }
}
