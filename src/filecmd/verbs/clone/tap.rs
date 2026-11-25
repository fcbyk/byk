//! `clone --tap` 的 GitHub 索引路由：`byk.tap.json` 只做一层查找。
//!
//! 索引可以来自本地只读缓存，也可以来自仓库根目录。键到目标是一对一映射：
//! 值可以是相对路径、本地绝对路径或完整的 http(s) 地址；目标必须是别名文件，
//! 不能再是索引——所以这里只解析一次，绝不递归。路由结果交给本地或远程的
//! 普通来源加载，clone 的裁剪、base 冻结与 `-d` 都不需要知道 tap 的存在。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::filecmd::source::{self, Forge};
use crate::utils::path::{canonical, expand_home};

const INDEX_FILE: &str = "byk.tap.json";

/// 解析后的 tap 来源：`<user>/<repo>[@<ref>][#<key>]`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Spec {
    user: String,
    repo: String,
    reference: String,
    /// 写了 `@<ref>` 就跳过本地索引：缓存目录不区分 ref，本地内容未必对得上。
    explicit_ref: bool,
    pub(super) key: String,
}

impl Spec {
    /// 仓库根目录的索引地址；`HEAD` 与 `-gh` 的默认 ref 一致。
    pub(super) fn index_url(&self) -> String {
        Forge::Github.raw_url(
            &source::encode_segment(&self.user),
            &source::encode_segment(&self.repo),
            &source::encode_ref(&self.reference),
            INDEX_FILE,
        )
    }

    /// 本地只读索引的路径；带 `@<ref>` 时不使用本地缓存。
    pub(super) fn local_index_path(&self, root: &Path) -> Option<PathBuf> {
        if self.explicit_ref {
            return None;
        }
        Some(
            root.join("tap")
                .join(&self.user)
                .join(&self.repo)
                .join(INDEX_FILE),
        )
    }
}

fn parse_spec(spec: &str) -> Result<Spec, String> {
    // `#` 之后是 key，ref 才能含 `/`；没有 `#` 时保持 `<user>/<repo>[@<ref>]/<key>`。
    let (head, explicit_key) = match spec.split_once('#') {
        Some((head, key)) => (head, Some(key)),
        None => (spec, None),
    };

    let mut parts = head.splitn(if explicit_key.is_some() { 2 } else { 3 }, '/');
    let user = parts.next().unwrap_or("");
    let repo_ref = parts.next().unwrap_or("");
    // 键只能是最后一段；这里不按 `/` 再切，后面直接拒绝多段键。
    let old_key = parts.next().unwrap_or("");

    if source::looks_like_host(user) {
        return Err(host_error(user, &source::rest_after_host(spec)));
    }
    if user.is_empty() || user == "." || user == ".." || repo_ref.is_empty() {
        return Err(shape_error());
    }

    let (repo, reference, explicit_ref) = match repo_ref.split_once('@') {
        Some((repo, reference)) => {
            if repo.is_empty() || reference.is_empty() || repo == "." || repo == ".." {
                return Err(shape_error());
            }
            if reference == "."
                || reference == ".."
                || reference.contains('@')
                || source::bad_ref_path(reference)
            {
                return Err("Invalid GitHub ref".to_string());
            }
            (repo, reference, true)
        }
        None => (repo_ref, "HEAD", false),
    };
    // 有 `#` 时 repo 只占一段；含 `/` 说明 key 被写在了 `#` 左边。
    if repo.contains('/') {
        return Err(shape_error());
    }
    if !valid_user(user) {
        return Err(invalid_user(user));
    }
    if !valid_repo(repo) {
        return Err(invalid_repo(repo));
    }
    let key = explicit_key.unwrap_or(old_key);
    validate_key(key)?;

    Ok(Spec {
        user: user.to_string(),
        repo: repo.to_string(),
        reference: reference.to_string(),
        explicit_ref,
        key: key.to_string(),
    })
}

/// 解析 `--tap` 的两种写法：完整 spec，或只有一段的 key。
#[derive(Debug)]
pub(super) enum SourceSpec {
    Full(Spec),
    Key(String),
}

