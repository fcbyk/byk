//! `byk add` 子命令逻辑。
//!
//! 包含：
//! - 插件安装（委托给 plugins 模块）
//! - 内置功能初始化（comp）
//! - 别名文件 $pip 依赖安装（pip）

use colored::Colorize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use crate::core::aliases;
use crate::core::assets;
use crate::core::paths::PathLayout;
use crate::core::plugins;
use crate::utils::shell;

// 重导出插件安装入口
pub use super::plugins::install::install_plugin;

// ---------------------------------------------------------------------------
// --add comp（原 --init comp）
// ---------------------------------------------------------------------------

/// 初始化 shell 补全。
///
/// 行为与 install.sh 保持一致：
/// - 从 `$SHELL` 检测当前 shell（zsh / bash）
/// - 在对应的 rc 文件中追加 `source <(byk completion <shell>)` 行
/// - 幂等：已配置则跳过
pub fn init_completion() {
    let (_, shell_name) = match shell::detect_shell() {
        Some(s) => s,
        None => {
            let shell_val = std::env::var("SHELL").unwrap_or_default();
            eprintln!(
                "{} {} {}",
                "Unsupported shell:".red(),
                shell_val.dimmed(),
                "(supported: zsh, bash)".dimmed()
            );
            return;
        }
    };

    let rc_path = match shell::rc_path() {
        Some(p) => p,
        None => {
            eprintln!("{}", "Cannot determine home directory.".red());
            return;
        }
    };

    let line = shell::completion_line(shell_name);

    // 读取 rc 文件检测是否已配置
    let content = fs::read_to_string(&rc_path).unwrap_or_default();
    if content.contains(&line) {
        println!(
            "Shell completion already configured in {}",
            rc_path.display().to_string().dimmed()
        );
        return;
    }

    // 追加到 rc 文件
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&rc_path)
        .unwrap_or_else(|e| {
            eprintln!("Failed to open {}: {}", rc_path.display(), e);
            std::process::exit(1);
        });

    // 确保从新行开始
    if !content.is_empty() && !content.ends_with('\n') {
        let _ = writeln!(file);
    }

    writeln!(file, "\n# byk shell completion\n{}", line).unwrap_or_else(|e| {
        eprintln!("Failed to write to {}: {}", rc_path.display(), e);
        std::process::exit(1);
    });

    println!(
        "+ Shell completion configured in {}",
        rc_path.display().to_string().dimmed()
    );
    println!(
        "  Restart your shell or run: {} {}",
        "source".dimmed(),
        rc_path.display().to_string().dimmed()
    );
}

// ---------------------------------------------------------------------------
// 安装 helper（pip / assets 共用）
// ---------------------------------------------------------------------------

/// 安装 pip 包列表到 byk 托管 venv（按需创建，含一次性确认）。
fn install_pip_packages(packages: &[String], layout: &PathLayout, subject: &str) {
    plugins::install::ensure_python_env(layout, subject);
    for pkg in packages {
        println!(
            "{}",
            format!("Installing pip package: {}", pkg).dimmed()
        );
        plugins::install::install_python_package(pkg, layout);
        println!("{} {}", "+".green(), pkg.bold());
    }
}

