mod cli;
mod core;
mod render;
mod utils;

use clap::{CommandFactory, Parser};
use std::process::exit;

use cli::{Cli, Commands, extract_options};
use core::aliases;
use core::filecmd;
use core::paths::PathLayout;

/// 管道下游提前关闭时（如 `byk -h | head`），按 Unix 惯例静默退出，
/// 而不是让 `println!` 抛出 `Broken pipe` 再 panic。
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: 只把 SIGPIPE 的处置恢复为默认值，不接触内存或指针。
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() {
    restore_sigpipe();

    let cmd = Cli::command();
    let options = extract_options(&cmd);
    let layout = PathLayout::new();

    let cli = Cli::parse();

    // 隐藏接口 __api__
    match cli.command {
        Some(Commands::Api { request }) => {
            core::api::dispatch(&request, &layout);
            return;
        }
        None => {}
    }

    // 全局选项
    if cli.help {
        render::help::render_all(&layout, &options);
        return;
    }
    if cli.check {
        if !cli.trailing.is_empty() {
            eprintln!("check takes no arguments");
            exit(1);
        }
        if render::check::render(&layout) != 0 {
            exit(1);
        }
        return;
    }
    if cli.paths {
        render::paths::render(&layout);
        return;
    }
    if cli.version {
        render::version::render();
        return;
    }
    if cli.trailing.is_empty() {
        render::help::render_all(&layout, &options);
        return;
    }

    let command_name = &cli.trailing[0];
    let command_args = &cli.trailing[1..];

    // 文件命令：不扫描别名，先于合并命名空间处理
    match filecmd::parse(command_name) {
        filecmd::ParseResult::NotFileCommand => {}
        filecmd::ParseResult::Command(command) => {
            filecmd::dispatch(&command, command_args, &layout);
            return;
        }
        filecmd::ParseResult::Error(err) => {
            eprintln!("{}", err.render(command_name));
            exit(1);
        }
    }

    // 精确语法 @file.key / @@file.key 优先；其余走合并命名空间
    match aliases::lookup(&layout, command_name) {
        Ok(hit) => aliases::execute_alias(&hit.resolved, command_args, &hit.display_source),
        Err(failure) => report_lookup_failure(failure),
    }
}

/// 查找失败时输出终端文案并退出。
fn report_lookup_failure(failure: aliases::LookupFailure) -> ! {
    match failure {
        aliases::LookupFailure::FileNotFound { file_key } => {
            eprintln!("Alias file not found: {}", file_key);
        }
        aliases::LookupFailure::FileUnavailable { file_key, reason } => {
            eprintln!("Alias file cannot be loaded: {}", file_key);
            eprintln!("  {}", reason);
            eprintln!("Run `byk --check` for details.");
        }
        aliases::LookupFailure::ReservedField {
            file_key,
            alias_key,
        } => {
            eprintln!(
                "Not an alias (reserved $ field): {}.{}",
                file_key, alias_key
            );
        }
        aliases::LookupFailure::KeyNotFound {
            file_key,
            alias_key,
        } => {
            eprintln!("Alias key not found in file {}: {}", file_key, alias_key);
        }
        aliases::LookupFailure::AliasNotFound { key, suggestions } => {
            let prefix = if key.starts_with('-') {
                "Unrecognized command, alias, or option"
            } else {
                "Unrecognized command or alias"
            };
            if suggestions.is_empty() {
                eprintln!("{}: {}", prefix, key);
            } else {
                eprintln!(
                    "{}: {}\nDid you mean:\n  {}",
                    prefix,
                    key,
                    suggestions.join("\n  ")
                );
            }
        }
    }
    exit(1);
}
