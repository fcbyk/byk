//! URL 与本地路径的词法处理，以及 clone 时 `$deps.base` 的冻结。

use std::path::{Path, PathBuf};

use serde_json::Value;

// ── URL 字面量：scheme 识别、点段归约、目录拼接 ─────────────────────────

/// `src` 要么是绝对 http(s) URL，要么是相对路径。
///
/// 相对形态只校验"不能是别处的东西"：绝对路径、盘符、`~` 都不行——它们
/// 指向文件系统，而这里最终只会变成一个 URL。
pub(super) fn check_src(index: usize, raw: &str) -> Result<(), String> {
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
pub(super) fn split_scheme(raw: &str) -> Option<(&str, &str)> {
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

pub(super) fn is_http(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

/// 形如 `C:` 的 Windows 盘符路径。
pub(super) fn is_drive_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// 归约 URL 路径里的 `.` / `..` 段（RFC 3986 5.2.4），只动路径。
pub(super) fn normalize_url(url: &str) -> String {
    let Some((head, path, tail)) = split_url(url) else {
        return url.to_string();
    };
    format!("{head}{}{tail}", remove_dot_segments(path))
}

/// URL 路径的末段，用于推导文件名。
pub(super) fn last_segment(url: &str) -> Option<&str> {
    let (_, path, _) = split_url(url)?;
    path.rsplit('/').next()
}

/// 绝对 URL → 目录 URL：归约点段并补上结尾的 `/`。
///
/// base 的定义就是目录，所以缺斜杠时补一个，而不是猜"最后一段是文件"。
/// 直接粘文档 URL 进来会拼出 `x.byk.json/bin/tool` 这种地址，错得很显眼，
/// 也比"有时当目录、有时当文件"好教。
pub(super) fn as_dir_url(url: &str) -> String {
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
pub(super) fn join_url(base: &str, rel: &str) -> String {
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
pub(super) fn path_to_slash(path: &Path) -> String {
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
pub(super) fn as_dir_path(text: &str) -> String {
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
pub(super) fn join_path(base: &str, rel: &str) -> PathBuf {
    normalize_path(&Path::new(base).join(rel))
}

// ── base 冻结：clone 期把相对值变成绝对地址 ─────────────────────────────

/// 带盘符、但这台机器用不了的本地路径：`C:/x` 在 Unix 上不是绝对路径，
/// `C:x` 在 Windows 上是「当前目录」的相对形式——两边都指望不上。
pub(super) fn foreign_path_error(text: &str) -> String {
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
pub(super) fn check_local_shape(text: &str) -> Result<(), String> {
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
/// 变量没定义时 `crate::core::config::vars::expand` 原样保留 `${name}`，
/// 于是 base 会停在一个半成品形态上；报错时点出名字，比说"不是 URL"有用。
pub(super) fn pending_var(text: &str) -> Option<&str> {
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
    use serde_json::{Value, json};
    use std::path::PathBuf;
    #[test]
    fn join_path_climbs_and_stops_at_the_root() {
        assert_eq!(join_path("/a/b/", "../x"), PathBuf::from("/a/x"));
        assert_eq!(join_path("/a/b/", "./x/./y"), PathBuf::from("/a/b/x/y"));
        assert_eq!(join_path("/a/b/", "../../../../x"), PathBuf::from("/x"));
    }

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
}
