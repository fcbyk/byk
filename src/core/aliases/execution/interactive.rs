//! 交互式占位符收集与确认。

use rustyline::DefaultEditor;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::exit;

use super::super::placeholder::{
    collect_placeholders, parse_alias_arguments_with_mapping, split_ternary,
};
use super::super::quote::quote;
use super::runner::run_command;

/// 判断占位符是否为具名占位符 `{xxx}`（单层花括号，不含 ?）。
pub(super) fn is_named_placeholder(ph: &str) -> bool {
    if ph.len() < 3 {
        return false;
    }
    let bytes = ph.as_bytes();
    if bytes[0] != b'{' || bytes[ph.len() - 1] != b'}' {
        return false;
    }
    // 排除 {{xxx}}
    if bytes[1] == b'{' {
        return false;
    }
    let inner = &ph[1..ph.len() - 1];
    !inner.contains('?') && !inner.starts_with('$') && !inner.contains('{') && !inner.contains('}')
}

/// 判断占位符是否为可选透传 `{{xxx}}`（双层花括号）。
pub(super) fn is_optional_placeholder(ph: &str) -> bool {
    if ph.len() < 5 {
        return false;
    }
    if !ph.starts_with("{{") || !ph.ends_with("}}") {
        return false;
    }
    let inner = &ph[2..ph.len() - 2];
    !inner.contains('?') && !inner.contains('{') && !inner.contains('}')
}

/// 判断占位符是否为位置占位符 `${N}`（N 为数字，不含 args 和 ...args）。
pub(super) fn is_positional_placeholder(ph: &str) -> bool {
    if !ph.starts_with("${") || !ph.ends_with('}') {
        return false;
    }
    let inner = &ph[2..ph.len() - 1];
    if inner == "args" || inner.starts_with("...") {
        return false;
    }
    inner.parse::<usize>().is_ok()
}

/// 判断占位符是否为条件渲染占位符 `{xxx?...}`。
pub(super) fn is_conditional_placeholder(ph: &str) -> bool {
    if ph.len() < 4 {
        return false;
    }
    let bytes = ph.as_bytes();
    if bytes[0] != b'{' || bytes[ph.len() - 1] != b'}' {
        return false;
    }
    // 排除 {{xxx}}
    if bytes[1] == b'{' {
        return false;
    }
    let inner = &ph[1..ph.len() - 1];
    inner.contains('?')
}

/// 从 CLI 参数预填充占位符值。
///
/// 返回 (占位符→值的映射, 剩余未消费的 CLI 参数)。
pub(super) fn pre_fill_from_cli(
    positional: &[&String],
    conditional: &[&String],
    named: &[&String],
    cli_args: &[String],
) -> (HashMap<String, String>, Vec<String>) {
    let mut pre_filled: HashMap<String, String> = HashMap::new();
    let mut consumed: HashSet<usize> = HashSet::new();

    for ph in positional {
        let n = &ph[2..ph.len() - 1];
        if let Ok(idx) = n.parse::<usize>()
            && idx < cli_args.len()
            && !consumed.contains(&idx)
        {
            pre_filled.insert((*ph).clone(), cli_args[idx].clone());
            consumed.insert(idx);
        }
    }

    for ph in named {
        let key = if is_named_placeholder(ph) {
            &ph[1..ph.len() - 1]
        } else {
            &ph[2..ph.len() - 2]
        };
        for (i, arg) in cli_args.iter().enumerate() {
            if arg == key && !consumed.contains(&i) {
                consumed.insert(i);
                if i + 1 < cli_args.len() && !consumed.contains(&(i + 1)) {
                    pre_filled.insert((*ph).clone(), cli_args[i + 1].clone());
                    consumed.insert(i + 1);
                } else {
                    pre_filled.insert((*ph).clone(), String::new());
                }
                break;
            }
        }
    }

    for ph in conditional {
        let inner = &ph[1..ph.len() - 1];
        let (key, true_branch, _) = split_ternary(inner);
        for (i, arg) in cli_args.iter().enumerate() {
            if *arg == key && !consumed.contains(&i) {
                pre_filled.insert((*ph).clone(), true_branch);
                consumed.insert(i);
                break;
            }
        }
    }

    // 剩余未消费的 CLI 参数 → 用于 ${args} / ${...args}
    let remaining: Vec<String> = cli_args
        .iter()
        .enumerate()
        .filter(|(i, _)| !consumed.contains(i))
        .map(|(_, a)| a.clone())
        .collect();

    (pre_filled, remaining)
}

