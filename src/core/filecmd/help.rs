//! `..help`：列出文件命令；指定命令时显示用法与选项。

use colored::Colorize;

use super::{FileScope, OPTIONS, VERBS, VerbInfo, unknown_verb_message};
use crate::utils::display::get_display_width;

pub(super) fn run(scope: FileScope, stem: &str, args: &[String]) -> Result<String, String> {
    match args {
        [] => Ok(render_index(scope, stem)),
        [command] => render_command(scope, stem, command),
        _ => Err("help takes at most one command".to_string()),
    }
}

fn base(scope: FileScope, stem: &str) -> String {
    match scope {
        FileScope::Local => format!("@{}", stem),
        FileScope::Global => format!("@@{}", stem),
    }
}

fn render_index(scope: FileScope, stem: &str) -> String {
    let base = base(scope, stem);
    let commands: Vec<(String, &'static str)> = VERBS
        .iter()
        .map(|info| (format!("{}..{}", base, info.name), info.description))
        .collect();

    let mut output = String::from("\n");
    output.push_str(&render_section("File commands:", &commands));
    output.push('\n');
    output.push_str(&format!(
        "Use {} <command> to see usage and options.\n",
        format!("{}..help", base).cyan().bold()
    ));
    output.push('\n');
    output
}

fn render_command(scope: FileScope, stem: &str, name: &str) -> Result<String, String> {
    let Some(info) = VERBS.iter().find(|info| info.name == name) else {
        return Err(unknown_verb_message(name));
    };
    Ok(render_verb(&base(scope, stem), info))
}

fn render_verb(base: &str, info: &VerbInfo) -> String {
    let options: Vec<(String, &'static str)> = OPTIONS
        .iter()
        .filter(|option| option.verb == info.verb)
        .map(|option| (option.syntax.to_string(), option.description))
        .collect();

    let mut usage = format!("{}..{}", base, info.name).cyan().bold().to_string();
    if !info.usage.is_empty() {
        usage.push(' ');
        usage.push_str(info.usage);
    }
    if !options.is_empty() {
        usage.push_str(" [options]");
    }

    let mut output = String::from("\n");
    output.push_str(&usage);
    output.push('\n');
    output.push_str(info.description);
    output.push('\n');
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

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
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
    fn lists_commands_without_options() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &[]).unwrap());

        assert!(text.starts_with("\nFile commands:\n"));
        assert!(text.ends_with("\n\n"));
        assert!(text.contains("  @@ops..init"));
        assert!(text.contains("Create an alias file"));
        assert!(text.contains("  @@ops..clear"));
        assert!(text.contains("Reset this file to an empty object"));
        assert!(text.contains("  @@ops..set"));
        assert!(text.contains("Create or update an alias"));
        assert!(text.contains("  @@ops..path"));
        assert!(text.contains("Print the absolute path"));
        assert!(text.contains("  @@ops..rename"));
        assert!(text.contains("Rename the alias file"));
        assert!(text.contains("  @@ops..link"));
        assert!(text.contains("Point this file at another alias file"));
        assert!(text.contains("  @@ops..help"));
        assert!(text.contains("Show this help"));
        assert!(!text.contains("Options:"));
        assert!(text.contains("Use @@ops..help <command> to see usage and options."));
    }

    #[test]
    fn command_help_shows_usage_and_options() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &args(&["set"])).unwrap());

        assert!(text.starts_with("\n@@ops..set <key> [options]\nCreate or update an alias\n"));
        assert!(text.ends_with("\n\n"));
        assert!(text.contains("\nOptions:\n"));
        assert!(text.contains("  --cmd <command>"));
        assert!(text.contains("Set the alias command"));
        assert!(text.contains("  --unset <fields>"));
        assert!(!text.contains("@@ops..init"));
    }

    #[test]
    fn command_help_without_options_omits_section() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &args(&["path"])).unwrap());

        assert!(text.starts_with("\n@@ops..path\nPrint the absolute path\n"));
        assert!(!text.contains("Options:"));
    }

    #[test]
    fn command_help_includes_positional_placeholders() {
        let rename = strip_ansi(&run(FileScope::Global, "ops", &args(&["rename"])).unwrap());
        assert!(rename.contains("@@ops..rename <new-name>"));

        let link = strip_ansi(&run(FileScope::Global, "ops", &args(&["link"])).unwrap());
        assert!(link.contains("@@ops..link <target>"));

        let init = strip_ansi(&run(FileScope::Global, "ops", &args(&["init"])).unwrap());
        assert!(init.contains("@@ops..init [options]"));
    }

    #[test]
    fn local_and_anonymous_prefix() {
        let local = strip_ansi(&run(FileScope::Local, "ops", &[]).unwrap());
        assert!(local.contains("  @ops..init"));

        let anonymous = strip_ansi(&run(FileScope::Global, "", &[]).unwrap());
        assert!(anonymous.contains("  @@..init"));

        let local_set = strip_ansi(&run(FileScope::Local, "ops", &args(&["set"])).unwrap());
        assert!(local_set.contains("@ops..set <key> [options]"));

        let anonymous_set = strip_ansi(&run(FileScope::Global, "", &args(&["set"])).unwrap());
        assert!(anonymous_set.contains("@@..set <key> [options]"));
    }

    #[test]
    fn rejects_unknown_command() {
        let err = run(FileScope::Global, "ops", &args(&["bogus"])).unwrap_err();

        assert!(err.starts_with("Unknown file command: \"bogus\""));
        assert!(
            err.contains("Available: init, destroy, clear, set, json, path, rename, link, help")
        );
    }

    #[test]
    fn rejects_extra_arguments() {
        let err = run(FileScope::Global, "ops", &args(&["set", "extra"])).unwrap_err();

        assert_eq!(err, "help takes at most one command");
    }
}
