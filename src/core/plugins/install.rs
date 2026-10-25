//! 插件安装流水线。
//!
//! 三阶段架构：
//! 1. 获取 + 预处理 byk.json → HashMap
//! 2. 解析为 protocol::Registry → 构建 execution::InstallPlan（变量 / ref 全部解析完毕）
//! 3. 执行 InstallPlan → 持久化到 plugins.cmd.json / plugins.pkg.json

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use colored::Colorize;

use super::protocol::{self, Registry};
use super::state::{load_pkg_state, load_plugin_state};
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
pub(crate) fn to_jsdelivr_url(raw_url: &str) -> String {
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

/// 下载脚本文件到目标路径（流式写入 + 进度条）。
pub(crate) fn download_script(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if response.status() != 200 {
        return Err(format!("HTTP {} when downloading {}", response.status(), url));
    }

    // 从响应头获取文件总大小（可能不存在）
    let total_size: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("Failed to create file {}: {}", dest.display(), e))?;

    let mut buf = [0u8; 8192];
    let mut downloaded: u64 = 0;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download failed: {}", e))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Failed to write file: {}", e))?;
        downloaded += n as u64;

        // 每 64KB 刷新一次进度条，避免刷屏
        if downloaded % 65536 < 8192 || downloaded == total_size.unwrap_or(0) {
            if let Some(total) = total_size {
                if total > 0 {
                    let pct = (downloaded * 100).checked_div(total).unwrap_or(0);
                    let bar_width: usize = 30;
                    let filled = (pct as usize * bar_width / 100).min(bar_width);
                    let bar = "#".repeat(filled) + &" ".repeat(bar_width - filled);
                    eprint!(
                        "\r  [{bar}] {:>3}%  ({}/{})\x1b[K",
                        pct,
                        format_size(downloaded),
                        format_size(total)
                    );
                }
            } else {
                eprint!("\r  Downloaded {}\x1b[K", format_size(downloaded));
            }
        }
    }

    // 清除进度行，输出完成提示
    let final_size = total_size.unwrap_or(downloaded);
    eprint!("\r\x1b[K");
    eprintln!("  Downloaded ({})", format_size(final_size));

    Ok(())
}

/// 将字节数格式化为人类可读的大小字符串。
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{}B", bytes)
    } else {
        format!("{:.1}{}", size, UNITS[idx])
    }
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

/// 从 URL 中提取文件名（最后一个路径段），提取不到返回 "unknown"。
fn extract_filename_from_url(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let path = path.split('#').next().unwrap_or(path);
    let segments: Vec<&str> = path.split('/').collect();
    let last = segments.last().unwrap_or(&"");
    if last.is_empty() { "unknown".to_string() } else { last.to_string() }
}

/// 检测 URL 是否有 `[tar]` 前缀。
fn is_tar_url(url: &str) -> bool {
    url.starts_with("[tar] ")
}

/// 剥离 `[tar] ` 前缀，返回真实 URL。
fn strip_tar_prefix(url: &str) -> &str {
    url.strip_prefix("[tar] ").unwrap_or(url)
}

/// 检测 URL 是否有 `[exe]` 前缀。
fn is_exe_url(url: &str) -> bool {
    url.starts_with("[exe] ")
}

/// 剥离 `[exe] ` 前缀，返回真实 URL。
fn strip_exe_prefix(url: &str) -> &str {
    url.strip_prefix("[exe] ").unwrap_or(url)
}

/// 剥离所有已知前缀（`[tar] ` 或 `[exe] `），返回真实 URL。
fn strip_prefix(url: &str) -> &str {
    strip_exe_prefix(strip_tar_prefix(url))
}

/// Peek 压缩包内容：返回顶层条目数。
/// 不实际解压，只列出内容。
fn peek_archive(path: &Path) -> Result<usize, String> {
    let output = Command::new("tar")
        .args(["-tf", path.to_str().unwrap()])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let entries: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
            return Ok(entries.len());
        }
        _ => {}
    }

    let output = Command::new("unzip")
        .args(["-l", path.to_str().unwrap()])
        .output()
        .map_err(|e| format!("failed to peek archive: {}", e))?;

    if !output.status.success() {
        return Err("failed to peek archive with both tar and unzip".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("Archive:")
                && !trimmed.starts_with("Length")
                && !trimmed.starts_with("---")
                && !trimmed.contains("file")
                && !trimmed.contains("files")
        })
        .count();
    Ok(count)
}

/// 解压压缩包到目标目录。
pub(crate) fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    if !dest.exists()
        && let Err(e) = std::fs::create_dir_all(dest)
    {
        return Err(format!("failed to create directory {}: {}", dest.display(), e));
    }

    let status = Command::new("tar")
        .args(["-xf", archive.to_str().unwrap(), "-C", dest.to_str().unwrap()])
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!(
            "tar extract failed with exit code {}",
            s.code().unwrap_or(1)
        )),
        Err(e) => Err(format!("failed to run tar: {}", e)),
    }
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
/// 将变量值解析为字符串：
/// - 字符串 → 直接使用
/// - 平台对象 `{"darwin-arm64": "...", ...}` → 取当前平台对应的值
fn resolve_var_to_string(val: &serde_json::Value, platform: &str) -> Option<String> {
    if let Some(s) = val.as_str() {
        Some(s.to_string())
    } else if let Some(obj) = val.as_object() {
        obj.get(platform).and_then(|v| v.as_str()).map(|s| s.to_string())
    } else {
        None
    }
}

