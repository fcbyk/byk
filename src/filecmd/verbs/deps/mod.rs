//! `deps`：安装、卸载或查看别名文件声明的依赖。
//!
//! 语法为 `<scope><stem> deps <verb>`，verb 取 `install` / `uninstall` /
//! `status` 之一，也认无歧义前缀（`i` / `u` / `s`）。不带 verb 等于 `status`。
//! 全局文件的依赖目录是 `~/.byk/deps/@@<stem>/`，局部文件是
//! `<文件所在目录>/.byk/deps/@<stem>/`：目录本身就是身份，跟着项目走。
//!
//! install 读链接文件并跟随指针，`$deps` 来自目标文档，落点却跟着链接
//! 文件的 key 走：声明跟着文件走，资产跟着身份走。

mod install;
mod parse;
mod status;

use std::path::{Path, PathBuf};

use crate::deps::Declaration;
use crate::deps::apply;
use crate::deps::deps_dir;
use crate::filecmd::io::path::resolve_target;
use crate::filecmd::prompt;
use crate::filecmd::prompt::Confirmation;
use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;
use crate::model::build_file_key;
use crate::resolve::{Diagnostic, Level, build_builtin_table, load_declaration};

use self::parse::{Action, parse_args};

pub(crate) use install::install;
use status::status;

#[derive(Debug)]
pub(crate) enum Outcome {
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

pub(crate) fn run(
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

/// 被寻址文件所在的目录：全局固定是别名目录，局部就是当前目录。
fn file_dir_of(scope: FileScope, layout: &PathLayout) -> PathBuf {
    match scope {
        FileScope::Global => layout.alias_dir.clone(),
        FileScope::Local => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// 提示里那条可复制的命令：`@@hello deps install` / `@hello deps install`。
fn deps_command(scope: FileScope, stem: &str) -> String {
    format!(
        "byk {} deps install",
        build_file_key(stem, scope.is_global())
    )
}

fn deps_dir_of(scope: FileScope, stem: &str, layout: &PathLayout) -> PathBuf {
    let key = build_file_key(stem, scope.is_global());
    deps_dir(
        &key,
        scope.is_global(),
        &file_dir_of(scope, layout),
        &layout.root_dir,
    )
}

/// 读 `$deps`；`None` 表示文件没有这个字段。链接会跟随到目标。
pub(crate) fn declaration(
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

    let declared = load_declaration(&target, &vars, Some(&deps_dir), &mut diagnostics);
    print_warnings(&diagnostics);
    Ok((target, declared?))
}

fn no_declaration(target: &Path) -> String {
    format!(
        "This file declares no \"$deps\": {}\nHint: add a \"$deps\" object with a \"files\", \"pip\" or \"npm\" array",
        target.display()
    )
}

/// 诊断的收口：warning 打到 stderr。
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

    apply::remove_all(deps_dir, scope.is_global())?;

    Ok(Outcome::Removed {
        dir: deps_dir.to_path_buf(),
    })
}

// ── 联动：rename / destroy / link 对依赖目录的处理 ──────────────────────

/// `rename` 成功后把依赖目录一起改名；目录不存在时静默跳过。
pub(crate) fn rename_dir(scope: FileScope, from_stem: &str, to_stem: &str, layout: &PathLayout) {
    let from = deps_dir_of(scope, from_stem, layout);
    let to = deps_dir_of(scope, to_stem, layout);
    apply::rename_dir(&from, &to);
}

/// `destroy` 成功后删除依赖目录；目录不存在时静默跳过。
pub(crate) fn remove_dir(scope: FileScope, stem: &str, layout: &PathLayout) {
    let dir = deps_dir_of(scope, stem, layout);
    apply::remove_dir(&dir, scope.is_global());
}

/// `link` 改指向时提醒旧的依赖目录还在。
pub(crate) fn warn_stale_dir(scope: FileScope, stem: &str, layout: &PathLayout) {
    let dir = deps_dir_of(scope, stem, layout);
    let key = build_file_key(stem, scope.is_global());
    apply::warn_stale_dir(&dir, &key);
}
