//! 占位符解析：模板占位符收集、条件渲染、参数解析。
//!
//! 支持四类占位符：
//! - `{xxx}`    — 具名占位符，匹配 argv 中 xxx 后面的值
//! - `{{xxx}}`  — 可选透传，有值则渲染 "xxx value"，无则消失
//! - `{xxx?}`   — 条件渲染，匹配时渲染真分支
//! - `${args}` / `${N}` / `${...args}` — 系统占位符

use std::collections::{HashMap, HashSet};

use super::quote::{join, normalize_spaces, quote};

/// 收集模板中所有唯一占位符，保持首次出现顺序。
///
/// 通过括号计数处理嵌套占位符（如 `{a?{b?x:y}:z}`），
/// 对 `${...}` 系列用独立规则避免与 `{` 计数混淆。
pub fn collect_placeholders(template: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '$' && i + 1 < len && chars[i + 1] == '{' {
            let start = i;
            let mut j = i + 2;
            while j < len && chars[j] != '}' {
                j += 1;
            }
            if j < len {
                let ph: String = chars[start..=j].iter().collect();
                if seen.insert(ph.clone()) {
                    result.push(ph);
                }
                i = j + 1;
                continue;
            }
        } else if chars[i] == '{' && i + 1 < len {
            let start = i;
            let mut depth = 1;
            let mut j = i + 1;
            while j < len && depth > 0 {
                if chars[j] == '{' {
                    depth += 1;
                } else if chars[j] == '}' {
                    depth -= 1;
                }
                j += 1;
            }
            let ph: String = chars[start..j].iter().collect();
            if seen.insert(ph.clone()) {
                result.push(ph.clone());
            }
            // 递归扫描内部占位符（{{...}} 除外）
            let is_double = ph.starts_with("{{") && ph.ends_with("}}");
            if !is_double && j >= start + 3 {
                let inner: String = chars[start + 1..j - 1].iter().collect();
                for iph in collect_placeholders(&inner) {
                    if seen.insert(iph.clone()) {
                        result.push(iph);
                    }
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    result
}

pub(crate) fn split_ternary(s: &str) -> (String, String, String) {
    let q_idx = match s.find('?') {
        Some(i) => i,
        None => return (s.to_string(), String::new(), String::new()),
    };
    let key = s[..q_idx].to_string();
    let branches = &s[q_idx + 1..];

    if branches.is_empty() {
        return (key.clone(), key.clone(), String::new());
    }

    let chars: Vec<char> = branches.chars().collect();
    let mut depth: i32 = 0;
    for (i, ch) in chars.iter().enumerate() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ':' if depth == 0 => {
                return (
                    key,
                    branches[..i].to_string(),
                    branches[i + 1..].to_string(),
                );
            }
            _ => {}
        }
    }

    (key, branches.to_string(), String::new())
}

/// 展开占位符值里引用的其他占位符，带循环检测。
fn resolve_nested(value: &str, resolved: &HashMap<String, String>) -> String {
    let mut visited = HashSet::new();
    resolve_nested_impl(value, resolved, &mut visited)
}

fn resolve_nested_impl(
    value: &str,
    resolved: &HashMap<String, String>,
    visited: &mut HashSet<String>,
) -> String {
    let mut result = value.to_string();
    let mut keys: Vec<&String> = resolved.keys().collect();
    keys.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for placeholder in &keys {
        if result.contains(*placeholder) {
            if !visited.insert((*placeholder).clone()) {
                continue; // 循环引用，跳过
            }
            let replacement = resolve_nested_impl(&resolved[*placeholder], resolved, visited);
            result = result.replace(*placeholder, &replacement);
        }
    }
    result
}

/// 解析单个占位符（`${...args}` 除外），结果写入 resolved。
///
/// 落到 resolved 里的字符串分两类：用户输入（`${N}` / `{xxx}` / `{{xxx}}` /
/// `${args}`）在插入时就已经按 shell 规则包裹好，并登记进 `literal`；模板
/// 片段（三元分支文本）保持原样，留给后面的嵌套展开。
fn resolve_placeholder(
    ph: &str,
    args: &[String],
    consumed: &mut HashSet<usize>,
    resolved: &mut HashMap<String, String>,
    literal: &mut HashSet<String>,
) {
    // {} 是 find 命令语法（find . -exec ... {} +），不是别名占位符
    if ph == "{}" {
        return;
    }

    if ph == "${args}" {
        resolved.insert(ph.to_string(), join(args));
        literal.insert(ph.to_string());
        return;
    }

    if ph.starts_with("${") && ph.ends_with('}') {
        let inner = &ph[2..ph.len() - 1];
        // `${name}` 是变量语法（内置或未定义），归变量层处理；
        // 这里只认位置参数 `${N}`，其余原样保留，避免把未定义变量清空。
        if let Ok(idx) = inner.parse::<usize>() {
            if idx < args.len() {
                resolved.insert(ph.to_string(), quote(&args[idx]));
                literal.insert(ph.to_string());
                consumed.insert(idx);
            } else {
                resolved.insert(ph.to_string(), String::new());
            }
        }
        return;
    }

    if ph.starts_with("{{") && ph.ends_with("}}") {
        let key = &ph[2..ph.len() - 2];
        for (i, arg) in args.iter().enumerate() {
            if arg == key {
                consumed.insert(i);
                let rendered = if i + 1 < args.len() {
                    consumed.insert(i + 1);
                    format!("{} {}", key, quote(&args[i + 1]))
                } else {
                    key.to_string()
                };
                resolved.insert(ph.to_string(), rendered);
                literal.insert(ph.to_string());
                return;
            }
        }
        resolved.insert(ph.to_string(), String::new());
        return;
    }

    if ph.contains('?') {
        let inner = &ph[1..ph.len() - 1];
        let (key, true_branch, false_branch) = split_ternary(inner);
        for (i, arg) in args.iter().enumerate() {
            if *arg == key {
                consumed.insert(i);
                resolved.insert(ph.to_string(), true_branch);
                return;
            }
        }
        resolved.insert(ph.to_string(), false_branch);
        return;
    }

    let key = &ph[1..ph.len() - 1];
    for (i, arg) in args.iter().enumerate() {
        if arg == key {
            consumed.insert(i);
            if i + 1 < args.len() {
                resolved.insert(ph.to_string(), quote(&args[i + 1]));
                literal.insert(ph.to_string());
                consumed.insert(i + 1);
            } else {
                resolved.insert(ph.to_string(), String::new());
            }
            return;
        }
    }
    resolved.insert(ph.to_string(), String::new());
}

/// 解析占位符，返回最终命令与「占位符 → 值」映射（交互模式回显用）。
///
/// 命令里没有占位符时自动追加所有参数，等价于隐式 `${args}`。
///
/// 返回的命令可以直接交给 `sh -c`：参数值在插入时已按 shell 规则包裹，
/// 边界（空格、空串、`;`、通配符、反斜杠）不会丢。
pub fn parse_alias_arguments_with_mapping(
    command: &str,
    args: &[String],
    pre_consumed: &[usize],
) -> (String, HashMap<String, String>) {
    let placeholders = collect_placeholders(command);

    // 无占位符 → 自动追加所有参数
    if placeholders.is_empty() {
        if args.is_empty() {
            return (command.to_string(), HashMap::new());
        }
        return (format!("{} {}", command, join(args)), HashMap::new());
    }

    let mut consumed: HashSet<usize> = pre_consumed.iter().cloned().collect();
    let mut resolved: HashMap<String, String> = HashMap::new();
    let mut literal: HashSet<String> = HashSet::new();
    let mut has_rest = false;

    // 第一趟：解析所有占位符
    for ph in &placeholders {
        if ph == "${...args}" {
            has_rest = true;
            continue;
        }
        resolve_placeholder(ph, args, &mut consumed, &mut resolved, &mut literal);
    }

    // ${...args} 依赖消费集合，最后计算
    if has_rest {
        let rest: Vec<String> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| !consumed.contains(i))
            .map(|(_, a)| a.clone())
            .collect();
        resolved.insert("${...args}".to_string(), join(&rest));
        literal.insert("${...args}".to_string());
    }

    // 递归展开 resolved 中的嵌套占位符；用户输入不参与——那是原文，
    // 展开它等于把参数内容当成模板再执行一遍。
    let keys: Vec<String> = resolved.keys().cloned().collect();
    for key in &keys {
        if literal.contains(key) {
            continue;
        }
        let expanded = resolve_nested(&resolved[key], &resolved);
        resolved.insert(key.clone(), expanded);
    }

    // 第二趟：模板替换，长的先替换
    let mut result = command.to_string();
    let mut sorted: Vec<&String> = resolved.keys().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for ph in &sorted {
        result = result.replace(*ph, &resolved[*ph]);
    }

    (normalize_spaces(&result), resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_named_placeholder() {
        let result = collect_placeholders("echo {name}");
        assert_eq!(result, vec!["{name}"]);
    }

    #[test]
    fn collect_multiple_placeholders() {
        let result = collect_placeholders("{cmd} --flag {arg}");
        assert_eq!(result, vec!["{cmd}", "{arg}"]);
    }

    #[test]
    fn collect_dollar_args() {
        let result = collect_placeholders("echo ${args}");
        assert_eq!(result, vec!["${args}"]);
    }

    #[test]
    fn collect_dollar_positional() {
        let result = collect_placeholders("cp ${0} ${1}");
        assert_eq!(result, vec!["${0}", "${1}"]);
    }

    #[test]
    fn collect_dollar_rest_args() {
        let result = collect_placeholders("run ${...args}");
        assert_eq!(result, vec!["${...args}"]);
    }

    #[test]
    fn collect_optional_placeholder() {
        let result = collect_placeholders("echo {{name}}");
        assert_eq!(result, vec!["{{name}}"]);
    }

    #[test]
    fn collect_conditional_placeholder() {
        let result = collect_placeholders("{debug?--verbose:}");
        assert_eq!(result, vec!["{debug?--verbose:}"]);
    }

    #[test]
    fn collect_nested_placeholder() {
        let result = collect_placeholders("{a?{b}:{c}}");
        // 外层 {a?{b}:{c}} + 内层 {b} 和 {c}
        assert!(result.contains(&"{a?{b}:{c}}".to_string()));
        assert!(result.contains(&"{b}".to_string()));
        assert!(result.contains(&"{c}".to_string()));
    }

    #[test]
    fn collect_deduplicates() {
        let result = collect_placeholders("{x} {x} {y}");
        assert_eq!(result, vec!["{x}", "{y}"]);
    }

    #[test]
    fn collect_skip_curly_find_syntax() {
        // {} 会被 collect_placeholders 收集（跳过逻辑在 resolve_placeholder 中处理）
        let result = collect_placeholders("find . -exec {} \\;");
        assert_eq!(result, vec!["{}"]);
    }

    #[test]
    fn collect_double_curly_no_recursion() {
        // {{...}} 内部不递归扫描
        let result = collect_placeholders("echo {{name}}");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "{{name}}");
    }

    #[test]
    fn collect_no_placeholders() {
        assert!(collect_placeholders("just a command").is_empty());
    }

    #[test]
    fn collect_empty_template() {
        assert!(collect_placeholders("").is_empty());
    }

    #[test]
    fn split_no_question_mark() {
        let (key, t, f) = split_ternary("simple");
        assert_eq!(key, "simple");
        assert!(t.is_empty());
        assert!(f.is_empty());
    }

    #[test]
    fn split_simple_ternary() {
        let (key, t, f) = split_ternary("debug?--verbose:");
        assert_eq!(key, "debug");
        assert_eq!(t, "--verbose");
        assert!(f.is_empty());
    }

    #[test]
    fn split_ternary_with_false_branch() {
        let (key, t, f) = split_ternary("debug?--verbose:--quiet");
        assert_eq!(key, "debug");
        assert_eq!(t, "--verbose");
        assert_eq!(f, "--quiet");
    }

    #[test]
    fn split_empty_branches() {
        let (key, t, f) = split_ternary("flag?");
        assert_eq!(key, "flag");
        assert_eq!(t, "flag"); // 空分支 = key
        assert!(f.is_empty());
    }

    #[test]
    fn split_nested_braces() {
        let (key, t, f) = split_ternary("a?{b?x:y}:{c}");
        assert_eq!(key, "a");
        assert_eq!(t, "{b?x:y}");
        assert_eq!(f, "{c}");
    }

    #[test]
    fn split_no_colon() {
        let (key, t, f) = split_ternary("flag?value");
        assert_eq!(key, "flag");
        assert_eq!(t, "value");
        assert!(f.is_empty());
    }

    #[test]
    fn parse_no_placeholders_appends_args() {
        let (cmd, map) =
            parse_alias_arguments_with_mapping("echo", &["hello".into(), "world".into()], &[]);
        assert_eq!(cmd, "echo hello world");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_no_placeholders_no_args() {
        let (cmd, map) = parse_alias_arguments_with_mapping("ls", &[], &[]);
        assert_eq!(cmd, "ls");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_dollar_args() {
        let (cmd, map) = parse_alias_arguments_with_mapping(
            "echo ${args}",
            &["hello".into(), "world".into()],
            &[],
        );
        assert_eq!(cmd, "echo hello world");
        assert_eq!(map.get("${args}").unwrap(), "hello world");
    }

    #[test]
    fn parse_positional() {
        let (cmd, _) =
            parse_alias_arguments_with_mapping("cp ${0} ${1}", &["src".into(), "dst".into()], &[]);
        assert_eq!(cmd, "cp src dst");
    }

    #[test]
    fn parse_undefined_named_var_kept() {
        // `${NAME}` 属于变量语法，占位符层原样保留（交给 shell 或保持字面）
        let (cmd, map) = parse_alias_arguments_with_mapping("echo ${UNDEF}", &[], &[]);
        assert_eq!(cmd, "echo ${UNDEF}");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_positional_out_of_range() {
        let (cmd, _) = parse_alias_arguments_with_mapping("echo ${5}", &["a".into()], &[]);
        assert_eq!(cmd, "echo");
    }

    #[test]
    fn parse_named_placeholder() {
        let (cmd, _) =
            parse_alias_arguments_with_mapping("{cmd}", &["cmd".into(), "build".into()], &[]);
        assert_eq!(cmd, "build");
    }

    #[test]
    fn parse_named_placeholder_not_matched() {
        let (cmd, _) = parse_alias_arguments_with_mapping("{missing}", &["other".into()], &[]);
        assert_eq!(cmd, "");
    }

    #[test]
    fn parse_conditional_true() {
        let (cmd, _) =
            parse_alias_arguments_with_mapping("{debug?--verbose:--quiet}", &["debug".into()], &[]);
        assert_eq!(cmd, "--verbose");
    }

    #[test]
    fn parse_conditional_false() {
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "{debug?--verbose:--quiet}",
            &["release".into()],
            &[],
        );
        assert_eq!(cmd, "--quiet");
    }

    #[test]
    fn parse_dollar_rest_args() {
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "run ${...args}",
            &["a".into(), "b".into(), "c".into()],
            &[],
        );
        assert_eq!(cmd, "run a b c");
    }

    #[test]
    fn parse_optional_placeholder_matched() {
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "echo {{name}}",
            &["name".into(), "Alice".into()],
            &[],
        );
        assert_eq!(cmd, "echo name Alice");
    }

    #[test]
    fn parse_optional_placeholder_not_matched() {
        let (cmd, _) = parse_alias_arguments_with_mapping("echo {{name}}", &[], &[]);
        assert_eq!(cmd, "echo");
    }

    #[test]
    fn parse_no_placeholders_quotes_args() {
        // 空格、空串、`;`、通配符都必须落在引号里
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "probe",
            &["my file.json".into(), String::new(), "a; rm -rf /".into()],
            &[],
        );
        // 引号形态随平台变化（POSIX 单引号 / cmd 双引号），期望值必须跟着走，
        // 否则这条测试只能在 Linux 上过。
        assert_eq!(
            cmd,
            format!(
                "probe {} {} {}",
                quote("my file.json"),
                quote(""),
                quote("a; rm -rf /")
            )
        );
    }

    #[test]
    fn parse_dollar_args_quotes_each_element() {
        let (cmd, _) =
            parse_alias_arguments_with_mapping("probe ${args}", &["a b".into(), "c".into()], &[]);
        assert_eq!(cmd, format!("probe {} c", quote("a b")));
    }

    #[test]
    fn parse_positional_quote() {
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "cp ${0} ${1}",
            &["my file".into(), "dst".into()],
            &[],
        );
        assert_eq!(cmd, format!("cp {} dst", quote("my file")));
    }

    #[test]
    fn parse_named_placeholder_quote() {
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "cp {--src} /tmp",
            &["--src".into(), "a b".into()],
            &[],
        );
        assert_eq!(cmd, format!("cp {} /tmp", quote("a b")));
    }

    #[test]
    fn parse_ternary_branch_is_not_quoted() {
        // 分支是模板文本，不是用户输入
        let (cmd, _) = parse_alias_arguments_with_mapping(
            "jest {--coverage?--coverage:--no-coverage}",
            &["--coverage".into()],
            &[],
        );
        assert_eq!(cmd, "jest --coverage");
    }

    /// 回归：参数边界必须原样穿过 `sh -c`。
    ///
    /// 旧实现把参数 `join(" ")` 后交给 `sh -c`，于是 `my file.json` 变成两个参数、
    /// 空串消失、`;` 后面被当成新命令执行。
    #[cfg(unix)]
    #[test]
    fn arguments_survive_sh_c() {
        let args: Vec<String> = vec![
            "my file.json".into(),
            String::new(),
            "a; echo INJECTED".into(),
            "*.json".into(),
            "it's".into(),
            r"C:\tmp\x".into(),
        ];
        let (cmd, _) = parse_alias_arguments_with_mapping("printf '<%s>\\n'", &args, &[]);

        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .unwrap();

        assert!(
            out.status.success(),
            "sh failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "<my file.json>\n<>\n<a; echo INJECTED>\n<*.json>\n<it's>\n<C:\\tmp\\x>\n"
        );
    }

    /// `${...args}` 只带走未被消耗的参数，且同样保持边界。
    #[cfg(unix)]
    #[test]
    fn rest_args_survive_sh_c() {
        let args: Vec<String> = vec!["--mode".into(), "dev".into(), "a b".into(), String::new()];
        let (cmd, _) =
            parse_alias_arguments_with_mapping("printf '<%s>\\n' {--mode} ${...args}", &args, &[]);

        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .unwrap();

        assert!(
            out.status.success(),
            "sh failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "<dev>\n<a b>\n<>\n");
    }
}
