/// CLI 信息格式化渲染（--info 选项）。
///
/// 三种模式：
/// - `byk --info` — 总览面板（向后兼容）
/// - `byk --info doctor` — 诊断检查
/// - `byk --info <name>` — 命令名全量路由查询
///
/// 本模块只负责格式化输出，业务逻辑在 core::info 中。

use colored::Colorize;

use crate::core::aliases::{AliasValue, ResolvedAlias};
use crate::core::info::{
    self, CompletionStatus, DoctorReport, InfoEntry, OverviewInfo, PythonOverviewInfo,
    PythonStatus,
};
use crate::core::paths::PathLayout;
use crate::core::plugins;
use crate::utils::display;

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 根据 topic 分发到对应渲染函数。
pub fn render_topic(topic: &str, layout: &PathLayout) {
    match topic {
        info::TOPIC_DOCTOR => {
            render_doctor(layout);
        }
        info::TOPIC_PLUGINS => {
            render_plugins_list(layout);
        }
        _ => {
            // 非保留词 → 命令名查询路由
            render_command_info(topic, layout);
        }
    }
}

// ---------------------------------------------------------------------------
// 总览面板（byk --info 无参数）
// ---------------------------------------------------------------------------

/// 渲染 CLI 完整信息：banner + 基础信息 + Node + Python。
pub fn render_all(layout: &PathLayout) {
    crate::render::banner::render();

    let overview = info::collect_overview(layout);
    render_overview(layout, &overview);
}

/// 渲染总览面板。
fn render_overview(layout: &PathLayout, overview: &OverviewInfo) {
    // 目录路径
    print_path("CLI Home", &layout.root_dir);
    print_path("Alias Directory", &layout.alias_dir);
    print_path("Logs Directory", &layout.logs_dir);

    // 缓存状态
    if overview.cache_initialized {
        println!("{}:  {}", "Cache".yellow(), "enabled".green());
    } else {
        println!("{}", "Cache not initialized.".yellow());
        println!("  {}", "$ byk init cache".dimmed());
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
        println!("  {}   {}", "$ byk init npm".dimmed(), "(node-pkgs)".dimmed());
        println!("  {}   {}", "$ byk init pnpm".dimmed(), "(node-pkgs)".dimmed());
    }

    // Python 状态
    println!("{}", "-".repeat(29).dimmed());
    render_python_overview(&overview.python);
}

// ---------------------------------------------------------------------------
// 命令名查询路由（byk --info <name>）
// ---------------------------------------------------------------------------

/// 渲染命令名查询结果。
fn render_command_info(name: &str, layout: &PathLayout) {
    let entries = info::query_command(name, layout);

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
                    info::lookup_builtin(name).unwrap_or_default()
                );
            }
            InfoEntry::Plugin {
                name,
                module,
                description,
            } => {
                println!("{}: {}", "Plugin".green().bold(), name.cyan().bold());
                println!("  {}: {}", "Module".yellow(), module);
                println!("  {}: {}", "Description".yellow(), description);
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
            let ph_type = info::classify_placeholder(ph);
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
// doctor 主题（byk --info doctor）
// ---------------------------------------------------------------------------

/// 渲染诊断信息。
fn render_doctor(layout: &PathLayout) {
    crate::render::banner::render();

    let report = info::run_diagnostics(layout);
    render_doctor_report(&report);
}

/// 渲染诊断报告。
fn render_doctor_report(report: &DoctorReport) {
    // 缓存健康
    print_status("Cache", &report.cache_status);

    // 别名文件统计
    println!(
        "{}: {} {}, {} {}",
        "Alias files".yellow(),
        report.alias_file_count.to_string().green(),
        "files".dimmed(),
        report.alias_count.to_string().green(),
        "aliases".dimmed(),
    );

    // 配置校验
    if report.config_warnings.is_empty() {
        println!("{}: {}", "Config errors".yellow(), "0".green());
    } else {
        println!(
            "{}: {}",
            "Config errors".yellow(),
            report.config_warnings.len().to_string().red()
        );
        for w in &report.config_warnings {
            println!("  {} {}", "-".red(), w);
        }
    }

    println!("{}", "-".repeat(29).dimmed());

    // 补全状态
    render_completion(&report.completion);

    // Node 状态
    if report.node_initialized {
        println!("{}: {}", "Node".yellow(), "initialized".green());
    } else {
        println!("{}: {}", "Node".yellow(), "not initialized".dimmed());
    }

    // Python 状态
    render_python_status(&report.python);

    println!("{}", "-".repeat(29).dimmed());

    // 冲突检测
    if report.conflicts.is_empty() {
        println!("{}: {}", "Conflicts".yellow(), "none".green());
    } else {
        println!(
            "{}: {} {}",
            "Conflicts".yellow(),
            report.conflicts.len().to_string().red(),
            "aliases shadowed".dimmed()
        );
        for c in &report.conflicts {
            println!("  {} {}", "-".red(), c);
        }
    }

    println!();
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
            println!("  {}", "$ byk init comp".dimmed());
        }
    }
}

