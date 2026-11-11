//! `..remove`：删除当前文件中的一个别名或整个分组。

use std::path::PathBuf;

use serde_json::{Map, Value};

use super::{FileScope, edit, key};
use crate::core::paths::PathLayout;

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(PathBuf, String), String> {
    let key = parse_args(args)?;

    edit::object(scope, stem, layout, None, "remove", |object| {
        let segments: Vec<&str> = key.split('.').collect();
        remove_in(object, &segments, &key)?;
        Ok(key.clone())
    })
}

fn parse_args(args: &[String]) -> Result<String, String> {
    match args {
        [] => Err("remove requires an alias key".to_string()),
        [key] => {
            key::validate(key)?;
            Ok(key.clone())
        }
        _ => Err("remove takes exactly one key".to_string()),
    }
}

/// 返回当前对象在删除后是否已成为可剪掉的分组。
fn remove_in(
    object: &mut Map<String, Value>,
    segments: &[&str],
    key: &str,
) -> Result<bool, String> {
    let (segment, rest) = segments
        .split_first()
        .expect("alias key has at least one segment");

    if rest.is_empty() {
        if object.remove(*segment).is_none() {
            return Err(format!("Alias not found: {}", key));
        }
        return Ok(prunable(object));
    }

    let child = match object.get_mut(*segment) {
        Some(Value::Object(child)) => child,
        Some(_) => return Err(format!("Cannot remove below non-object alias: {}", segment)),
        None => return Err(format!("Alias not found: {}", key)),
    };
    if remove_in(child, rest, key)? {
        object.remove(*segment);
    }
    Ok(prunable(object))
}

/// 没有别名值、只剩（或没有）`$` 元数据的对象视为空分组。
fn prunable(object: &Map<String, Value>) -> bool {
    !object.contains_key("$cmd") && object.keys().all(|key| key.starts_with('$'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join(".byk").join("alias"),
        }
    }

    fn prepare(layout: &PathLayout, content: &str) -> PathBuf {
        std::fs::create_dir_all(&layout.alias_dir).unwrap();
        let path = layout.alias_dir.join("ops.byk.json");
        std::fs::write(&path, content).unwrap();
        path
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn read(path: &PathBuf) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn run_ok(layout: &PathLayout, values: &[&str]) -> Value {
        run(FileScope::Global, "ops", &args(values), layout).unwrap();
        read(&layout.alias_dir.join("ops.byk.json"))
    }

    #[test]
    fn removes_leaf_alias() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"build":"echo hi","test":"echo test"}"#);

        let value = run_ok(&layout, &["build"]);

        assert_eq!(value, json!({"test": "echo test"}));
    }

    #[test]
    fn removes_nested_alias_and_prunes_empty_group() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":{"deploy":"echo hi"}}"#);

        let value = run_ok(&layout, &["web.deploy"]);

        assert_eq!(value, json!({}));
    }

    #[test]
    fn prunes_group_metadata_with_empty_group() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":{"$cwd":".","deploy":"echo hi"}}"#);

        let value = run_ok(&layout, &["web.deploy"]);

        assert_eq!(value, json!({}));
    }

    #[test]
    fn keeps_parent_with_command() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":{"$cmd":"echo web","deploy":"echo hi"}}"#);

        let value = run_ok(&layout, &["web.deploy"]);

        assert_eq!(value, json!({"web": {"$cmd": "echo web"}}));
    }

    #[test]
    fn removes_whole_group() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(
            &layout,
            r#"{"web":{"$cwd":".","deploy":"echo hi","test":"echo test"},"build":"echo build"}"#,
        );

        let value = run_ok(&layout, &["web"]);

        assert_eq!(value, json!({"build": "echo build"}));
    }

    #[test]
    fn removes_alias_object_with_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(
            &layout,
            r#"{"build":{"$cmd":"echo hi","$description":"keep"},"test":"echo test"}"#,
        );

        let value = run_ok(&layout, &["build"]);

        assert_eq!(value, json!({"test": "echo test"}));
    }

    #[test]
    fn missing_alias_errors() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{}");

        let err = run(FileScope::Global, "ops", &args(&["build"]), &layout).unwrap_err();

        assert_eq!(err, "Alias not found: build");
    }

    #[test]
    fn missing_intermediate_errors() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, "{}");

        let err = run(FileScope::Global, "ops", &args(&["web.deploy"]), &layout).unwrap_err();

        assert_eq!(err, "Alias not found: web.deploy");
    }

    #[test]
    fn refuses_to_traverse_non_object() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"{"web":"echo hi"}"#);

        let err = run(FileScope::Global, "ops", &args(&["web.deploy"]), &layout).unwrap_err();

        assert_eq!(err, "Cannot remove below non-object alias: web");
    }

    #[test]
    fn refuses_link_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        prepare(&layout, r#"["shared.byk.json"]"#);

        let err = run(FileScope::Global, "ops", &args(&["build"]), &layout).unwrap_err();

        assert!(err.starts_with("Cannot remove aliases in a link file:"));
    }

    #[test]
    fn rejects_metadata_key_with_unset_hint() {
        let err = parse_args(&args(&["web.$cwd"])).unwrap_err();

        assert!(err.contains("is a metadata field"));
        assert!(err.contains("--unset"));
    }

    #[test]
    fn parse_args_rejects_missing_and_extra_keys() {
        assert_eq!(parse_args(&[]).unwrap_err(), "remove requires an alias key");
        assert_eq!(
            parse_args(&args(&["a", "b"])).unwrap_err(),
            "remove takes exactly one key"
        );
    }

    #[test]
    fn missing_file_has_no_init_hint() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(FileScope::Global, "ops", &args(&["build"]), &layout).unwrap_err();

        assert!(err.starts_with("File not found:"));
        assert!(!err.contains("Hint:"));
    }

    #[test]
    fn rejects_invalid_json() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = prepare(&layout, "{not json");

        let err = run(FileScope::Global, "ops", &args(&["build"]), &layout).unwrap_err();

        assert!(err.starts_with("Failed to parse"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        std::fs::create_dir_all(&layout.alias_dir).unwrap();
        let real = temp.path().join("real.byk.json");
        std::fs::write(&real, "{}").unwrap();
        let link = layout.alias_dir.join("ops.byk.json");
        symlink(&real, &link).unwrap();

        let err = run(FileScope::Global, "ops", &args(&["build"]), &layout).unwrap_err();

        assert!(err.starts_with("Refusing to rewrite symlink:"));
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "{}");
    }

    #[test]
    fn refuses_directory() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        std::fs::create_dir_all(layout.alias_dir.join("ops.byk.json")).unwrap();

        let err = run(FileScope::Global, "ops", &args(&["build"]), &layout).unwrap_err();

        assert!(err.starts_with("Refusing to edit directory:"));
    }
}
