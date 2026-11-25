//! 文件命令的共享语法：scope、token 解析与 verb/option 注册表。
//!
//! 路由层（`src/alias/filecmd`）和 ops 业务共用这里，避免互相依赖。

use crate::model::Scope;
use crate::resolve::validate_filename;

/// `deps` 的子命令与说明；顺序即帮助、错误提示与补全顺序。
pub(crate) const SUBCOMMANDS: [(&str, &str); 3] = [
    (
        "install",
        "Install the declared files, pip and npm packages",
    ),
    ("uninstall", "Remove this file's dependency directory"),
    ("status", "Show what is declared, installed, or stale"),
];

pub(crate) fn subcommand_names() -> impl Iterator<Item = &'static str> {
    SUBCOMMANDS.iter().map(|(name, _)| *name)
}

pub(crate) fn describe_subcommand(name: &str) -> Option<&'static str> {
    SUBCOMMANDS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, description)| *description)
}

/// `local@` 指向当前目录，`global@` 指向全局别名目录。
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

    /// 命令里写的作用域前缀，用于拼提示文案。
    pub(crate) fn token(self) -> &'static str {
        match self {
            FileScope::Local => Scope::Local.token(),
            FileScope::Global => Scope::Global.token(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileCommand {
    pub(crate) scope: FileScope,
    pub(crate) stem: String,
    pub(crate) verb: Verb,
    /// 动词后面的参数。
    ///
    /// 帮助请求把目标命令放在这里：`init -h` → `["init"]`。
    pub(crate) args: Vec<String>,
}

/// 解析结果。`local@file` / `global@file` / `sys@file` 一旦归文件命令处理，失败时不再回退到别名查找。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseResult {
    NotFileCommand,
    Command(FileCommand),
    /// `sys@<name> <args...>`：系统文件命令，动词与语法归各自模块。
    ///
    /// 名字原样带过来，合法性与未知名单由 `sys` 判——哪些文件算系统文件是它的知识。
    System {
        stem: String,
        args: Vec<String>,
    },
    Error(FileCommandError),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FileCommandError {
    UnknownVerb(String),
    InvalidStem(String),
    /// `@` 左侧像作用域名，却不是任何作用域的前缀。
    UnknownScope(String),
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
            Self::UnknownScope(head) => {
                format!("Unknown scope: {}@\nAvailable: {}", head, Scope::tokens())
            }
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
        description: "Create this alias file",
        usage: "",
    },
    VerbInfo {
        name: "destroy",
        verb: Verb::Destroy,
        description: "Delete this alias file",
        usage: "",
    },
    VerbInfo {
        name: "clear",
        verb: Verb::Clear,
        description: "Reset this alias file to an empty object",
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
        description: "Show this alias file's JSON",
        usage: "",
    },
    VerbInfo {
        name: "path",
        verb: Verb::Path,
        description: "Print this alias file's absolute path",
        usage: "",
    },
    VerbInfo {
        name: "rename",
        verb: Verb::Rename,
        description: "Rename this alias file",
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
        usage: "[COMMAND]",
    },
];

/// 文件命令的帮助开关；`-h` / `--help` 不是命令，而是选项。
pub(crate) const HELP_FLAGS: [&str; 2] = ["--help", "-h"];

pub(crate) fn is_help_flag(arg: &str) -> bool {
    HELP_FLAGS.contains(&arg)
}

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
pub(crate) const OPTION_ALIASES: &[(&str, &str)] = &[("-t", "--tap"), ("-d", "--deps")];

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

/// `local@file` / `global@file` 的文件命令头；带点号的是精确别名语法，返回 `None`。
///
/// `sys@` 不是别名文件，也不参与点号规则：它交给 `sys`，由那边决定名字是否合法。
pub(crate) fn split_head(token: &str) -> Option<(FileScope, &str)> {
    let (scope, rest) = Scope::split(token)?;
    if rest.contains('.') {
        return None;
    }
    let scope = match scope {
        Scope::Local => FileScope::Local,
        Scope::Global => FileScope::Global,
        Scope::Sys => return None,
    };
    Some((scope, rest))
}

/// token 是否是文件命令头（不看后面的动词）。
///
/// `sys@` 不算：它没有动词表，参数含义由目标文件自己定。
pub(crate) fn is_file_head(token: &str) -> bool {
    split_head(token).is_some()
}

/// 解析 `local@file <verb> [args...]` / `global@file <verb> [args...]` / `sys@file [args...]`。
///
/// 首个参数是动词；缺省动词、`-h` / `--help`、以及动词后的帮助开关都走帮助。
/// `sys@` 只切出文件名，后面的参数原样带走——有没有动词、`-h` 算什么，都由该文件自己定。
pub(crate) fn parse(command_name: &str, args: &[String]) -> ParseResult {
    if let Some((Scope::Sys, stem)) = Scope::split(command_name) {
        return ParseResult::System {
            stem: stem.to_string(),
            args: args.to_vec(),
        };
    }

    let Some((scope, stem)) = split_head(command_name) else {
        if let Some(head) = Scope::unknown_prefix(command_name) {
            return ParseResult::Error(FileCommandError::UnknownScope(head.to_string()));
        }
        return ParseResult::NotFileCommand;
    };

    if !validate_filename(stem) {
        return ParseResult::Error(FileCommandError::InvalidStem(stem.to_string()));
    }

    let build = |verb: Verb, args: Vec<String>| {
        ParseResult::Command(FileCommand {
            scope,
            stem: stem.to_string(),
            verb,
            args,
        })
    };

    let Some(name) = args.first().map(String::as_str) else {
        return build(Verb::Help, Vec::new());
    };
    if is_help_flag(name) {
        return build(Verb::Help, Vec::new());
    }

    let Some(verb) = Verb::parse(name) else {
        return ParseResult::Error(FileCommandError::UnknownVerb(name.to_string()));
    };
    let rest = &args[1..];
    if rest.iter().any(|arg| is_help_flag(arg)) {
        return build(Verb::Help, vec![name.to_string()]);
    }

    build(verb, rest.to_vec())
}

/// token 是否属于文件命令命名空间（含 `sys@` 系统文件与非法形式）。
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
