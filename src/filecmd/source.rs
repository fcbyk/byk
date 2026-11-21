//! 别名文件来源解析：`@name` / `@@name` / 路径，link 与 clone 共用。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::deps::Origin;
use crate::deps::spec::gh_proxy_url;
use crate::filecmd::io::path::target_path;
use crate::filecmd::spec::invalid_stem_message;
use crate::model::PathLayout;
use crate::resolve::validate_filename;
use crate::utils::path::{canonical, expand_home};

/// 已读取并校验的来源内容。
#[derive(Debug)]
pub(crate) struct Loaded {
    pub(crate) label: String,
    pub(crate) data: Vec<u8>,
    /// 来源的绝对地址：远程是 URL，本地是规范化后的路径。
    ///
    /// `..clone` 靠它把 `$deps.base` 冻结成绝对地址——一旦落盘，这份文件
    /// 就再也没有自己的位置信息了。形态还决定冻结成 URL 还是本机路径。
    pub(crate) origin: Origin,
}

/// 解析链接目标：只接受本地文件；URL 明确拒绝并提示用 clone。
pub(crate) fn resolve(spec: &str, layout: &PathLayout) -> Result<PathBuf, String> {
    match classify(spec)? {
        SourceKind::Url(_) => Err(format!(
            "Remote source is not supported here: {}\nHint: use ..clone for URLs",
            spec
        )),
        SourceKind::Local => load_local(spec, layout).map(|(path, _)| path),
    }
}

/// 读取并校验来源；本地路径返回规范化路径，URL 通过外部 curl 获取。
pub(crate) fn load(spec: &str, layout: &PathLayout) -> Result<Loaded, String> {
    match classify(spec)? {
        SourceKind::Url(url) => {
            let data = fetch(url, FetchKind::Source)?;
            validate(&data, url)?;
            Ok(Loaded {
                label: url.to_string(),
                data,
                origin: Origin::Url(url.to_string()),
            })
        }
        SourceKind::Local => {
            let (path, data) = load_local(spec, layout)?;
            Ok(Loaded {
                label: path.display().to_string(),
                data,
                origin: Origin::File(path),
            })
        }
    }
}

/// 直接按路径读取别名文件。
///
/// 与 [`load`] 的区别是跳过 `@name` / `@@name` 与裸名补全：tap 索引里的值
/// 是精确指针，`@foo` 也必须当普通文件名，不能被解释成别名。
pub(crate) fn load_file(path: &Path) -> Result<Loaded, String> {
    let (path, data) = read_source(path.to_path_buf())?;
    Ok(Loaded {
        label: path.display().to_string(),
        data,
        origin: Origin::File(path),
    })
}

enum SourceKind<'a> {
    Local,
    Url(&'a str),
}

fn classify(spec: &str) -> Result<SourceKind<'_>, String> {
    if spec.is_empty() {
        return Err("Source is empty".to_string());
    }
    if let Some((scheme, _)) = spec.split_once("://") {
        return match scheme {
            "http" | "https" => Ok(SourceKind::Url(spec)),
            other => Err(format!(
                "Unsupported source scheme: {}\nHint: only http and https URLs are supported",
                other
            )),
        };
    }
    Ok(SourceKind::Local)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Forge {
    Github,
    Gitee,
}

impl Forge {
    fn name(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Gitee => "Gitee",
        }
    }

    pub(crate) fn raw_url(self, user: &str, repo: &str, reference: &str, path: &str) -> String {
        match self {
            Self::Github => format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                user, repo, reference, path
            ),
            Self::Gitee => format!(
                "https://gitee.com/{}/{}/raw/{}/{}",
                user, repo, reference, path
            ),
        }
    }
}

/// GitHub 文件的传输通道；`raw` 是平台自身，其余是第三方加速。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cdn {
    Raw,
    Jsdelivr,
    Ghproxy,
}

impl Cdn {
    pub(crate) const NAMES: [&'static str; 3] = ["raw", "jsdelivr", "ghproxy"];

    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "raw" => Ok(Self::Raw),
            "jsdelivr" => Ok(Self::Jsdelivr),
            "ghproxy" => Ok(Self::Ghproxy),
            other => Err(format!(
                "Unknown CDN: {}\nSupported: {}",
                other,
                Self::NAMES.join(", ")
            )),
        }
    }

    fn url(self, forge: Forge, user: &str, repo: &str, reference: &str, path: &str) -> String {
        match self {
            Self::Raw => forge.raw_url(user, repo, reference, path),
            Self::Jsdelivr => format!(
                "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                user, repo, reference, path
            ),
            // gh-proxy 接收完整的上游 URL，直接拼在域名后面
            Self::Ghproxy => gh_proxy_url(&forge.raw_url(user, repo, reference, path)),
        }
    }
}

