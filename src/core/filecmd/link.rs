//! `..link`：把当前文件改写为指向另一个别名文件的链接。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{FileScope, invalid_stem_message, resolve_target, target_path};
use crate::core::aliases::validate_filename;
use crate::core::paths::PathLayout;
use crate::utils::path::expand_home;

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(PathBuf, PathBuf), String> {
    let [spec] = args else {
        return Err("link takes exactly one target".to_string());
    };
    let target = resolve_spec(spec)?;

    let path = resolve_target(scope, stem, layout)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let prefix = match scope {
                FileScope::Local => "@",
                FileScope::Global => "@@",
            };
            return Err(format!(
                "File not found: {}\nHint: create it with {}{}..init --link {}",
                path.display(),
                prefix,
                stem,
                spec
            ));
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", path.display(), e)),
    };

    if metadata.file_type().is_symlink() {
        return Err(format!("Refusing to rewrite symlink: {}", path.display()));
    }
    if metadata.file_type().is_dir() {
        return Err(format!("Refusing to link directory: {}", path.display()));
    }

    let canonical = fs::canonicalize(&path)
        .map_err(|e| format!("Failed to resolve {}: {}", path.display(), e))?;
    if canonical == target {
        return Err(format!("Link cannot point to itself: {}", path.display()));
    }

    let text = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let current: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    let convertible = matches!(&current, Value::Array(_))
        || matches!(&current, Value::Object(map) if map.is_empty());
    if !convertible {
        return Err(format!(
            "Not a link: {}\nHint: link works on a link file or an empty `{{}}` file",
            path.display()
        ));
    }

    let content = content(&target)?;
    fs::write(&path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    Ok((path, target))
}

/// 解析并校验 link 目标：`@<name>` 指 cwd 的局部别名，其余按路径处理。
pub(super) fn resolve_spec(spec: &str) -> Result<PathBuf, String> {
    if spec.is_empty() {
        return Err("Link target is empty".to_string());
    }

    let raw = if let Some(stem) = spec.strip_prefix('@') {
        if stem.starts_with('@') {
            return Err("Link target must be @<name> or a path".to_string());
        }
        if !validate_filename(stem) {
            return Err(invalid_stem_message(stem));
        }
        let cwd = std::env::current_dir()
            .map_err(|e| format!("Failed to resolve current directory: {}", e))?;
        target_path(&cwd, stem)
    } else {
        let expanded = expand_home(spec);
        if expanded.is_relative() {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("Failed to resolve current directory: {}", e))?;
            cwd.join(expanded)
        } else {
            expanded
        }
    };

    let metadata = match fs::metadata(&raw) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(format!("Link target not found: {}", raw.display()));
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", raw.display(), e)),
    };
    if !metadata.is_file() {
        return Err(format!("Link target is not a file: {}", raw.display()));
    }

    let target = fs::canonicalize(&raw)
        .map_err(|e| format!("Failed to resolve link target {}: {}", raw.display(), e))?;
    let data = fs::read(&target)
        .map_err(|e| format!("Failed to read link target {}: {}", target.display(), e))?;
    match serde_json::from_slice::<Value>(&data) {
        Ok(Value::Object(_)) => {}
        Ok(Value::Array(_)) => {
            return Err(format!(
                "Link target is itself a link: {}",
                target.display()
            ));
        }
        Ok(_) => {
            return Err(format!(
                "Link target must be a JSON object: {}",
                target.display()
            ));
        }
        Err(e) => {
            return Err(format!(
                "Failed to parse link target {}: {}",
                target.display(),
                e
            ));
        }
    }

    Ok(target)
}

