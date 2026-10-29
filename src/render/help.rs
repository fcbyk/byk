//! 帮助信息渲染（Usage / Options / Aliases）。
//!
//! `render_all` 为 -h/--help 回调及无参数时的默认输出。

use colored::Colorize;

use crate::core::aliases;
use crate::core::paths::PathLayout;

/// 渲染完整帮助信息（Banner + Usage + Options + Aliases）。
///
/// `byk`、`byk -h`、`byk --help` 三条入口输出完全一致，banner 在最上方。
pub fn render_all(layout: &PathLayout, options: &[(String, String)]) {
    super::banner::render();
    println!();
    render_usage();
    println!();
    render_options(options);

    let (merged, _files) = aliases::load_merged_aliases(layout);
    super::aliases::render(&merged);

    println!();
}

/// 渲染 Usage 说明。
pub fn render_usage() {
    println!(
        "{} byk [OPTIONS] [ALIAS] [ARGS]...",
        "Usage:".green().bold()
    );
}

/// 渲染全局选项列表。
pub fn render_options(options: &[(String, String)]) {
    let lines = format_options_lines(options);
    if lines.is_empty() {
        return;
    }

    println!("{}", "Options:".green().bold());
    for line in &lines {
        println!("{}", line);
    }
}

/// 将选项格式化为对齐的展示行列表（不含标题）。
fn format_options_lines(options: &[(String, String)]) -> Vec<String> {
    if options.is_empty() {
        return Vec::new();
    }

    let max_label_len = options
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);

    options
        .iter()
        .map(|(label, desc)| {
            let padded = format!("{:width$}", label, width = max_label_len);
            format!("  {}  {}", padded.cyan().bold(), desc)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 去除 ANSI 转义码，方便断言。
    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c == '\x1b' {
                // 跳过整个转义序列，直到遇到字母（m）
                chars.next(); // skip ESC
                if chars.peek() == Some(&'[') {
                    chars.next(); // skip '['
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
    fn format_options_empty() {
        assert!(format_options_lines(&[]).is_empty());
    }

    #[test]
    fn format_options_single() {
        let options = vec![("-h, --help".into(), "Print help".into())];
        let result = format_options_lines(&options);
        assert_eq!(result.len(), 1);
        // 去除 ANSI 转义码后检查格式
        let plain = strip_ansi(&result[0]);
        assert_eq!(plain, "  -h, --help  Print help");
    }

    #[test]
    fn format_options_multiple_aligned() {
        let options = vec![
            ("-v, --version".into(), "Print version".into()),
            ("-h, --help".into(), "Print help".into()),
        ];
        let result = format_options_lines(&options);
        assert_eq!(result.len(), 2);
        // 两行的描述起始位置应该对齐
        let first = strip_ansi(&result[0]);
        let second = strip_ansi(&result[1]);
        let first_desc_start = first.find("Print version").unwrap();
        let second_desc_start = second.find("Print help").unwrap();
        assert_eq!(first_desc_start, second_desc_start);
        assert!(first.starts_with("  -v, --version"));
        assert!(second.starts_with("  -h, --help"));
    }

    #[test]
    fn format_options_long_label_alignment() {
        let options = vec![
            ("x".into(), "Short".into()),
            ("very-long-option".into(), "Long option description".into()),
        ];
        let result = format_options_lines(&options);
        assert_eq!(result.len(), 2);
        let first = strip_ansi(&result[0]);
        let second = strip_ansi(&result[1]);
        // Short key "x" should be padded to match "very-long-option"
        let x_pad = first.find("Short").unwrap();
        let long_pad = second.find("Long").unwrap();
        assert_eq!(x_pad, long_pad);
    }

    #[test]
    fn format_options_single_option_no_extra_lines() {
        let options = vec![
            ("--cdn".into(), "Use CDN".into()),
        ];
        let result = format_options_lines(&options);
        assert_eq!(result.len(), 1);
        let plain = strip_ansi(&result[0]);
        assert_eq!(plain, "  --cdn  Use CDN");
    }
}