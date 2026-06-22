/// `byk remove` 帮助信息渲染。

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
        "  {:<8} {}",
        "comp".cyan().bold(),
        "Remove shell completion (zsh/bash)"
    );
    println!(
        "  {:<8} {}",
        "node".cyan().bold(),
        "Remove node-pkgs, aliases, and cache"
    );
    println!(
        "  {:<8} {}",
        "all".cyan().bold(),
        "Remove everything (~/.byk/ + shell completion)"
    );
    println!();
    println!("{}", "Plugins:".green().bold());
    println!(
        "  {:<15} {}",
        "<plugin-key>".cyan().bold(),
        "Uninstall a plugin by its key"
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