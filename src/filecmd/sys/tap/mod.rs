//! `@@@tap`：把 tap 仓库装进 `~/.byk/tap/`，并记账到 `tap/index.json`。
//!
//! 这一版只干一件事：让仓库通过 CLI 落到 `clone --tap` 已经在用的那个目录里，
//! 不必再手敲 `git clone`。所以 `index.json` 只被这里读写——`clone --tap`
//! 仍然扫目录，一行都不用改，索引丢了也不影响 `--tap` 本身。
//!
//! 三条动词各管一件事：`list` 读账本、`add` 下载并登记、`remove` 注销并删除。
//! `remove` 只认账本里的键，所以手工放进 `tap/` 的仓库不会被误删。

mod fetch;
mod index;

use std::fs;
use std::io;
use std::path::PathBuf;

use colored::Colorize;
use serde_json::{Map, Value, json};

use super::store;

use crate::model::PathLayout;
use crate::utils::display::get_display_width;
use crate::utils::io::write_stdout;

/// 已注册的动词；顺序即帮助与错误提示顺序。
const VERBS: &[(&str, &str)] = &[
    ("list", "Show every installed repository"),
    ("add", "Download a repository into the tap directory"),
    ("remove", "Delete an installed repository"),
];

const USAGE: &str = "byk @@@tap [OPTIONS] [COMMAND] [ARGS]...";

/// 索引里记录的来源。目前只有 GitHub；字段留着，将来加 Gitee 时不必改格式。
const ORIGIN: &str = "github";

/// 默认 ref，与 `clone --tap` 一致。
const DEFAULT_REF: &str = "HEAD";

pub(crate) fn run(args: &[String], layout: &PathLayout) -> Result<(), String> {
    let Some((verb, rest)) = args.split_first() else {
        // 裸调用看账本：对仓库清单来说「现在装了什么」才是高频动作，
        // 「我还能做什么」交给 `-h`。
        return list(layout);
    };

    match verb.as_str() {
        "-h" | "--help" => write_stdout(render_help(layout).as_bytes()),
        "list" => {
            expect_count(rest, 0, "list takes no arguments")?;
            list(layout)
        }
        "add" => {
            let spec = single_argument(rest, "add takes one repository, e.g. aaa/bb")?;
            add(spec, layout)
        }
        "remove" => {
            let spec = single_argument(rest, "remove takes one repository, e.g. aaa/bb")?;
            remove(spec, layout)
        }
        other => Err(unknown_command(other)),
    }
}

/// 一次安装的仓库标识：`<user>/<repo>[@<ref>]`。
///
/// `key` 是索引的键，也是 `tap/<user>/<repo>` 的相对位置，永远不带 `@<ref>`。
struct Repo {
    key: String,
    user: String,
    repo: String,
    reference: String,
}

fn parse_repo(spec: &str) -> Result<Repo, String> {
    let (name, reference) = match spec.split_once('@') {
        Some((name, reference)) => {
            if !valid_reference(reference) {
                return Err(format!(
                    "Invalid ref: {:?}\nHint: write `<user>/<repo>@<ref>`, e.g. `aaa/bb@v1`",
                    reference
                ));
            }
            (name, reference)
        }
        None => (spec, DEFAULT_REF),
    };

    let Some((user, repo)) = name.split_once('/') else {
        return Err(shape_error());
    };
    if !valid_component(user) {
        return Err(invalid_component("user", user));
    }
    if !valid_component(repo) {
        return Err(invalid_component("repo", repo));
    }

    Ok(Repo {
        key: format!("{user}/{repo}"),
        user: user.to_string(),
        repo: repo.to_string(),
        reference: reference.to_string(),
    })
}

/// 目录名同时是 URL 的一段，所以只放行两边都安全的字符。
fn valid_component(value: &str) -> bool {
    (1..=100).contains(&value.len())
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// ref 只进 URL，允许 `feature/x` 这种带斜杠的分支名，但不许控制字符与空白。
fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '@')
}

