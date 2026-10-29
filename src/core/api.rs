//! `byk __api__`：面向程序的只读接口，不执行命令、不写磁盘，可任意频率调用。
//!
//! 形式为 `byk __api__ <request> [args...]`，结果走 stdout，错误走 stderr 并以非零码退出。
//! 不出现于 `byk -h`。

use serde::Serialize;

use crate::core::completion;
use crate::core::paths::PathLayout;

/// 全部请求；无参调用 `byk __api__` 时原样输出。
const REQUESTS: &[RequestInfo] = &[
    RequestInfo {
        name: "completion",
        args: "<shell>",
        description: "print the shell registration script (zsh|bash|fish)",
    },
    RequestInfo {
        name: "complete",
        args: "<words...>",
        description: "print completion candidates, one per line",
    },
];

#[derive(Serialize)]
struct RequestInfo {
    name: &'static str,
    args: &'static str,
    description: &'static str,
}

#[derive(Serialize)]
struct RequestList {
    requests: &'static [RequestInfo],
}

pub fn dispatch(args: &[String], layout: &PathLayout) {
    match args.first().map(String::as_str) {
        Some("completion") => {
            let shell = match args.get(1) {
                Some(s) => s,
                None => usage_error("completion", "missing <shell> (zsh|bash|fish)"),
            };
            completion::generate(shell);
        }
        Some("complete") => {
            completion::complete(&args[1..], layout);
        }
        Some(other) => {
            eprintln!("__api__: unknown request {:?}", other);
            eprintln!("  Run `byk __api__` to list available requests.");
            std::process::exit(1);
        }
        None => {
            let payload = RequestList { requests: REQUESTS };
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}

fn usage_error(request: &str, message: &str) -> ! {
    eprintln!("__api__ {}: {}", request, message);
    std::process::exit(1);
}
