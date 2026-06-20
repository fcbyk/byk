/// `byk install` / `byk remove <key>` 命令实现。
///
/// 从中心仓库 fcbyk/byk-plugins 获取 byk.json，
/// pip install / uninstall 指定插件，并将命令注册到 cache/plugins.json。

use std::collections::HashMap;
use std::process::{Command, exit};

use colored::Colorize;

use super::paths::PathLayout;
use super::plugins::{PackageInfo, PluginCache, PluginCommand, empty_plugin_cache};
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
// 中心仓库 URL
// ---------------------------------------------------------------------------

const REGISTRY_URL: &str = "https://raw.githubusercontent.com/fcbyk/byk-plugins/main/byk.json";

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 安装插件。
///
/// 流程：
/// 1. 检查 venv 是否存在
/// 2. 从中心仓库获取 byk.json
/// 3. 查找 key → 解析 install.target 和 commands
/// 4. pip install
/// 5. 更新 cache/plugins.json
pub fn install_plugin(key: &str, layout: &PathLayout) {
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");

    // 1. 检查 venv
    if !pip.is_file() {
        eprintln!(
            "{} Python venv not found. Run {} first.",
            "Error:".red(),
            "`byk init py-v`".bold(),
        );
        exit(1);
    }

    // 2. 从中心仓库获取 byk.json
    let body = match fetch_registry() {
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

    // 3. 查找 key
    let entry = match registry.get(key) {
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

    // 4. 解析 install.target 和 install.name
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
        .unwrap_or(key);

    // 5. 解析 commands（新格式：commands 子对象）
    let commands_obj = entry
        .get("commands")
        .and_then(|c| c.as_object());

    // 6. pip install
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

    // 7. 更新 cache/plugins.json
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
        key.to_string(),
        PackageInfo {
            name: install_name.to_string(),
            commands: cmd_names,
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
            "`byk init py-v`".bold(),
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
// HTTP 请求
// ---------------------------------------------------------------------------

/// 从中心仓库获取 byk.json 内容。
fn fetch_registry() -> Result<String, String> {
    let response = ureq::get(REGISTRY_URL)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if response.status() != 200 {
        return Err(format!(
            "HTTP {} when fetching {}",
            response.status(),
            REGISTRY_URL,
        ));
    }

    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))
}
