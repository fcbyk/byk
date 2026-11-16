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
//! 2. **临时文件与目标同级**。取回先落到 `dest` 同目录的唯一临时文件，
//!    成功后 rename 过去，中途失败不留半个文件。
//! 3. **落点圈死在专属目录内**。`to` 只接受相对子目录段，拒绝 `..`、
//!    绝对路径与反斜杠；归档条目额外做越界检查。
//!
//! `src` 可以写绝对 http(s) URL，也可以写相对路径——相对谁由 `$deps.base`
//! 决定。base 是**目录**，落盘后必须是绝对地址：别名文件住在 `~/.byk/alias/`，
//! 它自己没有任何位置信息，相对值在那里无从解析。冻结只发生在 `..clone`：
//! 远程来源写成 http(s) 目录 URL（[`freeze_remote_base`]），本地来源写成
//! 本机绝对路径（[`freeze_local_base`]）。
//!
//! 两种 base 对应两种来源：URL 交给 `curl`，本地路径走 `fs::copy`。远程 clone
//! 只会写出 URL 形态，于是「从网上拿到的别名文件读不了本机文件」这条边界由
//! clone 保证，安装期只管照着执行——本地路径形态只可能来自本地 clone 或用户
//! 手写。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::config::build_file_key;

/// `$deps` 顶层允许的字段；报错时按这个顺序列出。
const DEP_FIELDS: [&str; 2] = ["files", "base"];

/// `$deps.files` 条目允许的字段；报错时按这个顺序列出。
const ENTRY_FIELDS: [&str; 5] = ["src", "to", "name", "extract", "exec"];

/// Windows 保留设备名；作为落盘文件名的主名（首段）时拒绝。
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 一份完整的 `$deps` 声明：基准目录加条目列表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration {
    /// 相对 `src` 的解析基准。落盘态只可能是绝对地址，见 [`Base`]。
    pub(crate) base: Option<Base>,
    pub(crate) files: Vec<DepEntry>,
}

/// `$deps.base` 的两种落盘形态。
///
/// 两者都是**目录**，都必定以 `/` 结尾：远程 clone 写出 `Url`，本地 clone
/// 写出 `Dir`。形态决定 `src` 怎么拼、执行层用 `curl` 还是 `fs::copy`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Base {
    /// 绝对 http(s) 目录 URL。
    Url(String),
    /// 本机绝对目录路径，用 `/` 分隔（Windows 也是）。
    Dir(String),
}

impl Base {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Url(text) | Self::Dir(text) => text,
        }
    }
}

/// 一个依赖条目：语法层校验后的产物，不碰文件系统。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DepEntry {
    pub(crate) index: usize,
    /// 绝对 http(s) URL，或相对 `$deps.base` 的路径（原样保留，计划期拼接）。
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
    /// 解析后的绝对来源。
    pub(crate) origin: Origin,
    pub(crate) dest: PathBuf,
    /// 解压条目；`dest` 是目标目录本身。
    pub(crate) archive: bool,
    pub(crate) exec: bool,
}

/// 一个资产的绝对来源：远程 URL，或本机文件。
///
/// 形态由 `base` 的类型决定，`src` 自己不表态——绝对 URL 的 `src` 永远走到
/// [`Origin::Url`]，只有相对 `src` 才可能被本地 base 拼成本机路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    Url(String),
    File(PathBuf),
}

impl Origin {
    /// 展示与清单共用的字面量。本地路径统一 `/` 分隔，Windows 也一样。
    pub(crate) fn as_str(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::File(path) => path_to_slash(path),
        }
    }

    /// 来源是本机文件吗。这类条目走 `fs::copy`，不碰网络——汇总行据此
    /// 注明其中几条来自本地。
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Self::File(_))
    }
}

/// 某个别名文件的依赖目录。
///
/// 两种作用域同一套形状，只有根不同：
///
/// - 全局：`~/.byk/deps/@@<stem>`
/// - 局部：`<文件所在目录>/.byk/deps/@<stem>`
///
/// 局部依赖跟着项目走：删项目即删依赖，不需要哈希目录，也不需要额外的映射表
/// ——目录本身就是身份。
pub(crate) fn deps_dir(stem: &str, is_global: bool, file_dir: &Path, byk_root: &Path) -> PathBuf {
    let key = build_file_key(stem, is_global);
    if is_global {
        byk_root.join("deps").join(key)
    } else {
        file_dir.join(".byk").join("deps").join(key)
    }
}

/// 计算某文件应注入的 `deps_dir` 变量值。
pub(crate) fn deps_dir_var(
    stem: &str,
    is_global: bool,
    file_dir: &Path,
    byk_root: &Path,
) -> String {
    deps_dir(stem, is_global, file_dir, byk_root)
        .to_string_lossy()
        .into_owned()
}

// ── 语法层：`$deps` 结构校验 ────────────────────────────────────────────

