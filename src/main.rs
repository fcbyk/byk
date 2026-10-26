mod cli;
mod core;
mod render;
mod utils;

use clap::{CommandFactory, Parser};
use colored::Colorize;
use std::process::exit;

use cli::{Cli, Commands, extract_options};
use core::aliases;
use core::completion;
use core::add;
use core::paths::PathLayout;
use core::plugins;
use core::remove;

fn main() {
    let cmd = Cli::command();
    let options = extract_options(&cmd);
    let layout = PathLayout::new();

    let cli = Cli::parse();

    // Step 0: 内置子命令（优先级最高）
    match cli.command {
        Some(Commands::Completion { shell }) => {
            completion::generate(&shell);
            return;
        }
        Some(Commands::Complete { words }) => {
            completion::complete(&words, &layout);
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
                Some(key) => {
                    eprintln!(
                        "{} 'byk remove <plugin>' has been removed.",
                        "Note:".yellow()
                    );
                    eprintln!(
                        "  Plugins are now *.byk.json files; delete {} to uninstall.",
                        format!("~/.byk/alias/{}.byk.json", key).bold()
                    );
                }
                None => render::remove::render(),
            }
            return;
        }
        Some(Commands::Add { file, name, cdn, target }) => {
            match (name.as_deref(), file.as_deref(), target.as_deref()) {
                (None | Some("-h") | Some("--help"), None, None) => {
                    render::add::render();
                }
                (Some("comp"), None, None) => add::init_completion(),
                (Some("py-v") | Some("uv"), None, None) => {
                    eprintln!(
                        "{} 'byk add py-v' / 'byk add uv' have been removed.",
                        "Note:".yellow()
                    );
                    eprintln!("  Python environments are now created automatically when a plugin needs one.");
                    eprintln!("  Just run: {}", "byk add <user/repo>".bold());
                }
                (Some("pip"), None, target) => {
                    add::install_plugin_pip(target, &layout);
                }
                (Some("assets"), None, target) => {
                    add::install_plugin_assets(target, &layout, cdn);
                }
                (Some("pip" | "assets"), Some(_), _) => {
                    eprintln!(
                        "{} -f/--file is not used with 'byk add {}'; pass the .byk.json path directly.",
                        "Error:".red(),
                        name.as_deref().unwrap_or("")
                    );
                    exit(1);
                }
                (_, _, Some(t)) => {
                    eprintln!("{} unexpected argument '{}'", "Error:".red(), t);
                    eprintln!("  Run {} for usage.", "byk add -h".bold());
                    exit(1);
                }
                (None, Some(f), None) => {
                    // -f：本地 *.byk.json 文件路径
                    plugins::install_from_path(f, &layout, cdn);
                }
                (Some(_), Some(_), None) => {
                    eprintln!(
                        "{} -f/--file takes a single .byk.json path; unexpected argument '{}'",
                        "Error:".red(),
                        name.as_deref().unwrap_or("")
                    );
                    exit(1);
                }
                (Some(n), None, None) if n.contains('/') => {
                    // 远程：user/repo/path → 拉取 *.byk.json
                    plugins::install_remote(n, &layout, cdn);
                }
                (Some(n), None, None) => {
                    // 本地：当前目录 <n>.byk.json
                    plugins::install(Some(n), &layout, cdn);
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