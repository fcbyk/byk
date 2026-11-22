//! `link` 的入口参数解析。

use std::path::PathBuf;

use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;

pub(crate) use imp::content;

pub(crate) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<(PathBuf, PathBuf), String> {
    imp::apply(scope, stem, parse_args(args)?, layout)
}

fn parse_args(args: &[String]) -> Result<&str, String> {
    let [spec] = args else {
        return Err("link takes exactly one target".to_string());
    };
    Ok(spec)
}

mod imp {
    //! `link`：把当前文件改写为指向另一个别名文件的链接。

    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    use crate::filecmd::io::path::resolve_target;
    use crate::filecmd::source;
    use crate::filecmd::spec::FileScope;
    use crate::model::PathLayout;
    use crate::utils::json;
    use crate::utils::path::canonical;

    pub(crate) fn apply(
        scope: FileScope,
        stem: &str,
        spec: &str,
        layout: &PathLayout,
    ) -> Result<(PathBuf, PathBuf), String> {
        let target = source::resolve(spec, layout)?;

        let path = resolve_target(scope, stem, layout)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let prefix = match scope {
                    FileScope::Local => "@",
                    FileScope::Global => "@@",
                };
                return Err(format!(
                    "File not found: {}\nHint: create it with {}{} init --link {}",
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

        let resolved = canonical(&path);
        if resolved == target {
            return Err(format!("Link cannot point to itself: {}", path.display()));
        }

        let current = json::read_value(&path)?;
        let convertible = matches!(&current, Value::Array(_))
            || matches!(&current, Value::Object(map) if map.is_empty());
        if !convertible {
            return Err(format!(
                "Not a link: {}\nHint: link works on a link file or an empty `{{}}` file",
                path.display()
            ));
        }

        let content = content(&target)?;
        json::write_text(&path, &content)?;

        Ok((path, target))
    }

    /// 生成链接文件内容：单元素数组。
    pub(crate) fn content(target: &Path) -> Result<String, String> {
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
        fn run_retargets_existing_link() {
            let temp = tempfile::tempdir().unwrap();
            let layout = layout(&temp);
            fs::create_dir_all(&layout.alias_dir).unwrap();
            let link_path = layout.alias_dir.join("tools.byk.json");
            fs::write(&link_path, r#"["/old/tool.byk.json"]"#).unwrap();
            let target = object_file(temp.path(), "new.byk.json");

            let (path, linked) =
                apply(FileScope::Global, "tools", &spec(&target), &layout).unwrap();

            assert_eq!(path, link_path);
            assert_eq!(linked, canonical(&target));
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

            apply(FileScope::Global, "tools", &spec(&target), &layout).unwrap();

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

            let err = apply(FileScope::Global, "tools", &spec(&target), &layout).unwrap_err();

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

            let err = apply(FileScope::Global, "tools", &spec(&target), &layout).unwrap_err();

            assert!(err.starts_with("File not found:"));
            assert!(err.contains("@@tools init --link"));
        }

        #[test]
        fn run_rejects_self_link() {
            let temp = tempfile::tempdir().unwrap();
            let layout = layout(&temp);
            fs::create_dir_all(&layout.alias_dir).unwrap();
            let link_path = layout.alias_dir.join("tools.byk.json");
            fs::write(&link_path, "{}\n").unwrap();

            let err = apply(FileScope::Global, "tools", &spec(&link_path), &layout).unwrap_err();

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

            let err = apply(FileScope::Global, "tools", &spec(&target), &layout).unwrap_err();

            assert!(err.starts_with("Refusing to rewrite symlink:"));
            assert_eq!(fs::read_to_string(&real).unwrap(), "{}\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_one_target() {
        assert_eq!(parse_args(&["@tool".to_string()]).unwrap(), "@tool");
    }

    #[test]
    fn rejects_missing_and_extra_targets() {
        assert_eq!(
            parse_args(&[]).unwrap_err(),
            "link takes exactly one target"
        );
        assert_eq!(
            parse_args(&["a".to_string(), "b".to_string()]).unwrap_err(),
            "link takes exactly one target"
        );
    }
}