pub(super) fn parse_source(raw: &str) -> Result<SourceSpec, String> {
    if raw.contains('/') {
        parse_spec(raw).map(SourceSpec::Full)
    } else {
        validate_key(raw)?;
        Ok(SourceSpec::Key(raw.to_string()))
    }
}

/// GitHub 用户名：字母、数字、连字符，不能以连字符开头或结尾，最长 39。
fn valid_user(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=39).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

/// GitHub 仓库名：字母、数字、`.`、`-`、`_`，最长 100。
fn valid_repo(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=100).contains(&bytes.len())
        && value != "."
        && value != ".."
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn invalid_user(value: &str) -> String {
    format!(
        "Invalid GitHub tap user: {:?}\nHint: usernames use letters, digits, and hyphens, and cannot start or end with a hyphen",
        value
    )
}

fn invalid_repo(value: &str) -> String {
    format!(
        "Invalid GitHub tap repo: {:?}\nHint: repository names use letters, digits, `.`, `-`, and `_`, up to 100 characters",
        value
    )
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key == "." || key == ".." || key.contains('/') {
        return Err(shape_error());
    }
    if key.starts_with('$') {
        return Err(format!(
            "Tap key is reserved for metadata: {:?}\nHint: keys starting with `$` are ignored by the index",
            key
        ));
    }
    Ok(())
}

fn shape_error() -> String {
    "GitHub tap source must be <user>/<repo>[@<ref>][#<key>]\nHint: <key> is one path segment from byk.tap.json; put `#` before it when the ref contains `/`"
        .to_string()
}

fn host_error(host: &str, rest: &str) -> String {
    let mut message = format!(
        "GitHub tap source must not include a host: {}\nHint: write <user>/<repo>[@<ref>][#<key>] and leave the host out",
        host
    );
    // 去掉主机后还得是 user/repo/key 三段，建议出去才有意义。
    if rest.split('/').count() >= 3 {
        message.push_str(&format!("\nDid you mean `{}`?", rest));
    }
    message
}

/// 从索引里取出请求的键。`$` 开头的键是元数据，不参与查找。
///
/// `index` 既可能是本地路径也可能是 URL，只用于报错定位。
pub(super) fn select(data: &[u8], spec: &Spec, index: &str) -> Result<String, String> {
    let root: Value = serde_json::from_slice(data)
        .map_err(|e| format!("Failed to parse tap index {index}: {e}"))?;
    let Some(object) = root.as_object() else {
        return Err(format!("Tap index must be a JSON object: {index}"));
    };

    match object.get(&spec.key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.trim().to_string()),
        Some(Value::String(_)) => Err(format!("Tap entry {:?} is empty in {index}", spec.key)),
        Some(_) => Err(format!(
            "Tap entry {:?} must be a string in {index}",
            spec.key
        )),
        None => Err(missing_key(object, spec, index)),
    }
}

fn missing_key(object: &Map<String, Value>, spec: &Spec, index: &str) -> String {
    let mut keys: Vec<&str> = object
        .keys()
        .filter(|key| !key.starts_with('$'))
        .map(String::as_str)
        .collect();
    keys.sort_unstable();

    let mut message = format!("Tap key not found: {:?} in {index}", spec.key);
    if keys.is_empty() {
        message.push_str("\nThe index has no entries");
        return message;
    }

    const SHOWN: usize = 20;
    let shown = keys
        .iter()
        .take(SHOWN)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    message.push_str(&format!("\nAvailable: {shown}"));
    if keys.len() > SHOWN {
        message.push_str(&format!(", ... ({} more)", keys.len() - SHOWN));
    }
    message
}

/// 路由结果：本地文件按本地 clone 处理，URL 按远程 clone 处理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    Local(PathBuf),
    Remote(String),
}

