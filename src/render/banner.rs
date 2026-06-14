/// Banner 渲染。
///
/// 后续可扩展多种 banner 风格，按需选择。

use colored::Colorize;

/// 默认 banner：版本号 + 文档链接。
pub fn render() {
    let separator = "-".repeat(29);

    println!();
    println!(
        "{} {}",
        "BYK".green().bold(),
        format!("v{}", env!("CARGO_PKG_VERSION")).cyan().bold()
    );
    println!("{}", "Docs https://cli.fcbyk.com".dimmed());
    println!("{}", separator.dimmed());
}
