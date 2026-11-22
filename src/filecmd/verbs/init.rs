//! `init`：创建新的别名文件（空文件或链接文件）。

use std::path::PathBuf;

use super::link;
use crate::filecmd::io::create::{self as create, EMPTY_ALIAS_FILE};
use crate::filecmd::source;
use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;

pub(crate) fn run(
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

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn defaults_to_empty_file() {
        assert_eq!(parse_args(&[]), Ok(None));
    }

    #[test]
    fn accepts_link_target() {
        assert_eq!(
            parse_args(&args(&["--link", "@tool"])),
            Ok(Some("@tool".to_string()))
        );
    }

    #[test]
    fn rejects_other_arguments() {
        assert_eq!(
            parse_args(&args(&["extra"])).unwrap_err(),
            "init accepts only --link <target>"
        );
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
            &args(&["--link", source.to_str().unwrap()]),
            &layout,
        )
        .unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let items: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(std::path::PathBuf::from(&items[0]), canonical);
    }

    #[test]
    fn run_link_rejects_invalid_target_without_creating_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let missing = temp.path().join("missing.byk.json");

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["--link", missing.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Source not found:"));
        assert!(!layout.alias_dir.exists());
    }
}