fn shape_error() -> String {
    "Tap repository must be <user>/<repo>[@<ref>]\nHint: e.g. `byk @@@tap add aaa/bb`".to_string()
}

fn invalid_component(kind: &str, value: &str) -> String {
    format!(
        "Invalid repository {}: {:?}\nHint: use letters, digits, `.`, `-`, and `_`",
        kind, value
    )
}

/// 下载、校验、落到 `tap/<user>/<repo>`，最后才记账。
///
/// 顺序不能反：先有目录再写索引，写失败就删掉目录。反过来会留下一条指向
/// 空处的记录，而 `list` 之外没人能发现它。
fn add(spec: &str, layout: &PathLayout) -> Result<(), String> {
    let repo = parse_repo(spec)?;
    let dir = repo_dir(&repo.user, &repo.repo, layout);

    if dir.exists() {
        return Err(format!(
            "Tap repository already exists: {}\nHint: remove it first with `byk @@@tap remove {}`",
            dir.display(),
            repo.key
        ));
    }

    let index_path = layout.tap_index_path();
    let (object, warning) = store::read_object(&index_path);
    print_warning(warning);

    eprintln!("Fetching {}@{}", repo.key, repo.reference);
    fetch::install(&repo, layout)?;

    record(&repo, layout, object)
}

/// 记账。写不进去就把刚装好的目录删掉，回到「什么都没发生」。
fn record(repo: &Repo, layout: &PathLayout, mut object: Map<String, Value>) -> Result<(), String> {
    let dir = repo_dir(&repo.user, &repo.repo, layout);
    object.insert(
        repo.key.clone(),
        json!({ "origin": ORIGIN, "ref": repo.reference }),
    );

    if let Err(err) = store::write_object(&layout.tap_index_path(), &object) {
        // 目录已经就位，账本却写不进去：删掉刚装的那份，不留没人认领的目录
        let _ = fs::remove_dir_all(&dir);
        return Err(format!(
            "{err}\nHint: the checkout was removed; nothing was installed"
        ));
    }

    eprintln!("Installed {} to {}", repo.key, dir.display());
    eprintln!(
        "Hint: `clone --tap {}/<key>` now resolves through it",
        repo.key
    );
    Ok(())
}

/// 先注销再删目录：中途失败时留下的是「目录还在、账本没它」，
/// 比反过来（账本有它、目录没了）更好收拾。
fn remove(spec: &str, layout: &PathLayout) -> Result<(), String> {
    let repo = parse_repo(spec)?;
    let index_path = layout.tap_index_path();
    let (mut object, warning) = store::read_object(&index_path);
    print_warning(warning);

    if object.remove(&repo.key).is_none() {
        return Err(format!(
            "Tap repository is not installed: {:?}\nInstalled: {}",
            repo.key,
            installed_keys(&object)
        ));
    }
    store::write_object(&index_path, &object)?;

    let dir = repo_dir(&repo.user, &repo.repo, layout);
    match fs::remove_dir_all(&dir) {
        Ok(()) => eprintln!("Removed {}", dir.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("Removed {} (its directory was already gone)", repo.key);
        }
        Err(e) => {
            return Err(format!(
                "Failed to delete {}: {}\nHint: the entry is gone; delete the directory manually",
                dir.display(),
                e
            ));
        }
    }

    // 空的 user 目录留着只会让 `ls tap/` 变脏；删不掉（非空、被占用）不算错
    let _ = fs::remove_dir(layout.tap_dir().join(&repo.user));
    Ok(())
}

fn list(layout: &PathLayout) -> Result<(), String> {
    let (object, warning) = store::read_object(&layout.tap_index_path());
    print_warning(warning);

    let text = render_list(&index::entries(&object), layout);
    write_stdout(text.as_bytes())
}

