//! Shell 补全：生成 zsh / bash / fish 的注册脚本，并回答 `complete` 查询。

mod at_prefix;
mod filecmd_args;
mod sys_args;

use at_prefix::{complete_at_prefix, complete_nested_alias};

#[cfg(test)]
use at_prefix::collect_object_keys;

use std::collections::HashMap;

use crate::filecmd::spec as file_spec;
use crate::filecmd::sys as sys_files;
use crate::model::PathLayout;
use crate::resolve;
use crate::utils::path::display_path;

use self::filecmd_args as file_complete;
use self::sys_args as system_complete;

pub fn generate(shell: &str) {
    match shell {
        "zsh" => print_zsh(),
        "bash" => print_bash(),
        "fish" => print_fish(),
        other => {
            eprintln!("Unsupported shell: {}", other);
            eprintln!("  Supported: zsh, bash, fish");
            std::process::exit(1);
        }
    }
}

/// 输出补全候选，一行一个。
///
/// `with_descriptions` 为 true 时，内置选项、文件命令动词和文件 key 在候选后
/// 追加 tab 分隔的描述；文件 key 的描述是别名文件路径。
pub fn complete(args: &[String], layout: &PathLayout, with_descriptions: bool) {
    let prefer_file_option = args.len() >= 2
        && args
            .first()
            .is_some_and(|token| file_spec::is_file_head(token))
        && args.last().is_some_and(|partial| partial.starts_with('-'));

    let candidates = candidates(args, layout);
    let descriptions = if with_descriptions {
        dynamic_descriptions(args, &candidates, layout)
    } else {
        HashMap::new()
    };

    for candidate in &candidates {
        println!(
            "{}",
            format_candidate(
                candidate,
                with_descriptions,
                prefer_file_option,
                descriptions.get(candidate).map(String::as_str),
            )
        );
    }
}

/// 动态描述：文件 key 给别名文件路径，系统文件的候选给各自的说明。
///
/// 系统文件的说明只能按「问的是哪个文件」来查：`config` 与 `tap` 都有 `list`，
/// 前者是设置表、后者是仓库清单。
fn dynamic_descriptions(
    args: &[String],
    candidates: &[String],
    layout: &PathLayout,
) -> HashMap<String, String> {
    let mut descriptions = file_key_descriptions(candidates, layout);

    if let Some(name) = args.first().and_then(|token| sys_files::parse_token(token)) {
        for candidate in candidates {
            if let Some(description) = sys_files::describe(name, candidate) {
                descriptions.insert(candidate.clone(), description.to_string());
            }
        }
    }

    descriptions
}

/// 文件 key → 展示用别名文件路径。
///
/// 文件 key 不含点号；带点号的是精确别名路径。
/// 候选已经来自解析成功的文件，描述只需要文件名枚举，不必再解析一遍 JSON。
fn file_key_descriptions(candidates: &[String], layout: &PathLayout) -> HashMap<String, String> {
    let mentions_file_key = candidates
        .iter()
        .any(|candidate| candidate.starts_with('@') && !candidate.contains('.'));

    if !mentions_file_key {
        return HashMap::new();
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    resolve::list_alias_files(&cwd, false)
        .into_iter()
        .chain(resolve::list_alias_files(&layout.alias_dir, true))
        .map(|(key, path)| (key, display_path(&path)))
        .collect()
}

/// 内置选项的静态描述；动态候选不在这里。
fn describe_in(candidate: &str, prefer_file_option: bool) -> Option<&'static str> {
    if prefer_file_option && let Some(description) = file_spec::describe_option(candidate) {
        return Some(description);
    }

    match candidate {
        "--help" | "-h" => Some("Print help"),
        "--version" | "-v" => Some("Print version"),
        "--paths" => Some("Print CLI paths and file locations"),
        "--check" => Some("Check alias files for problems"),
        _ => file_spec::describe_option(candidate)
            .or_else(|| file_spec::describe_verb(candidate))
            .or_else(|| file_spec::describe_subcommand(candidate))
            .or_else(|| sys_files::describe_file(candidate)),
    }
}

fn format_candidate(
    candidate: &str,
    with_descriptions: bool,
    prefer_file_option: bool,
    dynamic_description: Option<&str>,
) -> String {
    if !with_descriptions {
        return candidate.to_string();
    }

    match dynamic_description.or_else(|| describe_in(candidate, prefer_file_option)) {
        Some(description) => format!("{}\t{}", candidate, description),
        None => candidate.to_string(),
    }
}