pub(super) fn content(target: &Path) -> Result<String, String> {
    let spec = target
        .to_str()
        .ok_or_else(|| format!("Path is not valid UTF-8: {}", target.display()))?;
    let mut content =
        serde_json::to_string(&[spec]).map_err(|e| format!("Failed to encode link: {}", e))?;
    content.push('\n');
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join(".byk").join("alias"),
        }
    }

    fn object_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, r#"{"cmd": "echo hi"}"#).unwrap();
        path
    }

    fn spec(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn resolve_spec_accepts_object() {
        let temp = tempfile::tempdir().unwrap();
        let target = object_file(temp.path(), "tool.byk.json");

        let resolved = resolve_spec(&spec(&target)).unwrap();

        assert_eq!(resolved, fs::canonicalize(&target).unwrap());
    }

    #[test]
    fn resolve_spec_rejects_empty() {
        assert_eq!(resolve_spec("").unwrap_err(), "Link target is empty");
    }

    #[test]
    fn resolve_spec_rejects_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.byk.json");

        let err = resolve_spec(&spec(&missing)).unwrap_err();

        assert!(err.starts_with("Link target not found:"));
    }

    #[test]
    fn resolve_spec_rejects_directory() {
        let temp = tempfile::tempdir().unwrap();

        let err = resolve_spec(&spec(temp.path())).unwrap_err();

        assert!(err.starts_with("Link target is not a file:"));
    }

    #[test]
    fn resolve_spec_rejects_invalid_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bad.byk.json");
        fs::write(&path, "{not json").unwrap();

        let err = resolve_spec(&spec(&path)).unwrap_err();

        assert!(err.starts_with("Failed to parse link target"));
    }

    #[test]
    fn resolve_spec_rejects_link_target() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("chain.byk.json");
        fs::write(&path, r#"["other.byk.json"]"#).unwrap();

        let err = resolve_spec(&spec(&path)).unwrap_err();

        assert!(err.starts_with("Link target is itself a link:"));
    }

    #[test]
    fn resolve_spec_rejects_non_object() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("scalar.byk.json");
        fs::write(&path, "42").unwrap();

        let err = resolve_spec(&spec(&path)).unwrap_err();

        assert!(err.starts_with("Link target must be a JSON object:"));
    }

    #[test]
    fn run_retargets_existing_link() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let link_path = layout.alias_dir.join("tools.byk.json");
        fs::write(&link_path, r#"["/old/tool.byk.json"]"#).unwrap();
        let target = object_file(temp.path(), "new.byk.json");

        let (path, linked) = run(FileScope::Global, "tools", &[spec(&target)], &layout).unwrap();

        assert_eq!(path, link_path);
        assert_eq!(linked, fs::canonicalize(&target).unwrap());
        let raw = fs::read_to_string(&link_path).unwrap();
        let items: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(items, vec![linked.to_str().unwrap().to_string()]);
    }

    #[test]
    fn run_converts_empty_object() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let link_path = layout.alias_dir.join("tools.byk.json");
        fs::write(&link_path, "{}\n").unwrap();
        let target = object_file(temp.path(), "tool.byk.json");

        run(FileScope::Global, "tools", &[spec(&target)], &layout).unwrap();

        let raw = fs::read_to_string(&link_path).unwrap();
        assert!(raw.starts_with("[\""));
        assert!(raw.ends_with("\"]\n"));
    }

    #[test]
    fn run_refuses_non_empty_object() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let link_path = layout.alias_dir.join("tools.byk.json");
        fs::write(&link_path, r#"{"keep": "echo keep"}"#).unwrap();
        let target = object_file(temp.path(), "tool.byk.json");

        let err = run(FileScope::Global, "tools", &[spec(&target)], &layout).unwrap_err();

        assert!(err.starts_with("Not a link:"));
        assert_eq!(
            fs::read_to_string(&link_path).unwrap(),
            r#"{"keep": "echo keep"}"#
        );
    }

    #[test]
    fn run_missing_file_suggests_init() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let target = object_file(temp.path(), "tool.byk.json");

        let err = run(FileScope::Global, "tools", &[spec(&target)], &layout).unwrap_err();

        assert!(err.starts_with("File not found:"));
        assert!(err.contains("@@tools..init --link"));
    }

    #[test]
    fn run_rejects_self_link() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let link_path = layout.alias_dir.join("tools.byk.json");
        fs::write(&link_path, "{}\n").unwrap();

        let err = run(FileScope::Global, "tools", &[spec(&link_path)], &layout).unwrap_err();

        assert!(err.starts_with("Link cannot point to itself:"));
    }

    #[cfg(unix)]
    #[test]
    fn run_refuses_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let real = temp.path().join("real.byk.json");
        fs::write(&real, "{}\n").unwrap();
        let link_path = layout.alias_dir.join("tools.byk.json");
        symlink(&real, &link_path).unwrap();
        let target = object_file(temp.path(), "tool.byk.json");

        let err = run(FileScope::Global, "tools", &[spec(&target)], &layout).unwrap_err();

        assert!(err.starts_with("Refusing to rewrite symlink:"));
        assert_eq!(fs::read_to_string(&real).unwrap(), "{}\n");
    }

    #[test]
    fn run_requires_exactly_one_target() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let target = object_file(temp.path(), "tool.byk.json");

        assert_eq!(
            run(FileScope::Global, "tools", &[], &layout).unwrap_err(),
            "link takes exactly one target"
        );
        assert_eq!(
            run(
                FileScope::Global,
                "tools",
                &[spec(&target), "extra".to_string()],
                &layout
            )
            .unwrap_err(),
            "link takes exactly one target"
        );
    }
}
