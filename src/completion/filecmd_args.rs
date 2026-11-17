//! 文件命令的补全候选：动词、选项、来源文件与已有 key。

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::model::PathLayout;
use crate::resolve::validate_filename;

use crate::filecmd::io::path::resolve_target;
use crate::filecmd::spec::ParseResult;
use crate::filecmd::spec::SUBCOMMANDS;
use crate::filecmd::spec::{OPTION_ALIASES, OPTIONS, VERBS, Verb, parse, split_prefix};

/// 补全 `@prefix..verb`；不要求目标文件存在。
///
/// 返回 `None` 表示不是 `..` 形式，调用方继续按别名补全。
pub(crate) fn complete_verbs(partial: &str) -> Option<Vec<String>> {
    let (prefix, rest) = split_prefix(partial)?;
    let sep = rest.find("..")?;
    let stem = &rest[..sep];

    if !validate_filename(stem) {
        return Some(Vec::new());
    }

    let verb_partial = &rest[sep + 2..];
    Some(
        VERBS
            .iter()
            .filter(|info| info.name.starts_with(verb_partial))
            .map(|info| format!("{}{}..{}", prefix, stem, info.name))
            .collect(),
    )
}

/// 补全文件命令后面的选项。
///
/// `None` 表示第一个词不是文件命令，交给原有逻辑；`Some(空)` 表示是文件命令
/// 但当前位置没有候选。
pub(crate) fn complete_options(command_token: &str, partial: &str) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        ParseResult::Error(_) => return Some(Vec::new()),
        ParseResult::NotFileCommand => return None,
    };

    if !partial.starts_with('-') {
        return Some(Vec::new());
    }

    let mut candidates = Vec::new();
    for info in OPTIONS.iter().filter(|info| info.verb == command.verb) {
        for (alias, flag) in OPTION_ALIASES {
            if *flag == info.flag && alias.starts_with(partial) {
                candidates.push((*alias).to_string());
            }
        }
        if info.flag.starts_with(partial) {
            candidates.push(info.flag.to_string());
        }
    }
    Some(candidates)
}

/// 补全 `..help` 后面的命令名。
///
/// `arg_index` 是当前词之前的词数；只有第一个参数位置有候选。
/// 返回 `None` 表示第一个词不是 `..help`。
pub(crate) fn complete_help_args(
    command_token: &str,
    arg_index: usize,
    partial: &str,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };
    if command.verb != Verb::Help {
        return None;
    }
    if arg_index != 1 {
        return Some(Vec::new());
    }

    Some(
        VERBS
            .iter()
            .filter(|info| info.verb != Verb::Help)
            .map(|info| info.name)
            .filter(|name| name.starts_with(partial))
            .map(str::to_string)
            .collect(),
    )
}

/// 补全 clone/link 的来源参数，以及 `init --link` 的值。
///
/// `@` 开头给别名文件候选；其他形态返回空，让 shell 回退到路径补全。
pub(crate) fn complete_source_args(
    command_token: &str,
    prev: &[String],
    partial: &str,
    layout: &PathLayout,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };

    match command.verb {
        Verb::Clone | Verb::Link => {
            if prev.len() != 1 {
                // 源已给：clone 还能补 `-d`/`--deps`，交给 complete_options；
                // link 无选项，落下去也只会得到空列表。
                return None;
            }
        }
        Verb::Init => {
            if !(prev.len() == 2 && prev[1] == "--link") {
                return None;
            }
        }
        _ => return None,
    }

    if partial.starts_with('-') {
        return None;
    }
    if !partial.starts_with('@') {
        return Some(Vec::new());
    }
    Some(source_candidates(partial, layout))
}

