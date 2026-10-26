//! `$args` 补全树：按补全树逐层下降收集候选。
//!
//! 与插件协议 `args`、别名 `$args` 同构。纯函数，被 `completion::mod` 的
//! 上下文补全逻辑调用。

/// 根据补全树（插件协议 args / 别名 `$args`）逐层下降，收集目标层级的候选。
///
/// `trailing`：已输入的子命令序列（不含命令名本身）
/// `partial`：当前正在输入的部分词
///
/// 协议约定（插件协议 args 与别名 `$args` 同构）：
/// - `$` 数组：叶子候选（不可重复，选中后无下级）
/// - `$options` 数组：可重复选项（选中后留在当前层，排除已选）
/// - 其他 key：子命令（选中后进入该 key 的值继续补全）
/// - value 可以是 string[] 或 object，支持任意嵌套
pub(crate) fn complete_args_tree(
    args: &serde_json::Value,
    trailing: &[&str],
    partial: &str,
) -> Vec<String> {
    let mut current = args;
    // consumed_options 记录在当前层已被消费的 $options
    let mut consumed_options: Vec<&str> = Vec::new();

    for t in trailing.iter() {
        // 判断 t 是否是当前层的子命令（非 $ 非 $options 的 key）
        if let serde_json::Value::Object(obj) = current
            && obj.contains_key(*t) && *t != "$" && *t != "$options"
        {
            // 子命令：深入
            current = &obj[*t];
            consumed_options.clear();
            continue;
        }

        // 不是子命令 → 可能是 option，检查当前层是否有 $options
        if let serde_json::Value::Object(obj) = current
            && let Some(opts) = obj.get("$options").and_then(|v| v.as_array())
        {
            let is_option = opts.iter().any(|v| v.as_str() == Some(*t));
            if is_option {
                // trailing 均为已确认输入的词（partial 由调用方单独传入），
                // 已选项加入 consumed，收集候选时排除
                consumed_options.push(t);
                continue;
            }
        }

        // 既不是子命令也不是已知 option → 无法匹配
        return Vec::new();
    }

    // 到达目标层级，收集候选
    collect_args_candidates(current, partial, &consumed_options)
}

/// 从 args 的某个节点收集补全候选。
///
/// `consumed_options`：当前层已消费的 $options（需排除）。
pub(crate) fn collect_args_candidates(
    node: &serde_json::Value,
    partial: &str,
    consumed: &[&str],
) -> Vec<String> {
    match node {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| s.starts_with(partial))
            .map(|s| s.to_string())
            .collect(),
        serde_json::Value::Object(obj) => {
            let mut candidates: Vec<String> = Vec::new();

            // $options：可重复选项，排除已消费的
            if let Some(opts) = obj.get("$options").and_then(|v| v.as_array()) {
                candidates.extend(
                    opts.iter()
                        .filter_map(|v| v.as_str())
                        .filter(|s| !consumed.contains(s) && s.starts_with(partial))
                        .map(|s| s.to_string()),
                );
            }

            // $ 数组中的叶子候选
            if let Some(leaves) = obj.get("$").and_then(|v| v.as_array()) {
                candidates.extend(
                    leaves
                        .iter()
                        .filter_map(|v| v.as_str())
                        .filter(|s| s.starts_with(partial))
                        .map(|s| s.to_string()),
                );
            }

            // 非 $ / $options 的 key（子命令）
            candidates.extend(
                obj.keys()
                    .filter(|k| *k != "$" && *k != "$options" && k.starts_with(partial))
                    .cloned(),
            );

            candidates.sort();
            candidates
        }
        _ => Vec::new(),
    }
}
