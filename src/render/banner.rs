//! Banner 渲染。
//!
//! 形态：两条等宽虚线夹住版本号与文档地址，作为 `--help` 的顶部区块。

use colored::Colorize;

/// 虚线宽度。
const SEPARATOR_WIDTH: usize = 29;

/// 文档地址。
const DOCS_URL: &str = "https://cli.fcbyk.com";

fn separator() -> String {
    "-".repeat(SEPARATOR_WIDTH)
}

/// 默认 banner：虚线 / 版本号 / 文档地址 / 虚线。
pub fn render() {
    println!();
    for line in lines() {
        println!("{}", line);
    }
}

/// banner 的文本行（含颜色，不含首尾空行）。
///
/// 独立出来便于断言，`render` 只负责打印。
fn lines() -> Vec<String> {
    vec![
        separator().dimmed().to_string(),
        format!(
            "{} {}",
            "BYK".green().bold(),
            format!("v{}", env!("CARGO_PKG_VERSION")).cyan().bold()
        ),
        format!("Docs {}", DOCS_URL).dimmed().to_string(),
        separator().dimmed().to_string(),
    ]
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
        render();
    }

    #[test]
    fn separator_is_29_dashes() {
        assert_eq!(separator().len(), SEPARATOR_WIDTH);
        assert_eq!(separator(), "-----------------------------");
    }

    #[test]
    fn banner_is_wrapped_by_two_separators() {
        let lines = lines();
        assert_eq!(lines.len(), 4);

        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert_eq!(plain[0], separator());
        assert_eq!(plain[3], separator());
    }

    #[test]
    fn banner_middle_holds_version_and_docs() {
        let lines = lines();
        let version = strip_ansi(&lines[1]);
        let docs = strip_ansi(&lines[2]);

        assert_eq!(version, format!("BYK v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(docs, format!("Docs {}", DOCS_URL));
    }
}
