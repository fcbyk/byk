/// `byk show` CLI 信息格式化渲染。
///
/// 三种模式：
/// - `byk show` — 帮助信息
/// - `byk show overview` — 总览面板
/// - `byk show plugins` — 已安装插件列表
/// - `byk show <name>` — 命令名全量路由查询
///
/// 本模块只负责格式化输出，业务逻辑在 core::show 中。

use colored::Colorize;

use crate::core::aliases::AliasValue;
use crate::core::show::{self, CompletionStatus, InfoEntry, OverviewInfo, PythonOverviewInfo};
use crate::core::aliases::ResolvedAlias;
use crate::core::paths::PathLayout;
use crate::core::plugins;
use crate::utils::display;

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 渲染 `byk show` 帮助信息。
pub fn render_help() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk show [SUBCOMMAND]".bold());
    println!();
    println!("{}", "Subcommands:".green().bold());
    println!(
        "  {:<20} {}",
        "overview".cyan().bold(),
        "Display system overview (paths, cache, completion, Node, Python)",
    );
    println!(
        "  {:<20} {}",
        "plugins".cyan().bold(),
        "List installed plugins and their commands",
    );
    println!(
        "  {:<20} {}",
        "<command-name>".cyan().bold(),
        "Query the source of a command (built-in / plugin / NPM / alias)",
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk show".into(), "Show this help".into()),
        ("byk show overview".into(), "Display system overview panel".into()),
        ("byk show plugins".into(), "List all installed plugins".into()),
        ("byk show build".into(), "Find all sources of the \"build\" command".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}

// ---------------------------------------------------------------------------
// 总览面板（byk show overview）
// ---------------------------------------------------------------------------

/// 渲染总览面板。
pub fn render_overview(layout: &PathLayout) {
    crate::render::banner::render();

    let overview = show::collect_overview(layout);
    render_overview_panel(layout, &overview);
}

/// 渲染总览面板内容。
fn render_overview_panel(layout: &PathLayout, overview: &OverviewInfo) {
    // 目录路径
    print_path("CLI Home", &layout.root_dir);
    print_path("Alias Directory", &layout.alias_dir);
    print_path("Logs Directory", &layout.logs_dir);

    // 缓存状态
    if overview.cache_initialized {
        println!("{}:  {}", "Cache".yellow(), "enabled".green());
    } else {
        println!("{}", "Cache not initialized.".yellow());
        println!("  {}", "$ byk add cache".dimmed());
    }

    println!("{}", "-".repeat(29).dimmed());

    // 补全状态
    render_completion(&overview.completion);

    // Node 状态
    println!("{}", "-".repeat(29).dimmed());
    if overview.node_initialized {
        print_path("Node Packages", &layout.node_pkgs_dir);
    } else {
        println!("{}", "Node package support not initialized.".yellow());
        println!("  {}   {}", "$ byk add npm".dimmed(), "(node-pkgs)".dimmed());
        println!("  {}   {}", "$ byk add pnpm".dimmed(), "(node-pkgs)".dimmed());
    }

    // Python 状态
    println!("{}", "-".repeat(29).dimmed());
    render_python_overview(&overview.python);
}

// ---------------------------------------------------------------------------
// 插件列表（byk show plugins）
// ---------------------------------------------------------------------------

/// 渲染已安装插件列表。
pub fn render_plugins(layout: &PathLayout) {
    println!();

    if !layout.venv_dir.is_dir() {
        println!("{}", "Python venv not initialized.".yellow());
        println!("  {}", "$ byk add <user/repo>".dimmed());
        println!();
        return;
    }

    let pkg_state = plugins::state::load_pkg_state(&layout.plugins_dir);

    if pkg_state.packages.is_empty() {
        println!("{}", "No plugins installed.".yellow());
        println!("  {}", "$ byk add <user/repo>".dimmed());
        println!();
        return;
    }

    let mut keys: Vec<&String> = pkg_state.packages.keys().collect();
    keys.sort();

    let entries: Vec<(String, String)> = keys
        .iter()
        .map(|key| {
            let pkg = &pkg_state.packages[*key];
            let mut parts: Vec<String> = Vec::new();
            parts.extend(pkg.commands.clone());
            if let Some(ref download) = pkg.download {
                parts.extend(download.scripts.clone());
            }
            let tuple = format!("({})", parts.join(", "));
            (key.to_string(), tuple)
        })
        .collect();

    let aligned = display::align_kv_pairs(&entries, "  ");

    println!("{}", "Installed plugins:".green().bold());
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.cyan().bold());
        println!("{}", rest);
    }
    println!();
}

// ---------------------------------------------------------------------------
// 命令名路由查询（byk show <name>）
// ---------------------------------------------------------------------------

