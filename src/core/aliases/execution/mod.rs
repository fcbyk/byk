//! 别名执行：环境构建、交互模式、执行入口。
//!
//! 执行门面：协调环境、递归保护、交互和进程启动。

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::exit;

use crate::core::config::{Mode, ResolvedAlias, to_alias_definition};

use super::placeholder::{collect_placeholders, parse_alias_arguments_with_mapping};

mod chain;
mod environment;
mod interactive;
mod runner;

use chain::{ChainError, ENV_ALLOW_RECURSION, env_truthy, inherited_stack, push_identity};
use environment::{absolutize_paths, build_path_prefixes, resolve_working_dir};
use interactive::execute_interactive_impl;
use runner::execute_direct_impl;

/// 执行别名命令。
///
/// 变量（内置 `${byk_dir}` / `${file_dir}` 与文件 `$var`）已在扫描期替换完毕，
/// 此处只剩参数占位符。`$mode` 决定怎么跑，三态互斥：
/// - `normal`：打印提示行后直接执行；
/// - `interactive`：打印提示行，逐项提示占位符，确认后执行；
/// - `silent`：连提示行都不打印，stdout 只剩子进程输出。
///
/// 入口先做调用链检查：命中重入就报错退出，而不是把「一行配置笔误」
/// 变成上千个进程的进程链。`silent` 也压不住这条错误——它本来就是
/// 用来解释「命令为什么卡住」的。
pub fn execute_alias(resolved: &ResolvedAlias, args: &[String], display_source: &str) {
    let definition = match to_alias_definition(&resolved.value) {
        Some(d) => d,
        None => {
            eprintln!("Invalid alias definition");
            exit(1);
        }
    };

    let chain = match push_identity(
        &inherited_stack(),
        display_source,
        definition.recursive || env_truthy(ENV_ALLOW_RECURSION),
    ) {
        Ok(chain) => chain,
        Err(err) => {
            eprintln!("Error: {}", err.message());
            if matches!(err, ChainError::Cycle { .. }) {
                eprintln!(
                    "Hint: if this self-invocation is intentional, add \"$recursive\": true to the alias or set {}=1",
                    ENV_ALLOW_RECURSION
                );
            }
            exit(1);
        }
    };
    let alias_stack = chain.join(",");

    let command = definition.command;
    let cwd = definition.cwd;
    let mode = definition.mode;

    // 相对 $paths 以配置文件所在目录为基准，与调用时的当前目录无关
    let config_dir = resolved.source_path.as_deref().and_then(|p| p.parent());
    let mut custom_paths = absolutize_paths(&resolved.paths, config_dir);
    // $deps 声明的可执行目录（venv 的 bin / node_modules/.bin）也自动前置到 PATH
    custom_paths.extend(resolved.prepend_paths.iter().cloned());

    let working_dir = resolve_working_dir(
        cwd.as_deref(),
        resolved.source_path.as_deref().and_then(|p| p.parent()),
    );
    let placeholders = collect_placeholders(&command);

    let announce = should_announce(mode);
    if announce {
        eprintln!("\n> {} {}", display_source, working_dir.display());
    }

    if mode == Mode::Interactive {
        execute_interactive_impl(
            &command,
            args,
            &placeholders,
            &working_dir,
            &custom_paths,
            &alias_stack,
        );
    } else {
        execute_direct_impl(
            &command,
            args,
            &working_dir,
            &custom_paths,
            announce,
            &alias_stack,
        );
    }
}

/// 提示行该不该打印：别名不是 `silent`，且 stderr 落在终端上。
///
/// 检查 stderr 而不是 stdout 是刻意的——提示行本来就写 stderr，判断「有没有人在看」
/// 得看它自己的去处。若检查 stdout，`byk build | grep error` 时管道会让 stdout
/// 不再是终端，提示被吞掉，可终端前的人其实正盯着这两行。
fn should_announce(mode: Mode) -> bool {
    mode != Mode::Silent && std::io::stderr().is_terminal()
}

/// 别名的解析结果（只解析、不执行），供 `byk __api__ resolve` 使用。
#[derive(Debug, Clone)]
pub struct ResolvedCommand {
    /// 最终命令：非交互别名已按参数解析占位符，交互别名为未解析模板
    pub command: String,
    /// 解析后的工作目录（`$cwd` + `~` 展开 + 相对路径已解析）
    pub cwd: PathBuf,
    /// 需要前置到 PATH 的目录列表（含 node_modules/.bin，已绝对化）
    pub paths: Vec<String>,
    /// 执行方式：`normal` / `interactive` / `silent`
    pub mode: Mode,
    /// 别名描述
    pub description: Option<String>,
}

