//! 网络传输：统一构造 byk 自己发起的 curl / git 子进程。
//!
//! 代理只注入到这些子进程，不改 byk 自身的进程环境，也不影响别名、pip、npm。

pub(crate) mod mirror;
pub(crate) mod proxy;

pub(crate) use mirror::{Channel, Route, is_github_file, note_credentials};

use std::process::Command;

/// byk 的 curl 一律从这里取，便于统一套用代理配置。
pub(crate) fn curl() -> Command {
    let mut command = Command::new("curl");
    proxy::apply(&mut command);
    command
}

/// byk 的 git 一律从这里取。
///
/// git 不认 curl 的 `--proxy`，代理要翻译成 `http.proxy`，所以和 [`curl`] 分开构造。
pub(crate) fn git() -> Command {
    let mut command = Command::new("git");
    proxy::apply_to_git(&mut command);
    command
}
