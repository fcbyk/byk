//! `..clone`：把另一个别名文件复制为当前文件。

use std::path::PathBuf;

use super::{FileScope, create, resolve_target, source};
use crate::core::paths::PathLayout;

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(PathBuf, String), String> {
    let source_arg = parse_args(args)?;

    // 先确认目标可用，避免为已存在的文件白跑网络。
    let target = resolve_target(scope, stem, layout)?;
    create::ensure_absent(&target)?;

    let loaded = match source_arg {
        SourceArg::Raw(spec) => source::load(&spec, layout)?,
        SourceArg::Forge(forge, spec) => source::load(&source::forge_url(forge, &spec)?, layout)?,
    };
    let path = create::destination(scope, stem, layout)?;
    create::create(&path, &loaded.data)?;

    Ok((path, loaded.label))
}

#[derive(Debug)]
enum SourceArg {
    Raw(String),
    Forge(source::Forge, String),
}

fn parse_args(args: &[String]) -> Result<SourceArg, String> {
    if let Some(first) = args.first() {
        if let Some(forge) = forge_flag(first) {
            return match args {
                [_, spec] => Ok(SourceArg::Forge(forge, spec.clone())),
                [_] => Err(format!("Option {} requires a value", first)),
                _ => Err("clone takes exactly one source".to_string()),
            };
        }
        if first.starts_with('-') {
            return Err(format!("Unknown option: {}", first));
        }
    }

    match args {
        [] => Err("clone requires a source".to_string()),
        [spec] => Ok(SourceArg::Raw(spec.clone())),
        _ => Err("clone takes exactly one source".to_string()),
    }
}

fn forge_flag(flag: &str) -> Option<source::Forge> {
    match flag {
        "-gh" | "--github" => Some(source::Forge::Github),
        "-ge" | "--gitee" => Some(source::Forge::Gitee),
        "-jd" | "--jsdelivr" => Some(source::Forge::Jsdelivr),
        _ => None,
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

    fn source_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn clones_bytes_exactly() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let content = "{\n  \"z\": \"echo z\",\n\n  \"a\": \"echo a\"\n}";
        let source = source_file(temp.path(), "src.byk.json", content);

        let (path, cloned_from) = run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap();

        assert_eq!(path, layout.alias_dir.join("ops.byk.json"));
        assert_eq!(
            cloned_from,
            crate::utils::path::canonical(&source).display().to_string()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn clones_global_name() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        source_file(&layout.alias_dir, "src.byk.json", "{\"a\": \"echo a\"}");

        let (path, _) = run(FileScope::Global, "ops", &args(&["@@src"]), &layout).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\": \"echo a\"}");
    }

    #[test]
    fn refuses_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(temp.path(), "src.byk.json", "{\"a\": \"echo a\"}");
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let path = layout.alias_dir.join("ops.byk.json");
        fs::write(&path, r#"{"keep": "echo keep"}"#).unwrap();

        let err = run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert_eq!(err, format!("File already exists: {}", path.display()));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            r#"{"keep": "echo keep"}"#
        );
    }

    #[test]
    fn rejects_missing_source_without_creating_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let missing = temp.path().join("missing.byk.json");

        let err = run(
            FileScope::Global,
            "ops",
            &args(&[missing.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Source not found:"));
        assert!(!layout.alias_dir.exists());
    }

    #[test]
    fn rejects_link_source() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(temp.path(), "src.byk.json", r#"["other.byk.json"]"#);

        let err = run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Source is itself a link:"));
        assert!(!layout.alias_dir.exists());
    }

    #[test]
    fn rejects_invalid_json_source() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(temp.path(), "src.byk.json", "{not json");

        let err = run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Failed to parse source"));
    }

    #[test]
    fn rejects_non_object_source() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(temp.path(), "src.byk.json", "42");

        let err = run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Source must be a JSON object:"));
    }

    #[test]
    fn parses_forge_sources() {
        assert!(matches!(
            parse_args(&args(&["-gh", "a/b/c"])),
            Ok(SourceArg::Forge(source::Forge::Github, spec)) if spec == "a/b/c"
        ));
        assert!(matches!(
            parse_args(&args(&["--github", "a/b/"])),
            Ok(SourceArg::Forge(source::Forge::Github, spec)) if spec == "a/b/"
        ));
        assert!(matches!(
            parse_args(&args(&["-ge", "a/b/c"])),
            Ok(SourceArg::Forge(source::Forge::Gitee, spec)) if spec == "a/b/c"
        ));
        assert!(matches!(
            parse_args(&args(&["--gitee", "a/b/"])),
            Ok(SourceArg::Forge(source::Forge::Gitee, spec)) if spec == "a/b/"
        ));
        assert!(matches!(
            parse_args(&args(&["-jd", "a/b/c"])),
            Ok(SourceArg::Forge(source::Forge::Jsdelivr, spec)) if spec == "a/b/c"
        ));
        assert!(matches!(
            parse_args(&args(&["--jsdelivr", "a/b/"])),
            Ok(SourceArg::Forge(source::Forge::Jsdelivr, spec)) if spec == "a/b/"
        ));
        assert_eq!(
            parse_args(&args(&["--github"])).unwrap_err(),
            "Option --github requires a value"
        );
        assert_eq!(
            parse_args(&args(&["-ge"])).unwrap_err(),
            "Option -ge requires a value"
        );
        assert_eq!(
            parse_args(&args(&["--jsdelivr"])).unwrap_err(),
            "Option --jsdelivr requires a value"
        );
        assert_eq!(
            parse_args(&args(&["-gh", "a", "b"])).unwrap_err(),
            "clone takes exactly one source"
        );
        assert!(
            parse_args(&args(&["--bad"]))
                .unwrap_err()
                .starts_with("Unknown option:")
        );
    }

    #[test]
    fn requires_exactly_one_source() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        assert_eq!(
            run(FileScope::Global, "ops", &[], &layout).unwrap_err(),
            "clone requires a source"
        );
        assert_eq!(
            run(FileScope::Global, "ops", &args(&["a", "b"]), &layout).unwrap_err(),
            "clone takes exactly one source"
        );
    }
}
