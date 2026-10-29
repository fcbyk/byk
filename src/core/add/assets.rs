//! `$assets` 解析器：把别名文件的资产声明解析为可执行计划。
//!
//! 由 `byk add` 使用（下载 / 解压 / chmod）。
//!
//! 目标规则：
//! - `$to` 显式设置 → root = 解析值，仅允许 `$byk` / `$workdir` / `$downloads` 开头；
//!   条目直接落 `<root>/<名字>`；
//! - `$to` 缺省 → root = `$downloads`（系统下载目录），条目平铺到该目录；
//! - 不再使用 `assets/` 容器目录，`$to` 怎么写就怎么下。
//!
//! 安全校验（双层）：
//! - L1：`$to` 原始字符串必须以 `$byk` / `$workdir` / `$downloads` 开头；
//! - L2：展开后 canonicalize 最长已存在前缀，确保未逃逸到基准目录外。
//!
//! 条目规则：名字即 root 下的相对路径（拒绝 `..` / 绝对路径）；
//! 源为 URL 或本地路径（相对文件目录解析）；`[tar]` 解压到同名子目录、
//! `[exe]` 下载后 `chmod +x`；条目值可为平台映射，当前平台不在映射 → 跳过。

use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde_json::Value;

use crate::core::aliases::variables::{VarContext, expand_variables};
use crate::core::paths::PathLayout;
use super::download;
use super::pip::{PYTHON_BIN, VENV_BIN};

/// 当前平台（编译期注入，与 `build.rs` 五值一致）。
const PLATFORM: &str = env!("PLATFORM");

/// `$to` 允许的变量前缀。
const ALLOWED_TO_PREFIXES: &[&str] = &["$byk", "$workdir", "$downloads"];

/// 解析上下文：一个别名文件 + 运行环境。
pub(crate) struct AssetsCtx<'a> {
    /// 别名文件路径（相对路径的解析基准）
    pub(crate) file_path: &'a Path,
    /// 当前工作目录（`$workdir`）
    pub(crate) cwd: &'a Path,
    /// byk 根目录（`$byk`）
    pub(crate) byk_root: &'a Path,
    /// venv 目录（`$venv`/`$python` 展开用）
    pub(crate) venv_dir: &'a Path,
    /// 系统下载目录（`$downloads`）
    pub(crate) downloads_dir: &'a Path,
    /// 远程文件的 base URL（远程分发时传入，相对路径据此解析成相对 URL；
    /// 本地文件为 None，相对路径按本地文件目录解析）
    pub(crate) remote_base: Option<&'a str>,
}

/// 资产来源。
#[derive(Debug, Clone, PartialEq)]
pub enum AssetSource {
    Url(String),
    LocalPath(PathBuf),
}

/// 下载后动作。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssetAction {
    /// 原样下载
    None,
    /// `chmod +x`（非 Windows）
    Exec,
    /// 解压到同名子目录
    Tar,
}

/// 单个资产条目。
#[derive(Debug, Clone)]
pub struct AssetEntry {
    /// root 下的相对路径（如 `bin/lint`）
    pub name: String,
    pub source: AssetSource,
    pub action: AssetAction,
}

/// 解析后的资产计划。
#[derive(Debug, Clone)]
pub struct AssetsPlan {
    /// 生效根目录（`$to` 或缺省根）
    pub root: PathBuf,
    /// 适用于当前平台的条目
    pub entries: Vec<AssetEntry>,
    /// 平台不适用被跳过的条目名
    pub skipped: Vec<String>,
}

impl AssetsPlan {
    /// 条目实际所在目录——`$assets` 变量的值。
    pub fn assets_dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// 条目目标路径。
    pub fn entry_dest(&self, name: &str) -> PathBuf {
        self.assets_dir().join(name)
    }
}

