//! 执行前准备：只解析，不执行。
//!
//! `execute_alias` 与 `api::resolve` 共用这里的规则，保证
//! `byk __api__ resolve` 的结果与真正执行时的命令、cwd、PATH 完全一致。

use std::path::PathBuf;

use crate::model::{Mode, ResolvedAlias};

use super::environment::{absolutize_paths, build_path_prefixes, resolve_working_dir};
use super::placeholder::parse_alias_arguments_with_mapping;

/// 别名的解析结果（只解析、不执行），供 `byk __api__ resolve` 使用。
#[derive(Debug, Clone)]
pub(crate) struct ResolvedCommand {
    /// 最终命令：非交互别名已按参数解析占位符，交互别名为未解析模板
    pub(crate) command: String,
    /// 解析后的工作目录（`$cwd` + `~` 展开 + 相对路径已解析）
    pub(crate) cwd: PathBuf,
    /// 需要前置到 PATH 的目录列表（含 node_modules/.bin，已绝对化）
    pub(crate) paths: Vec<String>,
    /// 执行方式：`normal` / `interactive` / `silent`
    pub(crate) mode: Mode,
    /// 别名描述
    pub(crate) description: Option<String>,
}

/// 解析别名但不执行：产出最终命令、工作目录与 PATH 前缀。
///
/// 与 `execute_alias` 共用同一套解析逻辑（变量替换已在扫描期完成，
/// 此处只处理 `$cwd`/`$paths` 继承与占位符），保证 `resolve` 的结果
/// 与 `byk <key>` 实际执行的命令完全一致。
///
/// 交互别名无法在无终端下确定占位符值，因此原样返回模板并置
/// `mode = "interactive"`，由调用方（插件）决定回退到 byk 或自行交互。
pub(crate) fn prepare(resolved: &ResolvedAlias, args: &[String]) -> ResolvedCommand {
    let definition = &resolved.definition;

    // 相对 $paths 以配置文件所在目录为基准，与 execute_alias 保持一致
    let config_dir = resolved.source_path.as_deref().and_then(|p| p.parent());
    let mut custom_paths = absolutize_paths(&resolved.paths, config_dir);
    // $deps 声明的可执行目录（venv 的 bin / node_modules/.bin）也自动前置到 PATH
    custom_paths.extend(resolved.prepend_paths.iter().cloned());
    let working_dir = resolve_working_dir(definition.cwd.as_deref(), config_dir);
    let paths = build_path_prefixes(&working_dir, &custom_paths);

    let command = if definition.mode == Mode::Interactive {
        definition.command.clone()
    } else {
        parse_alias_arguments_with_mapping(&definition.command, args, &[]).0
    };

    ResolvedCommand {
        command,
        cwd: working_dir,
        paths,
        mode: definition.mode,
        description: definition.description.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::to_alias_definition;

    fn resolved_str(cmd: &str) -> ResolvedAlias {
        use crate::model::AliasValue;
        ResolvedAlias {
            definition: to_alias_definition(&AliasValue::Str(cmd.into())),
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: Vec::new(),
        }
    }

    #[test]
    fn resolve_simple_string() {
        let rc = prepare(&resolved_str("echo hi"), &[]);
        assert_eq!(rc.command, "echo hi");
        assert_eq!(rc.mode, Mode::Normal);
        assert!(rc.description.is_none());
        assert!(rc.cwd.is_absolute());
        assert!(rc.paths.is_empty());
    }

    #[test]
    fn resolve_placeholder_with_args() {
        let rc = prepare(
            &resolved_str("echo {name}"),
            &["name".into(), "Alice".into()],
        );
        assert_eq!(rc.command, "echo Alice");
        assert_eq!(rc.mode, Mode::Normal);
    }

    #[test]
    fn resolve_interactive_returns_template() {
        use crate::model::AliasValue;
        let resolved = ResolvedAlias {
            definition: to_alias_definition(&AliasValue::Meta {
                cmd: "deploy --env {env}".into(),
                cwd: None,
                mode: Some(Mode::Interactive),
                description: Some("部署".into()),
                recursive: None,
            }),
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: Vec::new(),
        };
        let rc = prepare(&resolved, &[]);
        assert_eq!(rc.mode, Mode::Interactive);
        assert_eq!(rc.command, "deploy --env {env}");
        assert_eq!(rc.description.as_deref(), Some("部署"));
    }

    #[test]
    fn resolve_carries_silent() {
        use crate::model::AliasValue;
        let resolved = ResolvedAlias {
            definition: to_alias_definition(&AliasValue::Meta {
                cmd: "echo quiet".into(),
                cwd: None,
                mode: Some(Mode::Silent),
                description: None,
                recursive: None,
            }),
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: Vec::new(),
        };
        let rc = prepare(&resolved, &[]);
        assert_eq!(rc.mode, Mode::Silent);
    }

    #[test]
    fn resolve_meta_cwd_and_paths() {
        use crate::model::AliasValue;
        let dir = tempfile::tempdir().unwrap();
        let nm_bin = dir.path().join("node_modules").join(".bin");
        std::fs::create_dir_all(&nm_bin).unwrap();

        let resolved = ResolvedAlias {
            definition: to_alias_definition(&AliasValue::Meta {
                cmd: "make".into(),
                cwd: Some(dir.path().to_string_lossy().into_owned()),
                mode: None,
                description: None,
                recursive: None,
            }),
            source: "@t".into(),
            source_path: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            prepend_paths: Vec::new(),
        };

        let rc = prepare(&resolved, &[]);
        assert_eq!(rc.command, "make");
        assert_eq!(rc.mode, Mode::Normal);
        // $cwd 解析到 dir，且 node_modules/.bin 被纳入 PATH 前缀
        assert_eq!(rc.cwd, dir.path());
        let nm = nm_bin.to_string_lossy().into_owned();
        assert!(rc.paths.contains(&nm));
    }

    #[test]
    fn resolve_meta_with_prepend_paths_prepends_it() {
        use crate::model::AliasValue;
        let dir = tempfile::tempdir().unwrap();
        let venv_bin = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();

        let resolved = ResolvedAlias {
            definition: to_alias_definition(&AliasValue::Str("python -V".into())),
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: vec![venv_bin.to_string_lossy().into_owned()],
        };

        let rc = prepare(&resolved, &[]);
        // $deps 推导出的可执行目录被自动前置到 PATH
        let bin = venv_bin.to_string_lossy().into_owned();
        assert!(rc.paths.contains(&bin));
    }
}
