//! @ / @@ 前缀补全与 key 收集。

use std::collections::HashMap;

use crate::core::aliases::{self, MergedNode};

/// 展开路径开头的 `~` 为用户 HOME 目录（仅测试使用）。
#[cfg(test)]
pub(crate) fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~')
        && let Ok(home) = std::env::var("HOME")
    {
        return home + rest;
    }
    path.to_string()
}

/// 补全 @ 和 @@ 语法：文件引用和文件内 key。
pub(crate) fn complete_at_prefix(partial: &str, layout: &crate::core::paths::PathLayout) -> Vec<String> {
    let (_, files) = aliases::load_merged_aliases(layout);

    // 查找 partial 中是否包含 '.' — 如果有，补全文件内的 key
    if let Some(dot_idx) = partial.find('.') {
        let file_key = &partial[..dot_idx];
        let key_partial = &partial[dot_idx + 1..];

        for f in &files {
            if f.key == file_key {
                return collect_object_keys(&f.aliases, "")
                    .into_iter()
                    .filter(|k| k.starts_with(key_partial))
                    .map(|k| format!("{}.{}", file_key, k))
                    .collect();
            }
        }
        return Vec::new();
    }

    // 补全文件 key（@xxx 或 @@xxx 格式）
    files
        .iter()
        .map(|f| f.key.clone())
        .filter(|k| k.starts_with(partial))
        .collect()
}

/// 补全嵌套别名（含点号的路径，如 "部署.打"）。
pub(crate) fn complete_nested_alias(
    prefix: &str,
    key_partial: &str,
    layout: &crate::core::paths::PathLayout,
) -> Vec<String> {
    let (merged, _) = aliases::load_merged_aliases(layout);

    // 去掉末尾点号，按点拆成路径段
    let path_str = prefix.trim_end_matches('.');
    let parts: Vec<&str> = if path_str.is_empty() {
        Vec::new()
    } else {
        path_str.split('.').collect()
    };

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

/// 递归收集 JSON Object 中所有可补全的 key 路径。
/// $ 前缀的 key 为系统元数据，不参与补全。
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