/// 解析别名但不执行：产出最终命令、工作目录与 PATH 前缀。
///
/// 与 `execute_alias` 共用同一套解析逻辑（变量替换已在扫描期完成，
/// 此处只处理 `$cwd`/`$paths` 继承与占位符），保证 `resolve` 的结果
/// 与 `byk <key>` 实际执行的命令完全一致。
///
/// 交互别名无法在无终端下确定占位符值，因此原样返回模板并置
/// `mode = "interactive"`，由调用方（插件）决定回退到 byk 或自行交互。
pub fn resolve_alias(resolved: &ResolvedAlias, args: &[String]) -> ResolvedCommand {
    let definition = match to_alias_definition(&resolved.value) {
        Some(d) => d,
        None => {
            return ResolvedCommand {
                command: String::new(),
                cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                paths: Vec::new(),
                mode: Mode::Normal,
                description: None,
            };
        }
    };

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
        description: definition.description,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved_str(cmd: &str) -> ResolvedAlias {
        use crate::core::config::AliasValue;
        ResolvedAlias {
            value: AliasValue::Str(cmd.into()),
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: Vec::new(),
        }
    }

    #[test]
    fn resolve_simple_string() {
        let rc = resolve_alias(&resolved_str("echo hi"), &[]);
        assert_eq!(rc.command, "echo hi");
        assert_eq!(rc.mode, Mode::Normal);
        assert!(rc.description.is_none());
        assert!(rc.cwd.is_absolute());
        assert!(rc.paths.is_empty());
    }

    #[test]
    fn resolve_placeholder_with_args() {
        let rc = resolve_alias(
            &resolved_str("echo {name}"),
            &["name".into(), "Alice".into()],
        );
        assert_eq!(rc.command, "echo Alice");
        assert_eq!(rc.mode, Mode::Normal);
    }

    #[test]
    fn resolve_interactive_returns_template() {
        use crate::core::config::AliasValue;
        let resolved = ResolvedAlias {
            value: AliasValue::Meta {
                cmd: "deploy --env {env}".into(),
                cwd: None,
                mode: Some(Mode::Interactive),
                description: Some("部署".into()),
                recursive: None,
            },
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: Vec::new(),
        };
        let rc = resolve_alias(&resolved, &[]);
        assert_eq!(rc.mode, Mode::Interactive);
        assert_eq!(rc.command, "deploy --env {env}");
        assert_eq!(rc.description.as_deref(), Some("部署"));
    }

    #[test]
    fn resolve_carries_silent() {
        use crate::core::config::AliasValue;
        let resolved = ResolvedAlias {
            value: AliasValue::Meta {
                cmd: "echo quiet".into(),
                cwd: None,
                mode: Some(Mode::Silent),
                description: None,
                recursive: None,
            },
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: Vec::new(),
        };
        let rc = resolve_alias(&resolved, &[]);
        assert_eq!(rc.mode, Mode::Silent);
    }

    #[test]
    fn resolve_meta_cwd_and_paths() {
        use crate::core::config::AliasValue;
        let dir = tempfile::tempdir().unwrap();
        let nm_bin = dir.path().join("node_modules").join(".bin");
        std::fs::create_dir_all(&nm_bin).unwrap();

        let resolved = ResolvedAlias {
            value: AliasValue::Meta {
                cmd: "make".into(),
                cwd: Some(dir.path().to_string_lossy().into_owned()),
                mode: None,
                description: None,
                recursive: None,
            },
            source: "@t".into(),
            source_path: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            prepend_paths: Vec::new(),
        };

        let rc = resolve_alias(&resolved, &[]);
        assert_eq!(rc.command, "make");
        assert_eq!(rc.mode, Mode::Normal);
        // $cwd 解析到 dir，且 node_modules/.bin 被纳入 PATH 前缀
        assert_eq!(rc.cwd, dir.path());
        let nm = nm_bin.to_string_lossy().into_owned();
        assert!(rc.paths.contains(&nm));
    }

    #[test]
    fn resolve_meta_with_prepend_paths_prepends_it() {
        use crate::core::config::AliasValue;
        let dir = tempfile::tempdir().unwrap();
        let venv_bin = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&venv_bin).unwrap();

        let resolved = ResolvedAlias {
            value: AliasValue::Str("python -V".into()),
            source: "@t".into(),
            source_path: None,
            paths: Vec::new(),
            prepend_paths: vec![venv_bin.to_string_lossy().into_owned()],
        };

        let rc = resolve_alias(&resolved, &[]);
        // $deps 推导出的可执行目录被自动前置到 PATH
        let bin = venv_bin.to_string_lossy().into_owned();
        assert!(rc.paths.contains(&bin));
    }
}
