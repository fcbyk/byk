//! Shell 检测与 RC 文件操作工具。
//!
//! 供 `add comp` 读写补全配置使用。

use std::env;
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
        "if command -v byk >/dev/null 2>&1; then source <(byk __api__ completion {}); fi",
        shell_name
    )
}

/// 判断一行是否为 byk 的补全加载行。
///
/// 同时识旧形式 `byk completion <shell>`（`__api__` 之前写入 rc 的），
/// 以保证升级后重跑 `byk add comp` 能替换而不是重复追加。
fn is_completion_line(line: &str) -> bool {
    line.contains("byk completion") || line.contains("byk __api__ completion")
}

/// 判断 rc 内容里是否已有 byk 补全加载行（新旧形式都算）。
pub fn has_completion_line(content: &str) -> bool {
    content.lines().any(is_completion_line)
}

/// 从 RC 文件内容中移除 byk completion 行及前面的注释/空行。
pub fn strip_completion_lines(content: &str) -> String {
    let mut new_lines: Vec<&str> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if is_completion_line(l) {
            i += 1;
            continue;
        }
        if i + 1 < lines.len()
            && is_completion_line(lines[i + 1])
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
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== completion_line ====================

    #[test]
    fn completion_line_zsh() {
        let line = completion_line("zsh");
        assert!(line.contains("byk __api__ completion zsh"));
        assert!(line.starts_with("if command -v byk"));
    }

    #[test]
    fn completion_line_bash() {
        let line = completion_line("bash");
        assert!(line.contains("byk __api__ completion bash"));
        assert!(line.starts_with("if command -v byk"));
    }

    // ==================== strip_completion_lines ====================

    #[test]
    fn strip_removes_completion_line() {
        let input = "export PATH=/usr/bin:$PATH\nif command -v byk >/dev/null 2>&1; then source <(byk completion zsh); fi\n";
        let result = strip_completion_lines(input);
        assert!(!result.contains("byk completion"));
        assert!(result.contains("export PATH"));
    }

    #[test]
    fn strip_removes_comment_preceding_completion() {
        let input = "# byk shell completion\nif command -v byk >/dev/null 2>&1; then source <(byk completion zsh); fi\nother line\n";
        let result = strip_completion_lines(input);
        assert!(!result.contains("byk shell completion"));
        assert!(!result.contains("byk completion"));
        assert!(result.contains("other line"));
    }

    #[test]
    fn strip_removes_blank_line_before_completion() {
        let input = "some line\n\nif command -v byk >/dev/null 2>&1; then source <(byk completion zsh); fi\nlast\n";
        let result = strip_completion_lines(input);
        assert!(!result.contains("byk completion"));
        assert!(result.contains("some line"));
        assert!(result.contains("last"));
    }

    #[test]
    fn strip_no_completion_unchanged() {
        let input = "export PATH=/usr/bin:$PATH\n";
        let result = strip_completion_lines(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_removes_api_form_line() {
        let input = "# byk shell completion\nif command -v byk >/dev/null 2>&1; then source <(byk __api__ completion zsh); fi\nother line\n";
        let result = strip_completion_lines(input);
        assert!(!result.contains("__api__ completion"));
        assert!(!result.contains("byk shell completion"));
        assert!(result.contains("other line"));
    }

    #[test]
    fn strip_empty_content() {
        let result = strip_completion_lines("");
        assert_eq!(result, "\n");
    }

    // ==================== detect_shell (env-based, best-effort) ====================

    #[test]
    fn detect_shell_returns_expected_for_zsh() {
        let shell = std::env::var("SHELL").unwrap_or_default();
        if shell.ends_with("/zsh") {
            assert_eq!(detect_shell(), Some((".zshrc", "zsh")));
        }
    }

    #[test]
    fn detect_shell_returns_expected_for_bash() {
        let shell = std::env::var("SHELL").unwrap_or_default();
        if shell.ends_with("/bash") {
            assert_eq!(detect_shell(), Some((".bashrc", "bash")));
        }
    }
}
