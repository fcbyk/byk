//! `$deps` JSON 声明的语法校验。

use std::path::Path;

use serde_json::Value;

use super::path::{
    as_dir_path, as_dir_url, check_local_shape, check_src, foreign_path_error, is_drive_path,
    is_http, pending_var, split_scheme,
};
use crate::deps::types::{Base, Declaration, DepEntry};

/// `$deps` 顶层允许的字段；报错时按这个顺序列出。
const DEP_FIELDS: [&str; 4] = ["files", "base", "pip", "npm"];

/// `$deps.files` 条目允许的字段；报错时按这个顺序列出。
const ENTRY_FIELDS: [&str; 5] = ["src", "to", "name", "extract", "exec"];

/// Windows 保留设备名；作为落盘文件名的主名（首段）时拒绝。
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

// ── 语法层：`$deps` 结构校验 ────────────────────────────────────────────

/// 解析并校验 `$deps` 字段的值；返回声明。
///
/// 只回答"长得对不对"：类型、必填、互斥、未知字段、URL scheme。落点
/// 拼接、文件名推导与冲突检测留给 `plan_entries`。
pub(crate) fn parse_deps(value: &Value) -> Result<Declaration, String> {
    let Some(obj) = value.as_object() else {
        return Err("$deps must be an object".to_string());
    };

    for key in obj.keys() {
        if !DEP_FIELDS.contains(&key.as_str()) {
            return Err(format!(
                "$deps has unknown field \"{key}\"; expected \"files\", \"base\", \"pip\" or \"npm\""
            ));
        }
    }

    let base = match obj.get("base") {
        None => None,
        Some(raw) => Some(parse_base(raw)?),
    };

    let pip = match obj.get("pip") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                Value::String(text) if !text.trim().is_empty() => Ok(text.clone()),
                Value::String(_) => Err(format!("\"$deps.pip[{index}]\" must not be empty")),
                _ => Err(format!("\"$deps.pip[{index}]\" must be a string")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("$deps.pip must be an array of strings".to_string()),
    };

    let npm = match obj.get("npm") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                Value::String(text) if !text.trim().is_empty() => Ok(text.clone()),
                Value::String(_) => Err(format!("\"$deps.npm[{index}]\" must not be empty")),
                _ => Err(format!("\"$deps.npm[{index}]\" must be a string")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("$deps.npm must be an array of strings".to_string()),
    };

    let files = match obj.get("files") {
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| parse_entry(index, item))
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("$deps.files must be an array".to_string()),
        // files 只在声明了 pip 或 npm 时可缺省；否则 $deps 是个空声明，没有意义
        None if pip.is_empty() && npm.is_empty() => {
            return Err("$deps must name a \"files\", \"pip\" or \"npm\" array".to_string());
        }
        None => Vec::new(),
    };

    Ok(Declaration {
        base,
        files,
        pip,
        npm,
    })
}

/// `base`：目录，落盘态只认绝对地址——http(s) URL，或本机绝对路径。
///
/// 拒绝相对值：解析相对 base 需要文档自己的位置，而那份信息只在 clone 那一
/// 刻存在（见 `freeze_remote_base` / `freeze_local_base`）。放进来就会
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
pub(super) fn validate_asset_name(
    index: usize,
    field: &'static str,
    raw: &str,
) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::spec::freeze_local_base;
    use crate::deps::types::path_to_slash;
    use serde_json::{Value, json};
    use std::path::PathBuf;

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

    fn tmp_dir() -> PathBuf {
        tempfile::tempdir().unwrap().path().join("deps").join("@@t")
    }

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
            "$deps must name a \"files\", \"pip\" or \"npm\" array"
        );
    }

    #[test]
    fn pip_alone_parses_string_array() {
        let decl = parse_deps(&json!({"pip": ["requests", "click>=8.0"]})).unwrap();
        assert!(decl.files.is_empty());
        assert_eq!(
            decl.pip,
            vec!["requests".to_string(), "click>=8.0".to_string()]
        );
    }

    #[test]
    fn pip_rejects_non_array() {
        assert_eq!(
            parse_deps(&json!({"pip": "requests"})).unwrap_err(),
            "$deps.pip must be an array of strings"
        );
    }

    #[test]
    fn pip_rejects_non_string_entry() {
        assert!(
            parse_deps(&json!({"pip": [1]}))
                .unwrap_err()
                .contains("must be a string")
        );
    }

    #[test]
    fn pip_rejects_empty_entry() {
        assert!(
            parse_deps(&json!({"pip": [""]}))
                .unwrap_err()
                .contains("must not be empty")
        );
    }

    // ---------- npm ----------

    #[test]
    fn npm_alone_parses_string_array() {
        let decl = parse_deps(&json!({"npm": ["chalk", "commander@^12"]})).unwrap();
        assert!(decl.files.is_empty());
        assert!(decl.pip.is_empty());
        assert_eq!(
            decl.npm,
            vec!["chalk".to_string(), "commander@^12".to_string()]
        );
    }

    #[test]
    fn npm_rejects_non_array() {
        assert_eq!(
            parse_deps(&json!({"npm": "chalk"})).unwrap_err(),
            "$deps.npm must be an array of strings"
        );
    }

    #[test]
    fn npm_rejects_non_string_entry() {
        assert!(
            parse_deps(&json!({"npm": [1]}))
                .unwrap_err()
                .contains("must be a string")
        );
    }

    #[test]
    fn npm_rejects_empty_entry() {
        assert!(
            parse_deps(&json!({"npm": [""]}))
                .unwrap_err()
                .contains("must not be empty")
        );
    }

    // ---------- npm_package_name ----------

    #[test]
    fn unknown_top_field_is_rejected() {
        assert!(
            parse_deps(&json!({"cargo": []}))
                .unwrap_err()
                .contains("unknown field \"cargo\"")
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
}
