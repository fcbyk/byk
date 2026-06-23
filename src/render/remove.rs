//! `byk remove` 帮助信息渲染。

use colored::Colorize;

use crate::utils::display;

/// 渲染 remove 帮助信息（无子参数时显示）。
pub fn render() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk remove <FEATURE | PLUGIN_KEY>".bold());
    println!();
    println!("{}", "Features:".green().bold());
    println!(
        "  {:<8} Remove shell completion (zsh/bash)",
        "comp".cyan().bold()
    );
    println!(
        "  {:<8} Remove node-pkgs, aliases, and cache",
        "node".cyan().bold()
    );
    println!(
        "  {:<8} Remove everything (~/.byk/ + shell completion)",
        "all".cyan().bold()
    );
    println!(
        "  {:<8} Remove Python venv & aliases",
        "py".cyan().bold()
    );
    println!();
    println!("{}", "Plugins:".green().bold());
    println!(
        "  {:<15} Uninstall a plugin by its key",
        "<plugin-key>".cyan().bold()
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk remove comp".into(), "Remove shell completion".into()),
        ("byk remove node".into(), "Remove node-pkgs environment".into()),
        ("byk remove all".into(), "Remove everything".into()),
        ("byk remove hello".into(), "Uninstall plugin by key".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}