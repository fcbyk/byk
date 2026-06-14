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
use core::init;
use core::npm_commands;
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
                Some("py") => remove::rm_py(&layout),
                Some("py-v") => remove::rm_py_v(&layout),
                Some("npm") => remove::rm_npm(&layout),
                Some("pnpm") => remove::rm_pnpm(&layout),
                _ => remove::render_remove_help(),
            }
            return;
        }
        Some(Commands::Init { feature }) => {
            match feature.as_deref() {
                Some("npm") => init::init_npm(&layout),
                Some("pnpm") => init::init_pnpm(&layout),
                Some("py") => init::init_py_global(&layout),
                Some("py-v") => init::init_py(&layout),
                Some("comp") => init::init_completion(),
                _ => init::render_init_help(),
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
        match std::env::current_exe() {
            Ok(exe_path) => println!(
                "{} {}",
                "installed at".dimmed(),
                exe_path.display().to_string().dimmed(),
            ),
            Err(_) => {}
        }
        return;
    }
    if cli.info {
        render::info::render_all(&layout);
        return;
    }

    // 无额外参数 → 帮助（上下各空一行）
    if cli.trailing.is_empty() {
        println!();
        render::help::render_all(&layout, &options);
        return;
    }

    let command_name = &cli.trailing[0];
    let command_args = &cli.trailing[1..];

    // Step 2: 检查是否为插件命令（优先级高于 NPM）
    // 仅 ~/.byk 存在时加载插件缓存，避免触发 bykpy spawn 创建目录
    let plugin_cache = if layout.home_exists {
        plugins::load_plugin_cache(&layout.cache_dir)
    } else {
        plugins::empty_plugin_cache()
    };
    if plugin_cache.commands.contains_key(command_name) {
        plugins::execute_plugin_command(command_name, command_args, &layout.cache_dir);
        return;
    }

    // Step 3: 检查是否为 NPM Command
    let cache_file = layout.cache_dir.join("node-pkg.json");
    if let Some(cache) = npm_commands::load_npm_cache(&cache_file, &layout.node_pkgs_dir) {
        if cache.bin_map.contains_key(command_name) {
            npm_commands::execute_npm_command(command_name, command_args, &layout);
            return;
        }
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
            aliases::execute_alias(&resolved, command_args, &display_source);
            return;
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
