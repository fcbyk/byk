//! 插件系统模块。
//!
//! 三层架构：
//! - protocol：协议层，映射 byk.json JSON 形状
//! - types：执行层 + 状态层数据结构
//! - state：状态加载与持久化
//! - execute：命令执行引擎（可扩展 trait 架构）
//! - install：插件安装流水线（协议解析 → 构建计划 → 执行）

pub mod protocol;
pub mod types;
pub mod state;
pub mod execute;
pub mod install;