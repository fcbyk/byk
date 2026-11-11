//! 文件命令：绑定到别名文件的保留命令。
//!
//! 语法为 `@<file>..<verb>` / `@@<file>..<verb>`，`..` 是保留分隔符。
//! 文件命令独立于别名扫描、合并与执行，不进入合并命名空间。

mod clear;
mod destroy;
mod edit;
mod help;
mod init;
mod json;
mod key;
mod link;
mod path;
mod prompt;
mod remove;
mod rename;
mod set;
mod summary;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;

use serde_json::{Map, Value};

use crate::core::aliases::validate_filename;
use crate::core::paths::PathLayout;

/// `@` 指向当前目录，`@@` 指向全局别名目录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileScope {
    Local,
    Global,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verb {
    Init,
    Destroy,
    Clear,
    Set,
    Remove,
    Json,
    Path,
    Rename,
    Link,
    Help,
}

struct VerbInfo {
    name: &'static str,
    verb: Verb,
    description: &'static str,
    /// 位置参数片段；`[options]` 由是否有选项自动追加。
    usage: &'static str,
}

/// 已注册的动词；顺序即错误提示、补全与帮助输出顺序。
const VERBS: &[VerbInfo] = &[
    VerbInfo {
        name: "init",
        verb: Verb::Init,
        description: "Create an alias file",
        usage: "",
    },
    VerbInfo {
        name: "destroy",
        verb: Verb::Destroy,
        description: "Remove the alias file",
        usage: "",
    },
    VerbInfo {
        name: "clear",
        verb: Verb::Clear,
        description: "Reset this file to an empty object",
        usage: "",
    },
    VerbInfo {
        name: "set",
        verb: Verb::Set,
        description: "Create or update an alias",
        usage: "[key]",
    },
    VerbInfo {
        name: "remove",
        verb: Verb::Remove,
        description: "Remove an alias or group",
        usage: "<key>",
    },
    VerbInfo {
        name: "json",
        verb: Verb::Json,
        description: "Show the alias file JSON",
        usage: "",
    },
    VerbInfo {
        name: "path",
        verb: Verb::Path,
        description: "Print the absolute path",
        usage: "",
    },
    VerbInfo {
        name: "rename",
        verb: Verb::Rename,
        description: "Rename the alias file",
        usage: "<new-name>",
    },
    VerbInfo {
        name: "link",
        verb: Verb::Link,
        description: "Point this file at another alias file",
        usage: "<target>",
    },
    VerbInfo {
        name: "help",
        verb: Verb::Help,
        description: "Show this help",
        usage: "[command]",
    },
];

struct OptionInfo {
    verb: Verb,
    flag: &'static str,
    syntax: &'static str,
    description: &'static str,
}

/// 文件命令选项；顺序即补全与帮助输出顺序。
const OPTIONS: &[OptionInfo] = &[
    OptionInfo {
        verb: Verb::Init,
        flag: "--link",
        syntax: "--link",
        description: "Create as a link to another alias file",
    },
    OptionInfo {
        verb: Verb::Destroy,
        flag: "--yes",
        syntax: "--yes",
        description: "Skip confirmation",
    },
    OptionInfo {
        verb: Verb::Clear,
        flag: "--yes",
        syntax: "--yes",
        description: "Skip confirmation",
    },
    OptionInfo {
        verb: Verb::Json,
        flag: "--expand",
        syntax: "--expand",
        description: "Expand file variables and pretty-print",
    },
    OptionInfo {
        verb: Verb::Json,
        flag: "--no-check",
        syntax: "--no-check",
        description: "Skip JSON validation and print raw",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--cmd",
        syntax: "--cmd <command>",
        description: "Set the alias command",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--cwd",
        syntax: "--cwd <path>",
        description: "Set the working directory",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--mode",
        syntax: "--mode <mode>",
        description: "Set the execution mode",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--description",
        syntax: "--description <text>",
        description: "Set the description",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--recursive",
        syntax: "--recursive",
        description: "Allow re-entry on the call chain",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--no-recursive",
        syntax: "--no-recursive",
        description: "Remove the recursive flag",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--unset",
        syntax: "--unset <fields>",
        description: "Remove metadata fields",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--priority",
        syntax: "--priority <n>",
        description: "Set the file priority (omit key)",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--paths",
        syntax: "--paths <dir>",
        description: "Prepend directories to PATH (omit key)",
    },
];

