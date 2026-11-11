//! `..set`：在当前文件中创建或更新一个别名。

use std::path::PathBuf;

use serde_json::{Map, Value};

use super::{FileScope, edit, key};
use crate::core::aliases::Mode;
use crate::core::paths::PathLayout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnsetField {
    Cwd,
    Mode,
    Description,
    Recursive,
}

#[derive(Debug)]
struct SetArgs {
    key: String,
    cmd: Option<String>,
    cwd: Option<String>,
    mode: Option<Mode>,
    description: Option<String>,
    recursive: Option<bool>,
    unset: Vec<UnsetField>,
}

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(PathBuf, String), String> {
    let parsed = parse_args(args)?;
    let prefix = match scope {
        FileScope::Local => "@",
        FileScope::Global => "@@",
    };
    let hint = format!("Hint: create it with {}{}..init", prefix, stem);

    edit::object(scope, stem, layout, Some(hint), "set", |object| {
        let segments: Vec<&str> = parsed.key.split('.').collect();
        set_path(object, &segments, &parsed)?;
        Ok(parsed.key.clone())
    })
}

fn parse_args(args: &[String]) -> Result<SetArgs, String> {
    let Some(key) = args.first() else {
        return Err("set requires an alias key".to_string());
    };
    key::validate(key)?;

    let mut parsed = SetArgs {
        key: key.clone(),
        cmd: None,
        cwd: None,
        mode: None,
        description: None,
        recursive: None,
        unset: Vec::new(),
    };

    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--cmd" => {
                set_text(&mut parsed.cmd, "cmd", next_value(&mut rest, "--cmd")?)?;
            }
            "--cwd" => {
                set_text(&mut parsed.cwd, "cwd", next_value(&mut rest, "--cwd")?)?;
            }
            "--description" => {
                set_text(
                    &mut parsed.description,
                    "description",
                    next_value(&mut rest, "--description")?,
                )?;
            }
            "--mode" => {
                let raw = next_value(&mut rest, "--mode")?;
                if parsed.mode.is_some() {
                    return Err("Duplicate option: --mode".to_string());
                }
                parsed.mode = Some(parse_mode(raw)?);
            }
            "--recursive" => {
                if parsed.recursive.is_some() {
                    return Err("Duplicate option: --recursive".to_string());
                }
                parsed.recursive = Some(true);
            }
            "--no-recursive" => {
                if parsed.recursive.is_some() {
                    return Err("Duplicate option: --no-recursive".to_string());
                }
                parsed.recursive = Some(false);
            }
            "--unset" => {
                let raw = next_value(&mut rest, "--unset")?;
                for field in parse_unset(raw)? {
                    if !parsed.unset.contains(&field) {
                        parsed.unset.push(field);
                    }
                }
            }
            other => {
                return Err(format!(
                    "Unknown option: {}\nHint: quote commands containing spaces",
                    other
                ));
            }
        }
    }

    validate_conflicts(&parsed)?;
    if parsed.cmd.is_none()
        && parsed.cwd.is_none()
        && parsed.mode.is_none()
        && parsed.description.is_none()
        && parsed.recursive.is_none()
        && parsed.unset.is_empty()
    {
        return Err("set requires --cmd or a metadata change".to_string());
    }

    Ok(parsed)
}

fn next_value<'a>(rest: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a str, String> {
    rest.next()
        .map(String::as_str)
        .ok_or_else(|| format!("Option {} requires a value", flag))
}

fn set_text(slot: &mut Option<String>, name: &str, value: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("Duplicate option: --{}", name));
    }
    if value.is_empty() {
        return Err(format!("Option --{} requires a non-empty value", name));
    }
    *slot = Some(value.to_string());
    Ok(())
}

fn parse_mode(raw: &str) -> Result<Mode, String> {
    match raw {
        "normal" => Ok(Mode::Normal),
        "interactive" => Ok(Mode::Interactive),
        "silent" => Ok(Mode::Silent),
        _ => Err(format!(
            "Invalid mode: {:?}\nAvailable: normal, interactive, silent",
            raw
        )),
    }
}

