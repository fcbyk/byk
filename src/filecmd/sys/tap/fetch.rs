//! 取一份仓库：优先 `git clone --depth 1`，没有 git 才退回 curl + tar。
//!
//! 顺序不能反着偏好 curl，因为两条路的产物不一样：
//! - git 给的是真仓库（浅克隆），用户事后可以自己 `git pull`；
//! - tarball 给的是快照，没有 `.git`，要更新只能重新装。
//!
//! 解包那条路的临时物都以点号开头留在 `tap/` 根部——扫描器只认合法的
//! `<user>/<repo>` 目录名，这些名字会被跳过，不会污染 `clone --tap` 的查找。
//!
//! 落盘后一定要确认仓库根有 `byk.tap.json`：本地索引一旦存在就是权威，
//! 装一个坏索引会让该仓库所有 key 的 `--tap` 直接失败，byk 不能自己制造这种状态。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::DEFAULT_REF;
use super::Repo;

use crate::filecmd::source::encode_segment;
use crate::model::PathLayout;

/// 仓库根的索引文件；与 `clone --tap` 用的是同一个名字。
const INDEX_FILE: &str = "byk.tap.json";

/// 安装一份仓库；任何一步失败都不留下半个目录。
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

    let result = fetch_into(repo, layout, &dest).and_then(|()| validate(&dest));
    if result.is_err() {
        // 失败连空的 user 目录一起清掉：`ls tap/` 不该出现装了一半的用户
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::remove_dir(parent);
    }
    result
}

fn fetch_into(repo: &Repo, layout: &PathLayout, dest: &Path) -> Result<(), String> {
    match clone_with_git(repo, dest) {
        Ok(()) => Ok(()),
        Err(GitFailure::Missing) => unpack_from_archive(repo, layout, dest),
        Err(GitFailure::Failed(message)) => Err(message),
    }
}

enum GitFailure {
    /// 系统里没有 git：调用方该退回 curl。
    Missing,
    Failed(String),
}

fn clone_with_git(repo: &Repo, dest: &Path) -> Result<(), GitFailure> {
    let mut command = crate::net::git();
    command.arg("clone").args(["--depth", "1", "--quiet"]);
    if repo.reference != DEFAULT_REF {
        command.args(["--branch", &repo.reference]);
    }
    command.arg(git_url(repo)).arg(dest);

    let output = command.output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            GitFailure::Missing
        } else {
            GitFailure::Failed(format!("Failed to run git: {}", e))
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
        "\nHint: the network cannot reach GitHub; set BYK_PROXY and retry"
    } else {
        "\nHint: check the user, repository, and ref"
    });
    Err(GitFailure::Failed(message))
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

fn git_url(repo: &Repo) -> String {
    format!(
        "https://github.com/{}/{}.git",
        encode_segment(&repo.user),
        encode_segment(&repo.repo)
    )
}

/// 没有 git 时的退路：下载 tarball、解压、把唯一顶层目录 rename 到位。
fn unpack_from_archive(repo: &Repo, layout: &PathLayout, dest: &Path) -> Result<(), String> {
    let tap_dir = layout.tap_dir();
    let archive = tap_dir.join(format!(".{}.{}.tar.gz", repo.user, repo.repo));
    let unpacked = tap_dir.join(format!(".{}.{}.unpack", repo.user, repo.repo));

    let result = (|| {
        let url = tarball_url(repo);
        download(&url, &archive)?;
        extract(&archive, &unpacked)?;

        let top = only_directory(&unpacked)?;
        fs::rename(&top, dest).map_err(|e| {
            format!(
                "Failed to move {} to {}: {}",
                top.display(),
                dest.display(),
                e
            )
        })
    })();

    // 归档用完就删；解压目录成功时已被 rename 走，只剩一个空壳
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir_all(&unpacked);
    result
}

/// `https://codeload.github.com/<user>/<repo>/tar.gz/<ref>`，ref 里的 `/` 按段编码。
fn tarball_url(repo: &Repo) -> String {
    let reference = repo
        .reference
        .split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/");

    format!(
        "https://codeload.github.com/{}/{}/tar.gz/{}",
        encode_segment(&repo.user),
        encode_segment(&repo.repo),
        reference
    )
}

