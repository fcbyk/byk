//! `byk remove` 帮助信息渲染。

use colored::Colorize;

use crate::utils::display;

/// 渲染 remove 帮助信息（无子参数时显示）。
pub fn render() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!(
        "{}",
        " byk remove <comp | all | @<file> | @@<file>>".bold()
    );
    println!();
    println!("{}", "Features:".green().bold());
    println!(
        "  {:<8} Remove shell completion (zsh/bash)",
        "comp".cyan().bold()
    );
    println!(
        "  {:<8} Remove everything (~/.byk/ + shell completion)",
        "all".cyan().bold()
    );
    println!();
    println!("{}", "Uninstall from alias files:".green().bold());
    println!(
        "  {:<31} Uninstall from {}.byk.json in current directory",
        "@<file>".cyan().bold(),
        "<file>".dimmed()
    );
    println!(
        "  {:<31} Uninstall from {}.byk.json in global alias directory",
        "@@<file>".cyan().bold(),
        "<file>".dimmed()
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk remove comp".into(), "Remove shell completion".into()),
        ("byk remove all".into(), "Remove everything".into()),
        (
            "byk remove @demo".into(),
            "Uninstall demo.byk.json from current directory".into(),
        ),
        (
            "byk remove @@demo".into(),
            "Uninstall demo.byk.json from ~/.byk/alias/".into(),
        ),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_runs_without_panic() {
        render();
    }
}