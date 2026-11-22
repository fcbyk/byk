//! `deps` 的已安装清单与同步判定。
//!
//! 清单 `.byk-deps.json` 记的是「上次装了什么」：声明条目的规范化快照加相对
//! 落点。它让安装从「追加」变成「收敛到声明」——能跳过没变的、替换变了的、
//! 删掉已移除的，同时只删自己装过的路径。
//!
//! 清单只是证据，磁盘才是真相：归类之后一律再用实际文件校正一次。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::deps::spec::{NODE_MODULES, VENV_DIR};
use crate::deps::types::{DepEntry, Origin, PlannedAsset};

/// 清单文件名；点号开头，和 `node_modules/.package-lock.json` 一个路数。
pub(crate) const MANIFEST_NAME: &str = ".byk-deps.json";

/// 清单格式版本；读到更大的版本按「不可用」处理。
const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) version: u32,
    /// 写入时的 byk 版本，纯排查用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) byk: Option<String>,
    pub(crate) entries: Vec<ManifestEntry>,
}

/// 一条已安装记录：声明条目的快照加相对落点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ManifestEntry {
    pub(crate) src: String,
    /// 相对专属目录的子目录，`/` 分隔，空串表示根。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) extract: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) exec: bool,
    /// 相对专属目录的落点；`extract` 条目指向目录。
    pub(crate) dest: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ManifestEntry {
    /// 来源与形态是否一致（`exec` 单独比，它不用重下）。
    fn same_source(&self, other: &Self) -> bool {
        self.src == other.src
            && self.to == other.to
            && self.name == other.name
            && self.extract == other.extract
    }
}

/// 从声明条目与计划的落点造一条记录。
///
/// `src` 记**解析后的绝对地址**（URL 或本机路径），不是 `$deps` 里写的原文：
/// 清单回答的是"上次从哪儿装的"。所以改了 `base` 就是换了来源，下一次必须
/// 判成 `update` 而不是 `keep`。
pub(crate) fn entry_for(entry: &DepEntry, asset: &PlannedAsset, dest: String) -> ManifestEntry {
    ManifestEntry {
        src: asset.origin.as_str(),
        to: entry.to.join("/"),
        name: entry.name.clone(),
        extract: entry.extract,
        exec: entry.exec,
        dest,
    }
}

// ── 读写 ────────────────────────────────────────────────────────────────

pub(crate) enum State {
    /// 没有清单：老目录、手建目录，或第一次安装。
    Missing,
    /// 有文件但读不懂：按「没有清单」处理，不因为清单坏了就删文件。
    Unusable(String),
    Present(Manifest),
}

pub(crate) fn load(deps_dir: &Path) -> State {
    let path = deps_dir.join(MANIFEST_NAME);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return State::Missing,
        Err(e) => return State::Unusable(format!("cannot read it: {e}")),
    };

    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(e) => return State::Unusable(format!("invalid JSON: {e}")),
    };

    if manifest.version > MANIFEST_VERSION {
        return State::Unusable(format!(
            "format version {} is newer than {}",
            manifest.version, MANIFEST_VERSION
        ));
    }

    State::Present(manifest)
}

/// 原子写入：同级临时文件 + rename，中途失败不会留下半个 JSON。
pub(crate) fn save(
    deps_dir: &Path,
    entries: &[ManifestEntry],
    byk_version: &str,
) -> Result<(), String> {
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        byk: Some(byk_version.to_string()),
        entries: entries.to_vec(),
    };
    let body = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("cannot serialize the manifest: {e}"))?;

    fs::create_dir_all(deps_dir)
        .map_err(|e| format!("cannot create {}: {e}", deps_dir.display()))?;

    let temp = deps_dir.join(format!(".{MANIFEST_NAME}.{}.tmp", std::process::id()));
    fs::write(&temp, format!("{body}\n"))
        .map_err(|e| format!("cannot write {}: {e}", temp.display()))?;

    fs::rename(&temp, deps_dir.join(MANIFEST_NAME)).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!(
            "cannot write {}: {e}",
            deps_dir.join(MANIFEST_NAME).display()
        )
    })
}