impl Verb {
    fn parse(raw: &str) -> Option<Self> {
        VERBS
            .iter()
            .find(|info| info.name == raw)
            .map(|info| info.verb)
    }
}

fn verb_info(verb: Verb) -> &'static VerbInfo {
    VERBS
        .iter()
        .find(|info| info.verb == verb)
        .expect("registered verb")
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileCommand {
    scope: FileScope,
    stem: String,
    verb: Verb,
}

/// 解析结果。出现 `..` 即归文件命令处理，失败时不再回退到别名查找。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseResult {
    NotFileCommand,
    Command(FileCommand),
    Error(FileCommandError),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FileCommandError {
    MissingVerb,
    InvalidVerb(String),
    UnknownVerb(String),
    InvalidStem(String),
}

impl FileCommandError {
    pub(crate) fn render(&self, token: &str) -> String {
        match self {
            Self::MissingVerb => {
                format!(
                    "Invalid file command: {}\nHint: missing verb after `..`",
                    token
                )
            }
            Self::InvalidVerb(verb) => format!(
                "Invalid file command: {}\nHint: invalid verb {:?}",
                token, verb
            ),
            Self::UnknownVerb(verb) => unknown_verb_message(verb),
            Self::InvalidStem(stem) => invalid_stem_message(stem),
        }
    }
}

fn invalid_stem_message(stem: &str) -> String {
    format!(
        "Invalid file name: {:?}\nHint: file name cannot contain `.`, `@`, `/` or spaces",
        stem
    )
}

fn verb_names() -> Vec<&'static str> {
    VERBS.iter().map(|info| info.name).collect()
}

pub(super) fn unknown_verb_message(verb: &str) -> String {
    format!(
        "Unknown file command: {:?}\nAvailable: {}",
        verb,
        verb_names().join(", ")
    )
}

fn split_prefix(token: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = token.strip_prefix("@@") {
        Some(("@@", rest))
    } else {
        token.strip_prefix('@').map(|rest| ("@", rest))
    }
}

/// 文件命令的目标文件名：`<stem>.byk.json`，空 stem 为 `.byk.json`。
fn target_path(base: &Path, stem: &str) -> PathBuf {
    if stem.is_empty() {
        base.join(".byk.json")
    } else {
        base.join(format!("{}.byk.json", stem))
    }
}

/// 按 scope 解析文件命令的目标路径。
fn resolve_target(scope: FileScope, stem: &str, layout: &PathLayout) -> Result<PathBuf, String> {
    match scope {
        FileScope::Local => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("Failed to resolve current directory: {}", e))?;
            Ok(target_path(&cwd, stem))
        }
        FileScope::Global => Ok(target_path(&layout.alias_dir, stem)),
    }
}

/// 目标不存在时的错误；另一 scope 有同名文件时给出切换提示。
fn not_found_message(
    scope: FileScope,
    stem: &str,
    target: &Path,
    layout: &PathLayout,
    verb: &str,
) -> String {
    let mut message = format!("File not found: {}", target.display());

    let other = match scope {
        FileScope::Local => target_path(&layout.alias_dir, stem),
        FileScope::Global => match std::env::current_dir() {
            Ok(cwd) => target_path(&cwd, stem),
            Err(_) => return message,
        },
    };

    if other != target && fs::symlink_metadata(&other).is_ok() {
        let (label, prefix) = match scope {
            FileScope::Local => ("global", "@@"),
            FileScope::Global => ("local", "@"),
        };
        message.push_str(&format!(
            "\nHint: a {} file exists at {}; use {}{}..{}",
            label,
            other.display(),
            prefix,
            stem,
            verb
        ));
    }

    message
}

