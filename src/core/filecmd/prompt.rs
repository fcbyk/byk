//! 文件命令的交互确认。

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use colored::Colorize;

pub(super) enum Confirmation {
    Confirmed,
    Cancelled,
    Mismatch,
}

/// 非终端下拒绝执行需要确认的操作。
pub(super) fn require_tty(path: &Path, verb: &str) -> Result<(), String> {
    if io::stderr().is_terminal() {
        Ok(())
    } else {
        Err(format!(
            "Refusing to {} {} without confirmation; pass --yes",
            verb,
            path.display()
        ))
    }
}

/// 展示目标与摘要，要求输入 stem（匿名文件为完整文件名）确认。
pub(super) fn confirm(
    path: &Path,
    stem: &str,
    title: &str,
    note: Option<&str>,
) -> Result<Confirmation, String> {
    eprintln!();
    eprintln!("{}", title.yellow());
    eprintln!("  {}", path.display().to_string().dimmed());
    if let Some(note) = note {
        eprintln!("  {}", note.dimmed());
    }
    eprintln!();

    let expected = confirm_token(stem);
    eprint!("  {} {}: ", "Type".dimmed(), expected.yellow());
    io::stderr()
        .flush()
        .map_err(|e| format!("Failed to write confirmation prompt: {}", e))?;

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => {
            eprintln!();
            Ok(Confirmation::Cancelled)
        }
        Ok(_) if input.trim() == expected => Ok(Confirmation::Confirmed),
        Ok(_) => {
            eprintln!();
            Ok(Confirmation::Mismatch)
        }
        Err(e) => {
            eprintln!();
            Err(format!("Failed to read confirmation: {}", e))
        }
    }
}

/// 匿名文件没有 stem，改为输入完整文件名确认。
fn confirm_token(stem: &str) -> &str {
    if stem.is_empty() { ".byk.json" } else { stem }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_token_uses_stem_or_anonymous_name() {
        assert_eq!(confirm_token("ops"), "ops");
        assert_eq!(confirm_token(""), ".byk.json");
    }
}
