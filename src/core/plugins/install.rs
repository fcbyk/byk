//! 插件安装流水线。
//!
//! 三阶段架构：
//! 1. 获取 + 预处理 byk.json → HashMap
//! 2. 解析为 protocol::Registry → 构建 execution::InstallPlan（变量 / ref 全部解析完毕）
//! 3. 执行 InstallPlan → 持久化到 plugins.cmd.json / plugins.pkg.json

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, exit};

use colored::Colorize;

use super::protocol::{self, Registry};
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

/// 递归统计目录中的文件数量
fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files(&path);
            } else {
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// 资源路径解析
// ---------------------------------------------------------------------------

// ResolvedSrc 已迁移到 types.rs，直接复用。

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
fn resolve_asset(raw: &str, ref_base: &RefBase, cdn: bool) -> Result<ResolvedSrc, String> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        let url = if cdn && raw.starts_with("https://raw.githubusercontent.com/") {
            to_jsdelivr_url(raw)
        } else {
            raw.to_string()
        };
        return Ok(ResolvedSrc::Url(url));
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
            Ok(ResolvedSrc::Url(url))
        }
        RefBase::Local(dir) => {
            Ok(ResolvedSrc::LocalPath(dir.join(clean)))
        }
        RefBase::UrlBase { base_url } => {
            let url = resolve_relative_url(base_url, raw);
            Ok(ResolvedSrc::Url(url))
        }
    }
}

