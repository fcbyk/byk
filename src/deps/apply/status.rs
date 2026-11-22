//! `deps status` 的查询：声明、落点与当前状态。

use std::path::{Path, PathBuf};

use crate::deps::spec::plan_entries;
use crate::deps::types::Declaration;

use super::sync::{self, Change};
use super::{installed_pip_packages, npm_package_installed, pip_package_name};

/// 状态查询结果；渲染留给命令层。
pub(crate) struct StatusReport {
    pub(crate) key: String,
    pub(crate) deps_dir: PathBuf,
    pub(crate) base: Option<String>,
    pub(crate) declared: usize,
    pub(crate) present: usize,
    pub(crate) stale: usize,
    pub(crate) todo: usize,
    pub(crate) rows: Vec<(&'static str, String)>,
    pub(crate) warning: Option<String>,
}

pub(crate) fn inspect(
    deps_dir: &Path,
    key: &str,
    decl: &Declaration,
) -> Result<StatusReport, String> {
    let assets = plan_entries(decl, deps_dir, key)?;
    let inspection = sync::inspect(&decl.files, &assets, deps_dir, false)?;

    let mut rows: Vec<(&'static str, String)> = Vec::new();
    for (index, change) in &inspection.diff.steps {
        let state = match change {
            Change::Keep | Change::Adopt => "ok",
            Change::Add | Change::Repair => "missing",
            Change::Update | Change::Rechmod => "outdated",
        };
        rows.push((state, assets[*index].label.clone()));
    }
    for entry in &inspection.diff.removed {
        rows.push(("stale", label_of(key, &entry.dest)));
    }

    // pip 包：venv 里已装的才算 ok，缺失算 missing；URL/VCS 直接引用标 unknown，
    // 这类规格里没有包名，本地无法可靠判定装没装，交给用户自查。
    let installed = installed_pip_packages(deps_dir);
    for spec in &decl.pip {
        let state = if is_direct_reference(spec) {
            "unknown"
        } else if installed.contains(&pip_package_name(spec)) {
            "ok"
        } else {
            "missing"
        };
        rows.push((state, format!("pip:{spec}")));
    }

    // npm 包：node_modules 里已有对应目录的才算 ok，其余都算 missing
    for spec in &decl.npm {
        let state = if npm_package_installed(deps_dir, spec) {
            "ok"
        } else {
            "missing"
        };
        rows.push((state, format!("npm:{spec}")));
    }

    let present = rows.iter().filter(|(state, _)| *state == "ok").count();
    let stale = inspection.diff.removed.len();
    let todo = rows
        .iter()
        .filter(|(state, _)| matches!(*state, "missing" | "outdated"))
        .count();

    Ok(StatusReport {
        key: key.to_string(),
        deps_dir: deps_dir.to_path_buf(),
        base: decl.base.as_ref().map(|base| base.as_str().to_string()),
        declared: assets.len() + decl.pip.len() + decl.npm.len(),
        present,
        stale,
        todo,
        rows,
        warning: inspection.warning,
    })
}

fn label_of(key: &str, dest: &str) -> String {
    if dest.is_empty() {
        key.to_string()
    } else {
        format!("{key}/{dest}")
    }
}

/// URL/VCS 直接引用（`git+` / `hg+` / `svn+` / `bzr+` 或直链 URL）里没有包名，
/// 无法用 `pip list` 比对装没装，只能标 unknown 交给用户自查。
fn is_direct_reference(spec: &str) -> bool {
    let spec = spec.trim();
    let rest = ["git+", "hg+", "svn+", "bzr+"]
        .iter()
        .find_map(|prefix| spec.strip_prefix(prefix))
        .unwrap_or(spec);
    rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("file://")
}

#[cfg(test)]
mod tests {
    use super::is_direct_reference;

    #[test]
    fn flags_url_and_vcs_specs() {
        assert!(is_direct_reference(
            "git+https://github.com/fcbyk/slide.git#subdirectory=server"
        ));
        assert!(is_direct_reference(
            "git+https://github.com/fcbyk/fcbyk-cli.git"
        ));
        assert!(is_direct_reference("https://example.com/foo-1.0.tar.gz"));
        assert!(is_direct_reference("file:///tmp/foo.whl"));
    }

    #[test]
    fn leaves_plain_name_specs_alone() {
        assert!(!is_direct_reference("click>=8.0"));
        assert!(!is_direct_reference("requests[socks]>=2.0"));
        assert!(!is_direct_reference("pkg; python_version < '3.8'"));
    }
}