/// 解析并校验 `$deps` 字段的值；返回声明。
///
/// 只回答"长得对不对"：类型、必填、互斥、未知字段、URL scheme。落点
/// 拼接、文件名推导与冲突检测留给 [`plan_entries`]。
pub(crate) fn parse_deps(value: &Value) -> Result<Declaration, String> {
    let Some(obj) = value.as_object() else {
        return Err("$deps must be an object".to_string());
    };

    for key in obj.keys() {
        if !DEP_FIELDS.contains(&key.as_str()) {
            return Err(format!(
                "$deps has unknown field \"{key}\"; expected \"files\" or \"base\""
            ));
        }
    }

    let base = match obj.get("base") {
        None => None,
        Some(raw) => Some(parse_base(raw)?),
    };

    let Some(items) = obj.get("files") else {
        return Err("$deps must name a \"files\" array".to_string());
    };
    let Value::Array(items) = items else {
        return Err("$deps.files must be an array".to_string());
    };

    let files = items
        .iter()
        .enumerate()
        .map(|(index, item)| parse_entry(index, item))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Declaration { base, files })
}

/// `base`：目录，落盘态只认绝对地址——http(s) URL，或本机绝对路径。
///
/// 拒绝相对值：解析相对 base 需要文档自己的位置，而那份信息只在 clone 那一
/// 刻存在（见 [`freeze_remote_base`] / [`freeze_local_base`]）。放进来就会
/// 变成「写了个谁都看不懂的字符串」，或者更糟：被当成本地路径去拼。
///
/// Windows 上 `/sub` 也不算绝对地址——它没有盘符，按「当前盘」解释，写法本身
/// 就带着工作目录依赖，同样拒绝。
fn parse_base(raw: &Value) -> Result<Base, String> {
    let Value::String(text) = raw else {
        return Err("\"$deps.base\" must be a string".to_string());
    };
    if text.is_empty() {
        return Err(
            "\"$deps.base\" must not be empty (the value is empty after variable expansion)"
                .to_string(),
        );
    }
    if text.contains('\\') {
        return Err("\"$deps.base\" must use \"/\" as the path separator".to_string());
    }
    if text.contains(['?', '#']) {
        return Err(
            "\"$deps.base\" must not contain a query or fragment; it is a directory".to_string(),
        );
    }

    // 本地路径形态先过一遍：UNC 与 Windows 上没有盘符的根路径都在这里挡掉，
    // 否则 `/sub` 会被当成相对值拼到别处。
    check_local_shape(text)?;

    // 盘符要先于 scheme 判定：`C:/x` 里的 `C:` 会被 `split_scheme` 当成 scheme
    if is_drive_path(text) || Path::new(text).is_absolute() {
        return parse_local_base(text);
    }

    match split_scheme(text) {
        Some((scheme, rest)) if is_http(scheme) => {
            if !rest.starts_with("//") {
                return Err(format!(
                    "\"$deps.base\" \"{text}\" must contain \"//\" after the scheme; write https://host/path/"
                ));
            }
            Ok(Base::Url(as_dir_url(text)))
        }
        Some((scheme, _)) => Err(format!(
            "\"$deps.base\" uses unsupported scheme \"{scheme}\"; only http and https are supported"
        )),
        // 展开后还带着 `${...}`：变量没定义（或自引用），值本身就是半成品
        None => match pending_var(text) {
            Some(name) => Err(format!(
                "\"$deps.base\" \"{text}\" is not an absolute address: \"${{{name}}}\" was not expanded\nHint: a ${{...}} base must resolve to an absolute URL on its own, or write the absolute base yourself"
            )),
            None => Err(format!(
                "\"$deps.base\" \"{text}\" must be an absolute http(s) URL or an absolute directory path; a relative value only resolves while cloning\nHint: clone this file from its source, or write the absolute base yourself"
            )),
        },
    }
}

/// 本地绝对目录的落盘校验。盘符在非 Windows 上只能报错——`C:/x` 在 Unix 上
/// 不是绝对路径，当相对值拼会得到一份看似合法、实则指向别处的配置。
///
/// 形态（UNC、Windows 的 `/` 根路径）已在调用方过 [`check_local_shape`]，
/// 这里只剩「这台机器认不认得出这是绝对路径」。
fn parse_local_base(text: &str) -> Result<Base, String> {
    if Path::new(text).is_absolute() {
        return Ok(Base::Dir(as_dir_path(text)));
    }
    Err(foreign_path_error(text))
}

/// 带盘符、但这台机器用不了的本地路径：`C:/x` 在 Unix 上不是绝对路径，
/// `C:x` 在 Windows 上是「当前目录」的相对形式——两边都指望不上。
fn foreign_path_error(text: &str) -> String {
    if cfg!(windows) {
        return format!(
            "\"$deps.base\" \"{text}\" has no directory after the drive; write C:/dir/ (a drive letter alone does not name a path)"
        );
    }
    format!(
        "\"$deps.base\" \"{text}\" is a Windows path; this build cannot use it\nHint: write the path for the system byk runs on"
    )
}

