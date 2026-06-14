/// `byk remove` 子命令逻辑。
///
/// 删除 `byk init` 创建的持久化数据（venv、缓存、别名等），
/// 并提供 byk 包卸载指引。

use colored::Colorize;
use std::fs;
use std::process::Command;

use super::paths::PathLayout;
use crate::utils::shell;

// ---------------------------------------------------------------------------
// remove 帮助
// ---------------------------------------------------------------------------

/// 渲染 remove 帮助信息（无子参数时显示）。
pub fn render_remove_help() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk remove [feature]".bold());
    println!();
    println!("{}", "Feature:".green().bold());
    println!(
        "  {:<8} {}",
        "py".cyan().bold(),
        "Remove Python plugin cache (keep byk packages)"
    );
    println!(
        "  {:<8} {}",
        "py-v".cyan().bold(),
        "Remove venv, aliases, and plugin cache"
    );
    println!(
        "  {:<8} {}",
        "comp".cyan().bold(),
        "Remove shell completion (zsh/bash)"
    );
    println!(
        "  {:<8} {}",
        "node".cyan().bold(),
        "Remove node-pkgs, aliases, and cache"
    );
    println!(
        "  {:<8} {}",
        "all".cyan().bold(),
        "Remove everything (~/.byk/ + shell completion)"
    );
    println!();
}

// ---------------------------------------------------------------------------
// remove py
// ---------------------------------------------------------------------------

/// 删除全局 Python 插件缓存。
///
/// 删除 cache/app.json，检索所有 byk 开头的 pip 包，
/// 提供一键卸载命令。
pub fn rm_py(layout: &PathLayout) {
    let cache_path = layout.cache_dir.join("app.json");

    // 检索 byk 包（无条件）
    let py_exe = crate::core::plugins::get_python_executable(&layout.cache_dir);
    let byk_packages = find_byk_packages(&py_exe);

    // 删除缓存（存在时）
    if cache_path.exists() {
        let _ = fs::remove_file(&cache_path);
        println!("  {} cache/app.json {}", "-".red(), "(removed)".dimmed());
    } else {
        println!("{}", "Plugin cache not found, skipped.".dimmed());
    }

    print_byk_packages(byk_packages);
}

// ---------------------------------------------------------------------------
// remove py-v
// ---------------------------------------------------------------------------

/// 删除 Python venv 环境及所有关联数据。
///
/// 删除 ~/.byk/venv/、alias/py.byk.json、cache/app.json。
/// venv 整体删除，无需额外提示包卸载（目录已不存在）。
pub fn rm_py_v(layout: &PathLayout) {
    let venv_dir = &layout.venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");
    let cache_path = layout.cache_dir.join("app.json");

    if !venv_dir.exists() && !alias_path.exists() && !cache_path.exists() {
        println!(
            "{}",
            "Python venv not found. Nothing to remove.".dimmed()
        );
        return;
    }

    // 现有数据提示
    println!();
    println!("{}", "This will remove:".yellow());
    print_if_exists(venv_dir);
    print_if_exists(&alias_path);
    print_if_exists(&cache_path);
    println!();

    if !shell::prompt_confirm("py-v") {
        return;
    }

    // 删除
    remove_if_exists(venv_dir, "venv/");
    remove_if_exists(&alias_path, "alias/py.byk.json");
    remove_if_exists(&cache_path, "cache/app.json");

    println!();
    println!("{}", "Python venv removed.".green());
}

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
// remove node
// ---------------------------------------------------------------------------

/// 删除 node-pkgs 环境（覆盖 npm 和 pnpm）。
pub fn rm_node(layout: &PathLayout) {
    rm_node_pkgs(layout, "node");
}