/// 下载 $assets 块（解析 plan + 下载循环）。返回是否实际下载了资源。
fn download_assets_block(
    block: &serde_json::Value,
    path: &Path,
    file_key: &str,
    layout: &PathLayout,
    cdn: bool,
) -> bool {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ctx = assets::AssetsCtx {
        file_path: path,
        file_key,
        cwd: &cwd,
        byk_root: &layout.root_dir,
        venv_dir: &layout.venv_dir,
    };
    let plan = match assets::resolve_assets_plan(block, &ctx) {
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

    plugins::install::ensure_dir(&plan.assets_dir(), "assets");

    for entry in &plan.entries {
        let dest = plan.entry_dest(&entry.name);
        if let Some(parent) = dest.parent() {
            plugins::install::ensure_dir(parent, "asset");
        }

        let is_tar = entry.action == assets::AssetAction::Tar;
        let fetch_target = if is_tar {
            dest.with_extension("byk-tmp")
        } else {
            dest.clone()
        };

        match &entry.source {
            assets::AssetSource::Url(url) => {
                let url = if cdn {
                    plugins::install::to_jsdelivr_url(url)
                } else {
                    url.clone()
                };
                if let Err(e) = plugins::install::download_script(&url, &fetch_target) {
                    eprintln!(
                        "{} failed to download {}: {}",
                        "Error:".red(),
                        entry.name.bold(),
                        e
                    );
                    std::process::exit(1);
                }
            }
            assets::AssetSource::LocalPath(p) => {
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
            assets::AssetAction::Tar => {
                if let Err(e) = plugins::install::extract_archive(&fetch_target, &dest) {
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
            assets::AssetAction::Exec => {
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
            assets::AssetAction::None => {}
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
// $install 筛选
// ---------------------------------------------------------------------------

/// 副本保留的文件级字段（`$install` 是安装期元数据，刻意排除）。
const FILE_LEVEL_FIELDS: &[&str] = &[
    "$priority", "$cwd", "$interactive", "$paths", "$pip", "$pip-keep", "$assets", "$var",
];

/// `$install` 三级继承筛选：保留 install=true 的别名子树，剔除 `$install` 标志本身。
///
/// - 当前层有 `$install` → 覆盖继承值（与 `$interactive` 同构）
/// - 别名叶子（String / 含 `$cmd` 的 Object）→ cur 为 true 才保留
/// - 分组（无 `$cmd` 的 Object）→ 递归，筛选后为空则整组剔除
fn filter_install(
    obj: &serde_json::Map<String, serde_json::Value>,
    inherited: bool,
) -> serde_json::Map<String, serde_json::Value> {
    let cur = obj
        .get("$install")
        .and_then(|v| v.as_bool())
        .unwrap_or(inherited);
    let mut result = serde_json::Map::new();
    for (key, val) in obj {
        if key.starts_with('$') {
            continue;
        }
        if aliases::is_alias_value(val) {
            // 叶子自己的 $install 覆盖继承值（与 $interactive 同构）
            let leaf_install = val
                .as_object()
                .and_then(|o| o.get("$install"))
                .and_then(|v| v.as_bool())
                .unwrap_or(cur);
            if leaf_install {
                result.insert(key.clone(), strip_install(val));
            }
        } else if let serde_json::Value::Object(inner) = val {
            let sub = filter_install(inner, cur);
            if !sub.is_empty() {
                result.insert(key.clone(), serde_json::Value::Object(sub));
            }
        }
    }
    result
}

/// 剔除叶子对象里的 `$install` 键（其余原样返回）。
fn strip_install(val: &serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(obj) = val {
        let mut m = obj.clone();
        m.remove("$install");
        serde_json::Value::Object(m)
    } else {
        val.clone()
    }
}

// ---------------------------------------------------------------------------
// --add pip <target>
// ---------------------------------------------------------------------------

/// `byk add pip <target>`：安装别名文件 `$pip` / `$pip-keep` 声明的包到 byk venv。
///
/// target 三种形态：
/// - `@stem`  → 当前目录 `<stem>.byk.json`
/// - `@@stem` → 全局目录 `~/.byk/alias/<stem>.byk.json`
/// - 路径     → 任意 `.byk.json` 文件
///
/// `$pip`（remove 语义）与 `$pip-keep`（keep 语义）目前行为一致，均安装到共享
/// venv；卸载时的区分留待后续 remove 读 *.byk.json 时实现。
/// venv 不存在时按需创建（探测 uv / 系统 Python，一次性确认）。
/// 文件未声明两者时为友好 no-op（退出码 0）。
pub fn install_alias_pip(target: Option<&str>, layout: &PathLayout) {
    let target = match target {
        Some(t) => t,
        None => {
            crate::render::add::render_pip();
            return;
        }
    };

    // 解析目标文件路径
    let path = match resolve_alias_file(target, layout) {
        Some(p) => p,
        None => {
            eprintln!("{} alias file not found: {}", "Error:".red(), target.bold());
            eprintln!("  Target must be @file, @@file, or a path to a .byk.json file.");
            std::process::exit(1);
        }
    };

    // 读取并提取文件级 $pip / $pip-keep（宽松解析：string | string[]）
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} failed to read {}: {}", "Error:".red(), path.display(), e);
            std::process::exit(1);
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} invalid JSON in {}: {}", "Error:".red(), path.display(), e);
            std::process::exit(1);
        }
    };
    let packages = json
        .get("$pip")
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();
    let packages_keep = json
        .get("$pip-keep")
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();

    if packages.is_empty() && packages_keep.is_empty() {
        println!(
            "{} no $pip or $pip-keep declared in {}",
            "*".dimmed(),
            path.display().to_string().dimmed()
        );
        return;
    }

    println!(
        "{} Installing pip dependencies from {}",
        "==>".cyan().bold(),
        path.display().to_string().bold()
    );

    if !packages.is_empty() {
        install_pip_packages(&packages, layout, "byk add pip");
    }
    if !packages_keep.is_empty() {
        install_pip_packages(&packages_keep, layout, "byk add pip");
    }

    println!(
        "{} installed {} package(s) for {}",
        "Successfully".green(),
        packages.len() + packages_keep.len(),
        path.display().to_string().dimmed()
    );
}

/// `byk add assets <target>`：下载别名文件 `$assets` 声明的资源。
///
/// target: `@stem`（cwd 文件）/ `@@stem`（全局目录）。v1 不支持路径形态——
/// 资产目录需要文件 key 来定位。
/// 与 `add pip` 的本质差别：不涉及 venv，纯文件获取。
pub fn install_alias_assets(target: Option<&str>, layout: &PathLayout, cdn: bool) {
    let target = match target {
        Some(t) => t,
        None => {
            crate::render::add::render_assets();
            return;
        }
    };

    // 解析目标文件
    let path = match resolve_alias_file(target, layout) {
        Some(p) => p,
        None => {
            eprintln!("{} alias file not found: {}", "Error:".red(), target.bold());
            eprintln!("  Target must be @file or @@file.");
            std::process::exit(1);
        }
    };

    // 读取文件 + 阶段一变量替换（与扫描同一函数，保证两边看到一致的内容）
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} failed to read {}: {}", "Error:".red(), path.display(), e);
            std::process::exit(1);
        }
    };
    let mut json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} invalid JSON in {}: {}", "Error:".red(), path.display(), e);
            std::process::exit(1);
        }
    };
    crate::core::vars::preprocess_vars(&mut json);

    let block = match json.get("$assets") {
        Some(b) if b.is_object() => b.clone(),
        _ => {
            println!(
                "{} no $assets declared in {}",
                "*".dimmed(),
                path.display().to_string().dimmed()
            );
            return;
        }
    };

    download_assets_block(&block, &path, target, layout, cdn);
}

