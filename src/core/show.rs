//! `byk show` 命令的业务逻辑层。
//!
//! 提供命令名查询、总览数据收集等核心逻辑，
//! render 层仅负责格式化输出。

use std::env;
use std::fs;
use std::process::Command;

use crate::core::aliases::{self, ResolvedAlias};
use crate::core::node;
use crate::core::paths::PathLayout;
use crate::core::plugins;

// ---------------------------------------------------------------------------
// 命令名查询
// ---------------------------------------------------------------------------

/// 查询结果条目。
#[derive(Debug)]
pub enum InfoEntry {
    Builtin { name: String },
    Plugin {
        name: String,
        cmd_type: String,
        entry: String,
        desc: String,
    },
    Npm {
        name: String,
        package: String,
        version: String,
    },
    Alias {
        display_source: String,
        resolved: ResolvedAlias,
    },
}

/// 按命令名全量路由查询，所有层都查找，不提前 return。
pub fn query_command(name: &str, layout: &PathLayout) -> Vec<InfoEntry> {
    let mut entries: Vec<InfoEntry> = Vec::new();

    // 1. 内置子命令
    if lookup_builtin(name).is_some() {
        entries.push(InfoEntry::Builtin {
            name: name.to_string(),
        });
    }

    // 2. 插件命令
    if layout.venv_dir.is_dir() {
        let plugin_state = plugins::state::load_plugin_state(&layout.plugins_dir, &layout.venv_dir);
        if let Some(cmd) = plugin_state.commands.get(name) {
            entries.push(InfoEntry::Plugin {
                name: name.to_string(),
                cmd_type: cmd.cmd_type.clone(),
                entry: cmd.entry.clone(),
                desc: cmd.desc.clone(),
            });
        }
    }

    // 3. NPM 命令
    let cache_file = layout.cache_dir.join("node-pkg.json");
    if let Some(npm_cache) = node::load_npm_cache(&cache_file, &layout.node_pkgs_dir)
        && let Some(pkg_name) = npm_cache.bin_map.get(name) {
            let version = npm_cache
                .packages
                .iter()
                .find(|p| &p.name == pkg_name)
                .map(|p| p.version.clone())
                .unwrap_or_else(|| "unknown".to_string());
            entries.push(InfoEntry::Npm {
                name: name.to_string(),
                package: pkg_name.clone(),
                version,
            });
        }

    // 4. 精确别名 (@file.key / @@file.key)
    if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(name) {
        let (_, files) = aliases::load_merged_aliases(layout);
        if let Some((resolved, display_source)) =
            aliases::lookup_exact_alias(&files, &file_key, &alias_key)
        {
            entries.push(InfoEntry::Alias {
                display_source,
                resolved,
            });
        }
    }

    // 5. 普通别名（所有文件中的同名别名）
    {
        let (_, files) = aliases::load_merged_aliases(layout);
        let all = aliases::lookup_all_aliases(&files, name);
        for resolved in all {
            let display_source = format!("{}.{}", resolved.source, name);
            entries.push(InfoEntry::Alias {
                display_source,
                resolved,
            });
        }
    }

    entries
}

/// 查找内置子命令描述。
pub fn lookup_builtin(name: &str) -> Option<&'static str> {
    match name {
        "add" => Some("Add plugins or features"),
        "remove" => Some("Remove plugins or features"),
        "show" => Some("Show system info, plugins, or command sources"),
        "completion" => Some("Generate shell completion script"),
        _ => None,
    }
}

/// 分类占位符类型。
pub fn classify_placeholder(ph: &str) -> &'static str {
    if ph.starts_with("${...") {
        "rest"
    } else if ph.starts_with("${") {
        "positional"
    } else if ph.contains("{{") {
        "optional"
    } else if ph.contains('?') {
        "conditional"
    } else {
        "named"
    }
}

// ---------------------------------------------------------------------------
// 总览面板数据
// ---------------------------------------------------------------------------

/// 补全状态。
#[derive(Debug)]
pub struct CompletionStatus {
    pub shell: Option<String>,
    pub configured: bool,
}

/// 总览面板数据。
#[derive(Debug)]
pub struct OverviewInfo {
    pub cache_initialized: bool,
    pub completion: CompletionStatus,
    pub node_initialized: bool,
    pub python: PythonOverviewInfo,
    pub plugin_count: usize,
    pub alias_count: usize,
}

/// Python 总览信息。
#[derive(Debug)]
pub struct PythonOverviewInfo {
    pub initialized: bool,
    pub version: Option<String>,
}

/// 收集总览面板数据。
pub fn collect_overview(layout: &PathLayout) -> OverviewInfo {
    let cache_initialized = {
        let cache_dir = &layout.cache_dir;
        if cache_dir.exists() {
            let entries = fs::read_dir(cache_dir).ok();
            entries
                .map(|e| e.filter_map(|en| en.ok()).count() > 0)
                .unwrap_or(false)
        } else {
            false
        }
    };

    let completion = check_completion_status();
    let node_initialized = check_node_initialized(layout);

    let python_exe = plugins::state::get_python_executable(&layout.plugins_dir, &layout.venv_dir);
    let state_file = layout.plugins_dir.join("plugins.cmd.json");
    let py_initialized = state_file.exists() || layout.venv_dir.exists();

    let version = if py_initialized {
        Command::new(&python_exe)
            .args(["--version"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|v| !v.is_empty())
    } else {
        None
    };

    let python = PythonOverviewInfo {
        initialized: py_initialized,
        version,
    };

    let plugin_count = if layout.venv_dir.is_dir() {
        let pkg_state = plugins::state::load_pkg_state(&layout.plugins_dir);
        pkg_state.len()
    } else {
        0
    };

    let (merged, _files) = aliases::load_merged_aliases(layout);
    let alias_count = merged.len();

    OverviewInfo {
        cache_initialized,
        completion,
        node_initialized,
        python,
        plugin_count,
        alias_count,
    }
}

// ---------------------------------------------------------------------------
// 补全 / Node 检测
// ---------------------------------------------------------------------------

/// 检查补全状态。
fn check_completion_status() -> CompletionStatus {
    match detect_shell() {
        Some(shell_name) => {
            let (_, configured) = check_completion(shell_name);
            CompletionStatus {
                shell: Some(shell_name.to_string()),
                configured,
            }
        }
        None => CompletionStatus {
            shell: None,
            configured: false,
        },
    }
}

/// 检查 Node 初始化状态。
fn check_node_initialized(layout: &PathLayout) -> bool {
    let node_dir = &layout.node_pkgs_dir;
    let node_alias = layout.alias_dir.join("node.byk.json");
    let node_cache = layout.cache_dir.join("node-pkg.json");
    node_dir.exists() && node_alias.exists() && node_cache.exists()
}

/// 从 $SHELL 检测当前 shell。
pub fn detect_shell() -> Option<&'static str> {
    let shell = env::var("SHELL").unwrap_or_default();
    if shell.ends_with("/zsh") {
        Some("zsh")
    } else if shell.ends_with("/bash") {
        Some("bash")
    } else {
        None
    }
}

/// 检查 shell 补全是否已配置。
pub fn check_completion(shell_name: &str) -> (&'static str, bool) {
    let rc_filename = if shell_name == "zsh" {
        ".zshrc"
    } else {
        ".bashrc"
    };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return (rc_filename, false),
    };

    let rc_path = home.join(rc_filename);
    let content = fs::read_to_string(&rc_path).unwrap_or_default();

    let line = format!(
        "if command -v byk >/dev/null 2>&1; then source <(byk completion {}); fi",
        shell_name
    );

    (rc_filename, content.contains(&line))
}