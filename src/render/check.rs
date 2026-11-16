//! `--check`：汇总展示扫描期发现的别名文件问题。

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use colored::Colorize;

use crate::core::aliases::{self, ScannedAliases};
use crate::core::config::{Diagnostic, DiagnosticReport, Level};
use crate::core::paths::PathLayout;

/// 渲染检查报告；返回进程退出码（有问题为 1）。
pub fn render(layout: &PathLayout) -> i32 {
    let scanned = aliases::load_scanned_aliases(layout);
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(format_report(&scanned).as_bytes());
    let _ = stdout.flush();
    if scanned.report.is_clean() { 0 } else { 1 }
}

fn format_report(loaded: &ScannedAliases) -> String {
    let report = &loaded.report;
    let mut output = String::new();

    if report.is_clean() {
        output.push_str(&format!(
            "Scanned {}; no problems.\n",
            files_phrase(report.scanned)
        ));
        return output;
    }

    let problem_files = report.problem_file_count();
    let count = if problem_files == 1 { "has" } else { "have" };
    output.push_str(&format!(
        "Scanned {}; {} {} problems ({}).\n\n",
        files_phrase(report.scanned),
        problem_files,
        count,
        counts_phrase(report)
    ));

    let mut groups: BTreeMap<&Path, Vec<&Diagnostic>> = BTreeMap::new();
    for diagnostic in &report.items {
        groups
            .entry(diagnostic.path.as_path())
            .or_default()
            .push(diagnostic);
    }
    for diagnostics in groups.values_mut() {
        diagnostics.sort_by(|a, b| a.level.cmp(&b.level).then(a.message.cmp(&b.message)));
    }

    for (path, diagnostics) in groups {
        output.push_str(&format!("{}\n", path.display().to_string().bold()));
        for diagnostic in diagnostics {
            // 消息里可能带 `\nHint: ...`：续行缩进到消息起始列，否则建议会
            // 顶格跑出来，看着像下一条诊断
            let (label, width) = match diagnostic.level {
                Level::Error => ("error".red().bold(), "error".len()),
                Level::Warning => ("warning".yellow().bold(), "warning".len()),
            };
            let mut lines = diagnostic.message.split('\n');
            if let Some(first) = lines.next() {
                output.push_str(&format!("  {}: {}\n", label, first));
            }
            for line in lines {
                output.push_str(&format!("{}{}\n", " ".repeat(2 + width + 2), line));
            }
        }
        output.push('\n');
    }

    output
}

fn files_phrase(count: usize) -> String {
    if count == 1 {
        "1 alias file".to_string()
    } else {
        format!("{} alias files", count)
    }
}

fn counts_phrase(report: &DiagnosticReport) -> String {
    let mut parts = Vec::new();
    let errors = report.error_count();
    if errors > 0 {
        parts.push(format!(
            "{} error{}",
            errors,
            if errors == 1 { "" } else { "s" }
        ));
    }
    let warnings = report.warning_count();
    if warnings > 0 {
        parts.push(format!(
            "{} warning{}",
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn loaded(scanned: usize, items: Vec<Diagnostic>) -> ScannedAliases {
        ScannedAliases {
            files: Vec::new(),
            report: DiagnosticReport { scanned, items },
        }
    }

    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c == '\x1b' {
                chars.next();
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&inner) = chars.peek() {
                        chars.next();
                        if inner.is_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(c);
                chars.next();
            }
        }
        result
    }

    #[test]
    fn clean_report_has_no_details() {
        let text = strip_ansi(&format_report(&loaded(3, Vec::new())));
        assert_eq!(text, "Scanned 3 alias files; no problems.\n");
    }

    #[test]
    fn groups_by_path_and_orders_by_level() {
        let path = PathBuf::from("/tmp/ops.byk.json");
        let text = strip_ansi(&format_report(&loaded(
            1,
            vec![
                Diagnostic::warning(&path, "warn one"),
                Diagnostic::error(&path, "boom"),
            ],
        )));
        assert_eq!(
            text,
            "Scanned 1 alias file; 1 has problems (1 error, 1 warning).\n\n/tmp/ops.byk.json\n  error: boom\n  warning: warn one\n\n"
        );
    }

    #[test]
    fn continuation_lines_align_with_the_message() {
        let path = PathBuf::from("/tmp/ops.byk.json");
        let text = strip_ansi(&format_report(&loaded(
            1,
            vec![
                Diagnostic::warning(&path, "first line\nHint: read this"),
                Diagnostic::error(&path, "boom\nHint: fix it"),
            ],
        )));

        // 续行缩进到消息起始列，否则 `Hint:` 会顶格，看着像下一条诊断
        assert!(
            text.contains("  warning: first line\n           Hint: read this\n"),
            "{text}"
        );
        assert!(
            text.contains("  error: boom\n         Hint: fix it\n"),
            "{text}"
        );
    }
}
