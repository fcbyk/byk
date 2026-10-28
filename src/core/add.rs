//! `byk add` 子命令逻辑。
//!
//! 包含：
//! - 内置功能初始化（comp）
//! - 从本地别名文件安装（@xxx）
//! - 从全局别名文件安装（@@xxx）

use colored::Colorize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use crate::core::paths::PathLayout;
use crate::core::plugins;
use crate::utils::shell;

// ---------------------------------------------------------------------------
// --add comp
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
// --add @xxx（当前目录）
// ---------------------------------------------------------------------------

/// `byk add @xxx`：读取当前目录 `xxx.byk.json` 的 `$add` 字段并安装。
pub fn install_from_local(name: &str, layout: &PathLayout) {
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
    let path = cwd.join(alias_file_name(stem));

    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }

    let file_key = format!("@{}", stem);
    plugins::install_from_file(&path, &file_key, stem, layout);
}

// ---------------------------------------------------------------------------
// --add @@xxx（全局别名目录）
// ---------------------------------------------------------------------------

/// `byk add @@xxx`：读取 `~/.byk/alias/xxx.byk.json` 的 `$add` 字段并安装。
pub fn install_from_global(name: &str, layout: &PathLayout) {
    let stem = &name[2..]; // strip '@@'
    if stem.is_empty() {
        eprintln!(
            "{} invalid argument: {} (expected @@<name>)",
            "Error:".red(),
            name.bold()
        );
        std::process::exit(1);
    }

    let path = layout.alias_dir.join(alias_file_name(stem));

    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }

    let file_key = format!("@@{}", stem);
    plugins::install_from_file(&path, &file_key, stem, layout);
}

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------

/// stem → 文件名。空 stem（匿名 `@` / `@@`）→ `.byk.json`。
pub(crate) fn alias_file_name(stem: &str) -> String {
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
    fn alias_file_name_forms() {
        assert_eq!(alias_file_name("demo"), "demo.byk.json");
        assert_eq!(alias_file_name(""), ".byk.json");
    }
}