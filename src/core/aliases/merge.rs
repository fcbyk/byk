/// 深度合并、别名加载、路径收集与解析。
///
/// 将多个 AliasFile 按优先级合并为一棵 MergedConfig 树，
/// 并提供路径遍历和别名查找能力。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::cache::load_alias_cache;
use super::parse::{is_alias_value, to_alias_value};
use super::scan::{scan_alias_files, sort_alias_files};
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
    priority: i32,
    inherited_cwd: Option<&str>,
    inherited_interactive: Option<bool>,
    inherited_paths: &[String],
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
                    priority,
                    paths: inherited_paths.to_vec(),
                });

                if let serde_json::Value::Object(inner) = val {
                    deep_merge_dict(
                        &mut entry.children,
                        inner,
                        file_key,
                        source_path,
                        priority,
                        group_cwd,
                        group_interactive,
                        inherited_paths,
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
                priority,
                group_cwd,
                group_interactive,
                inherited_paths,
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
            description,
        } => AliasValue::Meta {
            cmd,
            cwd: inherited_cwd.map(String::from),
            interactive: interactive.or(inherited_interactive),
            description, // $description 不继承，直接透传
        },
        AliasValue::Meta {
            cmd,
            cwd: Some(c),
            interactive,
            description,
        } => AliasValue::Meta {
            cmd,
            cwd: Some(c), // 别名自己的 $cwd 优先
            interactive: interactive.or(inherited_interactive),
            description, // $description 不继承，直接透传
        },
        AliasValue::Str(cmd) => {
            if inherited_cwd.is_some() || inherited_interactive.is_some() {
                AliasValue::Meta {
                    cmd,
                    cwd: inherited_cwd.map(String::from),
                    interactive: inherited_interactive,
                    description: None,
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
            Some(f.path.as_path()),
            f.priority,
            f.inherited_cwd.as_deref(),
            f.inherited_interactive,
            &f.inherited_paths,
        );
    }
    merged
}

/// 扫描本地和全局别名文件，返回 (合并配置, 文件列表)。
///
/// - ~/.byk 存在 → 本地 + 全局别名，通过缓存避免重复扫描和合并
/// - ~/.byk 不存在 → 仅扫描 cwd 的本地别名，不走缓存不写文件
pub fn load_merged_aliases(layout: &PathLayout) -> (MergedConfig, Vec<AliasFile>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if layout.home_exists {
        let cache_file = layout.cache_dir.join("alias.json");
        let global_dir = &layout.alias_dir;

        let (merged, mut files) = load_alias_cache(&cache_file, &cwd, global_dir);
        sort_alias_files(&mut files);
        (merged, files)
    } else {
        let mut files = scan_alias_files(&cwd, false);
        sort_alias_files(&mut files);
        let merged = build_merged_aliases(&files);
        (merged, files)
    }
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

/// 在所有别名文件中查找同名的全部别名（用于 --info 查询路由）。
///
/// 与 `resolve_merged_alias` 不同，此函数遍历所有 AliasFile，
/// 返回所有包含该 key 路径的 ResolvedAlias，按文件优先级排序。
/// 这暴露了"谁覆盖了谁"的信息。
pub fn lookup_all_aliases(files: &[AliasFile], name: &str) -> Vec<ResolvedAlias> {
    let parts: Vec<&str> = name.split('.').collect();
    let mut results: Vec<ResolvedAlias> = Vec::new();

    for f in files {
        // 负数优先级文件不参与普通合并，但 --info 仍应展示
        let mut current: &serde_json::Map<String, serde_json::Value> = &f.aliases;

        // 沿路径遍历到倒数第二级
        let mut group_cwd = f.inherited_cwd.as_deref();
        let mut group_interactive = f.inherited_interactive;

        for (i, part) in parts.iter().enumerate() {
            let val = match current.get(*part) {
                Some(v) => v,
                None => break,
            };

            if i == parts.len() - 1 {
                // 最后一级：检查是否为别名值
                if is_alias_value(val) {
                    if let Some(av) = to_alias_value(val) {
                        let av = apply_inherited(av, group_cwd, group_interactive);
                        results.push(ResolvedAlias {
                            value: av,
                            source: f.key.clone(),
                            source_path: Some(f.path.clone()),
                            priority: f.priority,
                            paths: f.inherited_paths.clone(),
                        });
                    }
                }
                break;
            }

            // 中间级：必须是 object 才能继续遍历
            if let serde_json::Value::Object(inner) = val {
                // 累积分组级继承属性
                if let Some(c) = inner.get("$cwd").and_then(|v| v.as_str()) {
                    group_cwd = Some(c);
                }
                if let Some(v) = inner.get("$interactive") {
                    if let Some(b) = v.as_bool() {
                        group_interactive = Some(b);
                    }
                }
                current = inner;
            } else {
                break;
            }
        }
    }

    // 按优先级从高到低排列，高优先级（实际执行的）排前面
    results.reverse();
    results
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
            priority: 0,
            paths: Vec::new(),
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
            AliasValue::Meta { cmd, cwd, interactive, .. } => {
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
            description: None,
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
            description: None,
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
            description: None,
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
            description: None,
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

    // ==================== inherited_paths ====================

    #[test]
    fn inherited_paths_flows_to_resolved_alias() {
        let mut aliases = serde_json::Map::new();
        aliases.insert(
            "build".into(),
            serde_json::Value::String("make build".into()),
        );
        let files = vec![AliasFile {
            key: "@test".into(),
            priority: 0,
            aliases,
            path: PathBuf::from("/fake/test.byk.json"),
            inherited_cwd: None,
            inherited_interactive: None,
            inherited_paths: vec!["./scripts".into(), "~/tools/bin".into()],
        }];
        let merged = build_merged_aliases(&files);
        let resolved = resolve_merged_alias(&merged, "build").unwrap();
        assert_eq!(resolved.paths, vec!["./scripts", "~/tools/bin"]);
    }

    #[test]
    fn inherited_paths_empty_by_default() {
        let mut aliases = serde_json::Map::new();
        aliases.insert(
            "build".into(),
            serde_json::Value::String("make build".into()),
        );
        let files = vec![AliasFile {
            key: "@test".into(),
            priority: 0,
            aliases,
            path: PathBuf::from("/fake/test.byk.json"),
            inherited_cwd: None,
            inherited_interactive: None,
            inherited_paths: Vec::new(),
        }];
        let merged = build_merged_aliases(&files);
        let resolved = resolve_merged_alias(&merged, "build").unwrap();
        assert!(resolved.paths.is_empty());
    }
}
