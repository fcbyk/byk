//! `..deps install`：同步资产、pip venv 与 node_modules。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::super::FileScope;
use super::super::count_files;
use super::{deps_command, deps_dir_of, label_of};
use crate::core::config::build_file_key;
use crate::core::deps::{self, Declaration, PlannedAsset, plan_entries};
use crate::core::paths::PathLayout;
use crate::core::sync::{self, Change};

// ── install ─────────────────────────────────────────────────────────────

/// 安装声明里的依赖：`$deps.files` 同步文件，`$deps.pip` 建 venv 并装包，`$deps.npm` 装进 node_modules。
///
/// 不做二次确认，过程逐条打在 stderr；清单让失败后的下一次安装能接着做。
pub(crate) fn install(
    scope: FileScope,
    stem: &str,
    layout: &PathLayout,
    decl: &Declaration,
    force: bool,
) -> Result<(), String> {
    let deps_dir = deps_dir_of(scope, stem, layout);
    let key = build_file_key(stem, scope.is_global());

    // 局部依赖落在项目里，先放一个自忽略的 .gitignore，别让二进制进仓库
    if !scope.is_global() {
        ensure_local_gitignore(&deps_dir)?;
    }

    let assets = plan_entries(decl, &deps_dir, &key)?;
    let has_files = !assets.is_empty();
    let has_pip = !decl.pip.is_empty();
    let has_npm = !decl.npm.is_empty();
    if !has_files && !has_pip && !has_npm {
        return Err(
            "nothing to install: $deps.files, $deps.pip and $deps.npm are all empty\nHint: run `..deps uninstall` to remove the whole directory"
                .to_string(),
        );
    }

    if has_files {
        sync_files(scope, stem, &deps_dir, &key, decl, &assets, force)?;
    }

    if has_pip {
        deps::install_pip(&deps_dir, &decl.pip)?;
    }

    if has_npm {
        deps::install_npm(&deps_dir, &decl.npm)?;
    }

    Ok(())
}

/// 把专属目录同步到 `$deps.files`：装缺的、换变了的、删已移除的，没变的跳过。
fn sync_files(
    scope: FileScope,
    stem: &str,
    deps_dir: &Path,
    key: &str,
    decl: &Declaration,
    assets: &[PlannedAsset],
    force: bool,
) -> Result<(), String> {
    let inspection = sync::inspect(&decl.files, assets, deps_dir, force)?;
    if let Some(warning) = &inspection.warning {
        eprintln!("Warning: {warning}");
    }
    if inspection.diff.is_noop() {
        eprintln!(
            "{}: {} declared, {} present — up to date",
            key,
            assets.len(),
            assets.len()
        );
        // 没动作也要提一句目录里的野文件，否则它们永远没机会被发现
        warn_unknown(&inspection.diff.unknown, deps_dir);
        return Ok(());
    }

    print_header(assets, &inspection, deps_dir, force);

    let mut next: BTreeMap<String, sync::ManifestEntry> = inspection
        .installed
        .iter()
        .map(|entry| (entry.dest.clone(), entry.clone()))
        .collect();

    let mut written: Vec<PathBuf> = Vec::new();
    let mut installed_now = 0usize;
    let mut local_now = 0usize;
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
                installed_now += 1;
                if asset.origin.is_local() {
                    local_now += 1;
                }
                // 根上解压的落点就是专属目录，它一直都在，只能走替换路径
                let replace = !matches!(change, Change::Add) || asset.dest.as_path() == deps_dir;
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
                let _ = sync::save(deps_dir, &ordered(&next), byk_version());
                return Err(failure(message, &written, scope, stem));
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
        eprintln!("  remove  {}", label_of(key, &entry.dest));
        match deps::remove_installed(&path) {
            Ok(()) => {
                prune_empty_parents(deps_dir, &path);
                next.remove(&entry.dest);
                removed_count += 1;
            }
            Err(message) => {
                let _ = sync::save(deps_dir, &ordered(&next), byk_version());
                return Err(failure(message, &written, scope, stem));
            }
        }
    }

    next.retain(|dest, _| inspection.desired.iter().any(|want| &want.dest == dest));
    sync::save(deps_dir, &ordered(&next), byk_version())?;

    if installed_now > 0 {
        // 本地条目不下载，但汇总行只说取回了几个文件——来源在括号里交代
        let local = if local_now > 0 {
            format!(" ({local_now} local)")
        } else {
            String::new()
        };
        eprintln!(
            "Downloaded {} into {}{local}",
            count_files(installed_now),
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
    warn_unknown(&inspection.diff.unknown, deps_dir);

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

/// 一行动作的右半边：`<label> <- <address>`，解压条目补 `(extract)`。
fn describe(asset: &PlannedAsset) -> String {
    let how = if asset.archive { " (extract)" } else { "" };
    format!("{} <- {}{}", asset.label, asset.origin.as_str(), how)
}

fn ordered(map: &BTreeMap<String, sync::ManifestEntry>) -> Vec<sync::ManifestEntry> {
    map.values().cloned().collect()
}

fn byk_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 局部依赖目录自带一个 `.gitignore`（内容 `*`），仓库里就不会冒出下载产物。
/// 已存在就不动，用户可以自己改。
fn ensure_local_gitignore(deps_dir: &Path) -> Result<(), String> {
    let Some(root) = deps_dir.parent() else {
        return Ok(());
    };
    let path = root.join(".gitignore");
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
    fs::write(&path, "*\n").map_err(|e| format!("cannot write {}: {e}", path.display()))
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

fn failure(message: String, written: &[PathBuf], scope: FileScope, stem: &str) -> String {
    let mut out = message;
    if !written.is_empty() {
        out.push_str("\n  already written in this run:");
        for path in written {
            out.push_str(&format!("\n    {}", path.display()));
        }
    }
    out.push_str(&format!(
        "\nHint: fix the entry and rerun `{}`; add --force to rebuild from scratch",
        deps_command(scope, stem)
    ));
    out
}
