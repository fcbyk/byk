//! 别名调用链与递归保护。

/// 调用链标记：逗号分隔的 `file.key` 标识列表，随环境变量下发给子进程。
///
/// 环境变量是唯一能被任意 shell 透传的通道：`$cmd` 是任意 shell 字符串，
/// 没有别的办法把「谁调了我」告诉下一层 byk。它有两个已知的粗糙边角：
/// `env -i`、外部包装器主动清空环境时标记会丢（由深度兜底接住）；
/// 别名起了一个交互式 shell 时，标记会一直留在那层 shell 里，直到它退出。
/// 两者都可以用 `BYK_ALLOW_RECURSION` / `$recursive` 绕过。
pub(super) const ENV_ALIAS_STACK: &str = "BYK_ALIAS_STACK";

/// 逃生口：置为真值时跳过环路检查（深度上限仍然生效）。
pub(super) const ENV_ALLOW_RECURSION: &str = "BYK_ALLOW_RECURSION";

/// 调用链深度上限，覆盖标记被包装器清除的场景。
///
/// 32 层留给「别名套别名」的正常编排绰绰有余，又远小于把机器打死所需的量级。
pub(super) const MAX_ALIAS_DEPTH: usize = 32;

/// 别名链路检查失败的原因。
#[derive(Debug)]
pub(super) enum ChainError {
    /// 同一别名已在链路上：直接自引用，或 `A -> B -> A` 互递归。
    Cycle { chain: Vec<String> },
    /// 链路过长：标记被清除时唯一还剩下的闸门。
    TooDeep { chain: Vec<String> },
}

impl ChainError {
    /// 面向用户的错误正文（不含 `Error: ` 前缀）。
    pub(super) fn message(&self) -> String {
        match self {
            ChainError::Cycle { chain } => {
                format!("alias recursion detected: {}", format_chain(chain))
            }
            ChainError::TooDeep { chain } => format!(
                "alias call depth limit ({}) exceeded: {}",
                MAX_ALIAS_DEPTH,
                format_chain(chain)
            ),
        }
    }
}

