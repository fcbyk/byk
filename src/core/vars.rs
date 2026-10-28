//! 文件级 `$var` 变量：提取、平台解析、全文件字符串替换（阶段一）。
//!
//! 与内置变量（阶段二，见 `aliases::variables`）分工：
//! - **阶段一**（扫描/读取文件时，本模块）：用户声明的 `$var` 按当前平台
//!   解析后，对文件内所有字符串值做 `$NAME` 替换；
//! - **阶段二**（命令执行/下载时）：内置变量 `$python`/`$venv`/`$byk`/`$assets` 展开。
//!
//! 分两阶段的原因：`$assets` 的值依赖文件 Scope 与运行 cwd，扫描时求不出来。
//! 副作用好处：用户变量值里可组合内置变量（`"TOOLS": "$byk/tools"`），
//! 阶段一替换出字面量，阶段二再展开。
//!
//! 规则：
//! - `$var` 为文件级字段，作用于整个文件（`$cmd`/`$cwd`/`$paths`/`$pip`/`$assets` 均可用）；
//! - 变量值可以是字符串或平台映射（当前平台不在映射 → 该变量未定义）；
//! - 内置变量名（`python`/`venv`/`byk`/`assets`）声明无效——内置优先；
//! - 未定义的 `$NAME` 原样透传（URL 里的 `$`、shell 变量 `$HOME` 不受影响）；
//! - 用户变量约定全大写——大小写即归属信号。

use serde_json::Value;

/// 内置变量名（在 `$var` 中声明无效）。
pub const RESERVED_VARS: &[&str] = &["python", "venv", "byk", "assets", "downloads", "workdir"];

/// 当前平台（编译期注入，对齐 GitHub Release 命名）。
const PLATFORM: &str = env!("PLATFORM");

/// 平台映射取值：字符串直接用；对象取当前平台对应的值。
fn resolve_platform_value(val: &Value, platform: &str) -> Option<String> {
    if let Some(s) = val.as_str() {
        Some(s.to_string())
    } else if let Some(obj) = val.as_object() {
        obj.get(platform).and_then(|v| v.as_str()).map(|s| s.to_string())
    } else {
        None
    }
}

/// 从文件根对象提取 `$var`，构建变量表。
///
/// 返回 `(变量表, 被忽略的保留字声明名列表)`。
fn collect_vars(root: &serde_json::Map<String, Value>) -> (Vec<(String, String)>, Vec<String>) {
    let mut pairs = Vec::new();
    let mut shadowed = Vec::new();
    if let Some(vars) = root.get("$var").and_then(|v| v.as_object()) {
        for (k, v) in vars {
            if RESERVED_VARS.contains(&k.as_str()) {
                shadowed.push(k.clone());
                continue;
            }
            if let Some(s) = resolve_platform_value(v, PLATFORM) {
                pairs.push((k.clone(), s));
            }
        }
    }
    (pairs, shadowed)
}

