/// `byk add` / `byk remove <key>` 命令实现。
///
/// 协议：插件名 → 操作块(install/download/commands) → 具体配置。
/// 按 download → install → commands 顺序执行，持久化到 plugins/plugins.cmd.json 和 plugins/plugins.pkg.json。

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Command, exit};

use colored::Colorize;

use super::init;
use super::paths::PathLayout;
use super::plugins::{
    CmdState, PluginCommand, PkgState, PkgEntry, InstallInfo, DownloadInfo,
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

/// 引用解析的根路径。
enum RefBase {
    /// 远程：https://raw.githubusercontent.com/{owner}/{repo}/{branch}/
    Remote {
        owner: String,
        repo: String,
        branch: String,
    },
    /// 本地：文件所在目录
    Local(std::path::PathBuf),
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
/// 4. py-module：pip install；py-script：下载/拷贝脚本 + pip install 依赖
/// 5. 持久化到 plugins.cmd.json 和 plugins.pkg.json
pub fn install_plugin(
    spec_str: &str,
    branch: Option<&str>,
    file: Option<&str>,
    editable: Option<&str>,
    layout: &PathLayout,
) {
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

    // 2. 获取 byk.json（-e 目录 或 --file 本地文件 或 远程仓库）
    let (body, source_label, lookup_key, ref_base) = if let Some(dir) = editable {
        let ed_json = std::path::PathBuf::from(dir).join("byk.json");
        let content = match std::fs::read_to_string(&ed_json) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} failed to read {}: {}", "Error:".red(), ed_json.display(), e);
                exit(1);
            }
        };
        (content, None, spec_str, RefBase::Local(std::path::PathBuf::from(dir)))
    } else if let Some(f) = file {
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} failed to read {}: {}", "Error:".red(), f, e);
                exit(1);
            }
        };
        let base = std::path::PathBuf::from(f)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        (content, None, spec_str, RefBase::Local(base))
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

        (body, source_label, spec.key, RefBase::Remote {
            owner: owner.to_string(),
            repo: repo.to_string(),
            branch: branch.to_string(),
        })
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

    // 5. Ref 引用解析：entry 为字符串时拉取完整注册表（URL 或相对路径），取同名 key
    let entry_owned: serde_json::Value;
    let entry = if let Some(ref_str) = entry.as_str() {
        let body = if ref_str.starts_with("http://") || ref_str.starts_with("https://") {
            fetch_registry(ref_str).unwrap_or_else(|e| {
                eprintln!(
                    "{} failed to fetch ref for plugin \"{}\": {}",
                    "Error:".red(),
                    key,
                    e,
                );
                exit(1);
            })
        } else {
            // 相对路径：按模式解析
            match &ref_base {
                RefBase::Remote { owner, repo, branch } => {
                    let clean = ref_str.strip_prefix("./").unwrap_or(ref_str);
                    let url = format!(
                        "https://raw.githubusercontent.com/{}/{}/{}/{}",
                        owner, repo, branch, clean,
                    );
                    fetch_registry(&url).unwrap_or_else(|e| {
                        eprintln!(
                            "{} failed to fetch ref for plugin \"{}\": {}",
                            "Error:".red(),
                            key,
                            e,
                        );
                        exit(1);
                    })
                }
                RefBase::Local(dir) => {
                    let full = dir.join(ref_str);
                    std::fs::read_to_string(&full).unwrap_or_else(|e| {
                        eprintln!(
                            "{} failed to read ref for plugin \"{}\": {}",
                            "Error:".red(),
                            key,
                            e,
                        );
                        exit(1);
                    })
                }
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

    // 6. 按操作块执行：download → install → commands
    let mut any_operation_processed = false;

    // 准备状态文件
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let scripts_dir = layout.plugins_dir.join("scripts");
    let mut cmd_state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    let pip = layout.venv_dir.join(VENV_BIN).join("pip");

    let mut install_info: Option<InstallInfo> = None;
    let mut download_info: Option<DownloadInfo> = None;
    let mut registered_commands: Vec<String> = Vec::new();

    // ---- 6a. download：下载远程文件到本地 scripts 目录 ----
    if let Some(dl_block) = entry.get("download").and_then(|v| v.as_object()) {
        any_operation_processed = true;

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

        let mut downloaded_scripts: Vec<String> = Vec::new();

        if let Some(urls) = dl_block.get("scripts").and_then(|v| v.as_array()) {
            for url_val in urls {
                let url = match url_val.as_str() {
                    Some(u) => u,
                    None => continue,
                };

                // 从 URL 最后一个斜杠后提取文件名
                let filename = url.rsplit('/').next().unwrap_or("script");
                let dest_path = scripts_dir.join(filename);

                if let Err(e) = download_script(url, &dest_path) {
                    eprintln!("{} {}", "Error:".red(), e);
                    exit(1);
                }

                downloaded_scripts.push(filename.to_string());
            }
        }

        if !downloaded_scripts.is_empty() {
            download_info = Some(DownloadInfo {
                scripts: downloaded_scripts,
            });
        }
    }

    // ---- 6b. install：安装包到 venv（-e 选项决定 pip 还是 pip-e，互斥）----
    if let Some(inst_block) = entry.get("install").and_then(|v| v.as_object()) {
        any_operation_processed = true;

        let mut pip_packages: Vec<String> = Vec::new();
        let mut pip_e_paths: Vec<String> = Vec::new();

        if let Some(ed_dir) = editable {
            // -e 模式：只执行 pip install -e
            if let Some(pip_e_list) = inst_block.get("pip-e").and_then(|v| v.as_array()) {
                for path_val in pip_e_list {
                    let rel_path = match path_val.as_str() {
                        Some(p) => p,
                        None => continue,
                    };

                    let install_dir = std::path::PathBuf::from(ed_dir).join(rel_path);

                    let status = Command::new(&pip)
                        .arg("install")
                        .arg("-e")
                        .arg(&install_dir)
                        .status();

                    match status {
                        Ok(s) if s.success() => {}
                        Ok(s) => {
                            eprintln!(
                                "{} pip install -e failed with exit code {}",
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

                    pip_e_paths.push(rel_path.to_string());
                }
            }
        } else {
            // 普通模式：只执行 pip install
            if let Some(pip_list) = inst_block.get("pip").and_then(|v| v.as_array()) {
                for pkg_val in pip_list {
                    let pkg = match pkg_val.as_str() {
                        Some(p) => p,
                        None => continue,
                    };

                    let status = Command::new(&pip)
                        .arg("install")
                        .arg(pkg)
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

                    pip_packages.push(pkg.to_string());
                }
            }
        }

        if !pip_packages.is_empty() || !pip_e_paths.is_empty() {
            install_info = Some(InstallInfo {
                pip: pip_packages,
                pip_e: pip_e_paths,
            });
        }
    }

    // ---- 6c. commands：直接合并到 plugins.cmd.json ----
    if let Some(commands_obj) = entry.get("commands").and_then(|v| v.as_object()) {
        any_operation_processed = true;

        for (cmd_name, cmd_value) in commands_obj {
            let cmd_type = cmd_value
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("py-module");

            let mut entry_val = match cmd_value.get("entry").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => continue,
            };

            // 语法糖：py-script 的 entry 如果是远程 URL，自动下载到 scripts/ 目录
            if cmd_type == "py-script"
                && (entry_val.starts_with("http://") || entry_val.starts_with("https://"))
            {
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

                let filename = entry_val.rsplit('/').next().unwrap_or("script").to_string();
                let dest_path = scripts_dir.join(&filename);

                if let Err(e) = download_script(&entry_val, &dest_path) {
                    eprintln!("{} {}", "Error:".red(), e);
                    exit(1);
                }

                match &mut download_info {
                    Some(info) => info.scripts.push(filename.clone()),
                    None => {
                        download_info = Some(DownloadInfo {
                            scripts: vec![filename.clone()],
                        });
                    }
                }

                entry_val = filename;
            }

            let desc = cmd_value
                .get("desc")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            registered_commands.push(cmd_name.clone());
            cmd_state.commands.insert(
                cmd_name.clone(),
                PluginCommand {
                    cmd_type: cmd_type.to_string(),
                    entry: entry_val,
                    desc: desc.to_string(),
                },
            );
        }
    }

    if !any_operation_processed {
        eprintln!(
            "{} plugin \"{}\" has no supported operations (install/download/commands)",
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
    let pkg_entry = PkgEntry {
        source: source_label,
        install: install_info,
        download: download_info,
        commands: registered_commands,
    };
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
/// 2. 删除下载的脚本文件
/// 3. 从 plugins.cmd.json 删除该插件的所有命令
/// 4. 从 plugins.pkg.json 删除该 key
/// 5. 写回
///
/// 注意：不卸载 pip 包，因为一个包可能被多个插件共享。
pub fn uninstall_plugin(key: &str, layout: &PathLayout) {
    // 1. 检查 venv
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
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

    // 3. 删除脚本文件
    if let Some(ref download) = pkg.download {
        for script in &download.scripts {
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
    }

    // 4. 删除 commands
    for cmd_name in &pkg.commands {
        cmd_state.commands.remove(cmd_name);
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