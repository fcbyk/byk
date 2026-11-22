//! 文件命令路由：解析 `@file <verb>` / `@@file <verb>`，在别名查找之前处理。

pub(crate) mod io;
pub(crate) mod prompt;
pub(crate) mod source;
pub(crate) mod spec;
mod verbs;

use crate::filecmd::spec::{FileCommand, ParseResult, Verb, parse};
use crate::model::PathLayout;
use crate::utils::io::write_stdout;

use verbs::{clear, clone, deps, destroy, help, init, json, link, path, remove, rename, set};

/// 命中文件命令时返回退出码；未命中返回 `None`，交由别名查找。
pub(crate) fn run(command_name: &str, args: &[String], layout: &PathLayout) -> Option<i32> {
    match parse(command_name, args) {
        ParseResult::NotFileCommand => None,
        ParseResult::Command(command) => {
            Some(dispatch(&command, &args[command.arg_start..], layout))
        }
        ParseResult::Error(err) => {
            eprintln!("{}", err.render(command_name));
            Some(1)
        }
    }
}

fn dispatch(command: &FileCommand, args: &[String], layout: &PathLayout) -> i32 {
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
        Verb::Deps => match deps::run(command.scope, &command.stem, args, layout) {
            // 安装过程自己打完输出，这里不用再报一遍
            Ok(deps::Outcome::Installed) => Ok(()),
            Ok(deps::Outcome::Removed { dir }) => {
                eprintln!("Removed {}", dir.display());
                Ok(())
            }
            Ok(deps::Outcome::Status(text)) => write_stdout(text.as_bytes()),
            Ok(deps::Outcome::Cancelled) => {
                eprintln!("Cancelled.");
                Ok(())
            }
            Ok(deps::Outcome::Mismatch) => {
                eprintln!("Confirmation does not match. Cancelled.");
                Ok(())
            }
            Err(message) => Err(message),
        },
        Verb::Help => help::run(command.scope, &command.stem, args)
            .and_then(|text| write_stdout(text.as_bytes())),
    };

    if let Err(message) = result {
        eprintln!("{}", message);
        return 1;
    }
    0
}