/// 把 `<user>/<repo>[@<ref>][/<path>]` 展开为对应平台的 raw URL。
pub(crate) fn forge_url(forge: Forge, cdn: Cdn, spec: &str) -> Result<String, String> {
    let mut parts = spec.splitn(3, '/');
    let user = parts.next().unwrap_or("");
    let repo_ref = parts.next().unwrap_or("");
    // `<path>` 可省略：省了就是仓库根，与 `user/repo/` 等价。糖只有这一处——
    // `user/repo/ss` 仍然指 `ss.byk.json`，省略路径不会让第三段变成目录。
    let path = parts.next().unwrap_or("");

    // `github.com/user/repo` 是最容易手滑的形态：三段都能解析，会被当成
    // user=github.com / repo=user / path=repo 去 fetch，只能拿到 404。
    if looks_like_host(user) {
        return Err(host_error(forge, user, &rest_after_host(spec)));
    }
    if user.is_empty() || user == "." || user == ".." || repo_ref.is_empty() {
        return Err(shape_error(forge));
    }

    let (repo, reference) = match repo_ref.split_once('@') {
        Some((repo, reference)) => {
            if repo.is_empty() || reference.is_empty() || repo == "." || repo == ".." {
                return Err(shape_error(forge));
            }
            if reference == "." || reference == ".." {
                return Err(format!("Invalid {} ref", forge.name()));
            }
            (repo, reference)
        }
        None => (repo_ref, "HEAD"),
    };

    let (dirs, file) = split_forge_path(forge, path)?;

    let mut segments = Vec::with_capacity(dirs.len() + 4);
    segments.push(encode_segment(user));
    segments.push(encode_segment(repo));
    segments.push(encode_segment(reference));
    segments.extend(dirs.iter().map(|segment| encode_segment(segment)));
    segments.push(encode_segment(&file));

    Ok(cdn.url(
        forge,
        &segments[0],
        &segments[1],
        &segments[2],
        &segments[3..].join("/"),
    ))
}

fn shape_error(forge: Forge) -> String {
    format!(
        "{} source must be <user>/<repo>[@<ref>][/<path>]\nHint: omit <path> for the repository root, and end a directory path with `/`",
        forge.name()
    )
}

/// 首段长得像主机名就意味着用户把整条 URL 路径粘进来了。
///
/// GitHub 与 Gitee 的用户名都不含 `.`，所以带点即主机；`localhost` 单独判一次。
/// 以 `.` 开头的段（`.`、`..`、`.byk.json`）不算主机，交给形状检查报错。
pub(crate) fn looks_like_host(segment: &str) -> bool {
    !segment.starts_with('.')
        && (segment.contains('.') || segment.eq_ignore_ascii_case("localhost"))
}

/// 去掉首段主机名后剩下的部分，用来拼「你是不是想写……」的提示。
pub(crate) fn rest_after_host(spec: &str) -> String {
    let mut parts = spec.splitn(2, '/');
    parts.next();
    parts.next().unwrap_or("").to_string()
}

fn host_error(forge: Forge, host: &str, rest: &str) -> String {
    let mut message = format!(
        "{} source must not include a host: {}\nHint: write <user>/<repo>[@<ref>][/<path>] and leave the host out",
        forge.name(),
        host
    );
    // 至少还剩 user/repo 才值得建议；只剩一段时建议出去也还是非法形态。
    if rest.split('/').count() >= 2 {
        message.push_str(&format!("\nDid you mean `{}`?", rest));
    }
    message
}

fn split_forge_path(forge: Forge, path: &str) -> Result<(Vec<&str>, String), String> {
    let (dirs, file) = if path.is_empty() {
        (Vec::new(), ".byk.json".to_string())
    } else if let Some(dir) = path.strip_suffix('/') {
        let dirs = if dir.is_empty() {
            Vec::new()
        } else {
            dir.split('/').collect()
        };
        (dirs, ".byk.json".to_string())
    } else {
        let mut parts: Vec<&str> = path.split('/').collect();
        let last = parts.pop().unwrap_or("");
        let file = if last == ".byk.json" {
            ".byk.json".to_string()
        } else {
            let stem = last.strip_suffix(".byk.json").unwrap_or(last);
            if !validate_filename(stem) {
                return Err(invalid_stem_message(stem));
            }
            format!("{}.byk.json", stem)
        };
        (parts, file)
    };

    for segment in &dirs {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return Err(format!(
                "Invalid {} path segment: {:?}",
                forge.name(),
                segment
            ));
        }
    }
    Ok((dirs, file))
}