// ── 判定 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    /// 声明有、清单无、磁盘无。
    Add,
    /// 声明有、清单无、磁盘有：接管老文件，不下载。
    Adopt,
    /// 声明变了，重下覆盖。
    Update,
    /// 清单说装过，磁盘上没有：补装。
    Repair,
    /// 只有 `exec` 不一致：直接 `chmod`。
    Rechmod,
    /// 完全一致且磁盘上有。
    Keep,
}

#[derive(Debug)]
pub(crate) struct Diff {
    /// 与 desired 同序，包含 `Keep`。
    pub(crate) steps: Vec<(usize, Change)>,
    /// 清单里有、声明里没有、且磁盘上还在：要删。
    pub(crate) removed: Vec<ManifestEntry>,
    /// 目录里既不在声明也不在清单的相对路径：不删，只提示。
    pub(crate) unknown: Vec<String>,
}

impl Diff {
    pub(crate) fn is_noop(&self) -> bool {
        self.removed.is_empty() && self.steps.iter().all(|(_, change)| *change == Change::Keep)
    }
}

/// 一次安装所需的全部信息：调用方拿到就能打印与执行。
pub(crate) struct Inspection {
    pub(crate) desired: Vec<ManifestEntry>,
    /// 清单里原有的记录；执行失败时用来保留未完成条目的旧记录。
    pub(crate) installed: Vec<ManifestEntry>,
    pub(crate) diff: Diff,
    /// 目录里已有几个文件（判断是否首次安装）。
    pub(crate) existing: usize,
    /// 清单读不懂时的说明，调用方决定怎么提示。
    pub(crate) warning: Option<String>,
}

