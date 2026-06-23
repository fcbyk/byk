//! 插件安装流水线。
//!
//! 协议：插件名 → 操作块(install/download/commands) → 具体配置。
//! 按 download → install → commands 顺序执行，持久化到 plugins/plugins.cmd.json 和 plugins/plugins.pkg.json。

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, exit};

use colored::Colorize;

use super::state::{empty_cmd_state, load_pkg_state};
use super::types::*;
use crate::utils::json_io;

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
fn download_script(url: &str, dest: &Path) -> Result<(), String> {
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
    layout: &crate::core::paths::PathLayout,
) {
    let branch = branch.unwrap_or(DEFAULT_BRANCH);

    // 1. 检查 venv（不存在时提示选择包管理器）
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
    let py_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    let venv_ready = pip.is_file() || py_exe.is_file();
    if !venv_ready {
        println!("{}", "? Python venv not found.".yellow());
        println!("Choose package manager:");
        println!("[1] pip  [2] uv  [n] cancel");
        print!("Enter your choice: ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            exit(1);
        }
        let answer = input.trim().to_lowercase();
        match answer.as_str() {
            "1" | "y" => init_py(layout, false),
            "2" => init_py(layout, true),
            _ => exit(1),
        }
        let pip = layout.venv_dir.join(VENV_BIN).join("pip");
        let py_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        if !pip.is_file() && !py_exe.is_file() {
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
    // 追踪 ref 解析后的有效目录，用于 pip-e 路径解析
    let mut effective_editable: Option<String> = editable.map(|s| s.to_string());
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
                    // 更新 effective_editable 为 ref 文件所在目录，确保 pip-e 路径基于 ref 文件解析
                    if let Some(parent) = full.parent() {
                        effective_editable = Some(parent.to_string_lossy().to_string());
                    }
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

    let mut install_info: Option<InstallInfo> = None;
    let mut download_info: Option<DownloadInfo> = None;
    let mut registered_commands: Vec<String> = Vec::new();

    // ---- 6a. download：下载远程文件到本地 scripts 目录 ----
    if let Some(dl_block) = entry.get("download").and_then(|v| v.as_object()) {
        any_operation_processed = true;

        // 确保 scripts 目录存在
        if !scripts_dir.exists()
            && let Err(e) = std::fs::create_dir_all(&scripts_dir) {
                eprintln!(
                    "{} failed to create scripts directory: {}",
                    "Error:".red(),
                    e,
                );
                exit(1);
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

        if let Some(ed_dir) = &effective_editable {
            // -e 模式：pip install -e / uv add --editable
            if let Some(pip_e_list) = inst_block.get("pip-e").and_then(|v| v.as_array()) {
                for path_val in pip_e_list {
                    let rel_path = match path_val.as_str() {
                        Some(p) => p,
                        None => continue,
                    };

                    let install_dir = std::path::PathBuf::from(ed_dir).join(rel_path);
                    install_python_package("", layout, Some(&install_dir));
                    pip_e_paths.push(rel_path.to_string());
                }
            } else {
                // 默认 pip-e = ["."]
                let install_dir = std::path::PathBuf::from(ed_dir);
                install_python_package("", layout, Some(&install_dir));
                pip_e_paths.push(".".to_string());
            }
        } else {
            // 普通模式：pip install / uv add
            if let Some(pip_list) = inst_block.get("pip").and_then(|v| v.as_array()) {
                for pkg_val in pip_list {
                    let pkg = match pkg_val.as_str() {
                        Some(p) => p,
                        None => continue,
                    };
                    install_python_package(pkg, layout, None);
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
                if !scripts_dir.exists()
                    && let Err(e) = std::fs::create_dir_all(&scripts_dir) {
                        eprintln!(
                            "{} failed to create scripts directory: {}",
                            "Error:".red(),
                            e,
                        );
                        exit(1);
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
// Python 包安装工具
// ---------------------------------------------------------------------------

/// 检测是否为 uv 管理模式（pyproject.toml 存在）
fn is_uv_mode(layout: &crate::core::paths::PathLayout) -> bool {
    layout.py_venv_dir.join("pyproject.toml").is_file()
}

/// 安装 Python 包（自动检测 py-v / uv 模式）
fn install_python_package(pkg: &str, layout: &crate::core::paths::PathLayout, editable: Option<&std::path::Path>) {
    if is_uv_mode(layout) {
        let mut args: Vec<&str> = vec!["add"];
        let ed_str;
        if let Some(ed) = editable {
            args.push("--editable");
            ed_str = ed.to_string_lossy().to_string();
            args.push(&ed_str);
        } else {
            args.push(pkg);
        }
        let status = Command::new("uv")
            .args(&args)
            .current_dir(&layout.py_venv_dir)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                let label = if editable.is_some() { "uv add --editable" } else { "uv add" };
                eprintln!(
                    "{} {} failed with exit code {}",
                    "Error:".red(),
                    label,
                    s.code().unwrap_or(1),
                );
                exit(1);
            }
            Err(e) => {
                eprintln!("{} Failed to run uv: {}", "Error:".red(), e);
                exit(1);
            }
        }
    } else {
        let pip = layout.venv_dir.join(VENV_BIN).join("pip");
        let mut args: Vec<&str> = vec!["install"];
        let ed_str;
        if let Some(ed) = editable {
            args.push("-e");
            ed_str = ed.to_string_lossy().to_string();
            args.push(&ed_str);
        } else {
            args.push(pkg);
        }
        let status = Command::new(&pip)
            .args(&args)
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
    }
}

// ---------------------------------------------------------------------------
// Python venv 初始化
// ---------------------------------------------------------------------------

/// 初始化 Python 虚拟环境。
///
/// 创建 ~/.byk/py-venv/.venv/（不存在时），写入 plugins.cmd.json、plugins.pkg.json
/// 和 alias/py.byk.json。is_uv=true 时使用 uv venv + uv init 创建 pyproject.toml。
pub fn init_py(layout: &crate::core::paths::PathLayout, is_uv: bool) {
    let venv_dir = &layout.venv_dir;
    let py_venv_dir = &layout.py_venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");
    let pyproject_toml = py_venv_dir.join("pyproject.toml");

    #[cfg(windows)]
    let sys_python = "python";
    #[cfg(not(windows))]
    let sys_python = "python3";

    // ensure common dirs
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(&layout.alias_dir, "alias");
    ensure_dir(&layout.cache_dir, "cache");
    ensure_dir(&layout.plugins_dir, "plugins");

    // ① 创建 venv（不存在时）
    if venv_dir.exists() {
        println!("{}", "venv already exists, skipping creation.".dimmed());
    } else {
        println!("{}", "Creating Python virtual environment...".dimmed());
        if is_uv {
            let status = Command::new("uv")
                .args(["venv", &venv_dir.to_string_lossy()])
                .status();
            match status {
                Ok(s) if s.success() => {
                    println!("  {} venv/ {}", "+".green(), "(created)".dimmed());
                }
                Ok(s) => {
                    eprintln!(
                        "{} uv venv failed with code {}",
                        "Error:".red(),
                        s.code().unwrap_or(1)
                    );
                    return;
                }
                Err(e) => {
                    eprintln!("{} Failed to run uv venv: {}", "Error:".red(), e);
                    return;
                }
            }
        } else {
            let status = Command::new(sys_python)
                .args(["-m", "venv", &venv_dir.to_string_lossy()])
                .status();
            match status {
                Ok(s) if s.success() => {
                    println!("  {} venv/ {}", "+".green(), "(created)".dimmed());
                }
                Ok(s) => {
                    eprintln!(
                        "{} venv creation failed with code {}",
                        "Error:".red(),
                        s.code().unwrap_or(1)
                    );
                    return;
                }
                Err(e) => {
                    eprintln!("{} Failed to create venv: {}", "Error:".red(), e);
                    return;
                }
            }
        }
    }

    // ② uv 模式：创建 pyproject.toml
    if is_uv && !pyproject_toml.is_file() {
        println!("{}", "Initializing uv project...".dimmed());
        let status = Command::new("uv")
            .args(["init", "--name", "byk", "--no-readme", "--no-pin-python"])
            .current_dir(py_venv_dir)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("  {} pyproject.toml {}", "+".green(), "(created)".dimmed());
            }
            Ok(s) => {
                eprintln!(
                    "{} uv init failed with code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1)
                );
                return;
            }
            Err(e) => {
                eprintln!("{} Failed to run uv init: {}", "Error:".red(), e);
                return;
            }
        }
    }

    // ③ 写入别名模板
    let template = if is_uv {
        serde_json::json!({
            "$cwd": "../py-venv/",
            "pi": "uv add",
            "pu": "uv remove",
            "pl": "uv tree",
        })
    } else {
        serde_json::json!({
            "$cwd": format!("../py-venv/.venv/{}/", VENV_BIN),
            "pi": "./pip install",
            "pu": "./pip uninstall",
            "pl": "./pip list",
        })
    };
    let template_str = serde_json::to_string_pretty(&template).unwrap_or_default();
    if alias_path.exists() {
        println!("  {} alias/py.byk.json {}", "*".dimmed(), "(updated)".dimmed());
    } else {
        println!("  {} alias/py.byk.json {}", "+".green(), "(created)".dimmed());
    }
    std::fs::write(&alias_path, template_str).unwrap_or_else(|e| {
        eprintln!("Failed to write alias/py.byk.json: {}", e);
    });

    // ④ 写入最小状态（venv 刚创建，无插件，commands 为空）
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let python_exe = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    let cmd_state = CmdState {
        commands: std::collections::HashMap::new(),
        python_executable: Some(python_exe.to_string_lossy().to_string()),
    };
    let pkg_state = PkgState {
        packages: std::collections::HashMap::new(),
    };
    json_io::write_json(&cmd_file, &cmd_state);
    json_io::write_json(&pkg_file, &pkg_state);
    println!(
        "  {} plugins/plugins.cmd.json {}",
        "+".green(),
        "(created)".dimmed()
    );
    println!(
        "  {} plugins/plugins.pkg.json {}",
        "+".green(),
        "(created)".dimmed()
    );

    println!();
    println!(
        "{} {}",
        "Python environment ready.".green(),
        if is_uv { "(uv)".dimmed() } else { "(pip)".dimmed() }
    );
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 创建目录，打印操作信息。
fn ensure_dir(path: &Path, label: &str) {
    if path.exists() {
        println!("  {} {} {}", "+".dimmed(), label.dimmed(), "(exists)".dimmed());
    } else {
        std::fs::create_dir_all(path).unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", label, e);
        });
        println!("  {} {}", "+".green(), label.dimmed());
    }
}