pub(crate) fn encode_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

fn load_local(spec: &str, layout: &PathLayout) -> Result<(PathBuf, Vec<u8>), String> {
    if let Some(stem) = spec.strip_prefix("@@") {
        if !validate_filename(stem) {
            return Err(invalid_stem_message(stem));
        }
        return read_source(target_path(&layout.alias_dir, stem));
    }
    if let Some(stem) = spec.strip_prefix('@') {
        if !validate_filename(stem) {
            return Err(invalid_stem_message(stem));
        }
        return read_source(target_path(&current_dir()?, stem));
    }

    let expanded = expand_home(spec);
    let raw = if expanded.is_relative() {
        current_dir()?.join(expanded)
    } else {
        expanded
    };
    read_source(select_raw_source(spec, raw)?)
}

fn read_source(path: PathBuf) -> Result<(PathBuf, Vec<u8>), String> {
    require_file(&path)?;
    let target = canonical(&path);
    let data = fs::read(&target)
        .map_err(|e| format!("Failed to read source {}: {}", target.display(), e))?;
    validate(&data, &target.display().to_string())?;
    Ok((target, data))
}

/// 裸路径的解析规则：字面优先，找不到再补 `.byk.json`，目录则读匿名文件。
///
/// - 目录形态（`.`、`..`、结尾 `/`）直接指向目录里的 `.byk.json`。
/// - 只有能构成合法别名文件名的裸名字才补后缀：`tool` 会试
///   `tool.byk.json`，而 `hello.txt`、带空格的路径保持字面——它们的
///   `.byk.json` 变体本来就不是合法的别名文件名。
/// - 裸名字先读字面文件，保证 `tool` 在 `tool.byk.json` 也存在时仍然
///   可达；字面缺失才回退 `tool.byk.json`。字面是目录时，再读该目录的
///   `.byk.json`。
fn select_raw_source(spec: &str, raw: PathBuf) -> Result<PathBuf, String> {
    let last = Path::new(spec)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if is_dot_path(spec) || ends_with_separator(spec) {
        let anonymous = raw.join(".byk.json");
        require_file(&anonymous)?;
        return Ok(anonymous);
    }

    if !validate_filename(last) {
        require_file(&raw)?;
        return Ok(raw);
    }

    let mut tried = Vec::with_capacity(3);

    let raw_is_dir = match probe(&raw)? {
        Some(meta) if meta.is_file() => return Ok(raw),
        Some(meta) if meta.is_dir() => true,
        Some(_) => return Err(format!("Source is not a file: {}", raw.display())),
        None => false,
    };
    tried.push(raw.clone());

    let suffixed = append_suffix(&raw, ".byk.json");
    match probe(&suffixed)? {
        Some(meta) if meta.is_file() => return Ok(suffixed),
        _ => tried.push(suffixed),
    }

    if raw_is_dir {
        let anonymous = raw.join(".byk.json");
        match probe(&anonymous)? {
            Some(meta) if meta.is_file() => return Ok(anonymous),
            _ => tried.push(anonymous),
        }
    }

    Err(source_not_found(&tried))
}

fn require_file(path: &Path) -> Result<(), String> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!("Source is not a file: {}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(format!("Source not found: {}", path.display()))
        }
        Err(e) => Err(format!("Failed to inspect {}: {}", path.display(), e)),
    }
}

fn probe(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Failed to inspect {}: {}", path.display(), e)),
    }
}

fn source_not_found(tried: &[PathBuf]) -> String {
    let mut message = format!("Source not found: {}", tried[0].display());
    if tried.len() > 1 {
        let rest: Vec<String> = tried[1..]
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        message.push_str(&format!("\nAlso tried: {}", rest.join(", ")));
    }
    message
}

fn is_dot_path(spec: &str) -> bool {
    let trimmed = spec.trim_end_matches(|c: char| c == '/' || (cfg!(windows) && c == '\\'));
    trimmed == "." || trimmed == ".."
}

fn ends_with_separator(spec: &str) -> bool {
    spec.ends_with('/') || (cfg!(windows) && spec.ends_with('\\'))
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut text = path.as_os_str().to_os_string();
    text.push(suffix);
    PathBuf::from(text)
}