/// 校验条目名：root 下的相对路径，拒绝绝对路径与 `..` 逃逸。
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("asset name cannot be empty".into());
    }
    if name.starts_with('/') || name.starts_with('\\') || Path::new(name).is_absolute() {
        return Err(format!("asset name must be a relative path: {}", name));
    }
    for comp in name.split(['/', '\\']) {
        if comp == ".." {
            return Err(format!("asset name cannot contain '..': {}", name));
        }
    }
    Ok(())
}

/// 条目值 → 当前平台的源字符串。平台映射缺当前平台 → None（跳过）。
fn resolve_entry_value(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(obj) => obj.get(PLATFORM).and_then(|x| x.as_str()).map(String::from),
        _ => None,
    }
}

/// 展开 `~` 为用户主目录。
fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" || p.starts_with("~/") || p.starts_with("~\\") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        if p == "~" {
            return home;
        }
        return home.join(&p[2..]);
    }
    PathBuf::from(p)
}

/// 解析条目源字符串：剥离 `[tar] `/`[exe] ` 前缀，区分 URL / 相对 URL / 本地路径。
///
/// - `http(s)://` → 完整 URL
/// - 相对路径 + `remote_base`（远程文件）→ 解析成相对 URL
/// - 相对路径 + 无 `remote_base`（本地文件）→ 本地路径（相对文件目录）
fn parse_source(
    raw: &str,
    file_dir: &Path,
    remote_base: Option<&str>,
) -> Result<(AssetSource, AssetAction), String> {
    let (raw, action) = if let Some(u) = raw.strip_prefix("[tar] ") {
        (u, AssetAction::Tar)
    } else if let Some(u) = raw.strip_prefix("[exe] ") {
        (u, AssetAction::Exec)
    } else {
        (raw, AssetAction::None)
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty asset source".into());
    }
    let source = if raw.starts_with("http://") || raw.starts_with("https://") {
        AssetSource::Url(raw.to_string())
    } else if let Some(base) = remote_base {
        // 远程文件：相对路径相对 base URL 解析（如仓库内 `bin/tool` → `{base}/bin/tool`）
        AssetSource::Url(resolve_relative_url(base, raw))
    } else {
        let expanded = expand_tilde(raw);
        let p = if expanded.is_absolute() {
            expanded
        } else {
            file_dir.join(expanded)
        };
        AssetSource::LocalPath(p)
    };
    Ok((source, action))
}

/// 相对路径拼接为完整 URL。
/// base_url 应为目录 URL（末尾带 /），rel 直接拼接。
fn resolve_relative_url(base_url: &str, rel: &str) -> String {
    let rel = rel.strip_prefix("./").unwrap_or(rel);
    format!("{}{}", base_url, rel)
}

/// L2 安全校验：判断 path 是否在 base 的子目录内（防符号链接和 `..` 逃逸）。
///
/// 逐级向上找最长已存在前缀，canonicalize 后做 `starts_with` 比对。
/// base 通常为 `$byk` / `$workdir` / `$downloads` 展开后的路径。
fn is_under_dir(path: &Path, base: &Path) -> bool {
    let Ok(base_real) = base.canonicalize() else {
        // base 不存在 → canonicalize 失败，做纯前缀比对 + 防 `..`
        // 这种情况说明 base 尚未创建，我们在下载时会创建它
        return check_no_parent_escape(path, base);
    };

    // 逐级向上找最长已存在前缀
    let mut current = path.to_path_buf();
    loop {
        if let Ok(real) = current.canonicalize() {
            return real.starts_with(&base_real);
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => return false,
        }
    }
}

/// 纯前缀比对 + 防 `..` 逃逸（用于 base 不存在的场景）。
fn check_no_parent_escape(path: &Path, base: &Path) -> bool {
    // 先 normalize 去掉 `..`
    let normalized = normalize_path(path);
    normalized.starts_with(base) && !has_parent_escape(&normalized, base)
}

/// 简单的路径规范化：去掉 `.` 和 `..`，不访问文件系统。
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            c => {
                result.push(c.as_os_str());
            }
        }
    }
    result
}