/// 单遍替换：`$NAME` 命中变量表则替换，未命中（含内置变量、shell 变量）原样保留。
fn substitute_once(s: &str, vars: &[(String, String)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        let ident = &after[..end];
        match vars.iter().find(|(k, _)| k == ident) {
            Some((_, v)) => {
                out.push_str(v);
                rest = &after[end..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 多遍替换以支持交叉引用（`"a": "$b/x"`）。迭代上限防循环引用死循环。
fn substitute_deep(s: &str, vars: &[(String, String)]) -> String {
    let mut current = s.to_string();
    for _ in 0..8 {
        let next = substitute_once(&current, vars);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// 递归替换 JSON 中所有字符串值（key 不动）。
fn substitute_value(v: &mut Value, vars: &[(String, String)]) {
    match v {
        Value::String(s) => {
            let new = substitute_deep(s, vars);
            if new != *s {
                *s = new;
            }
        }
        Value::Array(arr) => arr.iter_mut().for_each(|x| substitute_value(x, vars)),
        Value::Object(obj) => obj.values_mut().for_each(|x| substitute_value(x, vars)),
        _ => {}
    }
}

/// 阶段一入口：提取 `$var` 并对整个文件做字符串值替换（原地修改）。
///
/// 返回被忽略的保留字声明名列表——调用方（扫描期）在缓存重建时警告一次，
/// 缓存命中路径不调用本函数，不会重复刷屏。
pub fn preprocess_vars(root: &mut Value) -> Vec<String> {
    let Some(obj) = root.as_object() else {
        return Vec::new();
    };
    let (vars, shadowed) = collect_vars(obj);
    if !vars.is_empty() {
        substitute_value(root, &vars);
    }
    shadowed
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_user_var() {
        let mut v = json!({"$var": {"VER": "v1.2.0"}, "run": {"$cmd": "tool --version $VER"}});
        let shadowed = preprocess_vars(&mut v);
        assert!(shadowed.is_empty());
        assert_eq!(v["run"]["$cmd"], json!("tool --version v1.2.0"));
    }

    #[test]
    fn unknown_dollar_passthrough() {
        let mut v = json!({"$var": {"VER": "v1"}, "run": {"$cmd": "echo $HOME $VER $unknown"}});
        preprocess_vars(&mut v);
        // $HOME（shell 变量）与 $unknown 原样透传
        assert_eq!(v["run"]["$cmd"], json!("echo $HOME v1 $unknown"));
    }

    #[test]
    fn reserved_vars_ignored_and_reported() {
        let mut v = json!({"$var": {"byk": "/fake", "OK": "yes"}, "run": {"$cmd": "$OK $byk"}});
        let shadowed = preprocess_vars(&mut v);
        assert_eq!(shadowed, vec!["byk".to_string()]);
        // 用户变量生效；$byk 是内置，留给阶段二
        assert_eq!(v["run"]["$cmd"], json!("yes $byk"));
    }

    #[test]
    fn platform_map_value_resolved() {
        let mut v = json!({"$var": {
            "BASE": {"linux-x86_64": "https://linux.example", "darwin-arm64": "https://mac.example"}
        }, "d": "$BASE/tool"});
        preprocess_vars(&mut v);
        // 当前平台（测试机）取到对应值
        let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "https://mac.example/tool"
        } else {
            "https://linux.example/tool"
        };
        assert_eq!(v["d"], json!(expected));
    }

    #[test]
    fn platform_map_missing_platform_var_undefined() {
        let mut v = json!({"$var": {"X": {"nonexistent-platform": "y"}}, "d": "$X/tool"});
        preprocess_vars(&mut v);
        // 变量未定义 → 透传
        assert_eq!(v["d"], json!("$X/tool"));
    }

    #[test]
    fn cross_reference_resolves() {
        let mut v = json!({"$var": {"BASE": "https://x", "FULL": "$BASE/tool"}, "d": "$FULL"});
        preprocess_vars(&mut v);
        assert_eq!(v["d"], json!("https://x/tool"));
    }

    #[test]
    fn cycle_reference_terminates() {
        let mut v = json!({"$var": {"A": "$B", "B": "$A"}, "d": "$A"});
        preprocess_vars(&mut v); // 不得 panic 或死循环
        assert!(v["d"].is_string());
    }

    #[test]
    fn no_var_field_is_noop() {
        let mut v = json!({"run": {"$cmd": "echo $HOME"}});
        let shadowed = preprocess_vars(&mut v);
        assert!(shadowed.is_empty());
        assert_eq!(v["run"]["$cmd"], json!("echo $HOME"));
    }

    #[test]
    fn replaces_nested_values_not_keys() {
        let mut v = json!({"$var": {"V": "1"}, "g": {"$cmd": "run $V", "sub": {"$cmd": "x $V"}}});
        preprocess_vars(&mut v);
        assert_eq!(v["g"]["$cmd"], json!("run 1"));
        assert_eq!(v["g"]["sub"]["$cmd"], json!("x 1"));
    }

    #[test]
    fn no_partial_word_match() {
        let mut v = json!({"$var": {"V": "1"}, "d": "$VER $V"});
        preprocess_vars(&mut v);
        // $VER 不是 $V（整词匹配）
        assert_eq!(v["d"], json!("$VER 1"));
    }
}