//! 读取目标文件里的 `$deps` 声明。
//!
//! 文档读取、链接跟随和变量展开属于 `resolve`；`deps` 只接收解析结果。

use std::path::Path;

use crate::deps::{Declaration, parse_deps};

use super::{Diagnostic, Level, VarTable, follow_link, load_document};

/// 读取目标文件里的 `$deps` 声明；`None` 表示没有该字段。
///
/// 只做配置读取：跟随 link、展开变量、解析声明；不涉及安装或状态。
pub(crate) fn load_declaration(
    target: &Path,
    vars: &VarTable,
    deps_dir: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<Declaration>, String> {
    let (link_data, link_path) = load_document(target, vars, deps_dir, target, diagnostics)
        .map_err(|_| first_error(diagnostics))?;
    let (data, _) = follow_link(link_data, &link_path, vars, deps_dir, diagnostics)
        .ok_or_else(|| first_error(diagnostics))?;
    let Some(value) = data.get("$deps") else {
        return Ok(None);
    };
    parse_deps(value).map(Some)
}

fn first_error(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .find(|item| item.level == Level::Error)
        .map(|item| item.message.clone())
        .unwrap_or_else(|| "cannot load the alias file".to_string())
}
