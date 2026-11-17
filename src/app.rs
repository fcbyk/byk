//! 组合根：全局选项、__api__ 与别名调用的路由。

use crate::alias;
use crate::api;
use crate::cli::{Cli, Commands};
use crate::filecmd;
use crate::model::PathLayout;
use crate::options;

/// 处理一次 CLI 调用，返回退出码。
pub fn run(cli: &Cli, option_specs: &[(String, String)], layout: &PathLayout) -> i32 {
    if let Some(Commands::Api { request }) = &cli.command {
        api::dispatch(request, layout);
        return 0;
    }

    dispatch(cli, option_specs, layout)
}

fn dispatch(cli: &Cli, option_specs: &[(String, String)], layout: &PathLayout) -> i32 {
    if cli.help {
        return options::help::run(layout, option_specs);
    }
    if cli.check {
        if !cli.trailing.is_empty() {
            eprintln!("check takes no arguments");
            return 1;
        }
        return options::check::run(layout);
    }
    if cli.paths {
        return options::paths::run(layout);
    }
    if cli.version {
        return options::version::run();
    }
    if cli.trailing.is_empty() {
        return options::help::run(layout, option_specs);
    }

    let command_name = &cli.trailing[0];
    let command_args = &cli.trailing[1..];

    // 文件命令优先；未命中再走别名查找与执行。
    if let Some(code) = filecmd::run(command_name, command_args, layout) {
        return code;
    }
    alias::run(command_name, command_args, layout)
}
