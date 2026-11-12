//! 别名扫描与合并模块。
//!
//! 扫描当前目录和全局 alias 目录下的 *.byk.json 文件，
//! 按优先级深度合并，保留单文件数据结构供精确执行查找。

mod exact;
mod execution;
mod merge;
pub(crate) mod placeholder;
mod quote;
mod scan;

// 统一出口：其他模块只从这里使用别名引擎能力；共享词汇在 core::config
pub use exact::{lookup_exact_alias, parse_exact_syntax};
pub use execution::{ResolvedCommand, execute_alias, resolve_alias};
pub use merge::{
    LoadedAliases, collect_merged_paths, load_merged_aliases, load_merged_aliases_report,
    resolve_merged_alias,
};
pub(crate) use scan::list_alias_files;
