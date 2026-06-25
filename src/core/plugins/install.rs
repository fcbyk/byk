//! 插件安装流水线。
//!
//! 协议：插件名 → 操作块(pip/pip-keep/download/commands) → 具体配置。
//! 按 download → pip/pip-keep → commands 顺序执行，持久化到 plugins/plugins.cmd.json 和 plugins/plugins.pkg.json。

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
    /// 分支/tag/哈希（默认 main）
    branch: &'a str,
    /// 插件 key（空字符串表示取 byk.json 第一个 key）
    key: &'a str,
}

/// 解析 spec 字符串，支持 `@` 指定分支。
///
/// - `user/repo@branch/key` → 指定分支和 key
/// - `user/repo@branch`     → 指定分支，取 byk.json 第一个 key
/// - `user/repo/key`        → 默认 main 分支，指定 key
/// - `user/repo`            → 默认 main 分支，取 byk.json 第一个 key
fn parse_spec<'a>(spec: &'a str) -> Option<Spec<'a>> {
    let parts: Vec<&str> = spec.splitn(3, '/').collect();
    match parts.len() {
        2 => {
            let (repo_part, branch) = split_branch(parts[1]);
            Some(Spec {
                owner: parts[0],
                repo: repo_part,
                branch,
                key: "",
            })
        }
        3 => {
            let (repo_part, branch) = split_branch(parts[1]);
            Some(Spec {
                owner: parts[0],
                repo: repo_part,
                branch,
                key: parts[2],
            })
        }
        _ => None,
    }
}

/// 从 repo 部分分离分支名。
///
/// - `repo@branch` → (repo, branch)
/// - `repo`        → (repo, "main")
fn split_branch(repo_part: &str) -> (&str, &str) {
    repo_part.split_once('@').unwrap_or((repo_part, DEFAULT_BRANCH))
}

/// 引用解析的根路径。
/// 每个 byk.json 自治：相对路径始终相对于该文件所在目录。
/// 解析 ref 时 ref_base 跟随更新到新文件所在目录。
enum RefBase {
    /// 远程：https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{subpath}
    /// subpath 为空字符串时表示仓库根目录
    Remote {
        owner: String,
        repo: String,
        branch: String,
        subpath: String,
    },
    /// 本地：byk.json 所在目录
    Local(std::path::PathBuf),
    /// 远程 URL：byk.json 所在目录的 URL（用于解析相对 ref）
    UrlBase {
        base_url: String,
    },
}

/// 构建 raw.githubusercontent.com URL。
fn build_registry_url(branch: &str, owner: &str, repo: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/byk.json",
        owner, repo, branch,
    )
}

/// 将 raw.githubusercontent.com URL 转换为 jsDelivr CDN URL。
///
/// raw:  https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}
/// cdn:  https://cdn.jsdelivr.net/gh/{owner}/{repo}@{branch}/{path}
fn to_jsdelivr_url(raw_url: &str) -> String {
    let prefix = "https://raw.githubusercontent.com/";
    if let Some(rest) = raw_url.strip_prefix(prefix) {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() >= 3 {
            let owner = parts[0];
            let repo = parts[1];
            let branch_and_path = parts[2];
            let (branch, path) = branch_and_path
                .split_once('/')
                .unwrap_or((branch_and_path, ""));
            return format!("https://cdn.jsdelivr.net/gh/{}/{}@{}/{}", owner, repo, branch, path);
        }
    }
    raw_url.to_string()
}

/// 构建 jsDelivr CDN 的 registry URL。
fn build_cdn_registry_url(branch: &str, owner: &str, repo: &str) -> String {
    format!(
        "https://cdn.jsdelivr.net/gh/{}/{}@{}/byk.json",
        owner, repo, branch,
    )
}

/// 将相对路径拼接为完整 URL。
///
/// 例如 base_url="https://example.com/foo/byk.json", rel="./bar/other.json"
/// → "https://example.com/foo/bar/other.json"
fn resolve_relative_url(base_url: &str, rel: &str) -> String {
    let rel = rel.strip_prefix("./").unwrap_or(rel);
    let base = match base_url.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => base_url,
    };
    format!("{}/{}", base, rel)
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
// 资源路径解析
// ---------------------------------------------------------------------------

