/// `byk init` 子命令逻辑。
///
/// 用户手动按需初始化 CLI 功能，不自动创建任何配置。
/// 模板文件位于 src/templates/，通过 include_str! 编译期嵌入。

use colored::Colorize;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use super::paths::PathLayout;
use crate::utils::shell;

// ---------------------------------------------------------------------------
// 模板（编译期嵌入）
// ---------------------------------------------------------------------------

const NPM_TEMPLATE: &str = include_str!("../templates/npm.byk.json");
const PNPM_TEMPLATE: &str = include_str!("../templates/pnpm.byk.json");

// ---------------------------------------------------------------------------
// init 帮助
// ---------------------------------------------------------------------------

/// 渲染 init 帮助信息（无子参数时显示）。
pub fn render_init_help() {
    println!();
    print!("{}", "Usage:".green().bold());
    println!("{}", " byk init [feature]".bold());
    println!();
    println!("{}", "Feature:".green().bold());
    println!(
        "  {:<8} {}",
        "npm".cyan().bold(),
        "Initialize with npm (node-pkgs)"
    );
    println!(
        "  {:<8} {}",
        "pnpm".cyan().bold(),
        "Initialize with pnpm (node-pkgs)"
    );
    println!(
        "  {:<8} {}",
        "comp".cyan().bold(),
        "Initialize shell completion (zsh/bash)"
    );
    println!(
        "  {:<8} {}",
        "cache".cyan().bold(),
        "Initialize CLI home & cache directories"
    );
    println!(
        "  {:<8} {}",
        "py".cyan().bold(),
        "Enable Python plugin system (global)"
    );
    println!(
        "  {:<8} {}",
        "py-v".cyan().bold(),
        "Initialize Python venv & aliases (recommended)"
    );
    println!();
}

// ---------------------------------------------------------------------------
// --init comp
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
// --init cache
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
// --init npm / --init pnpm
// ---------------------------------------------------------------------------

/// 初始化 npm 命令功能。
pub fn init_npm(layout: &PathLayout) {
    init_node_pkgs(layout, "npm", "ni", "nu", NPM_TEMPLATE);
}

/// 初始化 pnpm 命令功能。
pub fn init_pnpm(layout: &PathLayout) {
    init_node_pkgs(layout, "pnpm", "ni", "nu", PNPM_TEMPLATE);
}

// ---------------------------------------------------------------------------
// --init py (全局)
// ---------------------------------------------------------------------------

/// 初始化全局 Python 插件系统（系统级，不建 venv）。
///
/// 检查 bykpy 是否已安装 → 未安装则 pip install →
/// 运行 bykpy --scan-plugins 写入 cache/app.json。
/// 二次 init 仅重扫插件，不删除数据。
pub fn init_py_global(layout: &PathLayout) {
    let cache_path = layout.cache_dir.join("app.json");
    let py_exe = crate::core::plugins::get_python_executable(&layout.cache_dir);

    ensure_common_dirs(layout);

    // ① 检查 bykpy 是否已安装
    let check = Command::new(&py_exe)
        .args(["-c", "import bykpy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let has_bykpy = check.map(|s| s.success()).unwrap_or(false);

    if has_bykpy {
        println!("{}", "bykpy already installed, skipping pip install.".dimmed());
    } else {
        println!("{}", "Installing bykpy (global)...".dimmed());
        let status = Command::new(&py_exe)
            .args(["-m", "pip", "install", "bykpy"])
            .status();

        match status {
            Ok(s) if s.success() => {
                println!("  {} bykpy {}", "+".green(), "(installed)".dimmed());
            }
            Ok(s) => {
                eprintln!(
                    "{} pip install bykpy failed with code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1)
                );
                return;
            }
            Err(e) => {
                eprintln!("{} Failed to run pip: {}", "Error:".red(), e);
                return;
            }
        }
    }

    // ② 扫描插件，生成/更新 cache/app.json
    println!("{}", "Scanning plugins...".dimmed());
    let status = Command::new(&py_exe)
        .args(["-m", "bykpy", "--scan-plugins"])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!(
                "  {} cache/app.json {}",
                "+".green(),
                if cache_path.exists() { "(updated)" } else { "(created)" }.dimmed()
            );
        }
        Ok(s) => {
            eprintln!(
                "{} plugin scan failed with code {}",
                "Error:".red(),
                s.code().unwrap_or(1)
            );
            return;
        }
        Err(e) => {
            eprintln!("{} Failed to run bykpy scan: {}", "Error:".red(), e);
            return;
        }
    }

    println!();
    println!(
        "{} {}",
        "Python plugin system enabled (global).".green(),
        "(not recommended)".yellow()
    );
    println!(
        "  For isolated env with pip aliases: {}",
        "byk --init py-v".dimmed()
    );
}

