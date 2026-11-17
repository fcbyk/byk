//! 依赖资产的下载、复制、解压和原子落盘。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::deps::spec::plan_entries;
use crate::deps::types::{Declaration, Origin, PlannedAsset};

use super::count_files;
use super::packages::{install_npm, install_pip};
use super::sync::{self, Change};

// ── 执行层：取回（curl / 复制）、tar 预检与解压 ────────────────────────

/// 落一条普通资产。
///
/// `replace` 为真时覆盖已存在的目标：先取到同级临时文件，再 rename 过去，
/// 覆盖是原子的，中途失败不会留下半个文件。
pub(crate) fn install_file(asset: &PlannedAsset, replace: bool) -> Result<(), String> {
    let index = asset.index;

    if !replace && asset.dest.symlink_metadata().is_ok() {
        return Err(exists_message(index, &asset.label, &asset.dest));
    }

    let parent = asset
        .dest
        .parent()
        .ok_or_else(|| format!("files[{index}]: \"{}\" has no parent", asset.dest.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("files[{index}]: cannot create {}: {e}", parent.display()))?;

    let scratch = scratch_path(&asset.dest);
    if let Err(message) = fetch(index, &asset.origin, &scratch) {
        let _ = fs::remove_file(&scratch);
        return Err(message);
    }

    if let Err(e) = commit(&scratch, &asset.dest) {
        let _ = fs::remove_file(&scratch);
        return Err(format!(
            "files[{index}]: cannot write {}: {e}",
            asset.dest.display()
        ));
    }

    if asset.exec {
        grant_exec(index, &asset.dest)?;
    }
    Ok(())
}

/// 解压一条资产。整目录替换：先解到同级的临时目录，再两跳 rename 换上去。
///
/// 不能"解包覆盖"：新版本归档里删掉的文件，增量解包是删不掉的，会留下幽灵文件。
pub(crate) fn install_archive(asset: &PlannedAsset, replace: bool) -> Result<(), String> {
    let index = asset.index;
    let parent = asset
        .dest
        .parent()
        .ok_or_else(|| format!("files[{index}]: \"{}\" has no parent", asset.dest.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("files[{index}]: cannot create {}: {e}", parent.display()))?;

    if !replace && asset.dest.symlink_metadata().is_ok() {
        return Err(exists_message(index, &asset.label, &asset.dest));
    }

    let scratch = scratch_path(&asset.dest);
    if let Err(message) = fetch(index, &asset.origin, &scratch) {
        let _ = fs::remove_file(&scratch);
        return Err(message);
    }

    let staging = sibling(&asset.dest, "new");
    let _ = fs::remove_dir_all(&staging);
    let result = extract_archive(index, &asset.label, &scratch, &staging);
    let _ = fs::remove_file(&scratch);
    if let Err(message) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(message);
    }

    let outcome = swap_dir(&staging, &asset.dest);
    let _ = fs::remove_dir_all(&staging);
    outcome.map_err(|e| format!("files[{index}]: cannot write {}: {e}", asset.dest.display()))
}

/// 两跳换目录：旧的先让位到 trash，新的再上位，成功后清掉 trash。
///
/// 第二跳失败时把旧的搬回去——宁可回到装之前，也不留一个空落点。
fn swap_dir(staging: &Path, dest: &Path) -> std::io::Result<()> {
    if dest.symlink_metadata().is_err() {
        return fs::rename(staging, dest);
    }

    let trash = sibling(dest, "old");
    let _ = fs::remove_dir_all(&trash);
    fs::rename(dest, &trash)?;

    match fs::rename(staging, dest) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&trash);
            Ok(())
        }
        Err(e) => {
            let _ = fs::rename(&trash, dest);
            Err(e)
        }
    }
}

/// 删掉一条已安装的资产：文件删文件，目录删整片。
pub(crate) fn remove_installed(path: &Path) -> Result<(), String> {
    let meta = path
        .symlink_metadata()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| format!("cannot remove {}: {e}", path.display()))
}