/// 资源定位结果。
enum ResolvedAsset {
    /// 远程 URL
    Url(String),
    /// 本地文件路径
    LocalPath(std::path::PathBuf),
}

/// 提取文件名（URL 或路径的最后一段）。
fn extract_filename(path_or_url: &str) -> String {
    path_or_url
        .rsplit('/')
        .next()
        .unwrap_or("script")
        .to_string()
}

/// 校验相对路径：拒绝 ../、/xxx、~ 前缀。
fn validate_relative_path(raw: &str) -> Result<&str, String> {
    if raw.starts_with('/') {
        return Err(format!(
            "absolute path '{}' is not allowed in plugin protocol.\n   Use './xxx' for relative paths or a full URL.",
            raw,
        ));
    }
    if raw.starts_with('~') {
        return Err(format!(
            "home path '{}' is not allowed in plugin protocol.\n   Use './xxx' for relative paths or a full URL.",
            raw,
        ));
    }
    if raw.contains("../") {
        return Err(format!(
            "'../' is not allowed in plugin protocol: '{}'.\n   Use './xxx' for subdirectory paths or a full URL.",
            raw,
        ));
    }
    if raw == ".." {
        return Err(
            "'..' is not allowed in plugin protocol.\n   Use './xxx' for subdirectory paths or a full URL."
                .to_string(),
        );
    }
    Ok(raw)
}

/// 根据 RefBase 和 cdn 标志解析相对路径，返回资源定位。
///
/// 路径规则：
/// - `https://...` → 远程 URL
/// - `./xxx` 或 `xxx` → 相对于 byk.json 所在目录
/// - `../`、`/xxx`、`~` → 报错
fn resolve_asset(raw: &str, ref_base: &RefBase, cdn: bool) -> Result<ResolvedAsset, String> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        let url = if cdn && raw.starts_with("https://raw.githubusercontent.com/") {
            to_jsdelivr_url(raw)
        } else {
            raw.to_string()
        };
        return Ok(ResolvedAsset::Url(url));
    }

    validate_relative_path(raw)?;
    let clean = raw.strip_prefix("./").unwrap_or(raw);

    match ref_base {
        RefBase::Remote { owner, repo, branch, subpath } => {
            let path = if subpath.is_empty() {
                clean.to_string()
            } else {
                format!("{}/{}", subpath, clean)
            };
            let url = if cdn {
                format!(
                    "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                    owner, repo, branch, path,
                )
            } else {
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}",
                    owner, repo, branch, path,
                )
            };
            Ok(ResolvedAsset::Url(url))
        }
        RefBase::Local(dir) => {
            Ok(ResolvedAsset::LocalPath(dir.join(clean)))
        }
        RefBase::UrlBase { base_url } => {
            let url = resolve_relative_url(base_url, raw);
            Ok(ResolvedAsset::Url(url))
        }
    }
}

/// 根据 RefBase 解析 ref 引用，返回 (新 byk.json 内容, 更新后的 ref_base)。
fn resolve_ref(ref_str: &str, ref_base: &RefBase, cdn: bool) -> Result<(String, RefBase), String> {
    if ref_str.starts_with("http://") || ref_str.starts_with("https://") {
        let url = if cdn && ref_str.starts_with("https://raw.githubusercontent.com/") {
            to_jsdelivr_url(ref_str)
        } else {
            ref_str.to_string()
        };
        let body = fetch_registry(&url)?;
        let new_base = RefBase::UrlBase {
            base_url: url,
        };
        return Ok((body, new_base));
    }

    validate_relative_path(ref_str)?;
    let clean = ref_str.strip_prefix("./").unwrap_or(ref_str);

    match ref_base {
        RefBase::Remote { owner, repo, branch, subpath } => {
            let new_subpath = if subpath.is_empty() {
                clean.to_string()
            } else {
                format!("{}/{}", subpath, clean)
            };
            let url = if cdn {
                format!(
                    "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                    owner, repo, branch, new_subpath,
                )
            } else {
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}",
                    owner, repo, branch, new_subpath,
                )
            };
            let body = fetch_registry(&url)?;
            let parent_subpath = Path::new(&new_subpath)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            Ok((body, RefBase::Remote {
                owner: owner.clone(),
                repo: repo.clone(),
                branch: branch.clone(),
                subpath: parent_subpath.to_string(),
            }))
        }
        RefBase::Local(dir) => {
            let full = dir.join(clean);
            let body = std::fs::read_to_string(&full)
                .map_err(|e| format!("failed to read ref: {}", e))?;
            let parent = full.parent().map(|p| p.to_path_buf())
                .unwrap_or_else(|| dir.clone());
            Ok((body, RefBase::Local(parent)))
        }
        RefBase::UrlBase { base_url } => {
            let url = resolve_relative_url(base_url, ref_str);
            let body = fetch_registry(&url)?;
            let new_base = RefBase::UrlBase { base_url: url };
            Ok((body, new_base))
        }
    }
}

