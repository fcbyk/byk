//! `..help`：列出当前文件可用的文件命令与选项。

use colored::Colorize;

use super::{FileScope, OPTIONS, VERBS, verb_info};
use crate::utils::display::get_display_width;

pub(super) fn run(scope: FileScope, stem: &str, args: &[String]) -> Result<String, String> {
    if !args.is_empty() {
        return Err("help takes no arguments".to_string());
    }
    Ok(render(scope, stem))
}

fn render(scope: FileScope, stem: &str) -> String {
    let base = match scope {
        FileScope::Local => format!("@{}", stem),
        FileScope::Global => format!("@@{}", stem),
    };

    let commands: Vec<(String, &'static str)> = VERBS
        .iter()
        .map(|info| (format!("{}..{}", base, info.name), info.description))
        .collect();

    let options: Vec<(String, &'static str)> = OPTIONS
        .iter()
        .map(|info| {
            (
                format!("{}..{} {}", base, verb_info(info.verb).name, info.flag),
                info.description,
            )
        })
        .collect();

    let mut output = String::from("\n");
    output.push_str(&render_section("File commands:", &commands));
    if !options.is_empty() {
        output.push('\n');
        output.push_str(&render_section("Options:", &options));
    }
    output.push('\n');
    output
}

fn render_section(title: &str, entries: &[(String, &'static str)]) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let width = entries
        .iter()
        .map(|(command, _)| get_display_width(command))
        .max()
        .unwrap_or(0);

    let mut output = format!("{}\n", title.green().bold());
    for (command, description) in entries {
        let padding = " ".repeat(width.saturating_sub(get_display_width(command)));
        output.push_str(&format!(
            "  {}{}  {}\n",
            command.cyan().bold(),
            padding,
            description
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn lists_commands_and_options() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &[]).unwrap());

        assert!(text.starts_with("\nFile commands:\n"));
        assert!(text.ends_with("\n\n"));
        assert!(text.contains("  @@ops..init"));
        assert!(text.contains("Create an alias file"));
        assert!(text.contains("  @@ops..path"));
        assert!(text.contains("Print the absolute path"));
        assert!(text.contains("  @@ops..rename"));
        assert!(text.contains("Rename the alias file"));
        assert!(text.contains("  @@ops..link"));
        assert!(text.contains("Point this file at another alias file"));
        assert!(text.contains("  @@ops..help"));
        assert!(text.contains("Show this help"));
        assert!(text.contains("\nOptions:\n"));
        assert!(text.contains("  @@ops..init --link"));
        assert!(text.contains("Create as a link to another alias file"));
        assert!(text.contains("  @@ops..destroy --yes"));
        assert!(text.contains("Skip confirmation"));
        assert!(text.contains("  @@ops..json --expand"));
        assert!(text.contains("  @@ops..json --no-check"));
    }

    #[test]
    fn local_and_anonymous_prefix() {
        let local = strip_ansi(&run(FileScope::Local, "ops", &[]).unwrap());
        assert!(local.contains("  @ops..init"));

        let anonymous = strip_ansi(&run(FileScope::Global, "", &[]).unwrap());
        assert!(anonymous.contains("  @@..init"));
    }

    #[test]
    fn rejects_arguments() {
        let err = run(FileScope::Global, "ops", &["extra".to_string()]).unwrap_err();
        assert_eq!(err, "help takes no arguments");
    }
}