/// 检查路径组件中是否有相对于 base 的 `..` 逃逸。
fn has_parent_escape(path: &Path, base: &Path) -> bool {
    let base_depth = base.components().count();
    let path_depth = path.components().count();
    // 路径组件数少于 base → 逃逸了
    path_depth < base_depth
}

/// L1 安全校验：`$to` 原始字符串必须以 `$byk` / `$workdir` / `$downloads` 开头。
fn check_to_prefix(raw: &str) -> Result<(), String> {
    for prefix in ALLOWED_TO_PREFIXES {
        if raw == *prefix || raw.starts_with(&format!("{}/", prefix)) || raw.starts_with(&format!("{}\\", prefix)) {
            return Ok(());
        }
    }
    Err(format!(
        "$to must start with $byk, $workdir, or $downloads (got '{}')",
        raw
    ))
}

/// L1+L2 校验：展开后的 `$to` 路径是否在对应基准目录内。
///
/// 根据原始前缀确定基准目录：
/// - `$byk/...`    → 基准 = ctx.byk_root
/// - `$workdir/...`  → 基准 = ctx.cwd
/// - `$downloads/...` → 基准 = ctx.downloads_dir
fn verify_to_within_base(expanded: &Path, raw: &str, ctx: &AssetsCtx) -> Result<(), String> {
    let (prefix_name, base) = if raw == "$byk" || raw.starts_with("$byk/") || raw.starts_with("$byk\\") {
        ("$byk", ctx.byk_root)
    } else if raw == "$workdir" || raw.starts_with("$workdir/") || raw.starts_with("$workdir\\") {
        ("$workdir", ctx.cwd)
    } else if raw == "$downloads" || raw.starts_with("$downloads/") || raw.starts_with("$downloads\\") {
        ("$downloads", ctx.downloads_dir)
    } else {
        return Err(format!("$to must start with $byk, $workdir, or $downloads (got '{}')", raw));
    };

    if !is_under_dir(expanded, base) {
        return Err(format!(
            "$to '{}' expands to '{}' which escapes its base directory '{}' ({})",
            raw,
            expanded.display(),
            prefix_name,
            base.display()
        ));
    }
    Ok(())
}

/// 解析 `$to` → 显式根目录。None 表示未设置（走缺省根）。
fn resolve_explicit_root(block: &Value, ctx: &AssetsCtx) -> Result<Option<PathBuf>, String> {
    let Some(to_val) = block.get("$to") else {
        return Ok(None);
    };
    let raw = match to_val {
        Value::String(s) => s.clone(),
        Value::Object(obj) => match obj.get(PLATFORM).and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                return Err(format!(
                    "$to platform map has no entry for platform '{}'",
                    PLATFORM
                ));
            }
        },
        _ => return Err("$to must be a string or a platform map".into()),
    };

    // $assets 自引用禁用（在展开前检查，给出准确错误）
    if raw.contains("$assets") {
        return Err("$to cannot reference $assets (self-reference)".into());
    }

    // L1：原始字符串必须以合法变量开头
    check_to_prefix(&raw)?;

    // 阶段二：展开内置变量（$byk / $workdir / $downloads）
    let var_ctx = VarContext {
        byk_root: ctx.byk_root.to_path_buf(),
        venv_bin: ctx.venv_dir.join(VENV_BIN),
        venv_python: ctx.venv_dir.join(VENV_BIN).join(PYTHON_BIN),
        downloads_dir: ctx.downloads_dir.to_path_buf(),
        workdir: ctx.cwd.to_path_buf(),
    };
    let expanded = expand_variables(&raw, &var_ctx)
        .map_err(|e| format!("{} (while expanding $to '{}')", e, raw))?;
    let path = PathBuf::from(&expanded);

    // L2：展开后路径不能逃逸到基准目录外
    verify_to_within_base(&path, &raw, ctx)?;

    Ok(Some(path))
}

