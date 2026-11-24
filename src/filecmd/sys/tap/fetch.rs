//! 取一份仓库：`git clone --depth 1`；把它拉到上游当前位置：`fetch` + `reset --hard`。
//!
//! 这里曾经有一条 curl + tar 的兜底，后来砍掉了：两条路的产物不一样（有没有 `.git`），
//! 语义要解释两遍，故障面也多一倍（HTTP 状态、tar 是否存在、归档顶层目录名不可预测）。
//! Homebrew 的 tap 同样要求系统里有 git；byk 的其它下载（`clone` 远程、`$deps`）
//! 仍然只用 curl——`@@@tap` 需要 git 并不是给整个 CLI 加依赖。
//!
//! 落盘后一定要确认仓库根有 `byk.tap.json`：本地索引一旦存在就是权威，
//! 装一个坏索引会让该仓库所有 key 的 `--tap` 直接失败，byk 不能自己制造这种状态。
//! 更新同理——上游把索引删了，这次更新就退回去，不让 checkout 停在坏状态。

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

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

/// 浅克隆一份仓库；参数与错误文案是 `add` 的门面。
fn clone_with_git(repo: &Repo, dest: &Path) -> Result<(), String> {
    let mut command = crate::net::git();
    command.arg("clone").args(["--depth", "1", "--quiet"]);
    if repo.reference != DEFAULT_REF {
        command.args(["--branch", &repo.reference]);
    }
    // `--` 之后一律按位置参数解析，URL 不会被当成 git 选项
    command.arg("--").arg(git_url(repo)).arg(dest);

    let output = run_git("Installing", &mut command)?;
    if output.status.success() {
        return Ok(());
    }

    let hint = if repo.url.is_some() {
        "\nHint: check the source URL and ref"
    } else {
        "\nHint: check the user, repository, and ref"
    };
    Err(git_failure("clone", repo, &stderr_of(&output), hint))
}

/// 一次更新的结果：更新前后各一个短提交号，够说明「动没动」。
pub(super) struct Change {
    pub(super) from: String,
    pub(super) to: String,
}

impl Change {
    /// 上游有没有给出新提交。
    pub(super) fn is_new(&self) -> bool {
        self.from != self.to
    }
}

/// 把一份已经装好的仓库搬到上游当前的位置；失败时 checkout 保持原样。
///
/// 用 `fetch` + `reset --hard` 而不是 `pull`：tap 是只读的镜像，不需要合并语义，
/// 这样上游 force push 过的 ref 也能跟上，浅克隆也不会被悄悄补成完整历史。
pub(super) fn update(repo: &Repo, dir: &Path) -> Result<Change, String> {
    if !dir.is_dir() {
        return Err(format!(
            "Tap repository is missing: {}\nHint: install it again with `byk @@@tap add {}`",
            dir.display(),
            repo.key
        ));
    }
    // 手工拷进来的目录没有 `.git`，没有可更新的历史
    if !dir.join(".git").exists() {
        return Err(format!(
            "Not a git repository: {}\nHint: update only works on repositories that `add` cloned; move it aside and run `byk @@@tap add {}`",
            dir.display(),
            repo.key
        ));
    }
    // 本地动过就不碰：`reset --hard` 会把这些改动一起抹掉
    if has_local_changes(dir)? {
        return Err(format!(
            "Tap repository has local changes: {}\nHint: commit or move them aside, or `byk @@@tap remove {}` and add it again",
            repo.key, repo.key
        ));
    }

    let before = revision(dir)?;
    fetch_ref(dir, repo)?;
    reset_hard(dir, "FETCH_HEAD")?;
    let after = revision(dir)?;

    // 索引是 `--tap` 的权威：上游删了它，这个仓库的所有 key 都会坏，
    // 所以宁可退回原来的提交，也不留一个坏 checkout
    if after != before
        && let Err(message) = validate(dir)
    {
        let _ = reset_hard(dir, &before);
        return Err(format!(
            "{message}\nHint: the update was rolled back to {}",
            short(&before)
        ));
    }

    Ok(Change {
        from: short(&before).to_string(),
        to: short(&after).to_string(),
    })
}

/// 每条命令都靠 `-C` 钉在 checkout 里，byk 自己的 cwd 不动。
fn git_in(dir: &Path) -> Command {
    let mut command = crate::net::git();
    command.arg("-C").arg(dir);
    command
}

