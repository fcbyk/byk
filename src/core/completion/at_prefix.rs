//! @ / @@ 前缀补全、文件路径补全、show 话题补全与 key 收集。

use std::collections::HashMap;
use std::path::Path;

use crate::core::aliases::{self, MergedNode};

/// 补全文件路径。
///
/// 根据 partial 中已输入的路径前缀，返回匹配的文件/目录候选。
/// 目录后缀 `/`，隐藏文件默认不显示（除非 partial 以 `.` 开头）。
pub(crate) fn complete_file_path(partial: &str) -> Vec<String> {
    // 展开 ~ 到 HOME
    let expanded = expand_tilde(partial);

    // 确定搜索目录和文件名前缀
    let (base_dir, prefix) = if let Some(last_slash) = expanded.rfind('/') {
        let dir = if last_slash == 0 {
            // 以 / 开头（绝对路径），取 "/"
            "/".to_string()
        } else {
            expanded[..=last_slash].to_string()
        };
        let pre = expanded[last_slash + 1..].to_string();
        (dir, pre)
    } else {
        (".".to_string(), expanded)
    };

    let dir_path = Path::new(&base_dir);
    if !dir_path.is_dir() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let read_dir = match std::fs::read_dir(dir_path) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // 隐藏文件仅在 prefix 以 . 开头时显示
        if name.starts_with('.') && !prefix.starts_with('.') {
            continue;
        }
        if !name.starts_with(&prefix) {
            continue;
        }

        let entry_path = entry.path();
        let mut candidate = if let Some(last_slash) = partial.rfind('/') {
            format!("{}{}", &partial[..=last_slash], name)
        } else {
            name.clone()
        };

        if entry_path.is_dir() {
            candidate.push('/');
        }

        results.push(candidate);
    }

    results.sort();
    results
}

/// 展开路径开头的 `~` 为用户 HOME 目录。
pub(crate) fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~')
        && let Ok(home) = std::env::var("HOME")
    {
        return home + rest;
    }
    path.to_string()
}

/// 补全 `byk show` 后面的子命令。
pub(crate) fn complete_show_topic(partial: &str, layout: &crate::core::paths::PathLayout) -> Vec<String> {
    let mut candidates: Vec<String> = vec![
        "overview".into(),
        "paths".into(),
        "add".into(),
        "remove".into(),
        "completion".into(),
    ];

    // 别名路径
    let (merged, _files) = aliases::load_merged_aliases(layout);
    candidates.extend(aliases::collect_merged_paths(&merged, ""));

    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|c| c.starts_with(partial))
        .collect()
}

// ---------------------------------------------------------------------------
// 补全 @ 和 @@ 语法
// ---------------------------------------------------------------------------

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
