//! `$assets` 解析器：把别名文件的资产声明解析为可执行计划。
//!
//! 两个消费者共用同一份逻辑，保证“下载落点”与“`$assets` 变量值”永远一致：
//! - `byk add assets`（下载 / 解压 / chmod）
//! - 别名合并期（计算 `$assets` 变量的值）
//!
//! 目标规则（R1）：
//! - `$to` 显式设置 → root = 解析值（平台映射 / `~` / 相对文件目录 / 自动创建），
//!   条目直接落 `<root>/<名字>`；
//! - `$to` 缺省 → root = 本地文件用文件目录、全局文件用当前工作目录；此时
//!   声明 1 条目平铺，≥2 条目进 `<root>/assets/`（容器）；计数按声明总数
//!   （平台跳过只影响下载内容，不影响落点形状，保证跨平台一致）。
//!
//! 条目规则：名字即 root 下的相对路径（拒绝 `..` / 绝对路径）；
//! 源为 URL 或本地路径（相对文件目录解析）；`[tar]` 解压到同名子目录、
//! `[exe]` 下载后 `chmod +x`；条目值可为平台映射，当前平台不在映射 → 跳过。

use std::path::{Path, PathBuf};

use colored::Colorize;
use serde_json::Value;

use crate::core::aliases::variables::{VarContext, expand_variables};
use crate::core::plugins::types::{PYTHON_BIN, VENV_BIN};

/// 当前平台（编译期注入，与 `build.rs` 五值一致）。
const PLATFORM: &str = env!("PLATFORM");

/// 解析上下文：一个别名文件 + 运行环境。
pub struct AssetsCtx<'a> {
    /// 别名文件路径（相对路径的解析基准）
    pub file_path: &'a Path,
    /// 文件 key（`@stem` 本地 / `@@stem` 全局，判 Scope）
    pub file_key: &'a str,
    /// 当前工作目录（全局文件缺省根）
    pub cwd: &'a Path,
    /// byk 根目录（`$byk`）
    pub byk_root: &'a Path,
    /// venv 目录（`$venv`/`$python` 展开用）
    pub venv_dir: &'a Path,
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
    /// 是否套 `assets/` 容器（仅缺省根 + 多条目时）
    pub container: bool,
    /// 适用于当前平台的条目
    pub entries: Vec<AssetEntry>,
    /// 平台不适用被跳过的条目名
    pub skipped: Vec<String>,
}

