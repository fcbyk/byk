/// --init 选项逻辑。
///
/// 用户手动按需初始化 CLI 功能，不自动创建任何配置。
/// 模板文件位于 src/templates/，通过 include_str! 编译期嵌入。

use colored::Colorize;
use std::fs;
use std::io::{self, Write};

use super::paths::PathLayout;

// ---------------------------------------------------------------------------
// 模板（编译期嵌入）
// ---------------------------------------------------------------------------

const NPM_TEMPLATE: &str = include_str!("../templates/npm.byk.json");
const PNPM_TEMPLATE: &str = include_str!("../templates/pnpm.byk.json");

// ---------------------------------------------------------------------------
// --init 帮助
// ---------------------------------------------------------------------------

/// 渲染 --init 帮助信息（无子参数时显示）。
pub fn render_init_help() {
    let title = "byk --init <feature>";
    println!("{}", title.bold());
    println!();
    println!(
        "  {:<8} {}",
        "npm".yellow(),
        "Initialize with npm (node-pkgs)".dimmed()
    );
    println!(
        "  {:<8} {}",
        "pnpm".yellow(),
        "Initialize with pnpm (node-pkgs)".dimmed()
    );
}

// ---------------------------------------------------------------------------
// --init npm / --init pnpm
// ---------------------------------------------------------------------------

/// 初始化 npm 命令功能。
pub fn init_npm(layout: &PathLayout) {
    init_node_pkgs(layout, "npm", "i", "uni", NPM_TEMPLATE);
}

/// 初始化 pnpm 命令功能。
pub fn init_pnpm(layout: &PathLayout) {
    init_node_pkgs(layout, "pnpm", "add", "remove", PNPM_TEMPLATE);
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 创建目录，打印操作信息。
fn ensure_dir(path: &std::path::Path, label: &str) {
    if path.exists() {
        println!("  {} {} {}", "+".dimmed(), label.dimmed(), "(exists)".dimmed());
    } else {
        fs::create_dir_all(path).unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", label, e);
        });
        println!("  {} {}", "+".green(), label.dimmed());
    }
}

/// 写入文件内容。
fn write_file(path: &std::path::Path, content: &str, label: &str) {
    if path.exists() {
        println!(
            "  {} {} {}",
            "*".dimmed(),
            label.dimmed(),
            "(updated)".dimmed()
        );
    } else {
        println!("  {} {}", "+".green(), label.dimmed());
    }
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("Failed to create {}: {}", label, e);
    });
}

// ---------------------------------------------------------------------------
// 共享初始化逻辑
// ---------------------------------------------------------------------------

fn init_node_pkgs(
    layout: &PathLayout,
    pm: &str,
    install_alias: &str,
    remove_alias: &str,
    template: &str,
) {
    let node_pkgs_dir = &layout.node_pkgs_dir;
    let alias_path = layout.alias_dir.join("node.byk.json");
    let cache_path = layout.cache_dir.join("node-pkg.json");

    let has_existing =
        node_pkgs_dir.exists() || alias_path.exists() || cache_path.exists();

    if has_existing {
        println!();
        println!(
            "{}",
            "Existing node package data detected:".yellow()
        );
        if node_pkgs_dir.exists() {
            println!("  {}", layout.node_pkgs_dir.display().to_string().dimmed());
        }
        if alias_path.exists() {
            println!("  {}", alias_path.display().to_string().dimmed());
        }
        if cache_path.exists() {
            println!("  {}", cache_path.display().to_string().dimmed());
        }
        println!();
        println!(
            "{}",
            "This will permanently remove ALL existing node packages,".red().bold()
        );
        println!(
            "{}",
            "aliases, and cache. This action cannot be undone.".red().bold()
        );
        println!();

        // 输入 "node-pkgs" 确认删除
        let confirm_text = "node-pkgs".to_string();
        print!(
            "  {} {}: ",
            "Type".dimmed(),
            confirm_text.yellow()
        );
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            println!();
            println!("  {}", "Cancelled.".dimmed());
            return;
        }
        if input.trim() != confirm_text {
            println!();
            println!("  {}", "Confirmation does not match. Cancelled.".dimmed());
            return;
        }

        // 删除旧数据
        if node_pkgs_dir.exists() {
            let _ = fs::remove_dir_all(node_pkgs_dir);
            println!("  {} node-pkgs {}", "-".dimmed(), "(removed)".dimmed());
        }
        if alias_path.exists() {
            let _ = fs::remove_file(&alias_path);
            println!("  {} alias/node.byk.json {}", "-".dimmed(), "(removed)".dimmed());
        }
        if cache_path.exists() {
            let _ = fs::remove_file(&cache_path);
            println!("  {} cache/node-pkg.json {}", "-".dimmed(), "(removed)".dimmed());
        }
    }

    // 确保目录存在
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(node_pkgs_dir, "node-pkgs");
    ensure_dir(&layout.alias_dir, "alias");

    // 写入别名模板
    write_file(&alias_path, template, "alias/node.byk.json");

    println!();
    println!(
        "{} ({})",
        "Node package support initialized.".green(),
        pm.dimmed(),
    );
    println!("  Install packages:  byk {} <pkg>", install_alias.dimmed());
    println!("  Remove packages:   byk {} <pkg>", remove_alias.dimmed());
}
