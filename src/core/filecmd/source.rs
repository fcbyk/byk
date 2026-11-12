//! 别名文件来源解析：`@name` / `@@name` / 路径，link 与 clone 共用。

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use super::{invalid_stem_message, target_path};
use crate::core::aliases::validate_filename;
use crate::core::paths::PathLayout;
use crate::utils::path::{canonical, expand_home};

/// 已读取并校验的来源内容。
#[derive(Debug)]
pub(super) struct Loaded {
    pub(super) label: String,
    pub(super) data: Vec<u8>,
}

/// 解析链接目标：只接受本地文件；URL 明确拒绝并提示用 clone。
pub(super) fn resolve(spec: &str, layout: &PathLayout) -> Result<PathBuf, String> {
    match classify(spec)? {
        SourceKind::Url(_) => Err(format!(
            "Remote source is not supported here: {}\nHint: use ..clone for URLs",
            spec
        )),
        SourceKind::Local => load_local(spec, layout).map(|(path, _)| path),
    }
}

/// 读取并校验来源；本地路径返回规范化路径，URL 通过外部 curl 获取。
pub(super) fn load(spec: &str, layout: &PathLayout) -> Result<Loaded, String> {
    match classify(spec)? {
        SourceKind::Url(url) => {
            let data = fetch(url)?;
            validate(&data, url)?;
            Ok(Loaded {
                label: url.to_string(),
                data,
            })
        }
        SourceKind::Local => {
            let (path, data) = load_local(spec, layout)?;
            Ok(Loaded {
                label: path.display().to_string(),
                data,
            })
        }
    }
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
pub(super) enum Forge {
    Github,
    Gitee,
    Jsdelivr,
}

impl Forge {
    fn name(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Gitee => "Gitee",
            Self::Jsdelivr => "jsDelivr",
        }
    }

    fn raw_url(self, user: &str, repo: &str, reference: &str, path: &str) -> String {
        match self {
            Self::Github => format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                user, repo, reference, path
            ),
            Self::Gitee => format!(
                "https://gitee.com/{}/{}/raw/{}/{}",
                user, repo, reference, path
            ),
            Self::Jsdelivr => format!(
                "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                user, repo, reference, path
            ),
        }
    }
}

/// 把 `<user>/<repo>@<ref>/<path>` 展开为对应平台的 raw URL。
pub(super) fn forge_url(forge: Forge, spec: &str) -> Result<String, String> {
    let mut parts = spec.splitn(3, '/');
    let user = parts.next().unwrap_or("");
    let repo_ref = parts.next().ok_or_else(|| shape_error(forge))?;
    let path = parts.next().ok_or_else(|| shape_error(forge))?;

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

    Ok(forge.raw_url(
        &segments[0],
        &segments[1],
        &segments[2],
        &segments[3..].join("/"),
    ))
}

fn shape_error(forge: Forge) -> String {
    format!(
        "{} source must be <user>/<repo>[/<path>]\nHint: end with `/` for the root .byk.json",
        forge.name()
    )
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

fn encode_segment(segment: &str) -> String {
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
    let raw = if let Some(stem) = spec.strip_prefix("@@") {
        if !validate_filename(stem) {
            return Err(invalid_stem_message(stem));
        }
        target_path(&layout.alias_dir, stem)
    } else if let Some(stem) = spec.strip_prefix('@') {
        if !validate_filename(stem) {
            return Err(invalid_stem_message(stem));
        }
        target_path(&current_dir()?, stem)
    } else {
        let expanded = expand_home(spec);
        if expanded.is_relative() {
            current_dir()?.join(expanded)
        } else {
            expanded
        }
    };

    let metadata = match fs::metadata(&raw) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(format!("Source not found: {}", raw.display()));
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", raw.display(), e)),
    };
    if !metadata.is_file() {
        return Err(format!("Source is not a file: {}", raw.display()));
    }

    let target = canonical(&raw);
    let data = fs::read(&target)
        .map_err(|e| format!("Failed to read source {}: {}", target.display(), e))?;
    validate(&data, &target.display().to_string())?;
    Ok((target, data))
}

fn validate(data: &[u8], label: &str) -> Result<(), String> {
    match serde_json::from_slice::<Value>(data) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(Value::Array(_)) => Err(format!("Source is itself a link: {}", label)),
        Ok(_) => Err(format!("Source must be a JSON object: {}", label)),
        Err(e) => Err(format!("Failed to parse source {}: {}", label, e)),
    }
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("curl")
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
        return Err(if reason.is_empty() {
            format!("curl failed (exit {})", code)
        } else {
            format!("curl failed (exit {}): {}", code, reason)
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
            forge_url(Forge::Github, "aaa/bb/sss").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, "aaa/bb/").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, "aaa/bb/sss/xxx").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss/xxx.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, "aaa/bb@v1/sss").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/v1/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, "aaa/bb@abc123/").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/abc123/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, "aaa/bb/sss/xxx.byk.json").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss/xxx.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Github, "aaa/bb/sss/").unwrap(),
            "https://raw.githubusercontent.com/aaa/bb/HEAD/sss/.byk.json"
        );
    }

    #[test]
    fn forge_url_maps_gitee() {
        assert_eq!(
            forge_url(Forge::Gitee, "aaa/bb/sss").unwrap(),
            "https://gitee.com/aaa/bb/raw/HEAD/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Gitee, "aaa/bb@v1/").unwrap(),
            "https://gitee.com/aaa/bb/raw/v1/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Gitee, "aaa/bb/dir/xxx").unwrap(),
            "https://gitee.com/aaa/bb/raw/HEAD/dir/xxx.byk.json"
        );
    }

    #[test]
    fn forge_url_maps_jsdelivr() {
        assert_eq!(
            forge_url(Forge::Jsdelivr, "aaa/bb/sss").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/sss.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Jsdelivr, "aaa/bb@v1/").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@v1/.byk.json"
        );
        assert_eq!(
            forge_url(Forge::Jsdelivr, "aaa/bb@abc123/dir/xxx").unwrap(),
            "https://cdn.jsdelivr.net/gh/aaa/bb@abc123/dir/xxx.byk.json"
        );
    }

    #[test]
    fn forge_url_encodes_segments() {
        assert_eq!(
            forge_url(Forge::Github, "aa a/bb@v 1/dir name/xy").unwrap(),
            "https://raw.githubusercontent.com/aa%20a/bb/v%201/dir%20name/xy.byk.json"
        );
    }

    #[test]
    fn forge_url_rejects_bad_shape() {
        for spec in ["aaa", "aaa/bb", "aaa//sss", "aaa/bb@/sss", "aaa/bb/../sss"] {
            assert!(forge_url(Forge::Github, spec).is_err(), "{spec}");
        }
        assert!(forge_url(Forge::Github, "aaa/bb/sss//xxx").is_err());
    }

    #[test]
    fn rejects_invalid_name() {
        let temp = tempfile::tempdir().unwrap();

        let err = resolve("@@a/b", &layout(&temp)).unwrap_err();

        assert!(err.starts_with("Invalid file name:"));
    }
}