fn validate(data: &[u8], label: &str) -> Result<(), String> {
    match serde_json::from_slice::<Value>(data) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(Value::Array(_)) => Err(format!("Source is itself a link: {}", label)),
        Ok(_) => Err(format!("Source must be a JSON object: {}", label)),
        Err(e) => Err(format!("Failed to parse source {}: {}", label, e)),
    }
}

/// 索引和普通来源共用同一次下载，但 404 的措辞与提示不同。
#[derive(Clone, Copy)]
enum FetchKind {
    Source,
    TapIndex,
}

/// 下载 `byk.tap.json`；404 时提示在仓库根目录补索引，而不是检查路径。
pub(crate) fn fetch_tap_index(url: &str) -> Result<Vec<u8>, String> {
    fetch(url, FetchKind::TapIndex)
}

fn fetch(url: &str, kind: FetchKind) -> Result<Vec<u8>, String> {
    let output = crate::net::curl()
        .args(["-fsSL", "--", url])
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                "Remote clone requires curl\nHint: install curl and retry".to_string()
            } else {
                format!("Failed to run curl: {}", e)
            }
        })?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.trim();
        // 22 是 `-f` 下的 HTTP 错误，远程来源几乎总是 404：仓库/ref/路径三者
        // 有一个不对。裸 `curl failed (exit 22)` 只会让人去查网络。
        if code == 22 {
            let mut message = match kind {
                FetchKind::Source => format!("Remote source not found: {}", url),
                FetchKind::TapIndex => format!("Tap index not found: {}", url),
            };
            if !reason.is_empty() {
                message.push_str(&format!("\nServer said: {}", reason));
            }
            message.push_str(match kind {
                FetchKind::Source => {
                    "\nHint: check the repository, ref, and path; a directory path must end with `/`"
                }
                FetchKind::TapIndex => {
                    "\nHint: add byk.tap.json at the repository root"
                }
            });
            return Err(message);
        }
        return Err(if reason.is_empty() {
            format!("curl failed (exit {}) for {}", code, url)
        } else {
            format!("curl failed (exit {}) for {}: {}", code, url, reason)
        });
    }

    Ok(output.stdout)
}

