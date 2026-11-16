//! `..deps`：安装、卸载或查看全局别名文件声明的依赖。
//!
//! 语法为 `@@<file>..deps <verb>`，verb 取 `install` / `uninstall` / `status`
//! 之一，也认无歧义前缀（`i` / `u` / `s`）。不带 verb 等于 `status`。
//! 只对全局别名文件有效——依赖目录 `~/.byk/deps/@@<stem>/` 以文件 key 命名，
//! 局部文件的 key 跨项目不唯一，没有稳定的落点。局部别名想用依赖时，先用
//! `@@<file>..init --link` 拿到全局身份。
//!
//! install 读链接文件并跟随指针，`$deps` 来自目标文档，落点却跟着链接
//! 文件的 key 走：声明跟着文件走，资产跟着身份走。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::prompt::{self, Confirmation};
use super::{FileScope, count_files, resolve_target};
use crate::core::aliases::{follow_link, load_document};
use crate::core::config::{Diagnostic, Level, build_builtin_table, build_file_key};
use crate::core::deps::{self, DepEntry, PlannedAsset, parse_deps, plan_entries};
use crate::core::paths::PathLayout;
use crate::core::sync::{self, Change};

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
    if scope != FileScope::Global {
        return Err(
            "deps are only supported on global alias files\nHint: use @@<file>..deps, or create a global link with @@<file>..init --link"
                .to_string(),
        );
    }

    match parse_args(args)? {
        Action::Status => status(stem, layout),
        Action::Install { force } => {
            let (target, declared) = declaration(stem, layout)?;
            let entries = declared.ok_or_else(|| no_declaration(&target))?;
            install(stem, layout, &entries, force)?;
            Ok(Outcome::Installed)
        }
        Action::Uninstall { yes } => {
            uninstall(stem, &deps::deps_dir_for_stem(stem, &layout.root_dir), yes)
        }
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

// ── install ─────────────────────────────────────────────────────────────

/// 把目录同步到声明：装缺的、换变了的、删已移除的，没变的跳过。
///
/// 不做二次确认，过程逐条打在 stderr；清单让失败后的下一次安装能接着做。
pub(super) fn install(
    stem: &str,
    layout: &PathLayout,
    entries: &[DepEntry],
    force: bool,
) -> Result<(), String> {
    let deps_dir = deps::deps_dir_for_stem(stem, &layout.root_dir);
    let key = build_file_key(stem, true);

    let assets = plan_entries(entries, &deps_dir, &key)?;
    if assets.is_empty() {
        return Err(
            "nothing to install: $deps.files is empty\nHint: run `..deps uninstall` to remove the whole directory"
                .to_string(),
        );
    }

    let inspection = sync::inspect(entries, &assets, &deps_dir, force)?;
    if let Some(warning) = &inspection.warning {
        eprintln!("Warning: {warning}");
    }
    if inspection.diff.is_noop() {
        eprintln!(
            "{}: {} declared, {} installed — up to date",
            key,
            assets.len(),
            assets.len()
        );
        // 没动作也要提一句目录里的野文件，否则它们永远没机会被发现
        warn_unknown(&inspection.diff.unknown, &deps_dir);
        return Ok(());
    }

    print_header(&assets, &inspection, &deps_dir, force);

    let mut next: BTreeMap<String, sync::ManifestEntry> = inspection
        .installed
        .iter()
        .map(|entry| (entry.dest.clone(), entry.clone()))
        .collect();

    let mut written: Vec<PathBuf> = Vec::new();
    let mut downloaded = 0usize;
    let mut removed_count = 0usize;

    for (index, change) in &inspection.diff.steps {
        let asset = &assets[*index];
        let record = &inspection.desired[*index];

        let result = match change {
            Change::Keep => {
                eprintln!("  keep    {}", asset.label);
                Ok(())
            }
            Change::Adopt => {
                eprintln!("  adopt   {}", asset.label);
                Ok(())
            }
            Change::Rechmod => {
                eprintln!("  chmod   {}", asset.label);
                deps::set_exec(asset)
            }
            Change::Add | Change::Update | Change::Repair => {
                eprintln!("  {:<7} {}", verb_of(*change), describe(asset));
                downloaded += 1;
                // 根上解压的落点就是专属目录，它一直都在，只能走替换路径
                let replace = !matches!(change, Change::Add) || asset.dest == deps_dir;
                if asset.archive {
                    deps::install_archive(asset, replace)
                } else {
                    deps::install_file(asset, replace)
                }
            }
        };

        match result {
            Ok(()) => {
                next.insert(record.dest.clone(), record.clone());
                if !matches!(change, Change::Keep | Change::Adopt | Change::Rechmod) {
                    written.push(asset.dest.clone());
                }
            }
            Err(message) => {
                let _ = sync::save(&deps_dir, &ordered(&next), byk_version());
                return Err(failure(message, &written, stem));
            }
        }
    }

    for entry in &inspection.diff.removed {
        if entry.dest.is_empty() {
            eprintln!(
                "Warning: cannot remove the dependency root declared by {}; clear it with `..deps uninstall`",
                entry.src
            );
            continue;
        }

        let path = deps_dir.join(&entry.dest);
        eprintln!("  remove  {}", label_of(&key, &entry.dest));
        match deps::remove_installed(&path) {
            Ok(()) => {
                prune_empty_parents(&deps_dir, &path);
                next.remove(&entry.dest);
                removed_count += 1;
            }
            Err(message) => {
                let _ = sync::save(&deps_dir, &ordered(&next), byk_version());
                return Err(failure(message, &written, stem));
            }
        }
    }

    next.retain(|dest, _| inspection.desired.iter().any(|want| &want.dest == dest));
    sync::save(&deps_dir, &ordered(&next), byk_version())?;

    if downloaded > 0 {
        eprintln!(
            "Downloaded {} into {}",
            count_files(downloaded),
            deps_dir.display()
        );
    }
    if removed_count > 0 {
        eprintln!(
            "Removed {} from {}",
            count_files(removed_count),
            deps_dir.display()
        );
    }
    warn_unknown(&inspection.diff.unknown, &deps_dir);

    Ok(())
}

fn print_header(
    assets: &[PlannedAsset],
    inspection: &sync::Inspection,
    deps_dir: &Path,
    force: bool,
) {
    let fresh = inspection.installed.is_empty() && inspection.existing == 0;
    if force {
        eprintln!(
            "Rebuilding {} in {} (--force):",
            count_files(assets.len()),
            deps_dir.display()
        );
    } else if fresh {
        eprintln!(
            "Downloading {} into {}:",
            count_files(assets.len()),
            deps_dir.display()
        );
    } else {
        eprintln!(
            "Syncing {} in {}:",
            count_entries(assets.len()),
            deps_dir.display()
        );
    }
}

fn count_entries(count: usize) -> String {
    if count == 1 {
        "1 declared entry".to_string()
    } else {
        format!("{count} declared entries")
    }
}

fn verb_of(change: Change) -> &'static str {
    match change {
        Change::Add => "add",
        Change::Update => "update",
        Change::Repair => "repair",
        _ => "sync",
    }
}

