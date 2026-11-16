//! `..deps`：安装、卸载或查看别名文件声明的依赖。
//!
//! 语法为 `<scope><stem>..deps <verb>`，verb 取 `install` / `uninstall` /
//! `status` 之一，也认无歧义前缀（`i` / `u` / `s`）。不带 verb 等于 `status`。
//! 全局文件的依赖目录是 `~/.byk/deps/@@<stem>/`，局部文件是
//! `<文件所在目录>/.byk/deps/@<stem>/`：目录本身就是身份，跟着项目走。
//!
//! install 读链接文件并跟随指针，`$deps` 来自目标文档，落点却跟着链接
//! 文件的 key 走：声明跟着文件走，资产跟着身份走。

mod install;
mod status;

use std::fs;
use std::path::{Path, PathBuf};

use super::FileScope;
use super::command::resolve_target;
use super::prompt::{self, Confirmation};
use crate::core::aliases::{follow_link, load_document};
use crate::core::config::{Diagnostic, Level, build_builtin_table, build_file_key};
use crate::core::deps::{self, Declaration, parse_deps};
use crate::core::paths::PathLayout;

pub(super) use install::install;
use status::status;

/// 被寻址文件所在的目录：全局固定是别名目录，局部就是当前目录。
fn file_dir_of(scope: FileScope, layout: &PathLayout) -> PathBuf {
    match scope {
        FileScope::Global => layout.alias_dir.clone(),
        FileScope::Local => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// 提示里那条可复制的命令：`@@hello..deps i` / `@hello..deps i`。
fn deps_command(scope: FileScope, stem: &str) -> String {
    format!("byk {}..deps i", build_file_key(stem, scope.is_global()))
}

fn deps_dir_of(scope: FileScope, stem: &str, layout: &PathLayout) -> PathBuf {
    deps::deps_dir(
        stem,
        scope.is_global(),
        &file_dir_of(scope, layout),
        &layout.root_dir,
    )
}

/// 一行动作的标签：`<key>/<dest>`，根落点只显示 key。
fn label_of(key: &str, dest: &str) -> String {
    if dest.is_empty() {
        key.to_string()
    } else {
        format!("{key}/{dest}")
    }
}

#[derive(Debug)]
pub(super) enum Outcome {
    /// 安装完成；输出已由 install 自己打完，调用方无需再报。
    Installed,
    Removed {
        dir: PathBuf,
    },
    /// 只读状态：文本写 stdout，方便管道。
    Status(String),
    Cancelled,
    Mismatch,
}

/// 子命令；顺序即错误提示与补全顺序。
const SUBCOMMANDS: [&str; 3] = ["install", "uninstall", "status"];

#[derive(Debug)]
enum Action {
    Install { force: bool },
    Uninstall { yes: bool },
    Status,
}

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<Outcome, String> {
    match parse_args(args)? {
        Action::Status => status(scope, stem, layout),
        Action::Install { force } => {
            let (target, declared) = declaration(scope, stem, layout)?;
            let decl = declared.ok_or_else(|| no_declaration(&target))?;
            install(scope, stem, layout, &decl, force)?;
            Ok(Outcome::Installed)
        }
        Action::Uninstall { yes } => uninstall(scope, stem, &deps_dir_of(scope, stem, layout), yes),
    }
}

fn parse_args(args: &[String]) -> Result<Action, String> {
    let Some((verb, rest)) = args.split_first() else {
        return Ok(Action::Status);
    };

    let candidates: Vec<&str> = SUBCOMMANDS
        .iter()
        .copied()
        .filter(|name| name.starts_with(verb.as_str()))
        .collect();
    let name = match candidates.as_slice() {
        [name] => *name,
        [] => {
            return Err(format!(
                "Unknown deps subcommand: {:?}\nAvailable: {}",
                verb,
                SUBCOMMANDS.join(", ")
            ));
        }
        _ => {
            return Err(format!(
                "Ambiguous deps subcommand: {:?}\nAvailable: {}",
                verb,
                candidates.join(", ")
            ));
        }
    };

    match name {
        // 安装不做二次确认：`--yes` 已经没有意义，直接说不支持比静默忽略好。
        "install" => match rest {
            [] => Ok(Action::Install { force: false }),
            [flag] if flag == "--force" => Ok(Action::Install { force: true }),
            [flag, ..] => Err(format!(
                "Unknown option: {}\nHint: deps install runs without confirmation; the only option is --force",
                flag
            )),
        },
        "uninstall" => match rest {
            [] => Ok(Action::Uninstall { yes: false }),
            [flag] if flag == "--yes" => Ok(Action::Uninstall { yes: true }),
            _ => Err("deps uninstall accepts only --yes".to_string()),
        },
        _ => match rest {
            [] => Ok(Action::Status),
            _ => Err("deps status takes no options".to_string()),
        },
    }
}

/// 读 `$deps`；`None` 表示文件没有这个字段。链接会跟随到目标。
pub(super) fn declaration(
    scope: FileScope,
    stem: &str,
    layout: &PathLayout,
) -> Result<(PathBuf, Option<Declaration>), String> {
    let target = resolve_target(scope, stem, layout)?;
    if !target.is_file() {
        return Err(format!("File not found: {}", target.display()));
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let vars = build_builtin_table(
        &layout.root_dir,
        &std::env::current_dir().unwrap_or_default(),
    );
    let deps_dir = deps_dir_of(scope, stem, layout)
        .to_string_lossy()
        .into_owned();

    let (link_data, link_path) =
        load_document(&target, &vars, Some(&deps_dir), &target, &mut diagnostics)
            .map_err(|_| first_error(&diagnostics))?;
    let (data, _) = follow_link(
        link_data,
        &link_path,
        &vars,
        Some(&deps_dir),
        &mut diagnostics,
    )
    .ok_or_else(|| first_error(&diagnostics))?;

    print_warnings(&diagnostics);

    let Some(value) = data.get("$deps") else {
        return Ok((target, None));
    };
    parse_deps(value).map(|decl| (target, Some(decl)))
}

fn no_declaration(target: &Path) -> String {
    format!(
        "This file declares no \"$deps\": {}\nHint: add a \"$deps\" object with a \"files\", \"pip\" or \"npm\" array",
        target.display()
    )
}

/// 诊断的收口：warning 打到 stderr，error 转成错误消息。
fn first_error(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .find(|item| item.level == Level::Error)
        .map(|item| item.message.clone())
        .unwrap_or_else(|| "cannot load the alias file".to_string())
}

fn print_warnings(diagnostics: &[Diagnostic]) {
    for item in diagnostics {
        if item.level == Level::Warning {
            eprintln!("Warning: {}", item.message);
        }
    }
}

// ── uninstall ───────────────────────────────────────────────────────────

fn uninstall(scope: FileScope, stem: &str, deps_dir: &Path, yes: bool) -> Result<Outcome, String> {
    if !deps_dir.is_dir() {
        return Err(format!(
            "No dependencies installed for @@{stem}: {}",
            deps_dir.display()
        ));
    }

    if !yes {
        prompt::require_tty(deps_dir, "uninstall")?;
        match prompt::confirm(
            deps_dir,
            stem,
            "This will permanently remove the dependency directory:",
            None,
        )? {
            Confirmation::Confirmed => {}
            Confirmation::Cancelled => return Ok(Outcome::Cancelled),
            Confirmation::Mismatch => return Ok(Outcome::Mismatch),
        }
    }

    std::fs::remove_dir_all(deps_dir)
        .map_err(|e| format!("Failed to remove {}: {}", deps_dir.display(), e))?;
    if !scope.is_global() {
        prune_local_root(deps_dir);
    }

    Ok(Outcome::Removed {
        dir: deps_dir.to_path_buf(),
    })
}

/// 局部依赖删完后收掉空壳：`.byk/deps` 里只剩我们的 `.gitignore` 时，
/// 连它和空的 `.byk` 一起删，别把空目录留给项目。
fn prune_local_root(deps_dir: &Path) {
    let Some(root) = deps_dir.parent() else {
        return;
    };
    let only_gitignore = fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .all(|entry| entry.file_name() == std::ffi::OsStr::new(".gitignore"))
        })
        .unwrap_or(false);
    if !only_gitignore {
        return;
    }

    let _ = fs::remove_file(root.join(".gitignore"));
    if fs::remove_dir(root).is_err() {
        return;
    }
    if let Some(byk) = root.parent() {
        let _ = fs::remove_dir(byk);
    }
}

// ── 联动：rename / destroy / link 对依赖目录的处理 ──────────────────────

/// `..rename` 成功后把依赖目录一起改名；目录不存在时静默跳过。
pub(super) fn rename_dir(scope: FileScope, from_stem: &str, to_stem: &str, layout: &PathLayout) {
    let from = deps_dir_of(scope, from_stem, layout);
    if !from.is_dir() {
        return;
    }
    let to = deps_dir_of(scope, to_stem, layout);
    if let Err(e) = std::fs::rename(&from, &to) {
        eprintln!(
            "Warning: cannot move the dependency directory {}: {}",
            from.display(),
            e
        );
        return;
    }
    eprintln!("Moved {} to {}", from.display(), to.display());
}

/// `..destroy` 成功后删除依赖目录；目录不存在时静默跳过。
pub(super) fn remove_dir(scope: FileScope, stem: &str, layout: &PathLayout) {
    let dir = deps_dir_of(scope, stem, layout);
    if !dir.is_dir() {
        return;
    }
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        eprintln!(
            "Warning: cannot remove the dependency directory {}: {}",
            dir.display(),
            e
        );
        return;
    }
    if !scope.is_global() {
        prune_local_root(&dir);
    }
    eprintln!("Removed {}", dir.display());
}

