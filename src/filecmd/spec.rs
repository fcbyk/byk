//! 文件命令的共享语法：scope、token 解析与 verb/option 注册表。
//!
//! 路由层（`src/alias/filecmd`）和 ops 业务共用这里，避免互相依赖。

use crate::resolve::validate_filename;

/// `deps` 的子命令；顺序即错误提示与补全顺序。
pub(crate) const SUBCOMMANDS: [&str; 3] = ["install", "uninstall", "status"];

/// `@` 指向当前目录，`@@` 指向全局别名目录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileScope {
    Local,
    Global,
}

impl FileScope {
    pub(crate) fn is_global(self) -> bool {
        self == FileScope::Global
    }

    /// 面向 `__api__` JSON 的小写名。
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FileScope::Local => "local",
            FileScope::Global => "global",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileCommand {
    pub(crate) scope: FileScope,
    pub(crate) stem: String,
    pub(crate) verb: Verb,
    /// 动词自身参数在完整命令行参数里的起始下标。
    ///
    /// 缺省动词时没有参数；`-h` / `--help` 跳过这个 flag 本身。
    pub(crate) arg_start: usize,
}

/// 解析结果。`@file` / `@@file` 一旦归文件命令处理，失败时不再回退到别名查找。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseResult {
    NotFileCommand,
    Command(FileCommand),
    Error(FileCommandError),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FileCommandError {
    UnknownVerb(String),
    InvalidStem(String),
}

impl FileCommandError {
    pub(crate) fn render(&self, token: &str) -> String {
        match self {
            Self::UnknownVerb(verb) => format!(
                "{}\nHint: to run an alias from this file, use {}.<key>",
                unknown_verb_message(verb),
                token
            ),
            Self::InvalidStem(stem) => invalid_stem_message(stem),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verb {
    Init,
    Destroy,
    Clear,
    Set,
    Remove,
    Json,
    Path,
    Rename,
    Clone,
    Link,
    Deps,
    Help,
}

pub(crate) struct VerbInfo {
    pub(crate) name: &'static str,
    pub(crate) verb: Verb,
    pub(crate) description: &'static str,
    /// 位置参数片段；`[options]` 由是否有选项自动追加。
    pub(crate) usage: &'static str,
}

/// 已注册的动词；顺序即错误提示、补全与帮助输出顺序。
pub(crate) const VERBS: &[VerbInfo] = &[
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
        name: "clone",
        verb: Verb::Clone,
        description: "Copy a local or remote alias file into this file",
        usage: "<source>",
    },
    VerbInfo {
        name: "link",
        verb: Verb::Link,
        description: "Point this file at another alias file",
        usage: "<target>",
    },
    VerbInfo {
        name: "deps",
        verb: Verb::Deps,
        description: "Install, uninstall, or inspect this file's dependencies",
        usage: "[install|uninstall|status]",
    },
    VerbInfo {
        name: "help",
        verb: Verb::Help,
        description: "Show this help",
        usage: "[command]",
    },
];

pub(crate) struct OptionInfo {
    pub(crate) verb: Verb,
    pub(crate) flag: &'static str,
    pub(crate) syntax: &'static str,
    pub(crate) description: &'static str,
}

/// 文件命令选项；顺序即补全与帮助输出顺序。
pub(crate) const OPTIONS: &[OptionInfo] = &[
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
        flag: "--private",
        syntax: "--private",
        description: "Keep this file or alias out of merged lookup",
    },
    OptionInfo {
        verb: Verb::Set,
        flag: "--no-private",
        syntax: "--no-private",
        description: "Expose it to merged lookup again",
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
    OptionInfo {
        verb: Verb::Clone,
        flag: "--github",
        syntax: "-gh, --github <spec> [<cdn>]",
        description: "Clone from GitHub, optionally through a CDN (raw, jsdelivr, ghproxy)",
    },
    OptionInfo {
        verb: Verb::Clone,
        flag: "--gitee",
        syntax: "-ge, --gitee <spec>",
        description: "Clone from a Gitee repository",
    },
    OptionInfo {
        verb: Verb::Clone,
        flag: "--tap",
        syntax: "-t, --tap <spec>",
        description: "Resolve a key from a local or GitHub byk.tap.json index",
    },
    OptionInfo {
        verb: Verb::Clone,
        flag: "--deps",
        syntax: "-d, --deps",
        description: "Also install the cloned file's dependencies",
    },
    OptionInfo {
        verb: Verb::Deps,
        flag: "--force",
        syntax: "--force",
        description: "Reinstall dependencies from source (install)",
    },
    OptionInfo {
        verb: Verb::Deps,
        flag: "--yes",
        syntax: "--yes",
        description: "Skip the uninstall confirmation",
    },
];

/// 选项别名：短写 → 完整 flag；帮助里的展示由 `syntax` 负责。
pub(crate) const OPTION_ALIASES: &[(&str, &str)] = &[
    ("-gh", "--github"),
    ("-ge", "--gitee"),
    ("-t", "--tap"),
    ("-d", "--deps"),
];

impl Verb {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        VERBS
            .iter()
            .find(|info| info.name == raw)
            .map(|info| info.verb)
    }
}

pub(crate) fn invalid_stem_message(stem: &str) -> String {
    format!(
        "Invalid file name: {:?}\nHint: file name cannot contain `.`, `@`, `/` or spaces",
        stem
    )
}

fn verb_names() -> Vec<&'static str> {
    VERBS.iter().map(|info| info.name).collect()
}

pub(crate) fn unknown_verb_message(verb: &str) -> String {
    format!(
        "Unknown file command: {:?}\nAvailable: {}",
        verb,
        verb_names().join(", ")
    )
}

pub(crate) fn split_prefix(token: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = token.strip_prefix("@@") {
        Some(("@@", rest))
    } else {
        token.strip_prefix('@').map(|rest| ("@", rest))
    }
}

/// `@file` / `@@file` 的文件命令头；带点号的是精确别名语法，返回 `None`。
pub(crate) fn split_head(token: &str) -> Option<(&'static str, &str)> {
    let (prefix, rest) = split_prefix(token)?;
    if rest.contains('.') {
        return None;
    }
    Some((prefix, rest))
}

/// token 是否是文件命令头（不看后面的动词）。
pub(crate) fn is_file_head(token: &str) -> bool {
    split_head(token).is_some()
}

/// 解析 `@file <verb> [args...]` / `@@file <verb> [args...]`。
///
/// 首个参数是动词；缺省动词和 `-h` / `--help` 都映射到 `help`。
pub(crate) fn parse(command_name: &str, args: &[String]) -> ParseResult {
    let Some((prefix, stem)) = split_head(command_name) else {
        return ParseResult::NotFileCommand;
    };
    let scope = if prefix == "@@" {
        FileScope::Global
    } else {
        FileScope::Local
    };

    if !validate_filename(stem) {
        return ParseResult::Error(FileCommandError::InvalidStem(stem.to_string()));
    }

    let (verb, arg_start) = match args.first().map(String::as_str) {
        None => (Verb::Help, 0),
        Some("-h") | Some("--help") => (Verb::Help, 1),
        Some(name) => match Verb::parse(name) {
            Some(verb) => (verb, 1),
            None => return ParseResult::Error(FileCommandError::UnknownVerb(name.to_string())),
        },
    };

    ParseResult::Command(FileCommand {
        scope,
        stem: stem.to_string(),
        verb,
        arg_start,
    })
}

/// token 是否属于文件命令命名空间（含非法形式）。
///
/// 供只读接口判断：命中后不能继续当作别名解析或执行。
pub(crate) fn claims(command_name: &str, args: &[String]) -> bool {
    !matches!(parse(command_name, args), ParseResult::NotFileCommand)
}

/// 动词的静态描述；未知动词返回 `None`。
pub(crate) fn describe_verb(name: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|info| info.name == name)
        .map(|info| info.description)
}

/// 文件命令选项的静态描述；未知选项返回 `None`。
pub(crate) fn describe_option(flag: &str) -> Option<&'static str> {
    let canonical = OPTION_ALIASES
        .iter()
        .find(|(alias, _)| *alias == flag)
        .map(|(_, flag)| *flag)
        .unwrap_or(flag);

    OPTIONS
        .iter()
        .find(|info| info.flag == canonical)
        .map(|info| info.description)
}
