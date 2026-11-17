//! 共享数据结构：别名文件类型与 CLI 路径布局。

mod keys;
mod paths;
mod types;

pub(crate) use keys::build_file_key;
pub use paths::PathLayout;
pub use types::*;
