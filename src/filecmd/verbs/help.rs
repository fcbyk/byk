//! `help`：列出文件命令；指定命令时显示用法与选项。
//!
//! 输出格式与 `byk -h` 对齐：Usage / Options / Commands。

use std::path::Path;

use colored::Colorize;
use serde_json::Value;

use crate::filecmd::io::path::resolve_target;
use crate::filecmd::source::Forge;
use crate::filecmd::spec::FileScope;
use crate::filecmd::spec::{OPTIONS, SUBCOMMANDS, VERBS, Verb, VerbInfo, unknown_verb_message};
use crate::model::PathLayout;
use crate::utils::display::get_display_width;

pub(crate) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<String, String> {
    match args {
        [] => Ok(render_index(scope, stem, layout)),
        [command] => render_command(scope, stem, command, layout),
        _ => Err("help takes at most one command".to_string()),
    }
}

fn base(scope: FileScope, stem: &str) -> String {
    format!("{}{}", scope.token(), stem)
}

fn help_options() -> Vec<(String, &'static str)> {
    vec![("--help, -h".to_string(), "Show this help")]
}

/// `FilePath:` 的值：绝对路径，附带存在状态（未创建 / 链接文件）。
fn target_line(scope: FileScope, stem: &str, layout: &PathLayout) -> String {
    let Ok(path) = resolve_target(scope, stem, layout) else {
        return "-".to_string();
    };
    let marker = if !path.exists() {
        " (not created yet)".yellow().to_string()
    } else if is_link_file(&path) {
        " (link)".yellow().to_string()
    } else {
        String::new()
    };
    format!("{}{}", path.display(), marker)
}

fn is_link_file(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    matches!(
        serde_json::from_str::<Value>(&text),
        Ok(Value::Array(items)) if items.len() == 1 && items[0].is_string()
    )
}

