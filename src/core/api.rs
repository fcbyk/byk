//! `byk __api__`：面向程序的只读接口，不执行命令、不写磁盘，可任意频率调用。
//!
//! 形式为 `byk __api__ <request> [args...]`，结果走 stdout，错误走 stderr 并以非零码退出。
//! 不出现于 `byk -h`。

use std::path::PathBuf;

use serde::Serialize;

use crate::core::aliases;
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
    RequestInfo {
        name: "resolve",
        args: "<key> [args...]",
        description: "resolve an alias to its final command without executing",
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

/// `__api__ resolve` 的返回体：别名解析后的完整信息，供编辑器/插件直接执行。
#[derive(Serialize)]
struct ResolveResponse {
    key: String,
    source: String,
    #[serde(rename = "sourcePath")]
    source_path: Option<PathBuf>,
    command: String,
    cwd: PathBuf,
    paths: Vec<String>,
    interactive: bool,
    description: Option<String>,
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
        Some("resolve") => {
            let key = match args.get(1) {
                Some(s) if !s.is_empty() => s,
                _ => usage_error("resolve", "missing <key>"),
            };
            let command_args = &args[2..];

            // 与 main.rs 相同的查找顺序：精确语法优先，其次合并命名空间
            let (merged, files) = aliases::load_merged_aliases(layout);
            let resolved = if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(key) {
                match aliases::lookup_exact_alias(&files, &file_key, &alias_key) {
                    Some((r, _)) => r,
                    None => {
                        eprintln!("resolve: alias not found: {}", key);
                        std::process::exit(1);
                    }
                }
            } else {
                match aliases::resolve_merged_alias(&merged, key) {
                    Some(r) => r.clone(),
                    None => {
                        eprintln!("resolve: alias not found: {}", key);
                        std::process::exit(1);
                    }
                }
            };

            let rc: aliases::ResolvedCommand = aliases::resolve_alias(&resolved, command_args);

            let response = ResolveResponse {
                key: key.to_string(),
                source: resolved.source.clone(),
                source_path: resolved.source_path.clone(),
                command: rc.command,
                cwd: rc.cwd,
                paths: rc.paths,
                interactive: rc.interactive,
                description: rc.description,
            };

            println!(
                "{}",
                serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string())
            );
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
