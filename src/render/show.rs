//! `byk show` CLI 信息格式化渲染。
//!
//! 三种模式：
//! - `byk show` — 帮助信息
//! - `byk show overview` — 总览面板
//! - `byk show plugins` — 已安装插件列表
//! - `byk show <name>` — 命令名全量路由查询
//!
//! 本模块只负责格式化输出，业务逻辑在 core::show 中。

use colored::Colorize;

use crate::core::aliases::AliasValue;
use crate::core::show::{self, InfoEntry, OverviewInfo};
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
        "  {:<20} Display system overview (paths, cache, completion, Node, Python)",
        "overview".cyan().bold(),
    );
    println!(
        "  {:<20} List installed plugins and their commands",
        "plugins".cyan().bold(),
    );
    println!(
        "  {:<20} Query the source of a command (built-in / plugin / NPM / alias)",
        "<command-name>".cyan().bold(),
    );
    println!(
        "  {:<20} Display CLI paths and file locations",
        "paths".cyan().bold(),
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
    render_overview_panel(&overview);
}

/// 渲染总览面板内容。
fn render_overview_panel(overview: &OverviewInfo) {
    println!(
        "{}：{}",
        "插件数量".yellow(),
        overview.plugin_count.to_string().cyan().bold()
    );
    println!(
        "{}：{}",
        "别名数量".yellow(),
        overview.alias_count.to_string().cyan().bold()
    );

    println!("{}", "-".repeat(29).dimmed());

    if overview.cache_initialized {
        println!("{}: {}", "Cache".yellow(), "enabled".green());
    } else {
        println!("{}: {}", "Cache".yellow(), "disabled".dimmed());
    }

    if let Some(shell_name) = &overview.completion.shell {
        if overview.completion.configured {
            println!(
                "{}: {} ({})",
                "Completion".yellow(),
                "enabled".green(),
                shell_name
            );
        } else {
            println!("{}: {}", "Completion".yellow(), "disabled".dimmed());
        }
    } else {
        println!("{}: {}", "Completion".yellow(), "disabled".dimmed());
    }

    if overview.python.initialized {
        let version_display = overview
            .python
            .version
            .as_deref()
            .unwrap_or("unknown");
        println!(
            "{}: {} ({})",
            "Python Venv".yellow(),
            "enabled".green(),
            version_display
        );
    } else {
        println!("{}: {}", "Python Venv".yellow(), "disabled".dimmed());
    }

    if overview.node_initialized {
        println!("{}: {}", "Node Modules".yellow(), "enabled".green());
    } else {
        println!("{}: {}", "Node Modules".yellow(), "disabled".dimmed());
    }

    println!();
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

    if pkg_state.is_empty() {
        println!("{}", "No plugins installed.".yellow());
        println!("  {}", "$ byk add <user/repo>".dimmed());
        println!();
        return;
    }

    let mut keys: Vec<&String> = pkg_state.keys().collect();
    keys.sort();

    let separator = "-".repeat(29);

    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            println!("{}", separator.dimmed());
        }

        let pkg = &pkg_state[*key];

        println!("{}: {}", "name".yellow(), key.cyan().bold());

        if !pkg.commands.is_empty() {
            println!("{}: {}", "cmd".yellow(), pkg.commands.join(", "));
        }

        if !pkg.scripts.is_empty() {
            println!("{}: {}", "scripts".yellow(), pkg.scripts.join(", "));
        }

        if let Some(ref pip_list) = pkg.pip {
            let names: Vec<&str> = pip_list
                .iter()
                .filter_map(|item| extract_display_name(item))
                .collect();
            if !names.is_empty() {
                println!("{}: {}", "pip".yellow(), names.join(", "));
            }
        }
    }
    println!();
}

/// 从 pip 安装字符串中提取可展示的包名。
/// - "name @ url" → "name"
/// - "name" → "name"
/// - "https://..." → None（纯 URL 不展示）
fn extract_display_name(raw: &str) -> Option<&str> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        None
    } else if let Some(pos) = raw.find(" @ ") {
        Some(raw[..pos].trim())
    } else {
        Some(raw.trim())
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== extract_display_name ====================

    #[test]
    fn display_name_simple() {
        assert_eq!(extract_display_name("requests"), Some("requests"));
    }

    #[test]
    fn display_name_with_at_url() {
        assert_eq!(
            extract_display_name("mypkg @ https://example.com/whl"),
            Some("mypkg")
        );
    }

    #[test]
    fn display_name_pure_url_none() {
        assert_eq!(extract_display_name("https://example.com/pkg"), None);
    }

    #[test]
    fn display_name_http_url_none() {
        assert_eq!(extract_display_name("http://example.com/pkg"), None);
    }

    #[test]
    fn display_name_trim_whitespace() {
        assert_eq!(extract_display_name("  requests  "), Some("requests"));
    }

    // ==================== render_paths ====================

    #[test]
    fn render_paths_does_not_panic() {
        let layout = crate::core::paths::PathLayout::with_name("fcbyk_test_show_paths");
        render_paths(&layout);
    }

    // ==================== render_help ====================

    #[test]
    fn render_help_does_not_panic() {
        render_help();
    }
}

// ---------------------------------------------------------------------------
// 路径显示（byk show paths）
// ---------------------------------------------------------------------------

/// 渲染 CLI 路径和文件位置。
pub fn render_paths(layout: &PathLayout) {
    println!();

    let python_exe = plugins::state::get_python_executable(&layout.plugins_dir, &layout.venv_dir);
    let cmd_state_file = layout.plugins_dir.join("plugins.cmd.json");

    println!("{}: {}", "Python".yellow(), python_exe);
    println!("{}: {}", "Commands".yellow(), cmd_state_file.display());
    println!("{}", "-".repeat(29).dimmed());
    println!("{}: {}", "CLI Home".yellow(), layout.root_dir.display());
    println!("{}: {}", "Cache".yellow(), layout.cache_dir.display());
    println!("{}: {}", "Alias".yellow(), layout.alias_dir.display());
    println!("{}: {}", "Plugins".yellow(), layout.plugins_dir.display());
    println!("{}: {}", "Python Venv".yellow(), layout.py_venv_dir.display());
    println!("{}: {}", "Node Modules".yellow(), layout.node_pkgs_dir.display());

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