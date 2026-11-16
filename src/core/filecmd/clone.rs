//! `..clone`：把另一个别名文件复制为当前文件。
//!
//! `-d` 在同一次调用里接着装 `$deps`：克隆先落盘，再读新文件的声明。
//! 装依赖失败不回滚——文件本身已经合法，缺的只是资产，重跑
//! `@@<file>..deps i` 即可补齐。

use super::{FileScope, create, deps, resolve_target, source};
use crate::core::paths::PathLayout;

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(), String> {
    let options = parse_args(args)?;

    // 依赖目录按全局 key 命名，局部文件没有稳定落点：拦在落盘之前。
    if options.install_deps && scope != FileScope::Global {
        return Err(
            "--deps requires a global alias file\nHint: clone with @@<file> instead of @<file>; $deps is only supported there"
                .to_string(),
        );
    }

    // 先确认目标可用，避免为已存在的文件白跑网络。
    let target = resolve_target(scope, stem, layout)?;
    create::ensure_absent(&target)?;

    let loaded = match options.source {
        SourceArg::Raw(spec) => source::load(&spec, layout)?,
        SourceArg::Forge(forge, spec) => source::load(&source::forge_url(forge, &spec)?, layout)?,
    };
    let path = create::destination(scope, stem, layout)?;
    create::create(&path, &loaded.data)?;
    // 先报落盘结果：后面的装依赖可能失败，但这行必须已经出现过。
    eprintln!("Cloned {} to {}", loaded.label, path.display());

    if options.install_deps {
        install_deps(stem, layout)?;
    }
    Ok(())
}

/// 克隆成功后再装依赖。
///
/// 安装不二次确认，进度直接打出来；失败不回滚，提示重跑 `..deps i` 比删掉更安全。
fn install_deps(stem: &str, layout: &PathLayout) -> Result<(), String> {
    let fail = |err: String| {
        format!(
            "Failed to install dependencies: {}\nHint: run `byk @@{}..deps i` to retry",
            err, stem
        )
    };

    let (_, declared) = deps::declaration(stem, layout).map_err(fail)?;
    let Some(entries) = declared else {
        eprintln!("Cloned file declares no $deps; nothing to install");
        return Ok(());
    };
    if entries.is_empty() {
        eprintln!("Cloned file declares no assets ($deps.files is empty)");
        return Ok(());
    }

    deps::install(stem, layout, &entries, false).map_err(fail)
}

#[derive(Debug)]
struct Options {
    source: SourceArg,
    install_deps: bool,
}

#[derive(Debug)]
enum SourceArg {
    Raw(String),
    Forge(source::Forge, String),
}

/// 选项可以出现在来源之前或之后：`-gh a/b/c -d`、`a/b/c -d`、`-d -gh a/b/c` 等价。
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut source: Option<SourceArg> = None;
    let mut install_deps = false;

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "-d" || arg == "--deps" {
            install_deps = true;
            continue;
        }
        if let Some(forge) = forge_flag(arg) {
            let spec = rest
                .next()
                .ok_or_else(|| format!("Option {} requires a value", arg))?;
            if source.is_some() {
                return Err("clone takes exactly one source".to_string());
            }
            source = Some(SourceArg::Forge(forge, spec.clone()));
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            return Err(format!("Unknown option: {}", arg));
        }
        if source.is_some() {
            return Err("clone takes exactly one source".to_string());
        }
        source = Some(SourceArg::Raw(arg.clone()));
    }

    let source = source.ok_or_else(|| "clone requires a source".to_string())?;
    Ok(Options {
        source,
        install_deps,
    })
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

    fn source_file(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
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

        run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(layout.alias_dir.join("ops.byk.json")).unwrap(),
            content
        );
    }

    #[test]
    fn clones_global_name() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        source_file(&layout.alias_dir, "src.byk.json", "{\"a\": \"echo a\"}");

        run(FileScope::Global, "ops", &args(&["@@src"]), &layout).unwrap();

        assert_eq!(
            fs::read_to_string(layout.alias_dir.join("ops.byk.json")).unwrap(),
            "{\"a\": \"echo a\"}"
        );
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
        let forge = |values: &[&str]| match parse_args(&args(values)).unwrap().source {
            SourceArg::Forge(forge, spec) => (forge, spec),
            other => panic!("expected a forge source, got {:?}", other),
        };

        assert_eq!(
            forge(&["-gh", "a/b/c"]),
            (source::Forge::Github, "a/b/c".to_string())
        );
        assert_eq!(
            forge(&["--github", "a/b/"]),
            (source::Forge::Github, "a/b/".to_string())
        );
        assert_eq!(
            forge(&["-ge", "a/b/c"]),
            (source::Forge::Gitee, "a/b/c".to_string())
        );
        assert_eq!(
            forge(&["--gitee", "a/b/"]),
            (source::Forge::Gitee, "a/b/".to_string())
        );
        assert_eq!(
            forge(&["-jd", "a/b/c"]),
            (source::Forge::Jsdelivr, "a/b/c".to_string())
        );
        assert_eq!(
            forge(&["--jsdelivr", "a/b/"]),
            (source::Forge::Jsdelivr, "a/b/".to_string())
        );
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
    fn parses_install_deps_in_any_position() {
        assert!(parse_args(&args(&["a/b/c", "-d"])).unwrap().install_deps);
        assert!(
            parse_args(&args(&["--deps", "a/b/c"]))
                .unwrap()
                .install_deps
        );
        assert!(!parse_args(&args(&["a/b/c"])).unwrap().install_deps);

        let mixed = parse_args(&args(&["-gh", "a/b/c", "-d"])).unwrap();
        assert!(mixed.install_deps);
        assert!(matches!(mixed.source, SourceArg::Forge(..)));

        assert_eq!(
            parse_args(&args(&["-d"])).unwrap_err(),
            "clone requires a source"
        );
    }

    #[test]
    fn install_deps_is_global_only() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(temp.path(), "src.byk.json", "{\"a\": \"echo a\"}");

        let err = run(
            FileScope::Local,
            "ops",
            &args(&[source.to_str().unwrap(), "-d"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("--deps requires a global alias file"));
        assert!(!temp.path().join("ops.byk.json").exists());
    }

    #[test]
    fn install_deps_skips_without_declaration() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(temp.path(), "src.byk.json", "{\"a\": \"echo a\"}");

        run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap(), "-d"]),
            &layout,
        )
        .unwrap();

        assert!(layout.alias_dir.join("ops.byk.json").is_file());
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
