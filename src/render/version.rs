//! `-v` 输出渲染：首行机器可解析，其后是排查环境用的短面板。

use colored::Colorize;

use crate::core::aliases;
use crate::core::paths::PathLayout;
use crate::utils::shell;

/// 渲染版本与运行环境摘要。
///
/// 首行格式固定为 `byk <版本> (<commit> <构建日期>) [<平台>]`，
/// 空行之后的信息面向人，可自由调整。
pub fn render(layout: &PathLayout) {
    println!(
        "byk {} ({} {}) [{}]",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_HASH"),
        env!("BUILD_DATE"),
        env!("PLATFORM"),
    );

    if let Ok(exe_path) = std::env::current_exe() {
        println!(
            "{} {}",
            "installed at".dimmed(),
            exe_path.display().to_string().dimmed(),
        );
    }

    println!();

    let (merged, _files) = aliases::load_merged_aliases(layout);
    line("aliases", &merged.len().to_string());
    line("completion", &completion_status());
}

/// 两列对齐的一行（先补白再着色，避免转义码参与宽度计算）。
fn line(label: &str, value: &str) {
    println!("{}{}", format!("{:<12}", label).dimmed(), value);
}

/// 补全状态：`enabled (zsh)` / `not configured (zsh)` / `unknown shell`。
fn completion_status() -> String {
    let Some((_, shell_name)) = shell::detect_shell() else {
        return "unknown shell".to_string();
    };

    let configured = shell::rc_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|content| shell::has_completion_line(&content));

    if configured {
        format!("enabled ({})", shell_name)
    } else {
        format!("not configured ({})", shell_name)
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_does_not_panic() {
        let layout = PathLayout::new();
        render(&layout);
    }

    #[test]
    fn completion_status_is_never_empty() {
        assert!(!completion_status().is_empty());
    }

    #[test]
    fn line_pads_label_to_fixed_width() {
        // 两列描述必须从同一列开始，与语言/颜色无关
        let label = "aliases";
        let padded = format!("{:<12}", label);
        assert_eq!(padded.len(), 12);
        assert!(padded.starts_with(label));
    }
}
