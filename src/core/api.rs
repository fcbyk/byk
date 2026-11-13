//! `byk __api__`：面向程序的只读接口，不执行命令、不写磁盘，可任意频率调用。
//!
//! 形式为 `byk __api__ <request> [args...]`，结果走 stdout，错误走 stderr 并以非零码退出。
//! 不出现于 `byk -h`。

use std::path::PathBuf;

use serde::Serialize;

use crate::core::aliases;
use crate::core::completion;
use crate::core::config::{Level, Mode};
use crate::core::filecmd;
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
        args: "[--with-descriptions] <words...>",
        description: "print completion candidates; the flag appends a tab-separated description",
    },
    RequestInfo {
        name: "resolve",
        args: "<key> [args...]",
        description: "resolve an alias to its final command without executing",
    },
    RequestInfo {
        name: "paths",
        args: "[name]",
        description: "print CLI paths as JSON; with <name>, print only that path (root|alias)",
    },
    RequestInfo {
        name: "check",
        args: "",
        description: "print scan diagnostics as JSON; problems do not change the exit code",
    },
];

/// `paths <name>` 可取的键，顺序即错误提示里的顺序。
/// `PathsResponse` 的字段必须与它保持一致。
const PATH_NAMES: &[&str] = &["root", "alias"];

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
    /// 执行方式：`normal` / `interactive` / `silent`
    mode: &'static str,
    /// `mode == "interactive"` 的派生布尔。保留它是为了版本错配时的安全：
    /// 只认 `mode` 的旧插件读到 `null` 会把交互别名直接执行、跳过确认。
    interactive: bool,
    description: Option<String>,
}

/// `__api__ check` 的返回体：扫描诊断，供编辑器直接映射到问题面板。
///
/// 发现问题的请求本身依然成功（退出码 0）：调用方读 `items`，不必把
/// “有问题”与“请求失败”两种非零混在一起。
#[derive(Serialize)]
struct CheckResponse {
    scanned: usize,
    errors: usize,
    warnings: usize,
    items: Vec<CheckItem>,
}

#[derive(Serialize)]
struct CheckItem {
    level: &'static str,
    path: String,
    message: String,
}

/// `__api__ paths` 的返回体：byk 实际使用的两条目录。
///
/// 与 `--paths` 共用同一份 `PathLayout`，区别只在形状：
/// `--paths` 给人看（带标签和空行），这份 JSON 只给程序解析。
#[derive(Serialize)]
struct PathsResponse {
    root: PathBuf,
    alias: PathBuf,
}

/// 键名到目录的唯一映射，JSON 与 `paths <name>` 两条出口共用。
fn path_for<'a>(name: &str, layout: &'a PathLayout) -> Option<&'a PathBuf> {
    match name {
        "root" => Some(&layout.root_dir),
        "alias" => Some(&layout.alias_dir),
        _ => None,
    }
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
            let request = &args[1..];
            let (with_descriptions, words) = match request.first().map(String::as_str) {
                Some("--with-descriptions") => (true, &request[1..]),
                _ => (false, request),
            };
            completion::complete(words, layout, with_descriptions);
        }
        Some("resolve") => {
            let key = match args.get(1) {
                Some(s) if !s.is_empty() => s,
                _ => usage_error("resolve", "missing <key>"),
            };
            let command_args = &args[2..];

            // 文件命令不是别名：只读接口绝不能执行或创建文件
            if filecmd::claims(key) {
                eprintln!("resolve: not an alias (file command): {}", key);
                std::process::exit(1);
            }

            // 与 main.rs 相同的查找顺序：精确语法优先（只解析目标文件），其次合并命名空间
            let resolved = if let Some((file_key, alias_key)) = aliases::parse_exact_syntax(key) {
                match aliases::resolve_exact_alias(layout, &file_key, &alias_key) {
                    aliases::ExactLookup::Found { resolved, .. } => resolved,
                    _ => {
                        eprintln!("resolve: alias not found: {}", key);
                        std::process::exit(1);
                    }
                }
            } else {
                let (merged, _files) = aliases::load_merged_aliases(layout);
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
                mode: rc.mode.as_str(),
                interactive: rc.mode == Mode::Interactive,
                description: rc.description,
            };

            println!(
                "{}",
                serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string())
            );
        }
        Some("check") => {
            if args.len() > 1 {
                usage_error("check", "takes no arguments");
            }

            let scanned = aliases::load_scanned_aliases(layout);
            let report = &scanned.report;
            let response = CheckResponse {
                scanned: report.scanned,
                errors: report.error_count(),
                warnings: report.warning_count(),
                items: report
                    .items
                    .iter()
                    .map(|item| CheckItem {
                        level: match item.level {
                            Level::Error => "error",
                            Level::Warning => "warning",
                        },
                        path: item.path.to_string_lossy().into_owned(),
                        message: item.message.clone(),
                    })
                    .collect(),
            };

            println!(
                "{}",
                serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string())
            );
        }
        Some("paths") => match args.get(1) {
            // 不带名字：整份 JSON，一次拿全
            None => {
                let payload = PathsResponse {
                    root: layout.root_dir.clone(),
                    alias: layout.alias_dir.clone(),
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
                );
            }
            // 带名字：只打印那一个值的裸字符串，不引号、不转义，直接进 `$(...)`
            Some(name) => {
                if args.len() > 2 {
                    usage_error("paths", "too many arguments (expected [name])");
                }
                match path_for(name, layout) {
                    Some(path) => println!("{}", path.display()),
                    None => usage_error(
                        "paths",
                        &format!(
                            "unknown name {:?} (expected one of: {})",
                            name,
                            PATH_NAMES.join(", ")
                        ),
                    ),
                }
            }
        },
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