fn rm_node_pkgs(layout: &PathLayout, pm: &str) {
    let node_pkgs_dir = &layout.node_pkgs_dir;
    let alias_path = layout.alias_dir.join("node.byk.json");
    let cache_path = layout.cache_dir.join("node-pkg.json");

    if !node_pkgs_dir.exists() && !alias_path.exists() && !cache_path.exists() {
        println!(
            "{} {}",
            "Node package data not found.".dimmed(),
            format!("({})", pm).dimmed()
        );
        return;
    }

    println!();
    println!("{} ({})", "This will remove:".yellow(), pm.dimmed());
    print_if_exists(node_pkgs_dir);
    print_if_exists(&alias_path);
    print_if_exists(&cache_path);
    println!();

    if !shell::prompt_confirm("node-pkgs") {
        return;
    }

    remove_if_exists(node_pkgs_dir, "node-pkgs/");
    remove_if_exists(&alias_path, "alias/node.byk.json");
    remove_if_exists(&cache_path, "cache/node-pkg.json");

    println!();
    println!(
        "{} ({})",
        "Node packages removed.".green(),
        pm.dimmed()
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
                    "(all subdirectories: venv, aliases, caches, logs, node-pkgs, bin)".dimmed()
                );
            }
        }
        if has_comp {
            if let Some(p) = shell::rc_path() {
                println!("  {}", p.display().to_string().dimmed());
                println!("    {}", "(byk completion line)".dimmed());
            }
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
        if has_comp {
            if let Some(p) = shell::rc_path() {
                let content = fs::read_to_string(&p).unwrap_or_default();
                let new_content = shell::strip_completion_lines(&content);
                let _ = fs::write(&p, new_content);
                println!(
                    "  {} shell completion in {}",
                    "-".red(),
                    p.display().to_string().dimmed()
                );
            }
        }
        println!();
        println!("{}", "Everything removed.".green());
    }

    // ---------- 4. 全局 byk 包检测（与 ~/.byk/ 是否存在无关） ----------
    let py_exe = crate::core::plugins::get_python_executable(&layout.cache_dir);
    let byk_packages = find_byk_packages(&py_exe);
    print_byk_packages_with_label(byk_packages, "Global");
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 查找所有 byk 开头的 pip 包，返回一键卸载命令字符串。
fn find_byk_packages(python_exe: &str) -> Option<String> {
    let output = Command::new(python_exe)
        .args(["-m", "pip", "list", "--format=json"])
        .output()
        .ok()?;

    let packages: Vec<String> = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()?
        .as_array()?
        .iter()
        .filter_map(|p| p.get("name")?.as_str().map(String::from))
        .filter(|name| name.starts_with("byk"))
        .collect();

    if packages.is_empty() {
        None
    } else {
        Some(format!("pip uninstall {}", packages.join(" ")))
    }
}

/// 打印路径（存在时），dimmed 格式。
fn print_if_exists(path: &std::path::Path) {
    if path.exists() {
        println!("  {}", path.display().to_string().dimmed());
    }
}

/// 删除路径（存在时），打印 `- path (removed)`。
fn remove_if_exists(path: &std::path::Path, label: &str) {
    if path.exists() {
        let is_dir = path.is_dir();
        let _ = if is_dir {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        println!("  {} {} {}", "-".red(), label.dimmed(), "(removed)".dimmed());
    }
}

/// 使用 rm_py 的样式输出 byk 包卸载命令。
fn print_byk_packages(packages: Option<String>) {
    if let Some(cmd) = packages {
        println!();
        println!(
            "{} {}",
            "byk-related packages detected:".yellow(),
            "(copy to uninstall all)".dimmed()
        );
        println!("  {}", cmd.white());
    } else {
        println!(
            "  {}",
            "No byk packages found.".dimmed()
        );
    }
}

/// 使用 rm_all 的样式输出全局 byk 包卸载命令。
fn print_byk_packages_with_label(packages: Option<String>, label: &str) {
    if let Some(cmd) = packages {
        println!();
        println!(
            "{} {}",
            format!("{} byk-related packages detected:", label).yellow(),
            "(copy to uninstall)".dimmed()
        );
        println!("  {}", cmd.white());
    } else {
        println!();
        println!(
            "  {}",
            format!("No {} byk packages found.", label.to_lowercase()).dimmed()
        );
    }
}