/// 只调整可执行位，不动内容：`exec` 为真补 755，为假退回 644。
pub(crate) fn set_exec(asset: &PlannedAsset) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if asset.exec { 0o755 } else { 0o644 };
        fs::set_permissions(&asset.dest, fs::Permissions::from_mode(mode)).map_err(|e| {
            format!(
                "files[{}]: cannot set the permissions of {}: {e}",
                asset.index,
                asset.dest.display()
            )
        })
    }
    #[cfg(not(unix))]
    {
        Ok(())
    }
}

fn sibling(path: &Path, tag: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".to_string());
    path.with_file_name(format!(".byk-{tag}-{}-{name}", std::process::id()))
}

/// 覆盖式落位：POSIX 的 rename 可以直接盖掉目标；Windows 上得先删。
fn commit(scratch: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if dest.symlink_metadata().is_ok() {
        let meta = dest.symlink_metadata()?;
        if meta.is_dir() {
            fs::remove_dir_all(dest)?;
        } else {
            fs::remove_file(dest)?;
        }
    }

    fs::rename(scratch, dest)
}

fn extract_archive(index: usize, label: &str, archive: &Path, dest: &Path) -> Result<(), String> {
    let entries = list_archive(index, archive)?;

    // 越界与覆盖预检：宁可让用户手动清理，也不解到一半报错。
    for entry in &entries {
        let Some(relative) = archive_entry_path(index, entry)? else {
            continue;
        };
        let target = dest.join(relative.join("/"));
        if target.symlink_metadata().is_ok() {
            return Err(format!(
                "files[{index}]: target \"{label}/{}\" already exists at {}",
                relative.join("/"),
                target.display()
            ));
        }
    }

    fs::create_dir_all(dest)
        .map_err(|e| format!("files[{index}]: cannot create {}: {e}", dest.display()))?;

    let output = Command::new("tar")
        .args(["-xf"])
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .output()
        .map_err(|e| format!("files[{index}]: cannot run \"tar\": {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "files[{index}]: cannot extract the archive: \"tar -xf\" {}",
            first_stderr(&output.stderr)
        ));
    }
    Ok(())
}

fn list_archive(index: usize, archive: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("tar")
        .args(["-tf"])
        .arg(archive)
        .output()
        .map_err(|e| format!("files[{index}]: cannot run \"tar\": {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "files[{index}]: cannot list the archive: \"tar -tf\" {}\nHint: only tar archives (tar, tar.gz, tar.bz2, tar.xz) are supported",
            first_stderr(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

/// 归档条目 → 相对段；目录条目返回 `Ok(None)`；越界（绝对路径、`..`、反斜杠、
/// 冒号）报错。
fn archive_entry_path(index: usize, entry: &str) -> Result<Option<Vec<String>>, String> {
    let escape =
        || format!("files[{index}]: archive entry \"{entry}\" points outside the target directory");

    if entry.starts_with('/') {
        return Err(escape());
    }
    // 目录条目：以 `/` 结尾，没有文件名可校验
    if entry.ends_with('/') {
        return Ok(None);
    }

    let mut parts = Vec::new();
    for segment in entry.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return Err(escape()),
            other => {
                if other.contains('\\') || other.contains(':') || other.contains('\0') {
                    return Err(escape());
                }
                parts.push(other.to_string());
            }
        }
    }
    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts))
    }
}

/// 把来源取到临时文件。两条路都只写 `out`，不碰 `dest`——rename 之前失败
/// 都不会留下半个文件。
fn fetch(index: usize, origin: &Origin, out: &Path) -> Result<(), String> {
    match origin {
        Origin::Url(url) => download(index, url, out),
        Origin::File(path) => copy_local(index, path, out),
    }
}

/// 本地来源：直接复制。`extract` 条目也走这条——先落到临时文件，`tar` 再
/// 从那里解，和下载路径共用同一套预检与换目录逻辑。
fn copy_local(index: usize, src: &Path, out: &Path) -> Result<(), String> {
    let metadata = fs::metadata(src)
        .map_err(|e| format!("files[{index}]: cannot read \"{}\": {e}", src.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "files[{index}]: \"{}\" is not a file",
            src.display()
        ));
    }

    fs::copy(src, out)
        .map(|_| ())
        .map_err(|e| format!("files[{index}]: cannot copy \"{}\": {e}", src.display()))
}

