/// `byk add` 子命令逻辑。
///
/// 包含：
/// - 插件安装（委托给 plugins 模块）
/// - 内置功能初始化（npm、pnpm、cache、comp）

use colored::Colorize;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use super::paths::PathLayout;
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
// --add cache（原 --init cache）
// ---------------------------------------------------------------------------

/// 初始化 CLI 家目录及缓存目录结构。
///
/// 创建 ~/.byk/ 及其子目录（alias、cache、logs），
/// 使别名系统和其他缓存功能可用。幂等：已存在则跳过。
pub fn init_cache(layout: &PathLayout) {
    ensure_common_dirs(layout);
    ensure_dir(&layout.logs_dir, "logs");
}

// ---------------------------------------------------------------------------
// --add npm / --add pnpm
// ---------------------------------------------------------------------------

/// 初始化 npm 命令功能。
pub fn init_npm(layout: &PathLayout) {
    init_node_pm(layout, "npm");
}

/// 初始化 pnpm 命令功能。
pub fn init_pnpm(layout: &PathLayout) {
    init_node_pm(layout, "pnpm");
}

// ---------------------------------------------------------------------------
// --add py-v
// ---------------------------------------------------------------------------

/// 获取 venv 内 bin 目录名（Windows: Scripts, Unix: bin）。
#[cfg(windows)]
const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
const VENV_BIN: &str = "bin";

/// 初始化 Python 虚拟环境及 pip 别名。
///
/// 创建 ~/.byk/venv/（不存在时），写入/更新 alias/py.byk.json。
/// 仅创建 venv 和别名，不安装任何 Python 包。
pub fn init_py_v(layout: &PathLayout) {
    let venv_dir = &layout.venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");

    #[cfg(windows)]
    let sys_python = "python";
    #[cfg(not(windows))]
    let sys_python = "python3";

    ensure_common_dirs(layout);

    // ① 创建 venv（不存在时）
    if venv_dir.exists() {
        println!("{}", "venv/ already exists, skipping creation.".dimmed());
    } else {
        println!("{}", "Creating Python virtual environment...".dimmed());
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

    // ② 写入/更新别名模板
    let template = serde_json::json!({
        "$cwd": format!("../venv/{}/", VENV_BIN),
        "pi": "./pip install",
        "pu": "./pip uninstall",
        "pl": "./pip list",
    });
    let template_str = serde_json::to_string_pretty(&template).unwrap_or_default();
    write_file(&alias_path, &template_str, "alias/py.byk.json");

    println!();
    println!(
        "{} {}",
        "Python environment ready.".green(),
        "(venv)".dimmed()
    );
    println!("  Install packages:  byk pi <pkg>");
    println!("  Remove packages:   byk pu <pkg>");
    println!("  List packages:     byk pl");
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 创建目录，打印操作信息。
fn ensure_dir(path: &Path, label: &str) {
    if path.exists() {
        println!("  {} {} {}", "+".dimmed(), label.dimmed(), "(exists)".dimmed());
    } else {
        fs::create_dir_all(path).unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", label, e);
        });
        println!("  {} {}", "+".green(), label.dimmed());
    }
}

/// 确保公共目录存在：root、alias、cache、plugins。
fn ensure_common_dirs(layout: &PathLayout) {
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(&layout.alias_dir, "alias");
    ensure_dir(&layout.cache_dir, "cache");
    ensure_dir(&layout.plugins_dir, "plugins");
}

/// 写入文件内容。
fn write_file(path: &Path, content: &str, label: &str) {
    if path.exists() {
        println!(
            "  {} {} {}",
            "*".dimmed(),
            label.dimmed(),
            "(updated)".dimmed()
        );
    } else {
        println!("  {} {}", "+".green(), label.dimmed());
    }
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("Failed to create {}: {}", label, e);
    });
}

fn init_node_pm(layout: &PathLayout, pm: &str) {
    let template = serde_json::json!({
        "$cwd": "../node-pkgs/",
        "ni": format!("{} i", pm),
        "nu": format!("{} uni", pm),
    });
    let node_pkgs_dir = &layout.node_pkgs_dir;
    let alias_path = layout.alias_dir.join("node.byk.json");
    let cache_path = layout.cache_dir.join("node-pkg.json");

    let has_existing =
        node_pkgs_dir.exists() || alias_path.exists() || cache_path.exists();

    if has_existing {
        println!();
        println!(
            "{}",
            "Existing node package data detected:".yellow()
        );
        if node_pkgs_dir.exists() {
            println!("  {}", layout.node_pkgs_dir.display().to_string().dimmed());
        }
        if alias_path.exists() {
            println!("  {}", alias_path.display().to_string().dimmed());
        }
        if cache_path.exists() {
            println!("  {}", cache_path.display().to_string().dimmed());
        }
        println!();
        println!(
            "{}",
            "This will permanently remove ALL existing node packages".red().bold()
        );
        println!(
            "{}",
            "aliases and cache. This action cannot be undone.".red().bold()
        );
        println!();

        if !shell::prompt_confirm("node-pkgs") {
            return;
        }

        // 删除旧数据
        if node_pkgs_dir.exists() {
            let _ = fs::remove_dir_all(node_pkgs_dir);
            println!("  {} node-pkgs {}", "-".dimmed(), "(removed)".dimmed());
        }
        if alias_path.exists() {
            let _ = fs::remove_file(&alias_path);
            println!("  {} alias/node.byk.json {}", "-".dimmed(), "(removed)".dimmed());
        }
        if cache_path.exists() {
            let _ = fs::remove_file(&cache_path);
            println!("  {} cache/node-pkg.json {}", "-".dimmed(), "(removed)".dimmed());
        }
    }

    // 确保目录存在
    ensure_common_dirs(layout);
    ensure_dir(node_pkgs_dir, "node-pkgs");

    // 写入别名模板
    let template_str = serde_json::to_string_pretty(&template).unwrap_or_default();
    write_file(&alias_path, &template_str, "alias/node.byk.json");

    println!();
    println!(
        "{} ({})",
        "Node package support initialized.".green(),
        pm.dimmed()
    );
    println!("  Install packages:  {} <pkg>", "byk ni".dimmed());
    println!("  Remove packages:   {} <pkg>", "byk nu".dimmed());
}