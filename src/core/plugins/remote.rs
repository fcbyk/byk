//! 远程分发：`byk add user/repo/path` 拉取远程 *.byk.json 并安装。

use std::sync::mpsc;
use std::thread;

use colored::Colorize;

use crate::core::download;
use crate::core::paths::PathLayout;

/// 解析远程 spec：`user/repo[@branch]/path/to/file` → (owner, repo, branch, path)。
/// path 可为多级（`aaa/bbb`），空字符串表示不带 path（由调用方报错）。
fn parse_remote_spec(spec: &str) -> Option<(&str, &str, &str, &str)> {
    let (owner, rest) = spec.split_once('/')?;
    let (repo_part, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (repo, branch) = repo_part.split_once('@').unwrap_or((repo_part, "main"));
    Some((owner, repo, branch, path))
}

/// 拼接 *.byk.json 的远程 URL（path 无后缀时自动补 `.byk.json`）。
fn build_file_url(owner: &str, repo: &str, branch: &str, path: &str, cdn: bool) -> String {
    let file = if path.ends_with(".byk.json") {
        path.to_string()
    } else {
        format!("{}.byk.json", path)
    };
    if cdn {
        format!(
            "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
            owner, repo, branch, file
        )
    } else {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            owner, repo, branch, file
        )
    }
}

/// 从 path 提取 stem（最后一段，去 `.byk.json` 后缀）。
fn stem_from_path(path: &str) -> &str {
    let last = path.rsplit('/').next().unwrap_or(path);
    last.strip_suffix(".byk.json").unwrap_or(last)
}

/// 提取 path 的父目录，空字符串表示仓库根目录。
/// `aaa/bbb/ccc` → `"aaa/bbb"`，`ccc` → `""`。
fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(p, _)| p)
}

/// 拼接 raw.githubusercontent.com 格式的 URL（不含 CDN 转换）。
fn build_raw_url(owner: &str, repo: &str, branch: &str, file_path: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        owner, repo, branch, file_path
    )
}

/// 仓库根目录的 raw base URL（末尾带 /）。
/// 如 `https://raw.githubusercontent.com/u/r/main/`
fn raw_repo_base(owner: &str, repo: &str, branch: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/",
        owner, repo, branch
    )
}

/// 仓库根目录的 CDN base URL（末尾带 /）。
/// 如 `https://cdn.jsdelivr.net/gh/u/r@main/`
fn cdn_repo_base(owner: &str, repo: &str, branch: &str) -> String {
    format!(
        "https://cdn.jsdelivr.net/gh/{}/{}@{}/",
        owner, repo, branch
    )
}

/// 拼接仓库内目录的 base URL（末尾带 /），按需走 CDN。
/// parent 为空 → 仓库根目录；否则为 `parent/`
fn repo_base_url(owner: &str, repo: &str, branch: &str, parent: &str, cdn: bool) -> String {
    let base = if cdn {
        cdn_repo_base(owner, repo, branch)
    } else {
        raw_repo_base(owner, repo, branch)
    };
    if parent.is_empty() {
        base
    } else {
        format!("{}{}/", base, parent)
    }
}

/// 将 raw URL 按需转为 CDN URL。
fn to_cdn_if(raw_url: &str, cdn: bool) -> String {
    if cdn {
        download::to_jsdelivr_url(raw_url)
    } else {
        raw_url.to_string()
    }
}

/// 解析 $ref 值，返回最终 URL 和 stem。
/// - 绝对 URL：若是 raw.githubusercontent.com 则按需转 CDN，否则原样使用
/// - 相对路径：通过 `repo_base_url` 直接拼出 CDN/raw URL
fn resolve_ref_url(
    ref_value: &str,
    owner: &str,
    repo: &str,
    branch: &str,
    parent: &str,
    cdn: bool,
) -> (String, String) {
    let stem = stem_from_path(ref_value).to_string();

    if ref_value.starts_with("http://") || ref_value.starts_with("https://") {
        (to_cdn_if(ref_value, cdn), stem)
    } else {
        let base = repo_base_url(owner, repo, branch, parent, cdn);
        (format!("{}{}", base, ref_value), stem)
    }
}

