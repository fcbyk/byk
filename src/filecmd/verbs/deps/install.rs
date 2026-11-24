//! `deps install` 的命令适配：计算位置后交给 `deps::apply`。

use super::{deps_command, deps_dir_of};
use crate::deps::Declaration;
use crate::deps::apply;
use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;
use crate::model::build_file_key;

pub(crate) fn install(
    scope: FileScope,
    stem: &str,
    layout: &PathLayout,
    decl: &Declaration,
    force: bool,
) -> Result<(), String> {
    let deps_dir = deps_dir_of(scope, stem, layout);
    let key = build_file_key(stem, scope.is_global());
    let retry_hint = deps_command(scope, stem);
    let channel = crate::filecmd::sys::config::channel(layout);
    apply::install_declaration(
        &deps_dir,
        &key,
        scope.is_global(),
        decl,
        force,
        channel,
        &retry_hint,
    )
}
