//! `..deps status`：只读列出声明、落点与当前状态。

use super::super::FileScope;
use super::{Outcome, declaration, deps_command, deps_dir_of, label_of, no_declaration};
use crate::core::config::build_file_key;
use crate::core::deps::{self, plan_entries};
use crate::core::paths::PathLayout;
use crate::core::sync::{self, Change};

// ── status ──────────────────────────────────────────────────────────────

pub(super) fn status(scope: FileScope, stem: &str, layout: &PathLayout) -> Result<Outcome, String> {
    let (target, declared) = declaration(scope, stem, layout)?;
    let Some(decl) = declared else {
        return Err(no_declaration(&target));
    };

    let deps_dir = deps_dir_of(scope, stem, layout);
    let key = build_file_key(stem, scope.is_global());
    let assets = plan_entries(&decl, &deps_dir, &key)?;
    let inspection = sync::inspect(&decl.files, &assets, &deps_dir, false)?;
    if let Some(warning) = &inspection.warning {
        eprintln!("Warning: {warning}");
    }

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
        rows.push(("stale", label_of(&key, &entry.dest)));
    }

    // pip 包：venv 里已装的才算 ok，其余都算 missing
    let installed = deps::installed_pip_packages(&deps_dir);
    for spec in &decl.pip {
        let state = if installed.contains(&deps::pip_package_name(spec)) {
            "ok"
        } else {
            "missing"
        };
        rows.push((state, format!("pip:{spec}")));
    }

    // npm 包：node_modules 里已有对应目录的才算 ok，其余都算 missing
    for spec in &decl.npm {
        let state = if deps::npm_package_installed(&deps_dir, spec) {
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

    let declared = assets.len() + decl.pip.len() + decl.npm.len();
    let mut output = format!("{}: {} declared, {} present", key, declared, present);
    if stale > 0 {
        output.push_str(&format!(", {stale} stale"));
    }
    output.push('\n');
    for (state, label) in &rows {
        output.push_str(&format!("  {:<9}{label}\n", state));
    }
    if let Some(base) = &decl.base {
        output.push_str(&format!("Base: {}\n", base.as_str()));
    }
    output.push_str(&format!("Deps directory: {}\n", deps_dir.display()));

    if todo > 0 {
        output.push_str(&format!(
            "Hint: run `{}` to install the missing assets\n",
            deps_command(scope, stem)
        ));
    } else if stale > 0 {
        output.push_str(&format!(
            "Hint: run `{}` to remove the stale assets\n",
            deps_command(scope, stem)
        ));
    }

    Ok(Outcome::Status(output))
}
