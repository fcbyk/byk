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
use core::node;
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
                Some("node") => remove::rm_node(&layout),
                Some("py") => remove::rm_py_v(&layout),
                Some("all") => remove::rm_all(&layout),
                Some("-h") | Some("--help") => render::remove::render(),
                Some(key) => remove::uninstall_plugin(key, &layout),
                None => render::remove::render(),
            }
            return;
        }
        Some(Commands::Add { branch, file, editable, name }) => {
            match (name.as_deref(), editable.as_deref(), file.as_deref()) {
                (None | Some("-h") | Some("--help"), None, None) => {
                    render::add::render();
                }
                (Some("npm"), None, None) => add::init_npm(&layout),
                (Some("pnpm"), None, None) => add::init_pnpm(&layout),
                (Some("cache"), None, None) => add::init_cache(&layout),
                (Some("comp"), None, None) => add::init_completion(),
                (Some("py-v"), None, None) => add::init_py_v(&layout),
                (Some("uv"), None, None) => add::init_uv(&layout),
                (spec, editable, _file) => {
                    add::install_plugin(
                        spec.unwrap_or(""),
                        branch.as_deref(),
                        file.as_deref(),
                        editable,
                        &layout,
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
            "byk {} ({} {})",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_HASH"),
            env!("BUILD_DATE"),
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

    // Step 2: 检查是否为插件命令（优先级高于 NPM）
    // 仅 venv 存在时加载插件状态
    let cmd_state = if layout.venv_dir.is_dir() {
        plugins::state::load_plugin_state(&layout.plugins_dir, &layout.venv_dir)
    } else {
        plugins::state::empty_cmd_state()
    };
    if cmd_state.commands.contains_key(command_name) {
        plugins::execute::execute_plugin_command(
            command_name,
            command_args,
            &layout.plugins_dir,
            &layout.venv_dir,
            &cmd_state,
        );
        return;
    }

    // Step 3: 检查是否为 NPM Command
    let cache_file = layout.cache_dir.join("node-pkg.json");
    if let Some(cache) = node::load_npm_cache(&cache_file, &layout.node_pkgs_dir)
        && cache.bin_map.contains_key(command_name) {
            node::execute_npm_command(command_name, command_args, &layout);
            return;
        }

    // 提前加载别名数据（精确执行和普通查找共用）
    let (merged, files) = aliases::load_merged_aliases(&layout);

    // Step 4a: 检查精确执行语法（@file.key 或 @@file.key）
    if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(command_name) {
        match aliases::lookup_exact_alias(&files, &file_key, &alias_key) {
            Some((resolved, display_source)) => {
                aliases::execute_alias(&resolved, command_args, &display_source);
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
            aliases::execute_alias(resolved, command_args, &display_source);
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