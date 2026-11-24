//! `@@@tap`：把 tap 仓库装进 `~/.byk/tap/`，并记账到 `tap/index.json`。
//!
//! 目标只有一个：让仓库通过 CLI 落到 `clone --tap` 已经在用的那个目录里，
//! 不必再手敲 `git clone`。所以 `index.json` 只被这里读写——`clone --tap`
//! 仍然扫目录，一行都不用改，索引丢了也不影响 `--tap` 本身。
//!
//! 四条动词各管一件事：`list` 读账本、`add` 克隆并登记、`update` 拉到上游最新、
//! `remove` 注销并删除。`remove` 只认账本里的键，所以手工放进 `tap/` 的仓库不会被误删；
//! `update` 同样只动账本里的仓库，且只动它自己 clone 出来的那份。

mod fetch;
mod index;

use std::fs;
use std::io;
use std::path::PathBuf;

use colored::Colorize;
use serde_json::{Map, Value};

use super::store;

use crate::model::PathLayout;
use crate::utils::display::get_display_width;
use crate::utils::io::write_stdout;

/// 已注册的动词；顺序即帮助与错误提示顺序。
pub(crate) const VERBS: &[(&str, &str)] = &[
    ("list", "Show every installed repository"),
    ("add", "Install a repository, optionally from any git URL"),
    ("update", "Pull the latest commits from upstream"),
    ("remove", "Delete an installed repository"),
];

const USAGE: &str = "byk @@@tap [OPTIONS] [COMMAND] [ARGS]...";

/// 没写来源地址时的默认源，也是 `github.com` 在账本里的写法。
const ORIGIN: &str = "github";

/// 默认 ref，与 `clone --tap` 一致。
const DEFAULT_REF: &str = "HEAD";

/// 没有 host 的来源（`file://`、本地路径）在账本里的记法。
const LOCAL_ORIGIN: &str = "local";

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
        "add" => add(rest, layout),
        "update" => update(rest, layout),
        "remove" => {
            let spec = single_argument(rest, "remove takes one repository, e.g. aaa/bb")?;
            remove(spec, layout)
        }
        other => Err(unknown_command(other)),
    }
}

/// 一次安装的仓库标识：`<user>/<repo>[@<ref>] [<git-url>]`。
///
/// `key` 是索引的键，也是 `tap/<user>/<repo>` 的相对位置，永远不带 `@<ref>`。
/// 给了 `<git-url>` 时它才是真正的来源；`user/repo` 只是本地名字，与那个地址没有
/// 从属关系——所以不校验两者是否对应，跟 `brew tap user/repo URL` 一样。
#[derive(Debug)]
struct Repo {
    key: String,
    user: String,
    repo: String,
    reference: String,
    url: Option<String>,
}

