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
                Some(key) => remove::uninstall_plugin(key, &layout),
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
                    add::install_alias_pip(target, &layout);
                }
                (Some("pip"), Some(_), _) => {
                    eprintln!(
                        "{} -f/--file is not used with 'byk add pip'; pass the .byk.json path directly.",
                        "Error:".red()
                    );
                    exit(1);
                }
                (_, _, Some(t)) => {
                    eprintln!("{} unexpected argument '{}'", "Error:".red(), t);
                    eprintln!("  Run {} for usage.", "byk add -h".bold());
                    exit(1);
                }
                (spec, file, None) => {
                    add::install_plugin(
                        spec.unwrap_or(""),
                        file,
                        &layout,
                        cdn,
                    );
                }
            }
            return;
        }
        Some(Commands::Show { topic }) => {
            match topic.as_deref() {
                None | Some("-h") | Some("--help") => render::show::render_help(),
                Some("overview") => render::show::render_overview(&layout),
                Some("plugins") => render::show::render_plugins(&layout),
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

    // Step 2: 检查是否为插件命令（优先级高于别名）
    // 插件状态不依赖 venv：bin 类型命令与 Python 环境无关，
    // venv 被删除后仍可用；python 类命令在执行时按需检查（见 execute.rs）
    let cmd_state = plugins::state::load_plugin_state(&layout.plugins_dir, &layout.venv_dir);
    if cmd_state.commands.contains_key(command_name) {
        plugins::execute::execute_plugin_command(
            command_name,
            command_args,
            &layout.plugins_dir,
            &layout.venv_dir,
            &layout.root_dir,
            &cmd_state,
        );
        return;
    }

    // 提前加载别名数据（精确执行和普通查找共用）
    let (merged, files) = aliases::load_merged_aliases(&layout);

    // Step 4a: 检查精确执行语法（@file.key 或 @@file.key）
    if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(command_name) {
        match aliases::lookup_exact_alias(&files, &file_key, &alias_key) {
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