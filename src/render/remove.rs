/// `byk remove` 帮助信息渲染。

use colored::Colorize;

/// 渲染 remove 帮助信息（无子参数时显示）。
pub fn render() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk remove [feature]".bold());
    println!();
    println!("{}", "Feature:".green().bold());
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
}