fn download(index: usize, url: &str, out: &Path) -> Result<(), String> {
    let output = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(out)
        .arg("--")
        .arg(url)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "files[{index}]: curl is required to download dependencies\nHint: install curl and retry"
                )
            } else {
                format!("files[{index}]: cannot run \"curl\": {e}")
            }
        })?;

    if !output.status.success() {
        return Err(format!(
            "files[{index}]: cannot download \"{url}\": curl {}",
            first_stderr(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn grant_exec(index: usize, dest: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dest, fs::Permissions::from_mode(0o755)).map_err(|e| {
        format!(
            "files[{index}]: cannot mark {} executable: {e}",
            dest.display()
        )
    })
}

#[cfg(not(unix))]
fn grant_exec(_index: usize, _dest: &Path) -> Result<(), String> {
    Ok(())
}

fn exists_message(index: usize, label: &str, dest: &Path) -> String {
    format!(
        "files[{index}]: target \"{label}\" already exists at {}",
        dest.display()
    )
}

pub(super) fn first_stderr(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("failed")
        .to_string()
}

fn scratch_path(dest: &Path) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".to_string());
    dest.with_file_name(format!(".byk-dl-{}-{}-{name}.tmp", std::process::id(), n))
}

// ── 安装编排：声明 → 资产同步 → pip/npm ────────────────────────────────

/// 安装声明里的依赖：`$deps.files` 同步文件，`$deps.pip` 建 venv 并装包，
/// `$deps.npm` 装进 node_modules。过程逐条打在 stderr。
pub(crate) fn install_declaration(
    deps_dir: &Path,
    key: &str,
    is_global: bool,
    decl: &Declaration,
    force: bool,
    retry_hint: &str,
) -> Result<(), String> {
    // 局部依赖落在项目里，先放一个自忽略的 .gitignore，别让二进制进仓库
    if !is_global {
        ensure_local_gitignore(deps_dir)?;
    }

    let assets = plan_entries(decl, deps_dir, key)?;
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
        sync_files(deps_dir, key, decl, &assets, force, retry_hint)?;
    }

    if has_pip {
        install_pip(deps_dir, &decl.pip)?;
    }

    if has_npm {
        install_npm(deps_dir, &decl.npm)?;
    }

    Ok(())
}

/// 把专属目录同步到 `$deps.files`：装缺的、换变了的、删已移除的，没变的跳过。
fn sync_files(
    deps_dir: &Path,
    key: &str,
    decl: &Declaration,
    assets: &[PlannedAsset],
    force: bool,
    retry_hint: &str,
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
                set_exec(asset)
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
                    install_archive(asset, replace)
                } else {
                    install_file(asset, replace)
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
                return Err(failure(message, &written, retry_hint));
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
        match remove_installed(&path) {
            Ok(()) => {
                prune_empty_parents(deps_dir, &path);
                next.remove(&entry.dest);
                removed_count += 1;
            }
            Err(message) => {
                let _ = sync::save(deps_dir, &ordered(&next), byk_version());
                return Err(failure(message, &written, retry_hint));
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

fn failure(message: String, written: &[PathBuf], retry_hint: &str) -> String {
    let mut out = message;
    if !written.is_empty() {
        out.push_str("\n  already written in this run:");
        for path in written {
            out.push_str(&format!("\n    {}", path.display()));
        }
    }
    out.push_str(&format!(
        "\nHint: fix the entry and rerun `{retry_hint}`; add --force to rebuild from scratch"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_entries_reject_escape() {
        for entry in ["/etc/passwd", "../outside", "a/../../b", "a\\b", "a:b"] {
            assert!(archive_entry_path(0, entry).is_err(), "{entry}");
        }
    }

    #[test]
    fn archive_entries_keep_relative_paths() {
        assert_eq!(
            archive_entry_path(0, "bin/tool").unwrap().unwrap(),
            vec!["bin", "tool"]
        );
        assert_eq!(
            archive_entry_path(0, "./bin/./tool").unwrap().unwrap(),
            vec!["bin", "tool"]
        );
        assert!(archive_entry_path(0, "bin/").unwrap().is_none());
    }
}
