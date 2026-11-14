//! 统一别名查找：精确语法优先，否则查合并命名空间。

use super::exact::{ExactLookup, parse_exact_syntax, resolve_exact_alias};
use super::merge::{collect_merged_paths, load_merged, resolve_merged_alias};
use crate::core::config::{MergedConfig, ResolvedAlias};
use crate::core::paths::PathLayout;

/// 命中的别名及其展示用来源串。
pub struct LookupMatch {
    pub resolved: ResolvedAlias,
    pub display_source: String,
}

/// 查找失败的原因。
#[derive(Debug)]
pub enum LookupFailure {
    FileNotFound {
        file_key: String,
    },
    /// 文件存在，但读取、解析或链接目标加载失败。
    FileUnavailable {
        file_key: String,
        reason: String,
    },
    /// `$` 开头的路径段是保留字段或给其他程序的自定义字段，永远不是别名。
    ReservedField {
        file_key: String,
        alias_key: String,
    },
    KeyNotFound {
        file_key: String,
        alias_key: String,
    },
    /// 普通查找失败；`suggestions` 供终端提示使用，程序接口可忽略。
    AliasNotFound {
        key: String,
        suggestions: Vec<String>,
    },
}

/// 按 `key` 查找别名：精确语法只解析目标文件，其余走合并命名空间。
pub fn lookup(layout: &PathLayout, key: &str) -> Result<LookupMatch, LookupFailure> {
    if let Some((file_key, alias_key)) = parse_exact_syntax(key) {
        return match resolve_exact_alias(layout, &file_key, &alias_key) {
            ExactLookup::Found {
                resolved,
                display_source,
            } => Ok(LookupMatch {
                resolved,
                display_source,
            }),
            ExactLookup::FileNotFound => Err(LookupFailure::FileNotFound { file_key }),
            ExactLookup::FileUnavailable { reason } => {
                Err(LookupFailure::FileUnavailable { file_key, reason })
            }
            ExactLookup::ReservedField { key: alias_key } => Err(LookupFailure::ReservedField {
                file_key,
                alias_key,
            }),
            ExactLookup::KeyNotFound => Err(LookupFailure::KeyNotFound {
                file_key,
                alias_key,
            }),
        };
    }

    let merged = load_merged(layout);
    match resolve_merged_alias(&merged, key) {
        Some(resolved) => Ok(LookupMatch {
            resolved: resolved.clone(),
            display_source: format!("{}.{}", resolved.source, key),
        }),
        None => Err(LookupFailure::AliasNotFound {
            key: key.to_string(),
            suggestions: suggestions(&merged, key),
        }),
    }
}

/// 与输入同层级或互为父级的别名，最多 5 条。
fn suggestions(merged: &MergedConfig, key: &str) -> Vec<String> {
    let descendants = format!("{}.", key);
    collect_merged_paths(merged, "")
        .into_iter()
        .filter(|item| item.starts_with(&descendants) || key.starts_with(&format!("{}.", item)))
        .take(5)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{AliasValue, MergedNode};
    use std::collections::HashMap;
    use std::fs;

    fn temp_layout() -> (tempfile::TempDir, PathLayout) {
        let tmp = tempfile::tempdir().unwrap();
        let alias_dir = tmp.path().join("alias");
        fs::create_dir_all(&alias_dir).unwrap();
        let layout = PathLayout {
            root_dir: tmp.path().to_path_buf(),
            alias_dir,
        };
        (tmp, layout)
    }

    fn leaf() -> MergedNode {
        MergedNode {
            alias: Some(ResolvedAlias {
                value: AliasValue::Str("echo x".into()),
                source: "@test".into(),
                source_path: None,
                paths: Vec::new(),
            }),
            children: HashMap::new(),
        }
    }

    fn branch(children: &[(&str, MergedNode)]) -> MergedNode {
        MergedNode {
            alias: None,
            children: children
                .iter()
                .map(|(key, node)| (key.to_string(), node.clone()))
                .collect(),
        }
    }

    #[test]
    fn exact_hit_keeps_exact_display_source() {
        let (_tmp, layout) = temp_layout();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build": "make build"}"#,
        )
        .unwrap();

        let hit = lookup(&layout, "@@ops.build").unwrap();
        assert_eq!(hit.resolved.source, "@@ops");
        assert_eq!(hit.display_source, "@@ops.build");
    }

    #[test]
    fn exact_failures_keep_their_identity() {
        let (_tmp, layout) = temp_layout();
        assert!(matches!(
            lookup(&layout, "@@nope.build"),
            Err(LookupFailure::FileNotFound { .. })
        ));

        fs::write(layout.alias_dir.join("broken.byk.json"), "{not json").unwrap();
        assert!(matches!(
            lookup(&layout, "@@broken.build"),
            Err(LookupFailure::FileUnavailable { .. })
        ));

        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":{"$cmd":"echo hi"}}"#,
        )
        .unwrap();
        assert!(matches!(
            lookup(&layout, "@@ops.build.$cmd"),
            Err(LookupFailure::ReservedField { .. })
        ));
        assert!(matches!(
            lookup(&layout, "@@ops.deploy"),
            Err(LookupFailure::KeyNotFound { .. })
        ));
    }

    #[test]
    fn suggestions_include_children_and_parents() {
        let merged: MergedConfig = [(
            "deploy".to_string(),
            branch(&[("dev", leaf()), ("prod", leaf())]),
        )]
        .into_iter()
        .collect();

        let children = suggestions(&merged, "deploy");
        assert!(children.contains(&"deploy.dev".to_string()));
        assert!(children.contains(&"deploy.prod".to_string()));

        assert_eq!(
            suggestions(&merged, "deploy.prod.extra"),
            vec!["deploy.prod".to_string()]
        );
    }
}
