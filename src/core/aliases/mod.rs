//! 别名扫描与合并模块。
//!
//! 扫描当前目录和全局 alias 目录下的 *.byk.json 文件，
//! 按优先级深度合并，保留单文件数据结构供精确执行查找。

mod types;
mod parse;
mod scan;
mod merge;
mod exact;
pub(crate) mod placeholder;
mod execution;
pub(crate) mod variables;

// 统一出口：其他模块只从这里使用别名能力
pub use types::*;
pub use parse::to_alias_definition;
pub use merge::{collect_merged_paths, load_merged_aliases, resolve_merged_alias};
pub use exact::{lookup_exact_alias, parse_exact_syntax};
pub use execution::execute_alias;