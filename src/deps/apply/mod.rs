//! `$deps` 的安装执行：资产下载、解压、pip/npm 与同步清单。
//!
//! 本子模块只依赖 `deps::spec` / `deps::types`，不 import 项目内其他模块。

mod install;
mod packages;
mod remove;
mod status;
pub(crate) mod sync;

pub(crate) use install::install_declaration;
pub(crate) use packages::{installed_pip_packages, npm_package_installed, pip_package_name};
pub(crate) use remove::{remove_all, remove_dir, rename_dir, warn_stale_dir};
pub(crate) use status::inspect;

/// 依赖条数的文案；安装前的标题与安装后的总结共用。
pub(crate) fn count_files(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}