fn render_list(entries: &[index::Entry], layout: &PathLayout) -> String {
    let mut output = String::from("\n");
    output.push_str(&format!(
        "{} {}\n",
        "Tap:".green().bold(),
        layout.tap_dir().display()
    ));
    output.push_str(&format!(
        "{} {}\n\n",
        "Index:".green().bold(),
        layout.tap_index_path().display()
    ));

    if entries.is_empty() {
        output.push_str(&format!(
            "  No repositories installed\n  {}\n",
            "Hint: install one with `byk @@@tap add aaa/bb`".dimmed()
        ));
        return output;
    }

    let key_width = column_width(entries.iter().map(|entry| entry.key.as_str()), "repo");
    let origin_width = column_width(entries.iter().map(|entry| entry.origin.as_str()), "origin");
    let ref_width = column_width(entries.iter().map(|entry| entry.reference.as_str()), "ref");

    output.push_str(
        &format!(
            "  {}  {}  {}  {}\n",
            pad("repo", key_width),
            pad("origin", origin_width),
            pad("ref", ref_width),
            "state"
        )
        .dimmed()
        .to_string(),
    );

    for entry in entries {
        let state = match repo_dir_for_key(&entry.key, layout) {
            Some(dir) if dir.is_dir() => "ok".green(),
            _ => "missing".yellow(),
        };
        output.push_str(&format!(
            "  {}  {}  {}  {}\n",
            pad(&entry.key, key_width).cyan().bold(),
            pad(&entry.origin, origin_width),
            pad(&entry.reference, ref_width),
            state
        ));
    }
    output
}

