//! 远程分发：`byk add user/repo/path` 拉取远程 *.byk.json 并安装。

use std::fs;

use colored::Colorize;

use crate::core::download;
use crate::core::paths::PathLayout;

use super::install_from_file;

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

/// `byk add user/repo/path`：拉取远程 *.byk.json 并安装为全局插件。
pub fn install_remote(spec: &str, layout: &PathLayout, cdn: bool) {
    let (owner, repo, branch, path) = match parse_remote_spec(spec) {
        Some(x) => x,
        None => {
            eprintln!("{} invalid remote spec: {}", "Error:".red(), spec.bold());
            eprintln!("  Expected: byk add <user>/<repo>[/<path>]");
            std::process::exit(1);
        }
    };
    if path.is_empty() {
        eprintln!(
            "{} remote spec '{}' is missing a file path.",
            "Error:".red(),
            spec.bold()
        );
        eprintln!("  Use: byk add <user>/<repo>/<file>  (e.g. byk add fcbyk/byk/pysdk)");
        std::process::exit(1);
    }

    let url = build_file_url(owner, repo, branch, path, cdn);
    let stem = stem_from_path(path);
    println!(
        "{} Fetching {} from {}/{}",
        "==>".cyan().bold(),
        format!("{}.byk.json", stem).bold(),
        format!("{}/{}", owner, repo).dimmed(),
        branch.dimmed()
    );

    // 下载到临时文件
    let temp_dir = layout.cache_dir.join("tmp-remote");
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        eprintln!("{} failed to create {}: {}", "Error:".red(), temp_dir.display(), e);
        std::process::exit(1);
    }
    let temp_file = temp_dir.join(format!("{}.byk.json", stem));
    if let Err(e) = download::download_script(&url, &temp_file) {
        eprintln!("{} failed to download {}: {}", "Error:".red(), url, e);
        std::process::exit(1);
    }

    // 落点冲突：全局已有同名文件 → 提示覆盖（幂等）
    let dest = layout.alias_dir.join(format!("{}.byk.json", stem));
    if dest.exists() {
        println!(
            "{} {} already exists, overwriting.",
            "*".dimmed(),
            dest.display().to_string().dimmed()
        );
    }

    // 安装（装 $pip/$assets + 校验 $plugin + 写副本）；remote_base = 文件 URL，
    // 使 $assets 的相对路径解析成相对 URL
    let file_key = format!("@@{}", stem);
    install_from_file(&temp_file, &file_key, stem, layout, cdn, Some(&url));
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
}