// ---------------------------------------------------------------------------
// --add @xxx（别名安装）
// ---------------------------------------------------------------------------

/// `byk add @xxx`：安装本地别名文件到全局目录。
///
/// 流程：读原始 JSON → `$var` 阶段一替换（下载用）→ 装 `$pip` → 下载 `$assets`
/// → `$install` 筛选（保留 install=true 的别名子树）→ 写副本到 `~/.byk/alias/`。
///
/// 副本 = 文件级字段（`$var`/`$pip`/`$assets`/`$cwd`/`$paths`/`$priority`/`$interactive`）
///       + 筛选后的别名（`$install` 标志本身不进入副本）。
///
/// 无 `$install` 别名时，若文件有 `$pip`/`$assets` 仍写副本（记录已安装内容，为卸载提供钩子）；
/// 三者皆无 → 报错，不写副本。
pub fn install_alias(target: Option<&str>, layout: &PathLayout, cdn: bool) {
    let target = match target {
        Some(t) => t,
        None => {
            crate::render::add::render_alias();
            return;
        }
    };

    // 只支持 @xxx（本地文件安装到全局）；@@ / 路径形态不支持
    let stem = match target.strip_prefix('@') {
        Some(s) => s,
        None => {
            eprintln!(
                "{} install target must be @file (e.g. byk add @demo)",
                "Error:".red()
            );
            eprintln!("  Found: {}", target.bold());
            std::process::exit(1);
        }
    };
    if stem.starts_with('@') {
        eprintln!(
            "{} '@@file' is not supported for alias install.",
            "Error:".red()
        );
        eprintln!("  Global files are already installed; use @file to install a local file.");
        std::process::exit(1);
    }

    // 解析本地文件
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(alias_file_name(stem));
    if !path.is_file() {
        eprintln!("{} alias file not found: {}", "Error:".red(), target.bold());
        eprintln!(
            "  Expected {} in the current directory.",
            path.display().to_string().dimmed()
        );
        std::process::exit(1);
    }

    // 读原始 JSON
    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("{} failed to read {}: {}", "Error:".red(), path.display(), e);
        std::process::exit(1);
    });
    let original: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!(
            "{} invalid JSON in {}: {}",
            "Error:".red(),
            path.display(),
            e
        );
        std::process::exit(1);
    });

    // 阶段一变量替换（下载用，副本仍保留原始 $var 声明）
    let mut substituted = original.clone();
    crate::core::vars::preprocess_vars(&mut substituted);

    // 提取 pip / pip-keep / assets（替换后）
    let packages = substituted
        .get("$pip")
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();
    let packages_keep = substituted
        .get("$pip-keep")
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();
    let assets_block = substituted
        .get("$assets")
        .cloned()
        .filter(|v| v.is_object());

    // $install 筛选（对原始 JSON；文件级 $install 作为初始继承值）
    let obj = original.as_object().cloned().unwrap_or_default();
    let file_install = obj
        .get("$install")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let filtered = filter_install(&obj, file_install);

    // 判断是否有可安装内容
    let has_install_alias = !filtered.is_empty();
    let has_pip = !packages.is_empty() || !packages_keep.is_empty();
    let has_assets = assets_block.is_some();
    if !has_install_alias && !has_pip && !has_assets {
        eprintln!(
            "{} nothing to install in {}",
            "Error:".red(),
            path.display().to_string().dimmed()
        );
        eprintln!("  Mark aliases with $install: true, or declare $pip / $pip-keep / $assets.");
        std::process::exit(1);
    }

    // 装 pip（$pip + $pip-keep 目前行为一致，均安装到 venv）
    if has_pip {
        println!(
            "{} Installing pip dependencies from {}",
            "==>".cyan().bold(),
            path.display().to_string().bold()
        );
        if !packages.is_empty() {
            install_pip_packages(&packages, layout, "byk add");
        }
        if !packages_keep.is_empty() {
            install_pip_packages(&packages_keep, layout, "byk add");
        }
        println!(
            "{} installed {} package(s)",
            "Successfully".green(),
            packages.len() + packages_keep.len()
        );
    }

    // 下载 assets
    if let Some(block) = &assets_block {
        download_assets_block(block, &path, target, layout, cdn);
    }

    // 组装副本：文件级字段 + 筛选后别名（$install 排除）
    let mut result = serde_json::Map::new();
    for field in FILE_LEVEL_FIELDS {
        if let Some(v) = obj.get(*field) {
            result.insert((*field).to_string(), v.clone());
        }
    }
    for (k, v) in filtered {
        result.insert(k, v);
    }

    let dest = layout.alias_dir.join(alias_file_name(stem));
    // ~/.byk/alias/ 是内部目录，静默创建（不向用户报告）
    if let Err(e) = fs::create_dir_all(&layout.alias_dir) {
        eprintln!(
            "{} failed to create {}: {}",
            "Error:".red(),
            layout.alias_dir.display(),
            e
        );
        std::process::exit(1);
    }
    let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(result))
        .unwrap_or_else(|e| {
            eprintln!("{} failed to serialize: {}", "Error:".red(), e);
            std::process::exit(1);
        });
    fs::write(&dest, pretty).unwrap_or_else(|e| {
        eprintln!("{} failed to write {}: {}", "Error:".red(), dest.display(), e);
        std::process::exit(1);
    });

    println!(
        "{} installed alias file to {}",
        "Successfully".green(),
        dest.display().to_string().dimmed()
    );
}