/// 一行动作的右半边：`<label> <- <url>`，解压条目补 `(extract)`。
fn describe(asset: &PlannedAsset) -> String {
    let how = if asset.archive { " (extract)" } else { "" };
    format!("{} <- {}{}", asset.label, asset.url, how)
}

fn label_of(key: &str, dest: &str) -> String {
    if dest.is_empty() {
        key.to_string()
    } else {
        format!("{key}/{dest}")
    }
}

fn ordered(map: &BTreeMap<String, sync::ManifestEntry>) -> Vec<sync::ManifestEntry> {
    map.values().cloned().collect()
}

fn byk_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 删完一条资产后收掉空掉的父目录，别在专属目录里留一堆空壳。
///
/// 只 `remove_dir`（删不掉非空目录），所以不会误伤还装着东西的目录。
fn prune_empty_parents(deps_dir: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == deps_dir || !dir.starts_with(deps_dir) {
            break;
        }
        if fs::remove_dir(dir).is_err() {
            break;
        }
        current = dir.parent();
    }
}

fn warn_unknown(unknown: &[String], deps_dir: &Path) {
    if unknown.is_empty() {
        return;
    }
    let count = unknown.len();
    eprintln!(
        "Warning: {} in {} {} not declared and {} left untouched:",
        count_files(count),
        deps_dir.display(),
        if count == 1 { "is" } else { "are" },
        if count == 1 { "was" } else { "were" },
    );
    for path in unknown {
        eprintln!("  {}", deps_dir.join(path).display());
    }
}

fn failure(message: String, written: &[PathBuf], stem: &str) -> String {
    let mut out = message;
    if !written.is_empty() {
        out.push_str("\n  already written in this run:");
        for path in written {
            out.push_str(&format!("\n    {}", path.display()));
        }
    }
    out.push_str(&format!(
        "\nHint: fix the entry and rerun `byk @@{stem}..deps i`; add --force to rebuild from scratch"
    ));
    out
}

