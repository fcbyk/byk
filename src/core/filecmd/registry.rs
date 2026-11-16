//! 文件命令的动词与选项注册表。
//!
//! 这里的顺序即帮助、报错与补全的输出顺序，新增动词或选项时改这里。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verb {
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

pub(super) struct VerbInfo {
    pub(super) name: &'static str,
    pub(super) verb: Verb,
    pub(super) description: &'static str,
    /// 位置参数片段；`[options]` 由是否有选项自动追加。
    pub(super) usage: &'static str,
}

/// 已注册的动词；顺序即错误提示、补全与帮助输出顺序。
pub(super) const VERBS: &[VerbInfo] = &[
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

pub(super) struct OptionInfo {
    pub(super) verb: Verb,
    pub(super) flag: &'static str,
    pub(super) syntax: &'static str,
    pub(super) description: &'static str,
}

/// 文件命令选项；顺序即补全与帮助输出顺序。
pub(super) const OPTIONS: &[OptionInfo] = &[
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
        syntax: "-gh, --github <spec>",
        description: "Clone from a GitHub repository",
    },
    OptionInfo {
        verb: Verb::Clone,
        flag: "--gitee",
        syntax: "-ge, --gitee <spec>",
        description: "Clone from a Gitee repository",
    },
    OptionInfo {
        verb: Verb::Clone,
        flag: "--jsdelivr",
        syntax: "-jd, --jsdelivr <spec>",
        description: "Clone a GitHub file through jsDelivr",
    },
    OptionInfo {
        verb: Verb::Clone,
        flag: "--deps",
        syntax: "-d, --deps",
        description: "Also install the cloned file's dependencies",
    },
    OptionInfo {
        verb: Verb::Deps,
        flag: "--yes",
        syntax: "--yes",
        description: "Skip confirmation",
    },
];

/// 选项别名：短写 → 完整 flag；帮助里的展示由 `syntax` 负责。
pub(super) const OPTION_ALIASES: &[(&str, &str)] = &[
    ("-gh", "--github"),
    ("-ge", "--gitee"),
    ("-jd", "--jsdelivr"),
    ("-d", "--deps"),
];

impl Verb {
    pub(super) fn parse(raw: &str) -> Option<Self> {
        VERBS
            .iter()
            .find(|info| info.name == raw)
            .map(|info| info.verb)
    }
}

pub(super) fn verb_info(verb: Verb) -> &'static VerbInfo {
    VERBS
        .iter()
        .find(|info| info.verb == verb)
        .expect("registered verb")
}
