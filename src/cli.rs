//! CLI 参数定义与选项提取。

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "byk", disable_version_flag = true, disable_help_flag = true)]
pub struct Cli {
    /// Print version
    #[arg(short = 'v', long = "version")]
    pub version: bool,

    /// Print help
    #[arg(short = 'h', long = "help", action = clap::ArgAction::SetTrue)]
    pub help: bool,

    /// Print CLI paths and file locations
    #[arg(long = "paths")]
    pub paths: bool,

    /// 内置子命令
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 捕获命令名及参数（后续匹配插件命令 / 别名）
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub trailing: Vec<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 移除插件或已初始化的功能
    Remove {
        /// 插件 key 或功能名: comp | all
        #[arg(allow_hyphen_values = true)]
        feature: Option<String>,
    },
    /// 添加功能（comp）或从别名文件安装（@xxx / @@xxx）
    Add {
        /// 功能名(comp) 或别名文件(@xxx / @@xxx)
        #[arg(allow_hyphen_values = true)]
        name: Option<String>,
    },
    /// 查看系统信息、插件、命令来源
    Show {
        /// 子命令或命令名: overview | plugins | <command-name>
        #[arg(allow_hyphen_values = true)]
        topic: Option<String>,
    },
    /// 内部：面向程序的只读接口，返回结构化信息
    #[command(hide = true, name = "__api__")]
    Api {
        /// 请求名及其参数，如 completion zsh
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        request: Vec<String>,
    },
}

/// 从 clap Command 中提取选项信息。
pub fn extract_options(cmd: &clap::Command) -> Vec<(String, String)> {
    cmd.get_arguments()
        .filter(|a| !a.is_positional())
        .map(|a| {
            let flag = match (a.get_long(), a.get_short()) {
                (Some(l), Some(s)) => format!("--{}, -{}", l, s),
                (Some(l), None) => format!("--{}", l),
                (None, Some(s)) => format!("-{}", s),
                _ => String::new(),
            };
            let desc = a.get_help().map(|h| h.to_string()).unwrap_or_default();
            (flag, desc)
        })
        .collect()
}