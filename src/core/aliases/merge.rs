//! 按优先级把多个 AliasFile 合并成一棵 MergedConfig 树，并支持路径收集与查找。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::parse::{is_alias_value, to_alias_value};
use super::scan::{scan_alias_files, sort_alias_files};
use super::types::{AliasFile, AliasValue, MergedConfig, ResolvedAlias};
use crate::core::paths::PathLayout;

/// 深度合并上下文，打包所有跨递归传递的参数。
#[derive(Copy, Clone)]
struct MergeContext<'a> {
    file_key: &'a str,
    source_path: Option<&'a Path>,
    inherited_cwd: Option<&'a str>,
    inherited_interactive: Option<bool>,
    inherited_paths: &'a [String],
}

/// 将 source 深度合并进 target。
///
/// 同一层可以同时有 alias（`$cmd` 叶子命令）和 children（嵌套分组）：
/// string 或含 `$` 前缀的 object 落到 alias，其余 object 递归进 children。
fn deep_merge_dict(
    target: &mut MergedConfig,
    source: &serde_json::Map<String, serde_json::Value>,
    ctx: &MergeContext,
) {
    // 提取当前分组的 $cwd / $interactive，子级覆盖父级
    let group_cwd = source
        .get("$cwd")
        .and_then(|v| v.as_str())
        .or(ctx.inherited_cwd);
    let group_interactive = source
        .get("$interactive")
        .and_then(|v| v.as_bool())
        .or(ctx.inherited_interactive);

    for (key, val) in source {
        if key.starts_with('$') {
            continue;
        }

        if is_alias_value(val) {
            let alias_val = to_alias_value(val);
            if let Some(av) = alias_val {
                let av = apply_inherited(av, group_cwd, group_interactive);
                let entry = target.entry(key.clone()).or_default();
                entry.alias = Some(ResolvedAlias {
                    value: av,
                    source: ctx.file_key.to_string(),
                    source_path: ctx.source_path.map(|p| p.to_path_buf()),
                    paths: ctx.inherited_paths.to_vec(),
                });

                if let serde_json::Value::Object(inner) = val {
                    deep_merge_dict(
                        &mut entry.children,
                        inner,
                        &MergeContext {
                            inherited_cwd: group_cwd,
                            inherited_interactive: group_interactive,
                            ..*ctx
                        },
                    );
                }
            }
        } else if let serde_json::Value::Object(inner) = val {
            let entry = target.entry(key.clone()).or_default();
            deep_merge_dict(
                &mut entry.children,
                inner,
                &MergeContext {
                    inherited_cwd: group_cwd,
                    inherited_interactive: group_interactive,
                    ..*ctx
                },
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

/// 负数 priority 的文件不参与合并（只能 `@file.key` 精确调用）。
pub(crate) fn build_merged_aliases(files: &[AliasFile]) -> MergedConfig {
    let mut merged: MergedConfig = HashMap::new();
    for f in files {
        if f.priority < 0 {
            continue;
        }
        deep_merge_dict(
            &mut merged,
            &f.aliases,
            &MergeContext {
                file_key: &f.key,
                source_path: Some(f.path.as_path()),
                inherited_cwd: f.inherited_cwd.as_deref(),
                inherited_interactive: f.inherited_interactive,
                inherited_paths: &f.inherited_paths,
            },
        );
    }
    merged
}

/// 扫描当前目录与全局目录，返回合并配置和文件列表。
///
/// 每次调用都重新扫描：几十个文件的扫描以毫秒计，不值得为此维护缓存状态。
pub fn load_merged_aliases(layout: &PathLayout) -> (MergedConfig, Vec<AliasFile>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    load_merged_aliases_at(layout, &cwd)
}

/// 指定工作目录加载（测试与需要显式 cwd 的调用方使用）。
pub(crate) fn load_merged_aliases_at(
    layout: &PathLayout,
    cwd: &Path,
) -> (MergedConfig, Vec<AliasFile>) {
    let mut files = scan_alias_files(cwd, false);
    files.extend(scan_alias_files(&layout.alias_dir, true));
    sort_alias_files(&mut files);

    // cwd 恰好是全局目录时，同一批文件会被扫两次（@f 与 @@f）。
    // 两个身份都保留——这个文件确实同时可通过两种前缀访问；
    // 但合并只做一次，按全局身份参与，避免同一份声明被合并两遍。
    let merged = if same_dir(cwd, &layout.alias_dir) {
        let global_only: Vec<AliasFile> = files
            .iter()
            .filter(|f| f.key.starts_with("@@"))
            .cloned()
            .collect();
        build_merged_aliases(&global_only)
    } else {
        build_merged_aliases(&files)
    };

    (merged, files)
}

/// 两个路径是否指向同一目录（规范化后比较，符号链接也视作相同）。
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(pa), Ok(pb)) => pa == pb,
        _ => a == b,
    }
}

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

/// 沿点号路径遍历合并树，最后一级节点有 alias 则返回。
pub fn resolve_merged_alias<'a>(merged: &'a MergedConfig, name: &str) -> Option<&'a ResolvedAlias> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::aliases::types::MergedNode;
    use std::fs;

    /// 构造一个 root/alias 布局，返回 (临时目录, 布局, 工作目录)
    fn project() -> (tempfile::TempDir, PathLayout, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let alias_dir = tmp.path().join("alias");
        let cwd = tmp.path().join("proj");
        fs::create_dir_all(&alias_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let layout = PathLayout {
            root_dir: tmp.path().to_path_buf(),
            alias_dir,
        };
        (tmp, layout, cwd)
    }

    fn make_alias(cmd: &str) -> ResolvedAlias {
        ResolvedAlias {
            value: AliasValue::Str(cmd.into()),
            source: "@test".into(),
            source_path: None,
            paths: Vec::new(),
        }
    }

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
            AliasValue::Meta {
                cmd,
                cwd,
                interactive,
                ..
            } => {
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
            AliasValue::Meta {
                cmd, interactive, ..
            } => {
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

    #[test]
    fn paths_empty_config() {
        let merged: MergedConfig = HashMap::new();
        assert!(collect_merged_paths(&merged, "").is_empty());
    }

    #[test]
    fn paths_single_leaf() {
        let mut merged: MergedConfig = HashMap::new();
        let node = MergedNode {
            alias: Some(make_alias("cmd")),
            ..Default::default()
        };
        merged.insert("greet".into(), node);

        let result = collect_merged_paths(&merged, "");
        assert_eq!(result, vec!["greet"]);
    }

    #[test]
    fn paths_nested() {
        let mut merged: MergedConfig = HashMap::new();
        let child = MergedNode {
            alias: Some(make_alias("child_cmd")),
            ..Default::default()
        };
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
        let child = MergedNode {
            alias: Some(make_alias("sub_cmd")),
            ..Default::default()
        };
        let mut parent = MergedNode {
            alias: Some(make_alias("parent_cmd")),
            ..Default::default()
        };
        parent.children.insert("sub".into(), child);
        merged.insert("ns".into(), parent);

        let mut result = collect_merged_paths(&merged, "");
        result.sort();
        assert_eq!(result, vec!["ns", "ns.sub"]);
    }

    #[test]
    fn paths_with_prefix() {
        let mut merged: MergedConfig = HashMap::new();
        let node = MergedNode {
            alias: Some(make_alias("cmd")),
            ..Default::default()
        };
        merged.insert("leaf".into(), node);

        let result = collect_merged_paths(&merged, "root");
        assert_eq!(result, vec!["root.leaf"]);
    }

    #[test]
    fn paths_node_without_alias_but_with_children() {
        let mut merged: MergedConfig = HashMap::new();
        let child = MergedNode {
            alias: Some(make_alias("leaf_cmd")),
            ..Default::default()
        };
        let mut parent = MergedNode::default();
        // parent 没有 alias，只有 children
        parent.children.insert("leaf".into(), child);
        merged.insert("group".into(), parent);

        let result = collect_merged_paths(&merged, "");
        // "group" 本身没有 alias，不出现；"group.leaf" 有
        assert_eq!(result, vec!["group.leaf"]);
    }

    #[test]
    fn resolve_found() {
        let mut merged: MergedConfig = HashMap::new();
        let node = MergedNode {
            alias: Some(make_alias("echo hello")),
            ..Default::default()
        };
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
        let child = MergedNode {
            alias: Some(make_alias("nested_cmd")),
            ..Default::default()
        };
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

    #[test]
    fn local_file_overrides_global_by_default() {
        let (_tmp, layout, cwd) = project();
        fs::write(
            layout.alias_dir.join("g.byk.json"),
            r#"{"dev": "echo global"}"#,
        )
        .unwrap();
        fs::write(cwd.join("p.byk.json"), r#"{"dev": "echo local"}"#).unwrap();

        let (merged, _) = load_merged_aliases_at(&layout, &cwd);
        let resolved = resolve_merged_alias(&merged, "dev").unwrap();
        assert_eq!(resolved.source, "@p", "本地文件应覆盖全局同名别名");
    }

    #[test]
    fn negative_priority_opts_out_of_merge() {
        let (_tmp, layout, cwd) = project();
        fs::write(
            layout.alias_dir.join("g.byk.json"),
            r#"{"dev": "echo global"}"#,
        )
        .unwrap();
        fs::write(
            cwd.join("p.byk.json"),
            r#"{"$priority": -1, "dev": "echo local"}"#,
        )
        .unwrap();

        let (merged, files) = load_merged_aliases_at(&layout, &cwd);
        let resolved = resolve_merged_alias(&merged, "dev").unwrap();
        assert_eq!(resolved.source, "@@g", "-1 的文件不应参与合并");
        // 但文件本身仍被扫描到，@p.dev 可精确调用
        assert!(files.iter().any(|f| f.key == "@p"));
    }

    #[test]
    fn global_dir_as_cwd_merges_once_via_global_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let alias_dir = tmp.path().to_path_buf();
        fs::write(alias_dir.join("f.byk.json"), r#"{"hi": "echo hi"}"#).unwrap();

        let layout = PathLayout {
            root_dir: tmp.path().to_path_buf(),
            alias_dir: alias_dir.clone(),
        };
        let (merged, files) = load_merged_aliases_at(&layout, &alias_dir);

        // 两个身份都保留：@f 与 @@f 都能精确调用同一个文件
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.key == "@f"));
        assert!(files.iter().any(|f| f.key == "@@f"));

        // 合并只发生一次，来源是全局身份
        let resolved = resolve_merged_alias(&merged, "hi").unwrap();
        assert_eq!(resolved.source, "@@f");
    }
}