pub(crate) fn parse(token: &str) -> ParseResult {
    let Some((prefix, rest)) = split_prefix(token) else {
        return ParseResult::NotFileCommand;
    };

    let Some(sep) = rest.find("..") else {
        return ParseResult::NotFileCommand;
    };

    let scope = if prefix == "@@" {
        FileScope::Global
    } else {
        FileScope::Local
    };
    let stem = &rest[..sep];
    let verb = &rest[sep + 2..];

    if verb.is_empty() {
        return ParseResult::Error(FileCommandError::MissingVerb);
    }
    if !is_valid_verb(verb) {
        return ParseResult::Error(FileCommandError::InvalidVerb(verb.to_string()));
    }
    if !validate_filename(stem) {
        return ParseResult::Error(FileCommandError::InvalidStem(stem.to_string()));
    }

    match Verb::parse(verb) {
        Some(verb) => ParseResult::Command(FileCommand {
            scope,
            stem: stem.to_string(),
            verb,
        }),
        None => ParseResult::Error(FileCommandError::UnknownVerb(verb.to_string())),
    }
}

fn is_valid_verb(verb: &str) -> bool {
    verb.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// token 是否属于文件命令命名空间（含非法形式）。
///
/// 供只读接口判断：命中后不能继续当作别名解析或执行。
pub(crate) fn claims(token: &str) -> bool {
    !matches!(parse(token), ParseResult::NotFileCommand)
}

/// 文件命令动词的静态描述；非文件命令或非法形式返回 `None`。
pub(crate) fn describe(token: &str) -> Option<&'static str> {
    match parse(token) {
        ParseResult::Command(command) => Some(verb_info(command.verb).description),
        ParseResult::NotFileCommand | ParseResult::Error(_) => None,
    }
}

/// 文件命令选项的静态描述；未知选项返回 `None`。
pub(crate) fn describe_option(flag: &str) -> Option<&'static str> {
    OPTIONS
        .iter()
        .find(|info| info.flag == flag)
        .map(|info| info.description)
}

fn write_stdout(content: &[u8]) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(content)
        .and_then(|_| stdout.flush())
        .map_err(|e| format!("Failed to write output: {}", e))
}

pub(crate) fn dispatch(command: &FileCommand, args: &[String], layout: &PathLayout) {
    let result = match command.verb {
        Verb::Init => init::run(command.scope, &command.stem, args, layout).map(|path| {
            eprintln!("Created {}", path.display());
        }),
        Verb::Destroy => {
            destroy::run(command.scope, &command.stem, args, layout).map(|outcome| match outcome {
                destroy::Outcome::Removed { path, symlink } => {
                    if symlink {
                        eprintln!("Removed symlink {}", path.display());
                    } else {
                        eprintln!("Removed {}", path.display());
                    }
                }
                destroy::Outcome::Cancelled => eprintln!("Cancelled."),
                destroy::Outcome::Mismatch => {
                    eprintln!("Confirmation does not match. Cancelled.");
                }
            })
        }
        Verb::Clear => {
            clear::run(command.scope, &command.stem, args, layout).map(|outcome| match outcome {
                clear::Outcome::Cleared(path) => eprintln!("Cleared {}", path.display()),
                clear::Outcome::AlreadyEmpty(path) => {
                    eprintln!("Already empty: {}", path.display());
                }
                clear::Outcome::Cancelled => eprintln!("Cancelled."),
                clear::Outcome::Mismatch => {
                    eprintln!("Confirmation does not match. Cancelled.");
                }
            })
        }
        Verb::Set => {
            set::run(command.scope, &command.stem, args, layout).map(|(path, key)| match key {
                Some(key) => eprintln!("Set {} in {}", key, path.display()),
                None => eprintln!("Set file metadata in {}", path.display()),
            })
        }
        Verb::Remove => remove::run(command.scope, &command.stem, args, layout)
            .map(|(path, key)| eprintln!("Removed {} from {}", key, path.display())),
        Verb::Json => json::run(command.scope, &command.stem, args, layout)
            .and_then(|content| write_stdout(&content)),
        Verb::Path => path::run(command.scope, &command.stem, args, layout)
            .and_then(|text| write_stdout(text.as_bytes())),
        Verb::Rename => rename::run(command.scope, &command.stem, args, layout)
            .map(|(from, to)| eprintln!("Renamed {} to {}", from.display(), to.display())),
        Verb::Link => link::run(command.scope, &command.stem, args, layout)
            .map(|(path, target)| eprintln!("Linked {} to {}", path.display(), target.display())),
        Verb::Help => help::run(command.scope, &command.stem, args)
            .and_then(|text| write_stdout(text.as_bytes())),
    };

    if let Err(message) = result {
        eprintln!("{}", message);
        exit(1);
    }
}

