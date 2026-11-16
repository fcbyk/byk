//! `$deps`：别名文件的依赖声明、落点计划与安装执行。
//!
//! 一个别名文件可以声明它需要的资产（`$deps.files`），byk 负责把它们
//! 下载到该文件的专属目录：全局文件是 `~/.byk/deps/<key>/`，局部文件是
//! `<文件所在目录>/.byk/deps/<key>/`。byk 自己不做网络：下载交给系统
//! `curl`，解压交给系统 `tar`——两者在 Windows / macOS / Linux 上都是
//! 现代系统自带，缺失时报错并提示安装。
//!
//! 三条贯穿规则（沿自 byk-add）：
//!
//! 1. **不覆盖**。目标已存在就报错；归档先用 `tar -tf` 列清单逐个查。
//! 2. **临时文件与目标同级**。取回先落到 `dest` 同目录的唯一临时文件，
//!    成功后 rename 过去，中途失败不留半个文件。
//! 3. **落点圈死在专属目录内**。`to` 只接受相对子目录段，拒绝 `..`、
//!    绝对路径与反斜杠；归档条目额外做越界检查。
//!
//! `src` 可以写绝对 http(s) URL，也可以写相对路径——相对谁由 `$deps.base`
//! 决定。base 是**目录**，落盘后必须是绝对地址：别名文件住在 `~/.byk/alias/`，
//! 它自己没有任何位置信息，相对值在那里无从解析。冻结只发生在 `..clone`：
//! 远程来源写成 http(s) 目录 URL（[`freeze_remote_base`]），本地来源写成
//! 本机绝对路径（[`freeze_local_base`]）。
//!
//! 两种 base 对应两种来源：URL 交给 `curl`，本地路径走 `fs::copy`。远程 clone
//! 只会写出 URL 形态，于是「从网上拿到的别名文件读不了本机文件」这条边界由
//! clone 保证，安装期只管照着执行——本地路径形态只可能来自本地 clone 或用户
//! 手写。
//!
//! 模块边界：数据模型、声明解析、计划、资产安装、包安装与路径处理各自独立；
//! 本模块重导出既有接口，避免影响调用方。

mod install;
mod layout;
mod packages;
mod parse;
mod path;
mod plan;
mod types;

pub(super) use install::{install_archive, install_file, remove_installed, set_exec};
pub(super) use layout::{deps_dir, deps_dir_var};
pub(super) use packages::{
    NODE_MODULES, VENV_DIR, install_npm, install_pip, installed_pip_packages, node_modules_bin,
    npm_package_installed, pip_package_name, venv_bin,
};
pub(super) use parse::parse_deps;
pub(super) use path::{freeze_local_base, freeze_remote_base};
pub(super) use plan::plan_entries;
pub(super) use types::{Declaration, DepEntry, Origin, PlannedAsset};
