//! `path`：输出目标文件的绝对路径。

use crate::filecmd::io::path::resolve_target;
use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;

pub(crate) fn run(
    scope: FileScope,
    stem: &str,
    args: &[String],
    layout: &PathLayout,
) -> Result<String, String> {
    if !args.is_empty() {
        return Err("path takes no arguments".to_string());
    }
    let path = resolve_target(scope, stem, layout)?;
    Ok(format!("{}\n", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PathLayout;
    use std::path::PathBuf;

    fn layout() -> PathLayout {
        PathLayout {
            root_dir: PathBuf::from("/home/me/.byk"),
            alias_dir: PathBuf::from("/home/me/.byk/alias"),
        }
    }

    #[test]
    fn global_path() {
        let layout = layout();
        let expected = format!("{}\n", layout.alias_dir.join("ops.byk.json").display());
        assert_eq!(
            run(FileScope::Global, "ops", &[], &layout).unwrap(),
            expected
        );
    }

    #[test]
    fn anonymous_path() {
        let layout = layout();
        let expected = format!("{}\n", layout.alias_dir.join(".byk.json").display());
        assert_eq!(run(FileScope::Global, "", &[], &layout).unwrap(), expected);
    }

    #[test]
    fn rejects_arguments() {
        assert_eq!(
            run(FileScope::Global, "ops", &["extra".to_string()], &layout()).unwrap_err(),
            "path takes no arguments"
        );
    }
}