fn preprocess_registry(
    body: &str,
    platform: &str,
) -> Result<HashMap<String, serde_json::Value>, String> {
    // 先解析一次提取 $var
    let temp: HashMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse registry: {}", e))?;

    let vars = match temp.get("$var") {
        None => return Ok(temp),
        Some(v) => v
            .as_object()
            .ok_or_else(|| "\"$var\" must be a map".to_string())?,
    };

    // 收集所有变量（字符串直接使用，平台对象取当前平台）
    let pairs: Vec<(String, String)> = vars
        .iter()
        .filter_map(|(k, val)| {
            resolve_var_to_string(val, platform).map(|s| (k.clone(), s))
        })
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

    let mut result: HashMap<String, serde_json::Value> =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse registry: {}", e))?;

    // 将 $var 中的平台对象也替换为解析后的字符串，方便下游 parse_registry
    if let Some(var_map) = result.get_mut("$var").and_then(|v| v.as_object_mut()) {
        for (key, val) in &pairs {
            var_map.insert(key.clone(), serde_json::Value::String(val.clone()));
        }
    }

    Ok(result)
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
/// 4. pip-keep → pip → downloads → commands/command
/// 5. 持久化到 plugins.cmd.json 和 plugins.pkg.json
pub fn install_plugin(
    spec_str: &str,
    file: Option<&str>,
    layout: &crate::core::paths::PathLayout,
    cdn: bool,
) {

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

    let platform = env!("PLATFORM");
    let registry: HashMap<String, serde_json::Value> = match preprocess_registry(&body, platform) {
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
        match preprocess_registry(&body, platform) {
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

    // 仅当插件需要 Python venv 时才检查/初始化：
    // 协议中包含 pip / pip-keep 依赖，或注册了 python / python-m 类型命令
    let needs_venv = registry
        .plugins
        .get(&key)
        .map(plugin_needs_venv)
        .unwrap_or(false);

    if needs_venv {
        ensure_python_env(layout, &format!("plugin \"{}\"", key));
    }

    // 仅当插件协议中声明了 node 类型命令时，才检查 Node.js 环境。
    // 与 Python 不同：node 由用户自行管理（PATH 解析），byk 不创建
    // 也不托管运行时，缺失时直接拒绝安装并给出指引。
    let needs_node = registry.plugins.get(&key)
        .map(|def| {
            def.command
                .as_ref()
                .is_some_and(|c| c.cmd_type == "node")
                || def
                    .commands
                    .as_ref()
                    .is_some_and(|cs| cs.values().any(|c| c.cmd_type == "node"))
        })
        .unwrap_or(false);

    if needs_node && !tool_exists("node") {
        eprintln!(
            "{} plugin \"{}\" requires Node.js, but `node` was not found in PATH.",
            "Error:".red(),
            key
        );
        eprintln!();
        eprintln!("Install Node.js, then retry:");
        print_node_install_hints();
        exit(1);
    }

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

    let plan = build_install_plan(&registry, &key, source_label, &ref_base, cdn, layout);

    // 7. 加载状态（顺带迁移旧版条目：pip-bin → bin，相对 entry → 绝对路径）
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let mut cmd_state: CmdState = load_plugin_state(&layout.plugins_dir, &layout.venv_dir);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    // 8. 执行计划
    execute_install_plan(&plan, layout, &mut cmd_state);

    // 9. 持久化（workdir-only 插件仅下载文件到工作目录，无需记录安装状态）
    if !is_workdir_only(&plan) {
        // 仅当插件依赖 Python 时才记录解释器路径，
        // 避免为纯 bin 插件写入可能失效的 venv 路径
        if needs_venv && cmd_state.python_executable.is_none() {
            let py = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
            cmd_state.python_executable = Some(py.to_string_lossy().to_string());
        }

        let pkg_entry = build_pkg_entry(&plan);
        pkg_state.insert(key.clone(), pkg_entry);

        json_io::write_json(&cmd_file, &cmd_state);
        json_io::write_json(&pkg_file, &pkg_state);
    }

    println!(
        "{} installed {}",
        "Successfully".green().bold(),
        key.bold(),
    );
}

// ---------------------------------------------------------------------------
// 阶段 2：构建 InstallPlan（协议 → 执行，所有变量/ref 在此完成解析）
// ---------------------------------------------------------------------------

/// 递归展平下载映射为 Asset 列表。
///
/// - `DownloadsSection::BareUrl` → 单个 Asset，文件名从 URL 提取
/// - `DownloadsSection::Map` → 遍历条目
///   workdir 模式下：所有文件放入容器目录（默认 "downloads"，可用 `$name` 修改）
fn flatten_downloads(
    section: &super::protocol::DownloadsSection,
    target: AssetTarget,
    ref_base: &RefBase,
    cdn: bool,
    workdir_mode: bool,
) -> Vec<Asset> {
    let mut assets = Vec::new();

    match section {
        super::protocol::DownloadsSection::BareUrl(src) => {
            let is_archive = is_tar_url(src);
            let is_exe = is_exe_url(src);
            let clean = strip_prefix(src);
            match resolve_asset(clean, ref_base, cdn) {
                Ok(resolved) => {
                    let name = extract_filename_from_url(clean);
                    assets.push(Asset {
                        name,
                        target,
                        src: resolved,
                        is_archive,
                        chmod_x: is_exe,
                    });
                }
                Err(e) => {
                    eprintln!("{} {}", "Error:".red(), e);
                    exit(1);
                }
            }
        }
        super::protocol::DownloadsSection::Map(map) => {
            // workdir Map 模式：提取容器目录名
            let container = if workdir_mode {
                map.get("$name")
                    .and_then(|v| match v {
                        super::protocol::DownloadValue::Url(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .unwrap_or("downloads")
            } else {
                ""
            };

            for (key, value) in map {
                if key.starts_with('$') {
                    continue;
                }
                match value {
                    super::protocol::DownloadValue::Url(src) => {
                        let is_archive = is_tar_url(src);
                        let is_exe = is_exe_url(src);
                        let clean = strip_prefix(src);
                        match resolve_asset(clean, ref_base, cdn) {
                            Ok(resolved) => {
                                let name = if workdir_mode {
                                    format!("{}/{}", container, key)
                                } else {
                                    key.clone()
                                };
                                assets.push(Asset {
                                    name,
                                    target,
                                    src: resolved,
                                    is_archive,
                                    chmod_x: is_exe,
                                });
                            }
                            Err(e) => {
                                eprintln!("{} {}", "Error:".red(), e);
                                exit(1);
                            }
                        }
                    }
                    super::protocol::DownloadValue::Tree(entries) => {
                        let root_name = key.clone();
                        let root_name = if workdir_mode {
                            format!("{}/{}", container, root_name)
                        } else {
                            root_name
                        };
                        flatten_entries(entries, &root_name, target, "", ref_base, cdn, &mut assets);
                    }
                }
            }
        }
    }

    assets
}

/// 递归展平 DownloadEntry 树到 assets。
fn flatten_entries(
    entries: &HashMap<String, super::protocol::DownloadEntry>,
    root_name: &str,
    target: AssetTarget,
    prefix: &str,
    ref_base: &RefBase,
    cdn: bool,
    assets: &mut Vec<Asset>,
) {
    for (name, entry) in entries {
        if name.starts_with('$') {
            continue;
        }
        let rel_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        match entry {
            super::protocol::DownloadEntry::File(src) => {
                let is_archive = is_tar_url(src);
                let is_exe = is_exe_url(src);
                let clean = strip_prefix(src);
                match resolve_asset(clean, ref_base, cdn) {
                    Ok(resolved) => {
                        assets.push(Asset {
                            name: format!("{}/{}", root_name, rel_path),
                            target,
                            src: resolved,
                            is_archive,
                            chmod_x: is_exe,
                        });
                    }
                    Err(e) => {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                }
            }
            super::protocol::DownloadEntry::Dir(sub_entries) => {
                flatten_entries(sub_entries, root_name, target, &rel_path, ref_base, cdn, assets);
            }
        }
    }
}

/// 从 Registry 构建单个插件的 InstallPlan。
///
/// 此阶段完成所有判断：
/// - 提取 pip-keep 和 pip 依赖
/// - `[tar]` / `[exe]` 前缀检测 → is_archive / chmod_x
/// - key 推导 → name
/// - 合并 command + commands → Vec<CommandReg>
/// - 注入 plugin_key 前缀到 python / bin 类型的 entry
/// - 产出统一的 Vec<Asset>
fn build_install_plan(
    registry: &Registry,
    key: &str,
    source_label: Option<String>,
    ref_base: &RefBase,
    cdn: bool,
    layout: &crate::core::paths::PathLayout,
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

    let mut assets: Vec<Asset> = Vec::new();
    let mut commands: Vec<CommandReg> = Vec::new();

    // downloads → PluginDir
    if let Some(dl) = &def.downloads {
        assets.extend(flatten_downloads(dl, AssetTarget::PluginDir, ref_base, cdn, false));
    }

    // download-to-workdir → Workdir (workdir_mode=true 支持 $name)
    if let Some(dw) = &def.download_to_workdir {
        assets.extend(flatten_downloads(dw, AssetTarget::Workdir, ref_base, cdn, true));
    }

    // command + commands → 合并为 Vec<CommandReg>
    let mut has_command = false;

    if let Some(cmd) = &def.command {
        // bin：$this/$venv token → 绝对路径；python/node：注入 plugin_key 前缀
        let entry = resolve_command_entry(key, &cmd.entry, &cmd.cmd_type, layout);
        commands.push(CommandReg {
            name: key.to_string(),
            cmd_type: cmd.cmd_type.clone(),
            entry,
            desc: cmd.desc.clone(),
            args: cmd.args.clone(),
        });
        has_command = true;
    }

    if let Some(cs) = &def.commands {
        for (name, cd) in cs {
            if has_command && name == key {
                eprintln!(
                    "{} command name conflict: \"{}\" is defined in both 'command' and 'commands'",
                    "Error:".red(),
                    key,
                );
                exit(1);
            }
            let entry = resolve_command_entry(key, &cd.entry, &cd.cmd_type, layout);
            commands.push(CommandReg {
                name: name.clone(),
                cmd_type: cd.cmd_type.clone(),
                entry,
                desc: cd.desc.clone(),
                args: cd.args.clone(),
            });
        }
    }

    // 校验：至少有一个操作
    if assets.is_empty()
        && commands.is_empty()
        && def.pip.as_ref().is_none_or(|p| p.is_empty())
        && def.pip_keep.as_ref().is_none_or(|p| p.is_empty())
    {
        eprintln!(
            "{} plugin \"{}\" has no supported operations (pip/pip-keep/download/command/commands)",
            "Error:".red(),
            key,
        );
        exit(1);
    }

    InstallPlan {
        plugin: ResolvedPlugin {
            key: key.to_string(),
            source: source_label,
            pip_packages: def.pip.clone().unwrap_or_default(),
            pip_keep_packages: def.pip_keep.clone().unwrap_or_default(),
            assets,
            commands,
        },
    }
}

/// 为 python / node 类型命令的 entry 注入 `{plugin_key}/` 前缀。
/// python-m 等类型保持原样；bin 类型走 token 解析（resolve_command_entry）。
fn inject_plugin_prefix(plugin_key: &str, entry: &str, cmd_type: &str) -> String {
    match cmd_type {
        "python" | "node" => format!("{}/{}", plugin_key, entry),
        _ => entry.to_string(),
    }
}

// ---------------------------------------------------------------------------
// bin 命令 entry 解析（$this / $venv token）
// ---------------------------------------------------------------------------

/// bin 类型 entry 的根 token。
const THIS_TOKEN: &str = "$this";
const VENV_TOKEN: &str = "$venv";

/// 解析命令 entry 为执行期消费的最终形态：
/// - `bin`：`$this` / `$venv` token → 绝对路径（目录或文件形态）
/// - `python` / `node`：注入 `{plugin_key}/` 前缀（相对 plugins/ 根）
/// - 其余类型（python-m 等）：原样保留
fn resolve_command_entry(
    plugin_key: &str,
    entry: &str,
    cmd_type: &str,
    layout: &crate::core::paths::PathLayout,
) -> String {
    if cmd_type == "bin" {
        let plugin_dir = layout.plugins_dir.join(plugin_key);
        let venv_bin = layout.venv_dir.join(VENV_BIN);
        match resolve_bin_entry(entry, &plugin_dir, &venv_bin) {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(e) => {
                eprintln!(
                    "{} invalid bin entry for plugin \"{}\": {}",
                    "Error:".red(),
                    plugin_key,
                    e
                );
                exit(1);
            }
        }
    } else {
        inject_plugin_prefix(plugin_key, entry, cmd_type)
    }
}

/// 解析 bin 命令 entry：`$this` → 插件目录，`$venv` → venv bin 目录。
///
/// 合法形态：`$this`、`$venv`（裸目录）或 `$this/<子路径>`、`$venv/<子路径>`。
/// 拒绝裸相对路径、绝对路径、`~`、`..` 越界。
fn resolve_bin_entry(entry: &str, plugin_dir: &Path, venv_bin: &Path) -> Result<PathBuf, String> {
    let (root, rest) = if let Some(rest) = entry.strip_prefix(THIS_TOKEN) {
        (plugin_dir, rest)
    } else if let Some(rest) = entry.strip_prefix(VENV_TOKEN) {
        (venv_bin, rest)
    } else {
        return Err(format!(
            "entry must start with `$this` or `$venv` (e.g. \"$this/bin/my-tool\"), got: \"{entry}\""
        ));
    };

    // token 后必须是结尾或 `/`：拒绝 `$thisfoo` 之类的误写
    if !rest.is_empty() && !rest.starts_with('/') {
        return Err(format!(
            "invalid entry \"{entry}\": expected `$this`, `$venv`, `$this/<path>` or `$venv/<path>`"
        ));
    }

    let sub = rest.strip_prefix('/').unwrap_or(rest);
    if sub.is_empty() {
        // 裸 token：目录形态指向根本身
        return Ok(root.to_path_buf());
    }

    // 子路径复用相对路径校验（拒绝绝对路径 / ~ / ..）
    let sub = validate_relative_path(sub)?;
    Ok(root.join(sub))
}

/// 插件是否需要 Python venv：声明了 pip / pip-keep 依赖，
/// 或注册了 python / python-m 类型命令。
///
/// bin / node 类型与 Python 生态无关，不触发 venv 创建。
fn plugin_needs_venv(def: &protocol::PluginDef) -> bool {
    def.pip.as_ref().is_some_and(|p| !p.is_empty())
        || def.pip_keep.as_ref().is_some_and(|p| !p.is_empty())
        || def
            .command
            .as_ref()
            .is_some_and(|c| matches!(c.cmd_type.as_str(), "python" | "python-m"))
        || def.commands.as_ref().is_some_and(|cs| {
            cs.values()
                .any(|c| matches!(c.cmd_type.as_str(), "python" | "python-m"))
        })
}

// ---------------------------------------------------------------------------
// 阶段 3：执行 InstallPlan（所有判断已在阶段 2 完成，此处纯流水线）
// ---------------------------------------------------------------------------

/// 执行安装计划：pip → 下载/解压 → 注册命令。
fn execute_install_plan(
    plan: &InstallPlan,
    layout: &crate::core::paths::PathLayout,
    cmd_state: &mut CmdState,
) {
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let plugin_dir = layout.plugins_dir.join(&plan.plugin.key);

    // pip-keep（属于本插件，但卸载时保留）
    if !plan.plugin.pip_keep_packages.is_empty() {
        println!("{} pip-keep {}", "==>".cyan().bold(), "(keep)".dimmed());
        for pkg in &plan.plugin.pip_keep_packages {
            println!(
                "{}",
                format!("Installing pip package: {} (keep)", pkg).dimmed()
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

    // 统一下载/解压
    if !plan.plugin.assets.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut section_printed: HashMap<&str, bool> = HashMap::new();

        for asset in &plan.plugin.assets {
            let parent_dir = match asset.target {
                AssetTarget::PluginDir => plugin_dir.clone(),
                AssetTarget::Workdir => cwd.clone(),
            };

            let section_label = match asset.target {
                AssetTarget::PluginDir => "plugins",
                AssetTarget::Workdir => "workdir",
            };

            if !section_printed.contains_key(section_label) {
                let _ = std::fs::create_dir_all(&parent_dir);
                println!("{} {}", "==>".cyan().bold(), section_label);
                println!("  {}", format!("→ {}", parent_dir.display()).dimmed());
                section_printed.insert(section_label, true);
            }

            let temp_dir = layout.cache_dir.join("tmp-dl");
            let _ = std::fs::create_dir_all(&temp_dir);
            let temp_file = temp_dir.join("dl");

            match &asset.src {
                ResolvedSrc::Url(url) => {
                    println!("{}", format!("Downloading {}", asset.name).dimmed());
                    println!("  {}", format!("from {}", url).dimmed());
                    if let Err(e) = download_script(url, &temp_file) {
                        eprintln!("{} {}", "Error:".red(), e);
                        exit(1);
                    }
                }
                ResolvedSrc::LocalPath(path) => {
                    println!("{}", format!("Copying {} from {}", asset.name, path.display()).dimmed());
                    if let Err(e) = std::fs::copy(path, &temp_file) {
                        eprintln!("{} failed to copy from {}: {}", "Error:".red(), path.display(), e);
                        exit(1);
                    }
                }
            }

            if asset.is_archive {
                let needs_peek = matches!(asset.target, AssetTarget::Workdir);

                let (dest, peek_count) = if needs_peek {
                    let n = match peek_archive(&temp_file) {
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("{} {}", "Error:".red(), e);
                            exit(1);
                        }
                    };
                    if n == 1 {
                        (parent_dir.clone(), 1)
                    } else {
                        let d = parent_dir.join(&asset.name);
                        let _ = std::fs::create_dir_all(&d);
                        (d, n)
                    }
                } else {
                    (parent_dir.clone(), 0)
                };

                println!("{}", format!("Extracting to {}", dest.display()).dimmed());
                if let Err(e) = extract_archive(&temp_file, &dest) {
                    eprintln!("{} {}", "Error:".red(), e);
                    exit(1);
                }

                if needs_peek && peek_count > 1 {
                    let file_count = count_files(&dest);
                    println!("{}", format!("{} files extracted", file_count).dimmed());
                }
            } else {
                let dest = parent_dir.join(&asset.name);
                if let Some(d) = dest.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                println!("{}", format!("Saving to {}", dest.display()).dimmed());
                if let Err(e) = std::fs::copy(&temp_file, &dest) {
                    eprintln!("{} failed to save {}: {}", "Error:".red(), dest.display(), e);
                    exit(1);
                }
            }

            let _ = std::fs::remove_file(&temp_file);
            let _ = std::fs::remove_dir(&temp_dir);

            // chmod +x
            if asset.chmod_x {
                #[cfg(not(windows))]
                {
                    use std::os::unix::fs::PermissionsExt;

                    if !asset.is_archive {
                        let target_path = parent_dir.join(&asset.name);
                        if let Ok(metadata) = std::fs::metadata(&target_path) {
                            let mut perms = metadata.permissions();
                            perms.set_mode(0o755);
                            if let Err(e) = std::fs::set_permissions(&target_path, perms) {
                                eprintln!("{} failed to set executable permission on {}: {}", "Error:".red(), target_path.display(), e);
                                exit(1);
                            }
                        }
                        println!("{}", "chmod +x".dimmed());
                    }
                }
                #[cfg(windows)]
                {
                    // Windows 不需要 chmod
                }
            }

            println!("{} {}", "+".green(), asset.name.bold());
        }
    }

    // 命令注册
    if !plan.plugin.commands.is_empty() {
        println!("{} commands", "==>".cyan().bold());
        for cmd in &plan.plugin.commands {
            if cmd.cmd_type == "bin" {
                // entry 已在 build 期解析为绝对路径（token 已替换）：
                // 目录形态查 <目录>/<命令名>，文件形态查文件本身。
                // 此时 pip 已安装、downloads 已就位，可以 fail-fast。
                let entry_path = Path::new(&cmd.entry);
                let exe = if entry_path.is_dir() {
                    entry_path.join(&cmd.name)
                } else {
                    entry_path.to_path_buf()
                };
                if !exe.is_file() {
                    eprintln!(
                        "{} bin entry not found for command '{}': {}",
                        "Error:".red(),
                        cmd.name,
                        exe.display()
                    );
                    eprintln!("  (resolved from entry: {})", entry_path.display());
                    exit(1);
                }
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
                    args: cmd.args.clone(),
                },
            );
        }
    }
}

/// 判断插件是否仅包含 download-to-workdir（无命令、无 pip、无其他下载目标）。
/// 此类插件仅将文件下载到工作目录，无需在 plugins.pkg.json 中记录安装状态。
fn is_workdir_only(plan: &InstallPlan) -> bool {
    plan.plugin.commands.is_empty()
        && plan.plugin.pip_packages.is_empty()
        && plan.plugin.pip_keep_packages.is_empty()
        && !plan.plugin.assets.is_empty()
        && plan
            .plugin
            .assets
            .iter()
            .all(|a| matches!(a.target, AssetTarget::Workdir))
}

/// 从 InstallPlan 构建 PkgEntry（用于持久化到 plugins.pkg.json）。
fn build_pkg_entry(plan: &InstallPlan) -> PkgEntry {
    let assets: Vec<String> = plan
        .plugin
        .assets
        .iter()
        .filter(|a| !matches!(a.target, AssetTarget::PluginDir))
        .map(|a| a.name.clone())
        .collect();

    let commands: Vec<String> = plan.plugin.commands.iter().map(|c| c.name.clone()).collect();
    let pip = if plan.plugin.pip_packages.is_empty() {
        None
    } else {
        Some(plan.plugin.pip_packages.clone())
    };
    let pip_keep = if plan.plugin.pip_keep_packages.is_empty() {
        None
    } else {
        Some(plan.plugin.pip_keep_packages.clone())
    };

    PkgEntry {
        source: plan.plugin.source.clone(),
        pip,
        pip_keep,
        assets,
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
pub(crate) fn install_python_package(pkg: &str, layout: &crate::core::paths::PathLayout) {
    if is_uv_mode(layout) {
        if !tool_exists("uv") {
            eprintln!(
                "{} this environment is managed by uv, but uv was not found.",
                "Error:".red(),
            );
            eprintln!(
                "  Reinstall uv, or delete ~/.byk/py-venv/ to let byk rebuild it with {}.",
                default_python_name()
            );
            exit(1);
        }
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

// ---------------------------------------------------------------------------
// Python venv 初始化
// ---------------------------------------------------------------------------

/// venv 创建工具。
///
/// 优先级：uv 优于系统 Python——uv 不依赖系统 Python（可自行管理
/// 解释器），venv 创建和包安装也都更快。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PyTool {
    /// uv（`uv venv` + `uv init`）
    Uv,
    /// 系统 Python（具体的解释器命令，如 python3 / python / py）
    Python(&'static str),
}

impl PyTool {
    /// 提示文案中展示的工具名。
    fn label(&self) -> &'static str {
        match self {
            PyTool::Uv => "uv",
            PyTool::Python(exe) => exe,
        }
    }
}

/// 根据探测结果选择 venv 创建工具（纯函数，便于测试）。
fn choose_py_tool(has_uv: bool, has_python: bool) -> Option<PyTool> {
    if has_uv {
        Some(PyTool::Uv)
    } else if has_python {
        Some(PyTool::Python(default_python_name()))
    } else {
        None
    }
}

/// 探测 PATH 中可用的 venv 创建工具。
fn detect_py_tool() -> Option<PyTool> {
    let has_python = python_candidates()
        .iter()
        .any(|name| tool_exists(name));
    choose_py_tool(tool_exists("uv"), has_python)
}

/// 系统 Python 的候选可执行名（按优先级）。
fn python_candidates() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec!["python", "py"]
    }
    #[cfg(not(windows))]
    {
        vec!["python3", "python"]
    }
}

/// 默认系统 Python 可执行名（提示文案与 venv 创建共用）。
fn default_python_name() -> &'static str {
    python_candidates()[0]
}

/// 无 Python 环境时提示文案中的解释器名。
fn python_hint() -> &'static str {
    default_python_name()
}

/// 探测可执行文件是否在 PATH 中且可运行。
fn tool_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// 按平台打印 Python / uv 的安装指引。
fn print_python_install_hints() {
    #[cfg(target_os = "macos")]
    {
        eprintln!("  {}", "brew install uv                 # recommended".dimmed());
        eprintln!("  {}", "brew install python".dimmed());
    }
    #[cfg(target_os = "linux")]
    {
        eprintln!("  {}", "curl -LsSf https://astral.sh/uv/install.sh | sh".dimmed());
        eprintln!("  {}", "sudo apt install python3-venv    # Debian/Ubuntu".dimmed());
    }
    #[cfg(target_os = "windows")]
    {
        eprintln!("  {}", "winget install astral-sh.uv".dimmed());
        eprintln!("  {}", "winget install Python.Python.3.13".dimmed());
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        eprintln!("  {}", "https://docs.astral.sh/uv/".dimmed());
        eprintln!("  {}", "https://www.python.org/downloads/".dimmed());
    }
}

/// 按平台打印 Node.js 的安装指引。
fn print_node_install_hints() {
    #[cfg(target_os = "macos")]
    {
        eprintln!("  {}", "brew install node".dimmed());
    }
    #[cfg(target_os = "linux")]
    {
        eprintln!("  {}", "sudo apt install nodejs npm    # Debian/Ubuntu".dimmed());
    }
    #[cfg(target_os = "windows")]
    {
        eprintln!("  {}", "winget install OpenJS.NodeJS".dimmed());
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        eprintln!("  {}", "https://nodejs.org/en/download".dimmed());
    }
}

/// venv 是否已就绪（pip 或 python 可执行文件存在即可；
/// uv 创建的 venv 不含 pip，因此二者满足其一）。
fn venv_is_ready(layout: &crate::core::paths::PathLayout) -> bool {
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
    let py_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    pip.is_file() || py_exe.is_file()
}

/// 确保 Python venv 可用（不存在则探测工具、一次性确认、创建）。
///
/// venv 已就绪时直接返回。`subject` 为触发原因的描述，
/// 如 `plugin "hello"` / `byk add pip`。
///
/// 用户不需要理解“虚拟环境管理器选型”这个概念——
/// 探测到 uv 就用 uv（快、不依赖系统 Python），否则用系统 Python。
pub(crate) fn ensure_python_env(layout: &crate::core::paths::PathLayout, subject: &str) {
    if venv_is_ready(layout) {
        return;
    }
    match detect_py_tool() {
        Some(tool) => {
            println!();
            println!(
                "{} {} needs a Python virtual environment.",
                "!".yellow(),
                subject.bold()
            );
            println!(
                "  byk will create {} using {} and install its dependencies.",
                "~/.byk/py-venv".dimmed(),
                tool.label().bold()
            );
            println!();
            print!("Continue? [Y/n] ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                eprintln!();
                eprintln!(
                    "{} cancelled: cannot read confirmation from stdin.",
                    "Error:".red()
                );
                exit(1);
            }
            let answer = input.trim().to_lowercase();
            if answer == "n" || answer == "no" {
                println!("{}", "Cancelled.".dimmed());
                exit(1);
            }
            init_py(layout, tool);
            if !venv_is_ready(layout) {
                exit(1);
            }
        }
        None => {
            eprintln!(
                "{} {} needs a Python environment, but neither {} nor {} was found.",
                "Error:".red(),
                subject,
                "uv".bold(),
                python_hint().bold(),
            );
            eprintln!();
            eprintln!("Install one of them, then retry:");
            print_python_install_hints();
            exit(1);
        }
    }
}

/// 初始化 Python 虚拟环境（插件安装时按需调用）。
///
/// 创建 ~/.byk/py-venv/.venv/（不存在时）并合并写入状态文件（保留已注册的命令）。
/// uv 模式额外创建 pyproject.toml。
/// 刻意不注入 pi/pu/pl 别名：用户只使用插件命令，
/// 包管理是 byk 的内部事务。
fn init_py(layout: &crate::core::paths::PathLayout, tool: PyTool) {
    let venv_dir = &layout.venv_dir;
    let py_venv_dir = &layout.py_venv_dir;
    let pyproject_toml = py_venv_dir.join("pyproject.toml");

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
        let status = match tool {
            PyTool::Uv => Command::new("uv")
                .args(["venv", &venv_dir.to_string_lossy()])
                .status(),
            PyTool::Python(exe) => Command::new(exe)
                .args(["-m", "venv", &venv_dir.to_string_lossy()])
                .status(),
        };
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
                #[cfg(target_os = "linux")]
                if matches!(tool, PyTool::Python(_)) {
                    eprintln!(
                        "  {}",
                        "hint: Debian/Ubuntu may need: sudo apt install python3-venv".dimmed()
                    );
                }
                return;
            }
            Err(e) => {
                eprintln!("{} Failed to create venv: {}", "Error:".red(), e);
                return;
            }
        }
    }

    // ② uv 模式：创建 pyproject.toml
    if matches!(tool, PyTool::Uv) && !pyproject_toml.is_file() {
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

    // ③ 合并写入状态：保留已注册的命令与包条目，仅补 python_executable。
    //    plugins.cmd.json 可能已有命令（如 node 类型插件在 venv 创建前
    //    注册的命令），覆盖写会丢数据。
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let cmd_existed = cmd_file.is_file();
    let pkg_existed = pkg_file.is_file();

    let python_exe = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    merge_py_state(layout, &python_exe);

    println!(
        "  {} plugins/plugins.cmd.json {}",
        "+".green(),
        if cmd_existed { "(updated)".dimmed() } else { "(created)".dimmed() }
    );
    println!(
        "  {} plugins/plugins.pkg.json {}",
        "+".green(),
        if pkg_existed { "(updated)".dimmed() } else { "(created)".dimmed() }
    );

    println!();
    println!(
        "{} {}",
        "Python environment ready.".green(),
        format!("({})", tool.label()).dimmed()
    );
}

/// 合并写入插件状态（init_py 收尾）。
///
/// plugins.cmd.json / plugins.pkg.json 可能已有内容——node 类型插件
/// 可以在 venv 创建前就注册命令（npm 插件正是如此），直接覆盖写会丢数据。
/// 因此读-改-写：保留 commands 与包条目，仅在缺失时补 python_executable。
fn merge_py_state(layout: &crate::core::paths::PathLayout, python_exe: &Path) {
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");

    let mut cmd_state = load_plugin_state(&layout.plugins_dir, &layout.venv_dir);
    if cmd_state.python_executable.is_none() {
        cmd_state.python_executable = Some(python_exe.to_string_lossy().to_string());
    }
    let pkg_state = load_pkg_state(&layout.plugins_dir);

    json_io::write_json(&cmd_file, &cmd_state);
    json_io::write_json(&pkg_file, &pkg_state);
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 创建目录，打印操作信息。
pub(crate) fn ensure_dir(path: &Path, label: &str) {
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

    // ==================== choose_py_tool ====================

    #[test]
    fn choose_py_tool_prefers_uv() {
        // uv 优先：它不依赖系统 Python，venv 创建与包安装都更快
        assert_eq!(choose_py_tool(true, false), Some(PyTool::Uv));
        assert_eq!(choose_py_tool(true, true), Some(PyTool::Uv));
    }

    #[test]
    fn choose_py_tool_falls_back_to_system_python() {
        assert_eq!(
            choose_py_tool(false, true),
            Some(PyTool::Python(default_python_name()))
        );
    }

    #[test]
    fn choose_py_tool_none_when_nothing_available() {
        assert_eq!(choose_py_tool(false, false), None);
    }

    // ==================== inject_plugin_prefix ====================

    #[test]
    fn inject_prefix_for_plugin_dir_types() {
        // python / node 的 entry 相对于插件目录，需注入 {plugin_key}/ 前缀
        assert_eq!(inject_plugin_prefix("mykey", "main.py", "python"), "mykey/main.py");
        assert_eq!(inject_plugin_prefix("mykey", "main.js", "node"), "mykey/main.js");
    }

    #[test]
    fn inject_prefix_keeps_other_types_bare() {
        // python-m 的 entry 是模块路径，保持原样
        assert_eq!(inject_plugin_prefix("mykey", "mymod", "python-m"), "mymod");
    }

    // ==================== resolve_bin_entry ====================

    #[test]
    fn resolve_bin_entry_this_token() {
        let plugin_dir = Path::new("/home/u/.byk/plugins/my-tool");
        let venv_bin = Path::new("/home/u/.byk/py-venv/.venv/bin");

        // 裸 $this → 插件目录（目录形态，按命令名查找）
        assert_eq!(
            resolve_bin_entry("$this", plugin_dir, venv_bin).unwrap(),
            plugin_dir
        );
        // $this/<子路径> → 插件目录下的文件
        assert_eq!(
            resolve_bin_entry("$this/bin/my-tool", plugin_dir, venv_bin).unwrap(),
            plugin_dir.join("bin/my-tool")
        );
    }

    #[test]
    fn resolve_bin_entry_venv_token() {
        let plugin_dir = Path::new("/home/u/.byk/plugins/my-tool");
        let venv_bin = Path::new("/home/u/.byk/py-venv/.venv/bin");

        // 裸 $venv → venv bin 目录（目录形态，按命令名查找）
        assert_eq!(
            resolve_bin_entry("$venv", plugin_dir, venv_bin).unwrap(),
            venv_bin
        );
        // $venv/<名字> → venv bin 下的文件
        assert_eq!(
            resolve_bin_entry("$venv/hello", plugin_dir, venv_bin).unwrap(),
            venv_bin.join("hello")
        );
    }

    #[test]
    fn resolve_bin_entry_rejects_invalid_forms() {
        let plugin_dir = Path::new("/p");
        let venv_bin = Path::new("/v");

        // 裸相对路径 / 绝对路径 / ~ 均不接受
        assert!(resolve_bin_entry("bin/my-tool", plugin_dir, venv_bin).is_err());
        assert!(resolve_bin_entry("/usr/bin/tool", plugin_dir, venv_bin).is_err());
        assert!(resolve_bin_entry("~/bin/tool", plugin_dir, venv_bin).is_err());
        // .. 越界
        assert!(resolve_bin_entry("$this/../evil", plugin_dir, venv_bin).is_err());
        assert!(resolve_bin_entry("$venv/../evil", plugin_dir, venv_bin).is_err());
        // token 后必须是结尾或 /
        assert!(resolve_bin_entry("$thisx", plugin_dir, venv_bin).is_err());
        assert!(resolve_bin_entry("$venvx/y", plugin_dir, venv_bin).is_err());
        // 空 entry
        assert!(resolve_bin_entry("", plugin_dir, venv_bin).is_err());
    }

    // ==================== plugin_needs_venv ====================

    fn plugin_def(json: &str) -> protocol::PluginDef {
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(json).unwrap();
        let registry = protocol::parse_registry(&raw);
        registry.plugins.get("p").cloned().unwrap()
    }

    #[test]
    fn plugin_needs_venv_by_pip_deps() {
        assert!(plugin_needs_venv(&plugin_def(r#"{"p": {"pip": ["requests"]}}"#)));
        assert!(plugin_needs_venv(&plugin_def(r#"{"p": {"pip-keep": ["requests"]}}"#)));
    }

    #[test]
    fn plugin_needs_venv_by_python_command_types() {
        // python / python-m 命令类型同样需要 venv（修复此前仅看 pip 字段的缺口）
        assert!(plugin_needs_venv(&plugin_def(
            r#"{"p": {"command": {"type": "python", "entry": "main.py"}}}"#
        )));
        assert!(plugin_needs_venv(&plugin_def(
            r#"{"p": {"commands": {"x": {"type": "python-m", "entry": "m.cli"}}}}"#
        )));
    }

    #[test]
    fn plugin_needs_venv_false_for_bin_and_node() {
        // bin / node 与 Python 无关；pip-bin 已移除
        assert!(!plugin_needs_venv(&plugin_def(
            r#"{"p": {"command": {"type": "bin", "entry": "$this/bin/tool"}}}"#
        )));
        assert!(!plugin_needs_venv(&plugin_def(
            r#"{"p": {"command": {"type": "node", "entry": "main.js"}}}"#
        )));
        assert!(!plugin_needs_venv(&plugin_def(
            r#"{"p": {"downloads": "https://example.com/x"}}"#
        )));
    }

    // ==================== merge_py_state ====================

    #[test]
    fn merge_py_state_preserves_existing_commands() {
        // 回归测试：venv 创建前 plugins.cmd.json 可能已有命令
        // （node 类型插件在运行时注册），合并写必须保留 commands 与包条目
        let layout = crate::core::paths::PathLayout::with_name("fcbyk_test_merge_py_state");
        let _ = std::fs::remove_dir_all(&layout.root_dir);
        std::fs::create_dir_all(&layout.plugins_dir).unwrap();

        let mut commands = HashMap::new();
        commands.insert(
            "ni".to_string(),
            PluginCommand {
                cmd_type: "node".to_string(),
                entry: "npm/npm.js".to_string(),
                desc: "from @antfu/ni".to_string(),
                args: None,
            },
        );
        json_io::write_json(
            &layout.plugins_dir.join("plugins.cmd.json"),
            &CmdState {
                commands,
                python_executable: None,
            },
        );

        let mut pkg_state: PkgState = HashMap::new();
        pkg_state.insert(
            "npm".to_string(),
            PkgEntry {
                source: None,
                pip: None,
                pip_keep: None,
                assets: vec![],
                commands: vec!["ni".to_string()],
            },
        );
        json_io::write_json(&layout.plugins_dir.join("plugins.pkg.json"), &pkg_state);

        let python_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        merge_py_state(&layout, &python_exe);

        // 命令保留，python_executable 补上
        let expected = python_exe.to_string_lossy().to_string();
        let loaded: CmdState =
            json_io::read_json(&layout.plugins_dir.join("plugins.cmd.json")).unwrap();
        assert!(loaded.commands.contains_key("ni"));
        assert_eq!(loaded.commands.get("ni").unwrap().cmd_type, "node");
        assert_eq!(loaded.python_executable.as_deref(), Some(expected.as_str()));

        // 包条目保留
        let loaded_pkg: PkgState =
            json_io::read_json(&layout.plugins_dir.join("plugins.pkg.json")).unwrap();
        assert_eq!(loaded_pkg.get("npm").unwrap().commands, vec!["ni".to_string()]);

        let _ = std::fs::remove_dir_all(&layout.root_dir);
    }

    #[test]
    fn merge_py_state_keeps_existing_python_executable() {
        // 已有的 python_executable 不被覆盖
        let layout = crate::core::paths::PathLayout::with_name("fcbyk_test_merge_py_state2");
        let _ = std::fs::remove_dir_all(&layout.root_dir);
        std::fs::create_dir_all(&layout.plugins_dir).unwrap();

        json_io::write_json(
            &layout.plugins_dir.join("plugins.cmd.json"),
            &CmdState {
                commands: HashMap::new(),
                python_executable: Some("/custom/python".to_string()),
            },
        );

        let python_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
        merge_py_state(&layout, &python_exe);

        let loaded: CmdState =
            json_io::read_json(&layout.plugins_dir.join("plugins.cmd.json")).unwrap();
        assert_eq!(loaded.python_executable.as_deref(), Some("/custom/python"));

        let _ = std::fs::remove_dir_all(&layout.root_dir);
    }

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
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        assert!(result.contains_key("key1"));
        assert!(!result.contains_key("$var"));
    }

    #[test]
    fn preprocess_with_var_replacement() {
        let body = r#"{"$var": {"PKG": "requests"}, "plugin": {"pip": ["{PKG}"]}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        let plugin = result.get("plugin").unwrap();
        let pip_list = plugin.get("pip").unwrap().as_array().unwrap();
        assert_eq!(pip_list[0].as_str().unwrap(), "requests");
    }

    #[test]
    fn preprocess_multiple_vars() {
        let body = r#"{"$var": {"A": "a_val", "B": "b_val"}, "x": {"entry": "{A}-{B}"}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        let x = result.get("x").unwrap();
        assert_eq!(x.get("entry").unwrap().as_str().unwrap(), "a_val-b_val");
    }

    #[test]
    fn preprocess_var_not_string_skipped() {
        let body = r#"{"$var": {"N": 42}, "x": {"entry": "{N}"}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        let x = result.get("x").unwrap();
        // {N} not replaced because 42 is not a string
        assert_eq!(x.get("entry").unwrap().as_str().unwrap(), "{N}");
    }

    #[test]
    fn preprocess_platform_var_resolved() {
        let body = r#"{"$var": {"URL": {"linux-x86_64": "https://linux.example.com", "darwin-arm64": "https://darwin.example.com"}}, "plugin": {"downloads": {"tool": "{URL}"}}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        let plugin = result.get("plugin").unwrap();
        let downloads = plugin.get("downloads").unwrap();
        let url = downloads.get("tool").unwrap().as_str().unwrap();
        assert_eq!(url, "https://linux.example.com");
    }

    #[test]
    fn preprocess_platform_var_missing_platform() {
        let body = r#"{"$var": {"URL": {"darwin-arm64": "https://darwin.example.com"}}, "plugin": {"entry": "{URL}"}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        let plugin = result.get("plugin").unwrap();
        // {URL} not replaced because linux-x86_64 is not in the platform map
        assert_eq!(plugin.get("entry").unwrap().as_str().unwrap(), "{URL}");
    }

    #[test]
    fn preprocess_platform_var_in_string_interpolation() {
        let body = r#"{"$var": {"BASE": {"linux-x86_64": "https://cdn.linux.example.com", "darwin-arm64": "https://cdn.darwin.example.com"}}, "plugin": {"downloads": {"tool": "{BASE}/tool", "config": "{BASE}/config.json"}}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        let plugin = result.get("plugin").unwrap();
        let downloads = plugin.get("downloads").unwrap();
        assert_eq!(downloads.get("tool").unwrap().as_str().unwrap(), "https://cdn.linux.example.com/tool");
        assert_eq!(downloads.get("config").unwrap().as_str().unwrap(), "https://cdn.linux.example.com/config.json");
    }

    #[test]
    fn preprocess_platform_var_to_string_in_result() {
        let body = r#"{"$var": {"URL": {"linux-x86_64": "https://linux.example.com", "darwin-arm64": "https://darwin.example.com"}}, "plugin": {"entry": "{URL}"}}"#;
        let result = preprocess_registry(body, "linux-x86_64").unwrap();
        // $var 中的平台对象被替换为解析后的字符串
        let var_map = result.get("$var").unwrap().as_object().unwrap();
        assert_eq!(var_map.get("URL").unwrap().as_str().unwrap(), "https://linux.example.com");
    }

    #[test]
    fn preprocess_invalid_json() {
        let body = "not json";
        assert!(preprocess_registry(body, "linux-x86_64").is_err());
    }

    #[test]
    fn preprocess_var_not_map() {
        let body = r#"{"$var": "not_a_map"}"#;
        assert!(preprocess_registry(body, "linux-x86_64").is_err());
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