/// 读取从父进程继承下来的调用链。
///
/// 解析取宽容策略：空段直接丢掉。别名 key 理论上可以含逗号（只禁了 `@` 和 `.`），
/// 那种情况下拆出来的身份对不上，环路检查落空——深度上限仍在后面兜着。
pub(super) fn inherited_stack() -> Vec<String> {
    match std::env::var(ENV_ALIAS_STACK) {
        Ok(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 环境变量是否被置为真值（`1` / `true` / `yes` / `on`，不区分大小写）。
pub(super) fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// 把本级别名压入链路，返回压栈后的新链路。
///
/// 用栈而不是单值比对，才能同时覆盖 `A -> A` 和 `A -> B -> A`：
/// 后者单值比对只看得到「上一层不是 A」。
pub(super) fn push_identity(
    stack: &[String],
    identity: &str,
    allow_recursion: bool,
) -> Result<Vec<String>, ChainError> {
    if !allow_recursion && let Some(pos) = stack.iter().position(|id| id == identity) {
        // 从重复点截起：前面那些是外层上下文，要定位的是「绕回来的这一圈」
        let mut chain = stack.to_vec();
        chain.push(identity.to_string());
        return Err(ChainError::Cycle {
            chain: chain[pos..].to_vec(),
        });
    }

    let mut chain = stack.to_vec();
    chain.push(identity.to_string());
    if chain.len() > MAX_ALIAS_DEPTH {
        return Err(ChainError::TooDeep { chain });
    }
    Ok(chain)
}

/// 链路太长时折叠中段，只留头尾两段便于定位。
pub(super) fn format_chain(chain: &[String]) -> String {
    const HEAD: usize = 2;
    const TAIL: usize = 4;
    if chain.len() <= HEAD + TAIL + 1 {
        return chain.join(" -> ");
    }
    let mut parts: Vec<String> = chain[..HEAD].to_vec();
    parts.push(format!("... ({} more)", chain.len() - HEAD - TAIL));
    parts.extend_from_slice(&chain[chain.len() - TAIL..]);
    parts.join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ids(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn chain_first_entry_is_allowed() {
        let chain = push_identity(&[], "@f.build", false).unwrap();
        assert_eq!(chain, ids(&["@f.build"]));
    }

    #[test]
    fn chain_nested_different_aliases_allowed() {
        // 别名套别名是正常编排：只要链路上没有同一个身份就放行
        let chain = push_identity(&ids(&["@f.build"]), "@f.test", false).unwrap();
        assert_eq!(chain, ids(&["@f.build", "@f.test"]));
    }

    #[test]
    fn chain_direct_self_reference_detected() {
        let err = push_identity(&ids(&["@f.add"]), "@f.add", false).unwrap_err();
        match err {
            ChainError::Cycle { chain } => assert_eq!(chain, ids(&["@f.add", "@f.add"])),
            other => panic!("expected Cycle, got {:?}", other),
        }
    }

    #[test]
    fn chain_mutual_recursion_detected() {
        // A -> B -> A：单值比对看不出问题，栈才能看见
        let err = push_identity(&ids(&["@f.a", "@f.b"]), "@f.a", false).unwrap_err();
        match err {
            ChainError::Cycle { chain } => assert_eq!(chain, ids(&["@f.a", "@f.b", "@f.a"])),
            other => panic!("expected Cycle, got {:?}", other),
        }
    }

    #[test]
    fn chain_same_key_in_different_files_is_not_recursion() {
        // 两个文件里的同名别名是两张不同的配置，链路标识带 file 前缀正是为此
        let chain = push_identity(&ids(&["@dev.build"]), "@release.build", false).unwrap();
        assert_eq!(chain, ids(&["@dev.build", "@release.build"]));
    }

    #[test]
    fn chain_allow_recursion_bypasses_cycle_check() {
        let chain = push_identity(&ids(&["@f.loop"]), "@f.loop", true).unwrap();
        assert_eq!(chain, ids(&["@f.loop", "@f.loop"]));
    }

    #[test]
    fn chain_depth_limit_is_backstop() {
        let stack: Vec<String> = (0..MAX_ALIAS_DEPTH).map(|i| format!("@f.a{}", i)).collect();
        let err = push_identity(&stack, "@f.last", false).unwrap_err();
        assert!(matches!(err, ChainError::TooDeep { .. }));
        assert!(err.message().contains("depth limit"));
    }

    #[test]
    fn chain_allows_exactly_max_depth() {
        let stack: Vec<String> = (0..MAX_ALIAS_DEPTH - 1)
            .map(|i| format!("@f.a{}", i))
            .collect();
        let chain = push_identity(&stack, "@f.last", false).unwrap();
        assert_eq!(chain.len(), MAX_ALIAS_DEPTH);
    }

    #[test]
    fn chain_depth_limit_applies_even_when_recursion_allowed() {
        let stack: Vec<String> = (0..MAX_ALIAS_DEPTH)
            .map(|_| "@f.loop".to_string())
            .collect();
        let err = push_identity(&stack, "@f.loop", true).unwrap_err();
        assert!(matches!(err, ChainError::TooDeep { .. }));
    }

    #[test]
    fn chain_message_reports_full_path() {
        let err = push_identity(&ids(&["@f.a", "@f.b"]), "@f.a", false).unwrap_err();
        assert_eq!(
            err.message(),
            "alias recursion detected: @f.a -> @f.b -> @f.a"
        );
    }

    #[test]
    fn chain_format_keeps_short_chain_intact() {
        assert_eq!(format_chain(&ids(&["a", "b", "c"])), "a -> b -> c");
    }

    #[test]
    fn chain_format_folds_long_chain() {
        let long: Vec<String> = (0..20).map(|i| format!("a{}", i)).collect();
        let rendered = format_chain(&long);
        assert!(rendered.starts_with("a0 -> a1 -> ... (14 more) -> "));
        assert!(rendered.ends_with("a19"));
        // 中间那段被折叠掉，长度不会随深度线性膨胀
        assert!(!rendered.contains("a10"));
    }
}