/// 显示带高亮占位符的命令模板。
pub(super) fn display_interactive_template(command: &str, placeholders: &[String]) {
    let raw = crate::utils::display::escape_for_display(command);
    let mut display = raw.clone();
    for ph in placeholders {
        let colored = format!("\x1b[33m{}\x1b[0m", ph);
        display = display.replace(ph.as_str(), &colored);
    }
    println!("~ {}", display);
}

pub(super) fn execute_interactive_impl(
    command: &str,
    cli_args: &[String],
    placeholders: &[String],
    working_dir: &Path,
    custom_paths: &[String],
    alias_stack: &str,
) -> ! {
    // 零占位符：模板行自动追加 ${args}，始终显示 args: 信息
    let no_placeholders = placeholders.is_empty();
    let has_args_placeholder = placeholders.iter().any(|ph| ph == "${args}");
    let show_args_info = no_placeholders || has_args_placeholder;

    let mut rl = DefaultEditor::new().unwrap_or_else(|_| {
        eprintln!("Error: interactive mode requires a terminal");
        exit(1);
    });

    let (display_cmd, display_placeholders) = if no_placeholders {
        let d = format!("{} ${{args}}", command);
        (d, vec!["${args}".to_string()])
    } else {
        (command.to_string(), placeholders.to_vec())
    };

    display_interactive_template(&display_cmd, &display_placeholders);

    if no_placeholders {
        let args_str = if cli_args.is_empty() {
            "none".to_string()
        } else {
            cli_args.join(" ")
        };
        println!("  \x1b[33margs\x1b[0m: {}", args_str);

        let (final_command, _) = parse_alias_arguments_with_mapping(command, cli_args, &[]);
        println!("~ \x1b[1;32m{}\x1b[0m", final_command);

        if rl.readline("  Press Enter to execute...").is_err() {
            println!("\nCancelled");
            exit(0);
        }
        println!();

        run_command(&final_command, working_dir, custom_paths, alias_stack);
    }

    let positional: Vec<&String> = placeholders
        .iter()
        .filter(|ph| is_positional_placeholder(ph))
        .collect();
    let conditional: Vec<&String> = placeholders
        .iter()
        .filter(|ph| is_conditional_placeholder(ph))
        .collect();
    let named: Vec<&String> = placeholders
        .iter()
        .filter(|ph| is_named_placeholder(ph) || is_optional_placeholder(ph))
        .collect();
    let has_rest_args = placeholders.iter().any(|ph| ph == "${...args}");

    let (pre_filled, remaining_cli) =
        pre_fill_from_cli(&positional, &conditional, &named, cli_args);

    let mut cmd = command.to_string();
    let mut interactive_args: Vec<String> = Vec::new();
    let mut resolved_map: HashMap<String, String> = HashMap::new();

    // Step 1: ${N} 位置占位符
    for ph in &positional {
        if let Some(val) = pre_filled.get(ph.as_str()) {
            resolved_map.insert((*ph).clone(), val.clone());
            cmd = cmd.replace(ph.as_str(), val);
            println!("  \x1b[33m{}\x1b[0m: {}", ph, val);
            continue;
        }
        let n = &ph[2..ph.len() - 1];
        let input = match rl.readline(&format!("  ${{{}}}: ", n)) {
            Ok(line) => line.trim().to_string(),
            Err(_) => {
                println!("\nCancelled");
                exit(0);
            }
        };
        if !input.is_empty() {
            resolved_map.insert((*ph).clone(), input.clone());
            cmd = cmd.replace(ph.as_str(), &quote(&input));
        } else {
            resolved_map.insert((*ph).clone(), String::new());
            cmd = cmd.replace(ph.as_str(), "");
        }
    }

    // Step 2: {xxx?...} 条件占位符
    for ph in &conditional {
        if let Some(val) = pre_filled.get(ph.as_str()) {
            resolved_map.insert((*ph).clone(), val.clone());
            cmd = cmd.replace(ph.as_str(), val);
            println!("  \x1b[33m{}\x1b[0m: {}", ph, val);
            continue;
        }
        let inner = &ph[1..ph.len() - 1];
        let (_key, true_branch, false_branch) = split_ternary(inner);
        let false_label = if false_branch.is_empty() {
            "skip"
        } else {
            &false_branch
        };

        let input = match rl.readline(&format!("  [{}/{}] (y/N): ", true_branch, false_label)) {
            Ok(line) => line,
            Err(_) => {
                println!("\nCancelled");
                exit(0);
            }
        };
        let chosen = if input.trim().to_lowercase().starts_with('y') {
            true_branch.clone()
        } else {
            false_branch.clone()
        };
        resolved_map.insert((*ph).clone(), chosen.clone());
        cmd = cmd.replace(ph.as_str(), &chosen);
    }

    // Step 3: {xxx} 和 {{xxx}} 具名占位符
    for ph in &named {
        let key = if is_named_placeholder(ph) {
            &ph[1..ph.len() - 1]
        } else {
            &ph[2..ph.len() - 2]
        };

        if let Some(val) = pre_filled.get(ph.as_str()) {
            if !val.is_empty() {
                interactive_args.push(key.to_string());
                interactive_args.push(val.clone());
                println!("  \x1b[33m{}\x1b[0m: {}", ph, val);
                let resolved_val = if is_named_placeholder(ph) {
                    val.clone()
                } else {
                    format!("{} {}", key, val)
                };
                resolved_map.insert((*ph).clone(), resolved_val);
            }
            continue;
        }

        let input = match rl.readline(&format!("  {}: ", key)) {
            Ok(line) => line.trim().to_string(),
            Err(_) => {
                println!("\nCancelled");
                exit(0);
            }
        };
        if !input.is_empty() {
            interactive_args.push(key.to_string());
            interactive_args.push(input.clone());
            let resolved_val = if is_named_placeholder(ph) {
                input.clone()
            } else {
                format!("{} {}", key, input)
            };
            resolved_map.insert((*ph).clone(), resolved_val);
        }
    }

    // Step 4: ${...args} 剩余参数
    if has_rest_args {
        for arg in &remaining_cli {
            if !arg.is_empty() {
                interactive_args.push(arg.clone());
            }
        }

        let cli_str: Vec<String> = remaining_cli
            .iter()
            .filter(|a| !a.is_empty())
            .cloned()
            .collect();
        let hint = if !cli_str.is_empty() {
            format!("{} ", cli_str.join(" "))
        } else {
            String::new()
        };
        let input = match rl.readline(&format!("  ...args: {}", hint)) {
            Ok(line) => line,
            Err(_) => {
                println!("\nCancelled");
                exit(0);
            }
        };
        for arg in input.split_whitespace() {
            if !arg.is_empty() {
                interactive_args.push(arg.to_string());
            }
        }
    } else {
        // 无 ${...args}，注入 CLI 剩余参数供 ${args} 计算
        for arg in &remaining_cli {
            if !arg.is_empty() {
                interactive_args.push(arg.clone());
            }
        }
    }

    // 构建完整参数列表，用于 ${args} 解析
    let mut all_args: Vec<String> = Vec::new();
    let mut pre_consumed: Vec<usize> = Vec::new();
    for (i, ph) in positional.iter().enumerate() {
        if let Some(val) = resolved_map.get(ph.as_str())
            && !val.is_empty()
        {
            pre_consumed.push(i);
            all_args.push(val.clone());
        }
    }
    all_args.extend(interactive_args.clone());

    if show_args_info {
        let args_str = if all_args.is_empty() {
            "none".to_string()
        } else {
            all_args.join(" ")
        };
        println!("  \x1b[33margs\x1b[0m: {}", args_str);
    }

    let remaining_placeholders = collect_placeholders(&cmd);
    let final_command = if remaining_placeholders.is_empty() {
        cmd.clone()
    } else {
        parse_alias_arguments_with_mapping(&cmd, &all_args, &pre_consumed).0
    };

    println!("~ \x1b[1;32m{}\x1b[0m", final_command);
    if rl.readline("  Press Enter to execute...").is_err() {
        println!("\nCancelled");
        exit(0);
    }
    println!();

    run_command(&final_command, working_dir, custom_paths, alias_stack);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_placeholder_valid() {
        assert!(is_named_placeholder("{name}"));
        assert!(is_named_placeholder("{arg}"));
    }

    #[test]
    fn named_placeholder_too_short() {
        assert!(!is_named_placeholder("{}"));
        assert!(!is_named_placeholder("{a"));
        assert!(!is_named_placeholder("a}"));
    }

    #[test]
    fn named_placeholder_rejects_double_curly() {
        assert!(!is_named_placeholder("{{name}}"));
    }

    #[test]
    fn named_placeholder_rejects_dollar() {
        assert!(!is_named_placeholder("${args}"));
    }

    #[test]
    fn named_placeholder_rejects_ternary() {
        assert!(!is_named_placeholder("{debug?yes:no}"));
    }

    #[test]
    fn named_placeholder_rejects_nested() {
        assert!(!is_named_placeholder("{a{b}}"));
    }

    #[test]
    fn optional_placeholder_valid() {
        assert!(is_optional_placeholder("{{name}}"));
        assert!(is_optional_placeholder("{{flag}}"));
    }

    #[test]
    fn optional_placeholder_too_short() {
        // {{}} 只有 4 个字符，不满足 >=5 的要求
        assert!(!is_optional_placeholder("{{}}"));
        assert!(!is_optional_placeholder("{x}"));
    }

    #[test]
    fn optional_placeholder_rejects_ternary() {
        assert!(!is_optional_placeholder("{{debug?yes:no}}"));
    }

    #[test]
    fn positional_placeholder_valid() {
        assert!(is_positional_placeholder("${0}"));
        assert!(is_positional_placeholder("${1}"));
        assert!(is_positional_placeholder("${99}"));
    }

    #[test]
    fn positional_placeholder_rejects_args() {
        assert!(!is_positional_placeholder("${args}"));
    }

    #[test]
    fn positional_placeholder_rejects_rest_args() {
        assert!(!is_positional_placeholder("${...args}"));
    }

    #[test]
    fn positional_placeholder_rejects_non_numeric() {
        assert!(!is_positional_placeholder("${name}"));
    }

    #[test]
    fn conditional_placeholder_valid() {
        assert!(is_conditional_placeholder("{debug?--verbose:}"));
        assert!(is_conditional_placeholder("{flag?yes:no}"));
    }

    #[test]
    fn conditional_placeholder_too_short() {
        // {?} 只有 3 个字符，不满足 >=4 的要求
        assert!(!is_conditional_placeholder("{?}"));
        // {a} 不含 ?，不是条件占位符
        assert!(!is_conditional_placeholder("{a}"));
    }

    #[test]
    fn conditional_placeholder_rejects_double_curly() {
        assert!(!is_conditional_placeholder("{{debug?yes:no}}"));
    }

    #[test]
    fn conditional_placeholder_rejects_no_question() {
        assert!(!is_conditional_placeholder("{name}"));
    }

    #[test]
    fn pre_fill_empty_all() {
        let args: Vec<String> = vec![];
        let positional: Vec<&String> = vec![];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert!(filled.is_empty());
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_empty_cli_args() {
        let args: Vec<String> = vec![];
        let p0 = "${0}".to_string();
        let positional = vec![&p0];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // ${0} 索引 0 超出 args 范围，不会填充
        assert!(!filled.contains_key("${0}"));
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_positional_single() {
        let args = vec!["hello".to_string()];
        let p0 = "${0}".to_string();
        let positional = vec![&p0];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert_eq!(filled.get("${0}").unwrap(), "hello");
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_positional_out_of_range_skipped() {
        let args = vec!["a".to_string()];
        let p0 = "${0}".to_string();
        let p5 = "${5}".to_string();
        let positional = vec![&p0, &p5];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // ${0} 匹配，${5} 超出范围被跳过
        assert_eq!(filled.get("${0}").unwrap(), "a");
        assert!(!filled.contains_key("${5}"));
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_positional_multiple() {
        let args = vec!["first".to_string(), "second".to_string()];
        let p0 = "${0}".to_string();
        let p1 = "${1}".to_string();
        let positional = vec![&p0, &p1];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert_eq!(filled.get("${0}").unwrap(), "first");
        assert_eq!(filled.get("${1}").unwrap(), "second");
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_positional_no_duplicate_consumption() {
        // 同一个索引出现两次位置占位符时，后出现的不会重复消费
        let args = vec!["a".to_string()];
        let p0a = "${0}".to_string();
        let p0b = "${0}".to_string(); // 另一个 ${0}
        let positional = vec![&p0a, &p0b];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // 第一个 ${0} 消费了索引 0，第二个 ${0} 被 consumed 挡住了
        assert_eq!(filled.get("${0}").unwrap(), "a");
        assert_eq!(filled.len(), 1);
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_named_simple() {
        let args = vec!["name".to_string(), "Alice".to_string()];
        let named_str = "{name}".to_string();
        let named = vec![&named_str];
        let positional: Vec<&String> = vec![];
        let conditional: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert_eq!(filled.get("{name}").unwrap(), "Alice");
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_named_no_value_after_key() {
        // 具名占位符匹配到 key，但 key 是最后一个参数，没有 value
        let args = vec!["name".to_string()];
        let named_str = "{name}".to_string();
        let named = vec![&named_str];
        let positional: Vec<&String> = vec![];
        let conditional: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert_eq!(filled.get("{name}").unwrap(), "");
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_named_key_not_found() {
        let args = vec!["other".to_string(), "value".to_string()];
        let named_str = "{name}".to_string();
        let named = vec![&named_str];
        let positional: Vec<&String> = vec![];
        let conditional: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert!(filled.is_empty());
        assert_eq!(remaining, vec!["other", "value"]);
    }

    #[test]
    fn pre_fill_named_optional_placeholder() {
        let args = vec!["flag".to_string(), "on".to_string()];
        let named_str = "{{flag}}".to_string();
        let named = vec![&named_str];
        let positional: Vec<&String> = vec![];
        let conditional: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert_eq!(filled.get("{{flag}}").unwrap(), "on");
        assert!(remaining.is_empty());
    }

    #[test]
    fn pre_fill_named_skips_already_consumed() {
        // 位置占位符先消费了索引 0，具名占位符不应重复消费
        let args = vec!["a".to_string(), "b".to_string()];
        let p0 = "${0}".to_string();
        let named_str = "{a}".to_string(); // key 恰好是 "a"，和已消费的相同
        let positional = vec![&p0];
        let named = vec![&named_str];
        let conditional: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // ${0} 消费了 "a"
        assert_eq!(filled.get("${0}").unwrap(), "a");
        // {a} 试图匹配 key "a"，但索引 0 已消费，所以找下一个，索引 1="b" 不匹配
        assert!(!filled.contains_key("{a}"));
        assert_eq!(remaining, vec!["b"]);
    }

    #[test]
    fn pre_fill_conditional_matched() {
        let args = vec!["debug".to_string(), "extra".to_string()];
        let cond_str = "{debug?--verbose:}".to_string();
        let conditional = vec![&cond_str];
        let positional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // debug 匹配，true_branch = "--verbose"
        assert_eq!(filled.get("{debug?--verbose:}").unwrap(), "--verbose");
        assert_eq!(remaining, vec!["extra"]);
    }

    #[test]
    fn pre_fill_conditional_not_matched() {
        let args = vec!["release".to_string()];
        let cond_str = "{debug?--verbose:}".to_string();
        let conditional = vec![&cond_str];
        let positional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // debug 不匹配
        assert!(!filled.contains_key("{debug?--verbose:}"));
        assert_eq!(remaining, vec!["release"]);
    }

    #[test]
    fn pre_fill_conditional_key_already_consumed_by_positional() {
        let args = vec!["debug".to_string(), "extra".to_string()];
        let p0 = "${0}".to_string();
        let cond_str = "{debug?--verbose:}".to_string();
        let positional = vec![&p0];
        let conditional = vec![&cond_str];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        // ${0} 消费了 "debug"，条件占位符的 key 匹配但索引 0 已消费
        assert_eq!(filled.get("${0}").unwrap(), "debug");
        assert!(!filled.contains_key("{debug?--verbose:}"));
        assert_eq!(remaining, vec!["extra"]);
    }

    #[test]
    fn pre_fill_mixed_all_types() {
        let args = vec![
            "first".to_string(),    // idx 0 → ${0}
            "name".to_string(),     // idx 1 → key "name"
            "Alice".to_string(),    // idx 2 → value for {name}
            "debug".to_string(),    // idx 3 → key "debug"
            "leftover".to_string(), // idx 4 → remaining
        ];
        let p0 = "${0}".to_string();
        let named_str = "{name}".to_string();
        let cond_str = "{debug?--verbose:}".to_string();
        let positional = vec![&p0];
        let named = vec![&named_str];
        let conditional = vec![&cond_str];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert_eq!(filled.get("${0}").unwrap(), "first");
        assert_eq!(filled.get("{name}").unwrap(), "Alice");
        assert_eq!(filled.get("{debug?--verbose:}").unwrap(), "--verbose");
        assert_eq!(remaining, vec!["leftover"]);
    }

    #[test]
    fn pre_fill_remaining_includes_unmatched_args() {
        let args = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let positional: Vec<&String> = vec![];
        let conditional: Vec<&String> = vec![];
        let named: Vec<&String> = vec![];
        let (filled, remaining) = pre_fill_from_cli(&positional, &conditional, &named, &args);
        assert!(filled.is_empty());
        assert_eq!(remaining, vec!["x", "y", "z"]);
    }
}
