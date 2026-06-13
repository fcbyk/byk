/// --init 选项逻辑。
///
/// 用户手动按需初始化 CLI 功能，不自动创建任何配置。
/// 模板文件位于 src/templates/，通过 include_str! 编译期嵌入。

use colored::Colorize;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

use super::paths::PathLayout;

// ---------------------------------------------------------------------------
// 模板（编译期嵌入）
// ---------------------------------------------------------------------------

const NPM_TEMPLATE: &str = include_str!("../templates/npm.byk.json");
const PNPM_TEMPLATE: &str = include_str!("../templates/pnpm.byk.json");

// ---------------------------------------------------------------------------
// --init 帮助
// ---------------------------------------------------------------------------

/// 渲染 --init 帮助信息（无子参数时显示）。
pub fn render_init_help() {
    let title = "byk --init <feature>";
    println!("{}", title.bold());
    println!();
    println!(
        "  {:<8} {}",
        "npm".yellow(),
        "Initialize with npm (node-pkgs)".dimmed()
    );
    println!(
        "  {:<8} {}",
        "pnpm".yellow(),
        "Initialize with pnpm (node-pkgs)".dimmed()
    );
    println!(
        "  {:<8} {}",
        "comp".yellow(),
        "Initialize shell completion (zsh/bash)".dimmed()
    );
    println!(
        "  {:<8} {}",
        "py".yellow(),
        "Enable Python plugin system (global)".dimmed()
    );
    println!(
        "  {:<8} {}",
        "py-v".yellow(),
        "Initialize Python venv & aliases (recommended)".dimmed()
    );
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
    let shell = env::var("SHELL").unwrap_or_default();

    let (rc_filename, shell_name) = if shell.ends_with("/zsh") {
        (".zshrc", "zsh")
    } else if shell.ends_with("/bash") {
        (".bashrc", "bash")
    } else {
        eprintln!(
            "{} {} {}",
            "Unsupported shell:".red(),
            shell.dimmed(),
            "(supported: zsh, bash)".dimmed()
        );
        return;
    };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("{}", "Cannot determine home directory.".red());
            return;
        }
    };
    let rc_path = home.join(rc_filename);

    let line = format!(
        "if command -v byk >/dev/null 2>&1; then source <(byk completion {}); fi",
        shell_name
    );

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
// --init npm / --init pnpm
// ---------------------------------------------------------------------------

/// 初始化 npm 命令功能。
pub fn init_npm(layout: &PathLayout) {
    init_node_pkgs(layout, "npm", "i", "uni", NPM_TEMPLATE);
}

/// 初始化 pnpm 命令功能。
pub fn init_pnpm(layout: &PathLayout) {
    init_node_pkgs(layout, "pnpm", "add", "remove", PNPM_TEMPLATE);
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

    #[cfg(windows)]
    let python = "python";
    #[cfg(not(windows))]
    let python = "python3";

    // 确保目录存在
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(&layout.cache_dir, "cache");

    // ① 检查 bykpy 是否已安装
    let check = Command::new(python)
        .args(["-c", "import bykpy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let has_bykpy = check.map(|s| s.success()).unwrap_or(false);

    if has_bykpy {
        println!("{}", "bykpy already installed, skipping pip install.".dimmed());
    } else {
        println!("{}", "Installing bykpy (global)...".dimmed());
        let status = Command::new(python)
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
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{} Failed to run pip: {}", "Error:".red(), e);
                std::process::exit(1);
            }
        }
    }

    // ② 扫描插件，生成/更新 cache/app.json
    println!("{}", "Scanning plugins...".dimmed());
    let status = Command::new(python)
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
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{} Failed to run bykpy scan: {}", "Error:".red(), e);
            std::process::exit(1);
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

/// 初始化 Python 虚拟环境及别名（推荐方式）。
///
/// 创建 ~/.byk/venv/（不存在时），安装 bykpy（未安装时），
/// 扫描插件更新 cache/app.json，写入/更新 alias/py.byk.json。
/// 纯切换逻辑，不删除已有数据。
pub fn init_py(layout: &PathLayout) {
    let venv_dir = &layout.venv_dir;
    let alias_path = layout.alias_dir.join("py.byk.json");

    #[cfg(windows)]
    let python = "python";
    #[cfg(not(windows))]
    let python = "python3";

    #[cfg(windows)]
    let bin_dir = "Scripts";
    #[cfg(not(windows))]
    let bin_dir = "bin";

    // 确保目录存在
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(&layout.alias_dir, "alias");
    ensure_dir(&layout.cache_dir, "cache");

    // ① 创建 venv（不存在时）
    if venv_dir.exists() {
        println!("{}", "venv/ already exists, skipping creation.".dimmed());
    } else {
        println!("{}", "Creating Python virtual environment...".dimmed());
        let status = Command::new(python)
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
    let py = venv_dir.join(bin_dir).join(if cfg!(windows) { "python.exe" } else { "python" });
    let check = Command::new(&py)
        .args(["-c", "import bykpy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let has_bykpy = check.map(|s| s.success()).unwrap_or(false);

    if has_bykpy {
        println!("{}", "bykpy already installed, skipping pip install.".dimmed());
    } else {
        let pip = venv_dir.join(bin_dir).join(if cfg!(windows) { "pip.exe" } else { "pip" });
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
    let status = Command::new(&py)
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
        "$cwd": format!("../venv/{}/", bin_dir),
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
fn ensure_dir(path: &std::path::Path, label: &str) {
    if path.exists() {
        println!("  {} {} {}", "+".dimmed(), label.dimmed(), "(exists)".dimmed());
    } else {
        fs::create_dir_all(path).unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", label, e);
        });
        println!("  {} {}", "+".green(), label.dimmed());
    }
}

/// 写入文件内容。
fn write_file(path: &std::path::Path, content: &str, label: &str) {
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

        // 输入 "node-pkgs" 确认删除
        let confirm_text = "node-pkgs".to_string();
        print!(
            "  {} {}: ",
            "Type".dimmed(),
            confirm_text.yellow()
        );
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            println!();
            println!("  {}", "Cancelled.".dimmed());
            return;
        }
        if input.trim() != confirm_text {
            println!();
            println!("  {}", "Confirmation does not match. Cancelled.".dimmed());
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
    ensure_dir(&layout.root_dir, "CLI home");
    ensure_dir(node_pkgs_dir, "node-pkgs");
    ensure_dir(&layout.alias_dir, "alias");

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