fn has_local_changes(dir: &Path) -> Result<bool, String> {
    let mut command = git_in(dir);
    command.args(["status", "--porcelain"]);
    let output = run_git("Updating", &mut command)?;
    if !output.status.success() {
        return Err(format!(
            "Failed to read the status of {}\n{}",
            dir.display(),
            stderr_of(&output)
        ));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn revision(dir: &Path) -> Result<String, String> {
    let mut command = git_in(dir);
    command.args(["rev-parse", "HEAD"]);
    let output = run_git("Updating", &mut command)?;
    if !output.status.success() {
        return Err(format!(
            "Failed to read the revision of {}\n{}",
            dir.display(),
            stderr_of(&output)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// `HEAD` 交给远端解析，就是它默认分支的当前提交；固定 ref 直接取那个 ref。
///
/// `--force` 是为了上游 force push 过的分支与移动过的 tag：镜像只跟终点，不留本地历史。
fn fetch_ref(dir: &Path, repo: &Repo) -> Result<(), String> {
    let mut command = git_in(dir);
    command.args(["fetch", "--depth", "1", "--force", "--quiet", "origin"]);
    command.arg(&repo.reference);

    let output = run_git("Updating", &mut command)?;
    if output.status.success() {
        return Ok(());
    }

    let hint = format!(
        "\nHint: if the ref is gone upstream, `byk @@@tap remove {}` + `add` reinstalls it",
        repo.key
    );
    Err(git_failure("update", repo, &stderr_of(&output), &hint))
}

fn reset_hard(dir: &Path, target: &str) -> Result<(), String> {
    let mut command = git_in(dir);
    command.args(["reset", "--hard", "--quiet", target]);
    let output = run_git("Updating", &mut command)?;
    if !output.status.success() {
        return Err(format!(
            "Failed to move {} to {}\n{}",
            dir.display(),
            target,
            stderr_of(&output)
        ));
    }
    Ok(())
}

/// 跑一条 git；「没有 git」这条最常见的失败翻译成人话，其余原样上抛。
fn run_git(verb: &str, command: &mut Command) -> Result<Output, String> {
    // 私有仓库要求输入密码时立刻失败，别挂着等人
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            format!(
                "{verb} a tap repository requires git\nHint: install git and retry; `@@@tap list` and `remove` work without git"
            )
        } else {
            format!("Failed to run git: {}", e)
        }
    })
}

/// 失败文案：动作 + 仓库 + git 的原话 + 一条对症的建议。
fn git_failure(action: &str, repo: &Repo, reason: &str, otherwise: &str) -> String {
    let mut message = format!("Failed to {action} {}@{}", repo.key, repo.reference);
    if !reason.is_empty() {
        message.push('\n');
        message.push_str(reason);
    }
    message.push_str(if looks_like_network(reason) {
        "\nHint: the network cannot reach the source; set BYK_PROXY and retry"
    } else {
        otherwise
    });
    message
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// 显示用的短提交号；git 给的是十六进制，切前七个字节是安全的。
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
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

    #[test]
    fn trims_the_commit_to_seven_characters() {
        assert_eq!(short("1a2b3c4d5e6f"), "1a2b3c4");
        assert_eq!(short("abc"), "abc");
    }

    #[test]
    fn change_is_new_only_when_the_commit_moved() {
        let same = Change {
            from: "1a2b3c4".to_string(),
            to: "1a2b3c4".to_string(),
        };
        assert!(!same.is_new());

        let moved = Change {
            from: "1a2b3c4".to_string(),
            to: "5d6e7f8".to_string(),
        };
        assert!(moved.is_new());
    }

    #[test]
    fn failure_messages_name_the_action_and_the_repository() {
        let message = git_failure(
            "update",
            &repo("v1"),
            "fatal: couldn't find remote ref v1",
            "\nHint: check the ref",
        );

        assert_eq!(
            message,
            "Failed to update aaa/bb@v1\nfatal: couldn't find remote ref v1\nHint: check the ref"
        );
    }

    #[test]
    fn failure_messages_prefer_the_proxy_hint_for_transport_errors() {
        let message = git_failure(
            "update",
            &repo(DEFAULT_REF),
            "fatal: unable to access 'https://github.com/a/b.git/': CONNECT tunnel failed",
            "\nHint: check the ref",
        );

        assert!(message.contains("set BYK_PROXY and retry"), "{message}");
        assert!(!message.contains("check the ref"), "{message}");
    }

    #[test]
    fn a_failure_keeps_an_empty_reason_out_of_the_message() {
        let message = git_failure("clone", &repo(DEFAULT_REF), "", "\nHint: retry");

        assert_eq!(
            message, "Failed to clone aaa/bb@HEAD\nHint: retry",
            "没有 stderr 时不该留一个空行"
        );
    }
}
