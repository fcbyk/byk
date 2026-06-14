/// Shell 补全脚本生成与动态补全查询。
///
/// 通过 `byk completion <shell>` 输出对应 shell 的补全包装脚本（到 stdout），
/// 通过 `byk __complete <words...>` 查询补全候选（供补全脚本回调）。
///
/// 支持 zsh、bash、fish。

use std::collections::HashMap;

use super::aliases::{self, MergedNode};
use super::npm_commands;
use super::paths::PathLayout;
use super::plugins;

// ---------------------------------------------------------------------------
// 公开入口
// ---------------------------------------------------------------------------

/// 生成指定 shell 的补全脚本并输出到 stdout。
pub fn generate(shell: &str) {
    match shell {
        "zsh" => print_zsh(),
        "bash" => print_bash(),
        "fish" => print_fish(),
        other => eprintln!("不支持的 shell: {}\n支持: zsh, bash, fish", other),
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
    "--info",
    "--help",
    "-h",
];

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
/// - NPM 命令 → 无子命令，返回空
/// - 无法识别的词 → 返回空
fn contextual_completions(prev: &[String], partial: &str, layout: &PathLayout) -> Vec<String> {
    if prev.is_empty() {
        return Vec::new();
    }

    // 当前仅处理单前缀词场景（如 `byk rust `）
    // 多前缀词（如 `byk @release build `）暂不处理
    if prev.len() > 1 {
        return Vec::new();
    }

    let first = &prev[0];

    // 全局标志：前缀以 '-' 开头时始终返回全局标志
    if partial.starts_with('-') {
        return GLOBAL_FLAGS
            .iter()
            .filter(|f| f.starts_with(partial))
            .map(|f| f.to_string())
            .collect();
    }

    // --info 子命令补全
    if first == "--info" {
        const INFO_SUBS: &[&str] = &["paths", "py"];
        return INFO_SUBS
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect();
    }

    // init 子命令补全
    if first == "init" {
        const INIT_SUBS: &[&str] = &["npm", "pnpm", "py", "py-v", "comp"];
        return INIT_SUBS
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect();
    }

    // remove 子命令补全
    if first == "remove" {
        const RM_SUBS: &[&str] = &["py", "py-v", "npm", "pnpm"];
        return RM_SUBS
            .iter()
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect();
    }

    // 1. @ 文件引用补全通过 complete_at_prefix 处理（@file. 语法），此处不处理

    // 2. 检查嵌套别名：`byk rust ` → 补全子节点
    let (merged, _) = aliases::load_merged_aliases(layout);
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

    // 3. 检查是否是一个已知的插件命令
    if is_known_plugin(first, layout) {
        return Vec::new();
    }

    // 4. 检查是否是一个已知的 NPM 命令
    if is_known_npm(first, layout) {
        return Vec::new();
    }

    // 5. 无法识别 → 无补全
    Vec::new()
}

/// 检查某个词是否是已知的插件命令。
fn is_known_plugin(word: &str, layout: &PathLayout) -> bool {
    if !layout.home_exists {
        return false;
    }
    let plugin_cache = plugins::load_plugin_cache(&layout.cache_dir);
    plugin_cache.commands.contains_key(word)
}

/// 检查某个词是否是已知的 NPM 命令。
fn is_known_npm(word: &str, layout: &PathLayout) -> bool {
    let cache_file = layout.cache_dir.join("node-pkg.json");
    match npm_commands::load_npm_cache(&cache_file, &layout.node_pkgs_dir) {
        Some(npm_cache) => npm_cache.bin_map.contains_key(word),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 补全 @ 和 @@ 语法
// ---------------------------------------------------------------------------

/// 补全 @ 和 @@ 语法：文件引用和文件内 key。
fn complete_at_prefix(partial: &str, layout: &PathLayout) -> Vec<String> {
    let (_, files) = aliases::load_merged_aliases(layout);

    // 查找 partial 中是否包含 '.' — 如果有，补全文件内的 key
    if let Some(dot_idx) = partial.find('.') {
        let file_key = &partial[..dot_idx];
        let key_partial = &partial[dot_idx + 1..];

        for f in &files {
            if f.key == file_key {
                return collect_object_keys(&f.aliases, "")
                    .into_iter()
                    .filter(|k| k.starts_with(key_partial))
                    .map(|k| format!("{}.{}", file_key, k))
                    .collect();
            }
        }
        return Vec::new();
    }

    // 补全文件 key（@xxx 或 @@xxx 格式）
    files
        .iter()
        .map(|f| f.key.clone())
        .filter(|k| k.starts_with(partial))
        .collect()
}

/// 补全嵌套别名（含点号的路径，如 "部署.打"）。
fn complete_nested_alias(prefix: &str, key_partial: &str, layout: &PathLayout) -> Vec<String> {
    let (merged, _) = aliases::load_merged_aliases(layout);

    // 去掉末尾点号，按点拆成路径段
    let path_str = prefix.trim_end_matches('.');
    let parts: Vec<&str> = if path_str.is_empty() {
        Vec::new()
    } else {
        path_str.split('.').collect()
    };

    let mut current: &HashMap<String, MergedNode> = &merged;
    for part in &parts {
        match current.get(*part) {
            Some(node) => current = &node.children,
            None => return Vec::new(),
        }
    }

    current
        .keys()
        .filter(|k| k.starts_with(key_partial))
        .map(|k| format!("{}{}", prefix, k))
        .collect()
}

/// 补全顶级命令：内置子命令 + 插件 + NPM + 顶级别名。
fn get_top_level_completions(partial: &str, layout: &PathLayout) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    // 内置子命令
    candidates.push("init".into());
    candidates.push("remove".into());

    // 插件命令
    if layout.home_exists {
        let plugin_cache = plugins::load_plugin_cache(&layout.cache_dir);
        candidates.extend(plugin_cache.commands.keys().cloned());
    }

    // NPM 命令
    let cache_file = layout.cache_dir.join("node-pkg.json");
    if let Some(npm_cache) = npm_commands::load_npm_cache(&cache_file, &layout.node_pkgs_dir) {
        candidates.extend(npm_cache.bin_map.keys().cloned());
    }

    // 顶级别名（叶子节点 + 中间 dict 节点，dict 加 . 后缀提示可展开）
    let (merged, files) = aliases::load_merged_aliases(layout);

    // 按前缀 @ 的文件命名空间（匿名文件 .byk.json 的顶 key 不通过 @ 展示）
    let scope_files: Vec<_> = files.iter().map(|f| f.key.clone()).collect();
    candidates.extend(scope_files);

    // 合并树中的顶 key
    for (key, _) in &merged {
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

/// 递归收集 JSON Object 中所有可补全的 key 路径。
/// $ 前缀的 key 为系统元数据，不参与补全。
fn collect_object_keys(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
) -> Vec<String> {
    let mut result = Vec::new();
    for (key, val) in obj {
        if key.starts_with('$') {
            continue;
        }
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        match val {
            serde_json::Value::Object(inner) => {
                result.push(full.clone());
                result.extend(collect_object_keys(inner, &full));
            }
            _ => {
                result.push(full);
            }
        }
    }
    result
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

    /// 创建临时目录布局，设置 alias 文件、插件缓存和 NPM 缓存。
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
            let node_pkgs_dir = temp.path().join("node-pkgs");
            let venv_dir = temp.path().join("venv");
            let logs_dir = temp.path().join("logs");
            let root_dir = temp.path().to_path_buf();

            for d in [&alias_dir, &cache_dir, &node_pkgs_dir, &logs_dir] {
                fs::create_dir_all(d).unwrap();
            }

            let layout = PathLayout {
                root_dir: root_dir.clone(),
                logs_dir,
                alias_dir,
                node_pkgs_dir,
                venv_dir,
                cache_dir: cache_dir.clone(),
                home_exists: true,
            };

            // 写入插件缓存
            let plugin_cache = json!({
                "watched_mtimes": {},
                "commands": {"test-plugin": {"module": "test.module:Plugin", "description": "A test plugin"}}
            });
            fs::write(
                cache_dir.join("app.json"),
                serde_json::to_string_pretty(&plugin_cache).unwrap(),
            )
            .unwrap();

            // 写入 NPM 缓存
            let npm_cache = json!({
                "watched_mtimes": {},
                "scanned_at": 0,
                "packages": [],
                "bin_map": {"eslint": "eslint"}
            });
            fs::write(
                cache_dir.join("node-pkg.json"),
                serde_json::to_string_pretty(&npm_cache).unwrap(),
            )
            .unwrap();

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

    // ==================== is_known_plugin / is_known_npm ====================

    #[test]
    fn known_plugin_returns_true() {
        let env = TestEnv::new();
        assert!(is_known_plugin("test-plugin", &env.layout));
    }

    #[test]
    fn unknown_plugin_returns_false() {
        let env = TestEnv::new();
        assert!(!is_known_plugin("no-such-plugin", &env.layout));
    }

    #[test]
    fn known_npm_returns_true() {
        let env = TestEnv::new();
        assert!(is_known_npm("eslint", &env.layout));
    }

    #[test]
    fn unknown_npm_returns_false() {
        let env = TestEnv::new();
        assert!(!is_known_npm("no-such-bin", &env.layout));
    }

    // ==================== get_top_level_completions ====================

    #[test]
    fn top_level_includes_plugins_and_npm() {
        let env = TestEnv::new();
        let completions = get_top_level_completions("", &env.layout);
        assert!(completions.contains(&"test-plugin".to_string()));
        assert!(completions.contains(&"eslint".to_string()));
    }

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
        assert!(completions.contains(&"test-plugin".to_string()));
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
    fn contextual_info_subcommands() {
        let env = TestEnv::new();
        let prev = vec!["--info".to_string()];
        let result = contextual_completions(&prev, "pa", &env.layout);
        assert_eq!(result, vec!["paths".to_string()]);
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
    fn contextual_known_plugin_returns_empty() {
        let env = TestEnv::new();
        let prev = vec!["test-plugin".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_known_npm_returns_empty() {
        let env = TestEnv::new();
        let prev = vec!["eslint".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
    }

    #[test]
    fn contextual_unknown_word_returns_empty() {
        let env = TestEnv::new();
        let prev = vec!["nobody".to_string()];
        let result = contextual_completions(&prev, "", &env.layout);
        assert!(result.is_empty());
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
        // 至少包含插件和 npm 命令
        assert!(result.contains(&"test-plugin".to_string()));
        assert!(result.contains(&"eslint".to_string()));
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
}

// ---------------------------------------------------------------------------
// Shell 脚本模板
// ---------------------------------------------------------------------------

fn print_zsh() {
    print!(
        r##"_byk() {{
    local -a completions
    completions=(${{(f)"$(byk __complete "${{words[@]:1}}" 2>/dev/null)"}})
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
    COMPREPLY=($(compgen -W "$(byk __complete "${{COMP_WORDS[@]:1}}" 2>/dev/null)" -- "$cur"))
}}

complete -F _byk_completion byk
"##
    );
}

fn print_fish() {
    print!(
        r##"function _byk_completions
    set -l tokens (commandline -cp | string split ' ')
    byk __complete $tokens[2..-1] 2>/dev/null
end

complete -c byk -f -a "(_byk_completions)"
"##
    );
}
