mod cli;
mod core;
mod render;
mod utils;

use clap::{CommandFactory, Parser};
use std::process::exit;

use cli::{Cli, Commands, extract_options};
use core::aliases;
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

    // 别名数据只加载一次，精确执行与普通查找共用
    let (merged, files) = aliases::load_merged_aliases(&layout);

    // 精确执行：@file.key / @@file.key
    if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(command_name) {
        match aliases::lookup_exact_alias(&files, &file_key, &alias_key) {
            Some((resolved, display_source)) => {
                aliases::execute_alias(&resolved, command_args, &display_source, &layout);
                return;
            }
            None => {
                let file_exists = files.iter().any(|f| f.key == file_key);
                if file_exists {
                    eprintln!("Alias key not found in file {}: {}", file_key, alias_key);
                } else {
                    eprintln!("Alias file not found: {}", file_key);
                }
                exit(1);
            }
        }
    }

    // 普通别名：在合并后的 key 空间里找
    match aliases::resolve_merged_alias(&merged, command_name) {
        Some(resolved) => {
            let display_source = format!("{}.{}", resolved.source, command_name);
            aliases::execute_alias(resolved, command_args, &display_source, &layout);
        }
        None => {
            // 生成建议列表
            let paths = aliases::collect_merged_paths(&merged, "");
            let suggestions: Vec<String> = paths
                .into_iter()
                .filter(|item| {
                    item.starts_with(&format!("{}.", command_name))
                        || command_name.starts_with(&format!("{}.", item))
                })
                .take(5)
                .collect();

            let prefix = if command_name.starts_with('-') {
                "Unrecognized command, alias, or option"
            } else {
                "Unrecognized command or alias"
            };

            if suggestions.is_empty() {
                eprintln!("{}: {}", prefix, command_name);
            } else {
                eprintln!(
                    "{}: {}\nDid you mean:\n  {}",
                    prefix,
                    command_name,
                    suggestions.join("\n  ")
                );
            }
            exit(1);
        }
    }
}
