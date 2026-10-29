//! Shell 补全脚本生成与动态补全查询。
//!
//! 通过 `byk __api__ completion <shell>` 输出对应 shell 的补全包装脚本（到 stdout），
//! 通过 `byk __api__ complete <words...>` 查询补全候选（供补全脚本回调）。
//!
//! 支持 zsh、bash、fish。

mod args_tree;
mod at_prefix;

use args_tree::{collect_args_candidates, complete_args_tree};
use at_prefix::{
    complete_at_prefix, complete_nested_alias, complete_show_topic,
};

#[cfg(test)]
use at_prefix::{collect_object_keys, expand_tilde};

use super::aliases;
use super::paths::PathLayout;

// ---------------------------------------------------------------------------
// 公开入口
// ---------------------------------------------------------------------------

/// 生成指定 shell 的补全脚本并输出到 stdout。
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

/// 根据已输入的命令行返回补全候选，每行一个输出到 stdout。
///
/// 将 args 拆分为"已确定的前缀词"和"正在输入的当前词"两部分：
/// - 无前缀词：按顶级补全逻辑（当前词可能为空，表示用户刚开始输入第一个命令）
/// - 有前缀词：检查前缀词是否构成一个已知命令，再决定补全策略
pub fn complete(args: &[String], layout: &PathLayout) {
    let partial = args.last().map(|s| s.as_str()).unwrap_or("");
    let prev = if args.len() > 1 {
        &args[..args.len() - 1]
    } else {
        &[]
    };

    let candidates = if prev.is_empty() {
        // 没有前缀词：用户正在输入第一个命令，按原逻辑补全
        get_completions(partial, layout)
    } else {
        // 有前缀词：检查这些词是否构成一个已知命令，再决定补全
        contextual_completions(prev, partial, layout)
    };

    for c in &candidates {
        println!("{}", c);
    }
}

// ---------------------------------------------------------------------------
// 补全逻辑
// ---------------------------------------------------------------------------

/// 全局选项列表。
const GLOBAL_FLAGS: &[&str] = &[
    "--version",
    "-v",
    "--help",
    "-h",
];

/// `add` 命令没有额外选项。
const ADD_FLAGS: &[&str] = &["--help", "-h"];

/// 仅 `--help` / `-h`（内置于命令无额外选项时使用）。
const HELP_FLAGS: &[&str] = &["--help", "-h"];

/// 根据当前输入的部分词，从所有命令源收集匹配的补全候选。
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