/// 只读本地索引：只有文件真正不存在才返回 `None`。
///
/// 目录、无权限、坏符号链接都算「存在但不可用」，必须报错——本地索引一旦
/// 命中就是权威的，不能因为读不了就悄悄走远程。
pub(super) fn read_local_index(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "Failed to inspect local tap index {}: {e}",
            path.display()
        )),
        Ok(meta) => {
            if !meta.is_file() && !meta.file_type().is_symlink() {
                return Err(format!("Tap index is not a file: {}", path.display()));
            }
            fs::read(path)
                .map(Some)
                .map_err(|e| format!("Failed to read local tap index {}: {e}", path.display()))
        }
    }
}

/// 单 key 命中：仓库标识、索引路径与索引里的值。
#[derive(Debug)]
pub(super) struct Match {
    pub(super) repo: String,
    pub(super) index_path: PathBuf,
    pub(super) value: String,
}

/// 扫描结果。`searched` 是成功读取的索引数，`skipped` 是因错误跳过的索引数。
#[derive(Debug, Default)]
pub(super) struct SearchOutcome {
    pub(super) matches: Vec<Match>,
    pub(super) warnings: Vec<String>,
    pub(super) searched: usize,
    pub(super) skipped: usize,
    pub(super) tap_root_exists: bool,
}

/// 扫描所有本地 tap 仓库找 `key`。
///
/// 只认 `<root>/tap/<user>/<repo>/byk.tap.json` 两层结构；非法用户名 / 仓库名
/// 直接忽略，没有索引的目录跳过。坏索引只警告并跳过——缓存完全由用户维护，
/// 一个坏仓库不该让其余仓库不可用。
pub(super) fn search_local(root: &Path, key: &str) -> Result<SearchOutcome, String> {
    let tap_root = root.join("tap");
    let mut outcome = SearchOutcome::default();

    let users = match fs::read_dir(&tap_root) {
        Ok(entries) => {
            outcome.tap_root_exists = true;
            entries
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(outcome),
        Err(e) => {
            return Err(format!(
                "Failed to read local tap directory {}: {e}",
                tap_root.display()
            ));
        }
    };

    let mut user_paths = Vec::new();
    for entry in users {
        match entry {
            Ok(entry) => user_paths.push(entry.path()),
            Err(e) => outcome.warnings.push(format!(
                "Warning: failed to read an entry in {}: {e}",
                tap_root.display()
            )),
        }
    }
    user_paths.sort();

    for user_path in user_paths {
        let Some(user) = user_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !valid_user(user) {
            continue;
        }
        match fs::metadata(&user_path) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => continue,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("Warning: skipped local tap user {:?}: {e}", user));
                continue;
            }
        }

        let repos = match fs::read_dir(&user_path) {
            Ok(entries) => entries,
            Err(e) => {
                outcome
                    .warnings
                    .push(format!("Warning: skipped local tap user {:?}: {e}", user));
                continue;
            }
        };

        let mut repo_paths = Vec::new();
        for entry in repos {
            match entry {
                Ok(entry) => repo_paths.push(entry.path()),
                Err(e) => outcome.warnings.push(format!(
                    "Warning: failed to read an entry in {}: {e}",
                    user_path.display()
                )),
            }
        }
        repo_paths.sort();

        for repo_path in repo_paths {
            let Some(repo) = repo_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !valid_repo(repo) {
                continue;
            }
            match fs::metadata(&repo_path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => continue,
                Err(e) => {
                    outcome.warnings.push(format!(
                        "Warning: skipped local tap repository {:?}: {e}",
                        format!("{user}/{repo}")
                    ));
                    continue;
                }
            }

            let index_path = repo_path.join(INDEX_FILE);
            let data = match read_local_index(&index_path) {
                Ok(Some(data)) => data,
                Ok(None) => continue,
                Err(e) => {
                    outcome.skipped += 1;
                    outcome.warnings.push(format!(
                        "Warning: skipped local tap index {}/{}: {e}",
                        user, repo
                    ));
                    continue;
                }
            };

            match find_key(&data, key) {
                Ok(Some(value)) => {
                    outcome.searched += 1;
                    outcome.matches.push(Match {
                        repo: format!("{user}/{repo}"),
                        index_path: canonical(&index_path),
                        value,
                    });
                }
                Ok(None) => outcome.searched += 1,
                Err(e) => {
                    outcome.skipped += 1;
                    outcome.warnings.push(format!(
                        "Warning: skipped local tap index {}/{}: {e}",
                        user, repo
                    ));
                }
            }
        }
    }

    Ok(outcome)
}

