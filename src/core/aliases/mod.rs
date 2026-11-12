//! 别名扫描与合并模块。
//!
//! 扫描当前目录和全局 alias 目录下的 *.byk.json 文件，
//! 按优先级深度合并，保留单文件数据结构供精确执行查找。

mod diagnostic;
mod exact;
mod execution;
mod merge;
mod parse;
pub(crate) mod placeholder;
mod quote;
mod scan;
mod types;

// 统一出口：其他模块只从这里使用别名能力
pub use diagnostic::{Diagnostic, DiagnosticReport, Level};
pub use exact::{lookup_exact_alias, parse_exact_syntax};
pub use execution::{ResolvedCommand, execute_alias, resolve_alias};
pub use merge::{
    LoadedAliases, collect_merged_paths, load_merged_aliases, load_merged_aliases_report,
    resolve_merged_alias,
};
pub use parse::to_alias_definition;
pub use types::*;

/// 文件命令与别名文件共用同一套文件名规则。
pub(crate) use parse::validate_filename;