// ---------------------------------------------------------------------------
// JSON 预处理：$var 变量替换
// ---------------------------------------------------------------------------

/// 预处理 byk.json：提取 $var，对原始 JSON 字符串做 {var} 占位符替换。
///
/// 每个变量替换一次（k 个变量 = k 遍），未定义变量静默保留原文。
/// 变量作用域仅限当前 JSON 文件，不穿透到 ref 引用的文件。
fn preprocess_registry(body: &str) -> Result<HashMap<String, serde_json::Value>, String> {
    // 先解析一次提取 $var
    let temp: HashMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse registry: {}", e))?;

    let vars = match temp.get("$var") {
        None => return Ok(temp),
        Some(v) => v
            .as_object()
            .ok_or_else(|| "\"$var\" must be a map".to_string())?,
    };

    // 收集所有字符串类型的变量
    let pairs: Vec<(&str, &str)> = vars
        .iter()
        .filter_map(|(k, val)| val.as_str().map(|s| (k.as_str(), s)))
        .collect();

    if pairs.is_empty() {
        return Ok(temp);
    }

    // 直接在原始字符串上逐变量 replace
    let mut body = body.to_string();
    for (key, val) in &pairs {
        body = body.replace(&format!("{{{key}}}"), val);
    }

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse registry: {}", e))
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 安装插件。
///
/// 流程：
/// 1. 检查 venv 是否存在
/// 2. --file 读本地文件，否则远程拉取
/// 3. 查找 key → ref 引用解析 → 遍历行为列表
/// 4. py-module：pip install；py-script：下载/拷贝脚本 + pip install 依赖
/// 5. 持久化到 plugins.cmd.json 和 plugins.pkg.json
pub fn install_plugin(
    spec_str: &str,
    file: Option<&str>,
    layout: &crate::core::paths::PathLayout,
    cdn: bool,
) {

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

    // 2. 获取 byk.json（--file 本地文件/URL 或 远程仓库）
    let (body, source_label, lookup_key, mut ref_base) = if let Some(f) = file {
        if f.starts_with("http://") || f.starts_with("https://") {
            let body = match fetch_registry(f) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{} failed to fetch {}: {}", "Error:".red(), f, e);
                    exit(1);
                }
            };
            (body, None, spec_str, RefBase::UrlBase {
                base_url: f.to_string(),
            })
        } else {
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
        }
    } else {
        let spec = match parse_spec(spec_str) {
            Some(s) => s,
            None => {
                eprintln!("{} invalid spec: {}", "Error:".red(), spec_str);
                exit(1);
            }
        };

        let (owner, repo, source_label) = (spec.owner, spec.repo, Some(format!("{}/{}", spec.owner, spec.repo)));

        let url = if cdn {
            build_cdn_registry_url(spec.branch, owner, repo)
        } else {
            build_registry_url(spec.branch, owner, repo)
        };

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
            branch: spec.branch.to_string(),
            subpath: String::new(),
        })
    };

    let registry: HashMap<String, serde_json::Value> = match preprocess_registry(&body) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{} {}", "Error:".red(), e);
            exit(1);
        }
    };

    // 3. 确定 key
    let valid_keys: Vec<&String> = registry
        .keys()
        .filter(|k| !k.starts_with('$'))
        .collect();

    let key: String = if lookup_key.is_empty() {
        // 未指定 key：$default > 唯一 key > 报错
        if let Some(default_val) = registry.get("$default") {
            let default_key = default_val.as_str().unwrap_or("").to_string();
            if default_key.is_empty() || !registry.contains_key(&default_key) {
                eprintln!(
                    "{} $default \"{}\" is not a valid plugin key",
                    "Error:".red(),
                    default_key,
                );
                exit(1);
            }
            default_key
        } else if valid_keys.len() == 1 {
            valid_keys[0].to_string()
        } else if valid_keys.is_empty() {
            eprintln!("{} no plugins found in registry", "Error:".red());
            exit(1);
        } else {
            let keys_str = valid_keys
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "{} no default plugin specified\n   Available keys: {}\n   Tip: use 'byk add <user>/<repo>/<key>' or set \"$default\" in byk.json",
                "Error:".red(),
                keys_str,
            );
            exit(1);
        }
    } else {
        let key_str = lookup_key.to_string();
        if !registry.contains_key(&key_str) {
            let keys_str = valid_keys
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "{} plugin \"{}\" not found in registry\n   Available keys: {}",
                "Error:".red(),
                key_str,
                keys_str,
            );
            exit(1);
        }
        key_str
    };

    let entry = &registry[&key];

    // 5. Ref 引用解析：entry 为字符串时拉取完整注册表（URL 或相对路径），取同名 key
    let entry_owned: serde_json::Value;
    let entry = if let Some(ref_str) = entry.as_str() {
        let (body, new_ref_base) = match resolve_ref(ref_str, &ref_base, cdn) {
            Ok(result) => result,
            Err(e) => {
                eprintln!(
                    "{} failed to resolve ref for plugin \"{}\": {}",
                    "Error:".red(),
                    key,
                    e,
                );
                exit(1);
            }
        };
        ref_base = new_ref_base;
        let registry: HashMap<String, serde_json::Value> = match preprocess_registry(&body) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "{} failed to parse ref for plugin \"{}\": {}",
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

    // 6. 按操作块执行：pip → pip-keep → commands
    let mut any_operation_processed = false;

    // 准备状态文件
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let scripts_dir = layout.plugins_dir.join("scripts");
    let mut cmd_state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    let mut pip_packages: Option<Vec<String>> = None;
    let mut pip_keep_packages: Option<Vec<String>> = None;
    let mut scripts: Vec<String> = Vec::new();
    let mut registered_commands: Vec<String> = Vec::new();

    // ---- 6a. pip：安装 Python 包到 venv（卸载时自动清理） ----
    if let Some(pip_list) = entry.get("pip").and_then(|v| v.as_array()) {
        any_operation_processed = true;

        let mut packages: Vec<String> = Vec::new();

        for pkg_val in pip_list {
            let pkg = match pkg_val.as_str() {
                Some(p) => p,
                None => continue,
            };
            install_python_package(pkg, layout);
            packages.push(pkg.to_string());
        }

        if !packages.is_empty() {
            pip_packages = Some(packages);
        }
    }

    // ---- 6b. pip-keep：安装 Python 包到 venv（卸载时保留） ----
    if let Some(pip_keep_list) = entry.get("pip-keep").and_then(|v| v.as_array()) {
        any_operation_processed = true;

        let mut packages: Vec<String> = Vec::new();

        for pkg_val in pip_keep_list {
            let pkg = match pkg_val.as_str() {
                Some(p) => p,
                None => continue,
            };
            install_python_package(pkg, layout);
            packages.push(pkg.to_string());
        }

        if !packages.is_empty() {
            pip_keep_packages = Some(packages);
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

            // py-script 的 entry 解析：相对路径/URL → 下载或拷贝到 scripts/，entry 简化为纯文件名
            if cmd_type == "py-script" {
                if !scripts_dir.exists()
                    && let Err(e) = std::fs::create_dir_all(&scripts_dir) {
                        eprintln!(
                            "{} failed to create scripts directory: {}",
                            "Error:".red(),
                            e,
                        );
                        exit(1);
                    }

                let filename = extract_filename(&entry_val);
                let dest_path = scripts_dir.join(&filename);

                match resolve_asset(&entry_val, &ref_base, cdn) {
                    Ok(ResolvedAsset::Url(url)) => {
                        if let Err(e) = download_script(&url, &dest_path) {
                            eprintln!("{} {}", "Error:".red(), e);
                            exit(1);
                        }
                    }
                    Ok(ResolvedAsset::LocalPath(path)) => {
                        if let Err(e) = std::fs::copy(&path, &dest_path) {
                            eprintln!(
                                "{} failed to copy script from {}: {}",
                                "Error:".red(),
                                path.display(),
                                e,
                            );
                            exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                }

                scripts.push(filename.clone());

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

    // ---- 6d. command：注册单个命令（命令名 = 插件 key） ----
    if let Some(cmd_value) = entry.get("command") {
        any_operation_processed = true;

        // 冲突检测：commands 中不能有与插件 key 同名的子命令
        if let Some(commands_obj) = entry.get("commands").and_then(|v| v.as_object())
            && commands_obj.contains_key(&key)
        {
            eprintln!(
                "{} command name conflict: \"{}\" is defined in both 'command' and 'commands'",
                "Error:".red(),
                key,
            );
            exit(1);
        }

        let cmd_type = cmd_value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("py-module");

        let mut entry_val = match cmd_value.get("entry").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                eprintln!("{} 'command' field requires 'entry'", "Error:".red());
                exit(1);
            }
        };

        // py-script 的 entry 解析：相对路径/URL → 下载或拷贝到 scripts/，entry 简化为纯文件名
        if cmd_type == "py-script" {
            if !scripts_dir.exists()
                && let Err(e) = std::fs::create_dir_all(&scripts_dir) {
                    eprintln!(
                        "{} failed to create scripts directory: {}",
                        "Error:".red(),
                        e,
                    );
                    exit(1);
                }

            let filename = extract_filename(&entry_val);
            let dest_path = scripts_dir.join(&filename);

            match resolve_asset(&entry_val, &ref_base, cdn) {
                Ok(ResolvedAsset::Url(url)) => {
                    if let Err(e) = download_script(&url, &dest_path) {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                }
                Ok(ResolvedAsset::LocalPath(path)) => {
                    if let Err(e) = std::fs::copy(&path, &dest_path) {
                        eprintln!(
                            "{} failed to copy script from {}: {}",
                            "Error:".red(),
                            path.display(),
                            e,
                        );
                        exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("{} {}", "Error:".red(), e);
                    exit(1);
                }
            }

            scripts.push(filename.clone());

            entry_val = filename;
        }

        let desc = cmd_value
            .get("desc")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        registered_commands.push(key.clone());
        cmd_state.commands.insert(
            key.clone(),
            PluginCommand {
                cmd_type: cmd_type.to_string(),
                entry: entry_val,
                desc: desc.to_string(),
            },
        );
    }

    if !any_operation_processed {
        eprintln!(
            "{} plugin \"{}\" has no supported operations (pip/pip-keep/command/commands)",
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
        pip: pip_packages,
        pip_keep: pip_keep_packages,
        scripts,
        commands: registered_commands,
    };
    pkg_state.insert(key.clone(), pkg_entry);

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
fn install_python_package(pkg: &str, layout: &crate::core::paths::PathLayout) {
    if is_uv_mode(layout) {
        let args = vec!["add", pkg];
        let status = Command::new("uv")
            .args(&args)
            .current_dir(&layout.py_venv_dir)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!(
                    "{} uv add failed with exit code {}",
                    "Error:".red(),
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
        let args = vec!["install", pkg];
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
    let pkg_state: PkgState = std::collections::HashMap::new();
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

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== parse_spec ====================

    #[test]
    fn parse_spec_user_repo() {
        let s = parse_spec("user/repo").unwrap();
        assert_eq!(s.owner, "user");
        assert_eq!(s.repo, "repo");
        assert_eq!(s.branch, "main");
        assert_eq!(s.key, "");
    }

    #[test]
    fn parse_spec_user_repo_at_branch() {
        let s = parse_spec("user/repo@dev").unwrap();
        assert_eq!(s.owner, "user");
        assert_eq!(s.repo, "repo");
        assert_eq!(s.branch, "dev");
        assert_eq!(s.key, "");
    }

    #[test]
    fn parse_spec_user_repo_key() {
        let s = parse_spec("user/repo/my-key").unwrap();
        assert_eq!(s.owner, "user");
        assert_eq!(s.repo, "repo");
        assert_eq!(s.branch, "main");
        assert_eq!(s.key, "my-key");
    }

    #[test]
    fn parse_spec_user_repo_at_branch_key() {
        let s = parse_spec("user/repo@v2/my-key").unwrap();
        assert_eq!(s.owner, "user");
        assert_eq!(s.repo, "repo");
        assert_eq!(s.branch, "v2");
        assert_eq!(s.key, "my-key");
    }

    #[test]
    fn parse_spec_invalid_single_part() {
        assert!(parse_spec("onlyone").is_none());
    }

    #[test]
    fn parse_spec_invalid_empty() {
        assert!(parse_spec("").is_none());
    }

    #[test]
    fn parse_spec_with_hash_branch() {
        let s = parse_spec("a/b@abc123/plugin").unwrap();
        assert_eq!(s.branch, "abc123");
        assert_eq!(s.key, "plugin");
    }

    // ==================== split_branch ====================

    #[test]
    fn split_branch_with_at() {
        let (repo, branch) = split_branch("repo@dev");
        assert_eq!(repo, "repo");
        assert_eq!(branch, "dev");
    }

    #[test]
    fn split_branch_without_at() {
        let (repo, branch) = split_branch("repo");
        assert_eq!(repo, "repo");
        assert_eq!(branch, "main");
    }

    #[test]
    fn split_branch_empty_before_at() {
        let (repo, branch) = split_branch("@dev");
        assert_eq!(repo, "");
        assert_eq!(branch, "dev");
    }

    // ==================== validate_relative_path ====================

    #[test]
    fn validate_relative_clean() {
        assert_eq!(validate_relative_path("foo.py").unwrap(), "foo.py");
    }

    #[test]
    fn validate_relative_dot_slash() {
        assert_eq!(validate_relative_path("./foo.py").unwrap(), "./foo.py");
    }

    #[test]
    fn validate_relative_subdir() {
        assert_eq!(validate_relative_path("./scripts/main.py").unwrap(), "./scripts/main.py");
    }

    #[test]
    fn validate_rejects_absolute() {
        assert!(validate_relative_path("/etc/passwd").is_err());
    }

    #[test]
    fn validate_rejects_home() {
        assert!(validate_relative_path("~/foo").is_err());
    }

    #[test]
    fn validate_rejects_parent_dir() {
        assert!(validate_relative_path("../escape").is_err());
    }

    #[test]
    fn validate_rejects_double_dot() {
        assert!(validate_relative_path("..").is_err());
    }

    #[test]
    fn validate_rejects_nested_parent() {
        assert!(validate_relative_path("a/../../b").is_err());
    }

    // ==================== extract_filename ====================

    #[test]
    fn extract_filename_from_url() {
        assert_eq!(extract_filename("https://example.com/path/to/script.py"), "script.py");
    }

    #[test]
    fn extract_filename_from_path() {
        assert_eq!(extract_filename("scripts/main.py"), "main.py");
    }

    #[test]
    fn extract_filename_single_name() {
        assert_eq!(extract_filename("just_name"), "just_name");
    }

    #[test]
    fn extract_filename_empty_last_segment() {
        // "dir/" rsplit('/') → ["", "dir"], next() = ""
        assert_eq!(extract_filename("dir/"), "");
    }

    // ==================== to_jsdelivr_url ====================

    #[test]
    fn jsdelivr_conversion() {
        let url = to_jsdelivr_url(
            "https://raw.githubusercontent.com/user/repo/main/foo/bar.py"
        );
        assert_eq!(url, "https://cdn.jsdelivr.net/gh/user/repo@main/foo/bar.py");
    }

    #[test]
    fn jsdelivr_root_file() {
        let url = to_jsdelivr_url(
            "https://raw.githubusercontent.com/user/repo/branch/byk.json"
        );
        assert_eq!(url, "https://cdn.jsdelivr.net/gh/user/repo@branch/byk.json");
    }

    #[test]
    fn jsdelivr_non_github_url_unchanged() {
        let url = to_jsdelivr_url("https://example.com/file.json");
        assert_eq!(url, "https://example.com/file.json");
    }

    #[test]
    fn jsdelivr_too_short_url_unchanged() {
        let url = to_jsdelivr_url("https://raw.githubusercontent.com/user");
        assert_eq!(url, "https://raw.githubusercontent.com/user");
    }

    // ==================== build_registry_url ====================

    #[test]
    fn registry_url_default() {
        let url = build_registry_url("main", "user", "repo");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/user/repo/main/byk.json"
        );
    }

    #[test]
    fn registry_url_custom_branch() {
        let url = build_registry_url("dev", "org", "tool");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/org/tool/dev/byk.json"
        );
    }

    // ==================== resolve_relative_url ====================

    #[test]
    fn resolve_relative_subfile() {
        let result = resolve_relative_url(
            "https://example.com/foo/byk.json",
            "./bar/other.json"
        );
        assert_eq!(result, "https://example.com/foo/bar/other.json");
    }

    #[test]
    fn resolve_relative_without_dot_slash() {
        let result = resolve_relative_url(
            "https://example.com/foo/byk.json",
            "bar/other.json"
        );
        assert_eq!(result, "https://example.com/foo/bar/other.json");
    }

    #[test]
    fn resolve_relative_from_root_like() {
        // base_url like "https://example.com/byk.json" → parent split fails,
        // rsplit_once('/') = Some(("https://example.com", "byk.json"))
        let result = resolve_relative_url(
            "https://example.com/byk.json",
            "./other.json"
        );
        assert_eq!(result, "https://example.com/other.json");
    }

    // ==================== preprocess_registry ====================

    #[test]
    fn preprocess_no_var() {
        let body = r#"{"key1": {"pip": ["requests"]}}"#;
        let result = preprocess_registry(body).unwrap();
        assert!(result.contains_key("key1"));
        assert!(!result.contains_key("$var"));
    }

    #[test]
    fn preprocess_with_var_replacement() {
        let body = r#"{"$var": {"PKG": "requests"}, "plugin": {"pip": ["{PKG}"]}}"#;
        let result = preprocess_registry(body).unwrap();
        let plugin = result.get("plugin").unwrap();
        let pip_list = plugin.get("pip").unwrap().as_array().unwrap();
        assert_eq!(pip_list[0].as_str().unwrap(), "requests");
    }

    #[test]
    fn preprocess_multiple_vars() {
        let body = r#"{"$var": {"A": "a_val", "B": "b_val"}, "x": {"entry": "{A}-{B}"}}"#;
        let result = preprocess_registry(body).unwrap();
        let x = result.get("x").unwrap();
        assert_eq!(x.get("entry").unwrap().as_str().unwrap(), "a_val-b_val");
    }

    #[test]
    fn preprocess_var_not_string_skipped() {
        let body = r#"{"$var": {"N": 42}, "x": {"entry": "{N}"}}"#;
        let result = preprocess_registry(body).unwrap();
        let x = result.get("x").unwrap();
        // {N} not replaced because 42 is not a string
        assert_eq!(x.get("entry").unwrap().as_str().unwrap(), "{N}");
    }

    #[test]
    fn preprocess_invalid_json() {
        let body = "not json";
        assert!(preprocess_registry(body).is_err());
    }

    #[test]
    fn preprocess_var_not_map() {
        let body = r#"{"$var": "not_a_map"}"#;
        assert!(preprocess_registry(body).is_err());
    }

    // ==================== build_cdn_registry_url ====================

    #[test]
    fn cdn_registry_url() {
        let url = build_cdn_registry_url("main", "user", "repo");
        assert_eq!(
            url,
            "https://cdn.jsdelivr.net/gh/user/repo@main/byk.json"
        );
    }
}