//! 别名文件格式与共享词汇。
//!
//! 结构化类型、文件名/字段规则、变量替换与扫描诊断都放在这里；
//! 扫描、合并、执行等生命周期逻辑留在 `core::aliases`。

mod diagnostic;
mod format;
mod inherit;
mod types;
mod vars;

pub use diagnostic::{Diagnostic, DiagnosticReport, Level};
pub use format::to_alias_definition;
pub(crate) use format::{
    build_file_key, filter_invalid_keys, is_alias_value, is_valid_private, parse_priority,
    parse_private, to_alias_value, validate_filename,
};
pub(crate) use inherit::apply_inherited;
pub use types::*;
pub use vars::{
    FILE_DIR_VAR, VarTable, build_builtin_table, describe_issue, expand_document, preprocess_vars,
};
