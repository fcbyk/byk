/// 深度合并、别名加载、路径收集与解析。
///
/// 将多个 AliasFile 按优先级合并为一棵 MergedConfig 树，
/// 并提供路径遍历和别名查找能力。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::cache::load_alias_cache;
use super::parse::{is_alias_value, to_alias_value};
use super::scan::sort_alias_files;
use super::types::{AliasFile, AliasValue, MergedConfig, MergedNode, ResolvedAlias};
use crate::core::paths::PathLayout;

// ---------------------------------------------------------------------------
// 深度合并
// ---------------------------------------------------------------------------

/// 将 source 深度合并到 target。
///
/// 每个 MergedNode 可以同时拥有 alias（$cmd 等元数据定义的叶子命令）
/// 和 children（非 $ 前缀的嵌套分组）。
///
/// - String / 含 $ 前缀的 Object → 设置 target[key].alias
/// - 不含 $ 前缀的 Object → 递归合并到 target[key].children
///
/// inherited_cwd 为文件级 $cwd，子别名未指定 $cwd 时自动继承。
/// inherited_interactive 为文件级 $interactive，子别名未指定时自动继承。
fn deep_merge_dict(
    target: &mut MergedConfig,
    source: &serde_json::Map<String, serde_json::Value>,
    file_key: &str,
    source_path: Option<&Path>,
    inherited_cwd: Option<&str>,
    inherited_interactive: Option<bool>,
) {
    // 提取当前分组的 $cwd / $interactive，子级覆盖父级
    let group_cwd = source
        .get("$cwd")
        .and_then(|v| v.as_str())
        .or(inherited_cwd);
    let group_interactive = source
        .get("$interactive")
        .and_then(|v| v.as_bool())
        .or(inherited_interactive);

    for (key, val) in source {
        if key.starts_with('$') {
            continue;
        }

        if is_alias_value(val) {
            let alias_val = to_alias_value(val);
            if let Some(av) = alias_val {
                let av = apply_inherited(av, group_cwd, group_interactive);
                let entry = target
                    .entry(key.clone())
                    .or_insert_with(MergedNode::default);
                entry.alias = Some(ResolvedAlias {
                    value: av,
                    source: file_key.to_string(),
                    source_path: source_path.map(|p| p.to_path_buf()),
                });

                if let serde_json::Value::Object(inner) = val {
                    deep_merge_dict(
                        &mut entry.children,
                        inner,
                        file_key,
                        source_path,
                        group_cwd,
                        group_interactive,
                    );
                }
            }
        } else if let serde_json::Value::Object(inner) = val {
            let entry = target
                .entry(key.clone())
                .or_insert_with(MergedNode::default);
            deep_merge_dict(
                &mut entry.children,
                inner,
                file_key,
                source_path,
                group_cwd,
                group_interactive,
            );
        }
    }
}

