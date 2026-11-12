//! `..init`：创建新的别名文件（空文件或链接文件）。

use std::path::PathBuf;

use super::{FileScope, create, link, source};
use crate::core::paths::PathLayout;

/// 新建内容：空 JSON 对象加一个结尾换行。
pub(super) const EMPTY_ALIAS_FILE: &[u8] = b"{}\n";

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<PathBuf, String> {
    let link_spec = parse_args(args)?;
    let link_target = match &link_spec {
        Some(spec) => Some(source::resolve(spec, layout)?),
        None => None,
    };
    let content = match &link_target {
        Some(target) => link::content(target)?.into_bytes(),
        None => EMPTY_ALIAS_FILE.to_vec(),
    };

    let target = create::destination(scope, stem, layout)?;
    create::create(&target, &content)?;
    Ok(target)
}

fn parse_args(args: &[String]) -> Result<Option<String>, String> {
    match args {
        [] => Ok(None),
        [flag, spec] if flag == "--link" => Ok(Some(spec.clone())),
        _ => Err("init accepts only --link <target>".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join(".byk").join("alias"),
        }
    }

    fn link_args(spec: &str) -> Vec<String> {
        vec!["--link".to_string(), spec.to_string()]
    }

    #[test]
    fn run_creates_dirs_and_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let path = run(FileScope::Global, "ops", &[], &layout).unwrap();

        assert_eq!(path, layout.alias_dir.join("ops.byk.json"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn run_anonymous_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let path = run(FileScope::Global, "", &[], &layout).unwrap();

        assert_eq!(path, layout.alias_dir.join(".byk.json"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn run_link_creates_link_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = temp.path().join("tool.byk.json");
        fs::write(&source, "{}").unwrap();
        let canonical = crate::utils::path::canonical(&source);

        let path = run(
            FileScope::Global,
            "ops",
            &link_args(source.to_str().unwrap()),
            &layout,
        )
        .unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let items: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(PathBuf::from(&items[0]), canonical);
    }

    #[test]
    fn run_link_rejects_invalid_target_without_creating_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let missing = temp.path().join("missing.byk.json");

        let err = run(
            FileScope::Global,
            "ops",
            &link_args(missing.to_str().unwrap()),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Source not found:"));
        assert!(!layout.alias_dir.exists());
    }

    #[test]
    fn parse_args_variants() {
        assert_eq!(parse_args(&[]), Ok(None));
        assert_eq!(
            parse_args(&link_args("@tool")),
            Ok(Some("@tool".to_string()))
        );
        assert!(parse_args(&["extra".to_string()]).is_err());
        assert!(parse_args(&["--link".to_string()]).is_err());
        assert!(parse_args(&["--bad".to_string(), "@tool".to_string()]).is_err());
    }

    #[test]
    fn run_rejects_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(FileScope::Global, "ops", &["extra".to_string()], &layout).unwrap_err();

        assert_eq!(err, "init accepts only --link <target>");
        assert!(!layout.alias_dir.join("ops.byk.json").exists());
    }

    #[test]
    fn create_never_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");
        fs::write(&path, "{\"keep\": true}").unwrap();

        let err = create::create(&path, EMPTY_ALIAS_FILE).unwrap_err();

        assert_eq!(err, format!("File already exists: {}", path.display()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"keep\": true}");
    }

    #[test]
    fn create_writes_given_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ops.byk.json");

        create::create(&path, EMPTY_ALIAS_FILE).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }
}
