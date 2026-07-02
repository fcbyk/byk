//! `byk remove` 子命令逻辑。
//!
//! 删除 `byk add` 创建的持久化数据（venv、缓存、别名等），
//! 并提供插件卸载功能。

use std::collections::HashSet;
use colored::Colorize;
use std::fs;
use std::process::exit;

use super::paths::PathLayout;
use super::plugins::state::{empty_cmd_state, load_pkg_state};
use super::plugins::types::*;
use crate::utils::{json_io, shell};

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
// remove py-v
// ---------------------------------------------------------------------------

/// 删除 Python 虚拟环境及别名。
///
/// 删除 ~/.byk/py-venv/ 整个目录和 alias/py.byk.json。
/// 无论 py-v 还是 uv 模式，统一用此函数删除。
pub fn rm_py_v(layout: &PathLayout) {
    let py_venv_dir = &layout.py_venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");

    if !py_venv_dir.exists() && !alias_path.exists() {
        println!("{}", "Python venv not found, nothing to remove.".dimmed());
        return;
    }

    println!();
    println!("{}", "This will remove:".yellow());
    if py_venv_dir.exists() {
        println!("  {}", py_venv_dir.display().to_string().dimmed());
    }
    if alias_path.exists() {
        println!("  {}", alias_path.display().to_string().dimmed());
    }
    println!();

    if !shell::prompt_confirm("py-venv") {
        return;
    }

    if py_venv_dir.exists() {
        let _ = fs::remove_dir_all(py_venv_dir);
        println!("  {} py-venv/ {}", "-".red(), "(removed)".dimmed());
    }
    if alias_path.exists() {
        let _ = fs::remove_file(&alias_path);
        println!("  {} alias/py.byk.json {}", "-".red(), "(removed)".dimmed());
    }

    println!();
    println!("{}", "Python venv removed.".green());
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
// 卸载插件
// ---------------------------------------------------------------------------

/// 卸载插件。
///
/// 流程：
/// 1. 读取 plugins.pkg.json，在 packages 中查找 key
/// 2. 收集其他插件共享的 pip 包名（跨插件安全检查）
/// 3. 卸载 pip 包（跳过其他插件也依赖的包）
/// 4. 删除下载的脚本文件和二进制文件
/// 5. 从 plugins.cmd.json 删除该插件的所有命令
/// 6. 从 plugins.pkg.json 删除该 key
/// 7. 写回
///
/// 注意：$pip 全局共享依赖不在任何插件的 PkgEntry 中，永远不会被卸载。
pub fn uninstall_plugin(key: &str, layout: &PathLayout) {
    // 1. 检查 venv（与 install_plugin 一致：pip 或 python 可执行文件存在即可）
    let pip = layout.venv_dir.join(VENV_BIN).join("pip");
    let py_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    if !pip.is_file() && !py_exe.is_file() {
        eprintln!(
            "{} Python venv not found. Run {} first.",
            "Error:".red(),
            "`byk add <user/repo>`".bold(),
        );
        exit(1);
    }

    let is_uv = layout.py_venv_dir.join("pyproject.toml").is_file();

    // 2. 读取状态
    let cmd_file = layout.plugins_dir.join("plugins.cmd.json");
    let pkg_file = layout.plugins_dir.join("plugins.pkg.json");
    let scripts_dir = layout.plugins_dir.join("scripts");

    let mut cmd_state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    let mut pkg_state: PkgState = load_pkg_state(&layout.plugins_dir);

    let pkg = match pkg_state.get(key) {
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

    println!(
        "{} Uninstalling plugin: {}",
        "==>".cyan().bold(),
        key.bold(),
    );

    // 收集其他插件也依赖的 pip 包名（跨插件共享检查）
    let shared_pkgs: HashSet<&str> = pkg_state
        .iter()
        .filter(|(k, _)| k.as_str() != key)
        .flat_map(|(_, entry)| {
            entry.pip.iter().flatten()
        })
        .map(|p| extract_pkg_name(p))
        .filter(|n| !n.is_empty())
        .collect();

    // 3. 卸载 Python 包（跳过其他插件也依赖的包）
    if let Some(ref pip_list) = pkg.pip {
        println!("{} pip", "==>".cyan().bold());
        for item in pip_list {
            let name = extract_pkg_name(item);
            if name.is_empty() {
                continue;
            }
            // 跨插件共享检查
            if shared_pkgs.contains(name) {
                println!(
                    "{}",
                    format!("Skipping {} (also used by other plugins)", name).dimmed()
                );
                println!("{} {} {}", "*".dimmed(), name.bold(), "(shared)".dimmed());
                continue;
            }
            println!(
                "{}",
                format!("Uninstalling pip package: {}", name).dimmed()
            );
            if is_uv {
                let status = std::process::Command::new("uv")
                    .args(["remove", name])
                    .current_dir(&layout.py_venv_dir)
                    .status();
                match status {
                    Ok(s) if s.success() => {}
                    Ok(s) => {
                        eprintln!(
                            "{} uv remove {} failed (exit {})",
                            "Warning:".yellow(),
                            name,
                            s.code().unwrap_or(1),
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "{} Failed to run uv remove {}: {}",
                            "Warning:".yellow(),
                            name,
                            e,
                        );
                    }
                }
            } else {
                let status = std::process::Command::new(&pip)
                    .args(["uninstall", "-y", name])
                    .status();
                match status {
                    Ok(s) if s.success() => {}
                    Ok(s) => {
                        eprintln!(
                            "{} pip uninstall {} failed (exit {})",
                            "Warning:".yellow(),
                            name,
                            s.code().unwrap_or(1),
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "{} Failed to run pip uninstall {}: {}",
                            "Warning:".yellow(),
                            name,
                            e,
                        );
                    }
                }
            }
            println!("{} {}", "-".red(), name.bold());
        }
    }

    // 4. 删除脚本文件
    if !pkg.scripts.is_empty() {
        println!("{} scripts", "==>".cyan().bold());
    }
    for script in &pkg.scripts {
        let script_path = scripts_dir.join(script);
        if script_path.exists() {
            println!(
                "{}",
                format!("Deleting {}", script_path.display()).dimmed()
            );
            if let Err(e) = fs::remove_file(&script_path) {
                eprintln!(
                    "{} Warning: failed to delete script {}: {}",
                    "Warning:".yellow(),
                    script_path.display(),
                    e,
                );
            }
            println!("{} {}", "-".red(), script.bold());
        }
    }

    // 4b. 删除二进制文件
    let bin_dir = layout.plugins_dir.join("bin");
    if !pkg.bins.is_empty() {
        println!("{} bin", "==>".cyan().bold());
    }
    for bin in &pkg.bins {
        let bin_path = bin_dir.join(bin);
        if bin_path.exists() {
            println!(
                "{}",
                format!("Deleting {}", bin_path.display()).dimmed()
            );
            if let Err(e) = fs::remove_file(&bin_path) {
                eprintln!(
                    "{} Warning: failed to delete binary {}: {}",
                    "Warning:".yellow(),
                    bin_path.display(),
                    e,
                );
            }
            println!("{} {}", "-".red(), bin.bold());
        }
    }

    // 4c. 删除 bin-tar 解压出的目录
    if !pkg.bins_tar.is_empty() {
        println!("{} bin-tar", "==>".cyan().bold());
    }
    for key in &pkg.bins_tar {
        let extract_dir = bin_dir.join(key);
        if extract_dir.exists() {
            println!(
                "{}",
                format!("Deleting {}", extract_dir.display()).dimmed()
            );
            let _ = fs::remove_dir_all(&extract_dir);
            println!("{} {}", "-".red(), key.bold());
        }
    }

    // 4d. 删除工作目录下载的文件/目录
    if !pkg.workdir.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        println!("{} workdir", "==>".cyan().bold());
        println!("  {}", format!("→ {}", cwd.display()).dimmed());
        for name in &pkg.workdir {
            let path = cwd.join(name);
            if path.exists() {
                let is_dir = path.is_dir();
                println!(
                    "{}",
                    format!("Deleting {}", path.display()).dimmed()
                );
                if is_dir {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
                println!("{} {}", "-".red(), name.bold());
            }
        }
    }

    // 5. 删除 commands
    if !pkg.commands.is_empty() {
        println!("{} commands", "==>".cyan().bold());
    }
    for cmd_name in &pkg.commands {
        println!(
            "{}",
            format!("Removing command: {}", cmd_name).dimmed()
        );
        println!(
            "  {}",
            format!("from {}", cmd_file.display()).dimmed()
        );
        cmd_state.commands.remove(cmd_name);
        println!("{} {}", "-".red(), cmd_name.bold());
    }

    // 6. 删除 packages 条目
    pkg_state.remove(key);

    // 7. 写回
    json_io::write_json(&cmd_file, &cmd_state);
    json_io::write_json(&pkg_file, &pkg_state);

    println!(
        "{} uninstalled {}",
        "Successfully".green().bold(),
        key.bold(),
    );
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 从 pip 安装字符串中提取包名。
/// - "name @ url" → "name"
/// - "name" → "name"
/// - "https://..." → ""（纯 URL，无法卸载）
fn extract_pkg_name(raw: &str) -> &str {
    if let Some(pos) = raw.find(" @ ") {
        raw[..pos].trim()
    } else if raw.starts_with("http://") || raw.starts_with("https://") {
        ""
    } else {
        raw.trim()
    }
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== extract_pkg_name ====================

    #[test]
    fn extract_pkg_name_simple() {
        assert_eq!(extract_pkg_name("requests"), "requests");
    }

    #[test]
    fn extract_pkg_name_with_version() {
        assert_eq!(extract_pkg_name("requests>=2.0"), "requests>=2.0");
    }

    #[test]
    fn extract_pkg_name_with_at_url() {
        // "name @ url" format → extract name
        assert_eq!(extract_pkg_name("mypkg @ https://example.com/whl"), "mypkg");
    }

    #[test]
    fn extract_pkg_name_pure_url_returns_empty() {
        assert_eq!(extract_pkg_name("https://example.com/pkg.whl"), "");
    }

    #[test]
    fn extract_pkg_name_pure_http_url_returns_empty() {
        assert_eq!(extract_pkg_name("http://example.com/pkg.whl"), "");
    }

    #[test]
    fn extract_pkg_name_trim_whitespace() {
        assert_eq!(extract_pkg_name("  requests  "), "requests");
    }

    #[test]
    fn extract_pkg_name_at_url_with_extra_spaces() {
        // "name @ url" split on " @ " works even with extra spaces
        assert_eq!(extract_pkg_name("mypkg @ https://x.com/pkg"), "mypkg");
    }

    #[test]
    fn extract_pkg_name_empty_string() {
        assert_eq!(extract_pkg_name(""), "");
    }

    // ==================== remove_if_exists ====================

    use std::fs;

    #[test]
    fn remove_if_exists_deletes_file() {
        let tmp = std::env::temp_dir().join("fcbyk_test_rm_exists");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("test_file.txt");
        fs::write(&file, "data").unwrap();

        remove_if_exists(&file, "test_file.txt");
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_if_exists_deletes_dir() {
        let tmp = std::env::temp_dir().join("fcbyk_test_rm_dir");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let subdir = tmp.join("subdir");
        fs::create_dir_all(&subdir).unwrap();

        remove_if_exists(&subdir, "subdir");
        assert!(!subdir.exists());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_if_exists_noop_for_missing() {
        let tmp = std::env::temp_dir().join("fcbyk_test_rm_noop");
        let nonexistent = tmp.join("nothing_here");

        remove_if_exists(&nonexistent, "nothing_here");
        // should not panic
    }

    // ==================== print_if_exists ====================

    #[test]
    fn print_if_exists_when_file_exists() {
        let tmp = std::env::temp_dir().join("fcbyk_test_print_exists");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("existing.txt");
        fs::write(&file, "data").unwrap();

        print_if_exists(&file);
        // should not panic; output is to stdout
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn print_if_exists_when_file_missing() {
        let tmp = std::env::temp_dir().join("fcbyk_test_print_missing");
        let nonexistent = tmp.join("missing.txt");

        print_if_exists(&nonexistent);
        // should not panic
    }
}