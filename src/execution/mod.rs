//! 别名执行：环境构建、交互模式、执行入口。
//!
//! 执行门面：协调环境、递归保护、交互和进程启动。
//! 执行前的只读解析在 `prepare` 子模块。

use std::io::IsTerminal;
use std::process::exit;

use crate::model::{Mode, ResolvedAlias};

mod chain;
mod environment;
mod interactive;
mod placeholder;
mod prepare;
mod quote;
mod runner;

pub(crate) use prepare::{ResolvedCommand, prepare};

use self::placeholder::collect_placeholders;

use chain::{ChainError, ENV_ALLOW_RECURSION, env_truthy, inherited_stack, push_identity};
use environment::{absolutize_paths, resolve_working_dir};
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
    let definition = resolved.definition.clone();

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