/// 根据 RefBase 解析 ref 引用，返回 (新 byk.json 内容, 更新后的 ref_base)。
fn resolve_ref(ref_str: &str, ref_base: &RefBase, cdn: bool) -> Result<(String, RefBase, String), String> {
    if ref_str.starts_with("http://") || ref_str.starts_with("https://") {
        let url = if cdn && ref_str.starts_with("https://raw.githubusercontent.com/") {
            to_jsdelivr_url(ref_str)
        } else {
            ref_str.to_string()
        };
        let body = fetch_registry(&url)?;
        let new_base = RefBase::UrlBase {
            base_url: url.clone(),
        };
        return Ok((body, new_base, url));
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
            }, url))
        }
        RefBase::Local(dir) => {
            let full = dir.join(clean);
            let body = std::fs::read_to_string(&full)
                .map_err(|e| format!("failed to read ref: {}", e))?;
            let parent = full.parent().map(|p| p.to_path_buf())
                .unwrap_or_else(|| dir.clone());
            Ok((body, RefBase::Local(parent), full.display().to_string()))
        }
        RefBase::UrlBase { base_url } => {
            let url = resolve_relative_url(base_url, ref_str);
            let body = fetch_registry(&url)?;
            let new_base = RefBase::UrlBase { base_url: url.clone() };
            Ok((body, new_base, url))
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

    for (key, val) in &pairs {
        println!(
            "{}",
            format!("Substituting placeholder: {{{}}} → {}", key, val).dimmed()
        );
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
/// 3. 查找 key → ref 引用解析 → 按顺序执行操作块
/// 4. pip → pip-keep → script → commands/command
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
            println!(
                "{}",
                "Fetching byk.json".dimmed()
            );
            println!(
                "  {}",
                format!("from {}", f).dimmed()
            );
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
            println!(
                "{}",
                format!("Reading byk.json from {}", f).dimmed()
            );
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

        println!(
            "{}",
            "Fetching byk.json".dimmed()
        );
        println!(
            "  {}",
            format!("from {}", url).dimmed()
        );

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

    println!(
        "{}",
        format!("Resolved key: {}", key).dimmed()
    );

    let entry = &registry[&key];

    // 5. Ref 引用解析：entry 为字符串时拉取完整注册表，取同名 key
    let registry = if let Some(ref_str) = entry.as_str() {
        let (body, new_ref_base, resolved_url) = match resolve_ref(ref_str, &ref_base, cdn) {
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
        println!(
            "{}",
            format!("Resolving ref: {}", ref_str).dimmed()
        );
        println!(
            "  {}",
            format!("→ {}", resolved_url).dimmed()
        );
        match preprocess_registry(&body) {
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
        }
    } else {
        registry
    };

    // 6. 解析协议 → 构建执行计划
    let registry = protocol::parse_registry(&registry);

    let source_display = source_label
        .as_ref()
        .map(|s| format!(" ({})", s.dimmed()))
        .unwrap_or_default();
    println!(
        "{} Installing plugin: {}{}",
        "==>".cyan().bold(),
        key.bold(),
        source_display,
    );

    let plan = build_install_plan(&registry, &key, source_label, &ref_base, cdn);

    // 7. 加载状态
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let mut cmd_state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    // 8. 执行计划
    execute_install_plan(&plan, layout, &mut cmd_state);

    // 9. 持久化
    if cmd_state.python_executable.is_none() {
        let py = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        cmd_state.python_executable = Some(py.to_string_lossy().to_string());
    }

    let pkg_entry = build_pkg_entry(&plan);
    pkg_state.insert(key.clone(), pkg_entry);

    json_io::write_json(&cmd_file, &cmd_state);
    json_io::write_json(&pkg_file, &pkg_state);

    println!(
        "{} installed {}",
        "Successfully".green().bold(),
        key.bold(),
    );
}

// ---------------------------------------------------------------------------
// 阶段 2：构建 InstallPlan（协议 → 执行，所有变量/ref 在此完成解析）
// ---------------------------------------------------------------------------

/// 从 Registry 构建单个插件的 InstallPlan。
///
/// 此阶段完成：
/// - 提取 $pip 全局依赖
/// - 解析 download.scripts → FileOp（URL/路径判断 + 变量替换）
/// - 解析 download.bin → BinOp（按 $tar 分流 Download/Extract）
/// - 合并 command + commands → Vec<CommandReg>
/// - 校验命令冲突（command 名不能出现在 commands 中）
fn build_install_plan(
    registry: &Registry,
    key: &str,
    source_label: Option<String>,
    ref_base: &RefBase,
    cdn: bool,
) -> InstallPlan {
    let def = match registry.plugins.get(key) {
        Some(d) => d.clone(),
        None => {
            eprintln!(
                "{} plugin \"{}\" not found in registry after parsing",
                "Error:".red(),
                key,
            );
            exit(1);
        }
    };

    let mut def = def;
    def.normalize();

    let mut scripts: Vec<FileOp> = Vec::new();
    let mut bins: Vec<BinOp> = Vec::new();
    let mut commands: Vec<CommandReg> = Vec::new();
    let platform = env!("PLATFORM");

    // download.scripts → FileOp
    if let Some(dl) = &def.downloads {
        if let Some(sm) = &dl.scripts {
            for (filename, src) in sm {
                match resolve_asset(src, ref_base, cdn) {
                    Ok(resolved) => {
                        scripts.push(FileOp {
                            filename: filename.clone(),
                            src: resolved,
                        });
                    }
                    Err(e) => {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                }
            }
        }

        // download.bin → BinOp (按 $tar 分流)
        if let Some(bm) = &dl.bin {
            for (name, bs) in bm {
                let url = match bs.urls.get(platform) {
                    Some(u) => u,
                    None => continue, // 当前平台无对应 URL，跳过
                };

                match resolve_asset(url, ref_base, cdn) {
                    Ok(resolved) => {
                        if bs.tar {
                            bins.push(BinOp::Extract {
                                dir_name: name.clone(),
                                src: resolved,
                            });
                        } else {
                            bins.push(BinOp::Download {
                                filename: name.clone(),
                                src: resolved,
                            });
                        }
                    }
                    Err(e) => {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                }
            }
        }
    }

    // command + commands → 合并为 Vec<CommandReg>
    let mut has_command = false;

    if let Some(cmd) = &def.command {
        commands.push(CommandReg {
            name: key.to_string(),
            cmd_type: cmd.cmd_type.clone(),
            entry: cmd.entry.clone(),
            desc: cmd.desc.clone(),
        });
        has_command = true;
    }

    if let Some(cs) = &def.commands {
        for (name, cd) in cs {
            // 冲突检测
            if has_command && name == key {
                eprintln!(
                    "{} command name conflict: \"{}\" is defined in both 'command' and 'commands'",
                    "Error:".red(),
                    key,
                );
                exit(1);
            }
            commands.push(CommandReg {
                name: name.clone(),
                cmd_type: cd.cmd_type.clone(),
                entry: cd.entry.clone(),
                desc: cd.desc.clone(),
            });
        }
    }

    // 校验：至少有一个操作
    if scripts.is_empty()
        && bins.is_empty()
        && commands.is_empty()
        && def.pip.as_ref().is_none_or(|p| p.is_empty())
        && registry.global_pip.is_empty()
    {
        eprintln!(
            "{} plugin \"{}\" has no supported operations (pip/download/command/commands)",
            "Error:".red(),
            key,
        );
        exit(1);
    }

    InstallPlan {
        global_pip: registry.global_pip.clone(),
        plugin: ResolvedPlugin {
            key: key.to_string(),
            source: source_label,
            pip_packages: def.pip.clone().unwrap_or_default(),
            scripts,
            bins,
            commands,
        },
    }
}

// ---------------------------------------------------------------------------
// 阶段 3：执行 InstallPlan（所有判断已在阶段 2 完成，此处纯流水线）
// ---------------------------------------------------------------------------

/// 执行安装计划：pip → 下载文件 → 解压 tar → 注册命令。
fn execute_install_plan(
    plan: &InstallPlan,
    layout: &crate::core::paths::PathLayout,
    cmd_state: &mut CmdState,
) {
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let scripts_dir = layout.plugins_dir.join("scripts");
    let bin_dir = layout.plugins_dir.join("bin");

    // 全局 pip（共享依赖，不随插件卸载）
    if !plan.global_pip.is_empty() {
        println!("{} $pip {}", "==>".cyan().bold(), "(shared)".dimmed());
        for pkg in &plan.global_pip {
            println!(
                "{}",
                format!("Installing pip package: {} (shared)", pkg).dimmed()
            );
            install_python_package(pkg, layout);
            println!("{} {}", "+".green(), pkg.bold());
        }
    }

    // 插件 pip（卸载时自动清理）
    if !plan.plugin.pip_packages.is_empty() {
        println!("{} pip", "==>".cyan().bold());
        for pkg in &plan.plugin.pip_packages {
            println!(
                "{}",
                format!("Installing pip package: {}", pkg).dimmed()
            );
            install_python_package(pkg, layout);
            println!("{} {}", "+".green(), pkg.bold());
        }
    }

    // 脚本文件
    if !plan.plugin.scripts.is_empty() {
        println!("{} scripts", "==>".cyan().bold());
        for op in &plan.plugin.scripts {
            let dest = scripts_dir.join(&op.filename);
            if !scripts_dir.exists()
                && let Err(e) = std::fs::create_dir_all(&scripts_dir)
            {
                eprintln!("{} failed to create scripts directory: {}", "Error:".red(), e);
                exit(1);
            }
            match &op.src {
                ResolvedSrc::Url(url) => {
                    println!("{}", format!("Downloading {}", op.filename).dimmed());
                    println!("  {}", format!("from {}", url).dimmed());
                    if let Err(e) = download_script(url, &dest) {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                    println!("{}", format!("Saving to {}", dest.display()).dimmed());
                }
                ResolvedSrc::LocalPath(path) => {
                    println!(
                        "{}",
                        format!("Copying {} from {}", op.filename, path.display()).dimmed()
                    );
                    if let Err(e) = std::fs::copy(path, &dest) {
                        eprintln!("{} failed to copy script from {}: {}", "Error:".red(), path.display(), e);
                        exit(1);
                    }
                }
            }
            println!("{} {}", "+".green(), op.filename.bold());
        }
    }

    // 二进制：一个 match 处理 Download 和 Extract 两种操作
    if !plan.plugin.bins.is_empty() {
        let platform = env!("PLATFORM");
        let mut header_printed = false;

        for op in &plan.plugin.bins {
            match op {
                BinOp::Download { filename, src } => {
                    if !header_printed {
                        println!("{} bin", "==>".cyan().bold());
                        header_printed = true;
                    }
                    let dest = bin_dir.join(filename);
                    if !bin_dir.exists()
                        && let Err(e) = std::fs::create_dir_all(&bin_dir)
                    {
                        eprintln!("{} failed to create bin directory: {}", "Error:".red(), e);
                        exit(1);
                    }
                    match src {
                        ResolvedSrc::Url(url) => {
                            println!("{}", format!("Downloading {} ({})", filename, platform).dimmed());
                            println!("  {}", format!("from {}", url).dimmed());
                            if let Err(e) = download_script(url, &dest) {
                                eprintln!("{} {}", "Error:".red(), e);
                                exit(1);
                            }
                            println!("{}", format!("Saving to {}", dest.display()).dimmed());
                        }
                        ResolvedSrc::LocalPath(path) => {
                            println!("{}", format!("Copying {} from {}", filename, path.display()).dimmed());
                            if let Err(e) = std::fs::copy(path, &dest) {
                                eprintln!("{} failed to copy binary from {}: {}", "Error:".red(), path.display(), e);
                                exit(1);
                            }
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(metadata) = std::fs::metadata(&dest) {
                            let mut perms = metadata.permissions();
                            perms.set_mode(0o755);
                            if let Err(e) = std::fs::set_permissions(&dest, perms) {
                                eprintln!("{} failed to set executable permission on {}: {}", "Error:".red(), dest.display(), e);
                                exit(1);
                            }
                        }
                    }
                    println!("{}", "chmod +x".dimmed());
                    println!("{} {}", "+".green(), filename.bold());
                }
                BinOp::Extract { dir_name, src } => {
                    if !header_printed {
                        println!("{} bin", "==>".cyan().bold());
                        header_printed = true;
                    }
                    let extract_dir = bin_dir.join(dir_name);
                    let _ = std::fs::create_dir_all(&extract_dir);

                    let temp_dir = layout.cache_dir.join("tmp-bin");
                    let _ = std::fs::create_dir_all(&temp_dir);
                    let temp_file = temp_dir.join("archive");

                    match src {
                        ResolvedSrc::Url(url) => {
                            println!("{}", format!("Downloading {} ({})", dir_name, platform).dimmed());
                            println!("  {}", format!("from {}", url).dimmed());
                            if let Err(e) = download_script(url, &temp_file) {
                                eprintln!("{} {}", "Error:".red(), e);
                                exit(1);
                            }
                        }
                        ResolvedSrc::LocalPath(path) => {
                            println!("{}", format!("Copying {} from {}", dir_name, path.display()).dimmed());
                            if let Err(e) = std::fs::copy(path, &temp_file) {
                                eprintln!("{} failed to copy archive from {}: {}", "Error:".red(), path.display(), e);
                                exit(1);
                            }
                        }
                    }

                    println!("{}", format!("Extracting to {}", extract_dir.display()).dimmed());

                    let status = std::process::Command::new("tar")
                        .args(["-xf", temp_file.to_str().unwrap(), "-C", extract_dir.to_str().unwrap()])
                        .status();

                    match status {
                        Ok(s) if s.success() => {}
                        Ok(s) => {
                            eprintln!("{} tar extract failed with exit code {}", "Error:".red(), s.code().unwrap_or(1));
                            exit(1);
                        }
                        Err(e) => {
                            eprintln!("{} failed to run tar: {}\n   On Windows 10+, tar is built-in.", "Error:".red(), e);
                            exit(1);
                        }
                    }

                    let file_count = count_files(&extract_dir);
                    println!("{}", format!("{} files extracted", file_count).dimmed());

                    let _ = std::fs::remove_file(&temp_file);
                    let _ = std::fs::remove_dir(&temp_dir);

                    println!("{} {}", "+".green(), dir_name.bold());
                }
            }
        }
    }

    // 命令注册
    if !plan.plugin.commands.is_empty() {
        println!("{} commands", "==>".cyan().bold());
        for cmd in &plan.plugin.commands {
            // validate bin entry path
            if cmd.cmd_type == "bin"
                && let Err(e) = validate_relative_path(&cmd.entry)
            {
                eprintln!("{} invalid entry for bin command: {}", "Error:".red(), e);
                exit(1);
            }
            println!(
                "{}",
                format!("Registering command: {} ({})", cmd.name, cmd.cmd_type).dimmed()
            );
            println!("  {}", format!("in {}", cmd_file.display()).dimmed());
            println!(
                "{} {} ({})",
                "+".green(),
                cmd.name.bold(),
                cmd.cmd_type.dimmed()
            );
            cmd_state.commands.insert(
                cmd.name.clone(),
                PluginCommand {
                    cmd_type: cmd.cmd_type.clone(),
                    entry: cmd.entry.clone(),
                    desc: cmd.desc.clone(),
                },
            );
        }
    }
}

/// 从 InstallPlan 构建 PkgEntry（用于持久化到 plugins.pkg.json）。
fn build_pkg_entry(plan: &InstallPlan) -> PkgEntry {
    let mut scripts = Vec::new();
    let mut bins = Vec::new();
    let mut bins_tar = Vec::new();

    for op in &plan.plugin.scripts {
        scripts.push(op.filename.clone());
    }

    for op in &plan.plugin.bins {
        match op {
            BinOp::Download { filename, .. } => bins.push(filename.clone()),
            BinOp::Extract { dir_name, .. } => bins_tar.push(dir_name.clone()),
        }
    }

    let commands: Vec<String> = plan.plugin.commands.iter().map(|c| c.name.clone()).collect();
    let pip = if plan.plugin.pip_packages.is_empty() {
        None
    } else {
        Some(plan.plugin.pip_packages.clone())
    };

    PkgEntry {
        source: plan.plugin.source.clone(),
        pip,
        scripts,
        bins,
        bins_tar,
        commands,
    }
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