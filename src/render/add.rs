//! `byk add` 帮助信息渲染。

use colored::Colorize;

use crate::utils::display;

/// 渲染 `byk add` 帮助信息。
pub fn render() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add [OPTIONS] <user/repo[/path]> | <file> | FEATURE".bold());
    println!();
    println!("{}", "Options:".green().bold());
    println!(
        "  {:<22} Install a local .byk.json file by path",
        "-f, --file <PATH>".cyan().bold(),
    );
    println!(
        "  {:<22} Access GitHub repo via jsDelivr CDN",
        "--cdn".cyan().bold(),
    );
    println!();
    println!("{}", "Features:".green().bold());
    println!(
        "  {:<8} Initialize shell completion (zsh/bash)",
        "comp".cyan().bold()
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk add user/repo/pysdk".into(), "Fetch pysdk.byk.json from a GitHub repo".into()),
        ("byk add user/repo/tools/lint".into(), "Fetch lint.byk.json from a repo subdirectory".into()),
        ("byk add user/repo@dev/hello".into(), "Fetch hello.byk.json from the dev branch".into()),
        ("byk add --cdn user/repo/hello".into(), "Fetch via jsDelivr CDN".into()),
        ("byk add myfile".into(), "Install myfile.byk.json from the current directory".into()),
        ("byk add -f ./path/to/x.byk.json".into(), "Install a local .byk.json file by path".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}

/// 渲染 `byk add <file>` 用法信息。
pub fn render_alias() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add <file>".bold());
    println!();
    println!("Install a local *.byk.json plugin to the global directory (~/.byk/alias/).");
    println!("Requires a $add block. Installs its pip deps and downloads its assets.");
    println!();
    println!("{}", "Examples:".green().bold());
    println!("  {}", "byk add demo".dimmed());
    println!("  {}", "byk add user/repo/pysdk".dimmed());
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

    #[test]
    fn render_alias_runs_without_panic() {
        render_alias();
    }
}