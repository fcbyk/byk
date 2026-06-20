/// `byk add` / `byk remove <key>` 命令实现。
///
/// 支持中心仓库 fcbyk/byk-plugins 和社区仓库 user/repo。
/// pip install / uninstall 指定插件，将命令注册到 cache/plugins.json。

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Command, exit};

use colored::Colorize;

use super::init;
use super::paths::PathLayout;
use super::plugins::{PackageInfo, PluginCache, PluginCommand, empty_plugin_cache};
use crate::utils::display;
use crate::utils::json_io;

// ---------------------------------------------------------------------------
// 平台常量（与 plugins.rs 保持一致）
// ---------------------------------------------------------------------------

#[cfg(windows)]
const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
const VENV_BIN: &str = "bin";

#[cfg(windows)]
const PYTHON_BIN: &str = "python.exe";
#[cfg(not(windows))]
const PYTHON_BIN: &str = "python";

// ---------------------------------------------------------------------------
// 默认配置
// ---------------------------------------------------------------------------

const DEFAULT_BRANCH: &str = "main";
const CENTER_OWNER: &str = "fcbyk";
const CENTER_REPO: &str = "byk-plugins";

// ---------------------------------------------------------------------------
// Spec 解析
// ---------------------------------------------------------------------------

/// Spec 解析结果。
///
/// `center` = 中心仓库（无 user/repo 前缀）。
/// `community(user, repo)` = 社区仓库。
struct Spec<'a> {
    /// 社区仓库: Some("user/repo")，中心仓库: None
    community: Option<(&'a str, &'a str)>,
    /// 插件 key
    key: &'a str,
}

/// 解析 spec 字符串。
///
/// - 无 `/` → 中心仓库 + key
/// - 一个 `/`（user/repo） → 社区仓库 + 取 byk.json 第一个 key
/// - 两个 `/`（user/repo/key） → 社区仓库 + 指定 key
fn parse_spec<'a>(spec: &'a str) -> Option<Spec<'a>> {
    let parts: Vec<&str> = spec.splitn(3, '/').collect();
    match parts.len() {
        1 => Some(Spec {
            community: None,
            key: parts[0],
        }),
        2 => Some(Spec {
            community: Some((parts[0], parts[1])),
            key: "", // 稍后从 byk.json 取第一个
        }),
        3 => Some(Spec {
            community: Some((parts[0], parts[1])),
            key: parts[2],
        }),
        _ => None,
    }
}

/// 构建 raw.githubusercontent.com URL。
fn build_registry_url(branch: &str, owner: &str, repo: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/byk.json",
        owner, repo, branch,
    )
}

// ---------------------------------------------------------------------------
// HTTP 请求
// ---------------------------------------------------------------------------

