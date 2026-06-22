/// --info 选项的业务逻辑层。
///
/// 提供命令名查询、诊断检查、配置校验、冲突检测等核心逻辑，
/// render 层仅负责格式化输出。

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::core::aliases::{self, AliasFile, ResolvedAlias};
use crate::core::npm_commands;
use crate::core::paths::PathLayout;
use crate::core::plugins;

// ---------------------------------------------------------------------------
// 保留词
// ---------------------------------------------------------------------------

pub const TOPIC_DOCTOR: &str = "doctor";
pub const TOPIC_PLUGINS: &str = "plugins";

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
        let plugin_state = plugins::load_plugin_state(&layout.plugins_dir, &layout.venv_dir);
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
    if let Some(npm_cache) = npm_commands::load_npm_cache(&cache_file, &layout.node_pkgs_dir) {
        if let Some(pkg_name) = npm_cache.bin_map.get(name) {
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
        "remove" => Some("Remove plugins or features"),
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
    } else if ph.starts_with("{{") {
        "optional"
    } else if ph.contains('?') {
        "conditional"
    } else {
        "named"
    }
}

// ---------------------------------------------------------------------------
// 诊断检查
// ---------------------------------------------------------------------------

/// 诊断报告。
#[derive(Debug)]
pub struct DoctorReport {
    /// 缓存状态: "healthy" | "stale" | "missing"
    pub cache_status: String,
    /// 别名文件数量
    pub alias_file_count: usize,
    /// 别名总数
    pub alias_count: usize,
    /// 配置警告列表
    pub config_warnings: Vec<String>,
    /// 补全状态
    pub completion: CompletionStatus,
    /// Node 初始化状态
    pub node_initialized: bool,
    /// Python 状态
    pub python: PythonStatus,
    /// 别名冲突列表
    pub conflicts: Vec<String>,
}

/// 补全状态。
#[derive(Debug)]
pub struct CompletionStatus {
    /// 检测到的 shell 名称
    pub shell: Option<String>,
    /// 是否已配置
    pub configured: bool,
}

/// Python 状态。
#[derive(Debug)]
pub struct PythonStatus {
    /// 是否已初始化
    pub initialized: bool,
    /// bykpy 是否已安装
    pub bykpy_installed: bool,
}

/// 运行完整诊断。
pub fn run_diagnostics(layout: &PathLayout) -> DoctorReport {
    // 缓存健康
    let cache_status = check_cache_status(&layout.cache_dir);

    // 别名文件统计
    let (merged, files) = aliases::load_merged_aliases(layout);
    let alias_count = aliases::collect_merged_paths(&merged, "").len();

    // 配置校验
    let config_warnings = collect_config_warnings(&files);

    // 补全状态
    let completion = check_completion_status();

    // Node 状态
    let node_initialized = check_node_initialized(layout);

    // Python 状态
    let python = check_python_status(layout);

    // 冲突检测
    let conflicts = detect_alias_conflicts(&files);

    DoctorReport {
        cache_status,
        alias_file_count: files.len(),
        alias_count,
        config_warnings,
        completion,
        node_initialized,
        python,
        conflicts,
    }
}

/// 检查缓存状态。
fn check_cache_status(cache_dir: &std::path::Path) -> String {
    if cache_dir.exists() {
        let alias_cache = cache_dir.join("alias.json");
        let npm_cache = cache_dir.join("node-pkg.json");
        let has_any = alias_cache.exists() || npm_cache.exists();
        if has_any {
            "healthy".to_string()
        } else {
            "stale".to_string()
        }
    } else {
        "missing".to_string()
    }
}