/// 解析 `$assets` 块为资产计划。
pub(crate) fn resolve_assets_plan(block: &Value, ctx: &AssetsCtx) -> Result<AssetsPlan, String> {
    let Some(obj) = block.as_object() else {
        return Err("$assets must be an object".into());
    };

    let explicit_root = resolve_explicit_root(block, ctx)?;

    // 条目 = 非 $ 前缀 key（$to 等元数据不计入）
    let entry_names: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('$'))
        .cloned()
        .collect();

    // 无 `$to` → root = `$downloads`（系统下载目录），多文件平铺不套容器
    let root = match explicit_root {
        Some(r) => r,
        None => ctx.downloads_dir.to_path_buf(),
    };

    let file_dir = ctx.file_path.parent().unwrap_or(Path::new("."));
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    for name in &entry_names {
        validate_name(name)?;
        let val = &obj[name];
        let Some(raw) = resolve_entry_value(val) else {
            skipped.push(name.clone());
            continue;
        };
        let (source, action) = parse_source(&raw, file_dir, ctx.remote_base)?;
        entries.push(AssetEntry {
            name: name.clone(),
            source,
            action,
        });
    }

    Ok(AssetsPlan {
        root,
        entries,
        skipped,
    })
}

/// 下载 `$assets` 块（解析 plan + 下载循环）。返回是否实际下载了资源。
///
/// 插件安装（`byk add @xxx` / `byk add @@xxx`）共用。
pub(crate) fn download_block(
    block: &Value,
    path: &Path,
    layout: &PathLayout,
) -> bool {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let downloads_dir = crate::core::aliases::variables::system_downloads_dir();
    let ctx = AssetsCtx {
        file_path: path,
        cwd: &cwd,
        byk_root: &layout.root_dir,
        venv_dir: &layout.venv_dir,
        downloads_dir: &downloads_dir,
        remote_base: None,
    };
    let plan = match resolve_assets_plan(block, &ctx) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "{} invalid $assets in {}: {}",
                "Error:".red(),
                path.display(),
                e
            );
            std::process::exit(1);
        }
    };

    if plan.entries.is_empty() {
        let note = if plan.skipped.is_empty() {
            "no entries".to_string()
        } else {
            format!("{} skipped on this platform", plan.skipped.len())
        };
        println!("{} nothing to download ({})", "*".dimmed(), note.dimmed());
        return false;
    }

    println!(
        "{} Downloading {} asset(s) from {}",
        "==>".cyan().bold(),
        plan.entries.len(),
        path.display().to_string().bold()
    );
    println!(
        "  {} {}",
        "target:".dimmed(),
        plan.assets_dir().display().to_string().dimmed()
    );

    download::ensure_dir(&plan.assets_dir(), "assets");

    for entry in &plan.entries {
        let dest = plan.entry_dest(&entry.name);
        if let Some(parent) = dest.parent() {
            download::ensure_dir(parent, "asset");
        }

        let is_tar = entry.action == AssetAction::Tar;
        let fetch_target = if is_tar {
            dest.with_extension("byk-tmp")
        } else {
            dest.clone()
        };

        match &entry.source {
            AssetSource::Url(url) => {
                let url = url.clone();
                if let Err(e) = download::download_script(&url, &fetch_target) {
                    eprintln!(
                        "{} failed to download {}: {}",
                        "Error:".red(),
                        entry.name.bold(),
                        e
                    );
                    std::process::exit(1);
                }
            }
            AssetSource::LocalPath(p) => {
                if !p.is_file() {
                    eprintln!(
                        "{} local source not found for '{}': {}",
                        "Error:".red(),
                        entry.name.bold(),
                        p.display()
                    );
                    std::process::exit(1);
                }
                if let Err(e) = fs::copy(p, &fetch_target) {
                    eprintln!(
                        "{} failed to copy {}: {}",
                        "Error:".red(),
                        entry.name.bold(),
                        e
                    );
                    std::process::exit(1);
                }
            }
        }

        match entry.action {
            AssetAction::Tar => {
                if let Err(e) = download::extract_archive(&fetch_target, &dest) {
                    eprintln!(
                        "{} failed to extract {}: {}",
                        "Error:".red(),
                        entry.name.bold(),
                        e
                    );
                    std::process::exit(1);
                }
                let _ = fs::remove_file(&fetch_target);
            }
            AssetAction::Exec => {
                #[cfg(not(windows))]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = fs::metadata(&dest) {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        let _ = fs::set_permissions(&dest, perms);
                    }
                }
            }
            AssetAction::None => {}
        }

        println!("{} {}", "+".green(), entry.name.bold());
    }

    for name in &plan.skipped {
        println!(
            "{} {} (skipped on this platform)",
            "*".dimmed(),
            name.dimmed()
        );
    }

    println!(
        "{} downloaded {} asset(s) for {}",
        "Successfully".green(),
        plan.entries.len(),
        path.display().to_string().dimmed()
    );

    true
}
// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(file_path: &'a Path, _file_key: &'a str, cwd: &'a Path) -> AssetsCtx<'a> {
        AssetsCtx {
            file_path,
            cwd,
            byk_root: Path::new("/home/u/.byk"),
            venv_dir: Path::new("/home/u/.byk/py-venv/.venv"),
            downloads_dir: Path::new("/home/u/Downloads"),
            remote_base: None,
        }
    }

    #[test]
    fn single_entry_goes_to_downloads() {
        let block = json!({"tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/home/u/Downloads"));
        assert_eq!(plan.assets_dir(), PathBuf::from("/home/u/Downloads"));
        assert_eq!(plan.entry_dest("tool"), PathBuf::from("/home/u/Downloads/tool"));
    }

    #[test]
    fn multi_entry_goes_to_downloads_flat() {
        let block = json!({"tool": "https://x/tool", "lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/home/u/Downloads"));
        assert_eq!(plan.assets_dir(), PathBuf::from("/home/u/Downloads"));
        assert_eq!(plan.entry_dest("lint"), PathBuf::from("/home/u/Downloads/lint"));
    }

    #[test]
    fn global_file_defaults_to_downloads() {
        let block = json!({"tool": "https://x/tool", "lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/home/u/.byk/alias/demo.byk.json"), "@@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/home/u/Downloads"));
        assert_eq!(plan.assets_dir(), PathBuf::from("/home/u/Downloads"));
    }

    #[test]
    fn explicit_to_byk_works() {
        let block = json!({"$to": "$byk/tools/demo", "tool": "https://x/tool", "lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/home/u/.byk/tools/demo"));
        assert_eq!(plan.entry_dest("tool"), PathBuf::from("/home/u/.byk/tools/demo/tool"));
    }

    #[test]
    fn explicit_to_workdir_works() {
        let block = json!({"$to": "$workdir/out", "tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/proj/out"));
    }

    #[test]
    fn explicit_to_downloads_works() {
        let block = json!({"$to": "$downloads/my-tool", "tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/home/u/Downloads/my-tool"));
    }

    #[test]
    fn to_relative_rejected() {
        let block = json!({"$to": "./out", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/cfg/demo.byk.json"), "@demo", Path::new("/elsewhere"))).unwrap_err();
        assert!(err.contains("must start with $byk, $workdir, or $downloads"), "got: {}", err);
    }

    #[test]
    fn to_tilde_rejected() {
        let block = json!({"$to": "~/tools/x", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("must start with $byk, $workdir, or $downloads"), "got: {}", err);
    }

    #[test]
    fn to_absolute_rejected() {
        let block = json!({"$to": "/opt/linux", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("must start with $byk, $workdir, or $downloads"), "got: {}", err);
    }

    #[test]
    fn to_dotdot_escape_rejected() {
        let block = json!({"$to": "$byk/../../etc", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("escapes"), "got: {}", err);
    }

    #[test]
    fn to_dotdot_escape_workdir_rejected() {
        let block = json!({"$to": "$workdir/../..", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("escapes"), "got: {}", err);
    }

    #[test]
    fn to_self_reference_errors() {
        let block = json!({"$to": "$assets", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("self-reference"));
    }

    #[test]
    fn to_venv_rejected() {
        let block = json!({"$to": "$venv/tools", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("must start with $byk, $workdir, or $downloads"), "got: {}", err);
    }

    #[test]
    fn entry_platform_map_skips_missing_platform() {
        // 只含非当前平台键 → 跳过（不用具体平台名，避免测试机平台巧合命中）
        let block = json!({
            "tool": {"nonexistent-platform-a": "https://x/a", "nonexistent-platform-b": "https://x/b"}
        });
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert!(plan.entries.is_empty());
        assert_eq!(plan.skipped, vec!["tool".to_string()]);
    }

    #[test]
    fn entry_count_includes_skipped() {
        let block = json!({
            "tool": {"nonexistent-platform": "https://x/other"},
            "lint": "https://x/lint"
        });
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.skipped.len(), 1);
        // 默认 root = $downloads
        assert_eq!(plan.root, PathBuf::from("/home/u/Downloads"));
    }

    #[test]
    fn prefixes_parsed() {
        let block = json!({
            "a": "[exe] https://x/a",
            "b": "[tar] https://x/b.tar.gz",
            "c": "https://x/c"
        });
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        let actions: Vec<(&str, AssetAction)> = plan
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.action))
            .collect();
        assert!(actions.contains(&("a", AssetAction::Exec)));
        assert!(actions.contains(&("b", AssetAction::Tar)));
        assert!(actions.contains(&("c", AssetAction::None)));
    }

    #[test]
    fn local_path_relative_to_file_dir() {
        let block = json!({"tool": "./vendor/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/cfg/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(
            plan.entries[0].source,
            AssetSource::LocalPath(PathBuf::from("/proj/cfg/vendor/tool"))
        );
    }

    #[test]
    fn local_path_absolute_kept() {
        let block = json!({"tool": "/opt/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.entries[0].source, AssetSource::LocalPath(PathBuf::from("/opt/tool")));
    }

    #[test]
    fn name_with_slash_creates_subdir_dest() {
        let block = json!({"bin/lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.entry_dest("bin/lint"), PathBuf::from("/home/u/Downloads/bin/lint"));
    }

    #[test]
    fn dotdot_name_rejected() {
        let block = json!({"../evil": "https://x/evil"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains(".."));
    }

    #[test]
    fn absolute_name_rejected() {
        let block = json!({"/etc/passwd": "https://x/evil"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("relative"));
    }

    #[test]
    fn to_platform_map_resolved() {
        let block = json!({"$to": {"linux-x86_64": "$byk/tools/linux", "darwin-arm64": "$byk/tools/mac"}, "tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            PathBuf::from("/home/u/.byk/tools/mac")
        } else {
            PathBuf::from("/home/u/.byk/tools/linux")
        };
        assert_eq!(plan.root, expected);
    }

    #[test]
    fn relative_url_with_remote_base() {
        let mut c = ctx(Path::new("/tmp/demo.byk.json"), "@demo", Path::new("/proj"));
        c.remote_base = Some("https://raw.githubusercontent.com/u/r/main/");
        let block = json!({"tool": "bin/tool"});
        let plan = resolve_assets_plan(&block, &c).unwrap();
        assert_eq!(
            plan.entries[0].source,
            AssetSource::Url("https://raw.githubusercontent.com/u/r/main/bin/tool".into())
        );
    }

    #[test]
    fn full_url_ignores_remote_base() {
        let mut c = ctx(Path::new("/tmp/demo.byk.json"), "@demo", Path::new("/proj"));
        c.remote_base = Some("https://raw.githubusercontent.com/u/r/main/");
        let block = json!({"tool": "https://other.example/tool"});
        let plan = resolve_assets_plan(&block, &c).unwrap();
        assert_eq!(
            plan.entries[0].source,
            AssetSource::Url("https://other.example/tool".into())
        );
    }
}