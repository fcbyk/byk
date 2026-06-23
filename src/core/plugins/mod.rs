//! 插件系统模块。
//!
//! 职责：
//! - types：数据结构定义（CmdState, PkgState, PluginCommand 等）
//! - state：状态加载与持久化
//! - execute：命令执行引擎（可扩展 trait 架构）
//! - install：插件安装流水线

pub mod types;
pub mod state;
pub mod execute;
pub mod install;