/// 只取请求的 key，不关心其他条目；返回 `None` 表示索引里没有这个 key。
fn find_key(data: &[u8], key: &str) -> Result<Option<String>, String> {
    let root: Value = serde_json::from_slice(data).map_err(|e| format!("invalid JSON: {e}"))?;
    let Some(object) = root.as_object() else {
        return Err("index must be a JSON object".to_string());
    };
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => {
            Ok(Some(value.trim().to_string()))
        }
        Some(Value::String(_)) => Err(format!("tap entry {:?} is empty", key)),
        Some(_) => Err(format!("tap entry {:?} must be a string", key)),
    }
}

/// 本地索引里的值：相对路径按索引目录拼，绝对路径 / `~` 原样展开，URL 交给远程。
///
/// 不做 `.byk.json` 补全或目录回退：索引是精确指针，写什么就是什么。
pub(super) fn local_target(index_path: &Path, value: &str) -> Result<Target, String> {
    let value = value.trim();

    if let Some((scheme, _)) = value.split_once("://") {
        if scheme != "http" && scheme != "https" {
            return Err(unsupported_scheme_error(scheme));
        }
        ensure_not_index(value)?;
        return Ok(Target::Remote(value.to_string()));
    }

    let expanded = expand_home(value);
    let raw = if expanded.is_absolute() {
        expanded
    } else {
        index_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(expanded)
    };
    let path = canonical(&raw);
    if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE) {
        return Err(index_target_error(&path.display().to_string()));
    }
    Ok(Target::Local(path))
}

/// 把索引里的值变成最终要下载的 URL。
///
/// 相对值按仓库根拼；完整 URL 原样返回。目标最后一段是 `byk.tap.json` 时
/// 直接拒绝——索引只路由一跳，不允许套娃。
pub(super) fn target_url(spec: &Spec, value: &str) -> Result<String, String> {
    let value = value.trim();

    if let Some((scheme, _)) = value.split_once("://") {
        if scheme != "http" && scheme != "https" {
            return Err(unsupported_scheme_error(scheme));
        }
        ensure_not_index(value)?;
        return Ok(value.to_string());
    }

    let path = relative_path(value)?;
    let encoded = path
        .iter()
        .map(|segment| source::encode_segment(segment))
        .collect::<Vec<_>>()
        .join("/");
    let url = Forge::Github.raw_url(
        &source::encode_segment(&spec.user),
        &source::encode_segment(&spec.repo),
        &source::encode_segment(&spec.reference),
        &encoded,
    );
    ensure_not_index(&url)?;
    Ok(url)
}

fn relative_path(value: &str) -> Result<Vec<String>, String> {
    if value.starts_with('/') || value.starts_with('~') || looks_like_windows_drive(value) {
        return Err(local_path_error(value));
    }
    if value.contains('\\') {
        return Err(format!(
            "Invalid tap target {:?}: use `/` as the path separator",
            value
        ));
    }

    let mut segments = Vec::new();
    for segment in value.split('/') {
        if segment.is_empty() || segment == ".." {
            return Err(format!(
                "Invalid tap target {:?}: path must stay inside the repository",
                value
            ));
        }
        if segment == "." {
            continue;
        }
        segments.push(segment.to_string());
    }
    if segments.is_empty() {
        return Err(format!("Invalid tap target {:?}: the path is empty", value));
    }
    Ok(segments)
}

fn looks_like_windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn local_path_error(value: &str) -> String {
    format!(
        "Invalid tap target {:?}: must be an http(s) URL or a path relative to the repository root",
        value
    )
}

fn ensure_not_index(url: &str) -> Result<(), String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("");
    if last == INDEX_FILE {
        return Err(index_target_error(url));
    }
    Ok(())
}

