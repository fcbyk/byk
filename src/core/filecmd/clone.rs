//! `..clone`：把另一个别名文件复制为当前文件。
//!
//! `-d` 在同一次调用里接着装 `$deps`：克隆先落盘，再读新文件的声明。
//! 装依赖失败不回滚——文件本身已经合法，缺的只是资产，重跑
//! `@@<file>..deps i` 即可补齐。
//!
//! 远程来源把 `$deps.base` 冻结成绝对 URL，本地来源冻结成本机绝对路径：
//! 这是 base 唯一一次能被解释的时刻，落盘之后的文件没有自己的位置信息。
//! 两条路走不同的冻结函数，远程永远产不出本地路径形态。

use serde_json::{Map, Value};

use super::command::resolve_target;
use super::{FileScope, create, deps, source};
use crate::core::config::{build_file_key, is_alias_value, parse_clone};
use crate::core::deps::Origin;
use crate::core::paths::PathLayout;

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(), String> {
    let options = parse_args(args)?;

    // 先确认目标可用，避免为已存在的文件白跑网络。
    let target = resolve_target(scope, stem, layout)?;
    create::ensure_absent(&target)?;

    let loaded = match options.source {
        SourceArg::Raw(spec) => source::load(&spec, layout)?,
        SourceArg::Forge(forge, spec) => source::load(&source::forge_url(forge, &spec)?, layout)?,
    };
    let path = create::destination(scope, stem, layout)?;
    let prepared = prepare(&loaded.data, &loaded.origin)?;
    create::create(&path, &prepared.body)?;
    // 先报落盘结果：后面的装依赖可能失败，但这行必须已经出现过。
    eprintln!("Cloned {} to {}", loaded.label, path.display());
    if prepared.skipped > 0 {
        eprintln!(
            "Skipped {} {} marked $clone: false",
            prepared.skipped,
            if prepared.skipped == 1 {
                "alias"
            } else {
                "aliases"
            }
        );
    }
    if let Some(base) = prepared.base {
        eprintln!("Recorded $deps.base = {base}");
    }

    if options.install_deps {
        install_deps(scope, stem, layout)?;
    }
    Ok(())
}

/// 克隆成功后再装依赖。
///
/// 安装不二次确认，进度直接打出来；失败不回滚，提示重跑 `..deps i` 比删掉更安全。
fn install_deps(scope: FileScope, stem: &str, layout: &PathLayout) -> Result<(), String> {
    let hint = format!("byk {}..deps i", build_file_key(stem, scope.is_global()));
    let fail =
        |err: String| format!("Failed to install dependencies: {err}\nHint: run `{hint}` to retry");

    let (_, declared) = deps::declaration(scope, stem, layout).map_err(fail)?;
    let Some(decl) = declared else {
        eprintln!("Cloned file declares no $deps; nothing to install");
        return Ok(());
    };
    if decl.files.is_empty() && decl.pip.is_empty() && decl.npm.is_empty() {
        eprintln!(
            "Cloned file declares no assets ($deps.files, $deps.pip and $deps.npm are empty)"
        );
        return Ok(());
    }

    deps::install(scope, stem, layout, &decl, false).map_err(fail)
}

/// clone 的写回：剔除 `$clone: false` 的子树，并把 `$deps.base` 冻结成绝对地址。
///
/// 两种写入理由分开记：`removed` 决定「什么都没剩下」是否算失败，
/// `changed` 只决定要不要重新序列化。只写了 `$clone: true` 或 `false` 标记、
/// 却没真删内容的文件仍然照常 clone，只是标记不会留在副本里。
///
/// base 冻结成什么由来源决定：远程来源写 http(s) 目录 URL，本地来源写本机绝对路径。
/// 这条分叉就是安全边界的全部——远程 clone 产不出本地路径形态，所以网上拿来
/// 的别名文件读不了本机文件。
#[derive(Debug)]
struct Prepared {
    body: Vec<u8>,
    base: Option<String>,
    skipped: usize,
}

