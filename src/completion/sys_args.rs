//! 系统文件（`@@@config` / `@@@tap`）的补全候选：动词、帮助开关与参数。
//!
//! 与 `filecmd_args` 平行：文件命令的语法由 `filecmd::spec` 定义，系统文件的
//! 语法归各自的模块，所以候选也向它们要，不在这里另写一份名单。

use crate::filecmd::sys;
use crate::model::PathLayout;

/// 补全 `@@@config <verb> ...` / `@@@tap <verb> ...`。
///
/// `prev` 是光标前的完整词，`partial` 是当前词。返回 `None` 表示第一个词不是
/// 已注册的系统文件，调用方继续按文件命令与别名补全。
pub(crate) fn complete(prev: &[String], partial: &str, layout: &PathLayout) -> Option<Vec<String>> {
    let name = sys::parse_token(prev.first()?)?;

    // 刚敲完文件 token：`-` 开头补帮助开关，否则补动词
    if prev.len() == 1 {
        if partial.starts_with('-') {
            return Some(help_flags(partial));
        }
        return Some(verb_candidates(name, partial));
    }

    let verb = prev[1].as_str();
    let args = &prev[2..];

    match name {
        "config" => Some(config_args(verb, args, partial)),
        "tap" => Some(tap_args(verb, args, partial, layout)),
        _ => Some(Vec::new()),
    }
}

/// `-h` / `--help` 只在动词位认（`@@@config -h`）；`@@@config set -h` 里的
/// `-h` 是 `set` 的参数，各文件的 `run` 就是这么解的第一个词。
fn help_flags(partial: &str) -> Vec<String> {
    ["--help", "-h"]
        .iter()
        .filter(|flag| flag.starts_with(partial))
        .map(|flag| (*flag).to_string())
        .collect()
}

fn verb_candidates(name: &str, partial: &str) -> Vec<String> {
    sys::verbs(name)
        .iter()
        .map(|(verb, _)| *verb)
        .filter(|verb| verb.starts_with(partial))
        .map(str::to_string)
        .collect()
}

/// `get` / `set` / `unset` 认设置名；`set` 的第二位认该设置项的候选值。
fn config_args(verb: &str, args: &[String], partial: &str) -> Vec<String> {
    if partial.starts_with('-') {
        return Vec::new();
    }

    match (verb, args.len()) {
        ("get" | "set" | "unset", 0) => sys::config::setting_names()
            .into_iter()
            .filter(|key| key.starts_with(partial))
            .map(str::to_string)
            .collect(),
        ("set", 1) => sys::config::setting_choices(&args[0])
            .iter()
            .filter(|value| value.starts_with(partial))
            .map(|value| (*value).to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// `remove` / `update` 认账本里的仓库；`add` 的来源在远端，没有候选。
fn tap_args(verb: &str, args: &[String], partial: &str, layout: &PathLayout) -> Vec<String> {
    if partial.starts_with('-') || !args.is_empty() {
        return Vec::new();
    }
    if !matches!(verb, "remove" | "update") {
        return Vec::new();
    }

    sys::tap::installed_repo_keys(layout)
        .into_iter()
        .filter(|key| key.starts_with(partial))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn fixture() -> (tempfile::TempDir, PathLayout) {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        (temp, layout)
    }

    /// 账本里塞两条仓库，其中一条键不合法（不该被补出来）。
    fn install(layout: &PathLayout, keys: &[&str]) {
        fs::create_dir_all(layout.tap_dir()).unwrap();
        let object: serde_json::Map<String, serde_json::Value> = keys
            .iter()
            .map(|key| {
                (
                    (*key).to_string(),
                    serde_json::json!({ "origin": "github", "ref": "HEAD" }),
                )
            })
            .collect();
        fs::write(
            layout.tap_index_path(),
            serde_json::to_string(&object).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn lists_verbs_after_the_file_token() {
        let (_temp, layout) = fixture();

        assert_eq!(
            complete(&words(&["@@@config"]), "", &layout),
            Some(vec![
                "list".to_string(),
                "get".to_string(),
                "set".to_string(),
                "unset".to_string()
            ])
        );
        assert_eq!(
            complete(&words(&["@@@tap"]), "", &layout),
            Some(vec![
                "list".to_string(),
                "add".to_string(),
                "update".to_string(),
                "remove".to_string()
            ])
        );
        assert_eq!(
            complete(&words(&["@@@tap"]), "u", &layout),
            Some(vec!["update".to_string()])
        );
    }

    #[test]
    fn offers_help_flags_only_in_the_verb_position() {
        let (_temp, layout) = fixture();

        assert_eq!(
            complete(&words(&["@@@config"]), "-", &layout),
            Some(vec!["--help".to_string(), "-h".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@@tap"]), "--", &layout),
            Some(vec!["--help".to_string()])
        );
        // 动词之后 `-h` 是参数，不是开关
        assert_eq!(
            complete(&words(&["@@@tap", "list"]), "-", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn completes_setting_names() {
        let (_temp, layout) = fixture();

        assert_eq!(
            complete(&words(&["@@@config", "get"]), "", &layout),
            Some(vec!["banner".to_string(), "net-github".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@@config", "unset"]), "net", &layout),
            Some(vec!["net-github".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@@config", "set"]), "b", &layout),
            Some(vec!["banner".to_string()])
        );
    }

    #[test]
    fn completes_setting_values_after_a_name() {
        let (_temp, layout) = fixture();

        assert_eq!(
            complete(&words(&["@@@config", "set", "banner"]), "", &layout),
            Some(vec![
                "auto".to_string(),
                "always".to_string(),
                "never".to_string()
            ])
        );
        assert_eq!(
            complete(&words(&["@@@config", "set", "banner"]), "al", &layout),
            Some(vec!["always".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@@config", "set", "net-github"]), "g", &layout),
            Some(vec!["ghproxy".to_string()])
        );
        // 未知设置项没有候选
        assert_eq!(
            complete(&words(&["@@@config", "set", "baner"]), "", &layout),
            Some(Vec::new())
        );
        // 值已给：不再补
        assert_eq!(
            complete(&words(&["@@@config", "set", "banner", "auto"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn list_takes_no_candidates() {
        let (_temp, layout) = fixture();
        assert_eq!(
            complete(&words(&["@@@config", "list"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn completes_installed_repositories() {
        let (_temp, layout) = fixture();
        install(&layout, &["aaa/bb", "ccc/dd", "bad"]);

        assert_eq!(
            complete(&words(&["@@@tap", "remove"]), "", &layout),
            Some(vec!["aaa/bb".to_string(), "ccc/dd".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@@tap", "update"]), "aa", &layout),
            Some(vec!["aaa/bb".to_string()])
        );
        // 键已给：不再补
        assert_eq!(
            complete(&words(&["@@@tap", "remove", "aaa/bb"]), "", &layout),
            Some(Vec::new())
        );
        // `add` 的来源在远端
        assert_eq!(
            complete(&words(&["@@@tap", "add"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn missing_index_offers_nothing() {
        let (_temp, layout) = fixture();
        assert_eq!(
            complete(&words(&["@@@tap", "remove"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn ignores_tokens_it_does_not_own() {
        let (_temp, layout) = fixture();
        assert_eq!(complete(&words(&["@@@nope"]), "", &layout), None);
        assert_eq!(complete(&words(&["@@"]), "", &layout), None);
        assert_eq!(complete(&words(&["ops"]), "", &layout), None);
        assert_eq!(complete(&words(&["@@@"]), "", &layout), None);
    }
}
