/// 别名执行：危险命令检测、环境构建、交互模式、执行入口。

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use super::parse::to_alias_definition;
use super::placeholder::{collect_placeholders, parse_alias_arguments_with_mapping, split_ternary};
use super::types::ResolvedAlias;

// ---------------------------------------------------------------------------
// 危险命令检测（对应 Python `is_dangerous_command`）
// ---------------------------------------------------------------------------

/// 判断命令是否存在明显风险（rm -rf、git push -f、shutdown 等）。
#[allow(dead_code)]
pub fn is_dangerous_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    lower.contains("rm -rf")
        || lower.contains("rm -fr")
        || lower.contains("rm -r -f")
        || lower.contains("rm -f -r")
        || lower.contains("rm  -rf")
        || lower.contains("rm  -fr")
        || (lower.contains("git push") && (lower.contains(" -f") || lower.contains(" --force")))
        || lower.contains("shutdown")
        || lower.contains("reboot")
        || lower.starts_with("format ")
        || lower.starts_with("rd /s")
        || lower.starts_with("rd /q")
        || lower.starts_with("del /s")
        || lower.starts_with("del /q")
}

// ---------------------------------------------------------------------------
// 别名执行环境构建
// ---------------------------------------------------------------------------

/// 构造别名执行环境，将 node_modules/.bin 前置到 PATH。
pub fn build_alias_env(working_dir: &Path) -> HashMap<String, String> {
    let mut env_map = HashMap::new();
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let node_bin = working_dir.join("node_modules").join(".bin");

    if node_bin.is_dir() {
        env_map.insert(
            "PATH".to_string(),
            format!("{}:{}", node_bin.display(), existing_path),
        );
    } else {
        env_map.insert("PATH".to_string(), existing_path);
    }
    env_map
}

// ---------------------------------------------------------------------------
// 交互模式辅助函数
// ---------------------------------------------------------------------------

/// 判断占位符是否为具名占位符 `{xxx}`（单层花括号，不含 ?）。
fn is_named_placeholder(ph: &str) -> bool {
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
    !inner.contains('?')
        && !inner.starts_with('$')
        && !inner.contains('{')
        && !inner.contains('}')
}