/// 从指定 URL 获取 byk.json 内容。
fn fetch_registry(url: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if response.status() != 200 {
        return Err(format!("HTTP {} when fetching {}", response.status(), url));
    }

    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 安装插件。
///
/// 流程：
/// 1. 检查 venv 是否存在
/// 2. 解析 spec → 构建 URL → 获取 byk.json
/// 3. 查找 key → 解析 install.target 和 commands
/// 4. pip install
/// 5. 更新 cache/plugins.json
pub fn install_plugin(spec_str: &str, branch: Option<&str>, layout: &PathLayout) {
    let branch = branch.unwrap_or(DEFAULT_BRANCH);

    // 1. 检查 venv（不存在时提示创建）
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
    if !pip.is_file() {
        print!(
            "{} Python venv not found. Create? [Y/n] ",
            "?".yellow(),
        );
        let _ = io::stdout().flush();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            exit(1);
        }
        let answer = input.trim().to_lowercase();
        if answer.is_empty() || answer == "y" {
            init::init_py(layout);
            let pip = layout.venv_dir.join(VENV_BIN).join("pip");
            if !pip.is_file() {
                exit(1);
            }
        } else {
            exit(1);
        }
    }

    // 2. 解析 spec
    let spec = match parse_spec(spec_str) {
        Some(s) => s,
        None => {
            eprintln!("{} invalid spec: {}", "Error:".red(), spec_str);
            exit(1);
        }
    };

    let (owner, repo, source_label) = match spec.community {
        Some((u, r)) => (u, r, Some(format!("{}/{}", u, r))),
        None => (CENTER_OWNER, CENTER_REPO, None),
    };

    let url = build_registry_url(branch, owner, repo);

    // 3. 获取 byk.json
    let body = match fetch_registry(&url) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{} Failed to fetch registry: {}", "Error:".red(), e);
            exit(1);
        }
    };

    let registry: HashMap<String, serde_json::Value> = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} Failed to parse registry: {}", "Error:".red(), e);
            exit(1);
        }
    };

    // 4. 确定 key（社区仓库未指定 key 时取第一个）
    let key: String = if spec.key.is_empty() {
        registry.keys().next().cloned().unwrap_or_else(|| {
            eprintln!(
                "{} no plugins found in {}/{}",
                "Error:".red(),
                owner,
                repo,
            );
            exit(1);
        })
    } else {
        spec.key.to_string()
    };

    let entry = match registry.get(&key) {
        Some(e) => e,
        None => {
            eprintln!(
                "{} plugin \"{}\" not found in registry",
                "Error:".red(),
                key,
            );
            exit(1);
        }
    };

    // 5. 解析 install.target 和 install.name
    let install_obj = entry.get("install");
    let target = install_obj
        .and_then(|i| i.get("target"))
        .and_then(|t| t.as_str());

    let target = match target {
        Some(t) => t,
        None => {
            eprintln!(
                "{} plugin \"{}\" has no install target in byk.json",
                "Error:".red(),
                key,
            );
            exit(1);
        }
    };

    let install_name = install_obj
        .and_then(|i| i.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(&key);

    // 6. 解析 commands（新格式：commands 子对象）
    let commands_obj = entry
        .get("commands")
        .and_then(|c| c.as_object());

    // 7. pip install
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
    let status = Command::new(&pip)
        .arg("install")
        .arg(target)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!(
                "{} pip install failed with exit code {}",
                "Error:".red(),
                s.code().unwrap_or(1),
            );
            exit(1);
        }
        Err(e) => {
            eprintln!("{} Failed to run pip: {}", "Error:".red(), e);
            exit(1);
        }
    }

    // 8. 更新 cache/plugins.json
    let cache_file = layout.cache_dir.join("plugins.json");
    let mut cache: PluginCache = json_io::read_json(&cache_file).unwrap_or_else(empty_plugin_cache);

    let mut cmd_names: Vec<String> = Vec::new();

    if let Some(cmds) = commands_obj {
        for (cmd_name, cmd_value) in cmds {
            let module = cmd_value
                .get("module")
                .and_then(|v| v.as_str());

            let module = match module {
                Some(m) => m,
                None => continue,
            };

            let description = cmd_value
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            cmd_names.push(cmd_name.clone());
            cache.commands.insert(
                cmd_name.clone(),
                PluginCommand {
                    module: module.to_string(),
                    description: description.to_string(),
                },
            );
        }
    }

    cache.packages.insert(
        key.clone(),
        PackageInfo {
            name: install_name.to_string(),
            commands: cmd_names,
            source: source_label,
        },
    );

    // 确保 python_executable 已设置
    if cache.python_executable.is_none() {
        let py = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        cache.python_executable = Some(py.to_string_lossy().to_string());
    }

    json_io::write_json(&cache_file, &cache);

    println!(
        "{} plugin: {}",
        "Installed".green(),
        key.bold(),
    );
}

// ---------------------------------------------------------------------------
// 卸载
// ---------------------------------------------------------------------------

/// 卸载插件。
///
/// 流程：
/// 1. 读取 plugins.json，在 packages 中查找 key
/// 2. pip uninstall -y
/// 3. 删除 commands 中该插件的所有命令
/// 4. 删除 packages 中该 key
/// 5. 写回
pub fn uninstall_plugin(key: &str, layout: &PathLayout) {
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");

    // 1. 检查 venv
    if !pip.is_file() {
        eprintln!(
            "{} Python venv not found. Run {} first.",
            "Error:".red(),
            "`byk add <name>`".bold(),
        );
        exit(1);
    }

    // 2. 读取缓存
    let cache_file = layout.cache_dir.join("plugins.json");
    let mut cache: PluginCache = json_io::read_json(&cache_file).unwrap_or_else(empty_plugin_cache);

    let pkg = match cache.packages.get(key) {
        Some(p) => p.clone(),
        None => {
            eprintln!(
                "{} plugin \"{}\" is not installed",
                "Error:".red(),
                key,
            );
            exit(1);
        }
    };

    // 3. pip uninstall -y
    let status = Command::new(&pip)
        .arg("uninstall")
        .arg("-y")
        .arg(&pkg.name)
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!(
                "{} pip uninstall failed with exit code {}",
                "Error:".red(),
                s.code().unwrap_or(1),
            );
            exit(1);
        }
        Err(e) => {
            eprintln!("{} Failed to run pip: {}", "Error:".red(), e);
            exit(1);
        }
    }

    // 4. 删除 commands 中该插件的所有命令
    for cmd_name in &pkg.commands {
        cache.commands.remove(cmd_name);
    }

    // 5. 删除 packages 条目并写回
    cache.packages.remove(key);
    json_io::write_json(&cache_file, &cache);

    println!(
        "{} plugin: {}",
        "Uninstalled".green(),
        key.bold(),
    );
}

// ---------------------------------------------------------------------------
// 帮助
// ---------------------------------------------------------------------------

/// 渲染 `byk add` 帮助信息。
pub fn render_add_help() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk add [OPTIONS] <NAME>".bold());
    println!();
    println!("{}", "Options:".green().bold());
    println!(
        "  {:<16} {}",
        "--branch <NAME>".cyan().bold(),
        "Set branch (default: main)",
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk add hello".into(), "Install from center registry".into()),
        ("byk add user/repo/key".into(), "Install from community repo".into()),
        ("byk add user/repo".into(), "Install first key from community repo".into()),
        ("byk add --branch dev hello".into(), "Install from a specific branch".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}
