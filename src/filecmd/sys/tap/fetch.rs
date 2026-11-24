//! 取一份仓库：`git clone --depth 1`，只此一条路。
//!
//! 这里曾经有一条 curl + tar 的兜底，后来砍掉了：两条路的产物不一样（有没有 `.git`），
//! 语义要解释两遍，故障面也多一倍（HTTP 状态、tar 是否存在、归档顶层目录名不可预测）。
//! Homebrew 的 tap 同样要求系统里有 git；byk 的其它下载（`clone` 远程、`$deps`）
//! 仍然只用 curl——`@@@tap` 需要 git 并不是给整个 CLI 加依赖。
//!
//! 落盘后一定要确认仓库根有 `byk.tap.json`：本地索引一旦存在就是权威，
//! 装一个坏索引会让该仓库所有 key 的 `--tap` 直接失败，byk 不能自己制造这种状态。

use std::fs;
use std::io;
use std::path::Path;

use super::DEFAULT_REF;
use super::Repo;

use crate::filecmd::source::encode_segment;
use crate::model::PathLayout;

/// 仓库根的索引文件；与 `clone --tap` 用的是同一个名字。
const INDEX_FILE: &str = "byk.tap.json";

/// 克隆一份仓库；任何一步失败都不留下半个目录。
pub(super) fn install(repo: &Repo, layout: &PathLayout) -> Result<(), String> {
    let tap_dir = layout.tap_dir();
    fs::create_dir_all(&tap_dir)
        .map_err(|e| format!("Failed to create {}: {}", tap_dir.display(), e))?;

    let dest = tap_dir.join(&repo.user).join(&repo.repo);
    let Some(parent) = dest.parent() else {
        return Err(format!("Invalid tap directory: {}", dest.display()));
    };
    fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;

    let result = clone_with_git(repo, &dest).and_then(|()| validate(&dest));
    if result.is_err() {
        // 失败连空的 user 目录一起清掉：`ls tap/` 不该出现装了一半的用户
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::remove_dir(parent);
    }
    result
}

fn clone_with_git(repo: &Repo, dest: &Path) -> Result<(), String> {
    let mut command = crate::net::git();
    // 私有仓库要求输入密码时立刻失败，别挂着等人
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.arg("clone").args(["--depth", "1", "--quiet"]);
    if repo.reference != DEFAULT_REF {
        command.args(["--branch", &repo.reference]);
    }
    // `--` 之后一律按位置参数解析，URL 不会被当成 git 选项
    command.arg("--").arg(git_url(repo)).arg(dest);

    let output = command.output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            "Installing a tap repository requires git\nHint: install git and retry; `@@@tap list` and `remove` work without it"
                .to_string()
        } else {
            format!("Failed to run git: {}", e)
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr.trim();
    let mut message = format!("Failed to clone {}@{}", repo.key, repo.reference);
    if !reason.is_empty() {
        message.push_str(&format!("\n{}", reason));
    }
    message.push_str(if looks_like_network(reason) {
        "\nHint: the network cannot reach the source; set BYK_PROXY and retry"
    } else if repo.url.is_some() {
        "\nHint: check the source URL and ref"
    } else {
        "\nHint: check the user, repository, and ref"
    });
    Err(message)
}

/// 自定义来源原样使用；没写 URL 时默认 GitHub。
fn git_url(repo: &Repo) -> String {
    repo.url.clone().unwrap_or_else(|| {
        format!(
            "https://github.com/{}/{}.git",
            encode_segment(&repo.user),
            encode_segment(&repo.repo)
        )
    })
}

/// git 连不上与仓库不存在都要报错，但该给的建议完全不同。
fn looks_like_network(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    [
        "unable to access",
        "could not resolve",
        "connection",
        "connect tunnel",
        "timed out",
    ]
    .iter()
    .any(|marker| reason.contains(marker))
}

/// 仓库根必须有可解析的 `byk.tap.json`，否则装进去只会让 `--tap` 一直失败。
fn validate(dir: &Path) -> Result<(), String> {
    let path = dir.join(INDEX_FILE);
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "Not a tap repository: {} has no {}\nHint: the repository root must contain {}",
                dir.display(),
                INDEX_FILE,
                INDEX_FILE
            ));
        }
        Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
    };

    match serde_json::from_slice::<serde_json::Value>(&data) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        Ok(_) => Err(format!(
            "Not a tap repository: {} is not a JSON object",
            path.display()
        )),
        Err(e) => Err(format!(
            "Not a tap repository: failed to parse {}: {}",
            path.display(),
            e
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(reference: &str) -> Repo {
        Repo {
            key: "aaa/bb".to_string(),
            user: "aaa".to_string(),
            repo: "bb".to_string(),
            reference: reference.to_string(),
            url: None,
        }
    }

    fn custom(reference: &str, url: &str) -> Repo {
        Repo {
            url: Some(url.to_string()),
            ..repo(reference)
        }
    }

    #[test]
    fn builds_the_default_github_url() {
        assert_eq!(git_url(&repo("HEAD")), "https://github.com/aaa/bb.git");
    }

    #[test]
    fn uses_a_custom_source_verbatim() {
        for url in [
            "https://gitlab.com/aaa/bb.git",
            "git@gitlab.com:aaa/bb.git",
            "file:///srv/taps/bb",
            "/Users/coke/src/tap",
        ] {
            assert_eq!(git_url(&custom("HEAD", url)), url);
        }
    }

    #[test]
    fn suggests_a_proxy_only_for_transport_failures() {
        assert!(looks_like_network(
            "fatal: unable to access 'https://github.com/a/b.git/': CONNECT tunnel failed"
        ));
        assert!(looks_like_network(
            "fatal: Could not resolve host: github.com"
        ));
        assert!(!looks_like_network("fatal: repository 'a/b' not found"));
    }

    #[test]
    fn rejects_a_repository_without_an_index() {
        let temp = tempfile::tempdir().unwrap();

        let err = validate(temp.path()).unwrap_err();
        assert!(err.starts_with("Not a tap repository:"), "{err}");
        assert!(err.contains("byk.tap.json"), "{err}");
    }

    #[test]
    fn rejects_an_index_that_is_not_an_object() {
        for body in ["{not json", "[]"] {
            let temp = tempfile::tempdir().unwrap();
            fs::write(temp.path().join(INDEX_FILE), body).unwrap();

            let err = validate(temp.path()).unwrap_err();
            assert!(err.starts_with("Not a tap repository:"), "{body}: {err}");
        }
    }

    #[test]
    fn accepts_a_valid_index() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(INDEX_FILE), r#"{"build": "b.byk.json"}"#).unwrap();

        validate(temp.path()).unwrap();
    }
}
