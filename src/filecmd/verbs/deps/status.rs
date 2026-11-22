//! `deps status`：渲染状态报告。

use super::{Outcome, declaration, deps_command, deps_dir_of, no_declaration};
use crate::deps::apply;
use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;
use crate::model::build_file_key;

pub(super) fn status(scope: FileScope, stem: &str, layout: &PathLayout) -> Result<Outcome, String> {
    let (target, declared) = declaration(scope, stem, layout)?;
    let Some(decl) = declared else {
        return Err(no_declaration(&target));
    };

    let deps_dir = deps_dir_of(scope, stem, layout);
    let key = build_file_key(stem, scope.is_global());
    let report = apply::inspect(&deps_dir, &key, &decl)?;
    if let Some(warning) = &report.warning {
        eprintln!("Warning: {warning}");
    }

    let mut output = format!(
        "{}: {} declared, {} present",
        report.key, report.declared, report.present
    );
    if report.stale > 0 {
        output.push_str(&format!(", {} stale", report.stale));
    }
    output.push('\n');
    for (state, label) in &report.rows {
        output.push_str(&format!("  {:<9}{label}\n", state));
    }
    if let Some(base) = &report.base {
        output.push_str(&format!("Base: {base}\n"));
    }
    output.push_str(&format!("Deps directory: {}\n", report.deps_dir.display()));

    if report.todo > 0 {
        output.push_str(&format!(
            "Hint: run `{}` to install the missing assets\n",
            deps_command(scope, stem)
        ));
    } else if report.stale > 0 {
        output.push_str(&format!(
            "Hint: run `{}` to remove the stale assets\n",
            deps_command(scope, stem)
        ));
    }

    Ok(Outcome::Status(output))
}
