//! `byk remove` 子命令逻辑。
//!
//! 删除 `byk add` 创建的持久化数据（venv、缓存、别名等）。

use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

use super::add;
use super::add::assets;
use super::add::pip as add_pip;
use super::aliases;
use super::paths::PathLayout;
use super::vars;
use crate::utils::shell;

// ---------------------------------------------------------------------------
// remove comp
// ---------------------------------------------------------------------------

/// 移除 shell 补全配置。
///
/// 从 .zshrc / .bashrc 中移除 `byk completion` 相关行及前面的注释行。
pub fn rm_comp() {
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

    let content = fs::read_to_string(&rc_path).unwrap_or_default();
    let line = shell::completion_line(shell_name);

    if !content.contains(&line) {
        println!(
            "{}",
            "Shell completion not configured, nothing to remove.".dimmed()
        );
        return;
    }

    let new_content = shell::strip_completion_lines(&content);
    shell::write_rc(&rc_path, &new_content);

    println!(
        "  {} shell completion in {}",
        "-".red(),
        rc_path.display().to_string().dimmed()
    );
    println!(
        "{} {}",
        "Shell completion removed from".green(),
        rc_path.display().to_string().dimmed()
    );
}

// ---------------------------------------------------------------------------
// remove all
// ---------------------------------------------------------------------------

