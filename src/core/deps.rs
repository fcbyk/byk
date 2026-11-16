//! `$deps`：全局别名文件的依赖声明、落点计划与安装执行。
//!
//! 一个全局别名文件可以声明它需要的资产（`$deps.files`），byk 负责把
//! 它们下载到该文件的专属目录 `~/.byk/deps/<key>/`。byk 自己不做网络：
//! 下载交给系统 `curl`，解压交给系统 `tar`——两者在 Windows / macOS /
//! Linux 上都是现代系统自带，缺失时报错并提示安装。
//!
//! 三条贯穿规则（沿自 byk-add）：
//!
//! 1. **不覆盖**。目标已存在就报错；归档先用 `tar -tf` 列清单逐个查。
//! 2. **临时文件与目标同级**。下载先落到 `dest` 同目录的唯一临时文件，
//!    成功后 rename 过去，中途失败不留半个文件。
//! 3. **落点圈死在专属目录内**。`to` 只接受相对子目录段，拒绝 `..`、
//!    绝对路径与反斜杠；归档条目额外做越界检查。
//!
//! `src` 只支持 http(s) URL：别名文件住在 `~/.byk/alias/`，
//! 相对它解析本地路径没有意义。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::config::build_file_key;

/// `$deps.files` 条目允许的字段；报错时按这个顺序列出。
const ENTRY_FIELDS: [&str; 5] = ["src", "to", "name", "extract", "exec"];

/// Windows 保留设备名；作为落盘文件名的主名（首段）时拒绝。
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 一个依赖条目：语法层校验后的产物，不碰文件系统。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DepEntry {
    pub(crate) index: usize,
    /// http(s) URL。
    pub(crate) src: String,
    /// 相对专属目录的子目录段，空切片表示根。
    pub(crate) to: Vec<String>,
    /// 落点文件名；缺省从 `src` 推导。
    pub(crate) name: Option<String>,
    /// 把 `src` 当压缩包解压到 `to`，而不是落盘压缩包本身。
    pub(crate) extract: bool,
    /// 非 Windows 补 `chmod 755`。
    pub(crate) exec: bool,
}

/// 计划层产物：绝对落点已确定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedAsset {
    pub(crate) index: usize,
    /// 展示用标签：`<key>/<to>/<name>`。
    pub(crate) label: String,
    pub(crate) url: String,
    pub(crate) dest: PathBuf,
    /// 解压条目；`dest` 是目标目录本身。
    pub(crate) archive: bool,
    pub(crate) exec: bool,
}

/// 全局别名文件 `@@<stem>` 的依赖目录。
pub(crate) fn deps_dir_for_stem(stem: &str, byk_root: &Path) -> PathBuf {
    let key = build_file_key(stem, true);
    byk_root.join("deps").join(key)
}

/// 计算某文件应注入的 `deps_dir` 变量值；局部文件不注入（返回 `None`，
/// `${deps_dir}` 按变量替换规则原样保留）。
pub(crate) fn deps_dir_var(stem: &str, is_global: bool, byk_root: &Path) -> Option<String> {
    if !is_global {
        return None;
    }
    Some(
        deps_dir_for_stem(stem, byk_root)
            .to_string_lossy()
            .into_owned(),
    )
}

// ── 语法层：`$deps` 结构校验 ────────────────────────────────────────────

/// 解析并校验 `$deps` 字段的值；返回条目列表。
///
/// 只回答"长得对不对"：类型、必填、互斥、未知字段、URL scheme。落点
/// 拼接、文件名推导与冲突检测留给 [`plan_entries`]。
pub(crate) fn parse_deps(value: &Value) -> Result<Vec<DepEntry>, String> {
    let Some(obj) = value.as_object() else {
        return Err("$deps must be an object".to_string());
    };

    for key in obj.keys() {
        if key != "files" {
            return Err(format!(
                "$deps has unknown field \"{key}\"; only \"files\" is supported"
            ));
        }
    }

    let Some(items) = obj.get("files") else {
        return Err("$deps must name a \"files\" array".to_string());
    };
    let Value::Array(items) = items else {
        return Err("$deps.files must be an array".to_string());
    };

    items
        .iter()
        .enumerate()
        .map(|(index, item)| parse_entry(index, item))
        .collect()
}

