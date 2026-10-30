//! 别名执行：环境构建、交互模式、执行入口。

use rustyline::DefaultEditor;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use super::parse::to_alias_definition;
use super::placeholder::{collect_placeholders, parse_alias_arguments_with_mapping, split_ternary};
use super::types::ResolvedAlias;

/// 把 `$paths` 里的相对条目按配置文件目录展开为绝对路径。
///
/// `build_alias_env` 仍会把相对条目按命令落地目录兜底解析，
/// 但在别名定义里写下的相对路径应当跟着配置文件走——否则从子目录
/// 调用 `byk` 时，这些目录会被静默跳过。
fn absolutize_paths(paths: &[String], config_dir: Option<&Path>) -> Vec<String> {
    paths
        .iter()
        .map(|raw| {
            let expanded = expand_home(raw);
            if expanded.is_relative()
                && let Some(base) = config_dir
            {
                return base.join(&expanded).to_string_lossy().into_owned();
            }
            expanded.to_string_lossy().into_owned()
        })
        .collect()
}

/// 构造别名执行环境，将自定义路径和 node_modules/.bin 前置到 PATH。
///
/// `custom_paths` 来自别名文件的 `$paths` 字段，支持 `~` 展开和基于
/// `working_dir` 的相对路径解析。非字符串元素已在扫描阶段过滤。
pub fn build_alias_env(working_dir: &Path, custom_paths: &[String]) -> HashMap<String, String> {
    let mut env_map = HashMap::new();
    let existing_path = std::env::var("PATH").unwrap_or_default();

    let mut prefixes: Vec<String> = Vec::new();

    // 解析并前置 $paths 条目（保持配置中的顺序，最前的优先级最高）
    for raw in custom_paths {
        let expanded = expand_home(raw);
        let resolved = if expanded.is_relative() {
            working_dir.join(&expanded)
        } else {
            expanded
        };
        if resolved.is_dir() {
            prefixes.push(resolved.to_string_lossy().into_owned());
        }
    }

    let node_bin = working_dir.join("node_modules").join(".bin");
    if node_bin.is_dir() {
        prefixes.push(node_bin.display().to_string());
    }

    if prefixes.is_empty() {
        env_map.insert("PATH".to_string(), existing_path);
    } else {
        #[cfg(windows)]
        let sep = ";";
        #[cfg(not(windows))]
        let sep = ":";
        let prepended = prefixes.join(sep);
        env_map.insert(
            "PATH".to_string(),
            format!("{}{}{}", prepended, sep, existing_path),
        );
    }
    env_map
}

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
    !inner.contains('?') && !inner.starts_with('$') && !inner.contains('{') && !inner.contains('}')
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
fn display_interactive_template(command: &str, placeholders: &[String]) {
    let raw = crate::utils::display::escape_for_display(command);
    let mut display = raw.clone();
    for ph in placeholders {
        let colored = format!("\x1b[33m{}\x1b[0m", ph);
        display = display.replace(ph.as_str(), &colored);
    }
    println!("~ {}", display);
}

/// 执行最终命令（平台适配：Windows 用 cmd /C，Unix 用 sh -c）。
fn run_command(final_command: &str, working_dir: &Path, custom_paths: &[String]) -> ! {
    let env_map = build_alias_env(working_dir, custom_paths);

    #[cfg(windows)]
    let final_command = convert_unix_path_for_cmd(final_command);

    #[cfg(not(windows))]
    let final_command = final_command.to_string();

    #[cfg(windows)]
    let status = Command::new("cmd")
        .arg("/C")
        .arg(&final_command)
        .current_dir(working_dir)
        .envs(&env_map)
        .status();

    #[cfg(not(windows))]
    let status = Command::new("sh")
        .arg("-c")
        .arg(&final_command)
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

/// 将命令中的 Unix 风格 `./` 前缀转为 Windows `.\`，使 cmd 可识别。
/// 仅转换非路径部分的前缀（如 `./pip` → `.\pip`）。
#[cfg(windows)]
fn convert_unix_path_for_cmd(command: &str) -> String {
    // 只转换命令开头的 ./（如 ./pip install → .\pip install）
    if let Some(stripped) = command.strip_prefix("./") {
        return format!(".\\{}", stripped);
    }
    command.to_string()
}

/// 执行别名命令。
///
/// 变量（内置 `${byk}` / `${workdir}` 与文件 `$var`）已在扫描期替换完毕，
/// 此处只剩参数占位符。按 `$interactive` 决定行为：
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

    let command = definition.command;
    let cwd = definition.cwd;

    // 相对 $paths 以配置文件所在目录为基准，与调用时的当前目录无关
    let config_dir = resolved.source_path.as_deref().and_then(|p| p.parent());
    let custom_paths = absolutize_paths(&resolved.paths, config_dir);

    let working_dir = resolve_working_dir(
        cwd.as_deref(),
        resolved.source_path.as_deref().and_then(|p| p.parent()),
    );
    let placeholders = collect_placeholders(&command);

    println!("\n> {} {}", display_source, working_dir.display());

    if definition.interactive {
        execute_interactive_impl(&command, args, &placeholders, &working_dir, &custom_paths);
    } else {
        execute_direct_impl(&command, args, &placeholders, &working_dir, &custom_paths);
    }
}