/// 删除所有 byk 持久化数据：`~/.byk/` + shell 补全配置。
///
/// 如果当前运行的 byk 二进制在 `~/.byk/` 下，跳过自身所在目录，
/// 提示用户手动删除。否则直接 `remove_dir_all(~/.byk/)`。
/// 需要输入 "all" 确认。
pub fn rm_all(layout: &PathLayout) {
    // ---------- 1. 检测待删除项 ----------
    let has_home = layout.home_exists;
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_in_home = has_home && exe.starts_with(&layout.root_dir);

    let has_comp = match shell::detect_shell() {
        Some((_, sn)) => {
            match shell::rc_path() {
                Some(p) => {
                    let content = fs::read_to_string(&p).unwrap_or_default();
                    content.contains(&shell::completion_line(sn))
                }
                None => false,
            }
        }
        None => false,
    };

    // exe 在 ~/.byk/ 下但没别的东西可删 → 提前结束
    if exe_in_home && !has_comp {
        // 检查 root 下是否只有 exe 所在目录
        let only_exe = fs::read_dir(&layout.root_dir)
            .map(|mut entries| {
                !entries.any(|e| {
                    if let Ok(e) = e {
                        !exe.starts_with(e.path())
                    } else {
                        false
                    }
                })
            })
            .unwrap_or(false);
        if only_exe {
            let keep_display = exe
                .strip_prefix(&layout.root_dir)
                .unwrap_or(&exe)
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();
            println!(
                "{}",
                format!(
                    "Nothing to remove (~/.byk/{}/ kept — contains running binary).",
                    keep_display
                )
                .dimmed()
            );
            println!();
            println!(
                "{} {} {}",
                "!".yellow().bold(),
                format!("~/.byk/{}/", keep_display).dimmed(),
                "kept (contains running byk binary)".dimmed()
            );
            println!(
                "  Remove manually: rm -rf {}",
                layout.root_dir.join(&keep_display).display()
            );
            return;
        }
    }

    if !has_home && !has_comp {
        println!("{}", "Nothing to remove.".dimmed());
    } else {
        // ---------- 2. 列出待删项 + 确认 ----------
        println!();
        println!("{}", "This will remove everything:".yellow());
        if has_home {
            if exe_in_home {
                // 逐项列出，标记 keep
                println!("  {}", layout.root_dir.display().to_string().dimmed());
                if let Ok(entries) = fs::read_dir(&layout.root_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let label = path.strip_prefix(&layout.root_dir).unwrap_or(&path);
                        if exe.starts_with(&path) {
                            println!(
                                "    {}/ {}",
                                label.display(),
                                "(kept — contains running binary)".dimmed()
                            );
                        } else {
                            println!(
                                "    {}/ {}",
                                label.display(),
                                "(removed)".dimmed()
                            );
                        }
                    }
                }
            } else {
                println!("  {}", layout.root_dir.display().to_string().dimmed());
                println!(
                    "  {}",
                    "(all subdirectories: venv, aliases, caches, plugins)".dimmed()
                );
            }
        }
        if has_comp
            && let Some(p) = shell::rc_path() {
                println!("  {}", p.display().to_string().dimmed());
                println!("    {}", "(byk completion line)".dimmed());
            }
        println!();

        if !shell::prompt_confirm("all") {
            return;
        }

        // ---------- 3. 执行删除 ----------
        if has_home {
            if exe_in_home {
                // 运行中二进制在 ~/.byk/ 下 → 逐项删除，跳过自身所在目录
                let mut kept_path: Option<std::path::PathBuf> = None;
                if let Ok(entries) = fs::read_dir(&layout.root_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if exe.starts_with(&path) {
                            kept_path = Some(path.clone());
                            let label = path.strip_prefix(&layout.root_dir).unwrap_or(&path);
                            println!(
                                "  {} ~/.byk/{}/ {}",
                                "!".yellow(),
                                label.display(),
                                "(kept — contains running binary)".dimmed()
                            );
                            continue;
                        }
                        if path.is_dir() {
                            let _ = fs::remove_dir_all(&path);
                            let label = path.strip_prefix(&layout.root_dir).unwrap_or(&path);
                            println!(
                                "  {} ~/.byk/{}/ {}",
                                "-".red(),
                                label.display(),
                                "(removed)".dimmed()
                            );
                        } else {
                            let _ = fs::remove_file(&path);
                            let label = path.strip_prefix(&layout.root_dir).unwrap_or(&path);
                            println!(
                                "  {} ~/.byk/{} {}",
                                "-".red(),
                                label.display(),
                                "(removed)".dimmed()
                            );
                        }
                    }
                }
                let _ = fs::remove_dir(&layout.root_dir);

                if let Some(p) = kept_path {
                    let label = p.strip_prefix(&layout.root_dir).unwrap_or(&p);
                    println!();
                    println!(
                        "{} {} {}",
                        "!".yellow().bold(),
                        label.display().to_string().dimmed(),
                        "kept (contains running byk binary)".dimmed()
                    );
                    println!("  Remove manually: rm -rf {}", p.display());
                }
            } else {
                // 安全：二进制不在 ~/.byk/ 下
                let _ = fs::remove_dir_all(&layout.root_dir);
                println!(
                    "  {} ~/.byk/ {}",
                    "-".red(),
                    "(removed)".dimmed()
                );
            }
        }
        if has_comp
            && let Some(p) = shell::rc_path() {
                let content = fs::read_to_string(&p).unwrap_or_default();
                let new_content = shell::strip_completion_lines(&content);
                let _ = fs::write(&p, new_content);
                println!(
                    "  {} shell completion in {}",
                    "-".red(),
                    p.display().to_string().dimmed()
                );
            }
        println!();
        println!("{}", "Everything removed.".green());
    }
}

// ---------------------------------------------------------------------------
// remove @xxx（当前目录）
// ---------------------------------------------------------------------------

/// `byk remove @xxx`：读取当前目录 `xxx.byk.json` 的 `$remove` 字段并执行卸载。
pub fn uninstall_from_local(name: &str, layout: &PathLayout) {
    let stem = &name[1..]; // strip '@'
    if stem.is_empty() {
        eprintln!(
            "{} invalid argument: {} (expected @<name>)",
            "Error:".red(),
            name.bold()
        );
        std::process::exit(1);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(add::alias_file_name(stem));

    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }

    let file_key = format!("@{}", stem);
    uninstall_from_file(&path, &file_key, stem, layout);
}

// ---------------------------------------------------------------------------
// remove @@xxx（全局别名目录）
// ---------------------------------------------------------------------------

