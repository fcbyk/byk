//! `byk add` 帮助信息渲染。

use colored::Colorize;

use crate::utils::display;

/// 渲染 `byk add` 帮助信息。
pub fn render() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add <comp | @<file> | @@<file>>".bold());
    println!();
    println!("{}", "Features:".green().bold());
    println!(
        "  {:<8} Initialize shell completion (zsh/bash)",
        "comp".cyan().bold()
    );
    println!();
    println!("{}", "Install from alias files:".green().bold());
    println!(
        "  {:<31} Install from {}.byk.json in current directory",
        "@<file>".cyan().bold(),
        "<file>".dimmed()
    );
    println!(
        "  {:<31} Install from {}.byk.json in global alias directory",
        "@@<file>".cyan().bold(),
        "<file>".dimmed()
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk add comp".into(), "Configure shell completion".into()),
        ("byk add @demo".into(), "Install demo.byk.json from current directory".into()),
        ("byk add @@demo".into(), "Install demo.byk.json from ~/.byk/alias/".into()),
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
        // render() only prints to stdout; verify it doesn't panic
        render();
    }
}