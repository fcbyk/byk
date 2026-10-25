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
// --add pip <target>
// ---------------------------------------------------------------------------

/// `byk add pip <target>`：安装别名文件 `$pip` 声明的包到 byk venv。
///
/// target 三种形态：
/// - `@stem`  → 当前目录 `<stem>.byk.json`
/// - `@@stem` → 全局目录 `~/.byk/alias/<stem>.byk.json`
/// - 路径     → 任意 `.byk.json` 文件
///
/// venv 不存在时按需创建（探测 uv / 系统 Python，一次性确认）。
/// 文件未声明 `$pip` 时为友好 no-op（退出码 0）。
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

    // 读取并提取文件级 $pip（宽松解析：string | string[]）
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

    if packages.is_empty() {
        println!(
            "{} no $pip declared in {}",
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

    // venv 按需创建（含一次性确认；已就绪则直接返回）
    plugins::install::ensure_python_env(layout, "byk add pip");

    for pkg in &packages {
        println!("{}", format!("Installing pip package: {}", pkg).dimmed());
        plugins::install::install_python_package(pkg, layout);
        println!("{} {}", "+".green(), pkg.bold());
    }

    println!(
        "{} installed {} package(s) for {}",
        "Successfully".green(),
        packages.len(),
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

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ctx = assets::AssetsCtx {
        file_path: &path,
        file_key: target,
        cwd: &cwd,
        byk_root: &layout.root_dir,
        venv_dir: &layout.venv_dir,
    };
    let plan = match assets::resolve_assets_plan(&block, &ctx) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} invalid $assets in {}: {}", "Error:".red(), path.display(), e);
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
        return;
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

    // 资产根目录（含容器）自动创建
    plugins::install::ensure_dir(&plan.assets_dir(), "assets");

    for entry in &plan.entries {
        let dest = plan.entry_dest(&entry.name);
        if let Some(parent) = dest.parent() {
            plugins::install::ensure_dir(parent, "asset");
        }

        // 获取文件：[tar] 先进临时文件（解压后删除），其余直接落目标
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
                    eprintln!("{} failed to download {}: {}", "Error:".red(), entry.name.bold(), e);
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
                    eprintln!("{} failed to copy {}: {}", "Error:".red(), entry.name.bold(), e);
                    std::process::exit(1);
                }
            }
        }

        // 后处理：解压 / chmod
        match entry.action {
            assets::AssetAction::Tar => {
                if let Err(e) = plugins::install::extract_archive(&fetch_target, &dest) {
                    eprintln!("{} failed to extract {}: {}", "Error:".red(), entry.name.bold(), e);
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
}