fn parse_entry(index: usize, item: &Value) -> Result<DepEntry, String> {
    let fields = match item {
        Value::String(src) => {
            return parse_fields(index, |field| {
                if field == "src" {
                    Some(Value::String(src.clone()))
                } else {
                    None
                }
            });
        }
        Value::Object(fields) => fields,
        _ => {
            return Err(format!("files[{index}] must be a string or an object"));
        }
    };

    for key in fields.keys() {
        if !ENTRY_FIELDS.contains(&key.as_str()) {
            return Err(format!(
                "files[{index}]: unknown field \"{key}\"; expected {}",
                ENTRY_FIELDS
                    .iter()
                    .map(|f| format!("\"{f}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    parse_fields(index, |field| fields.get(field).cloned())
}

/// 条目字段的统一校验：字符串简写与对象写法归一后走同一条路。
fn parse_fields<F>(index: usize, read: F) -> Result<DepEntry, String>
where
    F: Fn(&str) -> Option<Value>,
{
    let string_field = |name: &str| -> Result<Option<String>, String> {
        match read(name) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(format!("files[{index}]: \"{name}\" must be a string")),
        }
    };

    let src = string_field("src")?.ok_or_else(|| format!("files[{index}]: \"src\" is required"))?;
    if src.is_empty() {
        return Err(format!("files[{index}]: \"src\" must not be empty"));
    }
    if src.contains('\\') {
        return Err(format!(
            "files[{index}]: \"src\" must use \"/\" as the path separator"
        ));
    }
    check_url(index, &src)?;

    let to = match string_field("to")? {
        None => Vec::new(),
        Some(raw) => parse_relative_dir(index, &raw)?,
    };

    let name = match string_field("name")? {
        None => None,
        Some(raw) => {
            validate_asset_name(index, "name", &raw)?;
            Some(raw)
        }
    };

    let extract = parse_flag(index, "extract", &read)?;
    let exec = parse_flag(index, "exec", &read)?;

    if extract && name.is_some() {
        return Err(format!(
            "files[{index}]: \"name\" is not allowed together with \"extract\"; name the directory in \"to\""
        ));
    }
    if extract && exec {
        return Err(format!(
            "files[{index}]: \"extract\" and \"exec\" cannot both be true"
        ));
    }

    Ok(DepEntry {
        index,
        src,
        to,
        name,
        extract,
        exec,
    })
}

fn parse_flag<F>(index: usize, field: &'static str, read: &F) -> Result<bool, String>
where
    F: Fn(&str) -> Option<Value>,
{
    match read(field) {
        None => Ok(false),
        Some(Value::Bool(flag)) => Ok(flag),
        Some(_) => Err(format!("files[{index}]: \"{field}\" must be true or false")),
    }
}

/// `to`：相对专属目录的子目录，`/` 分段，拒绝空段、`.`、`..`、反斜杠、
/// 绝对路径与 `~`。结尾的 `/` 被忽略。
fn parse_relative_dir(index: usize, raw: &str) -> Result<Vec<String>, String> {
    if raw.contains('\\') {
        return Err(format!(
            "files[{index}]: \"to\" must use \"/\" as the path separator"
        ));
    }
    if raw.starts_with('/') || is_drive_path(raw) {
        return Err(format!(
            "files[{index}]: \"to\" must be relative to the deps directory"
        ));
    }
    if raw == "~" || raw.starts_with("~/") {
        return Err(format!(
            "files[{index}]: \"to\" must not start with \"~\"; it is relative to the deps directory"
        ));
    }

    let mut segments: Vec<&str> = raw.split('/').collect();
    if segments.last() == Some(&"") {
        segments.pop();
    }

    let mut out = Vec::with_capacity(segments.len());
    for segment in segments {
        match segment {
            "" => {
                return Err(format!(
                    "files[{index}]: \"to\" must not contain an empty path segment"
                ));
            }
            "." => {
                return Err(format!(
                    "files[{index}]: \"to\" must not contain the \".\" segment"
                ));
            }
            ".." => {
                return Err(format!(
                    "files[{index}]: \"to\" must not contain the \"..\" segment"
                ));
            }
            other => out.push(other.to_string()),
        }
    }
    Ok(out)
}

/// 落盘文件名：单个路径段，允许扩展名（`.`），拒绝分隔符、控制字符、
/// 结尾空格或点，以及 Windows 保留设备名。
fn validate_asset_name(index: usize, field: &'static str, raw: &str) -> Result<(), String> {
    let reject = |why: String| format!("files[{index}]: \"{field}\" {why}");

    if raw.is_empty() {
        return Err(reject("must not be empty".to_string()));
    }
    if raw.contains('/') {
        return Err(reject(
            "must not contain '/'; name the directory in \"to\"".to_string(),
        ));
    }
    if raw.contains('\\') {
        return Err(reject("must not contain '\\'".to_string()));
    }
    if raw == "." || raw == ".." {
        return Err(reject(format!("must not be \"{raw}\"")));
    }
    if raw.chars().any(|c| c.is_control()) {
        return Err(reject("must not contain control characters".to_string()));
    }
    if raw.ends_with(' ') || raw.ends_with('.') {
        return Err(reject("must not end with a space or a dot".to_string()));
    }

    let stem = raw
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        return Err(reject(format!(
            "\"{stem}\" is a reserved device name on Windows"
        )));
    }
    Ok(())
}

// ── 计划层：URL 校验、文件名推导、落点与冲突 ────────────────────────────

/// 把条目变成绝对落点；冲突即报错。不检查目标是否已存在——那是执行期
/// 的事，计划要能在安装前完整打印。
pub(crate) fn plan_entries(
    entries: &[DepEntry],
    deps_dir: &Path,
    key: &str,
) -> Result<Vec<PlannedAsset>, String> {
    let mut assets = Vec::with_capacity(entries.len());

    for entry in entries {
        let name = match &entry.name {
            Some(name) => name.clone(),
            None => derive_name(entry.index, &entry.src)?,
        };

        let dest = if entry.extract {
            deps_dir.join(entry.to.join("/"))
        } else {
            deps_dir.join(entry.to.join("/")).join(&name)
        };

        // 标签跟着**落点**走：解压条目的落点是目录，压缩包文件名不该出现在
        // 标签里——否则日志和 `..deps status` 会暗示一个根本不存在的路径。
        let base = if entry.to.is_empty() {
            key.to_string()
        } else {
            format!("{key}/{}", entry.to.join("/"))
        };
        let label = if entry.extract {
            base
        } else {
            format!("{base}/{name}")
        };

        let clash = assets
            .iter()
            .find(|asset: &&PlannedAsset| asset.dest == dest && !(asset.archive && entry.extract));
        if let Some(clash) = clash {
            return Err(format!(
                "files[{}]: target \"{label}\" is already used by files[{}]",
                entry.index, clash.index
            ));
        }

        assets.push(PlannedAsset {
            index: entry.index,
            label,
            url: normalize_url(&entry.src),
            dest,
            archive: entry.extract,
            exec: entry.exec,
        });
    }

    // 解压到根（没有 `to`）的落点就是专属目录本身，重装时整目录替换；
    // 和其它条目并存会把它们一起换掉，所以只允许它单独存在。
    if assets.len() > 1 {
        for asset in &assets {
            if asset.archive && asset.dest == deps_dir {
                return Err(format!(
                    "files[{}]: extracting into the dependency root must be the only entry; set \"to\" to give it a directory",
                    asset.index
                ));
            }
        }
    }

    Ok(assets)
}

/// 没写 `name` 时从 URL 路径末段推导。
fn derive_name(index: usize, url: &str) -> Result<String, String> {
    let candidate = last_segment(url).unwrap_or_default();
    if candidate.is_empty() {
        return Err(format!(
            "files[{index}]: cannot derive a file name from \"{url}\"; set \"name\""
        ));
    }
    validate_asset_name(index, "name", candidate)
        .map_err(|err| {
            err.replace(
                &format!("files[{index}]: \"name\""),
                &format!("files[{index}]: derived file name is invalid (\"{candidate}\")"),
            )
        })
        .map(|_| candidate.to_string())
}

// ── 执行层：curl 下载、tar 预检与解压 ──────────────────────────────────

/// 落一条普通资产。
///
/// `replace` 为真时覆盖已存在的目标：先下到同级临时文件，再 rename 过去，
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
    if let Err(message) = download(index, &asset.url, &scratch) {
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
    if let Err(message) = download(index, &asset.url, &scratch) {
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

fn first_stderr(stderr: &[u8]) -> String {
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

// ── URL 字面量：scheme 识别、点段归约、末段推导 ─────────────────────────

/// `src` 必须是 http(s) URL。
fn check_url(index: usize, raw: &str) -> Result<(), String> {
    match split_scheme(raw) {
        None => Err(format!(
            "files[{index}]: \"src\" must be an http(s) URL; relative paths are not supported here"
        )),
        Some((scheme, rest)) if is_http(scheme) => {
            if !rest.starts_with("//") {
                Err(format!(
                    "files[{index}]: \"{raw}\" must contain \"//\" after the scheme; write https://host/path"
                ))
            } else {
                Ok(())
            }
        }
        Some((scheme, _)) => Err(format!(
            "files[{index}]: unsupported scheme \"{scheme}\"; only http and https are supported"
        )),
    }
}

/// 识别 `scheme:` 前缀；冒号必须出现在第一个 `/` 之前。
fn split_scheme(raw: &str) -> Option<(&str, &str)> {
    let colon = raw.find(':')?;
    let scheme = &raw[..colon];
    if scheme.is_empty() || scheme.contains('/') {
        return None;
    }

    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }

    Some((scheme, &raw[colon + 1..]))
}

fn is_http(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

/// 形如 `C:` 的 Windows 盘符路径。
fn is_drive_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// 归约 URL 路径里的 `.` / `..` 段（RFC 3986 5.2.4），只动路径。
fn normalize_url(url: &str) -> String {
    let Some((head, path, tail)) = split_url(url) else {
        return url.to_string();
    };
    format!("{head}{}{tail}", remove_dot_segments(path))
}

/// URL 路径的末段，用于推导文件名。
fn last_segment(url: &str) -> Option<&str> {
    let (_, path, _) = split_url(url)?;
    path.rsplit('/').next()
}

/// 拆成 (scheme + authority, 路径, query/fragment)。
fn split_url(url: &str) -> Option<(&str, &str, &str)> {
    let scheme_end = url.find("://")? + 3;
    let rest = &url[scheme_end..];
    let path_at = rest.find('/')?;
    let path_start = scheme_end + path_at;
    let after = &url[path_start..];
    let tail_at = after.find(['?', '#']).unwrap_or(after.len());
    Some((&url[..path_start], &after[..tail_at], &after[tail_at..]))
}

fn remove_dot_segments(path: &str) -> String {
    let mut input = path.to_string();
    let mut output = String::new();

    let pop = |output: &mut String| {
        if let Some(at) = output.rfind('/') {
            output.truncate(at);
        }
    };

    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../") {
            input = rest.to_string();
        } else if let Some(rest) = input.strip_prefix("./") {
            input = rest.to_string();
        } else if let Some(rest) = input.strip_prefix("/./") {
            input = format!("/{rest}");
        } else if input == "/." {
            input = "/".to_string();
        } else if let Some(rest) = input.strip_prefix("/../") {
            input = format!("/{rest}");
            pop(&mut output);
        } else if input == "/.." {
            input = "/".to_string();
            pop(&mut output);
        } else if input == "." || input == ".." {
            input.clear();
        } else {
            let start = usize::from(input.starts_with('/'));
            let at = input[start..]
                .find('/')
                .map(|at| start + at)
                .unwrap_or(input.len());
            output.push_str(&input[..at]);
            input = input[at..].to_string();
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(files: Value) -> Result<Vec<DepEntry>, String> {
        parse_deps(&json!({ "files": files }))
    }

    fn entry(files: Value, index: usize) -> DepEntry {
        parse(files).unwrap().remove(index)
    }

    // ---------- parse_deps ----------

    #[test]
    fn string_entry_is_shorthand_for_src() {
        let entry = entry(json!(["https://example.com/tool"]), 0);
        assert_eq!(entry.src, "https://example.com/tool");
        assert!(entry.to.is_empty());
        assert_eq!(entry.name, None);
        assert!(!entry.extract && !entry.exec);
    }

    #[test]
    fn object_entry_parses_all_fields() {
        let entry = entry(
            json!([{"src": "https://example.com/t", "to": "bin/sub/", "name": "t", "exec": true}]),
            0,
        );
        assert_eq!(entry.to, vec!["bin", "sub"]);
        assert_eq!(entry.name.as_deref(), Some("t"));
        assert!(entry.exec);
    }

    #[test]
    fn missing_files_is_an_error() {
        assert_eq!(
            parse_deps(&json!({})).unwrap_err(),
            "$deps must name a \"files\" array"
        );
    }

    #[test]
    fn unknown_top_field_is_rejected() {
        assert!(
            parse_deps(&json!({"npm": []}))
                .unwrap_err()
                .contains("unknown field \"npm\"")
        );
    }

    #[test]
    fn unknown_entry_field_is_rejected() {
        let err = parse(json!([{"src": "https://e.com/a", "version": 1}])).unwrap_err();
        assert!(err.contains("unknown field \"version\""), "{err}");
    }

    #[test]
    fn src_is_required_and_nonempty() {
        assert_eq!(
            parse(json!([{}])).unwrap_err(),
            "files[0]: \"src\" is required"
        );
        assert_eq!(
            parse(json!([""])).unwrap_err(),
            "files[0]: \"src\" must not be empty"
        );
    }

    #[test]
    fn src_must_be_http_url() {
        assert!(
            parse(json!(["./tool"]))
                .unwrap_err()
                .contains("http(s) URL")
        );
        assert!(
            parse(json!(["/abs/tool"]))
                .unwrap_err()
                .contains("http(s) URL")
        );
        assert!(parse(json!(["ftp://e.com/a"])).unwrap_err().contains("ftp"));
        assert!(parse(json!(["https:/e.com/a"])).unwrap_err().contains("//"));
        parse(json!(["http://e.com/a"])).unwrap();
        parse(json!(["HTTPS://e.com/a"])).unwrap();
    }

    #[test]
    fn to_rejects_escape_attempts() {
        for bad in [
            "../x", "a/../b", "a//b", "a/./b", "/abs", "~/x", "C:/x", "a\\b",
        ] {
            let err = parse(json!([{"src": "https://e.com/a", "to": bad}])).unwrap_err();
            assert!(err.contains("\"to\""), "{bad}: {err}");
        }
    }

    #[test]
    fn name_rejects_separators_and_reserved_names() {
        for bad in ["a/b", "a\\b", ".", "..", "CON", "con.exe", "x ", "x."] {
            let err = parse(json!([{"src": "https://e.com/a", "name": bad}])).unwrap_err();
            assert!(err.contains("\"name\""), "{bad}: {err}");
        }
    }

    #[test]
    fn name_allows_extensions() {
        parse(json!([{"src": "https://e.com/a", "name": "my-tool.tar.gz"}])).unwrap();
    }

    #[test]
    fn flags_must_be_boolean() {
        let err = parse(json!([{"src": "https://e.com/a", "extract": "yes"}])).unwrap_err();
        assert!(err.contains("\"extract\" must be true or false"), "{err}");
    }

    #[test]
    fn extract_is_mutually_exclusive() {
        let err =
            parse(json!([{"src": "https://e.com/a", "extract": true, "name": "x"}])).unwrap_err();
        assert!(
            err.contains("not allowed together with \"extract\""),
            "{err}"
        );

        let err =
            parse(json!([{"src": "https://e.com/a", "extract": true, "exec": true}])).unwrap_err();
        assert!(err.contains("cannot both be true"), "{err}");
    }

    // ---------- plan_entries ----------

    fn tmp_dir() -> PathBuf {
        tempfile::tempdir().unwrap().path().join("deps").join("@@t")
    }

    #[test]
    fn plan_derives_name_from_url() {
        let dir = tmp_dir();
        let entries = parse(json!(["https://e.com/dist/my-tool-1.4.tar.gz?token=x"])).unwrap();
        let assets = plan_entries(&entries, &dir, "@@t").unwrap();
        assert_eq!(assets[0].dest, dir.join("my-tool-1.4.tar.gz"));
        assert_eq!(assets[0].label, "@@t/my-tool-1.4.tar.gz");
        assert_eq!(
            assets[0].url,
            "https://e.com/dist/my-tool-1.4.tar.gz?token=x"
        );
    }

    #[test]
    fn plan_normalizes_dot_segments_in_url() {
        let dir = tmp_dir();
        let entries = parse(json!(["https://e.com/dist/../rel/tool"])).unwrap();
        let assets = plan_entries(&entries, &dir, "@@t").unwrap();
        assert_eq!(assets[0].url, "https://e.com/rel/tool");
    }

    #[test]
    fn plan_derive_failure_mentions_name() {
        let entries = parse(json!(["https://e.com/"])).unwrap();
        let err = plan_entries(&entries, &tmp_dir(), "@@t").unwrap_err();
        assert!(err.contains("set \"name\""), "{err}");
    }

    #[test]
    fn plan_extract_dest_is_the_directory() {
        let dir = tmp_dir();
        let entries =
            parse(json!([{"src": "https://e.com/b.tar.gz", "to": "dist", "extract": true}]))
                .unwrap();
        let assets = plan_entries(&entries, &dir, "@@t").unwrap();
        assert_eq!(assets[0].dest, dir.join("dist"));
        assert!(assets[0].archive);
        // 标签指向目录，而不是那个只用来中转的压缩包名
        assert_eq!(assets[0].label, "@@t/dist");
    }

    #[test]
    fn plan_extract_into_root_is_labelled_by_key() {
        let entries = parse(json!([{"src": "https://e.com/b.tar.gz", "extract": true}])).unwrap();
        let assets = plan_entries(&entries, &tmp_dir(), "@@t").unwrap();
        assert_eq!(assets[0].label, "@@t");
    }

    #[test]
    fn root_extract_must_be_the_only_entry() {
        let dir = tmp_dir();
        let entries = parse(json!([
            {"src": "https://e.com/b.tar.gz", "extract": true},
            {"src": "https://e.com/cfg.json"}
        ]))
        .unwrap();

        let err = plan_entries(&entries, &dir, "@@t").unwrap_err();

        assert!(err.contains("must be the only entry"), "{err}");

        // 单独一条就合法：lansend 那种发布包形态
        let single = parse(json!([{"src": "https://e.com/b.tar.gz", "extract": true}])).unwrap();
        plan_entries(&single, &dir, "@@t").unwrap();
    }

    #[test]
    fn plan_detects_clashes() {
        let entries =
            parse(json!(["https://e.com/a", {"src": "https://e.com/b", "name": "a"}])).unwrap();
        let err = plan_entries(&entries, &tmp_dir(), "@@t").unwrap_err();
        assert!(err.contains("already used by files[0]"), "{err}");
    }

    #[test]
    fn plan_allows_file_into_extracted_directory() {
        let entries = parse(json!([
            {"src": "https://e.com/b.tar.gz", "to": "dist", "extract": true},
            {"src": "https://e.com/README", "to": "dist", "name": "README"}
        ]))
        .unwrap();
        plan_entries(&entries, &tmp_dir(), "@@t").unwrap();
    }

    // ---------- archive_entry_path ----------

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

    // ---------- url helpers ----------

    #[test]
    fn last_segment_ignores_query() {
        assert_eq!(
            last_segment("https://e.com/a/b.tar.gz?x=1#f"),
            Some("b.tar.gz")
        );
        assert_eq!(last_segment("https://e.com"), None);
    }

    #[test]
    fn remove_dot_segments_follows_rfc() {
        assert_eq!(remove_dot_segments("/a/b/../c"), "/a/c");
        assert_eq!(remove_dot_segments("/a/./b"), "/a/b");
        assert_eq!(remove_dot_segments("/../x"), "/x");
        assert_eq!(remove_dot_segments("/a/../../b"), "/b");
    }

    #[test]
    fn deps_dir_var_is_global_only() {
        let root = PathBuf::from("/home/u/.byk");
        assert_eq!(
            deps_dir_var("ops", true, &root),
            Some("/home/u/.byk/deps/@@ops".to_string())
        );
        assert_eq!(deps_dir_var("ops", false, &root), None);
        assert_eq!(
            deps_dir_for_stem("ops", &root),
            PathBuf::from("/home/u/.byk/deps/@@ops")
        );
    }
}