/// 检查补全状态。
fn check_completion_status() -> CompletionStatus {
    match detect_shell() {
        Some(shell_name) => {
            let (_, configured) = check_completion(&shell_name);
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

/// 检查 Python 状态。
fn check_python_status(layout: &PathLayout) -> PythonStatus {
    let py_state = layout.plugins_dir.join("plugins.cmd.json");
    let py_venv = layout.venv_dir.exists();

    if !py_state.exists() && !py_venv {
        return PythonStatus {
            initialized: false,
            bykpy_installed: false,
        };
    }

    let py_exe = plugins::get_python_executable(&layout.plugins_dir, &layout.venv_dir);
    let bykpy_installed = Command::new(&py_exe)
        .args(["-c", "import bykpy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    PythonStatus {
        initialized: true,
        bykpy_installed,
    }
}

// ---------------------------------------------------------------------------
// 总览面板数据
// ---------------------------------------------------------------------------

/// 总览面板数据。
#[derive(Debug)]
pub struct OverviewInfo {
    /// 缓存是否已初始化
    pub cache_initialized: bool,
    /// 补全状态
    pub completion: CompletionStatus,
    /// Node 是否已初始化
    pub node_initialized: bool,
    /// Python 信息
    pub python: PythonOverviewInfo,
}

/// Python 总览信息。
#[derive(Debug)]
pub struct PythonOverviewInfo {
    /// 是否已初始化
    pub initialized: bool,
    /// Python 解释器路径
    pub python_exe: String,
    /// Python 版本
    pub version: Option<String>,
    /// 状态文件路径
    pub state_file: PathBuf,
    /// venv 目录路径
    pub venv_dir: PathBuf,
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

    let python_exe = plugins::get_python_executable(&layout.plugins_dir, &layout.venv_dir);
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
        python_exe,
        version,
        state_file,
        venv_dir: layout.venv_dir.clone(),
    };

    OverviewInfo {
        cache_initialized,
        completion,
        node_initialized,
        python,
    }
}

// ---------------------------------------------------------------------------
// 配置校验
// ---------------------------------------------------------------------------

/// 收集别名配置中的警告。
pub fn collect_config_warnings(files: &[AliasFile]) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();
    for f in files {
        collect_warnings_from_map(&f.aliases, &f.key, &mut warnings);
    }
    warnings
}

/// 递归收集 JSON Map 中的配置警告。
fn collect_warnings_from_map(
    data: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    warnings: &mut Vec<String>,
) {
    for (key, val) in data {
        let full_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", path, key)
        };

        // key 含 @ 或 .
        if key.contains('@') || key.contains('.') {
            warnings.push(format!(
                "{}: key \"{}\" contains invalid chars, skipped",
                path, key
            ));
            continue;
        }

        // 值类型检查
        match val {
            serde_json::Value::Number(_)
            | serde_json::Value::Array(_)
            | serde_json::Value::Null => {
                warnings.push(format!(
                    "{}: value for \"{}\" is {}, expected string/object",
                    path,
                    key,
                    match val {
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Null => "null",
                        _ => "unknown",
                    }
                ));
            }
            serde_json::Value::Object(inner) => {
                // 含 $ 前缀 key → 别名元数据，不递归
                if !inner.keys().any(|k| k.starts_with('$')) {
                    collect_warnings_from_map(inner, &full_path, warnings);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 冲突检测
// ---------------------------------------------------------------------------

/// 检测同名别名冲突（被覆盖的情况）。
pub fn detect_alias_conflicts(files: &[AliasFile]) -> Vec<String> {
    let mut conflicts: Vec<String> = Vec::new();
    let mut seen: HashMap<String, Vec<(String, i32)>> = HashMap::new();

    // 收集每个别名路径出现的文件和优先级
    for f in files {
        if f.priority < 0 {
            continue;
        }
        collect_alias_paths(&f.aliases, &f.key, f.priority, &mut seen);
    }

    // 找出出现多次的别名
    for (alias_path, sources) in &seen {
        if sources.len() > 1 {
            let mut sorted = sources.clone();
            sorted.sort_by_key(|s| s.1);
            let winner = sorted.last().unwrap();
            let shadowed: Vec<&(String, i32)> =
                sorted.iter().filter(|s| s.1 < winner.1).collect();
            for s in shadowed {
                conflicts.push(format!(
                    "\"{}\": {} (priority {}) shadows {} (priority {})",
                    alias_path, winner.0, winner.1, s.0, s.1
                ));
            }
        }
    }

    conflicts
}

/// 递归收集别名路径及其来源。
fn collect_alias_paths(
    data: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    priority: i32,
    seen: &mut HashMap<String, Vec<(String, i32)>>,
) {
    for (key, val) in data {
        if key.starts_with('$') {
            continue;
        }
        let full_path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        match val {
            serde_json::Value::String(_) => {
                seen.entry(full_path.clone())
                    .or_default()
                    .push((prefix.to_string(), priority));
            }
            serde_json::Value::Object(inner) => {
                if inner.keys().any(|k| k.starts_with('$')) {
                    seen.entry(full_path.clone())
                        .or_default()
                        .push((prefix.to_string(), priority));
                }
                collect_alias_paths(inner, &full_path, priority, seen);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 环境检测
// ---------------------------------------------------------------------------

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