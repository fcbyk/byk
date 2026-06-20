/// `byk add` / `byk remove <key>` 命令实现。
///
/// 支持中心仓库 fcbyk/byk-plugins 和社区仓库 user/repo。
/// 协议：插件名 → 行为类型(pip/npm/…) → 具体配置(name, url, commands)。
/// 遍历所有行为按顺序安装，将命令持久化到 plugins/pip.json。

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Command, exit};

use colored::Colorize;

use super::init;
use super::paths::PathLayout;
use super::plugins::{PackageInfo, PluginState, PluginCommand, empty_plugin_state};
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
/// 2. -e 读 <dir>/byk.json，--file 读本地文件，否则远程拉取
/// 3. 查找 key → ref 引用解析 → 遍历行为列表
/// 4. -e 模式：pip install -e <dir>；否则 --file 模式 local ?? url ?? name，远程 url ?? name
/// 5. 收集所有行为的 commands，持久化到 plugins/pip.json
pub fn install_plugin(
    spec_str: &str,
    branch: Option<&str>,
    file: Option<&str>,
    editable: Option<&str>,
    layout: &PathLayout,
) {
    let branch = branch.unwrap_or(DEFAULT_BRANCH);
    let is_local = file.is_some();
    let is_editable = editable.is_some();

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

    // 2. 获取 byk.json（-e 目录 或 --file 本地文件 或 远程仓库）
    let (body, source_label, lookup_key) = if let Some(dir) = editable {
        let ed_json = std::path::PathBuf::from(dir).join("byk.json");
        let content = match std::fs::read_to_string(&ed_json) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} failed to read {}: {}", "Error:".red(), ed_json.display(), e);
                exit(1);
            }
        };
        (content, None, spec_str)
    } else if let Some(f) = file {
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} failed to read {}: {}", "Error:".red(), f, e);
                exit(1);
            }
        };
        (content, None, spec_str)
    } else {
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

        let body = match fetch_registry(&url) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{} Failed to fetch registry: {}", "Error:".red(), e);
                exit(1);
            }
        };

        (body, source_label, spec.key)
    };

    let registry: HashMap<String, serde_json::Value> = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} Failed to parse registry: {}", "Error:".red(), e);
            exit(1);
        }
    };

    // 3. 确定 key（未指定时取第一个）
    let key: String = if lookup_key.is_empty() {
        registry.keys().next().cloned().unwrap_or_else(|| {
            eprintln!(
                "{} no plugins found in registry",
                "Error:".red(),
            );
            exit(1);
        })
    } else {
        lookup_key.to_string()
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

    // 5. Ref 引用解析：entry 为字符串 URL 时拉取并替换
    let entry_owned: serde_json::Value;
    let entry = if let Some(url) = entry.as_str() {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            eprintln!(
                "{} plugin \"{}\" ref is not a valid URL: {}",
                "Error:".red(),
                key,
                url,
            );
            exit(1);
        }
        let body = match fetch_registry(url) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "{} failed to fetch ref for plugin \"{}\": {}",
                    "Error:".red(),
                    key,
                    e,
                );
                exit(1);
            }
        };
        let resolved: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "{} failed to parse ref response for plugin \"{}\": {}",
                    "Error:".red(),
                    key,
                    e,
                );
                exit(1);
            }
        };
        if !resolved.is_object() {
            eprintln!(
                "{} ref for plugin \"{}\" returned non-object JSON (expected {{ \"pip\": {{...}} }})",
                "Error:".red(),
                key,
            );
            exit(1);
        }
        entry_owned = resolved;
        &entry_owned
    } else {
        entry
    };

    // 6. 遍历行为列表（插件名.行为类型.具体配置）
    let behaviors = match entry.as_object() {
        Some(obj) => obj,
        None => {
            eprintln!(
                "{} plugin \"{}\" has no behaviors in byk.json",
                "Error:".red(),
                key,
            );
            exit(1);
        }
    };

    let mut install_name: Option<String> = None;
    let mut cmd_names: Vec<String> = Vec::new();
    let mut any_behavior_processed = false;

    // 提前准备 state（可能在多个行为间共享写入）
    let state_file = layout.plugins_dir.join("pip.json");
    let mut state: PluginState = json_io::read_json(&state_file).unwrap_or_else(empty_plugin_state);

    let pip = layout.venv_dir.join(VENV_BIN).join("pip");

    for (behavior_type, behavior_config) in behaviors {
        match behavior_type.as_str() {
            "pip" => {
                any_behavior_processed = true;

                // 解析 name（可编辑模式下可选，其它模式必填）
                let pkg_name = behavior_config
                    .get("name")
                    .and_then(|v| v.as_str());

                if !is_editable && pkg_name.is_none() {
                    eprintln!(
                        "{} plugin \"{}\" pip behavior missing \"name\" field",
                        "Error:".red(),
                        key,
                    );
                    exit(1);
                }

                let effective_name = pkg_name.unwrap_or(&key);

                // 记录第一个 name 用于状态（byk remove 时 pip uninstall 使用）
                if install_name.is_none() {
                    install_name = Some(effective_name.to_string());
                }

                // pip install：可编辑模式用 -e <dir>，否则按 local ?? url ?? name 选择目标
                let install_result = if is_editable {
                    let ed_dir = editable.unwrap();
                    let pyproject_dir = behavior_config
                        .get("pyproject")
                        .and_then(|v| v.as_str());
                    let install_dir = match pyproject_dir {
                        Some(p) => std::path::PathBuf::from(ed_dir).join(p),
                        None => std::path::PathBuf::from(ed_dir),
                    };
                    Command::new(&pip)
                        .arg("install")
                        .arg("-e")
                        .arg(install_dir)
                        .status()
                } else {
                    let install_target = if is_local {
                        behavior_config
                            .get("local")
                            .and_then(|v| v.as_str())
                            .or_else(|| behavior_config.get("url").and_then(|v| v.as_str()))
                            .unwrap_or(pkg_name.unwrap())
                    } else {
                        behavior_config
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or(pkg_name.unwrap())
                    };
                    Command::new(&pip)
                        .arg("install")
                        .arg(install_target)
                        .status()
                };

                match install_result {
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

                // 解析该 behavior 下的 commands
                if let Some(commands_obj) = behavior_config
                    .get("commands")
                    .and_then(|c| c.as_object())
                {
                    for (cmd_name, cmd_value) in commands_obj {
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
                        state.commands.insert(
                            cmd_name.clone(),
                            PluginCommand {
                                module: module.to_string(),
                                description: description.to_string(),
                            },
                        );
                    }
                }
            }
            // 其他 behavior 类型：未来扩展点（npm, alias 等），目前跳过
            _ => {}
        }
    }

    if !any_behavior_processed {
        eprintln!(
            "{} plugin \"{}\" has no supported install behavior",
            "Error:".red(),
            key,
        );
        exit(1);
    }

    // 写入状态
    let install_name = install_name.unwrap_or_else(|| key.clone());
    state.packages.insert(
        key.clone(),
        PackageInfo {
            name: install_name,
            commands: cmd_names,
            source: source_label,
            behavior: Some("pip".to_string()),
        },
    );

    // 确保 python_executable 已设置
    if state.python_executable.is_none() {
        let py = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        state.python_executable = Some(py.to_string_lossy().to_string());
    }

    json_io::write_json(&state_file, &state);

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
/// 1. 读取 pip.json，在 packages 中查找 key
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

    // 2. 读取状态
    let state_file = layout.plugins_dir.join("pip.json");
    let mut state: PluginState = json_io::read_json(&state_file).unwrap_or_else(empty_plugin_state);

    let pkg = match state.packages.get(key) {
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
        state.commands.remove(cmd_name);
    }

    // 5. 删除 packages 条目并写回
    state.packages.remove(key);
    json_io::write_json(&state_file, &state);

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
    println!(
        "  {:<16} {}",
        "--file <PATH>".cyan().bold(),
        "Use local byk.json instead of remote registry",
    );
    println!(
        "  {:<16} {}",
        "-e, --editable <DIR>".cyan().bold(),
        "Editable install (pip install -e <DIR>, reads <DIR>/byk.json)",
    );
    println!();
    println!("{}", "Examples:".green().bold());
    let examples: Vec<(String, String)> = vec![
        ("byk add hello".into(), "Install from center registry".into()),
        ("byk add user/repo/key".into(), "Install from community repo".into()),
        ("byk add user/repo".into(), "Install first key from community repo".into()),
        ("byk add --branch dev hello".into(), "Install from a specific branch".into()),
        ("byk add --file ./local.json my-key".into(), "Install from local registry file".into()),
        ("byk add -e .".into(), "Editable install from current directory".into()),
    ];
    let aligned = display::align_kv_pairs(&examples, "  ");
    for (name, line) in &aligned {
        let rest = &line[2 + name.len()..];
        print!("  {}", name.dimmed());
        println!("{}", rest);
    }
    println!();
}