/// 渲染 Python 诊断状态。
fn render_python_status(python: &PythonStatus) {
    if !python.initialized {
        println!("{}: {}", "Python".yellow(), "not initialized".dimmed());
        return;
    }

    println!("{}: {}", "Python".yellow(), "initialized".green());
    let status_str = if python.bykpy_installed {
        "installed".green()
    } else {
        "missing".red()
    };
    println!("  {}: {}", "bykpy".yellow(), status_str);
}

/// 渲染已安装插件列表（--info plugins）。
fn render_plugins_list(layout: &PathLayout) {
    crate::render::banner::render();

    if !layout.venv_dir.is_dir() {
        println!("{}", "Python venv not initialized.".yellow());
        println!("  {}", "$ byk add <name>".dimmed());
        println!();
        return;
    }

    let plugin_state = plugins::load_plugin_state(&layout.plugins_dir, &layout.venv_dir);

    if plugin_state.packages.is_empty() {
        println!("{}", "No plugins installed.".yellow());
        println!("  {}", "$ byk add <name>".dimmed());
        println!();
        return;
    }

    let mut keys: Vec<&String> = plugin_state.packages.keys().collect();
    keys.sort();

    println!("{}", "Installed plugins:".green().bold());
    for key in &keys {
        let pkg = &plugin_state.packages[*key];
        let cmds = pkg.commands.join(", ");
        let source_str = pkg
            .source
            .as_ref()
            .map(|s| format!("    source: {}", s.dimmed()))
            .unwrap_or_default();
        println!(
            "  {}    pip: {}{}    commands: {}",
            key.cyan().bold(),
            pkg.name.dimmed(),
            source_str,
            cmds,
        );
        // 显示每个命令的模块路径
        for cmd_name in &pkg.commands {
            if let Some(cmd) = plugin_state.commands.get(cmd_name) {
                println!(
                    "    {} → {} {}",
                    cmd_name.dimmed(),
                    cmd.module.dimmed(),
                    format!("({})", cmd.description).dimmed(),
                );
            }
        }
    }
    println!();
}

/// 渲染 Python 总览信息。
fn render_python_overview(python: &PythonOverviewInfo) {
    print_path("Python venv", &python.venv_dir);

    if !python.initialized {
        println!("{}", "Python plugin system not initialized.".yellow());
        println!(
            "  {}",
            "$ byk add <name>".dimmed(),
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
    let source_display = "State file (pip.json)".dimmed();
    println!("{}:  {}", "Source".yellow(), source_display);
    println!();
}

/// 打印状态标签。
fn print_status(label: &str, status: &str) {
    let status_display = match status {
        "healthy" | "installed" | "enabled" | "initialized" => status.green(),
        "stale" | "missing" | "not configured" | "not initialized" => status.red(),
        _ => status.normal(),
    };
    println!("{}: {}", label.yellow(), status_display);
}

/// 打印路径，仅路径存在时显示。
fn print_path(label: &str, path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    println!("{}: {}", label.yellow(), path.display());
}
