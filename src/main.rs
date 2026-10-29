mod cli;
mod core;
mod render;
mod utils;

use clap::{CommandFactory, Parser};
use colored::Colorize;
use std::process::exit;

use cli::{Cli, Commands, extract_options};
use core::aliases;
use core::add;
use core::paths::PathLayout;
use core::remove;

fn main() {
    let cmd = Cli::command();
    let options = extract_options(&cmd);
    let layout = PathLayout::new();

    let cli = Cli::parse();

    // Step 0: 内置子命令（优先级最高）
    match cli.command {
        Some(Commands::Api { request }) => {
            core::api::dispatch(&request, &layout);
            return;
        }
        Some(Commands::Remove { feature }) => {
            match feature.as_deref() {
                Some("comp") => remove::rm_comp(),
                Some("py") => {
                    eprintln!("{} 'byk remove py' has been removed.", "Note:".yellow());
                    eprintln!("  The Python environment is now created automatically when a plugin needs one.");
                    eprintln!(
                        "  To remove everything including it: {}",
                        "byk remove all".bold()
                    );
                }
                Some("all") => remove::rm_all(&layout),
                Some("-h") | Some("--help") => render::remove::render(),
                Some(n) if n.starts_with("@@") => remove::uninstall_from_global(n, &layout),
                Some(n) if n.starts_with("@") => remove::uninstall_from_local(n, &layout),
                Some(key) => {
                    eprintln!(
                        "{} unknown argument: {}",
                        "Error:".red(),
                        key.bold()
                    );
                    eprintln!("  Expected: comp, all, @<file>, or @@<file>");
                    eprintln!("  Run {} for usage.", "byk remove -h".bold());
                }
                None => render::remove::render(),
            }
            return;
        }
        Some(Commands::Add { name }) => {
            match name.as_deref() {
                None | Some("-h") | Some("--help") => render::add::render(),
                Some("comp") => add::init_completion(),
                Some(n) if n.starts_with("@@") => add::install_from_global(n, &layout),
                Some(n) if n.starts_with("@") => add::install_from_local(n, &layout),
                Some(n) => {
                    eprintln!("{} unknown argument: {}", "Error:".red(), n);
                    eprintln!("  Expected: comp, @<file>, or @@<file>");
                    eprintln!("  Run {} for usage.", "byk add -h".bold());
                    exit(1);
                }
            }
            return;
        }
        Some(Commands::Show { topic }) => {
            match topic.as_deref() {
                None | Some("-h") | Some("--help") => render::show::render_help(),
                Some("overview") => render::show::render_overview(&layout),
                Some("plugins") => {
                    eprintln!("{} 'byk show plugins' has been removed.", "Note:".yellow());
                    eprintln!(
                        "  Plugins are now *.byk.json files; run {} to list commands.",
                        "byk -h".bold()
                    );
                }
                Some("paths") => render::show::render_paths(&layout),
                Some(name) => render::show::render_command(name, &layout),
            }
            return;
        }
        None => {}
    }

    // Step 1: 全局选项（优先级最高，直接返回）
    if cli.help {
        println!();
        render::help::render_all(&layout, &options);
        return;
    }
    if cli.version {
        println!(
            "byk {} ({} {}) [{}]",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_HASH"),
            env!("BUILD_DATE"),
            env!("PLATFORM"),
        );
        if let Ok(exe_path) = std::env::current_exe() { println!(
            "{} {}",
            "installed at".dimmed(),
            exe_path.display().to_string().dimmed(),
        ) }
        return;
    }
    if cli.trailing.is_empty() {
        println!();
        render::help::render_all(&layout, &options);
        return;
    }

    let command_name = &cli.trailing[0];
    let command_args = &cli.trailing[1..];

    // 提前加载别名数据（精确执行和普通查找共用）
    let (merged, files) = aliases::load_merged_aliases(&layout);

    // Step 4a: 检查精确执行语法（@file.key 或 @@file.key）
    if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(command_name) {
        match aliases::lookup_exact_alias(&files, &file_key, &alias_key, &layout) {
            Some((resolved, display_source)) => {
                aliases::execute_alias(&resolved, command_args, &display_source, &layout);
                return;
            }
            None => {
                let file_exists = files.iter().any(|f| f.key == file_key);
                if file_exists {
                    eprintln!(
                        "Alias key not found in file {}: {}",
                        file_key, alias_key
                    );
                } else {
                    eprintln!("Alias file not found: {}", file_key);
                }
                exit(1);
            }
        }
    }

    // Step 4b: 检查普通别名（在合并 key 空间中查找）
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