//! `..json`：输出别名文件；`--expand` 展开文件级变量。

use std::fs;
use std::io;

use colored::Colorize;
use serde_json::Value;

use super::{FileScope, not_found_message, resolve_target};
use crate::core::config::{FILE_DIR_VAR, build_builtin_table, describe_issue, preprocess_vars};
use crate::core::paths::PathLayout;
use crate::utils::path::dir_of;

pub(super) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<Vec<u8>, String> {
    let mode = parse_args(args)?;
    let target = resolve_target(scope, stem, layout)?;

    // 读操作跟随符号链接；目录和其他特殊文件不读取，避免阻塞或输出无关内容。
    let metadata = match fs::metadata(&target) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(not_found_message(scope, stem, &target, layout, "json"));
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", target.display(), e)),
    };

    if metadata.is_dir() {
        return Err(format!("Refusing to read directory: {}", target.display()));
    }
    if !metadata.is_file() {
        return Err(format!("Not a regular file: {}", target.display()));
    }

    let content =
        fs::read(&target).map_err(|e| format!("Failed to read {}: {}", target.display(), e))?;

    // 不校验模式：字节原样输出，连非法 UTF-8 也不拦。
    if matches!(mode, Mode::NoCheck) {
        return Ok(content);
    }

    let mut value: Value = serde_json::from_slice(&content)
        .map_err(|e| format!("Invalid JSON in {}: {}", target.display(), e))?;

    // 校验模式只解析、不重新序列化：输出精确的原始字节。
    if matches!(mode, Mode::Checked) {
        return Ok(content);
    }

    let mut vars = build_builtin_table(&layout.root_dir);
    vars.insert(FILE_DIR_VAR.to_string(), dir_of(&target));

    for issue in preprocess_vars(&mut value, &vars) {
        eprintln!(
            "{} {} in {}",
            "Warning:".yellow(),
            describe_issue(&issue),
            target.display()
        );
    }

    // $var 已完成使命：展开后继续保留只会暴露过时信息。
    if let Some(obj) = value.as_object_mut() {
        obj.remove("$var");
    }

    let mut output = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("Failed to render expanded JSON: {}", e))?
        .into_bytes();
    output.push(b'\n');
    Ok(output)
}

#[derive(Clone, Copy)]
enum Mode {
    Checked,
    NoCheck,
    Expand,
}

fn parse_args(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Checked),
        [flag] if flag == "--expand" => Ok(Mode::Expand),
        [flag] if flag == "--no-check" => Ok(Mode::NoCheck),
        _ if args.iter().any(|arg| arg == "--expand")
            && args.iter().any(|arg| arg == "--no-check") =>
        {
            Err("--expand and --no-check cannot be combined".to_string())
        }
        _ => Err("json accepts only --expand or --no-check".to_string()),
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

    fn write_file(layout: &PathLayout, stem: &str, content: &str) -> std::path::PathBuf {
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let path = layout.alias_dir.join(format!("{}.byk.json", stem));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn outputs_exact_raw_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let raw = "{\n  \"z\": \"echo z\",\n  \"a\": \"echo a\"\n}";
        write_file(&layout, "ops", raw);

        let output = run(FileScope::Global, "ops", &[], &layout).unwrap();

        assert_eq!(output, raw.as_bytes());
    }

    #[test]
    fn keeps_existing_trailing_newline() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let raw = "{\"a\": \"echo a\"}\n";
        write_file(&layout, "ops", raw);

        let output = run(FileScope::Global, "ops", &[], &layout).unwrap();

        assert_eq!(output, raw.as_bytes());
    }

    #[test]
    fn link_file_is_output_as_is() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let raw = "[\"shared.byk.json\"]\n";
        write_file(&layout, "ops", raw);

        let output = run(FileScope::Global, "ops", &[], &layout).unwrap();

        assert_eq!(output, raw.as_bytes());
    }

    #[test]
    fn expand_substitutes_vars_and_removes_var_block() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let raw = r#"{"$var":{"TOOLS":"${byk_dir}/tools"},"build":"cd ${file_dir} && ${TOOLS}/build.sh"}"#;
        write_file(&layout, "ops", raw);

        let output = run(FileScope::Global, "ops", &["--expand".to_string()], &layout).unwrap();
        let value: Value = serde_json::from_slice(&output).unwrap();

        assert!(value.get("$var").is_none());
        let expected = format!(
            "cd {} && {}/tools/build.sh",
            layout.alias_dir.display(),
            layout.root_dir.display()
        );
        assert_eq!(value["build"], serde_json::json!(expected));
        assert!(output.ends_with(b"\n"));
    }

    #[test]
    fn expand_works_for_link_file_without_following() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let raw = r#"["${byk_dir}/shared.byk.json"]"#;
        write_file(&layout, "ops", raw);

        let output = run(FileScope::Global, "ops", &["--expand".to_string()], &layout).unwrap();
        let value: Value = serde_json::from_slice(&output).unwrap();

        assert_eq!(
            value[0],
            serde_json::json!(format!("{}/shared.byk.json", layout.root_dir.display()))
        );
    }

    #[test]
    fn invalid_json_is_an_error_with_location() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        write_file(&layout, "ops", "{not json");

        let err = run(FileScope::Global, "ops", &[], &layout).unwrap_err();

        assert!(err.starts_with("Invalid JSON in "));
        assert!(err.contains("line 1"));
    }

    #[test]
    fn missing_file_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(FileScope::Global, "ops", &[], &layout).unwrap_err();

        assert!(err.starts_with("File not found:"));
    }

    #[test]
    fn directory_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let path = layout.alias_dir.join("ops.byk.json");
        fs::create_dir_all(&path).unwrap();

        let err = run(FileScope::Global, "ops", &[], &layout).unwrap_err();

        assert!(err.contains("Refusing to read directory"));
    }

    #[test]
    fn no_check_outputs_invalid_json_as_is() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        let raw = "{not json";
        write_file(&layout, "ops", raw);

        let output = run(
            FileScope::Global,
            "ops",
            &["--no-check".to_string()],
            &layout,
        )
        .unwrap();

        assert_eq!(output, raw.as_bytes());
    }

    #[test]
    fn no_check_outputs_non_utf8_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let raw = vec![0xff, 0xfe, b'{'];
        fs::write(layout.alias_dir.join("ops.byk.json"), &raw).unwrap();

        let output = run(
            FileScope::Global,
            "ops",
            &["--no-check".to_string()],
            &layout,
        )
        .unwrap();

        assert_eq!(output, raw);
    }

    #[test]
    fn expand_and_no_check_cannot_be_combined() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(
            FileScope::Global,
            "ops",
            &["--expand".to_string(), "--no-check".to_string()],
            &layout,
        )
        .unwrap_err();

        assert_eq!(err, "--expand and --no-check cannot be combined");
    }

    #[test]
    fn arguments_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);

        let err = run(FileScope::Global, "ops", &["--pretty".to_string()], &layout).unwrap_err();

        assert_eq!(err, "json accepts only --expand or --no-check");
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlink_for_reading() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let layout = test_layout(&temp);
        fs::create_dir_all(&layout.alias_dir).unwrap();
        let real = temp.path().join("real.byk.json");
        fs::write(&real, "{\"a\": \"echo a\"}\n").unwrap();
        let link = layout.alias_dir.join("ops.byk.json");
        symlink(&real, &link).unwrap();

        let output = run(FileScope::Global, "ops", &[], &layout).unwrap();

        assert_eq!(output, b"{\"a\": \"echo a\"}\n");
    }
}