fn prepare(data: &[u8], origin: &Origin) -> Result<Prepared, String> {
    let mut root: Value = serde_json::from_slice(data)
        .map_err(|e| format!("Failed to parse source {}: {e}", origin.as_str()))?;

    let strip = strip_clone_disabled(&mut root)?;
    let mut changed = strip.changed;
    let mut recorded = None;

    if let Some(object) = root.get_mut("$deps").and_then(Value::as_object_mut) {
        let current = object.get("base").cloned();
        // 变量在这里没有表可查，两路冻结都会直接拒绝：base 只该由程序写成
        // 字面量，猜一个写死只会把作者意图烧成假地址。
        let base = match origin {
            Origin::Url(url) => crate::core::deps::freeze_remote_base(current.as_ref(), url)?,
            Origin::File(path) => crate::core::deps::freeze_local_base(current.as_ref(), path)?,
        };
        if current.as_ref().and_then(Value::as_str) != Some(base.as_str()) {
            object.insert("base".to_string(), Value::String(base.clone()));
            changed = true;
            recorded = Some(base);
        }
    }

    let body = if changed {
        encode(&root)?
    } else {
        // 没有标记、没有需要冻结的 base：保持字节副本，连格式都不碰
        data.to_vec()
    };
    Ok(Prepared {
        body,
        base: recorded,
        skipped: strip.skipped,
    })
}

fn encode(root: &Value) -> Result<Vec<u8>, String> {
    let mut body = serde_json::to_string_pretty(root)
        .map_err(|e| format!("Failed to encode the clone: {e}"))?;
    body.push('\n');
    Ok(body.into_bytes())
}

/// 按 `$clone` 的继承规则剔除内容；根上没写时默认 `true`。
///
/// 与 `$private` 不同，`false` 的**分组**不整段删除：它只是给子级一个默认值，
/// 子级写 `true` 仍可重新暴露。真正被删的是别名叶子和剪空后的分组骨架。
fn strip_clone_disabled(root: &mut Value) -> Result<Strip, String> {
    let Some(object) = root.as_object_mut() else {
        return Ok(Strip::default());
    };
    let root_clone = object.get("$clone").map(parse_clone).unwrap_or(true);
    let mut strip = Strip::default();
    strip_object(object, root_clone, &mut strip);
    if object.remove("$clone").is_some() {
        strip.changed = true;
    }

    // 全部剪空时拒绝落盘：`..clone` 不该产出一个空对象
    let empty = object.keys().all(|key| key.starts_with('$'));
    if empty && (!root_clone || strip.removed) {
        return Err("Nothing to clone: every alias is marked $clone: false".to_string());
    }
    Ok(strip)
}

#[derive(Default)]
struct Strip {
    /// 需要重新序列化：删过内容，或删过 `$clone` 标记
    changed: bool,
    /// 真的删过内容；决定「剪空」是否算失败
    removed: bool,
    /// 被删掉的别名数量，用于 stderr 报告
    skipped: usize,
}

