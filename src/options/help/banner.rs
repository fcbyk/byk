//! Banner 渲染。
//!
//! 正常：两条等宽虚线夹住版本号与文档地址。
//! 扫描有问题：同样的框，中间换成问题数量与 `--check` 提示。

use colored::Colorize;

use crate::utils::display::get_display_width;

/// 虚线最小宽度。
const SEPARATOR_WIDTH: usize = 29;

/// 文档地址。
const DOCS_URL: &str = "https://cli.fcbyk.com";

fn separator(width: usize) -> String {
    "-".repeat(width)
}

/// 渲染 banner；`problem_files` 为存在诊断的别名文件数。
pub fn render(problem_files: usize) {
    println!();
    for line in lines(problem_files) {
        println!("{}", line);
    }
}

/// banner 的文本行（含颜色，不含首尾空行）。
///
/// 独立出来便于断言，`render` 只负责打印。
fn lines(problem_files: usize) -> Vec<String> {
    let (plain, colored): (Vec<String>, Vec<String>) = if problem_files == 0 {
        (
            vec![
                format!("BYK v{}", env!("CARGO_PKG_VERSION")),
                format!("Docs {}", DOCS_URL),
            ],
            vec![
                format!(
                    "{} {}",
                    "BYK".green().bold(),
                    format!("v{}", env!("CARGO_PKG_VERSION")).cyan().bold()
                ),
                format!("Docs {}", DOCS_URL).dimmed().to_string(),
            ],
        )
    } else {
        let plural = if problem_files == 1 { "" } else { "s" };
        let has = if problem_files == 1 { "has" } else { "have" };
        (
            vec![
                format!("{} alias file{} {} problems", problem_files, plural, has),
                "Run byk --check for details".to_string(),
            ],
            vec![
                format!(
                    "{} alias file{} {} {}",
                    problem_files,
                    plural,
                    has,
                    "problems".yellow().bold()
                ),
                format!("Run {} for details", "byk --check".cyan().bold()),
            ],
        )
    };

    let width = plain
        .iter()
        .map(|line| get_display_width(line))
        .max()
        .unwrap_or(0)
        .max(SEPARATOR_WIDTH);
    let separator = separator(width).dimmed().to_string();

    let mut output = vec![separator.clone()];
    output.extend(colored);
    output.push(separator);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 去除 ANSI 转义码，方便断言。
    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c == '\x1b' {
                chars.next();
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&inner) = chars.peek() {
                        chars.next();
                        if inner.is_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(c);
                chars.next();
            }
        }
        result
    }

    #[test]
    fn render_runs_without_panic() {
        render(0);
        render(3);
    }

    #[test]
    fn clean_banner_keeps_version_and_docs() {
        let lines = lines(0);
        assert_eq!(lines.len(), 4);

        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert_eq!(plain[1], format!("BYK v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(plain[2], format!("Docs {}", DOCS_URL));
    }

    #[test]
    fn problem_banner_has_no_version_or_docs() {
        let lines = lines(4);
        assert_eq!(lines.len(), 4);

        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert_eq!(plain[1], "4 alias files have problems");
        assert_eq!(plain[2], "Run byk --check for details");
        assert!(!plain.iter().any(|line| line.contains("BYK")));
        assert!(!plain.iter().any(|line| line.contains("Docs")));
    }

    #[test]
    fn problem_banner_uses_singular_forms() {
        let plain: Vec<String> = lines(1).iter().map(|l| strip_ansi(l)).collect();
        assert_eq!(plain[1], "1 alias file has problems");
    }

    #[test]
    fn separator_expands_to_the_longest_line() {
        let lines = lines(1000);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let width = get_display_width(&plain[1]);
        assert_eq!(plain[0], "-".repeat(width));
        assert_eq!(plain[3], "-".repeat(width));
    }
}
