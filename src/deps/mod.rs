//! `$deps` 子系统：类型、声明解析、计划、布局与安装执行。
//!
//! 本模块是依赖图叶子：不 import 项目内其他模块。

pub(crate) mod apply;
pub(crate) mod spec;
pub(crate) mod types;

pub(crate) use spec::{
    deps_dir, deps_dir_var, freeze_base, node_modules_bin, parse_deps, plan_entries, venv_bin,
};
pub(crate) use types::{Declaration, Origin};