fn render_help(layout: &PathLayout) -> String {
    let commands: Vec<(String, &'static str)> = VERBS
        .iter()
        .map(|(name, description)| ((*name).to_string(), *description))
        .collect();

    let mut output = String::from("\n");
    output.push_str(&format!(
        "{} {}\n",
        "Tap:".green().bold(),
        layout.tap_dir().display()
    ));
    output.push_str(&format!(
        "{} {}\n",
        "Index:".green().bold(),
        layout.tap_index_path().display()
    ));
    output.push_str(&format!("{} {}\n\n", "Usage:".green().bold(), USAGE));
    output.push_str(&render_section(
        "Options:",
        &[("--help, -h".to_string(), "Show this help")],
    ));
    output.push('\n');
    output.push_str(&render_section("Commands:", &commands));
    output.push('\n');
    output
}

fn render_section(title: &str, entries: &[(String, &'static str)]) -> String {
    let width = entries
        .iter()
        .map(|(name, _)| get_display_width(name))
        .max()
        .unwrap_or(0);

    let mut output = format!("{}\n", title.green().bold());
    for (name, description) in entries {
        output.push_str(&format!(
            "  {}{}  {}\n",
            name.cyan().bold(),
            " ".repeat(width - get_display_width(name)),
            description
        ));
    }
    output
}

/// 仓库目录：`<tap>/<user>/<repo>`。
fn repo_dir(user: &str, repo: &str, layout: &PathLayout) -> PathBuf {
    layout.tap_dir().join(user).join(repo)
}

/// 从索引的键还原目录；键不合法就没有目录可谈。
fn repo_dir_for_key(key: &str, layout: &PathLayout) -> Option<PathBuf> {
    let (user, repo) = key.split_once('/')?;
    if !valid_component(user) || !valid_component(repo) {
        return None;
    }
    Some(repo_dir(user, repo, layout))
}

fn column_width<'a>(values: impl Iterator<Item = &'a str>, header: &str) -> usize {
    values
        .map(get_display_width)
        .max()
        .unwrap_or(0)
        .max(get_display_width(header))
}

fn pad(text: &str, width: usize) -> String {
    format!(
        "{}{}",
        text,
        " ".repeat(width.saturating_sub(get_display_width(text)))
    )
}

fn installed_keys(object: &Map<String, Value>) -> String {
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    if keys.is_empty() {
        return "(none)".to_string();
    }
    keys.sort_unstable();
    keys.join(", ")
}

fn unknown_command(command: &str) -> String {
    format!(
        "Unknown command: {:?}\nAvailable: {}",
        command,
        VERBS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn single_argument<'a>(args: &'a [String], message: &str) -> Result<&'a str, String> {
    expect_count(args, 1, message)?;
    Ok(&args[0])
}

fn expect_count(args: &[String], expected: usize, message: &str) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

fn print_warning(warning: Option<String>) {
    if let Some(warning) = warning {
        eprintln!("{}", warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// 装好一个「已经下载完」的仓库：目录 + 索引，绕开网络。
    fn fake_install(layout: &PathLayout, key: &str) {
        let (user, repo) = key.split_once('/').unwrap();
        let dir = repo_dir(user, repo, layout);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("byk.tap.json"), r#"{"build": "b.byk.json"}"#).unwrap();

        let mut object = Map::new();
        object.insert(
            key.to_string(),
            json!({ "origin": ORIGIN, "ref": DEFAULT_REF }),
        );
        store::write_object(&layout.tap_index_path(), &object).unwrap();
    }

    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c == '\x1b' {
                chars.next();
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&inner) = chars.peek() {
                        chars.next();
                        if inner.is_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(c);
                chars.next();
            }
        }
        result
    }

    #[test]
    fn parses_user_and_repo_with_an_optional_ref() {
        let plain = parse_repo("aaa/bb").unwrap();
        assert_eq!(plain.key, "aaa/bb");
        assert_eq!(plain.user, "aaa");
        assert_eq!(plain.repo, "bb");
        assert_eq!(plain.reference, "HEAD");

        let pinned = parse_repo("aaa/bb@v1").unwrap();
        assert_eq!(pinned.key, "aaa/bb", "key 永远不带 ref");
        assert_eq!(pinned.reference, "v1");

        assert_eq!(
            parse_repo("aaa/bb@feature/x").unwrap().reference,
            "feature/x"
        );
    }

    #[test]
    fn rejects_shapes_that_cannot_be_paths() {
        for bad in [
            "",
            "aaa",
            "aaa/",
            "/bb",
            "aaa/bb/cc",
            "aaa/./bb",
            "./bb",
            "../bb",
            "aaa/..",
            "aaa/bb@",
            "aaa/bb@@v1",
            "a b/bb",
        ] {
            assert!(parse_repo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn add_refuses_to_overwrite_an_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fake_install(&layout, "aaa/bb");

        let err = add("aaa/bb", &layout).unwrap_err();
        assert!(err.starts_with("Tap repository already exists:"), "{err}");
        assert!(err.contains("byk @@@tap remove aaa/bb"), "{err}");
    }

    #[test]
    fn remove_deletes_the_directory_and_the_entry() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fake_install(&layout, "aaa/bb");
        let dir = repo_dir("aaa", "bb", &layout);

        remove("aaa/bb", &layout).unwrap();

        assert!(!dir.exists());
        assert!(
            !layout.tap_dir().join("aaa").exists(),
            "空的 user 目录也该清掉"
        );
        let (object, warning) = store::read_object(&layout.tap_index_path());
        assert!(warning.is_none());
        assert!(object.is_empty());
    }

    #[test]
    fn remove_only_touches_what_the_index_knows() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        // 手工 git clone 进来的仓库：账本里没有它
        let manual = repo_dir("ccc", "dd", &layout);
        fs::create_dir_all(&manual).unwrap();

        let err = remove("ccc/dd", &layout).unwrap_err();

        assert!(
            err.starts_with("Tap repository is not installed: \"ccc/dd\""),
            "{err}"
        );
        assert!(err.contains("Installed: (none)"), "{err}");
        assert!(manual.is_dir(), "不在账本里的目录不能被删");
    }

    #[test]
    fn remove_tolerates_a_directory_that_is_already_gone() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fake_install(&layout, "aaa/bb");
        fs::remove_dir_all(repo_dir("aaa", "bb", &layout)).unwrap();

        remove("aaa/bb", &layout).unwrap();

        let (object, _) = store::read_object(&layout.tap_index_path());
        assert!(object.is_empty());
    }

    #[test]
    fn record_writes_origin_and_ref() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let dir = repo_dir("aaa", "bb", &layout);
        fs::create_dir_all(&dir).unwrap();

        record(&parse_repo("aaa/bb@v1").unwrap(), &layout, Map::new()).unwrap();

        let (object, warning) = store::read_object(&layout.tap_index_path());
        assert!(warning.is_none());
        assert_eq!(object["aaa/bb"]["origin"], "github");
        assert_eq!(object["aaa/bb"]["ref"], "v1");
    }

    #[test]
    fn record_rolls_back_the_checkout_when_the_index_cannot_be_written() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let dir = repo_dir("aaa", "bb", &layout);
        fs::create_dir_all(&dir).unwrap();
        // 索引位置被目录占着，写入必定失败
        fs::create_dir_all(layout.tap_index_path()).unwrap();

        let err = record(&parse_repo("aaa/bb").unwrap(), &layout, Map::new()).unwrap_err();

        assert!(err.contains("the checkout was removed"), "{err}");
        assert!(!dir.exists(), "记账失败不能留下一个没人认领的目录");
    }

    #[test]
    fn list_marks_missing_directories() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fake_install(&layout, "aaa/bb");
        fs::remove_dir_all(repo_dir("aaa", "bb", &layout)).unwrap();
        let (object, _) = store::read_object(&layout.tap_index_path());

        let text = strip_ansi(&render_list(&index::entries(&object), &layout));

        assert!(text.contains("aaa/bb"), "{text}");
        assert!(text.contains("missing"), "{text}");
    }

    #[test]
    fn list_says_how_to_install_when_empty() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);

        let text = strip_ansi(&render_list(&[], &layout));

        assert!(text.contains("No repositories installed"), "{text}");
        assert!(text.contains("byk @@@tap add aaa/bb"), "{text}");
    }

    #[test]
    fn list_sorts_entries_by_key() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fake_install(&layout, "ccc/dd");
        let mut object = Map::new();
        object.insert(
            "aaa/bb".to_string(),
            json!({ "origin": ORIGIN, "ref": DEFAULT_REF }),
        );
        object.insert(
            "ccc/dd".to_string(),
            json!({ "origin": ORIGIN, "ref": DEFAULT_REF }),
        );

        let entries = index::entries(&object);
        let keys: Vec<&str> = entries.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys, ["aaa/bb", "ccc/dd"]);

        let text = strip_ansi(&render_list(&entries, &layout));
        assert!(text.find("aaa/bb").unwrap() < text.find("ccc/dd").unwrap());
    }

    #[test]
    fn help_lists_every_command() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);

        let text = strip_ansi(&render_help(&layout));
        assert!(text.contains("Usage: byk @@@tap [OPTIONS] [COMMAND] [ARGS]..."));
        assert!(text.contains("  list    Show every installed repository"));
        assert!(text.contains("  add     Download a repository"));
        assert!(text.contains("  remove  Delete an installed repository"));
    }

    #[test]
    fn rejects_bad_arity_before_touching_anything() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);

        assert_eq!(
            run(&args(&["list", "extra"]), &layout).unwrap_err(),
            "list takes no arguments"
        );
        assert_eq!(
            run(&args(&["add"]), &layout).unwrap_err(),
            "add takes one repository, e.g. aaa/bb"
        );
        assert_eq!(
            run(&args(&["remove"]), &layout).unwrap_err(),
            "remove takes one repository, e.g. aaa/bb"
        );
        assert!(!layout.tap_dir().exists(), "参数不对不该建目录");
    }

    #[test]
    fn unknown_command_lists_verbs() {
        assert_eq!(
            unknown_command("delete"),
            "Unknown command: \"delete\"\nAvailable: list, add, remove"
        );
    }
}