/// 尝试通过匿名 `.byk.json` 的 $ref 回退查找。
/// 成功返回 (最终 URL, stem)，失败返回错误描述。
fn try_resolve_via_ref(
    owner: &str,
    repo: &str,
    branch: &str,
    path: &str,
    cdn: bool,
) -> Result<(String, String), String> {
    let key = stem_from_path(path);
    let parent = parent_dir(path).unwrap_or("");

    let anon_path = if parent.is_empty() {
        ".byk.json".to_string()
    } else {
        format!("{}/.byk.json", parent)
    };
    let anon_raw = build_raw_url(owner, repo, branch, &anon_path);
    let anon_url = to_cdn_if(&anon_raw, cdn);

    let anon_content = download::download_string(&anon_url)?;

    let anon_json: serde_json::Value =
        serde_json::from_str(&anon_content).map_err(|e| format!("invalid JSON: {}", e))?;

    let ref_map = anon_json.get("$ref").and_then(|v| v.as_object());
    let ref_value = ref_map
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_str());

    match ref_value {
        Some(v) => Ok(resolve_ref_url(v, owner, repo, branch, parent, cdn)),
        None => Err(format!("no $ref entry for '{}'", key)),
    }
}

/// `byk add user/repo[/path]`：拉取远程 *.byk.json 并安装为全局插件。
/// 无 path 时默认在仓库根目录查找与仓库同名的 `{repo}.byk.json`。
/// 主文件与 $ref 回退并发请求，谁先成功用谁。
pub fn install_remote(spec: &str, layout: &PathLayout, cdn: bool) {
    let (owner, repo, branch, mut path) = match parse_remote_spec(spec) {
        Some(x) => x,
        None => {
            eprintln!("{} invalid remote spec: {}", "Error:".red(), spec.bold());
            eprintln!("  Expected: byk add <user>/<repo>[/<path>]");
            std::process::exit(1);
        }
    };
    if path.is_empty() {
        path = repo;
    }

    let stem = stem_from_path(path);
    let primary_url = build_file_url(owner, repo, branch, path, cdn);

    println!(
        "{} Fetching {} from {}/{}",
        "==>".cyan().bold(),
        format!("{}.byk.json", stem).bold(),
        format!("{}/{}", owner, repo).dimmed(),
        branch.dimmed()
    );

    // 并发：同时尝试主 URL 和 $ref 回退，取先成功者。内容直接下载到内存。
    let (tx, rx) = mpsc::channel();

    // Thread 1: 尝试主 URL
    let tx1 = tx.clone();
    let p_url = primary_url.clone();
    let p_stem = stem.to_string();
    thread::spawn(move || {
        match download::download_string(&p_url) {
            Ok(content) => {
                let _ = tx1.send(Ok((p_url, p_stem, content)));
            }
            Err(e) => {
                let _ = tx1.send(Err(format!("primary: {}", e)));
            }
        }
    });

    // Thread 2: 尝试 $ref 回退
    let tx2 = tx.clone();
    let owner2 = owner.to_string();
    let repo2 = repo.to_string();
    let branch2 = branch.to_string();
    let path2 = path.to_string();
    thread::spawn(move || {
        match try_resolve_via_ref(&owner2, &repo2, &branch2, &path2, cdn) {
            Ok((ref_url, ref_stem)) => {
                match download::download_string(&ref_url) {
                    Ok(content) => {
                        let _ = tx2.send(Ok((ref_url, ref_stem, content)));
                    }
                    Err(e) => {
                        let _ = tx2.send(Err(format!("$ref download: {}", e)));
                    }
                }
            }
            Err(e) => {
                let _ = tx2.send(Err(format!("$ref: {}", e)));
            }
        }
    });

    // 收集结果：取第一个成功
    let mut last_err = String::new();
    let mut winner: Option<(String, String, String)> = None; // (url, stem, content)
    for _ in 0..2 {
        match rx.recv() {
            Ok(Ok(result)) => {
                winner = Some(result);
                break;
            }
            Ok(Err(e)) => {
                last_err = e;
            }
            Err(_) => break,
        }
    }

    let (final_url, final_stem, content) = match winner {
        Some(w) => w,
        None => {
            eprintln!(
                "{} failed to fetch {}: {}",
                "Error:".red(),
                format!("{}.byk.json", stem).bold(),
                last_err
            );
            std::process::exit(1);
        }
    };

    // 打印下载确认
    println!(
        "{} Downloaded {} ({})",
        "==>".cyan().bold(),
        format!("{}.byk.json", final_stem).bold(),
        format_size(content.len())
    );

    // 落点冲突：全局已有同名文件 → 提示覆盖（幂等）
    let dest = layout.alias_dir.join(format!("{}.byk.json", final_stem));
    if dest.exists() {
        println!(
            "{} {} already exists, overwriting.",
            "*".dimmed(),
            dest.display().to_string().dimmed()
        );
    }

    // 安装：remote_base 取 final_url 的目录部分，供 $assets 相对路径拼 CDN
    let remote_base = final_url.rsplit_once('/')
        .map(|(dir, _)| format!("{}/", dir))
        .unwrap_or_else(|| final_url.clone());
    let file_key = format!("@@{}", final_stem);
    super::install_from_content(&content, &final_url, &file_key, &final_stem, layout, cdn, Some(&remote_base), None);
}

