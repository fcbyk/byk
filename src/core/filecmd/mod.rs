//! 文件命令：绑定到别名文件的保留命令。
//!
//! 语法为 `@<file>..<verb>` / `@@<file>..<verb>`，`..` 是保留分隔符。
//! 文件命令独立于别名扫描、合并与执行，不进入合并命名空间。

mod clear;
mod clone;
mod command;
mod complete;
mod create;
mod deps;
mod destroy;
mod edit;
mod help;
mod init;
mod json;
mod key;
mod link;
mod path;
mod prompt;
mod registry;
mod remove;
mod rename;
mod set;
mod source;
mod summary;

use std::io::Write;
use std::path::PathBuf;
use std::process::exit;

use crate::core::paths::PathLayout;

use command::{invalid_stem_message, unknown_verb_message};
use registry::Verb;

pub(crate) use command::{address_for_key, claims, describe, describe_option, parse};
pub(crate) use complete::{
    complete_deps_args, complete_help_args, complete_options, complete_remove_args,
    complete_source_args, complete_verbs,
};

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

/// `__api__ file` 的返回内容：key 对应的 scope 与目标文件路径。
pub(crate) struct FileAddress {
    pub scope: FileScope,
    pub path: PathBuf,
}

/// 依赖条数的文案；安装前的标题与安装后的总结共用。
pub(super) fn count_files(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
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
            let stem = command.stem.clone();
            destroy::run(command.scope, &command.stem, args, layout).map(|outcome| match outcome {
                destroy::Outcome::Removed { path, symlink } => {
                    // 符号链接删的是链接本身，目标的依赖目录不能跟着没
                    if !symlink {
                        deps::remove_dir(command.scope, &stem, layout);
                    }
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
        Verb::Rename => {
            let new_stem = args.first().cloned();
            rename::run(command.scope, &command.stem, args, layout).map(|(from, to)| {
                if let Some(new_stem) = new_stem {
                    deps::rename_dir(command.scope, &command.stem, &new_stem, layout);
                }
                eprintln!("Renamed {} to {}", from.display(), to.display());
            })
        }
        Verb::Clone => clone::run(command.scope, &command.stem, args, layout),
        Verb::Link => {
            link::run(command.scope, &command.stem, args, layout).map(|(path, target)| {
                deps::warn_stale_dir(command.scope, &command.stem, layout);
                eprintln!("Linked {} to {}", path.display(), target.display());
            })
        }
        Verb::Deps => {
            deps::run(command.scope, &command.stem, args, layout).map(|outcome| match outcome {
                // 安装过程自己打完输出，这里不用再报一遍
                deps::Outcome::Installed => {}
                deps::Outcome::Removed { dir } => {
                    eprintln!("Removed {}", dir.display());
                }
                deps::Outcome::Status(text) => {
                    if let Err(message) = write_stdout(text.as_bytes()) {
                        eprintln!("{}", message);
                        exit(1);
                    }
                }
                deps::Outcome::Cancelled => eprintln!("Cancelled."),
                deps::Outcome::Mismatch => {
                    eprintln!("Confirmation does not match. Cancelled.");
                }
            })
        }
        Verb::Help => help::run(command.scope, &command.stem, args)
            .and_then(|text| write_stdout(text.as_bytes())),
    };

    if let Err(message) = result {
        eprintln!("{}", message);
        exit(1);
    }
}