fn parse_repo(spec: &str, url: Option<&str>) -> Result<Repo, String> {
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
    if let Some(url) = url
        && !valid_source(url)
    {
        return Err(invalid_source(url));
    }

    Ok(Repo {
        key: format!("{user}/{repo}"),
        user: user.to_string(),
        repo: repo.to_string(),
        reference: reference.to_string(),
        url: url.map(str::to_string),
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

/// ref 只进 URL 或 `git fetch` 的位置参数，允许 `feature/x` 这种带斜杠的分支名，
/// 但不许控制字符与空白，也不许以 `-` 开头——那会被 git 当成选项。
fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.starts_with('-')
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

/// 自定义来源只要求「不像一个选项」：其余原样交给 git，任何 git 能克隆的地址都收。
///
/// 不设协议白名单——`brew tap user/repo URL` 也是这么做的，`ext::` 那类会让 URL
/// 变成命令的形态 git 自己就拒绝（`transport 'ext' not allowed`）。
fn valid_source(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn invalid_source(value: &str) -> String {
    format!(
        "Invalid source: {:?}\nHint: pass a git URL or path without spaces, and it cannot start with `-`",
        value
    )
}

/// 索引里记的来源：默认是 GitHub，自定义源取 host，没有 host 的（`file://`、本地路径）记 `local`。
fn origin_of(url: Option<&str>) -> String {
    let Some(url) = url else {
        return ORIGIN.to_string();
    };

    match host_of(url) {
        host if host.eq_ignore_ascii_case("github.com") => ORIGIN.to_string(),
        host => host,
    }
}

fn host_of(url: &str) -> String {
    let rest = if url.starts_with("file://") {
        return LOCAL_ORIGIN.to_string();
    } else if let Some((_, rest)) = url.split_once("://") {
        rest
    } else if let Some((_, rest)) = url.split_once('@') {
        // scp 风格：`git@host:path`
        rest
    } else {
        // 本地路径
        return LOCAL_ORIGIN.to_string();
    };

    let authority = rest.split(['/', '\\']).next().unwrap_or(rest);
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let host = host.split_once(':').map(|(host, _)| host).unwrap_or(host);

    if host.is_empty() {
        LOCAL_ORIGIN.to_string()
    } else {
        host.to_ascii_lowercase()
    }
}

/// 安装一份仓库，然后记账。
///
/// 顺序不能反：先有目录再写索引，写失败就删掉目录。反过来会留下一条指向
/// 空处的记录，而 `list` 之外没人能发现它。
fn add(args: &[String], layout: &PathLayout) -> Result<(), String> {
    let (spec, url) = match args {
        [spec] => (spec.as_str(), None),
        [spec, url] => (spec.as_str(), Some(url.as_str())),
        _ => {
            return Err(
                "add takes one repository and an optional git URL\nHint: e.g. `byk @@@tap add aaa/bb` or `byk @@@tap add aaa/bb https://git.example.com/aaa/bb.git`"
                    .to_string(),
            );
        }
    };

    let repo = parse_repo(spec, url)?;
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

    match &repo.url {
        Some(url) => eprintln!("Cloning {}@{} from {}", repo.key, repo.reference, url),
        None => eprintln!("Cloning {}@{}", repo.key, repo.reference),
    }
    fetch::install(&repo, layout)?;

    record(&repo, layout, object)
}

/// 记账。写不进去就把刚装好的目录删掉，回到「什么都没发生」。
fn record(repo: &Repo, layout: &PathLayout, mut object: Map<String, Value>) -> Result<(), String> {
    let dir = repo_dir(&repo.user, &repo.repo, layout);

    let mut entry = Map::new();
    entry.insert(
        "origin".to_string(),
        Value::String(origin_of(repo.url.as_deref())),
    );
    entry.insert("ref".to_string(), Value::String(repo.reference.clone()));
    if let Some(url) = &repo.url {
        // 默认源能推导出来，只有自定义地址值得落进账本
        entry.insert("url".to_string(), Value::String(url.clone()));
    }
    object.insert(repo.key.clone(), Value::Object(entry));

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

/// 拉上游：不给参数就更新账本里的每一份，给一份就只更新它。
///
/// 账本里的 `ref` 才是真相——`update aaa/bb` 不会因为命令行没写 `@v1` 就跑回默认分支。
fn update(args: &[String], layout: &PathLayout) -> Result<(), String> {
    match args {
        [] => update_all(layout),
        [spec] => {
            let key = parse_repo(spec, None)?.key;
            let (object, warning) = store::read_object(&layout.tap_index_path());
            print_warning(warning);

            let value = object
                .get(&key)
                .ok_or_else(|| not_installed(&key, &object))?;
            let repo = installed_repo(&key, value)?;
            let change = fetch::update(&repo, &repo_dir(&repo.user, &repo.repo, layout))?;
            print_change(&repo, &change);
            Ok(())
        }
        _ => Err(
            "update takes one repository, or none to update every repository\nHint: e.g. `byk @@@tap update` or `byk @@@tap update aaa/bb`"
                .to_string(),
        ),
    }
}

/// 整批更新：一份失败不拦下其余，逐条说完再把失败的键合成一句退出码 1。
fn update_all(layout: &PathLayout) -> Result<(), String> {
    let (object, warning) = store::read_object(&layout.tap_index_path());
    print_warning(warning);

    if object.is_empty() {
        eprintln!("No repositories installed");
        eprintln!("Hint: install one with `byk @@@tap add aaa/bb`");
        return Ok(());
    }

    let keys = sorted_keys(&object);
    let mut failed: Vec<&str> = Vec::new();

    for key in &keys {
        // 条目读不懂也算一份失败：跳过它等于悄悄少更新了一份
        let outcome = installed_repo(key, &object[*key]).and_then(|repo| {
            fetch::update(&repo, &repo_dir(&repo.user, &repo.repo, layout))
                .inspect(|change| print_change(&repo, change))
        });

        if let Err(message) = outcome {
            eprintln!("{}", message);
            failed.push(key);
        }
    }

    if failed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Failed to update {} of {} tap repositories: {}",
        failed.len(),
        keys.len(),
        failed.join(", ")
    ))
}

/// 用一个账本条目还原出更新需要的身份：`ref` 与 `url` 都以账本为准。
///
/// 键里不带 `@`，所以 [`parse_repo`] 给出的 ref 一定是默认值，随后按账本覆盖。
fn installed_repo(key: &str, value: &Value) -> Result<Repo, String> {
    let field = |name: &str| value.get(name).and_then(Value::as_str);

    let reference = field("ref").unwrap_or(DEFAULT_REF);
    if !valid_reference(reference) {
        return Err(format!(
            "Invalid ref in the tap index: {:?}\nHint: `byk @@@tap remove {}` and add it again",
            reference, key
        ));
    }

    let mut repo = parse_repo(key, field("url"))?;
    repo.reference = reference.to_string();
    Ok(repo)
}

fn print_change(repo: &Repo, change: &fetch::Change) {
    if change.is_new() {
        eprintln!(
            "Updated {} ({} → {})",
            repo.key,
            change.from.dimmed(),
            change.to.dimmed()
        );
    } else {
        eprintln!("{} is already up to date", repo.key);
    }
}

/// 先注销再删目录：中途失败时留下的是「目录还在、账本没它」，
/// 比反过来（账本有它、目录没了）更好收拾。
fn remove(spec: &str, layout: &PathLayout) -> Result<(), String> {
    let repo = parse_repo(spec, None)?;
    let index_path = layout.tap_index_path();
    let (mut object, warning) = store::read_object(&index_path);
    print_warning(warning);

    if object.remove(&repo.key).is_none() {
        return Err(not_installed(&repo.key, &object));
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

fn not_installed(key: &str, object: &Map<String, Value>) -> String {
    format!(
        "Tap repository is not installed: {:?}\nInstalled: {}",
        key,
        installed_keys(object)
    )
}

fn installed_keys(object: &Map<String, Value>) -> String {
    if object.is_empty() {
        return "(none)".to_string();
    }
    sorted_keys(object).join(", ")
}

/// 账本的键排序：`list` 与批量更新都要一个稳定的输出顺序。
fn sorted_keys(object: &Map<String, Value>) -> Vec<&str> {
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys
}

/// 补全用：账本里的仓库键（`user/repo`）。
///
/// 键不合法就没有目录可言，补出来也只会让 `remove` 报错，所以先过一遍
/// `repo_dir_for_key`。账本读不出来就当没装：补全不该为一个坏文件报错。
pub(crate) fn installed_repo_keys(layout: &PathLayout) -> Vec<String> {
    let (object, _) = store::read_object(&layout.tap_index_path());
    sorted_keys(&object)
        .into_iter()
        .filter(|key| repo_dir_for_key(key, layout).is_some())
        .map(str::to_string)
        .collect()
}

/// 补全说明：本文件的动词。
pub(crate) fn describe(candidate: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|(name, _)| *name == candidate)
        .map(|(_, description)| *description)
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

    use serde_json::json;

    fn layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// 装好一个「已经克隆完」的仓库：目录 + 索引，绕开网络。
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
        let plain = parse_repo("aaa/bb", None).unwrap();
        assert_eq!(plain.key, "aaa/bb");
        assert_eq!(plain.user, "aaa");
        assert_eq!(plain.repo, "bb");
        assert_eq!(plain.reference, "HEAD");

        let pinned = parse_repo("aaa/bb@v1", None).unwrap();
        assert_eq!(pinned.key, "aaa/bb", "key 永远不带 ref");
        assert_eq!(pinned.reference, "v1");

        assert_eq!(
            parse_repo("aaa/bb@feature/x", None).unwrap().reference,
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
            "aaa/bb@-oops",
            "a b/bb",
        ] {
            assert!(parse_repo(bad, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn add_refuses_to_overwrite_an_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        fake_install(&layout, "aaa/bb");

        let err = add(&args(&["aaa/bb"]), &layout).unwrap_err();
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

        record(&parse_repo("aaa/bb@v1", None).unwrap(), &layout, Map::new()).unwrap();

        let (object, warning) = store::read_object(&layout.tap_index_path());
        assert!(warning.is_none());
        assert_eq!(object["aaa/bb"]["origin"], "github");
        assert_eq!(object["aaa/bb"]["ref"], "v1");
        assert!(
            object["aaa/bb"].get("url").is_none(),
            "默认源能推导出地址，不写进账本"
        );
    }

    #[test]
    fn record_keeps_a_custom_source() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let dir = repo_dir("aaa", "bb", &layout);
        fs::create_dir_all(&dir).unwrap();

        let repo = parse_repo("aaa/bb", Some("ssh://git@git.example.com/aaa/bb.git")).unwrap();
        record(&repo, &layout, Map::new()).unwrap();

        let (object, _) = store::read_object(&layout.tap_index_path());
        assert_eq!(object["aaa/bb"]["origin"], "git.example.com");
        assert_eq!(
            object["aaa/bb"]["url"],
            "ssh://git@git.example.com/aaa/bb.git"
        );
    }

    #[test]
    fn derives_a_display_origin_from_the_source() {
        assert_eq!(origin_of(None), "github");
        assert_eq!(origin_of(Some("https://github.com/aaa/bb.git")), "github");
        assert_eq!(
            origin_of(Some("https://gitlab.com/aaa/bb.git")),
            "gitlab.com"
        );
        assert_eq!(origin_of(Some("git@gitlab.com:aaa/bb.git")), "gitlab.com");
        assert_eq!(origin_of(Some("https://host:8443/x")), "host");
        assert_eq!(origin_of(Some("file:///srv/taps/bb")), "local");
        assert_eq!(origin_of(Some("/Users/coke/src/tap")), "local");
        assert_eq!(origin_of(Some("./relative/tap")), "local");
    }

    #[test]
    fn accepts_any_source_git_can_clone() {
        for url in [
            "https://gitlab.com/a/b.git",
            "ssh://git@host/a/b.git",
            "git@host:a/b.git",
            "file:///srv/taps/b",
            "/Users/coke/src/tap",
            "./relative",
        ] {
            assert!(parse_repo("aaa/bb", Some(url)).is_ok(), "{url} 应该可以");
        }

        for bad in ["", "-oops", "--upload-pack=sh", "a b", "a\tb"] {
            let err = parse_repo("aaa/bb", Some(bad)).unwrap_err();
            assert!(err.starts_with("Invalid source:"), "{bad}: {err}");
        }
    }

    #[test]
    fn reads_the_identity_of_an_installed_repository_from_the_index() {
        let plain = installed_repo("aaa/bb", &json!({ "origin": "github", "ref": "v1" })).unwrap();
        assert_eq!(plain.key, "aaa/bb");
        assert_eq!(plain.reference, "v1", "更新跟着账本里的 ref 走");
        assert!(plain.url.is_none());

        let custom = installed_repo(
            "my/tools",
            &json!({
                "origin": "git.example.com",
                "ref": "main",
                "url": "ssh://git@git.example.com/my/tools.git"
            }),
        )
        .unwrap();
        assert_eq!(custom.reference, "main");
        assert_eq!(
            custom.url.as_deref(),
            Some("ssh://git@git.example.com/my/tools.git")
        );
    }

    #[test]
    fn falls_back_to_the_default_ref_when_the_index_says_nothing() {
        for entry in [json!({}), json!({ "ref": 1 }), json!({ "ref": null })] {
            assert_eq!(
                installed_repo("aaa/bb", &entry).unwrap().reference,
                DEFAULT_REF
            );
        }
    }

    #[test]
    fn rejects_an_index_ref_that_git_would_read_as_an_option() {
        let err = installed_repo("aaa/bb", &json!({ "ref": "-oops" })).unwrap_err();

        assert!(err.starts_with("Invalid ref in the tap index:"), "{err}");
        assert!(err.contains("byk @@@tap remove aaa/bb"), "{err}");
    }

    #[test]
    fn update_refuses_a_repository_the_index_does_not_know() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        // 手工 git clone 进来的仓库：账本里没有它
        fs::create_dir_all(repo_dir("aaa", "bb", &layout)).unwrap();

        let err = update(&args(&["aaa/bb"]), &layout).unwrap_err();

        assert!(err.starts_with("Tap repository is not installed"), "{err}");
        assert!(err.contains("Installed: (none)"), "{err}");
    }

    #[test]
    fn update_says_so_when_nothing_is_installed() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);

        assert!(update(&args(&[]), &layout).is_ok(), "空账本不算失败");
    }

    #[test]
    fn record_rolls_back_the_checkout_when_the_index_cannot_be_written() {
        let temp = tempfile::tempdir().unwrap();
        let layout = layout(&temp);
        let dir = repo_dir("aaa", "bb", &layout);
        fs::create_dir_all(&dir).unwrap();
        // 索引位置被目录占着，写入必定失败
        fs::create_dir_all(layout.tap_index_path()).unwrap();

        let err = record(&parse_repo("aaa/bb", None).unwrap(), &layout, Map::new()).unwrap_err();

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
        assert!(text.contains("  add     Install a repository"), "{text}");
        assert!(
            text.contains("  update  Pull the latest commits from upstream"),
            "{text}"
        );
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
            "add takes one repository and an optional git URL\nHint: e.g. `byk @@@tap add aaa/bb` or `byk @@@tap add aaa/bb https://git.example.com/aaa/bb.git`"
        );
        assert_eq!(
            run(&args(&["update", "aaa/bb", "ccc/dd"]), &layout).unwrap_err(),
            "update takes one repository, or none to update every repository\nHint: e.g. `byk @@@tap update` or `byk @@@tap update aaa/bb`"
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
            "Unknown command: \"delete\"\nAvailable: list, add, update, remove"
        );
    }
}
