//! 帮助信息渲染（Usage / Options / Commands / Aliases）。
//!
//! `render_all` 为 -h/--help 回调及无参数时的默认输出。

use colored::Colorize;

use crate::core::aliases;
use crate::core::paths::PathLayout;
use crate::core::plugins;
use crate::utils::display;

/// 渲染完整帮助信息（Usage + Options + Commands + Aliases）。
///
/// -h / --help 直接调用，仪表盘在之前叠加 banner。
pub fn render_all(layout: &PathLayout, options: &[(String, String)]) {
    render_usage();
    println!();
    render_options(options);

    // 内置 + 插件命令合并到一个 Commands 区块
    render_commands(layout);

    let (merged, _files) = aliases::load_merged_aliases(layout);
    super::aliases::render(&merged, &layout.alias_dir);

    println!();
}

/// 渲染内置 + 插件命令（合并到 Commands 区块）。
///
/// 分两个对齐区渲染：
/// - 单命令区：内置子命令 + 未分组的插件命令，内部互相对齐
/// - 命令组区：desc 相同的多个插件命令合并为一行（逗号拼接），
///   单独对齐——组 key 通常较长，不与单命令共用对齐列
pub fn render_commands(layout: &PathLayout) {
    // 内置子命令（始终单命令展示）
    let builtin: Vec<(String, String)> = vec![
        ("add".into(), "Add plugins or features".into()),
        ("remove".into(), "Remove plugins or features".into()),
        ("show".into(), "Show system info, plugins, or command sources".into()),
    ];

    // 插件命令（不依赖 venv：bin 类型命令与 Python 环境无关）
    let plugin_state = plugins::state::load_plugin_state(&layout.plugins_dir, &layout.venv_dir);
    let plugin_entries: Vec<(String, String)> = plugin_state
        .commands
        .iter()
        .map(|(name, cmd)| (name.clone(), cmd.desc.clone()))
        .collect();

    let (plugin_singles, groups) = group_commands(plugin_entries);

    // 单命令区 = 内置 + 插件单命令（内置在前，插件单命令已按名排序）
    let mut singles = builtin;
    singles.extend(plugin_singles);

    println!();
    println!("{}", "Commands:".green().bold());
    print_aligned(&singles);
    if !groups.is_empty() {
        print_aligned(&groups);
    }
}

/// 命令条目列表：（命令名/组 key, 描述）
type CmdEntries = Vec<(String, String)>;

/// 按 desc 聚合命令：desc 非空且相同的多个命令合并为一组，
/// 避免同包多个 bin 命令在帮助里重复显示同一描述。
/// desc 为空的命令不参与聚合（避免把无关命令因“都无描述”而合并）。
/// 返回 (单命令, 组)，组的 key 为成员排序后的逗号拼接；
/// 两部分各自按 key 排序。
fn group_commands(entries: CmdEntries) -> (CmdEntries, CmdEntries) {
    let mut by_desc: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut singles: Vec<(String, String)> = Vec::new();

    for (name, desc) in entries {
        if desc.is_empty() {
            singles.push((name, desc));
            continue;
        }
        by_desc.entry(desc).or_default().push(name);
    }

    let mut groups: Vec<(String, String)> = Vec::new();
    for (desc, mut names) in by_desc {
        if names.len() == 1 {
            singles.push((names.pop().unwrap(), desc));
        } else {
            names.sort();
            groups.push((names.join(", "), desc));
        }
    }

    singles.sort_by(|a, b| a.0.cmp(&b.0));
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    (singles, groups)
}

/// 打印一段对齐的命令行（缩进前缀 + 彩色命令名 + 描述）。
fn print_aligned(entries: &[(String, String)]) {
    for (name, line) in &display::align_kv_pairs(entries, "  ") {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.cyan().bold());
        println!("{}", rest);
    }
}

/// 渲染 Usage 说明。
pub fn render_usage() {
    println!(
        "{} byk [OPTIONS] [COMMAND|ALIAS] [ARGS]...",
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

    // ==================== group_commands ====================

    #[test]
    fn group_commands_no_duplicates_all_singles() {
        let entries = vec![
            ("add".to_string(), "Add plugins or features".to_string()),
            ("npm".to_string(), "Manage npm packages".to_string()),
        ];
        let (singles, groups) = group_commands(entries);
        assert_eq!(singles.len(), 2);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_commands_same_desc_merged_into_one_line() {
        // 同包多个 bin 命令（如 @antfu/ni 的 8 个）合并为一行
        let entries = vec![
            ("ni".to_string(), "from @antfu/ni@30.6.0".to_string()),
            ("na".to_string(), "from @antfu/ni@30.6.0".to_string()),
            ("nr".to_string(), "from @antfu/ni@30.6.0".to_string()),
            ("npm".to_string(), "Manage npm packages".to_string()),
        ];
        let (singles, groups) = group_commands(entries);
        assert_eq!(singles.len(), 1);
        assert_eq!(singles[0].0, "npm");
        assert_eq!(groups.len(), 1);
        // 成员按名排序后逗号拼接
        assert_eq!(groups[0].0, "na, ni, nr");
        assert_eq!(groups[0].1, "from @antfu/ni@30.6.0");
    }

    #[test]
    fn group_commands_different_descs_stay_separate() {
        let entries = vec![
            ("vite".to_string(), "from vite@8.3.4".to_string()),
            ("esbuild".to_string(), "from esbuild@0.24.0".to_string()),
        ];
        let (singles, groups) = group_commands(entries);
        assert_eq!(singles.len(), 2);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_commands_empty_desc_not_grouped() {
        // desc 为空的命令不聚合，避免无关命令因“都无描述”被合并
        let entries = vec![
            ("a".to_string(), String::new()),
            ("b".to_string(), String::new()),
        ];
        let (singles, groups) = group_commands(entries);
        assert_eq!(singles.len(), 2);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_commands_outputs_sorted() {
        let entries = vec![
            ("zeta".to_string(), "same desc".to_string()),
            ("npm".to_string(), "Manage npm".to_string()),
            ("alpha".to_string(), "same desc".to_string()),
            ("add".to_string(), "Add plugins".to_string()),
        ];
        let (singles, groups) = group_commands(entries);
        let single_names: Vec<&str> = singles.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(single_names, vec!["add", "npm"]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "alpha, zeta");
    }
}