fn execute_direct_impl(
    command: &str,
    args: &[String],
    _placeholders: &[String],
    working_dir: &Path,
    custom_paths: &[String],
) -> ! {
    let (final_command, _) = parse_alias_arguments_with_mapping(command, args, &[]);
    println!("> {}\n", final_command);
    run_command(&final_command, working_dir, custom_paths);
}

fn execute_interactive_impl(
    command: &str,
    cli_args: &[String],
    placeholders: &[String],
    working_dir: &Path,
    custom_paths: &[String],
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

        run_command(&final_command, working_dir, custom_paths);
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

    run_command(&final_command, working_dir, custom_paths);
}

/// 展开路径开头的 `~` 为用户主目录，其余原样返回。
fn expand_home(raw: &str) -> PathBuf {
    match raw.strip_prefix('~') {
        Some(rest) => {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
            PathBuf::from(format!("{}{}", home.to_string_lossy(), rest))
        }
        None => PathBuf::from(raw),
    }
}

/// 解析工作目录，支持 `~` 展开和基于配置文件目录的相对路径解析。
fn resolve_working_dir(cwd: Option<&str>, base_dir: Option<&Path>) -> PathBuf {
    let raw = match cwd {
        Some(s) if !s.is_empty() => s,
        _ => return std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let path = expand_home(raw);

    // 相对路径以配置文件所在目录为基准解析
    if path.is_relative()
        && let Some(base) = base_dir
    {
        let joined = base.join(&path);
        // 规范化路径（消除 .. 等），失败时回退到拼接结果
        if let Ok(canonical) = joined.canonicalize() {
            return strip_extended_path_prefix(canonical);
        }
        return joined;
    }
    path
}

/// 在 Windows 上剥离 `\\?\` 扩展路径前缀，保持标准 DOS 路径。
/// 其他平台原样返回。
fn strip_extended_path_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
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

    #[test]
    fn resolve_cwd_none_returns_current_dir() {
        let result = resolve_working_dir(None, None);
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

    #[test]
    fn env_no_custom_paths_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let env = build_alias_env(dir.path(), &[]);
        let path = env.get("PATH").unwrap();
        let existing = std::env::var("PATH").unwrap_or_default();
        assert_eq!(path, &existing);
    }

    #[test]
    fn env_custom_paths_prepended() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();

        let env = build_alias_env(dir.path(), &[bin_dir.to_string_lossy().into_owned()]);
        let path = env.get("PATH").unwrap();
        assert!(path.starts_with(&bin_dir.to_string_lossy().into_owned()));
    }

    #[test]
    fn env_skips_nonexistent_custom_paths() {
        let dir = tempfile::tempdir().unwrap();
        let env = build_alias_env(
            dir.path(),
            &["/nonexistent/path/that/does/not/exist".to_string()],
        );
        let path = env.get("PATH").unwrap();
        assert!(!path.contains("/nonexistent/path"));
    }

    #[test]
    fn env_relative_custom_paths_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("scripts");
        std::fs::create_dir(&scripts).unwrap();

        let env = build_alias_env(dir.path(), &["scripts".to_string()]);
        let path = env.get("PATH").unwrap();
        assert!(path.contains(&scripts.to_string_lossy().into_owned()));
    }

    #[test]
    fn env_multiple_custom_paths_order_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();

        let env = build_alias_env(
            dir.path(),
            &[
                a.to_string_lossy().into_owned(),
                b.to_string_lossy().into_owned(),
            ],
        );
        let path = env.get("PATH").unwrap();
        let a_pos = path.find(&a.to_string_lossy().into_owned()).unwrap();
        let b_pos = path.find(&b.to_string_lossy().into_owned()).unwrap();
        assert!(a_pos < b_pos, "配置中排前的路径应更靠前");
    }

    #[test]
    fn env_node_modules_bin_appended_after_custom() {
        let dir = tempfile::tempdir().unwrap();
        let nm_bin = dir.path().join("node_modules").join(".bin");
        std::fs::create_dir_all(&nm_bin).unwrap();
        let custom = dir.path().join("custom");
        std::fs::create_dir(&custom).unwrap();

        let env = build_alias_env(dir.path(), &[custom.to_string_lossy().into_owned()]);
        let path = env.get("PATH").unwrap();
        let custom_pos = path.find(&custom.to_string_lossy().into_owned()).unwrap();
        let nm_pos = path.find(&nm_bin.to_string_lossy().into_owned()).unwrap();
        assert!(custom_pos < nm_pos, "$paths 应在 node_modules/.bin 之前");
    }
}
