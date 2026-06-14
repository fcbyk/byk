/// `byk remove` 子命令逻辑。
///
/// 删除 `byk init` 创建的持久化数据（venv、缓存、别名等），
/// 并提供 byk 包卸载指引。

use colored::Colorize;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

use super::paths::PathLayout;

// ---------------------------------------------------------------------------
// remove 帮助
// ---------------------------------------------------------------------------

/// 渲染 remove 帮助信息（无子参数时显示）。
pub fn render_remove_help() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk remove [feature]".bold());
    println!();
    println!("{}", "Feature:".green().bold());
    println!(
        "  {:<8} {}",
        "py".cyan().bold(),
        "Remove Python plugin cache (keep byk packages)"
    );
    println!(
        "  {:<8} {}",
        "py-v".cyan().bold(),
        "Remove venv, aliases, and plugin cache"
    );
    println!(
        "  {:<8} {}",
        "comp".cyan().bold(),
        "Remove shell completion (zsh/bash)"
    );
    println!(
        "  {:<8} {}",
        "npm".cyan().bold(),
        "Remove node-pkgs, aliases, and cache"
    );
    println!(
        "  {:<8} {}",
        "pnpm".cyan().bold(),
        "Remove node-pkgs, aliases, and cache"
    );
    println!();
}

// ---------------------------------------------------------------------------
// remove py
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
// remove py-v
// ---------------------------------------------------------------------------

/// 删除 Python venv 环境及所有关联数据。
///
/// 删除 ~/.byk/venv/、alias/py.byk.json、cache/app.json。
/// venv 整体删除，无需额外提示包卸载（目录已不存在）。
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
}

// ---------------------------------------------------------------------------
// remove comp
// ---------------------------------------------------------------------------

/// 移除 shell 补全配置。
///
/// 从 .zshrc / .bashrc 中移除 `byk completion` 相关行及前面的注释行。
pub fn rm_comp() {
    let shell = env::var("SHELL").unwrap_or_default();

    let (rc_filename, shell_name) = if shell.ends_with("/zsh") {
        (".zshrc", "zsh")
    } else if shell.ends_with("/bash") {
        (".bashrc", "bash")
    } else {
        eprintln!(
            "{} {} {}",
            "Unsupported shell:".red(),
            shell.dimmed(),
            "(supported: zsh, bash)".dimmed()
        );
        return;
    };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("{}", "Cannot determine home directory.".red());
            return;
        }
    };
    let rc_path = home.join(rc_filename);

    let content = fs::read_to_string(&rc_path).unwrap_or_default();

    let line = format!(
        "if command -v byk >/dev/null 2>&1; then source <(byk completion {}); fi",
        shell_name
    );

    if !content.contains(&line) {
        println!(
            "{}",
            "Shell completion not configured, nothing to remove.".dimmed()
        );
        return;
    }

    // 过滤掉 completion 行及前面的注释行
    let mut new_lines: Vec<&str> = Vec::new();
    let prev_lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < prev_lines.len() {
        let l = prev_lines[i];
        if l.contains("byk completion") {
            i += 1;
            continue;
        }
        // 跳过紧接 completion 行前的空行和注释行
        if i + 1 < prev_lines.len()
            && prev_lines[i + 1].contains("byk completion")
            && (l.trim().is_empty() || l.trim().starts_with("# byk shell completion"))
        {
            i += 1;
            continue;
        }
        new_lines.push(l);
        i += 1;
    }

    let new_content = new_lines.join("\n") + "\n";
    fs::write(&rc_path, new_content).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", rc_path.display(), e);
        std::process::exit(1);
    });

    println!(
        "  {} shell completion in {}",
        "-".red(),
        rc_path.display().to_string().dimmed()
    );
    println!(
        "{} {}",
        "Shell completion removed from".green(),
        rc_path.display().to_string().dimmed()
    );
}

// ---------------------------------------------------------------------------
// remove npm / remove pnpm
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
