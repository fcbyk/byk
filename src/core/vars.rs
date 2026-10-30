//! 文件级变量替换（扫描期一次性完成）。
//!
//! 设计目标：**一张表、一个语法、一次替换**。
//!
//! - 表：内置变量（`byk` / `workdir`）与文件 `$var` 声明合并，声明覆盖内置；
//! - 语法：`${name}`，`name` 为 `[A-Za-z_][A-Za-z0-9_]*`（与 shell 标识符一致）；
//! - 裸 `$xx` 不再识别，原样透传给 shell（`$HOME`、`$PATH` 交给 shell 自己展开）；
//! - `\${...}` 输出字面量 `${...}`；
//! - 未命中或名字非法一律原样保留，不报错。
//!
//! 解析分两遍，都不依赖 `$var` 的书写顺序：
//!
//! 1. 内置名（`byk` / `workdir`）先按**内置原值**展开，
//!    因此 `{"byk": "${byk}/tools"}` 的右侧指 `~/.byk`，等价于 `local byk = $byk/tools`；
//! 2. 其余声明按已合并的表展开，再跑快照迭代直到不动点，消解交叉引用。
//!
//! 快照迭代（每轮用上一轮的表计算新表）而非原地替换，避免 `{"A": "${A}x"}`
//! 这类自引用被逐轮放大；直接自引用会提前冻结并告警。

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::Value;

/// 当前平台（编译期注入，对齐 GitHub Release 命名）。
const PLATFORM: &str = env!("PLATFORM");

/// 闭包迭代上限。变量链远超此深度属于病态配置，止步即可。
const MAX_ROUNDS: usize = 8;

/// 变量表：名字 → 替换值。
pub type VarTable = BTreeMap<String, String>;

/// 允许被 `$var` 覆盖的内置变量名。
pub const BUILTIN_NAMES: &[&str] = &["byk", "workdir"];

/// 禁止在 `$var` 中声明的名字：会遮蔽别名参数占位符。
pub const RESERVED_DECL_NAMES: &[&str] = &["args"];

/// `$var` 声明被拒绝或冻结的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarIssue {
    /// 名字不符合 `[A-Za-z_][A-Za-z0-9_]*`
    InvalidName(String),
    /// 与参数占位符重名（`${args}`）
    Reserved(String),
    /// 值里直接引用了自己，无法收敛
    SelfReference(String),
}

/// 变量名合法性：与 shell 标识符一致。
pub fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 构建内置变量表。
pub fn build_builtin_table(byk_root: &Path, workdir: &Path) -> VarTable {
    let mut table = VarTable::new();
    table.insert("byk".to_string(), byk_root.to_string_lossy().into_owned());
    table.insert(
        "workdir".to_string(),
        workdir.to_string_lossy().into_owned(),
    );
    table
}

/// 单遍展开：只认 `${name}`，命中则替换，其余原样保留。
///
/// `\$` 被视为「交给 shell」的标记：两个字符原样输出，
/// 由 shell 自己做转义（`\${byk}` 最终打印字面量 `${byk}`）。
pub fn expand(input: &str, table: &VarTable) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < input.len() {
        if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'$') {
            out.push('\\');
            out.push('$');
            i += 2;
            continue;
        }

        if bytes[i] == b'$'
            && bytes.get(i + 1) == Some(&b'{')
            && let Some(close) = input[i + 2..].find('}')
        {
            let name = &input[i + 2..i + 2 + close];
            if let Some(value) = table.get(name) {
                out.push_str(value);
                i = i + 2 + close + 1;
                continue;
            }
        }

        let ch = input[i..].chars().next().unwrap_or_default();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// 判断字符串里是否有未被转义的 `${name}` 引用。
fn contains_var(input: &str, name: &str) -> bool {
    let needle = format!("${{{}}}", name);
    let bytes = input.as_bytes();
    let mut i = 0;

    while let Some(pos) = input[i..].find("${") {
        let at = i + pos;
        let escaped = at > 0 && bytes[at - 1] == b'\\';
        if !escaped && input[at..].starts_with(&needle) {
            return true;
        }
        i = at + 2;
    }

    false
}