fn unsupported_scheme_error(scheme: &str) -> String {
    format!(
        "Unsupported tap target scheme: {}\nHint: only http and https URLs are supported",
        scheme
    )
}

fn index_target_error(target: &str) -> String {
    format!(
        "Tap target must be an alias file, not another tap index: {}\nHint: byk.tap.json only routes one level",
        target
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        parse_spec("aaa/bb@v1/build").unwrap()
    }

    #[test]
    fn parses_spec_with_and_without_ref() {
        let parsed = parse_spec("aaa/bb/build").unwrap();
        assert_eq!(parsed.user, "aaa");
        assert_eq!(parsed.repo, "bb");
        assert_eq!(parsed.reference, "HEAD");
        assert_eq!(parsed.key, "build");

        assert_eq!(
            parsed.index_url(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/byk.tap.json"
        );
        assert_eq!(
            spec().index_url(),
            "https://raw.githubusercontent.com/aaa/bb/v1/byk.tap.json"
        );
    }

    #[test]
    fn rejects_bad_shapes() {
        for bad in [
            "",
            "aaa",
            "aaa/",
            "aaa/bb",
            "aaa/bb/",
            "aaa//build",
            "aaa/bb/build/more",
            "aaa/bb/.",
            "aaa/bb/..",
            "aaa/bb@/build",
            "aaa/bb@@v1/build",
            ".byk.json",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_a_slashed_ref_behind_a_hash() {
        let parsed = parse_spec("aaa/bb@feature/x#build").unwrap();
        assert_eq!(parsed.reference, "feature/x");
        assert!(parsed.explicit_ref);
        assert_eq!(parsed.key, "build");
        assert_eq!(
            parsed.index_url(),
            "https://raw.githubusercontent.com/aaa/bb/feature/x/byk.tap.json"
        );

        let parsed = parse_spec("aaa/bb#build").unwrap();
        assert_eq!(parsed.reference, "HEAD");
        assert!(parsed.local_index_path(Path::new("/root")).is_some());
        assert_eq!(parsed.key, "build");
    }

    #[test]
    fn rejects_a_key_left_of_the_hash() {
        for bad in [
            "aaa/bb/build#x",
            "aaa/bb@v1/#build",
            "aaa/bb@feature//x#build",
            "aaa/bb#",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_reserved_metadata_keys() {
        let err = parse_spec("aaa/bb/$schema").unwrap_err();
        assert!(err.contains("reserved for metadata"), "{err}");
    }

    #[test]
    fn rejects_a_host_prefix() {
        let err = parse_spec("github.com/aaa/bb/build").unwrap_err();
        assert!(err.contains("must not include a host"), "{err}");
        assert!(err.contains("Did you mean `aaa/bb/build`?"), "{err}");

        // 去掉主机后不足三段，别建议一个同样非法的写法
        let err = parse_spec("github.com/aaa/bb").unwrap_err();
        assert!(err.contains("must not include a host"), "{err}");
        assert!(!err.contains("Did you mean"), "{err}");
    }

    #[test]
    fn selects_the_requested_entry() {
        let index = br#"{
            "build": "aliases/build.byk.json",
            "$schema": "https://example.com/tap.schema.json"
        }"#;
        assert_eq!(
            select(index, &spec(), "idx").unwrap(),
            "aliases/build.byk.json"
        );
    }

    #[test]
    fn missing_key_lists_sorted_entries_without_metadata() {
        let spec = parse_spec("aaa/bb/nope").unwrap();
        let err = select(br#"{"z": "1", "a": "2", "$x": "3"}"#, &spec, "idx").unwrap_err();

        assert!(err.contains("Tap key not found: \"nope\""), "{err}");
        assert!(err.contains("Available: a, z"), "{err}");
        assert!(!err.contains("$x"), "{err}");
    }

    #[test]
    fn missing_key_in_an_empty_index_says_so() {
        let spec = parse_spec("aaa/bb/nope").unwrap();
        let err = select(b"{}", &spec, "idx").unwrap_err();
        assert!(err.contains("The index has no entries"), "{err}");
    }

    #[test]
    fn rejects_values_that_are_not_usable_strings() {
        let err = select(br#"{"build": 1}"#, &spec(), "idx").unwrap_err();
        assert!(err.contains("must be a string"), "{err}");

        let err = select(br#"{"build": "  "}"#, &spec(), "idx").unwrap_err();
        assert!(err.contains("is empty"), "{err}");
    }

    #[test]
    fn rejects_invalid_index_documents() {
        let err = select(b"{not json", &spec(), "idx").unwrap_err();
        assert!(err.starts_with("Failed to parse tap index idx:"), "{err}");

        let err = select(b"[]", &spec(), "idx").unwrap_err();
        assert!(err.contains("must be a JSON object"), "{err}");
    }

    #[test]
    fn resolves_relative_targets_from_the_repository_root() {
        let spec = spec();
        let want = "https://raw.githubusercontent.com/aaa/bb/v1/aliases/build.byk.json";

        assert_eq!(target_url(&spec, "aliases/build.byk.json").unwrap(), want);
        assert_eq!(target_url(&spec, "./aliases/build.byk.json").unwrap(), want);
        assert_eq!(
            target_url(&spec, "dir name/中文.byk.json").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/v1/dir%20name/%E4%B8%AD%E6%96%87.byk.json"
        );
    }

    #[test]
    fn keeps_absolute_urls_untouched() {
        assert_eq!(
            target_url(&spec(), "https://example.com/x/tool.byk.json").unwrap(),
            "https://example.com/x/tool.byk.json"
        );
    }

    #[test]
    fn rejects_local_paths_and_escaping_segments() {
        for bad in [
            "/abs/x.byk.json",
            "~/x.byk.json",
            "C:/x.byk.json",
            "../x.byk.json",
            "a/../x.byk.json",
            "a//x.byk.json",
            "a/",
            ".",
            "a\\b.byk.json",
        ] {
            assert!(target_url(&spec(), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_unsupported_url_schemes() {
        let err = target_url(&spec(), "ftp://example.com/x.byk.json").unwrap_err();
        assert!(err.contains("Unsupported tap target scheme: ftp"), "{err}");
    }

    #[test]
    fn refuses_a_target_that_is_another_index() {
        for bad in [
            "byk.tap.json",
            "nested/byk.tap.json",
            "https://example.com/byk.tap.json",
            "https://example.com/byk.tap.json?x=1",
        ] {
            let err = target_url(&spec(), bad).unwrap_err();
            assert!(err.contains("not another tap index"), "{bad}: {err}");
        }
    }

    // ---------- 本地索引 ----------

    #[test]
    fn local_index_is_only_used_without_an_explicit_ref() {
        let root = Path::new("/root");
        let plain = parse_spec("aaa/bb/build").unwrap();
        assert!(!plain.explicit_ref);
        assert_eq!(
            plain.local_index_path(root).unwrap(),
            PathBuf::from("/root/tap/aaa/bb/byk.tap.json")
        );

        let pinned = parse_spec("aaa/bb@v1/build").unwrap();
        assert!(pinned.explicit_ref);
        assert!(pinned.local_index_path(root).is_none());
    }

    #[test]
    fn rejects_components_that_cannot_be_cache_paths() {
        for bad in [
            r"aaa/b\b/build",
            "aaa/b b/build",
            "C:/bb/build",
            "aaa/bb!/build",
        ] {
            let err = parse_spec(bad).unwrap_err();
            assert!(err.contains("Invalid GitHub tap"), "{bad}: {err}");
        }
    }

    #[test]
    fn reads_a_local_index_when_present() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(INDEX_FILE);
        let body = br#"{"build": "aliases/build.byk.json"}"#;
        fs::write(&path, body).unwrap();

        assert_eq!(read_local_index(&path).unwrap().unwrap(), body);
    }

    #[test]
    fn reports_a_missing_local_index() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            read_local_index(&temp.path().join(INDEX_FILE))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn refuses_a_local_index_that_is_a_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(INDEX_FILE);
        fs::create_dir(&path).unwrap();

        let err = read_local_index(&path).unwrap_err();
        assert!(err.contains("not a file"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_broken_local_index_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(INDEX_FILE);
        symlink(temp.path().join("missing"), &path).unwrap();

        let err = read_local_index(&path).unwrap_err();
        assert!(err.contains("Failed to read local tap index"), "{err}");
    }

    #[test]
    fn resolves_local_targets_without_path_sugar() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("aliases")).unwrap();
        let index = repo.join(INDEX_FILE);
        fs::write(&index, "{}").unwrap();
        let target = repo.join("aliases/build.byk.json");
        fs::write(&target, "{}").unwrap();
        let wanted = canonical(&target);

        assert_eq!(
            local_target(&index, "aliases/build.byk.json").unwrap(),
            Target::Local(wanted.clone())
        );
        assert_eq!(
            local_target(&index, "./aliases/build.byk.json").unwrap(),
            Target::Local(wanted)
        );
        // 无路径糖：`build` 就是字面文件，不会去找 `build.byk.json`
        assert_eq!(
            local_target(&index, "build").unwrap(),
            Target::Local(canonical(&repo.join("build")))
        );
        // 目录也不会回退到目录里的 .byk.json
        let dir = repo.join("directory");
        fs::create_dir(&dir).unwrap();
        assert_eq!(
            local_target(&index, "directory").unwrap(),
            Target::Local(canonical(&dir))
        );
    }

    #[test]
    fn accepts_absolute_and_url_local_targets() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let index = repo.join(INDEX_FILE);
        fs::write(&index, "{}").unwrap();
        let absolute = temp.path().join("outside.byk.json");
        fs::write(&absolute, "{}").unwrap();

        assert_eq!(
            local_target(&index, absolute.to_str().unwrap()).unwrap(),
            Target::Local(canonical(&absolute))
        );
        assert_eq!(
            local_target(&index, "https://example.com/x.byk.json").unwrap(),
            Target::Remote("https://example.com/x.byk.json".to_string())
        );
    }

    #[test]
    fn rejects_bad_local_targets() {
        let index = Path::new("/cache/tap/aaa/bb/byk.tap.json");

        let err = local_target(index, "ftp://example.com/x.byk.json").unwrap_err();
        assert!(err.contains("Unsupported tap target scheme: ftp"), "{err}");

        for bad in [
            "byk.tap.json",
            "nested/byk.tap.json",
            "https://example.com/byk.tap.json",
            "https://example.com/byk.tap.json?x=1",
        ] {
            let err = local_target(index, bad).unwrap_err();
            assert!(err.contains("not another tap index"), "{bad}: {err}");
        }
    }

    // ---------- 单 key 搜索 ----------

    fn write_index(root: &Path, repo: &str, body: &str) -> PathBuf {
        let dir = root.join("tap").join(repo);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(INDEX_FILE);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn parses_full_and_single_key_sources() {
        assert!(matches!(
            parse_source("aaa/bb/build").unwrap(),
            SourceSpec::Full(_)
        ));
        match parse_source("build").unwrap() {
            SourceSpec::Key(key) => assert_eq!(key, "build"),
            other => panic!("expected a single key, got {other:?}"),
        }
        assert!(parse_source("aaa/bb").is_err());
        assert!(parse_source("").is_err());
        assert!(parse_source("$schema").is_err());
    }

    #[test]
    fn validates_github_components() {
        assert!(valid_user("aaa"));
        assert!(valid_user("a-b"));
        assert!(!valid_user("-aaa"));
        assert!(!valid_user("aaa-"));
        assert!(!valid_user("a.b"));
        assert!(!valid_user("a_b"));

        assert!(valid_repo("bb"));
        assert!(valid_repo(".github"));
        assert!(valid_repo("a_b.c-d"));
        assert!(!valid_repo("."));
        assert!(!valid_repo(".."));
        assert!(!valid_repo("a b"));
        assert!(!valid_repo("a/b"));
    }

    #[test]
    fn searches_all_local_indexes_for_a_unique_match() {
        let temp = tempfile::tempdir().unwrap();
        let index = write_index(
            temp.path(),
            "aaa/bb",
            r#"{"build": "aliases/build.byk.json"}"#,
        );
        write_index(temp.path(), "ccc/dd", r#"{"deploy": "deploy.byk.json"}"#);
        fs::create_dir_all(temp.path().join("tap/eee/ff")).unwrap();

        let outcome = search_local(temp.path(), "build").unwrap();

        assert!(outcome.tap_root_exists);
        assert!(outcome.warnings.is_empty());
        assert_eq!(outcome.searched, 2);
        assert_eq!(outcome.skipped, 0);
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].repo, "aaa/bb");
        assert_eq!(outcome.matches[0].index_path, canonical(&index));
        assert_eq!(outcome.matches[0].value, "aliases/build.byk.json");
    }

    #[test]
    fn reports_multiple_matches_in_sorted_order() {
        let temp = tempfile::tempdir().unwrap();
        write_index(temp.path(), "ccc/dd", r#"{"build": "c.byk.json"}"#);
        write_index(temp.path(), "aaa/bb", r#"{"build": "a.byk.json"}"#);

        let outcome = search_local(temp.path(), "build").unwrap();

        let repos: Vec<&str> = outcome
            .matches
            .iter()
            .map(|item| item.repo.as_str())
            .collect();
        assert_eq!(repos, ["aaa/bb", "ccc/dd"]);
    }

    #[test]
    fn warns_and_skips_bad_indexes() {
        let temp = tempfile::tempdir().unwrap();
        write_index(temp.path(), "aaa/bb", "{not json");
        write_index(temp.path(), "ccc/dd", r#"{"build": "ok.byk.json"}"#);

        let outcome = search_local(temp.path(), "build").unwrap();

        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].repo, "ccc/dd");
        assert_eq!(outcome.searched, 1);
        assert_eq!(outcome.skipped, 1);
        assert_eq!(outcome.warnings.len(), 1);
        assert!(
            outcome.warnings[0].starts_with("Warning: skipped local tap index aaa/bb:"),
            "{:?}",
            outcome.warnings[0]
        );
    }

    #[test]
    fn ignores_indexes_outside_repo_directories() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("tap/aaa");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join(INDEX_FILE), r#"{"build": "x.byk.json"}"#).unwrap();

        let outcome = search_local(temp.path(), "build").unwrap();

        assert!(outcome.tap_root_exists);
        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.searched, 0);
        assert_eq!(outcome.skipped, 0);
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn ignores_invalid_user_and_repo_directories() {
        let temp = tempfile::tempdir().unwrap();
        write_index(temp.path(), ".git/repo", r#"{"build": "x.byk.json"}"#);
        write_index(temp.path(), "aaa/b b", r#"{"build": "y.byk.json"}"#);

        let outcome = search_local(temp.path(), "build").unwrap();

        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.searched, 0);
        assert_eq!(outcome.skipped, 0);
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn reports_a_missing_tap_root() {
        let temp = tempfile::tempdir().unwrap();

        let outcome = search_local(temp.path(), "build").unwrap();

        assert!(!outcome.tap_root_exists);
        assert!(outcome.matches.is_empty());
        assert_eq!(outcome.searched, 0);
        assert_eq!(outcome.skipped, 0);
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn warns_and_skips_an_invalid_value_for_the_requested_key() {
        let temp = tempfile::tempdir().unwrap();
        write_index(temp.path(), "aaa/bb", r#"{"build": 1}"#);
        write_index(temp.path(), "ccc/dd", r#"{"build": "ok.byk.json"}"#);

        let outcome = search_local(temp.path(), "build").unwrap();

        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].repo, "ccc/dd");
        assert_eq!(outcome.searched, 1);
        assert_eq!(outcome.skipped, 1);
        assert!(
            outcome.warnings[0].contains("must be a string"),
            "{:?}",
            outcome.warnings[0]
        );
    }
}