impl AssetsPlan {
    /// 条目实际所在目录——`$assets` 变量的值。
    pub fn assets_dir(&self) -> PathBuf {
        if self.container {
            self.root.join("assets")
        } else {
            self.root.clone()
        }
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

/// 解析条目源字符串：剥离 `[tar] `/`[exe] ` 前缀，区分 URL 与本地路径。
fn parse_source(raw: &str, file_dir: &Path) -> Result<(AssetSource, AssetAction), String> {
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

    // 阶段二：展开内置变量（$byk/$venv/$python）
    let var_ctx = VarContext {
        byk_root: ctx.byk_root.to_path_buf(),
        venv_bin: ctx.venv_dir.join(VENV_BIN),
        venv_python: ctx.venv_dir.join(VENV_BIN).join(PYTHON_BIN),
        assets_dir: None,
    };
    let expanded = expand_variables(&raw, &var_ctx)
        .map_err(|e| format!("{} (while expanding $to '{}')", e, raw))?;

    let path = expand_tilde(&expanded);
    let path = if path.is_absolute() {
        path
    } else {
        // 相对路径相对文件目录解析（确定性，不随运行 cwd 漂移）
        ctx.file_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(path)
    };
    Ok(Some(path))
}

/// 解析 `$assets` 块为资产计划。
pub fn resolve_assets_plan(block: &Value, ctx: &AssetsCtx) -> Result<AssetsPlan, String> {
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

    // 容器：仅缺省根 + 多条目；计数按声明总数（平台跳过不改变形状）
    let container = explicit_root.is_none() && entry_names.len() >= 2;

    let root = match explicit_root {
        Some(r) => r,
        None => {
            if ctx.file_key.starts_with("@@") {
                // 全局文件：清单语义，资源落在当前工作目录
                ctx.cwd.to_path_buf()
            } else {
                // 本地文件：资源属于文件所在项目
                ctx.file_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf()
            }
        }
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
        let (source, action) = parse_source(&raw, file_dir)?;
        entries.push(AssetEntry {
            name: name.clone(),
            source,
            action,
        });
    }

    Ok(AssetsPlan {
        root,
        container,
        entries,
        skipped,
    })
}

/// 供扫描/合并期使用：仅计算 `$assets` 变量值（目录），不做条目校验。
///
/// 计划解析失败时返回 None 并打印一次警告——单个文件的坏配置不应
/// 拖垮整个别名系统（`byk add assets` 会给出硬错误）。
pub fn assets_dir_or_warn(block: &Value, ctx: &AssetsCtx) -> Option<PathBuf> {
    match resolve_assets_plan(block, ctx) {
        Ok(plan) => Some(plan.assets_dir()),
        Err(e) => {
            eprintln!(
                "{} invalid $assets in {}: {}",
                "Warning:".yellow(),
                ctx.file_path.display(),
                e
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(file_path: &'a Path, file_key: &'a str, cwd: &'a Path) -> AssetsCtx<'a> {
        AssetsCtx {
            file_path,
            file_key,
            cwd,
            byk_root: Path::new("/home/u/.byk"),
            venv_dir: Path::new("/home/u/.byk/py-venv/.venv"),
        }
    }

    #[test]
    fn single_entry_local_flat() {
        let block = json!({"tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/proj"));
        assert!(!plan.container);
        assert_eq!(plan.assets_dir(), PathBuf::from("/proj"));
        assert_eq!(plan.entry_dest("tool"), PathBuf::from("/proj/tool"));
    }

    #[test]
    fn multi_entry_local_container() {
        let block = json!({"tool": "https://x/tool", "lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert!(plan.container);
        assert_eq!(plan.assets_dir(), PathBuf::from("/proj/assets"));
        assert_eq!(plan.entry_dest("lint"), PathBuf::from("/proj/assets/lint"));
    }

    #[test]
    fn multi_entry_global_uses_cwd() {
        let block = json!({"tool": "https://x/tool", "lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/home/u/.byk/alias/demo.byk.json"), "@@demo", Path::new("/proj"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/proj"));
        assert!(plan.container);
        assert_eq!(plan.assets_dir(), PathBuf::from("/proj/assets"));
    }

    #[test]
    fn explicit_to_skips_container() {
        let block = json!({"$to": "$byk/tools/demo", "tool": "https://x/tool", "lint": "https://x/lint"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert!(!plan.container);
        assert_eq!(plan.root, PathBuf::from("/home/u/.byk/tools/demo"));
        assert_eq!(plan.entry_dest("tool"), PathBuf::from("/home/u/.byk/tools/demo/tool"));
    }

    #[test]
    fn to_relative_resolves_against_file_dir() {
        let block = json!({"$to": "./out", "tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/cfg/demo.byk.json"), "@demo", Path::new("/elsewhere"))).unwrap();
        assert_eq!(plan.root, PathBuf::from("/proj/cfg/out"));
    }

    #[test]
    fn to_tilde_expands() {
        let block = json!({"$to": "~/tools/x", "tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(plan.root, home.join("tools/x"));
    }

    #[test]
    fn to_self_reference_errors() {
        let block = json!({"$to": "$assets", "tool": "https://x/tool"});
        let err = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap_err();
        assert!(err.contains("self-reference"));
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
    fn entry_count_includes_skipped_for_container() {
        // 声明 2 条，1 条被平台跳过 → 容器判定仍按声明总数（形状稳定）
        let block = json!({
            "tool": {"nonexistent-platform": "https://x/other"},
            "lint": "https://x/lint"
        });
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        assert!(plan.container);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.skipped.len(), 1);
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
        assert_eq!(plan.entry_dest("bin/lint"), PathBuf::from("/proj/bin/lint"));
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
        let block = json!({"$to": {"linux-x86_64": "/opt/linux", "darwin-arm64": "/opt/mac"}, "tool": "https://x/tool"});
        let plan = resolve_assets_plan(&block, &ctx(Path::new("/proj/demo.byk.json"), "@demo", Path::new("/proj"))).unwrap();
        let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            PathBuf::from("/opt/mac")
        } else {
            PathBuf::from("/opt/linux")
        };
        assert_eq!(plan.root, expected);
    }
}
