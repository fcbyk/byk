//! 文件命令的补全候选：动词、选项、来源文件与已有 key。

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::model::PathLayout;
use crate::resolve::validate_filename;

use crate::filecmd::io::path::resolve_target;
use crate::filecmd::source::Cdn;
use crate::filecmd::spec::FileScope;
use crate::filecmd::spec::subcommand_names;
use crate::filecmd::spec::{HELP_FLAGS, OPTION_ALIASES, OPTIONS, VERBS, Verb, split_head};

/// 补全 `@file <verb> ...`。
///
/// `prev` 是光标前的完整词，`partial` 是当前词。返回 `None` 表示第一个词
/// 不是 `@file` / `@@file`，调用方继续按别名补全。
pub(crate) fn complete(prev: &[String], partial: &str, layout: &PathLayout) -> Option<Vec<String>> {
    let (prefix, stem) = split_head(prev.first()?)?;
    if !validate_filename(stem) {
        return Some(Vec::new());
    }
    let scope = if prefix == "@@" {
        FileScope::Global
    } else {
        FileScope::Local
    };

    // 刚敲完文件 token：补动词；`-` 开头则补帮助开关
    if prev.len() == 1 {
        if partial.starts_with('-') {
            return Some(help_flags(partial));
        }
        return Some(verb_candidates(partial));
    }

    let verb = match Verb::parse(&prev[1]) {
        Some(verb) => verb,
        None => return Some(Vec::new()),
    };
    let args = &prev[2..];
    let arg_index = args.len();

    match verb {
        Verb::Help => Some(Vec::new()),
        Verb::Deps => {
            if arg_index == 0 {
                return Some(subcommand_candidates(partial));
            }
            options(verb, partial)
        }
        Verb::Remove => {
            if arg_index == 0 && !partial.starts_with('-') {
                return Some(existing_keys(scope, stem, partial, layout));
            }
            options(verb, partial)
        }
        Verb::Set => {
            if arg_index == 0 && !partial.starts_with('-') {
                return Some(existing_keys(scope, stem, partial, layout));
            }
            options(verb, partial)
        }
        Verb::Clone | Verb::Link | Verb::Init => {
            if let Some(sources) = source_args(verb, args, partial, layout) {
                return Some(sources);
            }
            if let Some(cdns) = clone_cdn(verb, args, partial) {
                return Some(cdns);
            }
            options(verb, partial)
        }
        _ => options(verb, partial),
    }
}

fn verb_candidates(partial: &str) -> Vec<String> {
    VERBS
        .iter()
        .map(|info| info.name)
        .filter(|name| name.starts_with(partial))
        .map(str::to_string)
        .collect()
}

/// `-h` / `--help` 是每个文件命令都有的帮助开关。
fn help_flags(partial: &str) -> Vec<String> {
    HELP_FLAGS
        .iter()
        .filter(|flag| flag.starts_with(partial))
        .map(|flag| (*flag).to_string())
        .collect()
}

fn subcommand_candidates(partial: &str) -> Vec<String> {
    subcommand_names()
        .filter(|name| name.starts_with(partial))
        .map(str::to_string)
        .collect()
}