/// 补全 `@prefix..verb`；不要求目标文件存在。
///
/// 返回 `None` 表示不是 `..` 形式，调用方继续按别名补全。
pub(crate) fn complete_verbs(partial: &str) -> Option<Vec<String>> {
    let (prefix, rest) = split_prefix(partial)?;
    let sep = rest.find("..")?;
    let stem = &rest[..sep];

    if !validate_filename(stem) {
        return Some(Vec::new());
    }

    let verb_partial = &rest[sep + 2..];
    Some(
        VERBS
            .iter()
            .filter(|info| info.name.starts_with(verb_partial))
            .map(|info| format!("{}{}..{}", prefix, stem, info.name))
            .collect(),
    )
}

/// 补全文件命令后面的选项。
///
/// `None` 表示第一个词不是文件命令，交给原有逻辑；`Some(空)` 表示是文件命令
/// 但当前位置没有候选。
pub(crate) fn complete_options(command_token: &str, partial: &str) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        ParseResult::Error(_) => return Some(Vec::new()),
        ParseResult::NotFileCommand => return None,
    };

    if !partial.starts_with('-') {
        return Some(Vec::new());
    }

    Some(
        OPTIONS
            .iter()
            .filter(|info| info.verb == command.verb)
            .map(|info| info.flag)
            .filter(|flag| flag.starts_with(partial))
            .map(|flag| flag.to_string())
            .collect(),
    )
}

/// 补全 `..help` 后面的命令名。
///
/// `arg_index` 是当前词之前的词数；只有第一个参数位置有候选。
/// 返回 `None` 表示第一个词不是 `..help`。
pub(crate) fn complete_help_args(
    command_token: &str,
    arg_index: usize,
    partial: &str,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };
    if command.verb != Verb::Help {
        return None;
    }
    if arg_index != 1 {
        return Some(Vec::new());
    }

    Some(
        VERBS
            .iter()
            .filter(|info| info.verb != Verb::Help)
            .map(|info| info.name)
            .filter(|name| name.starts_with(partial))
            .map(str::to_string)
            .collect(),
    )
}

/// 补全 `..remove` 后面的已有 key 路径。
///
/// 只有第一个参数位置有候选；链接文件、缺失或非法文件不补。
pub(crate) fn complete_remove_args(
    command_token: &str,
    arg_index: usize,
    partial: &str,
    layout: &PathLayout,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };
    if command.verb != Verb::Remove {
        return None;
    }
    if arg_index != 1 {
        return Some(Vec::new());
    }
    if partial.starts_with('-') {
        return None;
    }

    let Ok(path) = resolve_target(command.scope, &command.stem, layout) else {
        return Some(Vec::new());
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Some(Vec::new());
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&text) else {
        return Some(Vec::new());
    };

    let mut keys = Vec::new();
    collect_removable_keys(&object, "", &mut keys);
    Some(
        keys.into_iter()
            .filter(|key| key.starts_with(partial))
            .collect(),
    )
}

