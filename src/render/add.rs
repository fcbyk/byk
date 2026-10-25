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
    println!(
        "  {:<8} Install $pip dependencies from an alias file",
        "pip".cyan().bold()
    );
    println!(
        "  {:<8} Download $assets resources from an alias file",
        "assets".cyan().bold()
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
        ("byk add pip @jiahao".into(), "Install $pip deps declared by jiahao.byk.json".into()),
        ("byk add assets @jiahao".into(), "Download $assets resources declared by jiahao.byk.json".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}

/// 渲染 `byk add pip` 用法信息。
pub fn render_pip() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add pip <@file | @@file | PATH>".bold());
    println!();
    println!("Install the packages declared by a *.byk.json file's $pip field into the byk venv.");
    println!();
    println!("{}", "Targets:".green().bold());
    println!(
        "  {:<24}Local alias file in the current directory",
        "@file".cyan().bold()
    );
    println!(
        "  {:<24}Global alias file (~/.byk/alias/)",
        "@@file".cyan().bold()
    );
    println!(
        "  {:<24}Direct path to a .byk.json file",
        "./path.byk.json".cyan().bold()
    );
    println!();
    println!("{}", "Example:".green().bold());
    println!("  {}", "byk add pip @jiahao".dimmed());
    println!();
}

/// 渲染 `byk add assets` 用法信息。
pub fn render_assets() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add assets <@file | @@file>".bold());
    println!();
    println!("Download the resources declared by a *.byk.json file's $assets block.");
    println!();
    println!("{}", "Targets:".green().bold());
    println!(
        "  {:<24}Local alias file in the current directory",
        "@file".cyan().bold()
    );
    println!(
        "  {:<24}Global alias file (~/.byk/alias/)",
        "@@file".cyan().bold()
    );
    println!();
    println!("{}", "Where files land:".green().bold());
    println!("  With $to set        → directly in that directory");
    println!("  Local file, no $to  → the file's own directory");
    println!("  Global file, no $to → the current working directory");
    println!("  Multiple entries, no $to → an auto-created assets/ subdirectory");
    println!();
    println!("{}", "Example:".green().bold());
    println!("  {}", "byk add assets @jiahao".dimmed());
    println!();
}

/// 渲染 `byk add <file>` 用法信息。
pub fn render_alias() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add <file>".bold());
    println!();
    println!("Install a local *.byk.json file to the global directory (~/.byk/alias/).");
    println!("Installs its $pip deps, downloads its $assets, and keeps only aliases");
    println!("marked with $install: true.");
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
    fn render_pip_runs_without_panic() {
        render_pip();
    }

    #[test]
    fn render_assets_runs_without_panic() {
        render_assets();
    }

    #[test]
    fn render_alias_runs_without_panic() {
        render_alias();
    }
}