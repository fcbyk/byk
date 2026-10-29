//! `byk __api__`：面向程序与开发者的只读接口层。
//!
//! 单一端点 + 请求名：`byk __api__ <request> [args...]`。
//! 约定只读——不执行用户命令、不修改任何磁盘状态，因此可以被任意程序
//! 以任意频率安全调用。
//!
//! 输出走 stdout，格式由各请求自行约定；错误走 stderr 并以非零码退出。
//! 本模块不出现在 `byk -h` 中。

use serde::Serialize;

use crate::core::completion;
use crate::core::paths::PathLayout;

/// 已提供的请求。新增请求时同步登记，`byk __api__` 无参会输出该清单。
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

/// 单个请求的自描述条目。
#[derive(Serialize)]
struct RequestInfo {
    name: &'static str,
    args: &'static str,
    description: &'static str,
}

/// 无参调用的自描述输出。
#[derive(Serialize)]
struct RequestList {
    requests: &'static [RequestInfo],
}

/// 分派 `__api__` 请求。
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