/// `byk remove @@xxx`：读取 `~/.byk/alias/xxx.byk.json` 的 `$remove` 字段并执行卸载。
pub fn uninstall_from_global(name: &str, layout: &PathLayout) {
    let stem = &name[2..]; // strip '@@'
    if stem.is_empty() {
        eprintln!(
            "{} invalid argument: {} (expected @@<name>)",
            "Error:".red(),
            name.bold()
        );
        std::process::exit(1);
    }

    let path = layout.alias_dir.join(add::alias_file_name(stem));

    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }

    let file_key = format!("@@{}", stem);
    uninstall_from_file(&path, &file_key, stem, layout);
}

// ---------------------------------------------------------------------------
// 插件卸载核心逻辑
// ---------------------------------------------------------------------------

/// 从文件路径执行卸载。
pub(crate) fn uninstall_from_file(
    path: &Path,
    file_key: &str,
    stem: &str,
    layout: &PathLayout,
) {
    let content = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{} failed to read {}: {}", "Error:".red(), path.display(), e);
        std::process::exit(1);
    });
    let display_label = path.display().to_string();
    let source_dir = path.parent();
    uninstall_from_content(&content, &display_label, file_key, stem, layout, source_dir);
}

/// 从 JSON 内容执行卸载。
///
/// 读取 `$remove` 块的 `pip` / `assets` 布尔标志，为 true 的字段从
/// `$add` 块获取对应数据并卸载/删除。
///
/// `display_label` 用于日志/错误消息中标识来源。
/// `source_file_dir` 用于解析本地 assets 相对路径（文件所在目录）。
fn uninstall_from_content(
    content: &str,
    display_label: &str,
    file_key: &str,
    stem: &str,
    layout: &PathLayout,
    source_file_dir: Option<&Path>,
) {
    let original: serde_json::Value = serde_json::from_str(content).unwrap_or_else(|e| {
        eprintln!(
            "{} invalid JSON in {}: {}",
            "Error:".red(),
            display_label,
            e
        );
        std::process::exit(1);
    });

    // $remove 块：只有声明 $remove 的文件才能被卸载
    let remove_block = original.get("$remove").filter(|v| v.is_object());
    if remove_block.is_none() {
        eprintln!(
            "{} {} has no {} block, nothing to remove.",
            "Note:".yellow(),
            display_label.bold(),
            "$remove".bold()
        );
        return;
    }

    let remove_obj = remove_block.unwrap().as_object().unwrap();

    // 阶段一变量替换（assets 路径可能含变量）
    let mut substituted = original.clone();
    vars::preprocess_vars(&mut substituted);

    let has_pip = remove_obj
        .get("pip")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_assets = remove_obj
        .get("assets")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !has_pip && !has_assets {
        println!(
            "{} {} has no active {} flags, nothing to remove.",
            "Note:".yellow(),
            display_label.bold(),
            "$remove".bold()
        );
        return;
    }

    // 卸载 pip
    if has_pip {
        let packages = substituted
            .get("$add")
            .and_then(|v| v.get("pip"))
            .and_then(aliases::parse_pip_field)
            .unwrap_or_default();

        if packages.is_empty() {
            println!("{} no pip packages to remove", "*".dimmed());
        } else {
            println!(
                "{} Uninstalling pip dependencies from {}",
                "==>".cyan().bold(),
                display_label.bold()
            );
            add_pip::uninstall_packages(&packages, layout, "byk remove");
            println!(
                "{} uninstalled {} package(s)",
                "Successfully".green(),
                packages.len()
            );
        }
    }

    // 删除 assets
    if has_assets {
        let assets_block = substituted
            .get("$add")
            .and_then(|v| v.get("assets"))
            .cloned()
            .filter(|v| v.is_object());

        if let Some(block) = &assets_block {
            let assets_ctx_path = source_file_dir
                .map(|d| d.join(add::alias_file_name(stem)))
                .unwrap_or_else(|| layout.alias_dir.join(add::alias_file_name(stem)));
            assets::delete_assets_block(block, &assets_ctx_path, file_key, layout);
        } else {
            println!("{} no assets to remove", "*".dimmed());
        }
    }

    println!(
        "{} uninstalled {}",
        "Successfully".green(),
        display_label.dimmed()
    );
}