/// 解析 target 为具体文件路径（pip / assets 共用）。文件不存在返回 None。
fn resolve_alias_file(target: &str, layout: &PathLayout) -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = pip_target_path(target, &cwd, &layout.alias_dir);
    if path.is_file() { Some(path) } else { None }
}

/// 纯函数：target → 文件路径（不做存在性检查，便于测试）。
///
/// - `@stem`  → `<cwd>/<stem>.byk.json`
/// - `@@stem` → `<alias_dir>/<stem>.byk.json`
/// - 其他     → 原样当作路径
fn pip_target_path(target: &str, cwd: &Path, alias_dir: &Path) -> PathBuf {
    if let Some(stem) = target.strip_prefix("@@") {
        alias_dir.join(alias_file_name(stem))
    } else if let Some(stem) = target.strip_prefix('@') {
        cwd.join(alias_file_name(stem))
    } else {
        PathBuf::from(target)
    }
}

/// stem → 文件名。空 stem（匿名 `@` / `@@`）→ `.byk.json`。
fn alias_file_name(stem: &str) -> String {
    if stem.is_empty() {
        ".byk.json".to_string()
    } else {
        format!("{}.byk.json", stem)
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pip_target_local_key() {
        let p = pip_target_path("@demo", Path::new("/work"), Path::new("/home/.byk/alias"));
        assert_eq!(p, PathBuf::from("/work/demo.byk.json"));
    }

    #[test]
    fn pip_target_global_key() {
        let p = pip_target_path("@@demo", Path::new("/work"), Path::new("/home/.byk/alias"));
        assert_eq!(p, PathBuf::from("/home/.byk/alias/demo.byk.json"));
    }

    #[test]
    fn pip_target_anonymous_keys() {
        let p = pip_target_path("@", Path::new("/work"), Path::new("/home/.byk/alias"));
        assert_eq!(p, PathBuf::from("/work/.byk.json"));
        let p = pip_target_path("@@", Path::new("/work"), Path::new("/home/.byk/alias"));
        assert_eq!(p, PathBuf::from("/home/.byk/alias/.byk.json"));
    }

    #[test]
    fn pip_target_path_form() {
        let p = pip_target_path(
            "./configs/demo.byk.json",
            Path::new("/work"),
            Path::new("/home/.byk/alias"),
        );
        assert_eq!(p, PathBuf::from("./configs/demo.byk.json"));
    }

    #[test]
    fn alias_file_name_forms() {
        assert_eq!(alias_file_name("demo"), "demo.byk.json");
        assert_eq!(alias_file_name(""), ".byk.json");
    }

    // ==================== filter_install ====================

    #[test]
    fn filter_install_file_level_keeps_all() {
        let obj = json!({
            "$install": true,
            "a": "echo a",
            "b": {"$cmd": "echo b"}
        })
        .as_object()
        .unwrap()
        .clone();
        let result = filter_install(&obj, false);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("a"));
        assert!(result.contains_key("b"));
        assert!(!result.contains_key("$install"));
    }

    #[test]
    fn filter_install_default_false_drops_all() {
        let obj = json!({"a": "echo a", "b": "echo b"}).as_object().unwrap().clone();
        assert!(filter_install(&obj, false).is_empty());
    }

    #[test]
    fn filter_install_group_level() {
        let obj = json!({
            "tools": {"$install": true, "lint": "echo lint", "fmt": "echo fmt"},
            "local": {"dev": "echo dev"}
        })
        .as_object()
        .unwrap()
        .clone();
        let result = filter_install(&obj, false);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("tools"));
        assert!(!result.contains_key("local"));
        let tools = result["tools"].as_object().unwrap();
        assert_eq!(tools.len(), 2);
        assert!(!tools.contains_key("$install"));
    }

    #[test]
    fn filter_install_alias_level_strips_flag() {
        let obj = json!({
            "a": {"$cmd": "echo a", "$install": true},
            "b": "echo b"
        })
        .as_object()
        .unwrap()
        .clone();
        let result = filter_install(&obj, false);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("a"));
        let a = result["a"].as_object().unwrap();
        assert!(a.contains_key("$cmd"));
        assert!(!a.contains_key("$install"));
    }

    #[test]
    fn filter_install_subgroup_false_overrides() {
        let obj = json!({
            "tools": {
                "$install": true,
                "lint": "echo lint",
                "nested": {"$install": false, "x": "echo x"}
            }
        })
        .as_object()
        .unwrap()
        .clone();
        let result = filter_install(&obj, false);
        let tools = result["tools"].as_object().unwrap();
        assert!(tools.contains_key("lint"));
        assert!(!tools.contains_key("nested"));
    }

    #[test]
    fn filter_install_leaf_false_overrides_group() {
        let obj = json!({
            "g": {
                "$install": true,
                "a": {"$cmd": "echo a", "$install": false},
                "b": "echo b"
            }
        })
        .as_object()
        .unwrap()
        .clone();
        let result = filter_install(&obj, false);
        let g = result["g"].as_object().unwrap();
        assert!(g.contains_key("b"));
        assert!(!g.contains_key("a"));
    }
}

