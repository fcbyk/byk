//! `$deps` 的纯逻辑：声明解析、计划、布局与路径。
//!
//! 本子模块不 import 项目内其他模块；所有输入都是已展开的数据或路径参数。

mod layout;
mod parse;
mod path;
mod plan;

use serde_json::Value;

use super::types::Origin;

pub(crate) use layout::{
    NODE_MODULES, VENV_DIR, deps_dir, deps_dir_var, node_modules_bin, venv_bin,
};
pub(crate) use parse::parse_deps;
pub(crate) use path::{freeze_local_base, freeze_remote_base};
pub(crate) use plan::plan_entries;

/// 按来源冻结 `$deps.base`；返回新写入的 base（没变或没有 `$deps` 时为 `None`）。
pub(crate) fn freeze_base(root: &mut Value, origin: &Origin) -> Result<Option<String>, String> {
    let Some(object) = root.get_mut("$deps").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let current = object.get("base").cloned();
    let base = match origin {
        Origin::Url(url) => freeze_remote_base(current.as_ref(), url)?,
        Origin::File(path) => freeze_local_base(current.as_ref(), path)?,
    };
    if current.as_ref().and_then(Value::as_str) == Some(base.as_str()) {
        return Ok(None);
    }
    object.insert("base".to_string(), Value::String(base.clone()));
    Ok(Some(base))
}
