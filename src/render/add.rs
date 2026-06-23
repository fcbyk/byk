/// `byk add` 帮助信息渲染。

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
        "  {:<22} {}",
        "-b, --branch <NAME>".cyan().bold(),
        "Set branch (default: main)",
    );
    println!(
        "  {:<22} {}",
        "-f, --file <PATH>".cyan().bold(),
        "Use local byk.json instead of remote registry",
    );
    println!(
        "  {:<22} {}",
        "-e, --editable <DIR>".cyan().bold(),
        "Editable install",
    );
    println!();
    println!("{}", "Features:".green().bold());
    println!(
        "  {:<8} {}",
        "npm".cyan().bold(),
        "Initialize with npm (node-pkgs, ni/nu aliases)"
    );
    println!(
        "  {:<8} {}",
        "pnpm".cyan().bold(),
        "Initialize with pnpm (node-pkgs, ni/nu aliases)"
    );
    println!(
        "  {:<8} {}",
        "comp".cyan().bold(),
        "Initialize shell completion (zsh/bash)"
    );
    println!(
        "  {:<8} {}",
        "cache".cyan().bold(),
        "Initialize CLI home & cache directories"
    );
    println!(
        "  {:<8} {}",
        "py-v".cyan().bold(),
        "Initialize Python venv & pip aliases"
    );
    println!(
        "  {:<8} {}",
        "uv".cyan().bold(),
        "Initialize Python venv & uv aliases (uv add/remove)"
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk add user/repo/key".into(), "Install specific key from a repo".into()),
        ("byk add user/repo".into(), "Install first key from a repo".into()),
        ("byk add --branch dev user/repo/key".into(), "Install from a specific branch".into()),
        ("byk add --file ./local.json my-key".into(), "Install from local registry file".into()),
        ("byk add -e .".into(), "Editable install from current directory".into()),
        ("byk add -e . hello".into(), "Editable install a specific key".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}