fn strip_object(object: &mut Map<String, Value>, inherited: bool, strip: &mut Strip) {
    let keys: Vec<String> = object
        .keys()
        .filter(|key| !key.starts_with('$'))
        .cloned()
        .collect();

    for key in keys {
        let remove = match object.get_mut(&key) {
            None => false,
            Some(Value::Object(inner)) => {
                // 节点自己的 $clone 优先于父级给的默认值
                let node_clone = inner.get("$clone").map(parse_clone).unwrap_or(inherited);
                let is_alias = inner.contains_key("$cmd");
                if !node_clone && is_alias {
                    strip.removed = true;
                    strip.skipped += 1;
                    true
                } else {
                    let removed_before = strip.removed;
                    strip_object(inner, node_clone, strip);
                    let children_removed = strip.removed && !removed_before;
                    if inner.remove("$clone").is_some() {
                        strip.changed = true;
                    }
                    // 分组失去全部子节点才剪掉；本来就没写标记的空分组保持原样
                    !is_alias
                        && inner.keys().all(|key| key.starts_with('$'))
                        && (!node_clone || children_removed)
                }
            }
            Some(value) => {
                if inherited {
                    false
                } else {
                    if is_alias_value(value) {
                        strip.skipped += 1;
                    }
                    strip.removed = true;
                    true
                }
            }
        };

        if remove {
            object.remove(&key);
            strip.changed = true;
            strip.removed = true;
        }
    }
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
    use serde_json::json;
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

    // ---------- prepare：远程 base 冻结 ----------

    const DOC: &str = "https://hello.com/sdsd/asfdsg/qwe/we/aa.byk.json";

    fn remote() -> Origin {
        Origin::Url(DOC.to_string())
    }

    fn prepared(content: &str) -> (Value, Option<String>) {
        prepared_from(content, &remote())
    }

    fn prepared_from(content: &str, origin: &Origin) -> (Value, Option<String>) {
        let prepared = prepare(content.as_bytes(), origin).unwrap();
        (
            serde_json::from_slice(&prepared.body).unwrap(),
            prepared.base,
        )
    }

    #[test]
    fn prepare_leaves_documents_without_deps_untouched() {
        let content = "{ \"z\": \"echo z\",\n  \"a\": \"echo a\" }";
        let prepared = prepare(content.as_bytes(), &remote()).unwrap();
        assert_eq!(prepared.base, None);
        assert_eq!(
            prepared.body,
            content.as_bytes(),
            "没有 $deps 就是纯字节副本"
        );
    }

    #[test]
    fn prepare_fills_the_base_even_without_relative_src() {
        let (doc, base) = prepared(r#"{"$deps": {"files": ["https://e.com/tool"]}}"#);
        assert_eq!(
            base.as_deref(),
            Some("https://hello.com/sdsd/asfdsg/qwe/we/")
        );
        assert_eq!(
            doc["$deps"]["base"],
            "https://hello.com/sdsd/asfdsg/qwe/we/"
        );
        assert_eq!(doc["$deps"]["files"][0], "https://e.com/tool");
    }

    #[test]
    fn prepare_resolves_a_relative_base() {
        let (doc, _) = prepared(r#"{"$deps": {"base": ".", "files": ["./tool"]}}"#);
        assert_eq!(
            doc["$deps"]["base"],
            "https://hello.com/sdsd/asfdsg/qwe/we/"
        );

        let (doc, _) = prepared(r#"{"$deps": {"base": "..", "files": ["../x"]}}"#);
        assert_eq!(doc["$deps"]["base"], "https://hello.com/sdsd/asfdsg/qwe/");
    }

    #[test]
    fn prepare_remote_rejects_a_local_base() {
        // 远程来源只能产出 URL 形态，本地路径形态必须来自本地 clone
        for bad in [
            r#"{"$deps": {"base": "/sub", "files": ["x"]}}"#,
            r#"{"$deps": {"base": "/Users/coke/x", "files": ["x"]}}"#,
            r#"{"$deps": {"base": "C:/x", "files": ["x"]}}"#,
        ] {
            let err = prepare(bad.as_bytes(), &remote()).unwrap_err();
            assert!(err.contains("is a local path"), "{bad}: {err}");
        }
    }

    #[test]
    fn prepare_keeps_an_authors_absolute_base() {
        let content = r#"{"$deps": {"base": "https://cdn.example.com/x/", "files": ["a"]}}"#;
        let prepared = prepare(content.as_bytes(), &remote()).unwrap();

        assert_eq!(prepared.base, None, "已经是对的就不改，也不报");
        assert_eq!(prepared.body, content.as_bytes());
    }

    #[test]
    fn prepare_canonicalizes_an_absolute_base_once() {
        let (doc, base) =
            prepared(r#"{"$deps": {"base": "https://cdn.example.com/x", "files": []}}"#);
        assert_eq!(base.as_deref(), Some("https://cdn.example.com/x/"));
        assert_eq!(doc["$deps"]["base"], "https://cdn.example.com/x/");
    }

    #[test]
    fn prepare_rejects_a_variable_base() {
        // clone 期没有变量表：`${BASE}` 求不出值，猜一个写死只会更糟
        for content in [
            r#"{"$deps": {"base": "${BASE}", "files": ["a"]}}"#,
            r#"{"$deps": {"base": "${BASE}/x/", "files": ["a"]}}"#,
            r#"{"$var": {"B": "https://e.com/x/"}, "$deps": {"base": "${B}", "files": ["a"]}}"#,
        ] {
            let err = prepare(content.as_bytes(), &remote()).unwrap_err();
            assert!(
                err.contains("must not contain variables"),
                "{content}: {err}"
            );
        }
    }

    #[test]
    fn prepare_refuses_values_it_cannot_rewrite() {
        for bad in ["~/x", "ftp://e.com/x", ""] {
            let content = format!(r#"{{"$deps": {{"base": "{bad}", "files": []}}}}"#);
            let err = prepare(content.as_bytes(), &remote()).unwrap_err();
            assert!(err.contains("$deps.base"), "{bad}: {err}");
        }
    }

    // ---------- prepare：本地 base 冻结 ----------

    fn local(dir: &std::path::Path) -> Origin {
        Origin::File(dir.join("dev.byk.json"))
    }

    fn slash(path: &std::path::Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn prepare_local_fills_the_source_directory() {
        let temp = tempfile::tempdir().unwrap();
        let (doc, base) = prepared_from(r#"{"$deps": {"files": ["./tool"]}}"#, &local(temp.path()));
        let want = format!("{}/", slash(temp.path()));

        assert_eq!(base.as_deref(), Some(want.as_str()));
        assert_eq!(doc["$deps"]["base"], want);
        // 声明保持原样：相对写法留在文件里
        assert_eq!(doc["$deps"]["files"][0], "./tool");
    }

    #[test]
    fn prepare_local_resolves_a_relative_base() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("proj");
        let (doc, _) = prepared_from(
            r#"{"$deps": {"base": "..", "files": ["x"]}}"#,
            &local(&project),
        );

        assert_eq!(doc["$deps"]["base"], format!("{}/", slash(temp.path())));
    }

    #[test]
    fn prepare_local_keeps_absolute_bases() {
        let temp = tempfile::tempdir().unwrap();
        let origin = local(temp.path());

        // 绝对 URL 照搬：本地 clone 也能引用远程资产
        let (doc, base) = prepared_from(
            r#"{"$deps": {"base": "https://cdn.example.com/x/", "files": ["a"]}}"#,
            &origin,
        );
        assert_eq!(base, None);
        assert_eq!(doc["$deps"]["base"], "https://cdn.example.com/x/");

        // 绝对路径照搬，只补结尾的 /
        let shared = temp.path().join("shared");
        let text = slash(&shared);
        let content = format!(r#"{{"$deps": {{"base": "{text}", "files": ["a"]}}}}"#);
        let (doc, base) = prepared_from(&content, &origin);
        assert_eq!(base.as_deref(), Some(format!("{text}/").as_str()));
        assert_eq!(doc["$deps"]["base"], format!("{text}/"));
    }

    #[test]
    fn prepare_local_rejects_a_variable_base() {
        let temp = tempfile::tempdir().unwrap();
        let err = prepare(
            r#"{"$deps": {"base": "${DIR}", "files": ["a"]}}"#.as_bytes(),
            &local(temp.path()),
        )
        .unwrap_err();
        assert!(err.contains("must not contain variables"), "{err}");
    }

    #[test]
    fn local_clone_without_deps_stays_byte_exact() {
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

    // ---------- prepare：$clone 裁剪 ----------

    fn strip(content: &str) -> Prepared {
        prepare(content.as_bytes(), &remote()).unwrap()
    }

    #[test]
    fn prepare_drops_marked_aliases_and_all_tags() {
        let prepared =
            strip(r#"{"keep": "echo keep", "mine": {"$clone": false, "$cmd": "echo mine"}}"#);
        let doc: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(doc, json!({"keep": "echo keep"}));
        assert_eq!(prepared.skipped, 1);
    }

    #[test]
    fn prepare_keeps_the_group_skeleton_for_reexposed_children() {
        let prepared = strip(
            r#"{
                "$clone": false,
                "mine": "echo mine",
                "group": {
                    "$clone": false,
                    "hidden": "echo hidden",
                    "shared": {"$clone": true, "run": "echo run"}
                }
            }"#,
        );
        let doc: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(doc, json!({"group": {"shared": {"run": "echo run"}}}));
        assert_eq!(prepared.skipped, 2);
    }

    #[test]
    fn prepare_removes_a_true_tag_without_counting_it_as_skipped() {
        let prepared = strip(r#"{"$clone": true, "build": "echo build"}"#);
        let doc: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(doc, json!({"build": "echo build"}));
        assert_eq!(prepared.skipped, 0);
        assert_eq!(prepared.base, None);
    }

    #[test]
    fn prepare_refuses_when_every_alias_is_marked() {
        for content in [
            r#"{"$clone": false, "mine": "echo mine"}"#,
            r#"{"mine": {"$clone": false, "$cmd": "echo mine"}}"#,
            r#"{"group": {"$clone": false, "mine": "echo mine"}}"#,
        ] {
            let err = prepare(content.as_bytes(), &remote()).unwrap_err();
            assert_eq!(
                err, "Nothing to clone: every alias is marked $clone: false",
                "{content}"
            );
        }
    }

    #[test]
    fn prepare_still_clones_an_empty_document() {
        // 没写过 $clone 的空文件维持原行为：clone 出同样的字节副本
        let prepared = prepare(b"{}", &remote()).unwrap();
        assert_eq!(prepared.body, b"{}");
    }

    #[test]
    fn prepare_keeps_unmarked_empty_groups() {
        // 只有被标记过、或子级被剔除过的空分组才剪掉
        let content = br#"{"group": {}, "keep": "echo keep"}"#;
        let prepared = prepare(content, &remote()).unwrap();
        assert_eq!(prepared.body, content);
    }

    #[test]
    fn prepare_strips_and_freezes_the_base_in_one_write() {
        let prepared = strip(
            r#"{
                "$clone": false,
                "mine": "echo mine",
                "keep": {"$cmd": "echo keep", "$clone": true},
                "$deps": {"files": ["https://e.com/tool"]}
            }"#,
        );
        let doc: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert!(doc.get("mine").is_none());
        assert_eq!(doc["keep"]["$cmd"], "echo keep");
        assert!(doc["keep"].get("$clone").is_none());
        assert_eq!(
            doc["$deps"]["base"],
            "https://hello.com/sdsd/asfdsg/qwe/we/"
        );
        assert_eq!(prepared.skipped, 1);
    }

    #[test]
    fn prepare_strips_local_sources_the_same_way() {
        let temp = tempfile::tempdir().unwrap();
        let prepared = prepare(
            br#"{"keep": "echo keep", "mine": {"$clone": false, "$cmd": "echo mine"}}"#,
            &local(temp.path()),
        )
        .unwrap();
        let doc: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(doc, json!({"keep": "echo keep"}));
    }

    #[test]
    fn run_refuses_when_nothing_is_cloneable() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let source = source_file(
            temp.path(),
            "src.byk.json",
            r#"{"$clone": false, "mine": "echo mine"}"#,
        );

        let err = run(
            FileScope::Global,
            "ops",
            &args(&[source.to_str().unwrap()]),
            &layout,
        )
        .unwrap_err();

        assert_eq!(err, "Nothing to clone: every alias is marked $clone: false");
        assert!(!layout.alias_dir.join("ops.byk.json").exists());
    }
}