fn collect_removable_keys(object: &Map<String, Value>, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in object {
        if key.starts_with('$') {
            continue;
        }
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        match value {
            Value::Object(inner) => {
                out.push(path.clone());
                collect_removable_keys(inner, &path, out);
            }
            Value::String(_) | Value::Bool(_) => out.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(token: &str) -> FileCommand {
        match parse(token) {
            ParseResult::Command(command) => command,
            other => panic!("expected command, got {:?}", other),
        }
    }

    fn parse_err(token: &str) -> FileCommandError {
        match parse(token) {
            ParseResult::Error(err) => err,
            other => panic!("expected error, got {:?}", other),
        }
    }

    #[test]
    fn parse_global_init() {
        assert_eq!(
            parse_ok("@@ops..init"),
            FileCommand {
                scope: FileScope::Global,
                stem: "ops".into(),
                verb: Verb::Init,
            }
        );
    }

    #[test]
    fn parse_local_init() {
        assert_eq!(
            parse_ok("@ops..init"),
            FileCommand {
                scope: FileScope::Local,
                stem: "ops".into(),
                verb: Verb::Init,
            }
        );
    }

    #[test]
    fn parse_anonymous_files() {
        assert_eq!(parse_ok("@@..init").stem, "");
        assert_eq!(parse_ok("@..init").scope, FileScope::Local);
    }

    #[test]
    fn parse_exact_alias_is_not_a_file_command() {
        assert_eq!(parse("@@ops.init"), ParseResult::NotFileCommand);
        assert_eq!(parse("@ops.init.prod"), ParseResult::NotFileCommand);
        assert_eq!(parse("ops.init"), ParseResult::NotFileCommand);
    }

    #[test]
    fn parse_missing_verb() {
        assert_eq!(parse_err("@@ops.."), FileCommandError::MissingVerb);
        assert_eq!(parse_err("@@.."), FileCommandError::MissingVerb);
    }

    #[test]
    fn parse_invalid_verb() {
        assert_eq!(
            parse_err("@@ops...init"),
            FileCommandError::InvalidVerb(".init".into())
        );
        assert_eq!(
            parse_err("@@ops..init.prod"),
            FileCommandError::InvalidVerb("init.prod".into())
        );
    }

    #[test]
    fn parse_unknown_verb() {
        assert_eq!(
            parse_err("@@ops..explode"),
            FileCommandError::UnknownVerb("explode".into())
        );
    }

    #[test]
    fn parse_invalid_stem() {
        assert_eq!(
            parse_err("@@a/b..init"),
            FileCommandError::InvalidStem("a/b".into())
        );
        assert_eq!(
            parse_err("@@a.b..init"),
            FileCommandError::InvalidStem("a.b".into())
        );
        assert_eq!(
            parse_err("@@a b..init"),
            FileCommandError::InvalidStem("a b".into())
        );
        assert_eq!(
            parse_err("@@@ops..init"),
            FileCommandError::InvalidStem("@ops".into())
        );
    }

    #[test]
    fn claims_file_command_namespace() {
        assert!(claims("@@ops..init"));
        assert!(claims("@@ops.."));
        assert!(claims("@@ops..explode"));
        assert!(!claims("@@ops.init"));
        assert!(!claims("normal"));
    }

    #[test]
    fn describe_file_command_verbs() {
        assert_eq!(describe("@@ops..init"), Some("Create an alias file"));
        assert_eq!(describe("@ops..destroy"), Some("Remove the alias file"));
        assert_eq!(
            describe("@@ops..clear"),
            Some("Reset this file to an empty object")
        );
        assert_eq!(describe("@@ops..set"), Some("Create or update an alias"));
        assert_eq!(describe("@@ops..remove"), Some("Remove an alias or group"));
        assert_eq!(describe("@@..json"), Some("Show the alias file JSON"));
        assert_eq!(describe("@@ops..path"), Some("Print the absolute path"));
        assert_eq!(describe("@@ops..rename"), Some("Rename the alias file"));
        assert_eq!(
            describe("@@ops..link"),
            Some("Point this file at another alias file")
        );
        assert_eq!(describe("@@ops..help"), Some("Show this help"));
        assert_eq!(describe("@@ops.init"), None);
        assert_eq!(describe("@ops"), None);
    }

    #[test]
    fn parse_global_destroy() {
        assert_eq!(
            parse_ok("@@ops..destroy"),
            FileCommand {
                scope: FileScope::Global,
                stem: "ops".into(),
                verb: Verb::Destroy,
            }
        );
    }

    #[test]
    fn target_path_named_and_anonymous() {
        assert_eq!(
            target_path(Path::new("/base"), "ops"),
            PathBuf::from("/base/ops.byk.json")
        );
        assert_eq!(
            target_path(Path::new("/base"), ""),
            PathBuf::from("/base/.byk.json")
        );
    }

    #[test]
    fn complete_after_two_dots() {
        assert_eq!(
            complete_verbs("@@ops.."),
            Some(vec![
                "@@ops..init".to_string(),
                "@@ops..destroy".to_string(),
                "@@ops..clear".to_string(),
                "@@ops..set".to_string(),
                "@@ops..remove".to_string(),
                "@@ops..json".to_string(),
                "@@ops..path".to_string(),
                "@@ops..rename".to_string(),
                "@@ops..link".to_string(),
                "@@ops..help".to_string()
            ])
        );
        assert_eq!(
            complete_verbs("@@ops..i"),
            Some(vec!["@@ops..init".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..d"),
            Some(vec!["@@ops..destroy".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..c"),
            Some(vec!["@@ops..clear".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..s"),
            Some(vec!["@@ops..set".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..j"),
            Some(vec!["@@ops..json".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..p"),
            Some(vec!["@@ops..path".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..r"),
            Some(vec![
                "@@ops..remove".to_string(),
                "@@ops..rename".to_string()
            ])
        );
        assert_eq!(
            complete_verbs("@@ops..l"),
            Some(vec!["@@ops..link".to_string()])
        );
        assert_eq!(complete_verbs("@..i"), Some(vec!["@..init".to_string()]));
    }

    #[test]
    fn complete_ignores_single_dot() {
        assert_eq!(complete_verbs("@@ops.init"), None);
        assert_eq!(complete_verbs("@@ops."), None);
        assert_eq!(complete_verbs("ops"), None);
    }

    #[test]
    fn complete_options_per_verb() {
        assert_eq!(
            complete_options("@@ops..init", "--"),
            Some(vec!["--link".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..init", "--l"),
            Some(vec!["--link".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..json", "--"),
            Some(vec!["--expand".to_string(), "--no-check".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..json", "--e"),
            Some(vec!["--expand".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..json", "--n"),
            Some(vec!["--no-check".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..destroy", "--"),
            Some(vec!["--yes".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..clear", "--"),
            Some(vec!["--yes".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..set", "--"),
            Some(vec![
                "--cmd".to_string(),
                "--cwd".to_string(),
                "--mode".to_string(),
                "--description".to_string(),
                "--recursive".to_string(),
                "--no-recursive".to_string(),
                "--unset".to_string(),
                "--priority".to_string(),
                "--paths".to_string()
            ])
        );
        assert_eq!(
            complete_options("@@ops..set", "--c"),
            Some(vec!["--cmd".to_string(), "--cwd".to_string()])
        );
        assert_eq!(complete_options("@@ops..link", "--"), Some(Vec::new()));
        assert_eq!(complete_options("@@ops..remove", "--"), Some(Vec::new()));
        assert_eq!(complete_options("@@ops..json", ""), Some(Vec::new()));
        assert_eq!(complete_options("@@ops..json", "x"), Some(Vec::new()));
        assert_eq!(complete_options("normal", "--"), None);
    }

    #[test]
    fn complete_help_args_lists_verbs_without_help() {
        assert_eq!(
            complete_help_args("@@ops..help", 1, ""),
            Some(
                [
                    "init", "destroy", "clear", "set", "remove", "json", "path", "rename", "link"
                ]
                .iter()
                .map(|name| name.to_string())
                .collect()
            )
        );
        assert_eq!(
            complete_help_args("@@ops..help", 1, "s"),
            Some(vec!["set".to_string()])
        );
        assert_eq!(complete_help_args("@@ops..help", 2, ""), Some(Vec::new()));
        assert_eq!(complete_help_args("@@ops..set", 1, ""), None);
        assert_eq!(complete_help_args("bogus", 1, ""), None);
    }

    #[test]
    fn complete_remove_args_lists_keys_and_groups() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":"echo hi","web":{"$cwd":".","deploy":"echo d"},"$ignored":"x"}"#,
        )
        .unwrap();

        assert_eq!(
            complete_remove_args("@@ops..remove", 1, "", &layout),
            Some(vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ])
        );
        assert_eq!(
            complete_remove_args("@@ops..remove", 1, "web.", &layout),
            Some(vec!["web.deploy".to_string()])
        );
        assert_eq!(
            complete_remove_args("@@ops..remove", 2, "", &layout),
            Some(Vec::new())
        );
        assert_eq!(complete_remove_args("@@ops..set", 1, "", &layout), None);
        assert_eq!(complete_remove_args("bogus", 1, "", &layout), None);
    }

    #[test]
    fn complete_remove_args_skips_link_files() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"["shared.byk.json"]"#,
        )
        .unwrap();

        assert_eq!(
            complete_remove_args("@@ops..remove", 1, "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_invalid_stem_is_empty() {
        assert_eq!(complete_verbs("@@a/b.."), Some(Vec::new()));
    }
}
