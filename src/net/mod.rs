//! 网络传输：统一构造 byk 自己发起的 curl 子进程。
//!
//! 代理只注入到这些子进程，不改 byk 自身的进程环境，也不影响别名、pip、npm。

pub(crate) mod proxy;

use std::process::Command;

/// byk 的 curl 一律从这里取，便于统一套用代理配置。
pub(crate) fn curl() -> Command {
    let mut command = Command::new("curl");
    proxy::apply(&mut command);
    command
}