/// 递归替换 JSON 中所有字符串值（key 不动）。
fn substitute_value(v: &mut Value, table: &VarTable) {
    match v {
        Value::String(s) => {
            let new = expand(s, table);
            if new != *s {
                *s = new;
            }
        }
        Value::Array(arr) => arr.iter_mut().for_each(|x| substitute_value(x, table)),
        Value::Object(obj) => obj.values_mut().for_each(|x| substitute_value(x, table)),
        _ => {}
    }
}

/// 平台映射取值：字符串直接用；对象取当前平台对应的值。
fn resolve_platform_value(val: &Value, platform: &str) -> Option<String> {
    if let Some(s) = val.as_str() {
        Some(s.to_string())
    } else if let Some(obj) = val.as_object() {
        obj.get(platform)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// 从文件根对象提取 `$var` 声明（已按平台解析）。
fn collect_declared(root: &serde_json::Map<String, Value>) -> BTreeMap<String, String> {
    let mut declared = BTreeMap::new();
    if let Some(vars) = root.get("$var").and_then(|v| v.as_object()) {
        for (k, v) in vars {
            if let Some(s) = resolve_platform_value(v, PLATFORM) {
                declared.insert(k.clone(), s);
            }
        }
    }
    declared
}

/// 合并内置表与声明，解析到不动点。
fn resolve_table(
    builtin: &VarTable,
    declared: &BTreeMap<String, String>,
) -> (VarTable, Vec<VarIssue>) {
    let mut issues: Vec<VarIssue> = Vec::new();
    let mut table = builtin.clone();
    let mut accepted: Vec<String> = Vec::new();

    fn check(name: &str, issues: &mut Vec<VarIssue>) -> bool {
        if !is_valid_var_name(name) {
            issues.push(VarIssue::InvalidName(name.to_string()));
            return false;
        }
        if RESERVED_DECL_NAMES.contains(&name) {
            issues.push(VarIssue::Reserved(name.to_string()));
            return false;
        }
        true
    }

    // 第一遍：内置名覆盖项按内置原值展开，右侧的 ${byk} 指向内置值。
    for (name, raw) in declared {
        if !BUILTIN_NAMES.contains(&name.as_str()) || !check(name, &mut issues) {
            continue;
        }
        table.insert(name.clone(), expand(raw, builtin));
        accepted.push(name.clone());
    }

    // 第二遍：其余声明按已合并的表展开（能看到被覆盖后的内置值）。
    let merged_builtin = table.clone();
    for (name, raw) in declared {
        if BUILTIN_NAMES.contains(&name.as_str()) || !check(name, &mut issues) {
            continue;
        }
        table.insert(name.clone(), expand(raw, &merged_builtin));
        accepted.push(name.clone());
    }

    // 直接自引用无法收敛：冻结并告警，值原样保留。
    let frozen: HashSet<String> = accepted
        .iter()
        .filter(|name| table.get(*name).is_some_and(|v| contains_var(v, name)))
        .cloned()
        .collect();
    for name in &frozen {
        issues.push(VarIssue::SelfReference(name.clone()));
    }

    // 快照迭代消解交叉引用，与声明顺序无关。
    for _ in 0..MAX_ROUNDS {
        let snapshot = table.clone();
        let mut changed = false;
        for name in &accepted {
            if frozen.contains(name) {
                continue;
            }
            let current = snapshot.get(name).cloned().unwrap_or_default();
            let next = expand(&current, &snapshot);
            if next != current {
                table.insert(name.clone(), next);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    (table, issues)
}

/// 扫描期入口：合并内置变量与 `$var`，对整个文件的字符串值做替换（原地修改）。
///
/// 返回被拒绝或冻结的声明，供调用方提示。
pub fn preprocess_vars(root: &mut Value, builtin: &VarTable) -> Vec<VarIssue> {
    let Some(obj) = root.as_object() else {
        return Vec::new();
    };
    let declared = collect_declared(obj);
    let (table, issues) = resolve_table(builtin, &declared);
    substitute_value(root, &table);
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn builtin() -> VarTable {
        let mut t = VarTable::new();
        t.insert("byk".into(), "/home/u/.byk".into());
        t.insert("workdir".into(), "/home/u/proj".into());
        t
    }

    fn run(root: Value) -> (Value, Vec<VarIssue>) {
        let mut v = root;
        let issues = preprocess_vars(&mut v, &builtin());
        (v, issues)
    }

    // ---------- expand ----------

    #[test]
    fn expands_braced_builtin() {
        assert_eq!(
            expand("ls ${byk}/tools", &builtin()),
            "ls /home/u/.byk/tools"
        );
    }

    #[test]
    fn bare_dollar_passthrough() {
        assert_eq!(expand("echo $byk $HOME", &builtin()), "echo $byk $HOME");
    }

    #[test]
    fn brace_prefix_concatenation() {
        assert_eq!(expand("${byk}aaa", &builtin()), "/home/u/.bykaaa");
    }

    #[test]
    fn escaped_dollar_left_to_shell() {
        // 反斜杠保留，由 shell 处理成字面量
        assert_eq!(expand(r"echo \${byk}", &builtin()), r"echo \${byk}");
    }

    #[test]
    fn unknown_name_passthrough() {
        assert_eq!(expand("${nope}", &builtin()), "${nope}");
    }

    #[test]
    fn placeholder_forms_passthrough() {
        assert_eq!(expand("${0} ${...args}", &builtin()), "${0} ${...args}");
    }

    #[test]
    fn unterminated_brace_passthrough() {
        assert_eq!(expand("${byk", &builtin()), "${byk");
    }

    #[test]
    fn cjk_around_var_preserved() {
        assert_eq!(expand("中文${byk}中文", &builtin()), "中文/home/u/.byk中文");
    }

    // ---------- preprocess ----------

    #[test]
    fn substitutes_user_var() {
        let (v, issues) = run(json!({
            "$var": {"VER": "v1.2.0"},
            "run": {"$cmd": "tool --version ${VER}"}
        }));
        assert!(issues.is_empty());
        assert_eq!(v["run"]["$cmd"], json!("tool --version v1.2.0"));
    }

    #[test]
    fn builtin_expands_without_var_field() {
        let (v, issues) = run(json!({"hi": "echo ${byk}"}));
        assert!(issues.is_empty());
        assert_eq!(v["hi"], json!("echo /home/u/.byk"));
    }

    #[test]
    fn workdir_expands() {
        let (v, _) = run(json!({"p": "echo ${workdir}"}));
        assert_eq!(v["p"], json!("echo /home/u/proj"));
    }

    #[test]
    fn user_var_overrides_builtin() {
        let (v, issues) = run(json!({"$var": {"byk": "/tmp/x"}, "hi": "echo ${byk}"}));
        assert!(issues.is_empty());
        assert_eq!(v["hi"], json!("echo /tmp/x"));
    }

    #[test]
    fn override_rhs_sees_builtin_value() {
        // 等号右边的 ${byk} 指内置原值：local byk = $byk/tools
        let (v, _) = run(json!({"$var": {"byk": "${byk}/tools"}, "hi": "echo ${byk}"}));
        assert_eq!(v["hi"], json!("echo /home/u/.byk/tools"));
    }

    #[test]
    fn other_var_sees_overridden_builtin() {
        let (v, _) = run(json!({
            "$var": {"byk": "/tmp/x", "T": "${byk}/t"},
            "hi": "echo ${T}"
        }));
        assert_eq!(v["hi"], json!("echo /tmp/x/t"));
    }

    #[test]
    fn cross_reference_order_independent() {
        // 字母序 A 在前、依赖方向相反，仍应收敛
        let (v, issues) = run(json!({
            "$var": {"A": "1", "B": "${A}/x", "C": "${B}/y"},
            "hi": "echo ${C}"
        }));
        assert!(issues.is_empty());
        assert_eq!(v["hi"], json!("echo 1/x/y"));
    }

    #[test]
    fn self_reference_frozen_and_reported() {
        let (v, issues) = run(json!({"$var": {"A": "${A}x"}, "hi": "echo ${A}"}));
        assert_eq!(issues, vec![VarIssue::SelfReference("A".into())]);
        assert_eq!(v["hi"], json!("echo ${A}x"));
    }

    #[test]
    fn invalid_name_rejected() {
        let (v, issues) = run(json!({
            "$var": {"9bad": "x", "中文": "y"},
            "hi": "echo ${9bad}${中文}"
        }));
        assert!(issues.contains(&VarIssue::InvalidName("9bad".into())));
        assert!(issues.contains(&VarIssue::InvalidName("中文".into())));
        assert_eq!(v["hi"], json!("echo ${9bad}${中文}"));
    }

    #[test]
    fn reserved_name_rejected() {
        let (v, issues) = run(json!({"$var": {"args": "boom"}, "hi": "echo ${args}"}));
        assert_eq!(issues, vec![VarIssue::Reserved("args".into())]);
        assert_eq!(v["hi"], json!("echo ${args}"));
    }

    #[test]
    fn keys_not_substituted() {
        let (v, _) = run(json!({"$var": {"V": "1"}, "${V}": "echo hi"}));
        assert!(v.get("${V}").is_some());
    }

    #[test]
    fn nested_values_substituted() {
        let (v, _) = run(json!({
            "$var": {"V": "1"},
            "g": {"$cmd": "run ${V}", "sub": {"$cmd": "x ${V}"}}
        }));
        assert_eq!(v["g"]["$cmd"], json!("run 1"));
        assert_eq!(v["g"]["sub"]["$cmd"], json!("x 1"));
    }

    #[test]
    fn platform_map_value_resolved() {
        let (v, _) = run(json!({
            "$var": {"BASE": {
                "linux-x86_64": "https://linux.example",
                "darwin-arm64": "https://mac.example"
            }},
            "d": "${BASE}/tool"
        }));
        let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "https://mac.example/tool"
        } else {
            "https://linux.example/tool"
        };
        assert_eq!(v["d"], json!(expected));
    }

    #[test]
    fn platform_map_missing_leaves_var_undefined() {
        let (v, _) = run(json!({"$var": {"X": {"nonexistent": "y"}}, "d": "${X}/t"}));
        assert_eq!(v["d"], json!("${X}/t"));
    }

    #[test]
    fn cycle_terminates() {
        let (v, _) = run(json!({"$var": {"A": "${B}", "B": "${A}"}, "d": "${A}"}));
        assert!(v["d"].is_string());
    }

    #[test]
    fn shell_vars_passthrough() {
        let (v, issues) = run(json!({"run": {"$cmd": "echo $HOME ${HOME}"}}));
        assert!(issues.is_empty());
        assert_eq!(v["run"]["$cmd"], json!("echo $HOME ${HOME}"));
    }

    #[test]
    fn paths_array_substituted() {
        let (v, _) = run(json!({"$paths": ["${byk}/bin"], "b": "make"}));
        assert_eq!(v["$paths"][0], json!("/home/u/.byk/bin"));
    }

    #[test]
    fn build_builtin_table_values() {
        let t = build_builtin_table(&PathBuf::from("/r/.byk"), &PathBuf::from("/w"));
        assert_eq!(t["byk"], "/r/.byk");
        assert_eq!(t["workdir"], "/w");
    }

    #[test]
    fn valid_var_name_rules() {
        assert!(is_valid_var_name("FOO"));
        assert!(is_valid_var_name("_x1"));
        assert!(is_valid_var_name("byk"));
        assert!(!is_valid_var_name("1x"));
        assert!(!is_valid_var_name("a-b"));
        assert!(!is_valid_var_name(""));
        assert!(!is_valid_var_name("中文"));
    }
}