/// 字节数 -> 人类可读大小。
fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_spec_simple_path() {
        let (o, r, b, p) = parse_remote_spec("user/repo/pysdk").unwrap();
        assert_eq!((o, r, b, p), ("user", "repo", "main", "pysdk"));
    }

    #[test]
    fn remote_spec_multi_level_path() {
        let (o, r, b, p) = parse_remote_spec("user/repo/tools/lint").unwrap();
        assert_eq!((o, r, b, p), ("user", "repo", "main", "tools/lint"));
    }

    #[test]
    fn remote_spec_branch() {
        let (o, r, b, p) = parse_remote_spec("user/repo@dev/hello").unwrap();
        assert_eq!((o, r, b, p), ("user", "repo", "dev", "hello"));
    }

    #[test]
    fn remote_spec_no_path() {
        let (o, r, b, p) = parse_remote_spec("user/repo").unwrap();
        assert_eq!((o, r, b, p), ("user", "repo", "main", ""));
    }

    #[test]
    fn remote_spec_invalid() {
        assert!(parse_remote_spec("onlyone").is_none());
        assert!(parse_remote_spec("").is_none());
    }

    #[test]
    fn file_url_raw_and_cdn() {
        let raw = build_file_url("u", "r", "main", "pysdk", false);
        assert_eq!(
            raw,
            "https://raw.githubusercontent.com/u/r/main/pysdk.byk.json"
        );
        let cdn = build_file_url("u", "r", "dev", "tools/lint", true);
        assert_eq!(cdn, "https://cdn.jsdelivr.net/gh/u/r@dev/tools/lint.byk.json");
    }

    #[test]
    fn file_url_keeps_existing_suffix() {
        let url = build_file_url("u", "r", "main", "x.byk.json", false);
        assert_eq!(url, "https://raw.githubusercontent.com/u/r/main/x.byk.json");
    }

    #[test]
    fn stem_from_path_forms() {
        assert_eq!(stem_from_path("pysdk"), "pysdk");
        assert_eq!(stem_from_path("tools/lint"), "lint");
        assert_eq!(stem_from_path("x.byk.json"), "x");
    }

    #[test]
    fn parent_dir_forms() {
        assert_eq!(parent_dir("aaa/bbb/ccc"), Some("aaa/bbb"));
        assert_eq!(parent_dir("aaa/bbb"), Some("aaa"));
        assert_eq!(parent_dir("ccc"), None);
    }

    #[test]
    fn resolve_ref_url_relative() {
        let (url, stem) = resolve_ref_url("sub/plugin", "u", "r", "main", "aaa/bbb", false);
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/u/r/main/aaa/bbb/sub/plugin"
        );
        assert_eq!(stem, "plugin");
    }

    #[test]
    fn resolve_ref_url_relative_root() {
        let (url, stem) = resolve_ref_url("plugin", "u", "r", "main", "", false);
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/u/r/main/plugin"
        );
        assert_eq!(stem, "plugin");
    }

    #[test]
    fn resolve_ref_url_absolute() {
        let (url, stem) = resolve_ref_url(
            "https://example.com/x.json",
            "u", "r", "main", "aaa", false,
        );
        assert_eq!(url, "https://example.com/x.json");
        assert_eq!(stem, "x.json");
    }

    #[test]
    fn resolve_ref_url_with_byk_suffix() {
        let (url, stem) = resolve_ref_url("sub/x.byk.json", "u", "r", "main", "dir", false);
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/u/r/main/dir/sub/x.byk.json"
        );
        assert_eq!(stem, "x");
    }

    #[test]
    fn resolve_ref_url_cross_dir() {
        let (url, stem) = resolve_ref_url("../shared/plugin", "u", "r", "main", "aaa/bbb", false);
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/u/r/main/aaa/bbb/../shared/plugin"
        );
        assert_eq!(stem, "plugin");
    }

    #[test]
    fn build_raw_url_forms() {
        assert_eq!(
            build_raw_url("u", "r", "main", "pysdk.byk.json"),
            "https://raw.githubusercontent.com/u/r/main/pysdk.byk.json"
        );
        assert_eq!(
            build_raw_url("u", "r", "main", "dir/.byk.json"),
            "https://raw.githubusercontent.com/u/r/main/dir/.byk.json"
        );
    }

    #[test]
    fn to_cdn_if_converts() {
        let raw = "https://raw.githubusercontent.com/u/r/main/x.json";
        assert_eq!(to_cdn_if(raw, false), raw);
        assert_eq!(
            to_cdn_if(raw, true),
            "https://cdn.jsdelivr.net/gh/u/r@main/x.json"
        );
    }

    #[test]
    fn resolve_ref_url_cdn_mode() {
        let (url, _stem) = resolve_ref_url("sub/plugin", "u", "r", "main", "aaa/bbb", true);
        assert_eq!(
            url,
            "https://cdn.jsdelivr.net/gh/u/r@main/aaa/bbb/sub/plugin"
        );
    }

    #[test]
    fn resolve_ref_url_absolute_with_cdn() {
        let (url, _stem) = resolve_ref_url(
            "https://raw.githubusercontent.com/u/r/main/x.byk.json",
            "u", "r", "main", "", true,
        );
        assert_eq!(
            url,
            "https://cdn.jsdelivr.net/gh/u/r@main/x.byk.json"
        );
    }

    #[test]
    fn raw_repo_base_forms() {
        assert_eq!(
            raw_repo_base("u", "r", "main"),
            "https://raw.githubusercontent.com/u/r/main/"
        );
    }

    #[test]
    fn cdn_repo_base_forms() {
        assert_eq!(
            cdn_repo_base("u", "r", "main"),
            "https://cdn.jsdelivr.net/gh/u/r@main/"
        );
    }

    #[test]
    fn repo_base_url_root_and_subdir() {
        assert_eq!(
            repo_base_url("u", "r", "main", "", false),
            "https://raw.githubusercontent.com/u/r/main/"
        );
        assert_eq!(
            repo_base_url("u", "r", "main", "tools", false),
            "https://raw.githubusercontent.com/u/r/main/tools/"
        );
        assert_eq!(
            repo_base_url("u", "r", "main", "tools", true),
            "https://cdn.jsdelivr.net/gh/u/r@main/tools/"
        );
    }
}