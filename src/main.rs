mod alias;
mod api;
mod app;
mod cli;
mod completion;
mod deps;
mod execution;
mod filecmd;
mod model;
mod net;
mod options;
mod resolve;
mod utils;

use clap::{CommandFactory, Parser};
use std::process::exit;

use cli::{Cli, extract_options};
use model::PathLayout;

/// 管道下游提前关闭时（如 `byk -h | head`），按 Unix 惯例静默退出，
/// 而不是让 `println!` 抛出 `Broken pipe` 再 panic。
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: 只把 SIGPIPE 的处置恢复为默认值，不接触内存或指针。
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() {
    restore_sigpipe();

    let cmd = Cli::command();
    let option_specs = extract_options(&cmd);
    let layout = PathLayout::new();

    let cli = Cli::parse();
    let code = app::run(&cli, &option_specs, &layout);
    if code != 0 {
        exit(code);
    }
}
