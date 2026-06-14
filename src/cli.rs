/// CLI 参数定义与选项提取。

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "byk", disable_version_flag = true, disable_help_flag = true)]
pub struct Cli {
    /// Print version
    #[arg(short = 'v', long = "version")]
    pub version: bool,

    /// Show CLI info
    #[arg(long = "info", num_args = 0..=1, default_missing_value = "")]
    pub info: Option<String>,

    /// Print help
    #[arg(short = 'h', long = "help", action = clap::ArgAction::SetTrue)]
    pub help: bool,

    /// 内置子命令
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 捕获命令名及参数（后续匹配 npm 命令 / 别名）
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub trailing: Vec<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 生成 shell 补全脚本（输出到 stdout）
    Completion {
        /// 目标 shell: zsh | bash | fish
        shell: String,
    },
    /// 移除已初始化的 feature
    Remove {
        /// 要移除的 feature: py | py-v | npm | pnpm
        #[arg(allow_hyphen_values = true)]
        feature: Option<String>,
    },
    /// 初始化 feature
    Init {
        /// 要初始化的 feature: npm | pnpm | py | py-v | comp
        #[arg(allow_hyphen_values = true)]
        feature: Option<String>,
    },
    /// 内部：查询补全候选
    #[command(hide = true, name = "__complete")]
    Complete {
        /// 当前已输入的命令行（不含 byk 本身）
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        words: Vec<String>,
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
