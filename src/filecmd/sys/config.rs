//! `@@@config`：byk 的设置文件，落在 `~/.byk/config.json`。
//!
//! 当前只跑通路由：参数原样回显到 stderr（stdout 留给将来的键值输出，
//! 免得临时打印被脚本当成接口）。键的读写随后接入。

use crate::model::PathLayout;

pub(crate) fn run(args: &[String], _layout: &PathLayout) -> Result<(), String> {
    eprintln!("config: routing ok, args = {:?}", args);
    Ok(())
}