/// 本地路径字面量的公共前检：UNC 一律拒绝；Windows 上 `/sub` 这种没有盘符的
/// 根路径也拒绝——它挂在「当前盘」上，同一份配置换个工作目录就指向别处。
/// Unix 上 `/sub` 就是普通绝对路径，照收。
///
/// 只认相对路径与盘符开头的绝对路径，两边的含义才与工作目录无关。
fn check_local_shape(text: &str) -> Result<(), String> {
    if text.starts_with("//") {
        return Err(
            "\"$deps.base\" must not start with \"//\"; network (UNC) paths are not supported"
                .to_string(),
        );
    }
    if cfg!(windows) && text.starts_with('/') {
        return Err(format!(
            "\"$deps.base\" \"{text}\" has no drive letter; on Windows a \"/\"-rooted path hangs off the current drive\nHint: write a relative path, or an absolute path with a drive letter like C:/share/"
        ));
    }
    Ok(())
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
    check_src(index, &src)?;

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

/// 把条目变成绝对落点与绝对来源；冲突即报错。不检查目标是否已存在——
/// 那是执行期的事，计划要能在安装前完整打印。
///
/// 相对 `src` 在这里拼上 `base`，所以执行层拿到的永远是绝对地址：URL 交给
/// `curl`，本机路径交给 `fs::copy`。
pub(crate) fn plan_entries(
    decl: &Declaration,
    deps_dir: &Path,
    key: &str,
) -> Result<Vec<PlannedAsset>, String> {
    let files = &decl.files;
    let mut assets = Vec::with_capacity(files.len());

    for entry in files {
        let origin = resolve_src(entry, decl.base.as_ref())?;
        let name = match &entry.name {
            Some(name) => name.clone(),
            None => derive_name(entry.index, &origin)?,
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
            origin,
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

/// 条目的绝对来源：绝对 `src` 只做点段归约，相对 `src` 拼上 `base`。
///
/// 拼接方式跟着 base 的形态走：URL 按 RFC 3986 归约点段，路径按词法归约。
/// 两条路都不碰文件系统。
fn resolve_src(entry: &DepEntry, base: Option<&Base>) -> Result<Origin, String> {
    if split_scheme(&entry.src).is_some() {
        return Ok(Origin::Url(normalize_url(&entry.src)));
    }

    let base = base.ok_or_else(|| {
        format!(
            "files[{}]: \"src\" is a relative path but \"$deps.base\" is not set\nHint: add \"base\" with the directory these paths are relative to, or write absolute URLs",
            entry.index
        )
    })?;
    Ok(match base {
        Base::Url(url) => Origin::Url(join_url(url, &entry.src)),
        Base::Dir(dir) => Origin::File(join_path(dir, &entry.src)),
    })
}

/// 没写 `name` 时从来源的末段推导。
fn derive_name(index: usize, origin: &Origin) -> Result<String, String> {
    let candidate = match origin {
        Origin::Url(url) => last_segment(url).unwrap_or_default().to_string(),
        Origin::File(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if candidate.is_empty() {
        let shown = origin.as_str();
        return Err(format!(
            "files[{index}]: cannot derive a file name from \"{shown}\"; set \"name\""
        ));
    }
    validate_asset_name(index, "name", &candidate)
        .map_err(|err| {
            err.replace(
                &format!("files[{index}]: \"name\""),
                &format!("files[{index}]: derived file name is invalid (\"{candidate}\")"),
            )
        })
        .map(|_| candidate)
}

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

// ── URL 字面量：scheme 识别、点段归约、目录拼接 ─────────────────────────

/// `src` 要么是绝对 http(s) URL，要么是相对路径。
///
/// 相对形态只校验"不能是别处的东西"：绝对路径、盘符、`~` 都不行——它们
/// 指向文件系统，而这里最终只会变成一个 URL。
fn check_src(index: usize, raw: &str) -> Result<(), String> {
    if is_drive_path(raw) {
        return Err(format!(
            "files[{index}]: \"src\" must not be an absolute path; write it relative to \"$deps.base\""
        ));
    }

    match split_scheme(raw) {
        Some((scheme, rest)) if is_http(scheme) => {
            if rest.starts_with("//") {
                Ok(())
            } else {
                Err(format!(
                    "files[{index}]: \"{raw}\" must contain \"//\" after the scheme; write https://host/path"
                ))
            }
        }
        Some((scheme, _)) => Err(format!(
            "files[{index}]: unsupported scheme \"{scheme}\"; only http and https are supported"
        )),
        None if raw.starts_with('/') => Err(format!(
            "files[{index}]: \"src\" must not start with \"/\"; write it relative to \"$deps.base\""
        )),
        None if raw == "~" || raw.starts_with("~/") => Err(format!(
            "files[{index}]: \"src\" must not start with \"~\"; write it relative to \"$deps.base\""
        )),
        None => Ok(()),
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

/// 绝对 URL → 目录 URL：归约点段并补上结尾的 `/`。
///
/// base 的定义就是目录，所以缺斜杠时补一个，而不是猜"最后一段是文件"。
/// 直接粘文档 URL 进来会拼出 `x.byk.json/bin/tool` 这种地址，错得很显眼，
/// 也比"有时当目录、有时当文件"好教。
fn as_dir_url(url: &str) -> String {
    let normalized = normalize_url(url);
    if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    }
}

/// 把相对路径拼到目录 URL 上。
///
/// 点段按 RFC 3986 归约，`..` 可以一路向上，越界止于主机根——URL 里这不算
/// 权限边界，作者本来就掌控 base。相对路径自带的 query / fragment 原样保留。
fn join_url(base: &str, rel: &str) -> String {
    let (head, path) = split_base(base);
    let (rel_path, rel_tail) = split_tail(rel);
    let joined = remove_dot_segments(&format!("{}/{rel_path}", path.trim_end_matches('/')));
    format!("{head}{joined}{rel_tail}")
}

/// 拆出 (scheme + authority, 路径)；没有路径的 `https://host` 按根处理。
fn split_base(url: &str) -> (&str, &str) {
    match split_url(url) {
        Some((head, path, _)) => (head, path),
        None => (url, "/"),
    }
}

/// 拆出 (路径, query/fragment)。
fn split_tail(raw: &str) -> (&str, &str) {
    match raw.find(['?', '#']) {
        Some(at) => (&raw[..at], &raw[at..]),
        None => (raw, ""),
    }
}

/// 文档 URL → 它所在的目录。
fn dir_url(doc_url: &str) -> String {
    let (head, path) = split_base(doc_url);
    match path.rfind('/') {
        Some(at) => format!("{head}{}", &path[..=at]),
        None => format!("{head}/"),
    }
}

// ── 本地路径字面量：`/` 分隔、词法归约、目录拼接 ────────────────────────

/// 路径 → 落盘字面量：统一 `/` 分隔。
///
/// Windows 的 `Path::display()` 会给反斜杠，写进 JSON 得转义成 `\\`，diff
/// 和 `..deps status` 都难看；而 Windows 的文件 API 认 `/`，所以存储形态一律
/// 正斜杠，只在真正操作文件时才交回 `PathBuf`。
fn path_to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 目录 → 落盘字面量：`/` 分隔，必定以 `/` 结尾。和 [`as_dir_url`] 对称。
fn dir_string(path: &Path) -> String {
    let text = path_to_slash(path);
    if text.ends_with('/') {
        text
    } else {
        format!("{text}/")
    }
}

/// 本地路径 → 目录路径：归约点段并补上结尾的 `/`。
fn as_dir_path(text: &str) -> String {
    dir_string(&normalize_path(Path::new(text)))
}

/// 词法归约 `.` 与 `..`：不碰文件系统，也不解析符号链接。
///
/// 越界止于根——`/a/../../x` 得到 `/x`，与 URL 侧 [`remove_dot_segments`]
/// 的口径一致。base 是自己写的配置，`..` 不构成权限边界。
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 把相对路径拼到本地目录上，词法归约掉点段。
fn join_path(base: &str, rel: &str) -> PathBuf {
    normalize_path(&Path::new(base).join(rel))
}

// ── base 冻结：clone 期把相对值变成绝对地址 ─────────────────────────────

/// 两种冻结共用的字面量校验。
///
/// 变量一律拒绝：clone 期没有变量表，`${CDN}` 在这里求不出值，猜一个写死只会
/// 把作者意图烧成假地址。base 的定位就是"程序写的字面量"。
fn base_literal(current: Option<&Value>) -> Result<Option<&str>, String> {
    let Some(raw) = current else {
        return Ok(None);
    };
    let Value::String(text) = raw else {
        return Err("\"$deps.base\" must be a string".to_string());
    };
    if text.is_empty() {
        return Err("\"$deps.base\" must not be empty".to_string());
    }
    if text.contains('\\') {
        return Err("\"$deps.base\" must use \"/\" as the path separator".to_string());
    }
    if text.contains(['?', '#']) {
        return Err(
            "\"$deps.base\" must not contain a query or fragment; it is a directory".to_string(),
        );
    }
    if text.contains("${") {
        return Err(
            "\"$deps.base\" must not contain variables; a clone only freezes a literal value\nHint: write the absolute address in the source file — clone has no variable table to expand it with"
                .to_string(),
        );
    }
    if text == "~" || text.starts_with("~/") {
        return Err(
            "\"$deps.base\" must not start with \"~\"; a clone cannot resolve a home path"
                .to_string(),
        );
    }
    Ok(Some(text))
}

/// 远程 clone：把 `base` 冻结成绝对目录 URL。
///
/// - 缺失：文档 URL 所在目录，也就是"以文件自己为基准"
/// - 相对引用（`.`、`../`、`x/y`）：相对文档目录解析
/// - 绝对 http(s)：规范化，只保证以 `/` 结尾
///
/// 其余形态一律拒绝，**包括本机绝对路径**：远程来源只能产出 URL 形态的 base，
/// 于是"从网上 clone 来的别名文件读不了本机文件"是结构性保证，而不是靠检查。
/// Windows 上没有盘符的 `/sub` 也算在内：它既不是 URL，读出来也不是这台机器
/// 上确定的路径。
pub(crate) fn freeze_remote_base(current: Option<&Value>, doc_url: &str) -> Result<String, String> {
    let doc_dir = dir_url(doc_url);

    let Some(text) = base_literal(current)? else {
        return Ok(doc_dir);
    };

    // `/sub` 在 Windows 上没有盘符（按「当前盘」解释），在 Unix 上就是本机
    // 路径；`//` 是 UNC。三种读法都不属于远程来源，一律拒掉——否则同一份配置
    // 在 Windows 上会悄悄变成一个别的地址。
    if text.starts_with("//")
        || is_drive_path(text)
        || Path::new(text).is_absolute()
        || (cfg!(windows) && text.starts_with('/'))
    {
        return Err(format!(
            "\"$deps.base\" \"{text}\" is a local path; a remote clone only records http(s) URLs\nHint: point base at an http(s) URL, or clone from the file on disk"
        ));
    }

    match split_scheme(text) {
        Some((scheme, rest)) if is_http(scheme) => {
            if !rest.starts_with("//") {
                return Err(format!(
                    "\"$deps.base\" \"{text}\" must contain \"//\" after the scheme; write https://host/path/"
                ));
            }
            Ok(as_dir_url(text))
        }
        Some((scheme, _)) => Err(format!(
            "\"$deps.base\" uses unsupported scheme \"{scheme}\"; only http and https are supported"
        )),
        None => Ok(as_dir_url(&join_url(&doc_dir, text))),
    }
}

/// 本地 clone：把 `base` 冻结成绝对目录路径，绝对 http(s) URL 原样照搬。
///
/// - 缺失：源文件所在目录
/// - 绝对 http(s)：只做规范化（远程资产仍然从网上取）
/// - 绝对路径：照搬
/// - 相对引用（`.`、`../`、`x/y`）：相对源文件目录解析
/// - UNC 与 Windows 上没有盘符的 `/sub`：报错，见 [`check_local_shape`]
///
/// 基准一律是**源文件**所在目录，不是落盘目标目录：`files` 里的相对路径是
/// 作者照着源文档写的，`~/.byk/alias/` 旁边根本没有那些文件。
pub(crate) fn freeze_local_base(
    current: Option<&Value>,
    doc_path: &Path,
) -> Result<String, String> {
    let doc_dir = doc_path.parent().ok_or_else(|| {
        format!(
            "Cannot freeze \"$deps.base\": {} has no parent directory",
            doc_path.display()
        )
    })?;

    let Some(text) = base_literal(current)? else {
        return Ok(dir_string(&normalize_path(doc_dir)));
    };

    // UNC 与 Windows 上没有盘符的根路径在这里挡掉：`/sub` 会被 join 拼成
    // `C:/sub`，看着合法、指向别处，必须在冻结前失败。
    check_local_shape(text)?;

    // 盘符先判：`C:/x` 的 `C:` 会被当成 scheme
    if !is_drive_path(text)
        && let Some((scheme, rest)) = split_scheme(text)
    {
        return match is_http(scheme) {
            true if rest.starts_with("//") => Ok(as_dir_url(text)),
            true => Err(format!(
                "\"$deps.base\" \"{text}\" must contain \"//\" after the scheme; write https://host/path/"
            )),
            false => Err(format!(
                "\"$deps.base\" uses unsupported scheme \"{scheme}\"; only http and https are supported"
            )),
        };
    }

    let path = Path::new(text);
    if path.is_absolute() {
        return Ok(as_dir_path(text));
    }
    if is_drive_path(text) {
        return Err(foreign_path_error(text));
    }
    Ok(dir_string(&normalize_path(&doc_dir.join(path))))
}

/// 展开后仍留在字符串里的第一个 `${name}`。
///
/// 变量没定义时 [`crate::core::config::vars::expand`] 原样保留 `${name}`，
/// 于是 base 会停在一个半成品形态上；报错时点出名字，比说"不是 URL"有用。
fn pending_var(text: &str) -> Option<&str> {
    let start = text.find("${")? + 2;
    let end = text[start..].find('}')? + start;
    Some(&text[start..end])
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

    fn parse(files: Value) -> Result<Declaration, String> {
        parse_deps(&json!({ "files": files }))
    }

    fn parse_with(base: Value, files: Value) -> Result<Declaration, String> {
        parse_deps(&json!({ "base": base, "files": files }))
    }

    fn entry(files: Value, index: usize) -> DepEntry {
        let mut decl = parse(files).unwrap();
        decl.files.remove(index)
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
    fn src_accepts_http_urls_and_relative_paths() {
        // 绝对 URL
        parse(json!(["http://e.com/a"])).unwrap();
        parse(json!(["HTTPS://e.com/a"])).unwrap();

        // 相对路径：语法层放行，绑定 base 的事留给计划层
        parse(json!(["./tool"])).unwrap();
        parse(json!(["../bin/tool", "bin/tool"])).unwrap();

        // 指向文件系统的形态一律拒绝
        assert!(
            parse(json!(["/abs/tool"]))
                .unwrap_err()
                .contains("must not start with \"/\"")
        );
        assert!(
            parse(json!(["~/tool"]))
                .unwrap_err()
                .contains("must not start with \"~\"")
        );
        assert!(
            parse(json!(["C:/tool"]))
                .unwrap_err()
                .contains("must not be an absolute path")
        );
        assert!(parse(json!(["ftp://e.com/a"])).unwrap_err().contains("ftp"));
        assert!(parse(json!(["https:/e.com/a"])).unwrap_err().contains("//"));
    }

    // ---------- $deps.base ----------

    #[test]
    fn base_is_normalized_as_a_directory() {
        let decl = parse_with(json!("https://e.com/dist"), json!([])).unwrap();
        assert_eq!(
            decl.base.as_ref().map(Base::as_str),
            Some("https://e.com/dist/")
        );

        let decl = parse_with(json!("https://e.com/a/../b/"), json!([])).unwrap();
        assert_eq!(
            decl.base.as_ref().map(Base::as_str),
            Some("https://e.com/b/")
        );
    }

    #[test]
    fn relative_base_is_rejected_on_disk() {
        // `C:/x` 只在 Unix 上落进这里（在 Windows 上它本来就是绝对路径），
        // 单独由 `base_rejects_a_foreign_drive_path` 管
        for bad in [
            "",
            ".",
            "..",
            "sub/",
            "~/x",
            "//server/share",
            "ftp://e.com/x/",
        ] {
            let err = parse_with(json!(bad), json!([])).unwrap_err();
            assert!(err.contains("$deps.base"), "{bad}: {err}");
        }
    }

    #[test]
    fn base_accepts_an_absolute_directory_path() {
        let dir = tmp_dir();
        let text = path_to_slash(&dir);
        let decl = parse_with(json!(text), json!([])).unwrap();

        assert_eq!(
            decl.base.as_ref().map(Base::as_str),
            Some(format!("{}/", text.trim_end_matches('/')).as_str())
        );
    }

    #[test]
    fn base_rejects_a_foreign_drive_path() {
        if cfg!(windows) {
            return;
        }
        let err = parse_with(json!("C:/tool/"), json!([])).unwrap_err();
        assert!(err.contains("Windows path"), "{err}");
    }

    #[test]
    fn a_rooted_base_needs_a_drive_on_windows() {
        let doc = tmp_dir().join("x.byk.json");

        // Unix 上 `/srv/` 就是绝对路径，读取与 clone 都照收
        if !cfg!(windows) {
            assert_eq!(
                parse_with(json!("/srv/assets/"), json!([]))
                    .unwrap()
                    .base
                    .as_ref()
                    .map(Base::as_str),
                Some("/srv/assets/")
            );
            assert_eq!(
                freeze_local_base(Some(&json!("/srv/assets")), &doc).unwrap(),
                "/srv/assets/"
            );
            return;
        }

        // Windows 上 `/sub` 没有盘符，挂在「当前盘」上：同一份配置换个工作
        // 目录就指向别处，所以读取期与 clone 期都必须失败，不能悄悄拼成 C:/sub
        let err = parse_with(json!("/srv/assets/"), json!([])).unwrap_err();
        assert!(err.contains("has no drive letter"), "{err}");
        let err = freeze_local_base(Some(&json!("/srv/assets/")), &doc).unwrap_err();
        assert!(err.contains("has no drive letter"), "{err}");
    }

    #[test]
    fn local_base_keeps_its_path_shape() {
        let dir = tmp_dir();
        let text = path_to_slash(&dir);
        let decl = parse_with(json!(format!("{text}//")), json!([])).unwrap();

        match decl.base {
            Some(Base::Dir(path)) => assert_eq!(path, format!("{text}/")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn base_must_be_a_string_without_query() {
        assert!(
            parse_with(json!(1), json!([]))
                .unwrap_err()
                .contains("must be a string")
        );
        assert!(
            parse_with(json!("https://e.com/x/?t=1"), json!([]))
                .unwrap_err()
                .contains("query or fragment")
        );
        assert!(
            parse_with(json!("https://e.com\\x"), json!([]))
                .unwrap_err()
                .contains("path separator")
        );
    }

    #[test]
    fn unexpanded_variable_in_base_is_named() {
        // 变量没定义时展开器原样保留 `${...}`，base 停在半成品形态上：
        // 报错要点出名字，否则作者只会看到"不是绝对 URL"，想不到是变量名错了
        let err = parse_with(json!("${NOPE}"), json!([])).unwrap_err();
        assert!(err.contains("\"${NOPE}\" was not expanded"), "{err}");
        assert!(err.contains("resolve to an absolute URL"), "{err}");

        // 相对段 + 变量：两个问题同时存在，先点名变量
        let err = parse_with(json!("../${SUB}/"), json!([])).unwrap_err();
        assert!(err.contains("\"${SUB}\" was not expanded"), "{err}");

        // 纯相对值不该扯上变量
        let err = parse_with(json!("../x/"), json!([])).unwrap_err();
        assert!(!err.contains("was not expanded"), "{err}");
        assert!(err.contains("only resolves while cloning"), "{err}");
    }

    #[test]
    fn relative_src_needs_a_base_only_at_plan_time() {
        let decl = parse(json!(["../hhh.js"])).unwrap();
        let err = plan_entries(&decl, &tmp_dir(), "@@t").unwrap_err();
        assert!(
            err.contains("relative path but \"$deps.base\" is not set"),
            "{err}"
        );
    }

    #[test]
    fn relative_src_joins_base_and_climbs() {
        let dir = tmp_dir();
        let decl = parse_with(
            json!("https://e.com/sdsd/asfdsg/qwe/we/"),
            json!(["../hhh.js", "bin/tool", {"src": "./cfg.json", "to": "etc"}]),
        )
        .unwrap();

        let assets = plan_entries(&decl, &dir, "@@t").unwrap();

        assert_eq!(
            assets[0].origin.as_str(),
            "https://e.com/sdsd/asfdsg/qwe/hhh.js"
        );
        assert_eq!(assets[0].dest, dir.join("hhh.js"));
        assert_eq!(
            assets[1].origin.as_str(),
            "https://e.com/sdsd/asfdsg/qwe/we/bin/tool"
        );
        assert_eq!(
            assets[2].origin.as_str(),
            "https://e.com/sdsd/asfdsg/qwe/we/cfg.json"
        );
        assert_eq!(assets[2].dest, dir.join("etc").join("cfg.json"));
    }

    #[test]
    fn relative_src_keeps_its_query_when_joined() {
        let decl = parse_with(json!("https://e.com/a/"), json!(["tool?token=x"])).unwrap();
        let assets = plan_entries(&decl, &tmp_dir(), "@@t").unwrap();
        assert_eq!(assets[0].origin.as_str(), "https://e.com/a/tool?token=x");
        assert_eq!(assets[0].label, "@@t/tool");
    }

    #[test]
    fn relative_src_joins_a_local_base_as_a_path() {
        let dir = tmp_dir();
        let base = path_to_slash(&dir);
        let decl = parse_with(json!(base), json!(["../hhh.js", "bin/tool"])).unwrap();

        let assets = plan_entries(&decl, &dir, "@@t").unwrap();

        assert_eq!(
            assets[0].origin,
            Origin::File(dir.parent().unwrap().join("hhh.js"))
        );
        assert_eq!(assets[0].label, "@@t/hhh.js");
        assert_eq!(assets[1].origin, Origin::File(dir.join("bin").join("tool")));
    }

    #[test]
    fn absolute_src_ignores_a_local_base() {
        let dir = tmp_dir();
        let decl = parse_with(json!(path_to_slash(&dir)), json!(["https://e.com/x.js"])).unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();

        assert_eq!(
            assets[0].origin,
            Origin::Url("https://e.com/x.js".to_string())
        );
    }

    #[test]
    fn join_path_climbs_and_stops_at_the_root() {
        assert_eq!(join_path("/a/b/", "../x"), PathBuf::from("/a/x"));
        assert_eq!(join_path("/a/b/", "./x/./y"), PathBuf::from("/a/b/x/y"));
        assert_eq!(join_path("/a/b/", "../../../../x"), PathBuf::from("/x"));
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
        let decl = parse(json!(["https://e.com/dist/my-tool-1.4.tar.gz?token=x"])).unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();
        assert_eq!(assets[0].dest, dir.join("my-tool-1.4.tar.gz"));
        assert_eq!(assets[0].label, "@@t/my-tool-1.4.tar.gz");
        assert_eq!(
            assets[0].origin.as_str(),
            "https://e.com/dist/my-tool-1.4.tar.gz?token=x"
        );
    }

    #[test]
    fn plan_normalizes_dot_segments_in_url() {
        let dir = tmp_dir();
        let decl = parse(json!(["https://e.com/dist/../rel/tool"])).unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();
        assert_eq!(assets[0].origin.as_str(), "https://e.com/rel/tool");
    }

    #[test]
    fn plan_derive_failure_mentions_name() {
        let decl = parse(json!(["https://e.com/"])).unwrap();
        let err = plan_entries(&decl, &tmp_dir(), "@@t").unwrap_err();
        assert!(err.contains("set \"name\""), "{err}");
    }

    #[test]
    fn plan_extract_dest_is_the_directory() {
        let dir = tmp_dir();
        let decl = parse(json!([{"src": "https://e.com/b.tar.gz", "to": "dist", "extract": true}]))
            .unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();
        assert_eq!(assets[0].dest, dir.join("dist"));
        assert!(assets[0].archive);
        // 标签指向目录，而不是那个只用来中转的压缩包名
        assert_eq!(assets[0].label, "@@t/dist");
    }

    #[test]
    fn plan_extract_into_root_is_labelled_by_key() {
        let decl = parse(json!([{"src": "https://e.com/b.tar.gz", "extract": true}])).unwrap();
        let assets = plan_entries(&decl, &tmp_dir(), "@@t").unwrap();
        assert_eq!(assets[0].label, "@@t");
    }

    #[test]
    fn root_extract_must_be_the_only_entry() {
        let dir = tmp_dir();
        let decl = parse(json!([
            {"src": "https://e.com/b.tar.gz", "extract": true},
            {"src": "https://e.com/cfg.json"}
        ]))
        .unwrap();

        let err = plan_entries(&decl, &dir, "@@t").unwrap_err();

        assert!(err.contains("must be the only entry"), "{err}");

        // 单独一条就合法：lansend 那种发布包形态
        let single = parse(json!([{"src": "https://e.com/b.tar.gz", "extract": true}])).unwrap();
        plan_entries(&single, &dir, "@@t").unwrap();
    }

    #[test]
    fn plan_detects_clashes() {
        let decl =
            parse(json!(["https://e.com/a", {"src": "https://e.com/b", "name": "a"}])).unwrap();
        let err = plan_entries(&decl, &tmp_dir(), "@@t").unwrap_err();
        assert!(err.contains("already used by files[0]"), "{err}");
    }

    #[test]
    fn plan_allows_file_into_extracted_directory() {
        let decl = parse(json!([
            {"src": "https://e.com/b.tar.gz", "to": "dist", "extract": true},
            {"src": "https://e.com/README", "to": "dist", "name": "README"}
        ]))
        .unwrap();
        plan_entries(&decl, &tmp_dir(), "@@t").unwrap();
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
    fn join_url_treats_base_as_a_directory() {
        let base = "https://e.com/a/b/";
        assert_eq!(join_url(base, "tool"), "https://e.com/a/b/tool");
        assert_eq!(join_url(base, "../x.js"), "https://e.com/a/x.js");
        assert_eq!(join_url(base, "./x/./y"), "https://e.com/a/b/x/y");
        assert_eq!(join_url(base, ""), "https://e.com/a/b/");

        // 越界止于根，不会吐出畸形地址
        assert_eq!(join_url(base, "../../../../x"), "https://e.com/x");
        // 没有路径的 base 按根处理
        assert_eq!(join_url("https://e.com", "x"), "https://e.com/x");
    }

    #[test]
    fn dir_url_drops_the_document_name() {
        assert_eq!(
            dir_url("https://e.com/sdsd/asfdsg/qwe/we/aa.byk.json"),
            "https://e.com/sdsd/asfdsg/qwe/we/"
        );
        assert_eq!(dir_url("https://e.com/a/b/"), "https://e.com/a/b/");
        assert_eq!(dir_url("https://e.com"), "https://e.com/");
    }

    // ---------- freeze_remote_base ----------

    fn freeze(current: Value, doc: &str) -> Result<String, String> {
        freeze_remote_base(Some(&current), doc)
    }

    const DOC: &str = "https://hello.com/sdsd/asfdsg/qwe/we/aa.byk.json";

    #[test]
    fn freeze_fills_missing_base_with_the_document_directory() {
        assert_eq!(
            freeze_remote_base(None, DOC).unwrap(),
            "https://hello.com/sdsd/asfdsg/qwe/we/"
        );
    }

    #[test]
    fn freeze_reads_dot_as_the_document_directory() {
        let want = "https://hello.com/sdsd/asfdsg/qwe/we/";
        assert_eq!(freeze(json!("."), DOC).unwrap(), want);
        assert_eq!(freeze(json!("./"), DOC).unwrap(), want);
    }

    #[test]
    fn freeze_resolves_relative_forms_against_the_document() {
        assert_eq!(
            freeze(json!(".."), DOC).unwrap(),
            "https://hello.com/sdsd/asfdsg/qwe/"
        );
        assert_eq!(
            freeze(json!("sub/dir"), DOC).unwrap(),
            "https://hello.com/sdsd/asfdsg/qwe/we/sub/dir/"
        );
        // `..` 可以一路向上，止于主机根
        assert_eq!(
            freeze(json!("../../../../x"), DOC).unwrap(),
            "https://hello.com/x/"
        );
    }

    #[test]
    fn freeze_keeps_absolute_urls_and_canonicalizes_them() {
        assert_eq!(
            freeze(json!("https://cdn.example.com/x"), DOC).unwrap(),
            "https://cdn.example.com/x/"
        );
        assert_eq!(
            freeze(json!("https://cdn.example.com/a/../b/"), DOC).unwrap(),
            "https://cdn.example.com/b/"
        );
    }

    #[test]
    fn freeze_rejects_values_it_cannot_rewrite() {
        for bad in [
            json!(""),
            json!(1),
            json!("~/x"),
            json!("C:/x"),
            json!("ftp://e.com/x"),
            json!("https://e.com/x?t=1"),
            json!("a\\b"),
            json!("/"),
            json!("/sub"),
            json!("${CDN}"),
            json!("${CDN}/v1/"),
        ] {
            let err = freeze(bad.clone(), DOC).unwrap_err();
            assert!(err.contains("$deps.base"), "{bad}: {err}");
        }
    }

    #[test]
    fn deps_dir_mirrors_the_file_key_under_each_root() {
        let root = PathBuf::from("/home/u/.byk");
        let project = PathBuf::from("/work/app");

        assert_eq!(
            deps_dir_var("ops", true, &project, &root),
            "/home/u/.byk/deps/@@ops"
        );
        assert_eq!(
            deps_dir_var("ops", false, &project, &root),
            "/work/app/.byk/deps/@ops"
        );
        // 匿名文件：key 就是前缀本身
        assert_eq!(
            deps_dir("", false, &project, &root),
            PathBuf::from("/work/app/.byk/deps/@")
        );
        assert_eq!(
            deps_dir("", true, &project, &root),
            PathBuf::from("/home/u/.byk/deps/@@")
        );
    }
}