/// 第一个词是别名时只在第一词位置补；文件命令按 `@file <verb>` 的位置补。
fn candidates(args: &[String], layout: &PathLayout) -> Vec<String> {
    let Some((partial, prev)) = args.split_last() else {
        // 无参数：焦点在第一个词
        return get_completions("", layout);
    };

    if prev.is_empty() {
        return get_completions(partial, layout);
    }

    // `@@@file` 的语法由各文件自己定，先交给系统文件的补全；它只认注册表里的名字
    system_complete::complete(prev, partial, layout)
        .or_else(|| file_complete::complete(prev, partial, layout))
        .unwrap_or_default()
}

const GLOBAL_FLAGS: &[&str] = &["--version", "-v", "--help", "-h", "--paths", "--check"];

fn get_completions(partial: &str, layout: &PathLayout) -> Vec<String> {
    if partial.starts_with('-') {
        return GLOBAL_FLAGS
            .iter()
            .filter(|f| f.starts_with(partial))
            .map(|f| f.to_string())
            .collect();
    }

    if partial.is_empty() {
        return get_top_level_completions("", layout);
    }

    if partial.starts_with('@') {
        return complete_at_prefix(partial, layout);
    }

    if let Some(dot_idx) = partial.rfind('.') {
        let prefix = &partial[..=dot_idx]; // 含末尾点号
        let key_partial = &partial[dot_idx + 1..];
        return complete_nested_alias(prefix, key_partial, layout);
    }

    get_top_level_completions(partial, layout)
}

/// 补全顶级命令：只列合并树里的顶层 key。
///
/// 文件命名空间（`@file`）属于精确执行语法，只在词以 `@` / `@@` 开头时
/// 由 `complete_at_prefix` 提供，避免和可执行别名混在同一张菜单里。
fn get_top_level_completions(partial: &str, layout: &PathLayout) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    let merged = resolve::load_merged(layout);

    for key in merged.keys() {
        candidates.push(key.clone());
    }

    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|c| c.starts_with(partial))
        .collect()
}

fn print_zsh() {
    print!(
        r##"_byk() {{
    local -a items
    local value desc
    while IFS=$'\t' read -r value desc; do
        [[ -z "$value" ]] && continue
        if [[ -n "$desc" ]]; then
            items+=("${{value//:/\\:}}:$desc")
        else
            items+=("$value")
        fi
    done < <(byk __api__ complete --with-descriptions "${{words[@]:1}}" 2>/dev/null)

    if (( ${{#items}} )); then
        _describe 'byk' items
    fi
}}

compdef _byk byk
"##
    );
}

fn print_bash() {
    print!(
        r##"_byk_completion() {{
    local i cur
    local -a words
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    # COMP_WORDS 不含光标处的空词，按 COMP_CWORD 逐下标取（越界即空），
    # 让 complete 看到的形状与 zsh 一致：光标在空格后 = 最后一个词为空
    for ((i = 1; i <= COMP_CWORD; i++)); do
        words+=("${{COMP_WORDS[i]}}")
    done
    COMPREPLY=($(compgen -W "$(byk __api__ complete "${{words[@]}}" 2>/dev/null)" -- "$cur"))
}}

complete -F _byk_completion byk
"##
    );
}