// ---------------------------------------------------------------------------
// --init py-v
// ---------------------------------------------------------------------------

/// 获取 venv 内 bin 目录名（Windows: Scripts, Unix: bin）。
#[cfg(windows)]
const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
const VENV_BIN: &str = "bin";

/// 获取 venv 内 Python 可执行文件名。
#[cfg(windows)]
const PYTHON_BIN: &str = "python.exe";
#[cfg(not(windows))]
const PYTHON_BIN: &str = "python";

/// 初始化 Python 虚拟环境及别名（推荐方式）。
///
/// 创建 ~/.byk/venv/（不存在时），安装 bykpy（未安装时），
/// 扫描插件更新 cache/app.json，写入/更新 alias/py.byk.json。
/// 纯切换逻辑，不删除已有数据。
pub fn init_py(layout: &PathLayout) {
    let venv_dir = &layout.venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");
    let py_exe = crate::core::plugins::get_python_executable(&layout.cache_dir);

    ensure_common_dirs(layout);

    // ① 创建 venv（不存在时）
    if venv_dir.exists() {
        println!("{}", "venv/ already exists, skipping creation.".dimmed());
    } else {
        println!("{}", "Creating Python virtual environment...".dimmed());
        let status = Command::new(&py_exe)
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

    // ② 检查 bykpy 是否已安装在 venv 中
    let venv_python = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    let check = Command::new(&venv_python)
        .args(["-c", "import bykpy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let has_bykpy = check.map(|s| s.success()).unwrap_or(false);

    if has_bykpy {
        println!("{}", "bykpy already installed, skipping pip install.".dimmed());
    } else {
        let pip = venv_dir.join(VENV_BIN).join(if cfg!(windows) { "pip.exe" } else { "pip" });
        println!("{}", "Installing bykpy...".dimmed());
        let status = Command::new(&pip)
            .args(["install", "bykpy"])
            .status();

        match status {
            Ok(s) if s.success() => {
                println!("  {} bykpy {}", "+".green(), "(installed)".dimmed());
            }
            Ok(s) => {
                eprintln!(
                    "{} pip install bykpy failed with code {}",
                    "Error:".red(),
                    s.code().unwrap_or(1)
                );
                return;
            }
            Err(e) => {
                eprintln!("{} Failed to run pip: {}", "Error:".red(), e);
                return;
            }
        }
    }

    // ③ 扫描插件，生成/更新 cache/app.json
    println!("{}", "Scanning plugins...".dimmed());
    let status = Command::new(&venv_python)
        .args(["-m", "bykpy", "--scan-plugins"])
        .status();

    let cache_path = layout.cache_dir.join("app.json");
    match status {
        Ok(s) if s.success() => {
            println!(
                "  {} cache/app.json {}",
                "+".green(),
                if cache_path.exists() { "(updated)" } else { "(created)" }.dimmed()
            );
        }
        Ok(s) => {
            eprintln!(
                "{} plugin scan failed with code {}",
                "Error:".red(),
                s.code().unwrap_or(1)
            );
            return;
        }
        Err(e) => {
            eprintln!("{} Failed to run bykpy scan: {}", "Error:".red(), e);
            return;
        }
    }

    // ④ 写入/更新别名模板
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

/// 确保公共目录存在：root、alias、cache。
fn ensure_common_dirs(layout: &PathLayout) {
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(&layout.alias_dir, "alias");
    ensure_dir(&layout.cache_dir, "cache");
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

// ---------------------------------------------------------------------------
// 共享初始化逻辑
// ---------------------------------------------------------------------------

fn init_node_pkgs(
    layout: &PathLayout,
    pm: &str,
    install_alias: &str,
    remove_alias: &str,
    template: &str,
) {
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
            "This will permanently remove ALL existing node packages,".red().bold()
        );
        println!(
            "{}",
            "aliases, and cache. This action cannot be undone.".red().bold()
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
    write_file(&alias_path, template, "alias/node.byk.json");

    println!();
    println!(
        "{} ({})",
        "Node package support initialized.".green(),
        pm.dimmed(),
    );
    println!("  Install packages:  byk {} <pkg>", install_alias.dimmed());
    println!("  Remove packages:   byk {} <pkg>", remove_alias.dimmed());
}