fn source_candidates(partial: &str, layout: &PathLayout) -> Vec<String> {
    let mut candidates = Vec::new();

    if partial.starts_with("@@") {
        candidates.extend(alias_file_tokens(&layout.alias_dir, "@@"));
    } else if partial.starts_with('@') {
        if let Ok(cwd) = std::env::current_dir() {
            candidates.extend(alias_file_tokens(&cwd, "@"));
        }
        candidates.extend(alias_file_tokens(&layout.alias_dir, "@@"));
    }

    candidates.retain(|candidate| candidate.starts_with(partial));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn alias_file_tokens(dir: &Path, prefix: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let stem = name.strip_suffix(".byk.json")?;
            if stem.is_empty() || !validate_filename(stem) {
                return None;
            }
            Some(format!("{}{}", prefix, stem))
        })
        .collect()
}

/// 补全 `..deps` 后面的子命令。
///
/// 只有第一个参数位置有子命令候选；之后的参数返回 `None`，
/// 交给 `complete_options` 去补选项（`--force` / `--yes`）。
pub(crate) fn complete_deps_args(
    command_token: &str,
    arg_index: usize,
    partial: &str,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };
    if command.verb != Verb::Deps {
        return None;
    }
    if arg_index != 1 {
        return None;
    }

    Some(
        SUBCOMMANDS
            .iter()
            .filter(|name| name.starts_with(partial))
            .map(|name| (*name).to_string())
            .collect(),
    )
}

/// 补全 `..remove` 后面的已有 key 路径。
///
/// 只有第一个参数位置有候选；链接文件、缺失或非法文件不补。
pub(crate) fn complete_remove_args(
    command_token: &str,
    arg_index: usize,
    partial: &str,
    layout: &PathLayout,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };
    if command.verb != Verb::Remove {
        return None;
    }
    if arg_index != 1 {
        return Some(Vec::new());
    }
    if partial.starts_with('-') {
        return None;
    }

    let Ok(path) = resolve_target(command.scope, &command.stem, layout) else {
        return Some(Vec::new());
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Some(Vec::new());
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&text) else {
        return Some(Vec::new());
    };

    let mut keys = Vec::new();
    collect_alias_keys(&object, "", &mut keys);
    Some(
        keys.into_iter()
            .filter(|key| key.starts_with(partial))
            .collect(),
    )
}

/// 补全 `..set` 后面的已有 key 路径。
///
/// 与 `..remove` 不同，`..set` 后面还能接选项，所以非 key 位置或 `-` 开头
/// 时返回 `None`，交给 `complete_options`；只有第一个参数位置才给 key 候选。
pub(crate) fn complete_set_args(
    command_token: &str,
    arg_index: usize,
    partial: &str,
    layout: &PathLayout,
) -> Option<Vec<String>> {
    let command = match parse(command_token) {
        ParseResult::Command(command) => command,
        _ => return None,
    };
    if command.verb != Verb::Set {
        return None;
    }
    if arg_index != 1 {
        return None;
    }
    if partial.starts_with('-') {
        return None;
    }

    let Ok(path) = resolve_target(command.scope, &command.stem, layout) else {
        return Some(Vec::new());
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Some(Vec::new());
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&text) else {
        return Some(Vec::new());
    };

    let mut keys = Vec::new();
    collect_alias_keys(&object, "", &mut keys);
    Some(
        keys.into_iter()
            .filter(|key| key.starts_with(partial))
            .collect(),
    )
}

