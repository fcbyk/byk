//! `byk add` 帮助信息渲染。

use colored::Colorize;

use crate::utils::display;

/// 渲染 `byk add` 帮助信息。
pub fn render() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add [OPTIONS] <USER/REPO[/KEY] | FEATURE>".bold());
    println!();
    println!("{}", "Options:".green().bold());
    println!(
        "  {:<22} Set branch (default: main)",
        "-b, --branch <NAME>".cyan().bold(),
    );
    println!(
        "  {:<22} Use local byk.json instead of remote registry",
        "-f, --file <PATH>".cyan().bold(),
    );
    println!();
    println!("{}", "Features:".green().bold());
    println!(
        "  {:<8} Initialize with npm (node-pkgs, ni/nu aliases)",
        "npm".cyan().bold()
    );
    println!(
        "  {:<8} Initialize with pnpm (node-pkgs, ni/nu aliases)",
        "pnpm".cyan().bold()
    );
    println!(
        "  {:<8} Initialize shell completion (zsh/bash)",
        "comp".cyan().bold()
    );
    println!(
        "  {:<8} Initialize CLI home & cache directories",
        "cache".cyan().bold()
    );
    println!(
        "  {:<8} Initialize Python venv & pip aliases",
        "py-v".cyan().bold()
    );
    println!(
        "  {:<8} Initialize Python venv & uv aliases (uv add/remove)",
        "uv".cyan().bold()
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk add user/repo/key".into(), "Install specific key from a repo".into()),
        ("byk add user/repo".into(), "Install first key from a repo".into()),
        ("byk add --branch dev user/repo/key".into(), "Install from a specific branch".into()),
        ("byk add --file ./local.json my-key".into(), "Install from local registry file".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}