/// 读清单、算落点、比对声明、核对磁盘。纯计算，不打印也不改文件。
pub(crate) fn inspect(
    entries: &[DepEntry],
    assets: &[PlannedAsset],
    deps_dir: &Path,
    force: bool,
) -> Result<Inspection, String> {
    let (installed, warning) = match load(deps_dir) {
        State::Present(manifest) => (manifest.entries, None),
        State::Missing => (Vec::new(), None),
        State::Unusable(why) => (Vec::new(), Some(format!("ignoring {MANIFEST_NAME}: {why}"))),
    };

    let desired = entries
        .iter()
        .zip(assets)
        .map(|(entry, asset)| -> Result<ManifestEntry, String> {
            let dest = asset.dest.strip_prefix(deps_dir).map_err(|_| {
                format!(
                    "internal error: {} is outside {}",
                    asset.dest.display(),
                    deps_dir.display()
                )
            })?;
            Ok(entry_for(entry, asset, relative(dest)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let existing = existing_paths(deps_dir);
    let stale = stale_sources(&desired, assets, deps_dir);
    let diff = classify(&desired, &installed, &existing, force, &stale)?;

    Ok(Inspection {
        desired,
        installed,
        diff,
        existing: existing.len(),
        warning,
    })
}

/// 本地来源里，源文件比装好的那份新的条目（按 `dest` 列出）。
///
/// URL 可以当内容寻址：同一个地址就是同一份内容，`keep` 永远安全。本地文件
/// 不行——`../tool` 今天和明天是同一个路径、不同内容，改了源还报 up to date
/// 等于把"本地能测"这件事毁掉。所以本地来源拿 mtime 说话：源更新了就重装。
///
/// 源不在了也算"变了"：这样安装会走到读取并报出具体路径，而不是安静地跳过
/// ——静默跳过会让人以为装好了。
///
/// 本地不本地看 `Origin`，不从地址字面量猜。`desired` 与 `assets` 同序同长。
fn stale_sources(
    desired: &[ManifestEntry],
    assets: &[PlannedAsset],
    deps_dir: &Path,
) -> Vec<String> {
    desired
        .iter()
        .zip(assets)
        .filter(|(_, asset)| asset.origin.is_local())
        .filter(|(entry, asset)| source_is_newer(asset, &deps_dir.join(&entry.dest)))
        .map(|(entry, _)| entry.dest.clone())
        .collect()
}

/// 源比装好的那份新吗；源不见了也算。
fn source_is_newer(asset: &PlannedAsset, installed: &Path) -> bool {
    let Origin::File(source) = &asset.origin else {
        return false;
    };
    let Ok(source) = fs::metadata(source) else {
        return true;
    };
    let Ok(installed) = fs::metadata(installed) else {
        return false;
    };
    match (source.modified(), installed.modified()) {
        (Ok(source), Ok(installed)) => source > installed,
        _ => false,
    }
}

/// 按 dest 归类，再用磁盘实际内容校正。
///
/// 清单里的 `dest` 是用户可编辑的，删除前必须过 [`validate_dest`]；校验失败
/// 是整次拒绝，而不是跳过这一条——清单坏了说明状态不可信。
///
/// `stale` 是本地来源里源文件已经变新的 `dest` 集合，见 [`stale_sources`]。
pub(crate) fn classify(
    desired: &[ManifestEntry],
    installed: &[ManifestEntry],
    existing: &[String],
    force: bool,
    stale: &[String],
) -> Result<Diff, String> {
    for entry in desired.iter().chain(installed) {
        validate_dest(&entry.dest)
            .map_err(|why| format!("{MANIFEST_NAME}: entry {:?} has {why}", entry.src))?;
    }

    let mut known: BTreeMap<&str, &ManifestEntry> = BTreeMap::new();
    for entry in installed {
        known.insert(entry.dest.as_str(), entry);
    }

    let mut steps = Vec::with_capacity(desired.len());
    let mut desired_dests = Vec::with_capacity(desired.len());

    for (index, want) in desired.iter().enumerate() {
        desired_dests.push(want.dest.as_str());
        let recorded = known.get(want.dest.as_str());

        // 根上解压的落点就是专属目录本身，它永远"存在"，所以不能拿 dest 判定：
        //
        // - 清单记过 + 目录里有东西 → 算装好
        // - 清单记过 + 目录空到只剩清单 → 上次解压没成，补一次
        // - 没记过 + 目录里有东西 → 接管，不覆盖别人的文件
        // - 没记过 + 目录空 → 全新安装
        let (present, adoptable) = if want.dest.is_empty() {
            let has_content = !existing.is_empty();
            (
                recorded.is_some() && has_content,
                recorded.is_none() && has_content,
            )
        } else {
            (on_disk(&want.dest, existing), false)
        };

        let change = match recorded {
            _ if force && (present || adoptable) => Change::Update,
            _ if force => Change::Add,
            None if present || adoptable => Change::Adopt,
            None => Change::Add,
            Some(_) if !present => Change::Repair,
            Some(have) if !have.same_source(want) => Change::Update,
            Some(_) if stale.contains(&want.dest) => Change::Update,
            Some(have) if have.exec != want.exec => Change::Rechmod,
            Some(_) => Change::Keep,
        };
        steps.push((index, change));
    }

    let removed: Vec<ManifestEntry> = installed
        .iter()
        .filter(|have| {
            !desired_dests.contains(&have.dest.as_str()) && on_disk(&have.dest, existing)
        })
        .cloned()
        .collect();

    // 未知文件：两边都没有的路径。删它没有授权，只提示。
    let unknown = existing
        .iter()
        .filter(|path| {
            let owned_by_desired = desired.iter().any(|want| covers(&want.dest, path));
            let owned_by_manifest = installed.iter().any(|have| covers(&have.dest, path));
            !owned_by_desired && !owned_by_manifest
        })
        .cloned()
        .collect();

    Ok(Diff {
        steps,
        removed,
        unknown,
    })
}

/// 落点在磁盘上有没有对应的东西：文件看相等，目录看有没有内容。
fn on_disk(dest: &str, existing: &[String]) -> bool {
    if dest.is_empty() {
        return !existing.is_empty();
    }
    existing.iter().any(|path| covers(dest, path))
}

/// 某个落点是否覆盖这条路径：文件是相等，目录是前缀。
fn covers(dest: &str, path: &str) -> bool {
    if dest.is_empty() {
        return true;
    }
    path == dest
        || path
            .strip_prefix(dest)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// 清单里的相对落点必须是干净的相对路径。
pub(crate) fn validate_dest(dest: &str) -> Result<(), String> {
    if dest.is_empty() {
        return Ok(());
    }
    if dest.contains('\\') {
        return Err("a backslash in \"dest\"".to_string());
    }
    if dest.starts_with('/') || is_drive_path(dest) {
        return Err("an absolute \"dest\"".to_string());
    }
    for segment in dest.split('/') {
        match segment {
            "" | "." | ".." => return Err(format!("a \"{segment}\" segment in \"dest\"")),
            _ => {}
        }
    }
    Ok(())
}

fn is_drive_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

// ── 目录扫描 ────────────────────────────────────────────────────────────

/// 专属目录里所有文件的相对路径（`/` 分隔，已排序），跳过清单自己。
///
/// 只收文件：目录由它的内容代表，空的遗留目录不值得单独报。
pub(crate) fn existing_paths(deps_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(deps_dir, "", &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            // venv / node_modules 是 pip / npm 建的资产，属于 byk 自己管的，别报成未知文件
            if prefix.is_empty() && (name == VENV_DIR || name == NODE_MODULES) {
                continue;
            }
            walk(&entry.path(), &path, out);
        } else if path != MANIFEST_NAME
            && !(prefix.is_empty()
                && (name == ".gitignore" || name == "package.json" || name == "package-lock.json"))
        {
            // 清单、自家的 .gitignore 和 npm 的 package.json / lock 不算资产，别报成未知文件
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(src: &str, dest: &str) -> ManifestEntry {
        ManifestEntry {
            src: src.to_string(),
            to: String::new(),
            name: None,
            extract: false,
            exec: false,
            dest: dest.to_string(),
        }
    }

    fn kept(diff: &Diff) -> Vec<Change> {
        diff.steps.iter().map(|(_, change)| *change).collect()
    }

    fn slash(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn planned(origin: Origin, dest: &str) -> PlannedAsset {
        PlannedAsset {
            index: 0,
            label: format!("@@t/{dest}"),
            origin,
            dest: Path::new("/deps").join(dest),
            archive: false,
            exec: false,
        }
    }

    #[test]
    fn a_vanished_local_source_is_stale() {
        let temp = tempfile::tempdir().unwrap();
        let deps = temp.path().join("deps");
        fs::create_dir_all(&deps).unwrap();
        fs::write(deps.join("tool"), "old").unwrap();

        // 源没了就不能安静地报 up to date：下次安装要撞上读取失败
        let src = temp.path().join("gone");
        let want = entry(&slash(&src), "tool");
        assert_eq!(
            stale_sources(&[want], &[planned(Origin::File(src), "tool")], &deps),
            ["tool"]
        );
    }

    #[test]
    fn a_remote_entry_is_never_stale() {
        let temp = tempfile::tempdir().unwrap();
        let deps = temp.path().join("deps");
        fs::create_dir_all(&deps).unwrap();
        fs::write(deps.join("tool"), "old").unwrap();

        let remote = planned(Origin::Url("https://e.com/tool".to_string()), "tool");
        assert!(stale_sources(&[entry("https://e.com/tool", "tool")], &[remote], &deps).is_empty());

        // 刚复制过来的本地条目 mtime 不早于源，算干净
        let source = temp.path().join("fresh");
        fs::write(&source, "new").unwrap();
        fs::copy(&source, deps.join("fresh")).unwrap();
        assert!(
            stale_sources(
                &[entry(&slash(&source), "fresh")],
                &[planned(Origin::File(source.clone()), "fresh")],
                &deps
            )
            .is_empty()
        );

        // 源比装好的新：重装
        let newer =
            fs::metadata(&source).unwrap().modified().unwrap() + std::time::Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(newer)
            .unwrap();
        assert_eq!(
            stale_sources(
                &[entry(&slash(&source), "fresh")],
                &[planned(Origin::File(source), "fresh")],
                &deps
            ),
            ["fresh"]
        );
    }

    #[test]
    fn missing_file_is_added() {
        let want = vec![entry("https://e.com/a", "a")];
        let diff = classify(&want, &[], &[], false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Add]);
        assert!(diff.unknown.is_empty());
    }

    #[test]
    fn unrecorded_but_present_file_is_adopted() {
        let want = vec![entry("https://e.com/a", "a")];
        let existing = vec!["a".to_string()];
        let diff = classify(&want, &[], &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Adopt]);
    }

    #[test]
    fn same_declaration_keeps() {
        let want = vec![entry("https://e.com/a", "a")];
        let existing = vec!["a".to_string()];
        let diff = classify(&want, &want.clone(), &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Keep]);
        assert!(diff.is_noop());
    }

    #[test]
    fn changed_src_is_updated() {
        let want = vec![entry("https://e.com/v2", "a")];
        let have = vec![entry("https://e.com/v1", "a")];
        let existing = vec!["a".to_string()];
        let diff = classify(&want, &have, &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Update]);
    }

    #[test]
    fn missing_from_disk_is_repaired() {
        let want = vec![entry("https://e.com/a", "a")];
        let diff = classify(&want, &want.clone(), &[], false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Repair]);
    }

    #[test]
    fn exec_only_difference_rechmods() {
        let want = vec![ManifestEntry {
            exec: true,
            ..entry("https://e.com/a", "a")
        }];
        let have = vec![entry("https://e.com/a", "a")];
        let existing = vec!["a".to_string()];
        let diff = classify(&want, &have, &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Rechmod]);
    }

    #[test]
    fn undeclared_but_present_is_removed() {
        let want = vec![entry("https://e.com/new", "new")];
        let have = vec![entry("https://e.com/old", "old")];
        let existing = vec!["new".to_string(), "old".to_string()];
        let diff = classify(&want, &have, &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Adopt]);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].dest, "old");
    }

    #[test]
    fn undeclared_and_absent_is_silently_forgotten() {
        let want = vec![entry("https://e.com/new", "new")];
        let have = vec![entry("https://e.com/old", "old")];
        let existing = vec!["new".to_string()];
        let diff = classify(&want, &have, &existing, false, &[]).unwrap();

        assert!(diff.removed.is_empty());
    }

    #[test]
    fn directory_dest_owns_its_contents() {
        let mut want = entry("https://e.com/b.tar.gz", "dist");
        want.extract = true;
        let existing = vec!["dist/tool".to_string()];

        let diff = classify(&[want], &[], &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Adopt]);
        assert!(diff.unknown.is_empty());
    }

    #[test]
    fn root_extract_is_added_into_an_empty_directory() {
        let mut want = entry("https://e.com/tool.tar.gz", "");
        want.extract = true;

        // 只剩清单的目录 = 什么都没装
        let diff = classify(&[want.clone()], &[], &[], false, &[]).unwrap();
        assert_eq!(kept(&diff), vec![Change::Add]);

        let diff = classify(&[want], &[], &[], true, &[]).unwrap();
        assert_eq!(kept(&diff), vec![Change::Add]);
    }

    #[test]
    fn root_extract_that_landed_keeps() {
        let mut want = entry("https://e.com/tool.tar.gz", "");
        want.extract = true;
        let existing = vec!["lansend".to_string()];

        let diff = classify(&[want.clone()], &[want], &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Keep]);
    }

    #[test]
    fn root_extract_recorded_over_an_empty_directory_is_repaired() {
        let mut want = entry("https://e.com/tool.tar.gz", "");
        want.extract = true;

        // 清单说装过，但目录里空到只剩清单：上次解压没成，补一次
        let diff = classify(&[want.clone()], &[want], &[], false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Repair]);
    }

    #[test]
    fn root_extract_over_unrecorded_content_is_adopted() {
        let mut want = entry("https://e.com/tool.tar.gz", "");
        want.extract = true;
        let existing = vec!["lansend".to_string()];

        let diff = classify(&[want], &[], &existing, false, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Adopt]);
        assert!(diff.unknown.is_empty(), "根上的文件都归这条解压条目");
    }

    #[test]
    fn stray_files_are_reported_not_removed() {
        let want = vec![entry("https://e.com/a", "a")];
        let existing = vec!["a".to_string(), "scratch.txt".to_string()];
        let diff = classify(&want, &[], &existing, false, &[]).unwrap();

        assert_eq!(diff.unknown, vec!["scratch.txt".to_string()]);
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn force_reinstalls_everything() {
        let want = vec![entry("https://e.com/a", "a")];
        let existing = vec!["a".to_string()];
        let diff = classify(&want, &want.clone(), &existing, true, &[]).unwrap();

        assert_eq!(kept(&diff), vec![Change::Update]);
    }

    #[test]
    fn escaping_dest_fails_the_whole_run() {
        let want = vec![entry("https://e.com/a", "../x")];
        let err = classify(&want, &[], &[], false, &[]).unwrap_err();

        assert!(err.contains("a \"..\" segment"), "{err}");
    }

    #[test]
    fn dest_validation_shapes() {
        assert!(validate_dest("").is_ok());
        assert!(validate_dest("bin/tool").is_ok());
        assert!(validate_dest("../x").is_err());
        assert!(validate_dest("/abs").is_err());
        assert!(validate_dest("C:/x").is_err());
        assert!(validate_dest("a\\b").is_err());
        assert!(validate_dest("a//b").is_err());
        assert!(validate_dest("./a").is_err());
    }

    #[test]
    fn manifest_roundtrips_and_skips_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![entry("https://e.com/a", "a")];
        save(dir.path(), &entries, "0.8.1").unwrap();

        let text = fs::read_to_string(dir.path().join(MANIFEST_NAME)).unwrap();
        assert!(!text.contains("extract"), "{text}");

        let State::Present(manifest) = load(dir.path()) else {
            panic!("expected a manifest");
        };
        assert_eq!(manifest.entries, entries);
        assert_eq!(manifest.byk.as_deref(), Some("0.8.1"));
    }

    #[test]
    fn new_manifest_version_is_unusable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(MANIFEST_NAME),
            r#"{"version": 99, "entries": []}"#,
        )
        .unwrap();

        let State::Unusable(why) = load(dir.path()) else {
            panic!("expected unusable");
        };
        assert!(why.contains("newer"), "{why}");
    }

    #[test]
    fn broken_manifest_is_unusable_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(MANIFEST_NAME), "{not json").unwrap();

        assert!(matches!(load(dir.path()), State::Unusable(_)));
    }

    #[test]
    fn existing_paths_skips_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin/tool"), "x").unwrap();
        fs::write(dir.path().join(MANIFEST_NAME), "{}").unwrap();

        assert_eq!(existing_paths(dir.path()), vec!["bin/tool".to_string()]);
    }

    #[test]
    fn existing_paths_skips_the_pip_venv() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(VENV_DIR).join("bin")).unwrap();
        fs::write(dir.path().join(VENV_DIR).join("bin/python"), "x").unwrap();
        fs::write(dir.path().join("tool"), "x").unwrap();

        assert_eq!(existing_paths(dir.path()), vec!["tool".to_string()]);
    }

    #[test]
    fn entry_for_keeps_to_as_slash_path() {
        let dep = DepEntry {
            index: 0,
            src: "bin/tool".to_string(),
            to: vec!["bin".to_string(), "sub".to_string()],
            name: Some("tool".to_string()),
            extract: false,
            exec: true,
        };
        let asset = PlannedAsset {
            index: 0,
            label: "@@t/bin/sub/tool".to_string(),
            origin: crate::deps::Origin::Url("https://e.com/a/bin/tool".to_string()),
            dest: std::path::PathBuf::from("/deps/bin/sub/tool"),
            archive: false,
            exec: true,
        };
        let record = entry_for(&dep, &asset, "bin/sub/tool".to_string());

        assert_eq!(record.to, "bin/sub");
        assert!(record.exec);
        // 清单记的是解析后的地址，相对写法留在声明里
        assert_eq!(record.src, "https://e.com/a/bin/tool");
    }
}