/// 渲染命令名查询结果。
pub fn render_command(name: &str, layout: &PathLayout) {
    let entries = show::query_command(name, layout);

    if entries.is_empty() {
        eprintln!("No match found: {}", name);
        return;
    }

    println!();
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            println!("{}", "-".repeat(29).dimmed());
        }
        match entry {
            InfoEntry::Builtin { name } => {
                println!("{}: {}", "Built-in".green().bold(), name.cyan().bold());
                println!(
                    "  {}: {}",
                    "Description".yellow(),
                    show::lookup_builtin(name).unwrap_or_default()
                );
            }
            InfoEntry::Plugin {
                name,
                cmd_type,
                entry,
                desc,
            } => {
                println!("{}: {}", "Plugin".green().bold(), name.cyan().bold());
                let label = match cmd_type.as_str() {
                    "py-script" => "Script",
                    _ => "Module",
                };
                println!("  {}: {}", label.yellow(), entry);
                println!("  {}: {}", "Type".yellow(), cmd_type);
                println!("  {}: {}", "Desc".yellow(), desc);
            }
            InfoEntry::Npm {
                name,
                package,
                version,
            } => {
                println!("{}: {}", "NPM".green().bold(), name.cyan().bold());
                println!("  {}: {}", "Package".yellow(), package);
                println!("  {}: {}", "Version".yellow(), version);
                let bin_path = layout
                    .node_pkgs_dir
                    .join("node_modules")
                    .join(".bin")
                    .join(name);
                println!("  {}: {}", "Bin Path".yellow(), bin_path.display());
            }
            InfoEntry::Alias {
                display_source,
                resolved,
            } => {
                println!(
                    "{}: {}",
                    "Alias".green().bold(),
                    display_source.cyan().bold()
                );
                render_alias_detail(resolved);
            }
        }
    }
    println!();
}

/// 渲染别名详细信息。
fn render_alias_detail(resolved: &ResolvedAlias) {
    let (command, cwd, interactive, description) = match &resolved.value {
        AliasValue::Str(cmd) => (cmd.clone(), None, false, None),
        AliasValue::Meta {
            cmd,
            cwd,
            interactive,
            description,
        } => (
            cmd.clone(),
            cwd.clone(),
            interactive.unwrap_or(false),
            description.clone(),
        ),
    };

    // 来源文件
    if let Some(source_path) = &resolved.source_path {
        println!("  {}: {}", "Source".yellow(), source_path.display());
    }

    // 优先级
    println!("  {}: {}", "Priority".yellow(), resolved.priority);

    // 命令模板
    let escaped = display::escape_for_display(&command);
    println!("  {}: {}", "Template".yellow(), escaped);

    // 工作目录（解析为绝对路径显示）
    if let Some(cwd) = cwd {
        let base_dir = resolved.source_path.as_deref().and_then(|p| p.parent());
        let cwd_display = crate::render::aliases::resolve_cwd_display(&cwd, base_dir);
        println!("  {}: {}", "CWD".yellow(), cwd_display);
    }

    // 交互模式
    println!(
        "  {}: {}",
        "Interactive".yellow(),
        if interactive { "true" } else { "false" }
    );

    // 描述
    if let Some(desc) = description {
        println!("  {}: {}", "Description".yellow(), desc);
    }

    // 占位符
    let placeholders = crate::core::aliases::collect_placeholders(&command);
    if placeholders.is_empty() {
        println!("  {}: {}", "Placeholders".yellow(), "none".dimmed());
    } else {
        println!("  {}:", "Placeholders".yellow());
        for ph in &placeholders {
            let ph_type = show::classify_placeholder(ph);
            println!("    {}  {}", ph, ph_type.dimmed());
        }
    }

    // $paths
    if resolved.paths.is_empty() {
        println!("  {}: {}", "Paths".yellow(), "none".dimmed());
    } else {
        println!("  {}: {}", "Paths".yellow(), resolved.paths.join(", "));
    }
}

// ---------------------------------------------------------------------------
// 共享渲染组件
// ---------------------------------------------------------------------------

/// 渲染补全状态。
fn render_completion(completion: &CompletionStatus) {
    if let Some(shell_name) = &completion.shell {
        if completion.configured {
            println!(
                "{}: {} ({})",
                "Completion".yellow(),
                "enabled".green(),
                shell_name
            );
        } else {
            println!("{}: {}", "Completion".yellow(), "not configured".red());
            println!("  {}", "$ byk add comp".dimmed());
        }
    }
}

/// 渲染 Python 总览信息。
fn render_python_overview(python: &PythonOverviewInfo) {
    print_path("Python venv", &python.venv_dir);

    if !python.initialized {
        println!("{}", "Python plugin system not initialized.".yellow());
        println!(
            "  {}",
            "$ byk add <user/repo>".dimmed(),
        );
        println!();
        return;
    }

    // Python 解释器路径
    println!("{}: {}", "Python".yellow(), python.python_exe);

    // Python 版本
    if let Some(version) = &python.version {
        println!("{}: {}", "Version".yellow(), version);
    }

    // 状态文件路径
    println!("{}: {}", "State".yellow(), python.state_file.display());

    // 来源提示
    let source_display = "State file (plugins.cmd.json)".dimmed();
    println!("{}:  {}", "Source".yellow(), source_display);
    println!();
}

/// 打印路径，仅路径存在时显示。
fn print_path(label: &str, path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    println!("{}: {}", label.yellow(), path.display());
}