// ── status ──────────────────────────────────────────────────────────────

fn status(stem: &str, layout: &PathLayout) -> Result<Outcome, String> {
    let (target, declared) = declaration(stem, layout)?;
    let Some(entries) = declared else {
        return Err(no_declaration(&target));
    };

    let deps_dir = deps::deps_dir_for_stem(stem, &layout.root_dir);
    let key = build_file_key(stem, true);
    let assets = plan_entries(&entries, &deps_dir, &key)?;
    let inspection = sync::inspect(&entries, &assets, &deps_dir, false)?;
    if let Some(warning) = &inspection.warning {
        eprintln!("Warning: {warning}");
    }

    let mut rows: Vec<(&'static str, String)> = Vec::new();
    for (index, change) in &inspection.diff.steps {
        let state = match change {
            Change::Keep | Change::Adopt => "ok",
            Change::Add | Change::Repair => "missing",
            Change::Update | Change::Rechmod => "outdated",
        };
        rows.push((state, assets[*index].label.clone()));
    }
    for entry in &inspection.diff.removed {
        rows.push(("stale", label_of(&key, &entry.dest)));
    }

    let installed = rows.iter().filter(|(state, _)| *state == "ok").count();
    let stale = inspection.diff.removed.len();
    let todo = rows
        .iter()
        .filter(|(state, _)| matches!(*state, "missing" | "outdated"))
        .count();

    let mut output = format!(
        "{}: {} declared, {} installed",
        key,
        assets.len(),
        installed
    );
    if stale > 0 {
        output.push_str(&format!(", {stale} stale"));
    }
    output.push('\n');
    for (state, label) in &rows {
        output.push_str(&format!("  {:<9}{label}\n", state));
    }
    output.push_str(&format!("Deps directory: {}\n", deps_dir.display()));

    if todo > 0 {
        output.push_str(&format!(
            "Hint: run `byk @@{}..deps i` to install the missing assets\n",
            stem
        ));
    } else if stale > 0 {
        output.push_str(&format!(
            "Hint: run `byk @@{}..deps i` to remove the stale assets\n",
            stem
        ));
    }

    Ok(Outcome::Status(output))
}

/// 读 `$deps`；`None` 表示文件没有这个字段。链接会跟随到目标。
pub(super) fn declaration(
    stem: &str,
    layout: &PathLayout,
) -> Result<(PathBuf, Option<Vec<DepEntry>>), String> {
    let target = resolve_target(FileScope::Global, stem, layout)?;
    if !target.is_file() {
        return Err(format!("File not found: {}", target.display()));
    }

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let vars = build_builtin_table(
        &layout.root_dir,
        &std::env::current_dir().unwrap_or_default(),
    );
    let deps_dir = deps::deps_dir_for_stem(stem, &layout.root_dir);
    let deps_dir = deps_dir.to_string_lossy().into_owned();

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
    parse_deps(value).map(|entries| (target, Some(entries)))
}

fn no_declaration(target: &Path) -> String {
    format!(
        "This file declares no \"$deps\": {}\nHint: add a \"$deps\" object with a \"files\" array",
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

fn uninstall(stem: &str, deps_dir: &Path, yes: bool) -> Result<Outcome, String> {
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

    Ok(Outcome::Removed {
        dir: deps_dir.to_path_buf(),
    })
}

// ── 联动：rename / destroy / link 对依赖目录的处理 ──────────────────────

/// `..rename` 成功后把依赖目录一起改名；目录不存在时静默跳过。
pub(super) fn rename_dir(from_stem: &str, to_stem: &str, layout: &PathLayout) {
    let from = deps::deps_dir_for_stem(from_stem, &layout.root_dir);
    if !from.is_dir() {
        return;
    }
    let to = deps::deps_dir_for_stem(to_stem, &layout.root_dir);
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
pub(super) fn remove_dir(stem: &str, layout: &PathLayout) {
    let dir = deps::deps_dir_for_stem(stem, &layout.root_dir);
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
    eprintln!("Removed {}", dir.display());
}

/// `..link` 改指向时提醒旧的依赖目录还在。
pub(super) fn warn_stale_dir(stem: &str, layout: &PathLayout) {
    let dir = deps::deps_dir_for_stem(stem, &layout.root_dir);
    if dir.is_dir() {
        eprintln!(
            "Warning: this file still has a dependency directory at {};\nHint: run @@{}..deps uninstall first if the new target needs different assets",
            dir.display(),
            stem
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