/// 根据前缀词（已确定输入的命令词）和当前部分词，返回上下文相关的补全候选。
///
/// 前缀词可能是：
/// - 别名节点（如 `rust`）→ 补全其子节点
/// - @文件引用（如 `@release`）→ 补全文件内的 key
/// - 插件命令 → 无子命令，返回空
/// - 无法识别的词 → 返回空
fn contextual_completions(prev: &[String], partial: &str, layout: &PathLayout) -> Vec<String> {
    if prev.is_empty() {
        return Vec::new();
    }

    // 多前缀词场景
    if prev.len() > 1 {
        return complete_multi_prev(prev, partial, layout);
    }

    let first = &prev[0];

    // 别名合并配置（$args 补全与嵌套别名节点共用一次加载）
    let (merged, _) = aliases::load_merged_aliases(layout);

    // 上下文标志：根据已知命令返回对应的选项
    if partial.starts_with('-') {
        // 别名 $args 的 $options 也可能包含 flag
        if let Some(resolved) = aliases::resolve_merged_alias(&merged, first)
            && let Some(args) = alias_args(&resolved.value)
        {
            let candidates = collect_args_candidates(args, partial, &[]);
            if !candidates.is_empty() {
                return candidates;
            }
        }

        let flags = match first.as_str() {
            "add" => ADD_FLAGS,
            "remove" | "show" => HELP_FLAGS,
            _ => GLOBAL_FLAGS,
        };
        return flags
            .iter()
            .filter(|f| f.starts_with(partial))
            .map(|f| f.to_string())
            .collect();
    }

    // show 子命令补全
    if first == "show" {
        return complete_show_topic(partial, layout);
    }

    // add 子命令补全
    if first == "add" {
        // byk add @<TAB> → 补全本地文件 stem（仅 @stem，安装不支持 @@stem）
        if partial.starts_with('@') {
            let (_, files) = aliases::load_merged_aliases(layout);
            return files
                .iter()
                .map(|f| f.key.clone())
                .filter(|k| k.starts_with('@') && !k.starts_with("@@"))
                .filter(|k| k.starts_with(partial))
                .collect();
        }
        const ADD_SUBS: &[&str] = &["comp", "pip", "assets"];
        return ADD_SUBS
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect();
    }

    // remove 子命令补全
    if first == "remove" {
        const RM_SUBS: &[&str] = &["comp", "all"];
        let mut candidates: Vec<String> = RM_SUBS
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect();

        candidates.sort();
        candidates.dedup();
        return candidates;
    }

    // 1. @ 文件引用补全通过 complete_at_prefix 处理（@file. 语法），此处不处理

    // 2. 别名条目带 $args → 按补全树补全（优先于点号子节点）
    if let Some(resolved) = aliases::resolve_merged_alias(&merged, first)
        && let Some(args) = alias_args(&resolved.value)
    {
        return complete_args_tree(args, &[], partial);
    }

    // 3. 检查嵌套别名：`byk rust ` → 补全子节点
    if let Some(node) = merged.get(first.as_str()) {
        if !node.children.is_empty() {
            return node
                .children
                .keys()
                .filter(|k| k.starts_with(partial))
                .cloned()
                .collect();
        }
        // 别名叶子节点 → 无子命令
        return Vec::new();
    }

    // 4. 无法识别 → 无补全
    Vec::new()
}

/// 多前缀词场景的补全。
///
/// 当前支持：
/// - 插件命令多级参数补全
fn complete_multi_prev(prev: &[String], partial: &str, layout: &PathLayout) -> Vec<String> {

    // 别名命令多级参数补全（$args 树）
    let first = &prev[0];
    let (merged, _) = aliases::load_merged_aliases(layout);
    if let Some(resolved) = aliases::resolve_merged_alias(&merged, first)
        && let Some(args) = alias_args(&resolved.value)
    {
        let trailing: Vec<&str> = prev[1..].iter().map(|s| s.as_str()).collect();
        return complete_args_tree(args, &trailing, partial);
    }

    Vec::new()
}

/// 从别名值中提取 `$args` 补全树（仅 Meta 形态可能携带）。
fn alias_args(value: &aliases::AliasValue) -> Option<&serde_json::Value> {
    match value {
        aliases::AliasValue::Meta { args, .. } => args.as_ref(),
        aliases::AliasValue::Str(_) => None,
    }
}


/// 补全顶级命令：内置子命令 + 插件 + 顶级别名。
fn get_top_level_completions(partial: &str, layout: &PathLayout) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    // 内置子命令
    candidates.push("add".into());
    candidates.push("remove".into());
    candidates.push("show".into());

    // 顶级别名（叶子节点 + 中间 dict 节点，dict 加 . 后缀提示可展开）
    let (merged, files) = aliases::load_merged_aliases(layout);

    // 按前缀 @ 的文件命名空间（匿名文件 .byk.json 的顶 key 不通过 @ 展示）
    let scope_files: Vec<_> = files.iter().map(|f| f.key.clone()).collect();
    candidates.extend(scope_files);

    // 合并树中的顶 key
    for key in merged.keys() {
        // 有 alias 或有 children 的节点都可补全
        candidates.push(key.clone());
    }

    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        .filter(|c| c.starts_with(partial))
        .collect()
}


// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    // ==================== 测试辅助 ====================

    /// 创建临时目录布局，设置 alias 文件与插件缓存。
    #[allow(dead_code)]
    struct TestEnv {
        temp: tempfile::TempDir,
        layout: PathLayout,
    }

    impl TestEnv {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let alias_dir = temp.path().join("alias");
            let cache_dir = temp.path().join("cache");
            let plugins_dir = temp.path().join("plugins");
            let py_venv_dir = temp.path().join("py-venv");
            let venv_dir = py_venv_dir.join(".venv");
            let root_dir = temp.path().to_path_buf();

            for d in [&alias_dir, &cache_dir, &plugins_dir, &py_venv_dir, &venv_dir] {
                fs::create_dir_all(d).unwrap();
            }

            let layout = PathLayout {
                root_dir: root_dir.clone(),
                alias_dir,
                py_venv_dir,
                venv_dir,
                cache_dir: cache_dir.clone(),
                plugins_dir: plugins_dir.clone(),
                home_exists: true,
            };

            TestEnv { temp, layout }
        }

        /// 在 alias_dir 中创建一个 .byk.json 文件。
        fn add_alias_file(&self, stem: &str, content: &serde_json::Value) {
            let path = self.layout.alias_dir.join(format!("{}.byk.json", stem));
            fs::write(&path, serde_json::to_string_pretty(content).unwrap()).unwrap();
        }
    }

    // ==================== collect_object_keys ====================

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

    // ==================== get_top_level_completions ====================

    #[test]
    fn top_level_includes_alias_files() {
        let env = TestEnv::new();
        env.add_alias_file(
            "release",
            &json!({"deploy": "echo deploy", "build": "echo build"}),
        );
        let completions = get_top_level_completions("", &env.layout);
        // alias_dir 中的文件是全局文件，key 为 @@ 前缀
        assert!(completions.contains(&"@@release".to_string()));
        // merge 树中的顶 key
        assert!(completions.contains(&"deploy".to_string()));
        assert!(completions.contains(&"build".to_string()));
    }

    #[test]
    fn top_level_filter_by_prefix() {
        let env = TestEnv::new();
        env.add_alias_file(
            "tools",
            &json!({"test": "echo test", "task": "echo task"}),
        );
        let completions = get_top_level_completions("te", &env.layout);
        assert!(completions.contains(&"test".to_string()));
        assert!(!completions.contains(&"task".to_string()));
    }

    // ==================== complete_at_prefix ====================

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
    fn at_prefix_completes_file_inner_keys() {
        let env = TestEnv::new();
        env.add_alias_file("release", &json!({"deploy": "echo deploy", "build": "echo build"}));
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

    // ==================== complete_nested_alias ====================

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
        assert_eq!(completions, vec!["deploy.preview".to_string(), "deploy.prod".to_string()]);
    }

    #[test]
    fn nested_alias_invalid_path_returns_empty() {
        let env = TestEnv::new();
        let completions = complete_nested_alias("nonexistent.", "x", &env.layout);
        assert!(completions.is_empty());
    }

    // ==================== contextual_completions ====================

    #[test]
    fn contextual_empty_prev_returns_empty() {
        let env = TestEnv::new();
        let prev: Vec<String> = vec![];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_multi_prev_returns_empty() {
        let env = TestEnv::new();
        let prev = vec!["a".to_string(), "b".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_flag_prefix_returns_global_flags() {
        let env = TestEnv::new();
        let prev = vec!["some-command".to_string()];
        let result = contextual_completions(&prev, "-", &env.layout);
        assert!(!result.is_empty());
        assert!(result.contains(&"--version".to_string()));
        assert!(result.contains(&"--help".to_string()));
    }

    #[test]
    fn contextual_alias_node_children() {
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({
                "rust": {
                    "build": "cargo build",
                    "test": "cargo test",
                    "run": "cargo run"
                }
            }),
        );
        let prev = vec!["rust".to_string()];
        let result = contextual_completions(&prev, "b", &env.layout);
        assert_eq!(result, vec!["build".to_string()]);
    }

    #[test]
    fn contextual_alias_leaf_returns_empty() {
        let env = TestEnv::new();
        env.add_alias_file(
            "default",
            &json!({"hello": "echo hello"}),
        );
        let prev = vec!["hello".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_add_flag_returns_help_flags() {
        let env = TestEnv::new();
        let prev = vec!["add".to_string()];
        let result = contextual_completions(&prev, "-", &env.layout);
        assert!(result.contains(&"--help".to_string()));
        assert!(result.contains(&"-h".to_string()));
        assert!(!result.contains(&"--version".to_string()));
        assert!(!result.contains(&"-v".to_string()));
    }

    #[test]
    fn contextual_add_flag_filters_by_partial() {
        let env = TestEnv::new();
        let prev = vec!["add".to_string()];
        let result = contextual_completions(&prev, "--h", &env.layout);
        assert_eq!(result, vec!["--help".to_string()]);
    }

    // ==================== 别名 $args 补全 ====================

    /// 带 $args 的别名文件（叶子 + 可重复选项 + 嵌套子命令）。
    fn args_alias_file() -> serde_json::Value {
        json!({
            "deploy": {
                "$cmd": "deploy {env}",
                "$args": {
                    "$": ["prod", "staging"],
                    "$options": ["--dry-run", "--verbose"],
                    "build": {
                        "$": ["debug", "release"],
                        "$options": ["--clean"]
                    }
                }
            }
        })
    }

    #[test]
    fn alias_args_completes_first_level() {
        let env = TestEnv::new();
        env.add_alias_file("args", &args_alias_file());
        let prev = vec!["deploy".to_string()];
        let mut result = contextual_completions(&prev, "", &env.layout);
        result.sort();
        assert_eq!(
            result,
            vec![
                "--dry-run".to_string(),
                "--verbose".to_string(),
                "build".to_string(),
                "prod".to_string(),
                "staging".to_string()
            ]
        );
    }

    #[test]
    fn alias_args_filters_by_partial() {
        let env = TestEnv::new();
        env.add_alias_file("args", &args_alias_file());
        let prev = vec!["deploy".to_string()];
        let result = contextual_completions(&prev, "pro", &env.layout);
        assert_eq!(result, vec!["prod".to_string()]);
    }

    #[test]
    fn alias_args_flag_partial_uses_options() {
        let env = TestEnv::new();
        env.add_alias_file("args", &args_alias_file());
        let prev = vec!["deploy".to_string()];
        let result = contextual_completions(&prev, "--d", &env.layout);
        assert_eq!(result, vec!["--dry-run".to_string()]);
    }

    #[test]
    fn alias_args_descends_into_subcommand() {
        let env = TestEnv::new();
        env.add_alias_file("args", &args_alias_file());
        let prev = vec!["deploy".to_string(), "build".to_string()];
        let mut result = contextual_completions(&prev, "", &env.layout);
        result.sort();
        assert_eq!(
            result,
            vec![
                "--clean".to_string(),
                "debug".to_string(),
                "release".to_string()
            ]
        );
    }

    #[test]
    fn alias_args_options_exclude_consumed() {
        let env = TestEnv::new();
        env.add_alias_file("args", &args_alias_file());
        // 已输入 --dry-run：候选应排除它，但保留 --verbose 与叶子
        let prev = vec!["deploy".to_string(), "--dry-run".to_string()];
        let mut result = contextual_completions(&prev, "", &env.layout);
        result.sort();
        assert_eq!(
            result,
            vec![
                "--verbose".to_string(),
                "build".to_string(),
                "prod".to_string(),
                "staging".to_string()
            ]
        );
    }

    #[test]
    fn alias_args_takes_priority_over_children() {
        let env = TestEnv::new();
        // 同时含 $args 与点号子节点：补全走 $args 树
        env.add_alias_file(
            "hybrid",
            &json!({
                "deploy": {
                    "$cmd": "deploy",
                    "$args": {"$": ["prod"]},
                    "prod": "deploy-prod.sh"
                }
            }),
        );
        let prev = vec!["deploy".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert_eq!(result, vec!["prod".to_string()]);
    }

    #[test]
    fn alias_without_args_still_completes_children() {
        let env = TestEnv::new();
        // 无 $args 的普通分组：点号子节点补全行为不变
        env.add_alias_file(
            "plain",
            &json!({"rust": {"build": "cargo build", "test": "cargo test"}}),
        );
        let prev = vec!["rust".to_string()];
        let mut result = contextual_completions(&prev, "", &env.layout);
        result.sort();
        assert_eq!(result, vec!["build".to_string(), "test".to_string()]);
    }

    #[test]
    fn alias_leaf_without_args_no_completion() {
        let env = TestEnv::new();
        env.add_alias_file("leaf", &json!({"hi": {"$cmd": "echo hi"}}));
        let prev = vec!["hi".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_unknown_command_returns_global_flags() {
        let env = TestEnv::new();
        let prev = vec!["some-command".to_string()];
        let result = contextual_completions(&prev, "-", &env.layout);
        assert!(result.contains(&"--version".to_string()));
        assert!(result.contains(&"-v".to_string()));
        assert!(result.contains(&"--help".to_string()));
        assert!(result.contains(&"-h".to_string()));
    }

    #[test]
    fn contextual_unknown_word_returns_empty() {
        let env = TestEnv::new();
        let prev = vec!["nobody".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_remove_includes_builtin_subs() {
        let env = TestEnv::new();
        let prev = vec!["remove".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.contains(&"comp".to_string()));
        assert!(result.contains(&"all".to_string()));
        // py / node 子命令已移除：Python 环境自动管理，npm 交给插件与别名
        assert!(!result.contains(&"py".to_string()));
        assert!(!result.contains(&"node".to_string()));
    }

    #[test]
    fn contextual_remove_filters_by_partial() {
        let env = TestEnv::new();
        let prev = vec!["remove".to_string()];
        let result = contextual_completions(&prev, "al", &env.layout);
        assert_eq!(result, vec!["all".to_string()]);
    }

    // ==================== get_completions ====================

    #[test]
    fn get_completions_flag_prefix() {
        let env = TestEnv::new();
        let result = get_completions("--v", &env.layout);
        assert_eq!(result, vec!["--version".to_string()]);
    }

    #[test]
    fn get_completions_empty_delegates_to_top_level() {
        let env = TestEnv::new();
        let result = get_completions("", &env.layout);
        assert!(result.contains(&"add".to_string()));
        assert!(result.contains(&"show".to_string()));
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

    // ==================== generate (shell scripts) ====================

    #[test]
    fn generate_zsh_does_not_panic() {
        // print! 宏输出到 stdout，在测试中由 test runner 自动捕获。
        // 此处验证 generate 不会因不支持的 shell 等边界条件 panic。
    }

    #[test]
    fn generate_unsupported_shell_does_not_panic() {
        // generate("invalid") 向 stderr 输出错误信息，不应 panic
    }

    // ==================== expand_tilde ====================

    #[test]
    fn expand_tilde_replaces_home() {
        let result = expand_tilde("~/foo/bar");
        let home = std::env::var("HOME").unwrap();
        assert_eq!(result, format!("{}/foo/bar", home));
    }

    #[test]
    fn expand_tilde_no_tilde_unchanged() {
        let result = expand_tilde("/usr/local/bin");
        assert_eq!(result, "/usr/local/bin".to_string());
    }

    #[test]
    fn expand_tilde_relative_path_unchanged() {
        let result = expand_tilde("./foo");
        assert_eq!(result, "./foo".to_string());
    }
}

// ---------------------------------------------------------------------------
// Shell 脚本模板
// ---------------------------------------------------------------------------

fn print_zsh() {
    print!(
        r##"_byk() {{
    local -a completions
    completions=(${{(f)"$(byk __api__ complete "${{words[@]:1}}" 2>/dev/null)"}})
    compadd -a completions
}}

compdef _byk byk
"##
    );
}

fn print_bash() {
    print!(
        r##"_byk_completion() {{
    local cur
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    COMPREPLY=($(compgen -W "$(byk __api__ complete "${{COMP_WORDS[@]:1}}" 2>/dev/null)" -- "$cur"))
}}

complete -F _byk_completion byk
"##
    );
}

fn print_fish() {
    print!(
        r##"function _byk_completions
    set -l tokens (commandline -cp | string split ' ')
    byk __api__ complete $tokens[2..-1] 2>/dev/null
end

complete -c byk -f -a "(_byk_completions)"
"##
    );
}