fn current_dir() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("Failed to resolve current directory: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join(".byk").join("alias"),
        }
    }

    fn object_file(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, r#"{"cmd": "echo hi"}"#).unwrap();
        path
    }

    fn spec(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn resolves_object_path() {
        let temp = tempfile::tempdir().unwrap();
        let target = object_file(temp.path(), "tool.byk.json");

        let resolved = resolve(&spec(&target), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn resolves_global_name() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let target = object_file(&layout.alias_dir, "tool.byk.json");

        let resolved = resolve("@@tool", &layout).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn resolves_bare_name_with_alias_suffix() {
        let temp = tempfile::tempdir().unwrap();
        let target = object_file(temp.path(), "tool.byk.json");

        let resolved = resolve(&spec(&temp.path().join("tool")), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn prefers_the_literal_file_over_its_alias_sibling() {
        let temp = tempfile::tempdir().unwrap();
        object_file(temp.path(), "tool.byk.json");
        let literal = temp.path().join("tool");
        fs::write(&literal, r#"{"cmd": "echo literal"}"#).unwrap();

        let resolved = resolve(&spec(&literal), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&literal));
    }

    #[test]
    fn explicit_alias_sibling_stays_reachable() {
        let temp = tempfile::tempdir().unwrap();
        let target = object_file(temp.path(), "tool.byk.json");
        fs::write(temp.path().join("tool"), r#"{"cmd": "echo literal"}"#).unwrap();

        let resolved = resolve(&spec(&target), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn falls_back_to_a_literal_file() {
        let temp = tempfile::tempdir().unwrap();
        let literal = temp.path().join("tool");
        fs::write(&literal, r#"{"cmd": "echo literal"}"#).unwrap();

        let resolved = resolve(&spec(&literal), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&literal));
    }

    #[test]
    fn keeps_other_extensions_literal() {
        let temp = tempfile::tempdir().unwrap();
        let literal = temp.path().join("config.json");
        fs::write(&literal, r#"{"cmd": "echo hi"}"#).unwrap();

        let resolved = resolve(&spec(&literal), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&literal));
    }

    #[test]
    fn never_appends_the_suffix_to_other_extensions() {
        let temp = tempfile::tempdir().unwrap();
        object_file(temp.path(), "hello.txt.byk.json");

        let err = resolve(&spec(&temp.path().join("hello.txt")), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Source not found:"), "{err}");
        assert!(!err.contains("hello.txt.byk.json"), "{err}");
    }

    #[test]
    fn keeps_hidden_names_literal() {
        let temp = tempfile::tempdir().unwrap();
        let literal = temp.path().join(".hidden");
        fs::write(&literal, r#"{"cmd": "echo hi"}"#).unwrap();

        let resolved = resolve(&spec(&literal), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&literal));
    }

    #[test]
    fn resolves_a_trailing_slash_to_the_anonymous_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("proj");
        let target = object_file(&dir, ".byk.json");
        let slashed = format!("{}/", dir.display());

        let resolved = resolve(&slashed, &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn resolves_dot_to_the_anonymous_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = object_file(temp.path(), ".byk.json");

        let resolved = select_raw_source(".", temp.path().to_path_buf()).unwrap();

        assert_eq!(resolved, temp.path().join(".byk.json"));
        assert_eq!(resolved, target);
    }

    #[test]
    fn resolves_a_directory_name_to_its_anonymous_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("proj");
        let target = object_file(&dir, ".byk.json");

        let resolved = resolve(&spec(&dir), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn prefers_the_suffixed_file_over_a_directory_anonymous_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("proj");
        object_file(&dir, ".byk.json");
        let target = object_file(temp.path(), "proj.byk.json");

        let resolved = resolve(&spec(&dir), &layout(&temp)).unwrap();

        assert_eq!(resolved, canonical(&target));
    }

    #[test]
    fn missing_bare_name_lists_every_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let bare = temp.path().join("tool");

        let err = resolve(&spec(&bare), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Source not found:"), "{err}");
        assert!(err.contains("tool.byk.json"), "{err}");
        assert!(err.contains("Also tried:"), "{err}");
        assert!(err.contains("tool"), "{err}");
    }

    #[test]
    fn explicit_byk_json_directory_is_not_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("dir.byk.json");
        fs::create_dir_all(&dir).unwrap();

        let err = resolve(&spec(&dir), &layout(&temp)).unwrap_err();

        assert_eq!(err, format!("Source is not a file: {}", dir.display()));
    }

    #[test]
    fn classifies_dot_paths_and_separators() {
        assert!(is_dot_path("."));
        assert!(is_dot_path("./"));
        assert!(is_dot_path(".."));
        assert!(is_dot_path("../"));
        assert!(!is_dot_path("..x"));
        assert!(ends_with_separator("a/b/"));
        assert!(!ends_with_separator("a/b"));
        // 只有能构成合法别名文件名的裸名字才补 `.byk.json`
        assert!(validate_filename("tool"));
        assert!(validate_filename("发布"));
        assert!(!validate_filename("tool.txt"));
        assert!(!validate_filename(".hidden"));
        assert!(!validate_filename("my tool"));
    }

    #[test]
    fn rejects_empty() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(resolve("", &layout(&temp)).unwrap_err(), "Source is empty");
    }

    #[test]
    fn rejects_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.byk.json");

        let err = resolve(&spec(&missing), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Source not found:"));
    }

    #[test]
    fn rejects_directory() {
        let temp = tempfile::tempdir().unwrap();

        let err = resolve(&spec(temp.path()), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Source is not a file:"));
    }

    #[test]
    fn rejects_invalid_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bad.byk.json");
        fs::write(&path, "{not json").unwrap();

        let err = resolve(&spec(&path), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Failed to parse source"));
    }

    #[test]
    fn rejects_link_source() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("chain.byk.json");
        fs::write(&path, r#"["other.byk.json"]"#).unwrap();

        let err = resolve(&spec(&path), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Source is itself a link:"));
    }

    #[test]
    fn rejects_non_object() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("scalar.byk.json");
        fs::write(&path, "42").unwrap();

        let err = resolve(&spec(&path), &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Source must be a JSON object:"));
    }

    #[test]
    fn rejects_url_for_link_targets() {
        let temp = tempfile::tempdir().unwrap();

        let err = resolve("https://example.com/tool.byk.json", &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Remote source is not supported here:"));
        assert!(err.contains("use ..clone for URLs"));
    }

    #[test]
    fn rejects_unsupported_scheme() {
        let temp = tempfile::tempdir().unwrap();

        let err = load("ftp://example.com/tool.byk.json", &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Unsupported source scheme:"));
        assert!(err.contains("only http and https"));
    }

    #[test]
    fn forge_url_maps_shorthand() {
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb@v1").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/v1/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb/sss").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb/").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb/sss/xxx").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss/xxx.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb@v1/sss").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/v1/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb@abc123/").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/abc123/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb/sss/xxx.byk.json").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss/xxx.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aaa/bb/sss/").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss/.byk.json"
        );
    }

    #[test]
    fn forge_url_maps_gitee() {
        assert_eq!(
            forge_url(Forge::Gitee, Cdn::Raw, "aaa/bb").unwrap(),
            "https://gitee.com/aaa/bb/raw/HEAD/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Gitee, Cdn::Raw, "aaa/bb/sss").unwrap(),
            "https://gitee.com/aaa/bb/raw/HEAD/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Gitee, Cdn::Raw, "aaa/bb@v1/").unwrap(),
            "https://gitee.com/aaa/bb/raw/v1/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Gitee, Cdn::Raw, "aaa/bb/dir/xxx").unwrap(),
            "https://gitee.com/aaa/bb/raw/HEAD/dir/xxx.byk.json"
        );
    }

    #[test]
    fn forge_url_maps_jsdelivr() {
        assert_eq!(
            forge_url(Forge::Github, Cdn::Jsdelivr, "aaa/bb").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Jsdelivr, "aaa/bb/sss").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Jsdelivr, "aaa/bb@v1/").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@v1/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Jsdelivr, "aaa/bb@abc123/dir/xxx").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@abc123/dir/xxx.byk.json"
        );
    }

    #[test]
    fn forge_url_maps_ghproxy() {
        assert_eq!(
            forge_url(Forge::Github, Cdn::Ghproxy, "aaa/bb").unwrap(),
            "https://gh-proxy.com/https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, Cdn::Ghproxy, "aaa/bb@v1/sss").unwrap(),
            "https://gh-proxy.com/https://raw.githubusercontent.com/aaa/bb/v1/sss.byk.json"
        );
    }

    #[test]
    fn cdn_parses_known_names() {
        assert_eq!(Cdn::parse("raw").unwrap(), Cdn::Raw);
        assert_eq!(Cdn::parse("jsdelivr").unwrap(), Cdn::Jsdelivr);
        assert_eq!(Cdn::parse("ghproxy").unwrap(), Cdn::Ghproxy);

        let err = Cdn::parse("fastgit").unwrap_err();
        assert!(err.starts_with("Unknown CDN: fastgit"), "{err}");
        assert!(err.contains("raw, jsdelivr, ghproxy"), "{err}");
    }

    #[test]
    fn forge_url_encodes_segments() {
        assert_eq!(
            forge_url(Forge::Github, Cdn::Raw, "aa a/bb@v 1/dir name/xy").unwrap(),
            "https://raw.githubusercontent.com/aa%20a/bb/v%201/dir%20name/xy.byk.json"
        );
    }

    #[test]
    fn forge_url_rejects_bad_shape() {
        // 两段现在是合法的（仓库根），剩下的都是真的形状错误
        for spec in ["", ".byk.json", "aaa", "aaa/bb@/sss", "aaa/bb/../sss"] {
            assert!(forge_url(Forge::Github, Cdn::Raw, spec).is_err(), "{spec}");
        }
        assert!(forge_url(Forge::Github, Cdn::Raw, "aaa//sss").is_err());
        assert!(forge_url(Forge::Github, Cdn::Raw, "aaa/bb/sss//xxx").is_err());
    }

    #[test]
    fn forge_url_rejects_a_host_prefix() {
        let err = forge_url(Forge::Github, Cdn::Raw, "github.com/aaa/bb").unwrap_err();
        assert!(err.contains("must not include a host"), "{err}");
        assert!(err.contains("Did you mean `aaa/bb`?"), "{err}");

        let err = forge_url(Forge::Github, Cdn::Raw, "www.github.com/aaa/bb/sss").unwrap_err();
        assert!(err.contains("Did you mean `aaa/bb/sss`?"), "{err}");

        // 去掉主机后不到两段，就只能报形状错，别建议一个同样非法的写法
        let err = forge_url(Forge::Github, Cdn::Raw, "github.com/aaa").unwrap_err();
        assert!(err.contains("must not include a host"), "{err}");
        assert!(!err.contains("Did you mean"), "{err}");
    }

    #[test]
    fn rejects_invalid_name() {
        let temp = tempfile::tempdir().unwrap();

        let err = resolve("@@a/b", &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Invalid file name:"));
    }
}
