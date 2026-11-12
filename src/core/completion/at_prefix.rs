//! @ / @@ 前缀补全与 key 收集。

use std::collections::HashMap;

use crate::core::aliases;
use crate::core::config::MergedNode;
use crate::core::filecmd;

/// 补全 `@` / `@@`：先补文件 key，输入点号后补该文件内的别名路径。
///
/// `..` 是文件命令分隔符，出现两个点后只补动词。
pub(crate) fn complete_at_prefix(
    partial: &str,
    layout: &crate::core::paths::PathLayout,
) -> Vec<String> {
    if let Some(verbs) = filecmd::complete_verbs(partial) {
        return verbs;
    }

    // 点号之后只需要目标文件：直接解析它，不扫描其余文件
    if let Some(dot_idx) = partial.find('.') {
        let file_key = &partial[..dot_idx];
        let key_partial = &partial[dot_idx + 1..];

        let Some(file) = aliases::load_file_by_key(layout, file_key) else {
            return Vec::new();
        };
        return collect_object_keys(&file.aliases, "")
            .into_iter()
            .filter(|k| k.starts_with(key_partial))
            .map(|k| format!("{}.{}", file_key, k))
            .collect();
    }

    // 补全文件 key（@xxx 或 @@xxx 格式）：需要知道哪些文件能解析，只能全量扫描
    let files = aliases::load_scanned_aliases(layout).files;
    let mut candidates: Vec<String> = files
        .iter()
        .map(|f| f.key.clone())
        .filter(|k| k.starts_with(partial))
        .collect();
    candidates.sort();
    candidates
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
