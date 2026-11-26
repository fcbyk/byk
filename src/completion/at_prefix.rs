//! 作用域前缀（`work@` / `global@` / `special@`）的补全与 key 收集。

use std::collections::HashMap;

use crate::model::{MergedNode, PathLayout, Scope, build_file_key};
use crate::resolve;

/// 补全 `work@` / `global@` / `special@`：先补文件 key，输入点号后补该文件内的别名路径。
///
/// 候选回显用户已经敲的前缀（`wor@` 给 `wor@build`），否则光标处的词会被换成完整写法。
pub(crate) fn complete_at_prefix(partial: &str, layout: &PathLayout) -> Vec<String> {
    let Some((scope, rest)) = Scope::split(partial) else {
        return Vec::new();
    };
    let typed = &partial[..partial.len() - rest.len()];

    // `special@` 是特殊文件命名空间：候选来自注册表，不来自磁盘，也没有点号子路径
    if scope == Scope::Special {
        return crate::filecmd::special::names()
            .filter(|name| name.starts_with(rest))
            .map(|name| format!("{}{}", typed, name))
            .collect();
    }

    // 点号之后只需要目标文件：直接解析它，不扫描其余文件
    if let Some(dot_idx) = rest.find('.') {
        let stem = &rest[..dot_idx];
        let key_partial = &rest[dot_idx + 1..];
        let file_key = build_file_key(stem, scope.is_global());

        let Ok(file) = resolve::load_file_by_key(layout, &file_key) else {
            return Vec::new();
        };
        return collect_object_keys(&file.aliases, "")
            .into_iter()
            .filter(|k| k.starts_with(key_partial))
            .map(|k| format!("{}{}.{}", typed, stem, k))
            .collect();
    }

    // 补全文件 key：只列该作用域下的文件
    let dir = if scope.is_global() {
        layout.alias_dir.clone()
    } else {
        match std::env::current_dir() {
            Ok(dir) => dir,
            Err(_) => return Vec::new(),
        }
    };

    let mut candidates: Vec<String> = resolve::list_alias_files(&dir, scope.is_global())
        .into_iter()
        .filter_map(|(key, _)| {
            key.strip_prefix(scope.token())
                .map(|stem| format!("{}{}", typed, stem))
        })
        .filter(|candidate| candidate.starts_with(partial))
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

/// 补全嵌套别名（含点号的路径，如 "部署.打"）。
pub(crate) fn complete_nested_alias(
    prefix: &str,
    key_partial: &str,
    layout: &crate::model::PathLayout,
) -> Vec<String> {
    let merged = resolve::load_merged(layout);

    // 去掉末尾点号，按点拆成路径段
    let path_str = prefix.trim_end_matches('.');
    // 剥完点号什么都不剩，说明这是点号开头的 token（如 `.git`）。
    // 前导点号没有路径前缀，这种写法不是可执行别名，所以不产出候选；
    // 否则会把整棵合并树的顶层 key 都加上点号前缀吐出去。
    if path_str.is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = path_str.split('.').collect();

    let mut current: &HashMap<String, MergedNode> = &merged;
    for part in &parts {
        match current.get(*part) {
            Some(node) => current = &node.children,
            None => return Vec::new(),
        }
    }

    current
        .keys()
        .filter(|k| k.starts_with(key_partial))
        .map(|k| format!("{}{}", prefix, k))
        .collect()
}

/// 递归收集可补全的 key 路径；`$` 前缀的系统字段跳过。
pub(crate) fn collect_object_keys(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
) -> Vec<String> {
    let mut result = Vec::new();
    for (key, val) in obj {
        if key.starts_with('$') {
            continue;
        }
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        match val {
            serde_json::Value::Object(inner) => {
                result.push(full.clone());
                result.extend(collect_object_keys(inner, &full));
            }
            _ => {
                result.push(full);
            }
        }
    }
    result
}
