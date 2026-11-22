//! 唯一读取和解析 byk.json 的模块：扫描、合并、校验、查找，
//! 以及文件契约、变量展开、继承与诊断。

pub(crate) mod declaration;
mod diagnostic;
mod exact;
mod format;
mod inherit;
mod lookup;
mod merge;
mod scan;
mod validate;
mod vars;

pub use crate::model::to_alias_definition;
pub(crate) use declaration::load_declaration;
pub use diagnostic::{Diagnostic, DiagnosticReport, Level};
pub use exact::load_file_by_key;
pub(crate) use format::{
    filter_invalid_keys, is_alias_value, is_meta_object, is_meta_only, is_valid_clone,
    is_valid_private, parse_clone, parse_paths, parse_priority, parse_private, to_alias_value,
    validate_filename,
};
pub(crate) use inherit::apply_inherited;
pub use lookup::{LookupFailure, lookup};
pub use merge::{
    ScannedAliases, collect_merged_paths, load_merged, load_merged_aliases_report,
    load_scanned_aliases, resolve_merged_alias,
};
pub(crate) use scan::{follow_link, list_alias_files, load_document};
pub use vars::{
    DEPS_DIR_VAR, FILE_DIR_VAR, VarTable, build_builtin_table, describe_issue, expand_document,
    expand_document_without_builtins, preprocess_vars,
};