/// `..link` 改指向时提醒旧的依赖目录还在。
pub(super) fn warn_stale_dir(scope: FileScope, stem: &str, layout: &PathLayout) {
    let dir = deps_dir_of(scope, stem, layout);
    if dir.is_dir() {
        eprintln!(
            "Warning: this file still has a dependency directory at {};\nHint: run `{}..deps uninstall` first if the new target needs different assets",
            dir.display(),
            build_file_key(stem, scope.is_global())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn action(values: &[&str]) -> Action {
        parse_args(&args(values)).unwrap()
    }

    #[test]
    fn parse_args_accepts_full_and_short_verbs() {
        assert!(matches!(
            action(&["install"]),
            Action::Install { force: false }
        ));
        assert!(matches!(action(&["i"]), Action::Install { force: false }));
        assert!(matches!(action(&["in"]), Action::Install { force: false }));
        assert!(matches!(action(&["u"]), Action::Uninstall { yes: false }));
        assert!(matches!(action(&["uni"]), Action::Uninstall { yes: false }));
        assert!(matches!(
            action(&["uninstall", "--yes"]),
            Action::Uninstall { yes: true }
        ));
        assert!(matches!(action(&["s"]), Action::Status));
        assert!(matches!(
            action(&["i", "--force"]),
            Action::Install { force: true }
        ));
    }

    #[test]
    fn bare_deps_means_status() {
        assert!(matches!(action(&[]), Action::Status));
        assert!(matches!(action(&["status"]), Action::Status));
    }

    #[test]
    fn parse_args_rejects_unknown_extra_and_status_options() {
        assert!(
            parse_args(&args(&["upgrade"]))
                .unwrap_err()
                .contains("Unknown deps subcommand")
        );

        // 安装不再确认，`--yes` 就是非法选项
        assert_eq!(
            parse_args(&args(&["install", "--yes"])).unwrap_err(),
            "Unknown option: --yes\nHint: deps install runs without confirmation; the only option is --force"
        );

        assert_eq!(
            parse_args(&args(&["uninstall", "--yes", "x"])).unwrap_err(),
            "deps uninstall accepts only --yes"
        );

        assert_eq!(
            parse_args(&args(&["status", "--yes"])).unwrap_err(),
            "deps status takes no options"
        );
    }
}