fn print_fish() {
    print!(
        r##"function _byk_completions
    set -l words (commandline -cp | string split ' ')
    set -e words[1]
    byk __api__ complete "$words" 2>/dev/null
end

complete -c byk -f -a "(_byk_completions)"
"##
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    /// 创建临时目录布局，写入待测别名文件。
    struct TestEnv {
        /// 持有临时目录，Drop 时整棵删除
        _temp: tempfile::TempDir,
        layout: PathLayout,
    }

    impl TestEnv {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let alias_dir = temp.path().join("alias");
            let root_dir = temp.path().to_path_buf();

            fs::create_dir_all(&alias_dir).unwrap();

            let layout = PathLayout {
                root_dir: root_dir.clone(),
                alias_dir,
            };

            TestEnv {
                _temp: temp,
                layout,
            }
        }

        /// 在 alias_dir 中创建一个 .byk.json 文件。
        fn add_alias_file(&self, stem: &str, content: &serde_json::Value) {
            let path = self.layout.alias_dir.join(format!("{}.byk.json", stem));
            fs::write(&path, serde_json::to_string_pretty(content).unwrap()).unwrap();
        }
    }

    #[test]
    fn collect_empty_object() {
        let obj = serde_json::Map::new();
        assert!(collect_object_keys(&obj, "").is_empty());
    }

    #[test]
    fn collect_flat_object() {
        let data = json!({"foo": "bar", "baz": 42});
        let obj = data.as_object().unwrap();
        let mut result = collect_object_keys(obj, "");
        result.sort();
        assert_eq!(result, vec!["baz", "foo"]);
    }

    #[test]
    fn collect_skips_dollar_keys() {
        let data = json!({"$cmd": "echo", "$priority": 1, "normal": "yes"});
        let obj = data.as_object().unwrap();
        let result = collect_object_keys(obj, "");
        // $ 前缀的 key 被跳过
        assert_eq!(result, vec!["normal"]);
    }

    #[test]
    fn collect_nested_object() {
        let data = json!({
            "deploy": {
                "prod": "deploy-prod.sh",
                "dev": "deploy-dev.sh"
            }
        });
        let obj = data.as_object().unwrap();
        let mut result = collect_object_keys(obj, "");
        result.sort();
        // "deploy" 是中间节点（Object），它也作为一个路径出现
        // "deploy.prod" 和 "deploy.dev" 是叶子节点
        assert_eq!(result.len(), 3);
        assert!(result.contains(&"deploy".to_string()));
        assert!(result.contains(&"deploy.prod".to_string()));
        assert!(result.contains(&"deploy.dev".to_string()));
    }

    #[test]
    fn collect_deeply_nested() {
        let data = json!({
            "a": {
                "b": {
                    "c": "leaf"
                }
            }
        });
        let obj = data.as_object().unwrap();
        let mut result = collect_object_keys(obj, "");
        result.sort();
        assert_eq!(result, vec!["a", "a.b", "a.b.c"]);
    }

    #[test]
    fn collect_with_prefix() {
        let data = json!({"inner": "value"});
        let obj = data.as_object().unwrap();
        let result = collect_object_keys(obj, "root");
        assert_eq!(result, vec!["root.inner"]);
    }

    #[test]
    fn collect_mixed_types() {
        let data = json!({
            "string_val": "hello",
            "num_val": 123,
            "bool_val": true,
            "null_val": null,
            "obj_val": {"nested": "yes"}
        });
        let obj = data.as_object().unwrap();
        let mut result = collect_object_keys(obj, "");
        result.sort();
        // obj_val 是 Object，产生 obj_val + obj_val.nested，共 6 条
        assert_eq!(result.len(), 6);
        assert!(result.contains(&"string_val".to_string()));
        assert!(result.contains(&"num_val".to_string()));
        assert!(result.contains(&"bool_val".to_string()));
        assert!(result.contains(&"null_val".to_string()));
        assert!(result.contains(&"obj_val".to_string()));
        assert!(result.contains(&"obj_val.nested".to_string()));
    }

    #[test]
    fn top_level_excludes_file_namespaces() {
        let env = TestEnv::new();
        env.add_alias_file(
            "release",
            &json!({"deploy": "echo deploy", "build": "echo build"}),
        );
        let completions = get_top_level_completions("", &env.layout);
        // 文件 key 属于精确执行语法，只在 @ 前缀下出现
        assert!(!completions.iter().any(|c| c.starts_with('@')));
        // merge 树中的顶 key 仍然列出
        assert!(completions.contains(&"deploy".to_string()));
        assert!(completions.contains(&"build".to_string()));
    }

    #[test]
    fn top_level_filter_by_prefix() {
        let env = TestEnv::new();
        env.add_alias_file("tools", &json!({"test": "echo test", "task": "echo task"}));
        let completions = get_top_level_completions("te", &env.layout);
        assert!(completions.contains(&"test".to_string()));
        assert!(!completions.contains(&"task".to_string()));
    }

    #[test]
    fn at_prefix_completes_file_keys() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy"}));
        env.add_alias_file("debug", &json!({"build": "echo build"}));
        // alias_dir 中是全局文件，key 为 @@ 前缀
        let completions = complete_at_prefix("@@r", &env.layout);
        assert_eq!(completions, vec!["@@release".to_string()]);
    }

    #[test]
    fn at_prefix_file_keys_are_sorted() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy"}));
        env.add_alias_file("debug", &json!({"build": "echo build"}));
        assert_eq!(
            complete_at_prefix("@@", &env.layout),
            vec!["@@debug".to_string(), "@@release".to_string()]
        );
    }

    #[test]
    fn at_prefix_completes_file_inner_keys() {
        let env = TestEnv::new();
        env.add_alias_file(
            "release",
            &json!({"deploy": "echo deploy", "build": "echo build"}),
        );
        let mut completions = complete_at_prefix("@@release.d", &env.layout);
        completions.sort();
        assert_eq!(completions, vec!["@@release.deploy".to_string()]);
    }

    #[test]
    fn at_prefix_unknown_file_returns_empty() {
        let env = TestEnv::new();
        let completions = complete_at_prefix("@@nope.x", &env.layout);
        assert!(completions.is_empty());
    }

    #[test]
    fn at_prefix_completes_system_files() {
        let env = TestEnv::new();
        let files = vec!["@@@config".to_string(), "@@@tap".to_string()];
        // 候选来自注册表而不是磁盘：没有 @@@config 这个文件也能补出来
        assert_eq!(complete_at_prefix("@@@", &env.layout), files);
        assert_eq!(
            complete_at_prefix("@@@c", &env.layout),
            vec!["@@@config".to_string()]
        );
        assert_eq!(
            complete_at_prefix("@@@t", &env.layout),
            vec!["@@@tap".to_string()]
        );
        assert!(complete_at_prefix("@@@nope", &env.layout).is_empty());
        // 三级前缀不能被 `@@` 的文件 key 分支吞掉
        assert_eq!(get_completions("@@@", &env.layout), files);
    }

    #[test]
    fn at_prefix_double_dot_is_not_a_file_command() {
        // 文件命令改成空格分隔后，`..` 不再保留含义，只是普通点号路径。
        let env = TestEnv::new();
        assert!(complete_at_prefix("@@anything..", &env.layout).is_empty());
        assert!(complete_at_prefix("@..i", &env.layout).is_empty());
    }

    #[test]
    fn at_prefix_single_dot_does_not_offer_file_commands() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy"}));
        let completions = complete_at_prefix("@@release.", &env.layout);
        assert_eq!(completions, vec!["@@release.deploy".to_string()]);
    }

    #[test]
    fn nested_alias_completes_children() {
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({
                "deploy": {
                    "prod": "deploy-prod.sh",
                    "dev": "deploy-dev.sh"
                }
            }),
        );
        let mut completions = complete_nested_alias("deploy.", "", &env.layout);
        completions.sort();
        assert!(completions.contains(&"deploy.prod".to_string()));
        assert!(completions.contains(&"deploy.dev".to_string()));
        // "deploy" 本身也是中间节点
        assert_eq!(completions.len(), 2);
    }

    #[test]
    fn nested_alias_filters_by_key_partial() {
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({
                "deploy": {
                    "prod": "deploy-prod.sh",
                    "dev": "deploy-dev.sh",
                    "preview": "deploy-preview.sh"
                }
            }),
        );
        let mut completions = complete_nested_alias("deploy.", "pr", &env.layout);
        completions.sort();
        assert_eq!(
            completions,
            vec!["deploy.preview".to_string(), "deploy.prod".to_string()]
        );
    }

    #[test]
    fn nested_alias_invalid_path_returns_empty() {
        let env = TestEnv::new();
        let completions = complete_nested_alias("nonexistent.", "x", &env.layout);
        assert!(completions.is_empty());
    }

    #[test]
    fn nested_alias_bare_dot_returns_empty() {
        // 回归：`byk .<TAB>` 曾把顶层 key 全加上点号前缀吐出 `.git` / `.dev`，
        // 而 `byk .git` 不是可执行别名 —— 前导点号没有路径前缀，不该有候选。
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({"git": {"tag": "git tag"}, "dev": "cargo run"}),
        );
        assert!(complete_nested_alias(".", "", &env.layout).is_empty());
        assert!(get_completions(".", &env.layout).is_empty());
        assert!(get_completions(".g", &env.layout).is_empty());
        assert!(candidates(&words(&["."]), &env.layout).is_empty());
        assert!(candidates(&words(&[".gi"]), &env.layout).is_empty());
    }

    /// 把命令行词表转成 `complete` 收到的 `args`。
    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn candidates_no_words_completes_first_word() {
        let env = TestEnv::new();
        env.add_alias_file("demo", &json!({"hello": "echo hi"}));
        let result = candidates(&words(&[]), &env.layout);
        assert!(result.contains(&"hello".to_string()));
    }

    #[test]
    fn candidates_first_word_partial() {
        let env = TestEnv::new();
        env.add_alias_file("demo", &json!({"hello": "echo hi", "hi": "echo hi"}));
        assert_eq!(candidates(&words(&["h"]), &env.layout), vec!["hello", "hi"]);
    }

    #[test]
    fn candidates_group_name_itself_is_still_offered() {
        let env = TestEnv::new();
        env.add_alias_file("default", &json!({"rust": {"build": "cargo build"}}));
        // 还没敲空格：第一个词位置照常补组名，敲完点号再补子项
        assert_eq!(candidates(&words(&["rus"]), &env.layout), vec!["rust"]);
    }

    #[test]
    fn candidates_after_group_word_returns_empty() {
        // 回归：`byk rust <TAB>` 曾吐出 build/test，而 `byk rust build` 不成立
        // ——嵌套别名只认 `rust.build` 这一个 token。
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({"rust": {"build": "cargo build", "test": "cargo test"}}),
        );
        assert!(candidates(&words(&["rust", ""]), &env.layout).is_empty());
        assert!(candidates(&words(&["rust", "b"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_after_dotted_word_returns_empty() {
        let env = TestEnv::new();
        env.add_alias_file("default", &json!({"rust": {"build": "cargo build"}}));
        assert!(candidates(&words(&["rust.build", ""]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_after_leaf_word_returns_empty() {
        let env = TestEnv::new();
        env.add_alias_file("default", &json!({"hello": "echo hello"}));
        assert!(candidates(&words(&["hello", ""]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_flag_after_word_returns_empty() {
        // `byk hello --help` 里的 --help 属于目标命令，不是 byk 的选项
        let env = TestEnv::new();
        env.add_alias_file("default", &json!({"hello": "echo hello"}));
        assert!(candidates(&words(&["hello", "--h"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_unknown_word_returns_empty() {
        let env = TestEnv::new();
        assert!(candidates(&words(&["nope", ""]), &env.layout).is_empty());
    }

    #[test]
    fn get_completions_flag_prefix() {
        let env = TestEnv::new();
        let result = get_completions("--v", &env.layout);
        assert_eq!(result, vec!["--version".to_string()]);
    }

    #[test]
    fn get_completions_empty_delegates_to_top_level() {
        let env = TestEnv::new();
        env.add_alias_file("demo", &json!({"hello": "echo hi"}));
        let result = get_completions("", &env.layout);
        // 内置命令已清空：顶级只剩别名，文件作用域不在空输入里出现
        assert!(result.contains(&"hello".to_string()));
        assert!(!result.iter().any(|c| c.starts_with('@')));
        assert!(!result.contains(&"add".to_string()));
        assert!(!result.contains(&"show".to_string()));
    }

    #[test]
    fn get_completions_includes_paths_flag() {
        let env = TestEnv::new();
        let result = get_completions("--p", &env.layout);
        assert_eq!(result, vec!["--paths".to_string()]);
        assert_eq!(
            get_completions("--c", &env.layout),
            vec!["--check".to_string()]
        );
    }

    #[test]
    fn get_completions_at_prefix_delegates() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy"}));
        let result = get_completions("@@r", &env.layout);
        assert_eq!(result, vec!["@@release".to_string()]);
    }

    #[test]
    fn get_completions_nested_dot_delegates() {
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({
                "deploy": {
                    "prod": "deploy-prod.sh",
                    "dev": "deploy-dev.sh"
                }
            }),
        );
        let completions = get_completions("deploy.pr", &env.layout);
        assert_eq!(completions, vec!["deploy.prod".to_string()]);
    }

    #[test]
    fn candidates_file_command_verbs_after_file_token() {
        let env = TestEnv::new();
        assert_eq!(
            candidates(&words(&["@@ops", "i"]), &env.layout),
            vec!["init".to_string()]
        );
        assert_eq!(
            candidates(&words(&["@@ops", ""]), &env.layout),
            vec![
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
            ]
        );
        // 精确别名不受影响
        assert!(candidates(&words(&["@@ops.init", ""]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_file_command_options() {
        let env = TestEnv::new();
        assert_eq!(
            candidates(&words(&["@@ops", "json", "--"]), &env.layout),
            vec![
                "--help".to_string(),
                "--expand".to_string(),
                "--no-check".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "json", "--n"]), &env.layout),
            vec!["--no-check".to_string()]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "destroy", "--"]), &env.layout),
            vec!["--help".to_string(), "--yes".to_string()]
        );
        assert!(candidates(&words(&["@@ops", "json", ""]), &env.layout).is_empty());
        assert!(candidates(&words(&["@@ops", "json", "--expand", ""]), &env.layout).is_empty());
        assert!(candidates(&words(&["hello", "--"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_file_command_help_flags() {
        let env = TestEnv::new();
        assert_eq!(
            candidates(&words(&["@@ops", "--"]), &env.layout),
            vec!["--help".to_string()]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "-"]), &env.layout),
            vec!["--help".to_string(), "-h".to_string()]
        );
        assert!(candidates(&words(&["@@ops", "help", ""]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_file_command_options_after_positional() {
        let env = TestEnv::new();
        assert_eq!(
            candidates(&words(&["@@ops", "set", "build", "--"]), &env.layout),
            vec![
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
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "set", "build", "--c"]), &env.layout),
            vec!["--cmd".to_string(), "--cwd".to_string()]
        );
    }

    #[test]
    fn candidates_deps_completes_options_after_subcommand() {
        let env = TestEnv::new();
        assert_eq!(
            candidates(&words(&["@@ops", "deps", ""]), &env.layout),
            vec![
                "install".to_string(),
                "uninstall".to_string(),
                "status".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "deps", "install", "--"]), &env.layout),
            vec![
                "--help".to_string(),
                "--force".to_string(),
                "--yes".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "deps", "install", "--f"]), &env.layout),
            vec!["--force".to_string()]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "deps", "uninstall", "--"]), &env.layout),
            vec![
                "--help".to_string(),
                "--force".to_string(),
                "--yes".to_string()
            ]
        );
        assert!(candidates(&words(&["@@ops", "deps", "install", "x"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_remove_completes_existing_keys() {
        let env = TestEnv::new();
        env.add_alias_file(
            "ops",
            &json!({"build": "echo hi", "web": {"deploy": "echo d"}}),
        );

        assert_eq!(
            candidates(&words(&["@@ops", "remove", ""]), &env.layout),
            vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "remove", "web."]), &env.layout),
            vec!["web.deploy".to_string()]
        );
        assert!(candidates(&words(&["@@ops", "remove", "build", ""]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_clone_completes_source_files() {
        let env = TestEnv::new();
        env.add_alias_file("src", &json!({"build": "echo hi"}));

        assert_eq!(
            candidates(&words(&["@@ops", "clone", "@@"]), &env.layout),
            vec!["@@src".to_string()]
        );
        assert!(candidates(&words(&["@@ops", "clone", "./x"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_clone_completes_options_after_source() {
        let env = TestEnv::new();
        env.add_alias_file("src", &json!({"build": "echo hi"}));

        // 源已给之后仍能补其余选项
        assert_eq!(
            candidates(&words(&["@@ops", "clone", "@@src", "-"]), &env.layout),
            vec![
                "--help".to_string(),
                "-h".to_string(),
                "-t".to_string(),
                "--tap".to_string(),
                "-d".to_string(),
                "--deps".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "clone", "@@src", "--d"]), &env.layout),
            vec!["--deps".to_string()]
        );
        // 第二个源不补
        assert!(candidates(&words(&["@@ops", "clone", "@@src", "@"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_clone_completes_source_prefixes() {
        let env = TestEnv::new();

        assert_eq!(
            candidates(&words(&["@@ops", "clone", "git"]), &env.layout),
            vec!["github:".to_string(), "gitee:".to_string()]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "clone", "gitee"]), &env.layout),
            vec!["gitee:".to_string()]
        );
        // 来源已给：不再补前缀
        assert!(candidates(&words(&["@@ops", "clone", "github:a/b", "j"]), &env.layout).is_empty());
    }

    #[test]
    fn candidates_set_completes_existing_keys() {
        let env = TestEnv::new();
        env.add_alias_file(
            "ops",
            &json!({"build": "echo hi", "web": {"deploy": "echo d"}}),
        );

        assert_eq!(
            candidates(&words(&["@@ops", "set", ""]), &env.layout),
            vec![
                "build".to_string(),
                "web".to_string(),
                "web.deploy".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@ops", "set", "web."]), &env.layout),
            vec!["web.deploy".to_string()]
        );
        // key 之后还能补选项
        assert_eq!(
            candidates(&words(&["@@ops", "set", "build", "--"]), &env.layout),
            vec![
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
            ]
        );
    }

    #[test]
    fn builtin_options_have_descriptions() {
        for flag in GLOBAL_FLAGS {
            assert!(describe_in(flag, false).is_some(), "{flag}");
        }
        for flag in ["--expand", "--no-check", "--yes"] {
            assert!(describe_in(flag, false).is_some(), "{flag}");
        }
    }

    #[test]
    fn system_file_tokens_carry_descriptions() {
        assert_eq!(describe_in("@@@config", false), Some("byk settings"));
        assert_eq!(describe_in("@@@tap", false), Some("tap repositories"));
        assert_eq!(
            format_candidate("@@@config", true, false, None),
            "@@@config\tbyk settings"
        );
    }

    #[test]
    fn candidates_system_file_verbs() {
        let env = TestEnv::new();
        assert_eq!(
            candidates(&words(&["@@@config", ""]), &env.layout),
            vec![
                "list".to_string(),
                "get".to_string(),
                "set".to_string(),
                "unset".to_string()
            ]
        );
        assert_eq!(
            candidates(&words(&["@@@tap", "u"]), &env.layout),
            vec!["update".to_string()]
        );
    }

    #[test]
    fn system_file_descriptions_follow_the_addressed_file() {
        let env = TestEnv::new();
        let config =
            dynamic_descriptions(&words(&["@@@config", ""]), &words(&["list"]), &env.layout);
        let tap = dynamic_descriptions(&words(&["@@@tap", ""]), &words(&["list"]), &env.layout);

        // 同名动词在两个文件里含义不同，描述不能串
        assert_eq!(
            config.get("list").map(String::as_str),
            Some("Show every setting")
        );
        assert_eq!(
            tap.get("list").map(String::as_str),
            Some("Show every installed repository")
        );
    }

    #[test]
    fn system_file_descriptions_cover_setting_names() {
        let env = TestEnv::new();
        let map = dynamic_descriptions(
            &words(&["@@@config", "set", ""]),
            &words(&["banner"]),
            &env.layout,
        );
        assert_eq!(
            map.get("banner").map(String::as_str),
            Some("Show the banner in help output")
        );
    }

    #[test]
    fn format_candidate_plain_and_described() {
        assert_eq!(format_candidate("--help", false, false, None), "--help");
        assert_eq!(
            format_candidate("--help", true, false, None),
            "--help\tPrint help"
        );
        assert_eq!(format_candidate("build", true, false, None), "build");
        assert_eq!(
            format_candidate("init", true, false, None),
            "init\tCreate this alias file"
        );
        assert_eq!(
            format_candidate("--paths", true, false, None),
            "--paths\tPrint CLI paths and file locations"
        );
        assert_eq!(
            format_candidate("--paths", true, true, None),
            "--paths\tPrepend directories to PATH (omit key)"
        );
        assert_eq!(
            format_candidate(
                "@@release",
                true,
                false,
                Some("~/.byk/alias/release.byk.json")
            ),
            "@@release\t~/.byk/alias/release.byk.json"
        );
        assert_eq!(
            format_candidate("@@release", false, false, Some("ignored")),
            "@@release"
        );
    }

    #[test]
    fn file_key_descriptions_use_display_path() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy"}));
        let descriptions = file_key_descriptions(&["@@release".to_string()], &env.layout);
        let path = descriptions.get("@@release").expect("path for @@release");
        assert!(path.ends_with("release.byk.json"));
        assert!(!path.contains('\\'));
    }

    #[test]
    fn file_key_descriptions_skip_unrelated_candidates() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy"}));
        assert!(file_key_descriptions(&["build".to_string()], &env.layout).is_empty());
        assert!(file_key_descriptions(&["@release.deploy".to_string()], &env.layout).is_empty());
    }

    #[test]
    fn generate_zsh_does_not_panic() {
        // print! 宏输出到 stdout，在测试中由 test runner 自动捕获。
        // 此处验证 generate 不会因不支持的 shell 等边界条件 panic。
    }

    #[test]
    fn generate_unsupported_shell_does_not_panic() {
        // generate("invalid") 向 stderr 输出错误信息，不应 panic
    }
}
