/// `byk add` / `byk remove <key>` 命令实现。
///
/// 协议：插件名 → 行为类型(py-m/py-f/…) → 具体配置。
/// 遍历所有行为按顺序安装，持久化到 plugins/plugins.cmd.json 和 plugins/plugins.pkg.json。

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Command, exit};

use colored::Colorize;

use super::init;
use super::paths::PathLayout;
use super::plugins::{
    CmdState, PluginCommand, PkgState, PkgEntry, PyMInfo, PyFInfo,
    empty_cmd_state, load_pkg_state,
};
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

// ---------------------------------------------------------------------------
// Spec 解析
// ---------------------------------------------------------------------------

/// Spec 解析结果。
struct Spec<'a> {
    /// 仓库 owner
    owner: &'a str,
    /// 仓库名
    repo: &'a str,
    /// 插件 key（空字符串表示取 byk.json 第一个 key）
    key: &'a str,
}

/// 解析 spec 字符串。
///
/// - 一个 `/`（user/repo） → 取 byk.json 第一个 key
/// - 两个 `/`（user/repo/key） → 指定 key
fn parse_spec<'a>(spec: &'a str) -> Option<Spec<'a>> {
    let parts: Vec<&str> = spec.splitn(3, '/').collect();
    match parts.len() {
        2 => Some(Spec {
            owner: parts[0],
            repo: parts[1],
            key: "",
        }),
        3 => Some(Spec {
            owner: parts[0],
            repo: parts[1],
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

/// 下载脚本文件到目标路径。
fn download_script(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if response.status() != 200 {
        return Err(format!("HTTP {} when downloading {}", response.status(), url));
    }

    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    std::fs::write(dest, &body)
        .map_err(|e| format!("Failed to write script to {}: {}", dest.display(), e))?;

    Ok(())
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
/// 4. py-m：pip install；py-f：下载/拷贝脚本 + pip install 依赖
/// 5. 持久化到 plugins.cmd.json 和 plugins.pkg.json
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

        let (owner, repo, source_label) = (spec.owner, spec.repo, Some(format!("{}/{}", spec.owner, spec.repo)));

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

    // 5. Ref 引用解析：entry 为字符串 URL 时拉取完整注册表，取同名 key
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
        let registry: HashMap<String, serde_json::Value> = match serde_json::from_str(&body) {
            Ok(r) => r,
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
        entry_owned = registry
            .get(&key)
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
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

    let mut any_behavior_processed = false;

    // 准备状态文件
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let scripts_dir = layout.plugins_dir.join("scripts");
    let mut cmd_state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    let mut pkg_entry = PkgEntry {
        source: source_label,
        py_m: None,
        py_f: None,
    };

    let pip = layout.venv_dir.join(VENV_BIN).join("pip");

    for (behavior_type, behavior_config) in behaviors {
        match behavior_type.as_str() {
            "py-m" => {
                any_behavior_processed = true;

                let pkg_name = behavior_config
                    .get("pip")
                    .and_then(|v| v.as_str());

                if !is_editable && pkg_name.is_none() {
                    eprintln!(
                        "{} plugin \"{}\" py-m behavior missing \"pip\" field",
                        "Error:".red(),
                        key,
                    );
                    exit(1);
                }

                let effective_name = pkg_name.unwrap_or(&key);

                // pip install
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

                // 解析 commands
                let mut py_m_cmds: Vec<String> = Vec::new();
                if let Some(commands_obj) = behavior_config
                    .get("commands")
                    .and_then(|c| c.as_object())
                {
                    for (cmd_name, cmd_value) in commands_obj {
                        let target = cmd_value
                            .get("target")
                            .and_then(|v| v.as_str());

                        let target = match target {
                            Some(t) => t,
                            None => continue,
                        };

                        let description = cmd_value
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        py_m_cmds.push(cmd_name.clone());
                        cmd_state.commands.insert(
                            cmd_name.clone(),
                            PluginCommand {
                                behavior: "py-m".to_string(),
                                target: target.to_string(),
                                description: description.to_string(),
                            },
                        );
                    }
                }

                pkg_entry.py_m = Some(PyMInfo {
                    name: effective_name.to_string(),
                    commands: py_m_cmds,
                });
            }

            "py-f" => {
                any_behavior_processed = true;

                // 确保 scripts 目录存在
                if !scripts_dir.exists() {
                    if let Err(e) = std::fs::create_dir_all(&scripts_dir) {
                        eprintln!(
                            "{} failed to create scripts directory: {}",
                            "Error:".red(),
                            e,
                        );
                        exit(1);
                    }
                }

                let mut py_f_scripts: Vec<String> = Vec::new();
                let mut py_f_cmds: Vec<String> = Vec::new();
                let mut py_f_deps: Vec<String> = Vec::new();

                // py-f 支持对象（单条目）或数组（多条目）两种写法
                let entries: Vec<&serde_json::Value> = if let Some(arr) = behavior_config.as_array() {
                    arr.iter().collect()
                } else if behavior_config.is_object() {
                    vec![behavior_config]
                } else {
                    eprintln!(
                        "{} plugin \"{}\" py-f must be an object or array",
                        "Error:".red(),
                        key,
                    );
                    exit(1);
                };

                for entry in entries {
                        let cmd_name = entry
                            .get("commands")
                            .and_then(|v| v.as_str());

                        let cmd_name = match cmd_name {
                            Some(c) => c,
                            None => {
                                eprintln!(
                                    "{} plugin \"{}\" py-f entry missing \"commands\" field",
                                    "Error:".red(),
                                    key,
                                );
                                exit(1);
                            }
                        };

                        let script_filename = format!("{}.py", cmd_name);
                        let dest_path = scripts_dir.join(&script_filename);

                        // 下载或拷贝脚本
                        // 本地模式优先 local，fallback 到 url；远程模式只用 url
                        let local_path = entry
                            .get("local")
                            .and_then(|v| v.as_str());
                        let remote_url = entry
                            .get("url")
                            .and_then(|v| v.as_str());

                        if let Some(src) = local_path {
                            if let Err(e) = std::fs::copy(src, &dest_path) {
                                eprintln!(
                                    "{} failed to copy script from {} to {}: {}",
                                    "Error:".red(),
                                    src,
                                    dest_path.display(),
                                    e,
                                );
                                exit(1);
                            }
                        } else if let Some(u) = remote_url {
                            if let Err(e) = download_script(u, &dest_path) {
                                eprintln!("{} {}", "Error:".red(), e);
                                exit(1);
                            }
                        } else {
                            eprintln!(
                                "{} plugin \"{}\" py-f entry missing both \"local\" and \"url\" fields",
                                "Error:".red(),
                                key,
                            );
                            exit(1);
                        }

                        // 安装依赖
                        let deps: Vec<String> = entry
                            .get("dependencies")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|d| d.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        if !deps.is_empty() {
                            let mut pip_cmd = Command::new(&pip);
                            pip_cmd.arg("install");
                            for dep in &deps {
                                pip_cmd.arg(dep);
                            }
                            let dep_result = pip_cmd.status();
                            match dep_result {
                                Ok(s) if s.success() => {}
                                Ok(s) => {
                                    eprintln!(
                                        "{} pip install dependencies failed with exit code {}",
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
                            py_f_deps.extend(deps);
                        }

                        let description = entry
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        py_f_scripts.push(script_filename.clone());
                        py_f_cmds.push(cmd_name.to_string());
                        cmd_state.commands.insert(
                            cmd_name.to_string(),
                            PluginCommand {
                                behavior: "py-f".to_string(),
                                target: script_filename,
                                description: description.to_string(),
                            },
                        );
                    }

                if !py_f_scripts.is_empty() {
                    pkg_entry.py_f = Some(PyFInfo {
                        scripts: py_f_scripts,
                        commands: py_f_cmds,
                        dependencies: py_f_deps,
                    });
                }
            }
            // 其他 behavior 类型：未来扩展点
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

    // 确保 python_executable 已设置
    if cmd_state.python_executable.is_none() {
        let py = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        cmd_state.python_executable = Some(py.to_string_lossy().to_string());
    }

    // 写入 pkg 状态
    pkg_state.packages.insert(key.clone(), pkg_entry);

    json_io::write_json(&cmd_file, &cmd_state);
    json_io::write_json(&pkg_file, &pkg_state);

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
/// 1. 读取 plugins.pkg.json，在 packages 中查找 key
/// 2. py-m：pip uninstall -y
/// 3. py-f：删除脚本文件
/// 4. 从 plugins.cmd.json 删除该插件的所有命令
/// 5. 从 plugins.pkg.json 删除该 key
/// 6. 写回
pub fn uninstall_plugin(key: &str, layout: &PathLayout) {
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");

    // 1. 检查 venv
    if !pip.is_file() {
        eprintln!(
            "{} Python venv not found. Run {} first.",
            "Error:".red(),
            "`byk add <user/repo>`".bold(),
        );
        exit(1);
    }

    // 2. 读取状态
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let scripts_dir = layout.plugins_dir.join("scripts");

    let mut cmd_state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    let pkg = match pkg_state.packages.get(key) {
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

    // 3. py-m：pip uninstall -y
    if let Some(ref py_m) = pkg.py_m {
        let status = Command::new(&pip)
            .arg("uninstall")
            .arg("-y")
            .arg(&py_m.name)
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

        // 删除 commands
        for cmd_name in &py_m.commands {
            cmd_state.commands.remove(cmd_name);
        }
    }

    // 4. py-f：删除脚本文件
    if let Some(ref py_f) = pkg.py_f {
        for script in &py_f.scripts {
            let script_path = scripts_dir.join(script);
            if script_path.exists() {
                if let Err(e) = std::fs::remove_file(&script_path) {
                    eprintln!(
                        "{} Warning: failed to delete script {}: {}",
                        "Warning:".yellow(),
                        script_path.display(),
                        e,
                    );
                }
            }
        }

        // 删除 commands
        for cmd_name in &py_f.commands {
            cmd_state.commands.remove(cmd_name);
        }
    }

    // 5. 删除 packages 条目
    pkg_state.packages.remove(key);

    // 6. 写回
    json_io::write_json(&cmd_file, &cmd_state);
    json_io::write_json(&pkg_file, &pkg_state);

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
    println!("{}", " byk add [OPTIONS] <USER/REPO[/KEY]>".bold());
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
        ("byk add user/repo/key".into(), "Install specific key from a repo".into()),
        ("byk add user/repo".into(), "Install first key from a repo".into()),
        ("byk add --branch dev user/repo/key".into(), "Install from a specific branch".into()),
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