fn render_index(scope: FileScope, stem: &str, layout: &PathLayout) -> String {
    let commands: Vec<(String, &'static str)> = VERBS
        .iter()
        .map(|info| (info.name.to_string(), info.description))
        .collect();
    let kind = match scope {
        FileScope::Local => "local",
        FileScope::Global => "global",
    };
    let base = base(scope, stem);

    let mut output = String::from("\n");
    output.push_str(&format!(
        "Commands for this {} alias file. Run `byk {} <command> -h` for details.\n\n",
        kind, base
    ));
    output.push_str(&format!(
        "{} {}\n",
        "FilePath:".green().bold(),
        target_line(scope, stem, layout)
    ));
    output.push_str(&format!(
        "{} byk {} [OPTIONS] [COMMAND] [ARGS]...\n",
        "Usage:".green().bold(),
        base
    ));
    output.push('\n');
    output.push_str(&render_section("Options:", &help_options()));
    output.push('\n');
    output.push_str(&render_section("Commands:", &commands));
    output.push('\n');
    output
}

fn render_command(
    scope: FileScope,
    stem: &str,
    name: &str,
    layout: &PathLayout,
) -> Result<String, String> {
    let Some(info) = VERBS.iter().find(|info| info.name == name) else {
        return Err(unknown_verb_message(name));
    };
    Ok(render_verb(
        &base(scope, stem),
        info,
        &target_line(scope, stem, layout),
    ))
}

fn render_verb(base: &str, info: &VerbInfo, target: &str) -> String {
    let options: Vec<(String, &'static str)> = OPTIONS
        .iter()
        .filter(|option| option.verb == info.verb)
        .map(|option| (option.syntax.to_string(), option.description))
        .collect();

    let mut usage = format!(
        "{} byk {} {} [OPTIONS]",
        "Usage:".green().bold(),
        base,
        info.name
    );
    if !info.usage.is_empty() {
        usage.push(' ');
        usage.push_str(info.usage);
    }

    let mut entries = help_options();
    entries.extend(options);

    let subcommands: Vec<(String, &'static str)> = if info.verb == Verb::Deps {
        SUBCOMMANDS
            .iter()
            .map(|(name, description)| ((*name).to_string(), *description))
            .collect()
    } else {
        Vec::new()
    };

    let mut output = String::from("\n");
    output.push_str(info.description);
    output.push_str("\n\n");
    output.push_str(&format!("{} {}\n", "FilePath:".green().bold(), target));
    output.push_str(&usage);
    output.push_str("\n\n");
    output.push_str(&render_section("Options:", &entries));
    if info.verb == Verb::Clone {
        output.push('\n');
        output.push_str(&render_section("Sources:", &clone_sources()));
        output.push_str(
            "  <spec> = <user>/<repo>[@<ref>][#<path>]; <ref> defaults to HEAD, a\n  directory <path> ends with `/`, and the `#` form lets the ref contain `/`.\n",
        );
    }
    if !subcommands.is_empty() {
        output.push('\n');
        output.push_str(&render_section("Commands:", &subcommands));
    }
    output.push('\n');
    output
}

/// `clone` 的来源形态：位置参数的全部写法。
///
/// `--tap` 已经在 Options 里，这里不再重复；四个前缀的名字取自
/// [`Prefix::NAMES`]，将来加通道只需要改那一处。
fn clone_sources() -> Vec<(String, &'static str)> {
    let mut rows = vec![
        (
            "local@name, global@name".to_string(),
            "An alias file in the current or global directory",
        ),
        (
            "<path>".to_string(),
            "A local file; a bare name gets `.byk.json` appended",
        ),
        (
            "<url>".to_string(),
            "An http(s) alias file, downloaded with curl",
        ),
    ];
    rows.extend(Forge::NAMES.iter().map(|name| {
        let description = match *name {
            "github" => "A GitHub repository; the transport comes from `net-github`",
            _ => "A Gitee repository",
        };
        (format!("{name}:<spec>"), description)
    }));
    rows
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
    use std::path::PathBuf;

    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn layout() -> PathLayout {
        PathLayout {
            root_dir: PathBuf::from("/root"),
            alias_dir: PathBuf::from("/root/alias"),
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
    fn index_describes_the_addressed_file() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &[], &layout()).unwrap());

        assert!(text.starts_with(
            "\nCommands for this global alias file. Run `byk global@ops <command> -h` for details.\n\nFilePath: "
        ));
        assert!(text.contains(
            "ops.byk.json (not created yet)\nUsage: byk global@ops [OPTIONS] [COMMAND] [ARGS]...\n\n"
        ));
        assert!(text.contains("Options:\n  --help, -h  Show this help\n"));
        assert!(text.contains("Commands:\n"));
        assert!(text.contains("  init     Create this alias file\n"));
        assert!(text.contains("  destroy  Delete this alias file\n"));
        assert!(text.contains("  clear    Reset this alias file to an empty object\n"));
        assert!(text.contains("  set      Create or update an alias\n"));
        assert!(text.contains("  remove   Remove an alias or group\n"));
        assert!(text.contains("  json     Show this alias file's JSON\n"));
        assert!(text.contains("  path     Print this alias file's absolute path\n"));
        assert!(text.contains("  rename   Rename this alias file\n"));
        assert!(text.contains("  clone    Copy a local or remote alias file into this file\n"));
        assert!(text.contains("  link     Point this file at another alias file\n"));
        assert!(
            text.contains("  deps     Install, uninstall, or inspect this file's dependencies\n")
        );
        assert!(!text.contains("  help "));
        assert!(text.ends_with("\n\n"));
    }

    #[test]
    fn target_line_marks_missing_plain_and_link_files() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        std::fs::create_dir_all(&layout.alias_dir).unwrap();

        let missing = strip_ansi(&run(FileScope::Global, "ops", &[], &layout).unwrap());
        assert!(missing.contains("ops.byk.json (not created yet)"));

        std::fs::write(layout.alias_dir.join("ops.byk.json"), "{}\n").unwrap();
        let plain = strip_ansi(&run(FileScope::Global, "ops", &[], &layout).unwrap());
        assert!(plain.contains("ops.byk.json\nUsage:"));

        std::fs::write(
            layout.alias_dir.join("ops.byk.json"),
            "[\"/tmp/other.byk.json\"]\n",
        )
        .unwrap();
        let link = strip_ansi(&run(FileScope::Global, "ops", &[], &layout).unwrap());
        assert!(link.contains("ops.byk.json (link)"));
    }

    #[test]
    fn command_help_shows_usage_and_options() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &args(&["set"]), &layout()).unwrap());

        assert!(text.starts_with("\nCreate or update an alias\n\nFilePath: "));
        assert!(text.contains("\nUsage: byk global@ops set [OPTIONS] [key]\n\nOptions:\n"));
        assert!(text.contains("  --help, -h"));
        assert!(text.contains("Show this help"));
        assert!(text.contains("  --cmd <command>"));
        assert!(text.contains("Set the alias command"));
        assert!(text.contains("  --priority <n>"));
        assert!(text.contains("  --paths <dir>"));
        assert!(text.contains("  --unset <fields>"));
        assert!(text.ends_with("\n\n"));
    }

    #[test]
    fn command_help_still_lists_help_option_without_other_options() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &args(&["path"]), &layout()).unwrap());

        assert!(text.starts_with("\nPrint this alias file's absolute path\n\nFilePath: "));
        assert!(text.contains("\nUsage: byk global@ops path [OPTIONS]\n\nOptions:\n"));
        assert!(text.contains("  --help, -h  Show this help"));
    }

    #[test]
    fn command_help_includes_positional_placeholders() {
        let rename =
            strip_ansi(&run(FileScope::Global, "ops", &args(&["rename"]), &layout()).unwrap());
        assert!(rename.contains("Usage: byk global@ops rename [OPTIONS] <new-name>"));

        let link = strip_ansi(&run(FileScope::Global, "ops", &args(&["link"]), &layout()).unwrap());
        assert!(link.contains("Usage: byk global@ops link [OPTIONS] <target>"));

        let clone =
            strip_ansi(&run(FileScope::Global, "ops", &args(&["clone"]), &layout()).unwrap());
        assert!(clone.contains("Usage: byk global@ops clone [OPTIONS] <source>"));

        let init = strip_ansi(&run(FileScope::Global, "ops", &args(&["init"]), &layout()).unwrap());
        assert!(init.contains("Usage: byk global@ops init [OPTIONS]"));

        let remove =
            strip_ansi(&run(FileScope::Global, "ops", &args(&["remove"]), &layout()).unwrap());
        assert!(remove.contains("Usage: byk global@ops remove [OPTIONS] <key>"));
    }

    #[test]
    fn command_help_for_clone_lists_sources_instead_of_options() {
        let text =
            strip_ansi(&run(FileScope::Global, "ops", &args(&["clone"]), &layout()).unwrap());

        assert!(text.contains("Usage: byk global@ops clone [OPTIONS] <source>"));
        assert!(text.contains("\nSources:\n"));
        assert!(text.contains("local@name, global@name"));
        assert!(text.contains("github:<spec>"));
        assert!(text.contains("the transport comes from `net-github`"));
        assert!(text.contains("gitee:<spec>"));
        // 通道不在来源语法里，帮助的 Sources 段不该再出现它们
        assert!(!text.contains("jsdelivr"));
        assert!(!text.contains("ghproxy"));
        // 旧选项已随前缀一起消失，不能再出现在帮助里
        assert!(!text.contains("-gh"));
        assert!(!text.contains("--gitee"));
        assert!(text.contains("-t, --tap <spec>"));
        assert!(text.contains("Resolve a key from a local or GitHub byk.tap.json index"));
    }

    #[test]
    fn command_help_for_deps_lists_options_and_subcommands() {
        let text = strip_ansi(&run(FileScope::Global, "ops", &args(&["deps"]), &layout()).unwrap());

        assert!(text.contains("Usage: byk global@ops deps [OPTIONS] [COMMAND]"));
        assert!(text.contains("  --force"));
        assert!(text.contains("Reinstall dependencies from source (install)"));
        assert!(text.contains("  --yes"));
        assert!(text.contains("Skip the uninstall confirmation"));
        assert!(text.contains("\nCommands:\n"));
        assert!(text.contains("  install    Install the declared files, pip and npm packages"));
        assert!(text.contains("  uninstall  Remove this file's dependency directory"));
        assert!(text.contains("  status     Show what is declared, installed, or stale"));
    }

    #[test]
    fn local_and_anonymous_prefix() {
        let local = strip_ansi(&run(FileScope::Local, "ops", &[], &layout()).unwrap());
        assert!(local.starts_with(
            "\nCommands for this local alias file. Run `byk local@ops <command> -h` for details.\n\nFilePath: "
        ));
        assert!(local.contains("Usage: byk local@ops [OPTIONS]"));
        assert!(local.contains("  init     Create this alias file"));

        let anonymous = strip_ansi(&run(FileScope::Global, "", &[], &layout()).unwrap());
        assert!(anonymous.contains("Usage: byk global@ [OPTIONS]"));

        let local_set =
            strip_ansi(&run(FileScope::Local, "ops", &args(&["set"]), &layout()).unwrap());
        assert!(local_set.contains("Usage: byk local@ops set [OPTIONS] [key]"));

        let anonymous_set =
            strip_ansi(&run(FileScope::Global, "", &args(&["set"]), &layout()).unwrap());
        assert!(anonymous_set.contains("Usage: byk global@ set [OPTIONS] [key]"));
    }

    #[test]
    fn rejects_unknown_command() {
        let err = run(FileScope::Global, "ops", &args(&["bogus"]), &layout()).unwrap_err();

        assert!(err.starts_with("Unknown file command: \"bogus\""));
        assert!(err.contains(
            "Available: init, destroy, clear, set, remove, json, path, rename, clone, link, deps"
        ));
        assert!(!err.contains(", help"));
    }

    #[test]
    fn rejects_extra_arguments() {
        let err = run(
            FileScope::Global,
            "ops",
            &args(&["set", "extra"]),
            &layout(),
        )
        .unwrap_err();

        assert_eq!(err, "help takes at most one command");
    }
}