fn collect_alias_keys(object: &Map<String, Value>, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in object {
        if key.starts_with('$') {
            continue;
        }
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        match value {
            Value::Object(inner) => {
                out.push(path.clone());
                collect_alias_keys(inner, &path, out);
            }
            Value::String(_) | Value::Bool(_) => out.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn complete_after_two_dots() {
        assert_eq!(
            complete_verbs("@@ops.."),
            Some(vec![
                "@@ops..init".to_string(),
                "@@ops..destroy".to_string(),
                "@@ops..clear".to_string(),
                "@@ops..set".to_string(),
                "@@ops..remove".to_string(),
                "@@ops..json".to_string(),
                "@@ops..path".to_string(),
                "@@ops..rename".to_string(),
                "@@ops..clone".to_string(),
                "@@ops..link".to_string(),
                "@@ops..deps".to_string(),
                "@@ops..help".to_string()
            ])
        );
        assert_eq!(
            complete_verbs("@@ops..i"),
            Some(vec!["@@ops..init".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..d"),
            Some(vec![
                "@@ops..destroy".to_string(),
                "@@ops..deps".to_string()
            ])
        );
        assert_eq!(
            complete_verbs("@@ops..c"),
            Some(vec!["@@ops..clear".to_string(), "@@ops..clone".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..s"),
            Some(vec!["@@ops..set".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..j"),
            Some(vec!["@@ops..json".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..p"),
            Some(vec!["@@ops..path".to_string()])
        );
        assert_eq!(
            complete_verbs("@@ops..r"),
            Some(vec![
                "@@ops..remove".to_string(),
                "@@ops..rename".to_string()
            ])
        );
        assert_eq!(
            complete_verbs("@@ops..l"),
            Some(vec!["@@ops..link".to_string()])
        );
        assert_eq!(complete_verbs("@..i"), Some(vec!["@..init".to_string()]));
    }

    #[test]
    fn complete_ignores_single_dot() {
        assert_eq!(complete_verbs("@@ops.init"), None);
        assert_eq!(complete_verbs("@@ops."), None);
        assert_eq!(complete_verbs("ops"), None);
    }

    #[test]
    fn complete_options_per_verb() {
        assert_eq!(
            complete_options("@@ops..init", "--"),
            Some(vec!["--link".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..init", "--l"),
            Some(vec!["--link".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..json", "--"),
            Some(vec!["--expand".to_string(), "--no-check".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..json", "--e"),
            Some(vec!["--expand".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..json", "--n"),
            Some(vec!["--no-check".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..destroy", "--"),
            Some(vec!["--yes".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..clear", "--"),
            Some(vec!["--yes".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..set", "--"),
            Some(vec![
                "--cmd".to_string(),
                "--cwd".to_string(),
                "--mode".to_string(),
                "--description".to_string(),
                "--recursive".to_string(),
                "--no-recursive".to_string(),
                "--private".to_string(),
                "--no-private".to_string(),
                "--unset".to_string(),
                "--priority".to_string(),
                "--paths".to_string()
            ])
        );
        assert_eq!(
            complete_options("@@ops..set", "--c"),
            Some(vec!["--cmd".to_string(), "--cwd".to_string()])
        );
        assert_eq!(complete_options("@@ops..link", "--"), Some(Vec::new()));
        assert_eq!(complete_options("@@ops..remove", "--"), Some(Vec::new()));
        assert_eq!(
            complete_options("@@ops..clone", "-"),
            Some(vec![
                "-gh".to_string(),
                "--github".to_string(),
                "-ge".to_string(),
                "--gitee".to_string(),
                "-jd".to_string(),
                "--jsdelivr".to_string(),
                "-d".to_string(),
                "--deps".to_string()
            ])
        );
        assert_eq!(
            complete_options("@@ops..clone", "--"),
            Some(vec![
                "--github".to_string(),
                "--gitee".to_string(),
                "--jsdelivr".to_string(),
                "--deps".to_string()
            ])
        );
        assert_eq!(
            complete_options("@@ops..clone", "-g"),
            Some(vec!["-gh".to_string(), "-ge".to_string()])
        );
        assert_eq!(
            complete_options("@@ops..clone", "-j"),
            Some(vec!["-jd".to_string()])
        );
        assert_eq!(complete_options("@@ops..json", ""), Some(Vec::new()));
        assert_eq!(complete_options("@@ops..json", "x"), Some(Vec::new()));
        assert_eq!(complete_options("normal", "--"), None);
    }

    #[test]
    fn complete_help_args_lists_verbs_without_help() {
        assert_eq!(
            complete_help_args("@@ops..help", 1, ""),
            Some(
                [
                    "init", "destroy", "clear", "set", "remove", "json", "path", "rename", "clone",
                    "link", "deps"
                ]
                .iter()
                .map(|name| name.to_string())
                .collect()
            )
        );
        assert_eq!(
            complete_help_args("@@ops..help", 1, "s"),
            Some(vec!["set".to_string()])
        );
        assert_eq!(complete_help_args("@@ops..help", 2, ""), Some(Vec::new()));
        assert_eq!(complete_help_args("@@ops..set", 1, ""), None);
        assert_eq!(complete_help_args("bogus", 1, ""), None);
    }

    #[test]
    fn complete_source_args_offers_alias_files() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        fs::write(layout.alias_dir.join("ops.byk.json"), "{}").unwrap();
        fs::write(layout.alias_dir.join("src.byk.json"), "{}").unwrap();
        fs::write(layout.alias_dir.join("bad.name.byk.json"), "{}").unwrap();

        assert_eq!(
            complete_source_args("@@ops..clone", &words(&["@@ops..clone"]), "@@", &layout),
            Some(vec!["@@ops".to_string(), "@@src".to_string()])
        );
        assert_eq!(
            complete_source_args("@@ops..link", &words(&["@@ops..link"]), "@@s", &layout),
            Some(vec!["@@src".to_string()])
        );
        assert_eq!(
            complete_source_args(
                "@@ops..init",
                &words(&["@@ops..init", "--link"]),
                "@@",
                &layout
            ),
            Some(vec!["@@ops".to_string(), "@@src".to_string()])
        );
    }

    #[test]
    fn complete_source_args_falls_back_to_paths() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();

        assert_eq!(
            complete_source_args("@@ops..clone", &words(&["@@ops..clone"]), "./x", &layout),
            Some(Vec::new())
        );
        assert_eq!(
            complete_source_args(
                "@@ops..clone",
                &words(&["@@ops..clone", "src"]),
                "",
                &layout
            ),
            None
        );
        assert_eq!(
            complete_source_args("@@ops..init", &words(&["@@ops..init"]), "--", &layout),
            None
        );
        assert_eq!(
            complete_source_args("@@ops..set", &words(&["@@ops..set"]), "@", &layout),
            None
        );
    }

    #[test]
    fn complete_remove_args_lists_keys_and_groups() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":"echo hi","web":{"$cwd":".","deploy":"echo d"},"$ignored":"x"}"#,
        )
        .unwrap();

        assert_eq!(
            complete_remove_args("@@ops..remove", 1, "", &layout),
            Some(vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ])
        );
        assert_eq!(
            complete_remove_args("@@ops..remove", 1, "web.", &layout),
            Some(vec!["web.deploy".to_string()])
        );
        assert_eq!(
            complete_remove_args("@@ops..remove", 2, "", &layout),
            Some(Vec::new())
        );
        assert_eq!(complete_remove_args("@@ops..set", 1, "", &layout), None);
        assert_eq!(complete_remove_args("bogus", 1, "", &layout), None);
    }

    #[test]
    fn complete_remove_args_skips_link_files() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"["shared.byk.json"]"#,
        )
        .unwrap();

        assert_eq!(
            complete_remove_args("@@ops..remove", 1, "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_set_args_lists_existing_keys() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":"echo hi","web":{"$cwd":".","deploy":"echo d"},"$ignored":"x"}"#,
        )
        .unwrap();

        assert_eq!(
            complete_set_args("@@ops..set", 1, "", &layout),
            Some(vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ])
        );
        assert_eq!(
            complete_set_args("@@ops..set", 1, "web.", &layout),
            Some(vec!["web.deploy".to_string()])
        );
        assert_eq!(complete_set_args("@@ops..set", 1, "--", &layout), None);
        assert_eq!(complete_set_args("@@ops..set", 2, "", &layout), None);
        assert_eq!(complete_set_args("@@ops..remove", 1, "", &layout), None);
        assert_eq!(complete_set_args("bogus", 1, "", &layout), None);
    }

    #[test]
    fn complete_invalid_stem_is_empty() {
        assert_eq!(complete_verbs("@@a/b.."), Some(Vec::new()));
    }
}