/// 对未指定 $cwd / $interactive 的别名应用文件级继承。
pub(crate) fn apply_inherited(
    av: AliasValue,
    inherited_cwd: Option<&str>,
    inherited_interactive: Option<bool>,
) -> AliasValue {
    match av {
        AliasValue::Meta {
            cmd,
            cwd: None,
            interactive,
        } => AliasValue::Meta {
            cmd,
            cwd: inherited_cwd.map(String::from),
            interactive: interactive.or(inherited_interactive),
        },
        AliasValue::Meta {
            cmd,
            cwd: Some(c),
            interactive,
        } => AliasValue::Meta {
            cmd,
            cwd: Some(c), // 别名自己的 $cwd 优先
            interactive: interactive.or(inherited_interactive),
        },
        AliasValue::Str(cmd) => {
            if inherited_cwd.is_some() || inherited_interactive.is_some() {
                AliasValue::Meta {
                    cmd,
                    cwd: inherited_cwd.map(String::from),
                    interactive: inherited_interactive,
                }
            } else {
                AliasValue::Str(cmd)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 别名加载（主入口）
// ---------------------------------------------------------------------------

/// 从已排序的 AliasFile 列表构建合并配置树。
///
/// 负数 priority 的文件不参与合并（仅可通过精确语法 @file.key 访问）。
pub(crate) fn build_merged_aliases(files: &[AliasFile]) -> MergedConfig {
    let mut merged: MergedConfig = HashMap::new();
    for f in files {
        if f.priority < 0 {
            continue;
        }
        deep_merge_dict(
            &mut merged,
            &f.aliases,
            &f.key,
            f.path.parent(),
            f.inherited_cwd.as_deref(),
            f.inherited_interactive,
        );
    }
    merged
}

/// 扫描本地和全局别名文件，返回 (合并配置, 文件列表)。
///
/// 通过缓存（~/.byk/cache/alias.json）避免每次启动重复扫描和合并。
/// 缓存通过文件 mtime 快照检测失效：文件增删改、CWD 变化均自动触发重建。
pub fn load_merged_aliases(layout: &PathLayout) -> (MergedConfig, Vec<AliasFile>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cache_file = layout.cache_dir.join("alias.json");
    let global_dir = &layout.alias_dir;

    let (merged, mut files) = load_alias_cache(&cache_file, &cwd, global_dir);

    // 文件列表需要保持排序（load_alias_cache 内部保证排序后写入缓存）
    // 防御性排序，确保调用方拿到的始终有序
    sort_alias_files(&mut files);

    (merged, files)
}

// ---------------------------------------------------------------------------
// 路径收集
// ---------------------------------------------------------------------------

/// 收集合并配置中所有可执行别名路径。
pub fn collect_merged_paths(merged: &MergedConfig, prefix: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for (key, node) in merged {
        let cur = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        if node.alias.is_some() {
            result.push(cur.clone());
        }
        if !node.children.is_empty() {
            result.extend(collect_merged_paths(&node.children, &cur));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// 别名解析
// ---------------------------------------------------------------------------

/// 在合并配置中按 key 路径查找，返回 ResolvedAlias 或 None。
///
/// 查找规则：沿点号路径遍历 MergedNode 树，最后一级节点有 alias 则返回。
pub fn resolve_merged_alias<'a>(
    merged: &'a MergedConfig,
    name: &str,
) -> Option<&'a ResolvedAlias> {
    let parts: Vec<&str> = name.split('.').collect();
    let mut current: &MergedConfig = merged;
    for (i, part) in parts.iter().enumerate() {
        let node = current.get(*part)?;
        if i == parts.len() - 1 {
            return node.alias.as_ref();
        }
        current = &node.children;
    }
    None
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_alias(cmd: &str) -> ResolvedAlias {
        ResolvedAlias {
            value: AliasValue::Str(cmd.into()),
            source: "@test".into(),
            source_path: None,
        }
    }

    // ==================== apply_inherited ====================

    #[test]
    fn inherit_str_no_inherited_stays_str() {
        let av = AliasValue::Str("echo hi".into());
        let result = apply_inherited(av, None, None);
        assert!(matches!(result, AliasValue::Str(s) if s == "echo hi"));
    }

    #[test]
    fn inherit_str_with_cwd_becomes_meta() {
        let av = AliasValue::Str("build".into());
        let result = apply_inherited(av, Some("/app"), None);
        match result {
            AliasValue::Meta { cmd, cwd, interactive } => {
                assert_eq!(cmd, "build");
                assert_eq!(cwd, Some("/app".into()));
                assert_eq!(interactive, None);
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_str_with_interactive_becomes_meta() {
        let av = AliasValue::Str("run".into());
        let result = apply_inherited(av, None, Some(true));
        match result {
            AliasValue::Meta { cmd, interactive, .. } => {
                assert_eq!(cmd, "run");
                assert_eq!(interactive, Some(true));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_keeps_own_cwd() {
        let av = AliasValue::Meta {
            cmd: "test".into(),
            cwd: Some("/own".into()),
            interactive: None,
        };
        let result = apply_inherited(av, Some("/inherit"), None);
        match result {
            AliasValue::Meta { cwd, .. } => {
                assert_eq!(cwd, Some("/own".into())); // 自己的优先
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_fills_missing_cwd() {
        let av = AliasValue::Meta {
            cmd: "test".into(),
            cwd: None,
            interactive: None,
        };
        let result = apply_inherited(av, Some("/inherit"), None);
        match result {
            AliasValue::Meta { cwd, .. } => {
                assert_eq!(cwd, Some("/inherit".into()));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_fills_missing_interactive() {
        let av = AliasValue::Meta {
            cmd: "ask".into(),
            cwd: None,
            interactive: None,
        };
        let result = apply_inherited(av, None, Some(true));
        match result {
            AliasValue::Meta { interactive, .. } => {
                assert_eq!(interactive, Some(true));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_keeps_own_interactive() {
        let av = AliasValue::Meta {
            cmd: "ask".into(),
            cwd: None,
            interactive: Some(false),
        };
        let result = apply_inherited(av, None, Some(true));
        match result {
            AliasValue::Meta { interactive, .. } => {
                assert_eq!(interactive, Some(false)); // 自己的优先
            }
            _ => panic!("Expected Meta"),
        }
    }

    // ==================== collect_merged_paths ====================

    #[test]
    fn paths_empty_config() {
        let merged: MergedConfig = HashMap::new();
        assert!(collect_merged_paths(&merged, "").is_empty());
    }

    #[test]
    fn paths_single_leaf() {
        let mut merged: MergedConfig = HashMap::new();
        let mut node = MergedNode::default();
        node.alias = Some(make_alias("cmd"));
        merged.insert("greet".into(), node);

        let result = collect_merged_paths(&merged, "");
        assert_eq!(result, vec!["greet"]);
    }

    #[test]
    fn paths_nested() {
        let mut merged: MergedConfig = HashMap::new();
        let mut child = MergedNode::default();
        child.alias = Some(make_alias("child_cmd"));
        let mut parent = MergedNode::default();
        parent.children.insert("sub".into(), child);
        merged.insert("ns".into(), parent);

        let mut result = collect_merged_paths(&merged, "");
        result.sort();
        assert_eq!(result, vec!["ns.sub"]);
    }

    #[test]
    fn paths_intermediate_node_with_alias() {
        let mut merged: MergedConfig = HashMap::new();
        let mut child = MergedNode::default();
        child.alias = Some(make_alias("sub_cmd"));
        let mut parent = MergedNode::default();
        parent.alias = Some(make_alias("parent_cmd")); // 父节点也有 alias
        parent.children.insert("sub".into(), child);
        merged.insert("ns".into(), parent);

        let mut result = collect_merged_paths(&merged, "");
        result.sort();
        assert_eq!(result, vec!["ns", "ns.sub"]);
    }

    #[test]
    fn paths_with_prefix() {
        let mut merged: MergedConfig = HashMap::new();
        let mut node = MergedNode::default();
        node.alias = Some(make_alias("cmd"));
        merged.insert("leaf".into(), node);

        let result = collect_merged_paths(&merged, "root");
        assert_eq!(result, vec!["root.leaf"]);
    }

    #[test]
    fn paths_node_without_alias_but_with_children() {
        let mut merged: MergedConfig = HashMap::new();
        let mut child = MergedNode::default();
        child.alias = Some(make_alias("leaf_cmd"));
        let mut parent = MergedNode::default();
        // parent 没有 alias，只有 children
        parent.children.insert("leaf".into(), child);
        merged.insert("group".into(), parent);

        let result = collect_merged_paths(&merged, "");
        // "group" 本身没有 alias，不出现；"group.leaf" 有
        assert_eq!(result, vec!["group.leaf"]);
    }

    // ==================== resolve_merged_alias ====================

    #[test]
    fn resolve_found() {
        let mut merged: MergedConfig = HashMap::new();
        let mut node = MergedNode::default();
        node.alias = Some(make_alias("echo hello"));
        merged.insert("greet".into(), node);

        let result = resolve_merged_alias(&merged, "greet");
        assert!(result.is_some());
        if let AliasValue::Str(cmd) = &result.unwrap().value {
            assert_eq!(cmd, "echo hello");
        }
    }

    #[test]
    fn resolve_not_found() {
        let merged: MergedConfig = HashMap::new();
        assert!(resolve_merged_alias(&merged, "nope").is_none());
    }

    #[test]
    fn resolve_nested_path() {
        let mut merged: MergedConfig = HashMap::new();
        let mut child = MergedNode::default();
        child.alias = Some(make_alias("nested_cmd"));
        let mut parent = MergedNode::default();
        parent.children.insert("sub".into(), child);
        merged.insert("ns".into(), parent);

        let result = resolve_merged_alias(&merged, "ns.sub");
        assert!(result.is_some());
    }

    #[test]
    fn resolve_partial_path_no_alias() {
        let mut merged: MergedConfig = HashMap::new();
        let child = MergedNode::default();
        // child 没有 alias
        let mut parent = MergedNode::default();
        parent.children.insert("sub".into(), child);
        merged.insert("ns".into(), parent);

        // "ns.sub" 对应的节点没有 alias → None
        assert!(resolve_merged_alias(&merged, "ns.sub").is_none());
    }
}
