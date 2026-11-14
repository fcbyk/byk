//! `byk __api__`：面向程序的只读接口，不执行命令、不写磁盘，可任意频率调用。
//!
//! 形式为 `byk __api__ <request> [args...]`，结果走 stdout，错误走 stderr 并以非零码退出。
//! 不出现于 `byk -h`。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::aliases;
use crate::core::completion;
use crate::core::config::{Level, Mode, describe_issue, expand_document};
use crate::core::filecmd;
use crate::core::paths::PathLayout;
use crate::utils::path::dir_of;

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
        description: "print CLI paths and alias file locations as JSON; with <name>, print only that path (root|alias)",
    },
    RequestInfo {
        name: "file",
        args: "<file-key> | --list",
        description: "resolve an alias file key (@name / @@name) to its path, or list every alias file with --list",
    },
    RequestInfo {
        name: "expand",
        args: "<path>",
        description: "expand file-level $var and built-in variables in a JSON file without alias validation",
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

/// `__api__ paths` 的返回体：byk 实际使用的目录与本次扫描到的别名文件。
///
/// 与 `--paths` 共用同一份 `PathLayout` 和文件名枚举，区别只在形状：
/// `--paths` 给人看（带标签和空行），这份 JSON 只给程序解析。
#[derive(Serialize)]
struct PathsResponse {
    root: PathBuf,
    alias: PathBuf,
    workdir: PathBuf,
    /// 两处目录下的别名文件绝对路径，先全局后本地，各自按 key 排序。
    /// 需要 key / scope / dir 时用 `__api__ file --list`。
    #[serde(rename = "aliasFiles")]
    alias_files: Vec<PathBuf>,
}

/// `__api__ file` 的返回体：单个别名文件的位置信息。
#[derive(Serialize)]
struct AliasFileInfo {
    key: String,
    scope: &'static str,
    path: PathBuf,
    dir: PathBuf,
}

/// `__api__ file --list` 的返回体：全部别名文件的位置信息。
#[derive(Serialize)]
struct FilesResponse {
    files: Vec<AliasFileInfo>,
}

/// 只按文件名枚举两处目录；不解析内容，坏文件与链接文件照样列出。
/// 先全局后本地，各自按 key 排序。
fn alias_files(layout: &PathLayout, workdir: &Path) -> Vec<AliasFileInfo> {
    let mut files = Vec::new();
    for (key, path) in aliases::list_alias_files(&layout.alias_dir, true) {
        files.push(alias_file_info(
            key,
            filecmd::FileScope::Global.as_str(),
            path,
        ));
    }
    for (key, path) in aliases::list_alias_files(workdir, false) {
        files.push(alias_file_info(
            key,
            filecmd::FileScope::Local.as_str(),
            path,
        ));
    }
    files
}

fn alias_file_info(key: String, scope: &'static str, path: PathBuf) -> AliasFileInfo {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    AliasFileInfo {
        key,
        scope,
        path,
        dir,
    }
}

fn current_workdir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
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
                None => fail("completion", "missing <shell> (zsh|bash|fish)"),
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
                _ => fail("resolve", "missing <key>"),
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
                fail("check", "takes no arguments");
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
                let workdir = current_workdir();
                let payload = PathsResponse {
                    root: layout.root_dir.clone(),
                    alias: layout.alias_dir.clone(),
                    alias_files: alias_files(layout, &workdir)
                        .into_iter()
                        .map(|file| file.path)
                        .collect(),
                    workdir,
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
                );
            }
            // 带名字：只打印那一个值的裸字符串，不引号、不转义，直接进 `$(...)`
            Some(name) => {
                if args.len() > 2 {
                    fail("paths", "too many arguments (expected [name])");
                }
                match path_for(name, layout) {
                    Some(path) => println!("{}", path.display()),
                    None => fail(
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
        Some("file") => match args.get(1).map(String::as_str) {
            Some("--list") => {
                if args.len() > 2 {
                    fail("file", "too many arguments (expected --list)");
                }
                let workdir = current_workdir();
                let payload = FilesResponse {
                    files: alias_files(layout, &workdir),
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
                );
            }
            Some(option) if option.starts_with('-') => fail(
                "file",
                &format!("unknown option {:?} (expected --list)", option),
            ),
            Some(key) if !key.is_empty() => {
                if args.len() > 2 {
                    fail("file", "too many arguments (expected <file-key>)");
                }
                let address = match filecmd::address_for_key(key, layout) {
                    Ok(address) => address,
                    Err(message) => fail("file", &message),
                };
                let payload =
                    alias_file_info(key.to_string(), address.scope.as_str(), address.path);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
                );
            }
            _ => fail("file", "missing <file-key> (or use --list)"),
        },
        Some("expand") => {
            if args.len() > 2 {
                fail("expand", "too many arguments (expected <path>)");
            }
            let raw = match args.get(1) {
                Some(path) if !path.is_empty() => path,
                _ => fail("expand", "missing <path>"),
            };

            let cwd = current_workdir();
            let target = if Path::new(raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                cwd.join(raw)
            };

            let metadata = match fs::metadata(&target) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    fail("expand", &format!("file not found: {}", target.display()))
                }
                Err(e) => fail(
                    "expand",
                    &format!("failed to inspect {}: {}", target.display(), e),
                ),
            };
            if metadata.is_dir() {
                fail(
                    "expand",
                    &format!("refusing to read directory: {}", target.display()),
                );
            }
            if !metadata.is_file() {
                fail(
                    "expand",
                    &format!("not a regular file: {}", target.display()),
                );
            }

            let content = match fs::read(&target) {
                Ok(content) => content,
                Err(e) => fail(
                    "expand",
                    &format!("failed to read {}: {}", target.display(), e),
                ),
            };
            let mut value: serde_json::Value = match serde_json::from_slice(&content) {
                Ok(value) => value,
                Err(e) => fail(
                    "expand",
                    &format!("invalid JSON in {}: {}", target.display(), e),
                ),
            };

            for issue in expand_document(&mut value, &layout.root_dir, &cwd, &dir_of(&target)) {
                eprintln!(
                    "Warning: {} in {}",
                    describe_issue(&issue),
                    target.display()
                );
            }

            match serde_json::to_string_pretty(&value) {
                Ok(output) => println!("{}", output),
                Err(e) => fail("expand", &format!("failed to render JSON: {}", e)),
            }
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

fn fail(request: &str, message: &str) -> ! {
    eprintln!("__api__ {}: {}", request, message);
    std::process::exit(1);
}
