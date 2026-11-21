//! 文件命令的交互确认。

use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

use colored::Colorize;
use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Behavior, Config, Editor, Helper};

pub(crate) enum Confirmation {
    Confirmed,
    Cancelled,
    Mismatch,
}

/// 非终端下拒绝执行需要确认的操作。
pub(crate) fn require_tty(path: &Path, verb: &str) -> Result<(), String> {
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
pub(crate) fn confirm(
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
    let plain = format!("  Type {expected}: ");
    let colored = format!("  {} {}: ", "Type".dimmed(), expected.yellow());

    match read_token(&plain, &colored)? {
        None => {
            eprintln!();
            Ok(Confirmation::Cancelled)
        }
        Some(input) if input.trim() == expected => Ok(Confirmation::Confirmed),
        Some(_) => {
            eprintln!();
            Ok(Confirmation::Mismatch)
        }
    }
}

/// 渲染时给 prompt 上色，宽度计算则用调用方传入的纯文本 prompt。
///
/// rustyline 在 Windows 上按字符宽度累计布局，不会跳过 ANSI 转义序列
/// （unix 上会跳过），带色串直接当 prompt 会让光标多偏移一段转义字符的宽度。
struct PromptHelper(String);

impl Completer for PromptHelper {
    type Candidate = String;
}

impl Hinter for PromptHelper {
    type Hint = String;
}

impl Validator for PromptHelper {}

impl Helper for PromptHelper {}

impl Highlighter for PromptHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        _prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Owned(self.0.clone())
    }
}

/// 读取确认输入；`None` 表示取消（EOF / Ctrl-C）。
/// 终端输入交给 rustyline，它按 Unicode 宽度重绘，退格能完整删除中文；
/// 管道输入仍走 `read_line`，保留脚本里 `echo token | byk ...` 的用法。
fn read_token(prompt: &str, colored_prompt: &str) -> Result<Option<String>, String> {
    if io::stdin().is_terminal() {
        // PreferTerm 让提示直接落到终端而不是 stdout，保持 stdout 干净
        let config = Config::builder().behavior(Behavior::PreferTerm).build();
        let mut editor = Editor::<PromptHelper, DefaultHistory>::with_config(config)
            .map_err(|e| format!("Failed to initialize confirmation prompt: {}", e))?;
        editor.set_helper(Some(PromptHelper(colored_prompt.to_string())));
        match editor.readline(prompt) {
            Ok(line) => Ok(Some(line)),
            Err(ReadlineError::Eof | ReadlineError::Interrupted) => Ok(None),
            Err(e) => {
                eprintln!();
                Err(format!("Failed to read confirmation: {}", e))
            }
        }
    } else {
        eprint!("{}", colored_prompt);
        io::stderr()
            .flush()
            .map_err(|e| format!("Failed to write confirmation prompt: {}", e))?;

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(input)),
            Err(e) => {
                eprintln!();
                Err(format!("Failed to read confirmation: {}", e))
            }
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