fn parse_unset(raw: &str) -> Result<Vec<UnsetField>, String> {
    let mut fields = Vec::new();
    for part in raw.split(',') {
        let field = match part.trim() {
            "cwd" => UnsetField::Cwd,
            "mode" => UnsetField::Mode,
            "description" => UnsetField::Description,
            "recursive" => UnsetField::Recursive,
            "" => return Err("Option --unset requires a field".to_string()),
            other => {
                return Err(format!(
                    "Invalid unset field: {:?}\nAvailable: cwd, mode, description, recursive",
                    other
                ));
            }
        };
        if !fields.contains(&field) {
            fields.push(field);
        }
    }
    Ok(fields)
}

fn validate_conflicts(parsed: &SetArgs) -> Result<(), String> {
    for field in &parsed.unset {
        let conflict = match field {
            UnsetField::Cwd => parsed.cwd.is_some(),
            UnsetField::Mode => parsed.mode.is_some(),
            UnsetField::Description => parsed.description.is_some(),
            UnsetField::Recursive => parsed.recursive.is_some(),
        };
        if conflict {
            return Err("Cannot set and unset the same field".to_string());
        }
    }
    Ok(())
}

fn set_path(
    object: &mut Map<String, Value>,
    segments: &[&str],
    args: &SetArgs,
) -> Result<(), String> {
    let (segment, rest) = segments
        .split_first()
        .expect("alias key has at least one segment");

    if rest.is_empty() {
        return set_node(object, segment, args);
    }

    let entry = object
        .entry((*segment).to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    match entry {
        Value::Object(child) => set_path(child, rest, args),
        _ => Err(format!(
            "Cannot create group under non-object alias: {}",
            segment
        )),
    }
}

fn set_node(object: &mut Map<String, Value>, key: &str, args: &SetArgs) -> Result<(), String> {
    match object.get_mut(key) {
        None => {
            let Some(cmd) = &args.cmd else {
                return Err("set requires --cmd when creating a new alias".to_string());
            };
            let wants_object = args.cwd.is_some()
                || args.mode.is_some()
                || args.description.is_some()
                || args.recursive == Some(true);
            if wants_object {
                let mut node = Map::new();
                node.insert("$cmd".to_string(), Value::String(cmd.clone()));
                apply_metadata(&mut node, args, true)?;
                object.insert(key.to_string(), Value::Object(node));
            } else {
                object.insert(key.to_string(), Value::String(cmd.clone()));
            }
        }
        Some(value) => match value {
            Value::String(old) => {
                let has_metadata = args.cwd.is_some()
                    || args.mode.is_some()
                    || args.description.is_some()
                    || args.recursive.is_some()
                    || !args.unset.is_empty();
                if !has_metadata {
                    *value = Value::String(args.cmd.clone().expect("validated change"));
                    return Ok(());
                }
                if args.cmd.is_none()
                    && args.cwd.is_none()
                    && args.mode.is_none()
                    && args.description.is_none()
                    && args.recursive.is_none()
                {
                    return Err("String aliases have no metadata to unset".to_string());
                }
                let cmd = args.cmd.clone().unwrap_or_else(|| old.clone());
                let mut node = Map::new();
                node.insert("$cmd".to_string(), Value::String(cmd));
                apply_metadata(&mut node, args, true)?;
                *value = collapse(node);
            }
            Value::Object(node) => {
                apply_to_object(node, args)?;
                let taken = std::mem::take(node);
                *value = collapse(taken);
            }
            _ => {
                return Err(format!("Cannot set alias at non-object value: {}", key));
            }
        },
    }
    Ok(())
}

fn apply_to_object(node: &mut Map<String, Value>, args: &SetArgs) -> Result<(), String> {
    match (&args.cmd, node.get("$cmd")) {
        (Some(cmd), _) => {
            node.insert("$cmd".to_string(), Value::String(cmd.clone()));
        }
        (None, Some(Value::String(_))) | (None, None) => {}
        (None, Some(_)) => {
            return Err("Existing $cmd is not a string; pass --cmd to replace it".to_string());
        }
    }

    let has_command = matches!(node.get("$cmd"), Some(Value::String(_)));
    apply_metadata(node, args, has_command)
}

fn apply_metadata(
    node: &mut Map<String, Value>,
    args: &SetArgs,
    has_command: bool,
) -> Result<(), String> {
    if !has_command && (args.description.is_some() || args.recursive.is_some()) {
        return Err("$description and $recursive require a command; pass --cmd".to_string());
    }

    if let Some(cwd) = &args.cwd {
        node.insert("$cwd".to_string(), Value::String(cwd.clone()));
    }
    if let Some(mode) = args.mode {
        node.insert(
            "$mode".to_string(),
            Value::String(mode.as_str().to_string()),
        );
    }
    if let Some(description) = &args.description {
        node.insert(
            "$description".to_string(),
            Value::String(description.clone()),
        );
    }
    if let Some(recursive) = args.recursive {
        if recursive {
            node.insert("$recursive".to_string(), Value::Bool(true));
        } else {
            node.remove("$recursive");
        }
    }
    for field in &args.unset {
        let key = match field {
            UnsetField::Cwd => "$cwd",
            UnsetField::Mode => "$mode",
            UnsetField::Description => "$description",
            UnsetField::Recursive => "$recursive",
        };
        node.remove(key);
    }

    Ok(())
}

/// 只剩 `$cmd` 的对象折叠回字符串，保持文件简洁。
fn collapse(node: Map<String, Value>) -> Value {
    if node.len() == 1
        && let Some(Value::String(cmd)) = node.get("$cmd")
    {
        return Value::String(cmd.clone());
    }
    Value::Object(node)
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

    fn prepare(layout: &PathLayout, content: &str) -> PathBuf {
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let path = layout.alias_dir.join("ops.byk.json");
        fs::write(&path, content).unwrap();
        path
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn read(path: &PathBuf) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn run_ok(layout: &PathLayout, values: &[&str]) -> Value {
        run(FileScope::Global, "ops", &args(values), layout).unwrap();
        read(&layout.alias_dir.join("ops.byk.json"))
    }

    #[test]
    fn creates_string_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{}\n");

        let value = run_ok(&layout, &["build", "--cmd", "echo hi"]);

        assert_eq!(value["build"], json!("echo hi"));
    }

    #[test]
    fn creates_nested_groups() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{}\n");

        let value = run_ok(&layout, &["web.deploy", "--cmd", "echo hi"]);

        assert_eq!(value["web"]["deploy"], json!("echo hi"));
    }

    #[test]
    fn creates_meta_object() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{}\n");

        let value = run_ok(
            &layout,
            &[
                "build",
                "--cmd",
                "echo hi",
                "--cwd",
                "./web",
                "--mode",
                "silent",
                "--description",
                "Build it",
                "--recursive",
            ],
        );

        assert_eq!(value["build"]["$cmd"], json!("echo hi"));
        assert_eq!(value["build"]["$cwd"], json!("./web"));
        assert_eq!(value["build"]["$mode"], json!("silent"));
        assert_eq!(value["build"]["$description"], json!("Build it"));
        assert_eq!(value["build"]["$recursive"], json!(true));
    }

    #[test]
    fn updates_string_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"build":"echo old"}"#);

        let value = run_ok(&layout, &["build", "--cmd", "echo new"]);

        assert_eq!(value["build"], json!("echo new"));
    }

    #[test]
    fn keeps_metadata_when_updating_command() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(
            &layout,
            r#"{"build":{"$cmd":"echo old","$description":"keep"}}"#,
        );

        let value = run_ok(&layout, &["build", "--cmd", "echo new"]);

        assert_eq!(value["build"]["$cmd"], json!("echo new"));
        assert_eq!(value["build"]["$description"], json!("keep"));
    }

    #[test]
    fn converts_string_to_meta_with_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"build":"echo hi"}"#);

        let value = run_ok(&layout, &["build", "--cwd", "./web"]);

        assert_eq!(value["build"]["$cmd"], json!("echo hi"));
        assert_eq!(value["build"]["$cwd"], json!("./web"));
    }

    #[test]
    fn sets_group_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":{"deploy":"echo hi"}}"#);

        let value = run_ok(&layout, &["web", "--cwd", ".", "--mode", "interactive"]);

        assert_eq!(value["web"]["$cwd"], json!("."));
        assert_eq!(value["web"]["$mode"], json!("interactive"));
        assert_eq!(value["web"]["deploy"], json!("echo hi"));
    }

    #[test]
    fn group_rejects_description_without_command() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":{"deploy":"echo hi"}}"#);

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["web", "--description", "x"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.contains("require a command"));
    }

    #[test]
    fn refuses_to_create_under_string() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":"echo hi"}"#);

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["web.deploy", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Cannot create group under non-object alias:"));
    }

    #[test]
    fn refuses_non_alias_value() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"build":42}"#);

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Cannot set alias at non-object value:"));
    }

    #[test]
    fn unsets_metadata_and_collapses_to_string() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"build":{"$cmd":"echo hi","$mode":"silent"}}"#);

        let value = run_ok(&layout, &["build", "--unset", "mode"]);

        assert_eq!(value["build"], json!("echo hi"));
    }

    #[test]
    fn no_recursive_removes_field() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"build":{"$cmd":"echo hi","$recursive":true}}"#);

        let value = run_ok(&layout, &["build", "--no-recursive"]);

        assert_eq!(value["build"], json!("echo hi"));
    }

    #[test]
    fn new_alias_requires_command() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{}\n");

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cwd", "."]),
            &layout,
        )
        .unwrap_err();

        assert_eq!(err, "set requires --cmd when creating a new alias");
    }

    #[test]
    fn refuses_link_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"["shared.byk.json"]"#);

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Cannot set aliases in a link file:"));
    }

    #[test]
    fn rejects_invalid_json() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{not json");

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Failed to parse"));
    }

    #[test]
    fn missing_file_hints_init() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("File not found:"));
        assert!(err.contains("@@ops..init"));
    }

    #[test]
    fn refuses_directory() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = layout.alias_dir.join("ops.byk.json");
        fs::create_dir_all(&path).unwrap();

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Refusing to edit directory:"));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let real = temp.path().join("real.byk.json");
        fs::write(&real, "{}").unwrap();
        let link = layout.alias_dir.join("ops.byk.json");
        symlink(&real, &link).unwrap();

        let err = run(
            FileScope::Global,
            "ops",
            &args(&["build", "--cmd", "echo hi"]),
            &layout,
        )
        .unwrap_err();

        assert!(err.starts_with("Refusing to rewrite symlink:"));
        assert_eq!(fs::read_to_string(&real).unwrap(), "{}");
    }

    #[test]
    fn parse_args_requires_key() {
        let err = parse_args(&[]).unwrap_err();
        assert_eq!(err, "set requires an alias key");
    }

    #[test]
    fn parse_args_rejects_duplicate_flags() {
        let err = parse_args(&args(&["build", "--cmd", "a", "--cmd", "b"])).unwrap_err();
        assert!(err.starts_with("Duplicate option: --cmd"));

        let err =
            parse_args(&args(&["build", "--mode", "normal", "--mode", "silent"])).unwrap_err();
        assert!(err.starts_with("Duplicate option: --mode"));

        let err = parse_args(&args(&["build", "--recursive", "--no-recursive"])).unwrap_err();
        assert_eq!(err, "Duplicate option: --no-recursive");
    }

    #[test]
    fn parse_args_rejects_missing_value() {
        let err = parse_args(&args(&["build", "--cmd"])).unwrap_err();
        assert_eq!(err, "Option --cmd requires a value");
    }

    #[test]
    fn parse_args_rejects_unknown_option() {
        let err = parse_args(&args(&["build", "--bad"])).unwrap_err();
        assert!(err.starts_with("Unknown option: --bad"));
    }

    #[test]
    fn parse_args_rejects_invalid_key() {
        assert!(parse_args(&args(&["", "--cmd", "x"])).is_err());
        assert!(parse_args(&args(&["-build", "--cmd", "x"])).is_err());
        assert!(parse_args(&args(&["a..b", "--cmd", "x"])).is_err());
        assert!(parse_args(&args(&["a@b", "--cmd", "x"])).is_err());
        assert!(parse_args(&args(&["$priority", "--cmd", "x"])).is_err());
    }

    #[test]
    fn parse_args_rejects_set_and_unset_conflict() {
        let err = parse_args(&args(&["build", "--cwd", ".", "--unset", "cwd"])).unwrap_err();
        assert_eq!(err, "Cannot set and unset the same field");
    }

    #[test]
    fn parse_args_rejects_invalid_unset_field() {
        let err = parse_args(&args(&["build", "--unset", "bogus"])).unwrap_err();
        assert!(err.starts_with("Invalid unset field:"));
    }

    #[test]
    fn parse_args_rejects_invalid_mode() {
        let err = parse_args(&args(&["build", "--mode", "loud"])).unwrap_err();
        assert!(err.starts_with("Invalid mode:"));
    }

    #[test]
    fn parse_args_requires_a_change() {
        let err = parse_args(&args(&["build"])).unwrap_err();
        assert_eq!(err, "set requires --cmd or a metadata change");
    }
}