/// 补全文件命令后面的选项；非选项位置没有候选。
fn options(verb: Verb, partial: &str) -> Option<Vec<String>> {
    if !partial.starts_with('-') {
        return Some(Vec::new());
    }

    let mut candidates = help_flags(partial);
    for info in OPTIONS.iter().filter(|info| info.verb == verb) {
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

/// 补全 clone/link 的来源参数，以及 `init --link` 的值。
///
/// `@` 开头给别名文件候选；其他形态返回空，让 shell 回退到路径补全。
fn source_args(
    verb: Verb,
    args: &[String],
    partial: &str,
    layout: &PathLayout,
) -> Option<Vec<String>> {
    match verb {
        Verb::Clone | Verb::Link => {
            if !args.is_empty() {
                // 源已给：clone 还能补 `-d`/`--deps`，交给 options；
                // link 无选项，落下去也只会得到空列表。
                return None;
            }
        }
        Verb::Init => {
            if !(args.len() == 1 && args[0] == "--link") {
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

/// 补全 `-gh <spec> ` 之后的 CDN 名。
///
/// 只有一个值的位置：紧跟在 `-gh` / `--github` 的 spec 后面。
fn clone_cdn(verb: Verb, args: &[String], partial: &str) -> Option<Vec<String>> {
    if verb != Verb::Clone || partial.starts_with('-') || args.len() < 2 {
        return None;
    }
    let flag = &args[args.len() - 2];
    if flag != "-gh" && flag != "--github" {
        return None;
    }

    Some(
        Cdn::NAMES
            .iter()
            .filter(|name| name.starts_with(partial))
            .map(|name| (*name).to_string())
            .collect(),
    )
}

fn existing_keys(scope: FileScope, stem: &str, partial: &str, layout: &PathLayout) -> Vec<String> {
    let Ok(path) = resolve_target(scope, stem, layout) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };

    let mut keys = Vec::new();
    collect_alias_keys(&object, "", &mut keys);
    keys.into_iter()
        .filter(|key| key.starts_with(partial))
        .collect()
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

    fn fixture() -> (tempfile::TempDir, PathLayout) {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };
        fs::create_dir_all(&layout.alias_dir).unwrap();
        (temp, layout)
    }

    #[test]
    fn complete_verbs_at_second_word() {
        let (_temp, layout) = fixture();
        assert_eq!(
            complete(&words(&["@@ops"]), "", &layout),
            Some(vec![
                "init".to_string(),
                "destroy".to_string(),
                "clear".to_string(),
                "set".to_string(),
                "remove".to_string(),
                "json".to_string(),
                "path".to_string(),
                "rename".to_string(),
                "clone".to_string(),
                "link".to_string(),
                "deps".to_string()
            ])
        );
        assert_eq!(
            complete(&words(&["@@ops"]), "i", &layout),
            Some(vec!["init".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops"]), "d", &layout),
            Some(vec!["destroy".to_string(), "deps".to_string()])
        );
        assert_eq!(
            complete(&words(&["@"]), "i", &layout),
            Some(vec!["init".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops"]), "--", &layout),
            Some(vec!["--help".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops"]), "-", &layout),
            Some(vec!["--help".to_string(), "-h".to_string()])
        );
    }

    #[test]
    fn complete_ignores_precise_alias_and_flags() {
        let (_temp, layout) = fixture();
        assert_eq!(complete(&words(&["@@ops.init"]), "", &layout), None);
        assert_eq!(complete(&words(&["ops"]), "i", &layout), None);
        assert_eq!(
            complete(&words(&["@@ops", "explode"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_options_per_verb() {
        let (_temp, layout) = fixture();
        let opts = |verb: &str, partial: &str| complete(&words(&["@@ops", verb]), partial, &layout);

        assert_eq!(
            opts("init", "--"),
            Some(vec!["--help".to_string(), "--link".to_string()])
        );
        assert_eq!(opts("init", "--l"), Some(vec!["--link".to_string()]));
        assert_eq!(
            opts("json", "--"),
            Some(vec![
                "--help".to_string(),
                "--expand".to_string(),
                "--no-check".to_string()
            ])
        );
        assert_eq!(opts("json", "--e"), Some(vec!["--expand".to_string()]));
        assert_eq!(opts("json", "--n"), Some(vec!["--no-check".to_string()]));
        assert_eq!(
            opts("destroy", "--"),
            Some(vec!["--help".to_string(), "--yes".to_string()])
        );
        assert_eq!(
            opts("clear", "--"),
            Some(vec!["--help".to_string(), "--yes".to_string()])
        );
        assert_eq!(
            opts("set", "--"),
            Some(vec![
                "--help".to_string(),
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
            opts("set", "--c"),
            Some(vec!["--cmd".to_string(), "--cwd".to_string()])
        );
        assert_eq!(opts("link", "--"), Some(vec!["--help".to_string()]));
        assert_eq!(opts("remove", "--"), Some(vec!["--help".to_string()]));
        assert_eq!(
            opts("clone", "-"),
            Some(vec![
                "--help".to_string(),
                "-h".to_string(),
                "-gh".to_string(),
                "--github".to_string(),
                "-ge".to_string(),
                "--gitee".to_string(),
                "-t".to_string(),
                "--tap".to_string(),
                "-d".to_string(),
                "--deps".to_string()
            ])
        );
        assert_eq!(
            opts("clone", "--"),
            Some(vec![
                "--help".to_string(),
                "--github".to_string(),
                "--gitee".to_string(),
                "--tap".to_string(),
                "--deps".to_string()
            ])
        );
        assert_eq!(
            opts("clone", "-g"),
            Some(vec!["-gh".to_string(), "-ge".to_string()])
        );
        assert_eq!(opts("clone", "-t"), Some(vec!["-t".to_string()]));
        assert_eq!(opts("json", ""), Some(Vec::new()));
        assert_eq!(opts("json", "x"), Some(Vec::new()));
    }

    #[test]
    fn complete_source_args_offers_alias_files() {
        let (_temp, layout) = fixture();
        fs::write(layout.alias_dir.join("ops.byk.json"), "{}").unwrap();
        fs::write(layout.alias_dir.join("src.byk.json"), "{}").unwrap();
        fs::write(layout.alias_dir.join("bad.name.byk.json"), "{}").unwrap();

        assert_eq!(
            complete(&words(&["@@ops", "clone"]), "@@", &layout),
            Some(vec!["@@ops".to_string(), "@@src".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops", "link"]), "@@s", &layout),
            Some(vec!["@@src".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops", "init", "--link"]), "@@", &layout),
            Some(vec!["@@ops".to_string(), "@@src".to_string()])
        );
    }

    #[test]
    fn complete_source_args_falls_back_to_paths() {
        let (_temp, layout) = fixture();

        assert_eq!(
            complete(&words(&["@@ops", "clone"]), "./x", &layout),
            Some(Vec::new())
        );
        assert_eq!(
            complete(&words(&["@@ops", "clone", "src"]), "", &layout),
            Some(Vec::new())
        );
        assert_eq!(
            complete(&words(&["@@ops", "init"]), "--", &layout),
            Some(vec!["--help".to_string(), "--link".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops", "set"]), "@", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_clone_cdn_after_github_spec() {
        let (_temp, layout) = fixture();
        assert_eq!(
            complete(&words(&["@@ops", "clone", "-gh", "aaa/bb"]), "", &layout),
            Some(vec![
                "raw".to_string(),
                "jsdelivr".to_string(),
                "ghproxy".to_string()
            ])
        );
        assert_eq!(
            complete(
                &words(&["@@ops", "clone", "--github", "aaa/bb"]),
                "js",
                &layout
            ),
            Some(vec!["jsdelivr".to_string()])
        );
        // 只对 `-gh` 生效；Gitee 或值已给完时交给其他补全
        assert_eq!(
            complete(&words(&["@@ops", "clone", "-ge", "aaa/bb"]), "", &layout),
            Some(Vec::new())
        );
        assert_eq!(
            complete(
                &words(&["@@ops", "clone", "-gh", "aaa/bb", "raw"]),
                "",
                &layout
            ),
            Some(Vec::new())
        );
        assert_eq!(
            complete(&words(&["@@ops", "link", "-gh", "a/b"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_remove_args_lists_keys_and_groups() {
        let (_temp, layout) = fixture();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":"echo hi","web":{"$cwd":".","deploy":"echo d"},"$ignored":"x"}"#,
        )
        .unwrap();

        assert_eq!(
            complete(&words(&["@@ops", "remove"]), "", &layout),
            Some(vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ])
        );
        assert_eq!(
            complete(&words(&["@@ops", "remove"]), "web.", &layout),
            Some(vec!["web.deploy".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops", "remove", "build"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_remove_args_skips_link_files() {
        let (_temp, layout) = fixture();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"["shared.byk.json"]"#,
        )
        .unwrap();

        assert_eq!(
            complete(&words(&["@@ops", "remove"]), "", &layout),
            Some(Vec::new())
        );
    }

    #[test]
    fn complete_set_args_lists_existing_keys() {
        let (_temp, layout) = fixture();
        fs::write(
            layout.alias_dir.join("ops.byk.json"),
            r#"{"build":"echo hi","web":{"$cwd":".","deploy":"echo d"},"$ignored":"x"}"#,
        )
        .unwrap();

        assert_eq!(
            complete(&words(&["@@ops", "set"]), "", &layout),
            Some(vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ])
        );
        assert_eq!(
            complete(&words(&["@@ops", "set"]), "web.", &layout),
            Some(vec!["web.deploy".to_string()])
        );
        assert_eq!(
            complete(&words(&["@@ops", "set"]), "--", &layout),
            Some(vec![
                "--help".to_string(),
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
    }

    #[test]
    fn complete_invalid_stem_is_empty() {
        let (_temp, layout) = fixture();
        assert_eq!(complete(&words(&["@@a/b"]), "", &layout), Some(Vec::new()));
    }
}
