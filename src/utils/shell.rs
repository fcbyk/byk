/// Shell 检测与 RC 文件操作工具。
///
/// 供 `init`、`rm`、`completion` 模块共用，避免重复代码。

use colored::Colorize;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Shell 检测
// ---------------------------------------------------------------------------

/// 从 `$SHELL` 解析当前 shell，返回 `(rc_filename, shell_name)`。
/// 不支持 zsh/bash 以外返回 `None`。
pub fn detect_shell() -> Option<(&'static str, &'static str)> {
    let shell = env::var("SHELL").unwrap_or_default();
    if shell.ends_with("/zsh") {
        Some((".zshrc", "zsh"))
    } else if shell.ends_with("/bash") {
        Some((".bashrc", "bash"))
    } else {
        None
    }
}

/// 返回当前 shell 的 RC 文件路径。
pub fn rc_path() -> Option<PathBuf> {
    let (rc_filename, _) = detect_shell()?;
    dirs::home_dir().map(|h| h.join(rc_filename))
}

// ---------------------------------------------------------------------------
// RC 文件内容
// ---------------------------------------------------------------------------

/// 生成 byk completion 追加行。
pub fn completion_line(shell_name: &str) -> String {
    format!(
        "if command -v byk >/dev/null 2>&1; then source <(byk completion {}); fi",
        shell_name
    )
}

/// 从 RC 文件内容中移除 byk completion 行及前面的注释/空行。
pub fn strip_completion_lines(content: &str) -> String {
    let mut new_lines: Vec<&str> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.contains("byk completion") {
            i += 1;
            continue;
        }
        if i + 1 < lines.len()
            && lines[i + 1].contains("byk completion")
            && (l.trim().is_empty() || l.trim().starts_with("# byk shell completion"))
        {
            i += 1;
            continue;
        }
        new_lines.push(l);
        i += 1;
    }
    new_lines.join("\n") + "\n"
}

// ---------------------------------------------------------------------------
// 交互确认
// ---------------------------------------------------------------------------

/// 提示用户输入确认文本，匹配返回 `true`，不匹配打印取消信息返回 `false`。
pub fn prompt_confirm(text: &str) -> bool {
    print!("  {} {}: ", "Type".dimmed(), text.yellow());
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        println!();
        println!("  {}", "Cancelled.".dimmed());
        return false;
    }
    if input.trim() != text {
        println!();
        println!(
            "{}",
            "Confirmation does not match. Cancelled.".dimmed()
        );
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// RC 写入安全封装
// ---------------------------------------------------------------------------

/// 安全写入 RC 文件，失败时 eprintln 并 exit(1)。
pub fn write_rc(path: &PathBuf, content: &str) {
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", path.display(), e);
        std::process::exit(1);
    });
}