fn download(url: &str, archive: &Path) -> Result<(), String> {
    // 不用 `-f`：HTTP 错误码经 `-w` 拿到，比 curl 的退出码可靠
    // （同一个 404 在不同版本上会报 22 或 56，文案也各不相同）。
    let output = crate::net::curl()
        .args(["-sSL", "-o"])
        .arg(archive)
        .args(["-w", "%{http_code}"])
        .args(["--", url])
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                "Installing a tap repository requires git or curl\nHint: install git (a shallow clone) or curl (a tarball fallback)"
                    .to_string()
            } else {
                format!("Failed to run curl: {}", e)
            }
        })?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.trim();
        let mut message = if reason.is_empty() {
            format!("curl failed (exit {}) for {}", code, url)
        } else {
            format!("curl failed (exit {}) for {}: {}", code, url, reason)
        };
        message.push_str("\nHint: the network cannot reach GitHub; set BYK_PROXY and retry");
        return Err(message);
    }

    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if status == "200" {
        return Ok(());
    }

    Err(format!(
        "Tap repository not found: {}\nServer said: HTTP {}\nHint: check the user, repository, and ref",
        url, status
    ))
}

fn extract(archive: &Path, into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| format!("Failed to create {}: {}", into.display(), e))?;

    let output = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                "Extracting a tap repository requires tar\nHint: install tar, or install git and retry"
                    .to_string()
            } else {
                format!("Failed to run tar: {}", e)
            }
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "Failed to extract {}: {}",
        archive.display(),
        stderr.trim()
    ))
}

/// 归档里只应有一个顶层目录。
///
/// 名字不可预测——`codeload` 会把 `v1.0` 写成 `repo-1.0`——所以不拼名字，
/// 而是读完整个解压目录后取唯一那一项。
fn only_directory(root: &Path) -> Result<PathBuf, String> {
    let entries =
        fs::read_dir(root).map_err(|e| format!("Failed to read {}: {}", root.display(), e))?;

    let mut dirs = Vec::new();
    let mut others = 0;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read {}: {}", root.display(), e))?;
        if entry.path().is_dir() {
            dirs.push(entry.path());
        } else {
            others += 1;
        }
    }

    match dirs.len() {
        1 if others == 0 => Ok(dirs.remove(0)),
        _ => Err(format!(
            "Unexpected archive layout: expected one top-level directory in {}",
            root.display()
        )),
    }
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
        }
    }

    #[test]
    fn builds_the_clone_and_tarball_urls() {
        assert_eq!(git_url(&repo("HEAD")), "https://github.com/aaa/bb.git");
        assert_eq!(
            tarball_url(&repo("HEAD")),
            "https://codeload.github.com/aaa/bb/tar.gz/HEAD"
        );
        assert_eq!(
            tarball_url(&repo("feature/x")),
            "https://codeload.github.com/aaa/bb/tar.gz/feature/x"
        );
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
    fn takes_the_only_top_level_directory() {
        let temp = tempfile::tempdir().unwrap();
        let top = temp.path().join("bb-1.0");
        fs::create_dir(&top).unwrap();

        assert_eq!(only_directory(temp.path()).unwrap(), top);
    }

    #[test]
    fn rejects_an_archive_with_more_than_one_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("a")).unwrap();
        fs::create_dir(temp.path().join("b")).unwrap();

        let err = only_directory(temp.path()).unwrap_err();
        assert!(err.starts_with("Unexpected archive layout:"), "{err}");

        fs::remove_dir_all(temp.path().join("b")).unwrap();
        fs::write(temp.path().join("stray"), "x").unwrap();
        assert!(
            only_directory(temp.path()).is_err(),
            "多出散文件也算形态不对"
        );
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

    #[test]
    fn extracts_a_github_shaped_archive() {
        // 解压靠系统 tar；没有 tar 的环境直接跳过，别把缺工具当成回归
        if Command::new("tar").arg("--version").output().is_err() {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        let top = work.join("bb-HEAD");
        fs::create_dir_all(&top).unwrap();
        fs::write(top.join(INDEX_FILE), r#"{"build": "b.byk.json"}"#).unwrap();

        let archive = temp.path().join("bb.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&work)
            .arg("bb-HEAD")
            .status()
            .unwrap();
        assert!(status.success());

        let into = temp.path().join("out");
        extract(&archive, &into).unwrap();
        let root = only_directory(&into).unwrap();
        validate(&root).unwrap();

        assert_eq!(root.file_name().unwrap(), "bb-HEAD");
    }
}