/// 判断占位符是否为可选透传 `{{xxx}}`（双层花括号）。
fn is_optional_placeholder(ph: &str) -> bool {
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
fn is_positional_placeholder(ph: &str) -> bool {
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
fn is_conditional_placeholder(ph: &str) -> bool {
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
fn pre_fill_from_cli(
    positional: &[&String],
    conditional: &[&String],
    named: &[&String],
    cli_args: &[String],
) -> (HashMap<String, String>, Vec<String>) {
    let mut pre_filled: HashMap<String, String> = HashMap::new();
    let mut consumed: HashSet<usize> = HashSet::new();

    // 位置占位符 ${N}
    for ph in positional {
        let n = &ph[2..ph.len() - 1];
        if let Ok(idx) = n.parse::<usize>() {
            if idx < cli_args.len() && !consumed.contains(&idx) {
                pre_filled.insert((*ph).clone(), cli_args[idx].clone());
                consumed.insert(idx);
            }
        }
    }

    // 具名占位符 {xxx} / {{xxx}}
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

    // 条件占位符 {xxx?...}
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

// ---------------------------------------------------------------------------
// 共享输出辅助
// ---------------------------------------------------------------------------

/// 显示带高亮占位符的命令模板。
fn display_interactive_template(command: &str, placeholders: &[String]) {
    let raw = crate::utils::display::escape_for_display(command);
    let mut display = raw.clone();
    for ph in placeholders {
        let colored = format!("\x1b[33m{}\x1b[0m", ph);
        display = display.replace(ph.as_str(), &colored);
    }
    println!("~ {}", display);
}

/// 执行最终命令（sh -c）。
fn run_command(final_command: &str, working_dir: &Path) -> ! {
    let env_map = build_alias_env(working_dir);
    let status = Command::new("sh")
        .arg("-c")
        .arg(final_command)
        .current_dir(working_dir)
        .envs(&env_map)
        .status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to execute alias: {}", e);
            exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// 别名执行（统一入口）
// ---------------------------------------------------------------------------

/// 执行别名命令。
///
/// 根据别名定义中的 `$interactive` 字段决定行为：
/// - `false`（默认）：从 CLI 参数自动解析占位符，显示映射关系后直接执行。
/// - `true`：逐项提示用户输入占位符值，显示最终命令后按 Enter 确认执行。
pub fn execute_alias(resolved: &ResolvedAlias, args: &[String], display_source: &str) {
    let definition = match to_alias_definition(&resolved.value) {
        Some(d) => d,
        None => {
            eprintln!("Invalid alias definition");
            exit(1);
        }
    };

    let working_dir = resolve_working_dir(definition.cwd.as_deref(), resolved.source_path.as_deref());
    let placeholders = collect_placeholders(&definition.command);

    // --- 公共输出: header ---
    println!("\n> {} {}", display_source, working_dir.display());

    if definition.interactive {
        execute_interactive_impl(&definition.command, args, &placeholders, &working_dir);
    } else {
        execute_direct_impl(&definition.command, args, &placeholders, &working_dir);
    }
}

// ---------------------------------------------------------------------------
// 非交互执行
// ---------------------------------------------------------------------------

fn execute_direct_impl(
    command: &str,
    args: &[String],
    _placeholders: &[String],
    working_dir: &Path,
) -> ! {
    let (final_command, _) = parse_alias_arguments_with_mapping(command, args, &[]);
    println!("> {}\n", final_command);
    run_command(&final_command, working_dir);
}

// ---------------------------------------------------------------------------
// 交互执行
// ---------------------------------------------------------------------------

fn execute_interactive_impl(
    command: &str,
    cli_args: &[String],
    placeholders: &[String],
    working_dir: &Path,
) -> ! {
    // 零占位符：模板行自动追加 ${args}，始终显示 args: 信息
    let no_placeholders = placeholders.is_empty();
    let has_args_placeholder = placeholders.iter().any(|ph| ph == "${args}");
    let show_args_info = no_placeholders || has_args_placeholder;

    let (display_cmd, display_placeholders) = if no_placeholders {
        let d = format!("{} ${{args}}", command);
        (d, vec!["${args}".to_string()])
    } else {
        (command.to_string(), placeholders.to_vec())
    };

    // --- ② 模板行 ---
    display_interactive_template(&display_cmd, &display_placeholders);

    // --- 零占位符快捷路径 ---
    if no_placeholders {
        let args_str = if cli_args.is_empty() {
            "none".to_string()
        } else {
            cli_args.join(" ")
        };
        println!("  \x1b[33margs\x1b[0m: {}", args_str);

        let (final_command, _) = parse_alias_arguments_with_mapping(command, cli_args, &[]);
        println!("~ \x1b[1;32m{}\x1b[0m", final_command);

        print!("  Press Enter to execute...");
        std::io::stdout().flush().unwrap();
        let mut _buf = String::new();
        if std::io::stdin().read_line(&mut _buf).is_err() {
            println!("\nCancelled");
            exit(0);
        }
        println!();

        run_command(&final_command, working_dir);
    }

    // --- 有占位符: 逐项收集 ---

    // 分离占位符类型
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

    // 从 CLI 参数预填充
    let (pre_filled, remaining_cli) =
        pre_fill_from_cli(&positional, &conditional, &named, cli_args);

    let mut cmd = command.to_string();
    let mut interactive_args: Vec<String> = Vec::new();
    let mut resolved_map: HashMap<String, String> = HashMap::new();

    // --- ③ 输入区 ---

    // Step 1: ${N} 位置占位符
    for ph in &positional {
        if let Some(val) = pre_filled.get(ph.as_str()) {
            resolved_map.insert((*ph).clone(), val.clone());
            cmd = cmd.replace(ph.as_str(), val);
            println!("  \x1b[33m{}\x1b[0m: {}", ph, val);
            continue;
        }
        let n = &ph[2..ph.len() - 1];
        print!("  \x1b[33m${{{}}}\x1b[0m: ", n);
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            println!("\nCancelled");
            exit(0);
        }
        let input = input.trim().to_string();
        if !input.is_empty() {
            resolved_map.insert((*ph).clone(), input.clone());
            cmd = cmd.replace(ph.as_str(), &input);
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

        print!(
            "  [\x1b[33m{}\x1b[0m / {}] (y/N): ",
            true_branch, false_label
        );
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            println!("\nCancelled");
            exit(0);
        }
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

        print!("  \x1b[33m{}\x1b[0m: ", key);
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            println!("\nCancelled");
            exit(0);
        }
        let input = input.trim().to_string();
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
        print!("  \x1b[33m...args\x1b[0m: {}", hint);
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            println!("\nCancelled");
            exit(0);
        }
        for arg in input.trim().split_whitespace() {
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
        if let Some(val) = resolved_map.get(ph.as_str()) {
            if !val.is_empty() {
                pre_consumed.push(i);
                all_args.push(val.clone());
            }
        }
    }
    all_args.extend(interactive_args.clone());

    // --- args: 信息行 ---
    if show_args_info {
        let args_str = if all_args.is_empty() {
            "none".to_string()
        } else {
            all_args.join(" ")
        };
        println!("  \x1b[33margs\x1b[0m: {}", args_str);
    }

    // 构建最终命令：若 cmd 已无占位符则直接使用，否则通过 parse_alias_arguments_with_mapping 解析
    let remaining_placeholders = collect_placeholders(&cmd);
    let final_command = if remaining_placeholders.is_empty() {
        cmd.clone()
    } else {
        parse_alias_arguments_with_mapping(&cmd, &all_args, &pre_consumed).0
    };

    // --- ④ 最终命令 + 确认 ---
    println!("~ \x1b[1;32m{}\x1b[0m", final_command);
    print!("  Press Enter to execute...");
    std::io::stdout().flush().unwrap();
    let mut _buf = String::new();
    if std::io::stdin().read_line(&mut _buf).is_err() {
        println!("\nCancelled");
        exit(0);
    }
    println!();

    run_command(&final_command, working_dir);
}

/// 解析工作目录，支持 `~` 展开和基于配置文件目录的相对路径解析。
fn resolve_working_dir(cwd: Option<&str>, base_dir: Option<&Path>) -> PathBuf {
    let raw = match cwd {
        Some(s) if !s.is_empty() => s,
        _ => return std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let path = if raw.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        PathBuf::from(raw.replacen('~', &home.to_string_lossy(), 1))
    } else {
        PathBuf::from(raw)
    };

    // 相对路径以配置文件所在目录为基准解析
    if path.is_relative() {
        if let Some(base) = base_dir {
            let joined = base.join(&path);
            // 规范化路径（消除 .. 等），失败时回退到拼接结果
            if let Ok(canonical) = joined.canonicalize() {
                return canonical;
            }
            return joined;
        }
    }
    path
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== is_dangerous_command ====================

    #[test]
    fn dangerous_rm_rf_variants() {
        assert!(is_dangerous_command("rm -rf /tmp/test"));
        assert!(is_dangerous_command("rm -fr /tmp/test"));
        assert!(is_dangerous_command("rm -r -f /tmp/test"));
        assert!(is_dangerous_command("rm -f -r /tmp/test"));
        assert!(is_dangerous_command("rm  -rf /tmp/test"));
        assert!(is_dangerous_command("rm  -fr /tmp/test"));
    }

    #[test]
    fn dangerous_git_push_force() {
        assert!(is_dangerous_command("git push -f origin main"));
        assert!(is_dangerous_command("git push --force origin main"));
    }

    #[test]
    fn dangerous_shutdown_reboot() {
        assert!(is_dangerous_command("shutdown now"));
        assert!(is_dangerous_command("reboot"));
    }

    #[test]
    fn dangerous_format_del_rd() {
        assert!(is_dangerous_command("format c:"));
        assert!(is_dangerous_command("rd /s /q folder"));
        assert!(is_dangerous_command("del /s /q *.tmp"));
    }

    #[test]
    fn not_dangerous_normal_commands() {
        assert!(!is_dangerous_command("echo hello"));
        assert!(!is_dangerous_command("git push origin main"));
        assert!(!is_dangerous_command("npm start"));
        assert!(!is_dangerous_command("ls -la"));
    }

    #[test]
    fn not_dangerous_partial_match() {
        // "rm" 单独出现不算危险
        assert!(!is_dangerous_command("rm file.txt"));
        assert!(!is_dangerous_command("formatting text")); // "format " 带空格才算
    }

    // ==================== is_named_placeholder ====================

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

    // ==================== is_optional_placeholder ====================

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

    // ==================== is_positional_placeholder ====================

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

    // ==================== is_conditional_placeholder ====================

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

    // ==================== pre_fill_from_cli ====================

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
        // 只有一个 ${0} 被填
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
        // remaining 包含所有未被消费的 args
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
            "first".to_string(),   // idx 0 → ${0}
            "name".to_string(),    // idx 1 → key "name"
            "Alice".to_string(),   // idx 2 → value for {name}
            "debug".to_string(),   // idx 3 → key "debug"
            "leftover".to_string(),// idx 4 → remaining
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

    // ==================== resolve_working_dir ====================

    #[test]
    fn resolve_cwd_none_returns_current_dir() {
        let result = resolve_working_dir(None, None);
        // 返回当前工作目录
        assert!(result.is_absolute());
    }

    #[test]
    fn resolve_cwd_empty_returns_current_dir() {
        let result = resolve_working_dir(Some(""), None);
        assert!(result.is_absolute());
    }

    #[test]
    fn resolve_cwd_absolute_path() {
        let result = resolve_working_dir(Some("/tmp/project"), None);
        assert_eq!(result, PathBuf::from("/tmp/project"));
    }

    #[test]
    fn resolve_cwd_relative_with_base_dir() {
        let base = PathBuf::from("/home/user/config");
        let result = resolve_working_dir(Some("project"), Some(&base));
        assert_eq!(result, PathBuf::from("/home/user/config/project"));
    }

    #[test]
    fn resolve_cwd_relative_with_parent_dir() {
        let base = PathBuf::from("/home/user/config");
        let result = resolve_working_dir(Some("../project"), Some(&base));
        // ../project 从 /home/user/config 出发 → /home/user/project
        // canonicalize 在测试环境中可能返回实际路径，检查以 project 结尾
        let s = result.to_string_lossy();
        assert!(s.ends_with("project") || s.ends_with("project/"));
    }

    #[test]
    fn resolve_cwd_relative_no_base_dir() {
        // 无 base_dir 时相对路径直接返回（不解析）
        let result = resolve_working_dir(Some("relative/path"), None);
        assert_eq!(result, PathBuf::from("relative/path"));
    }
}
