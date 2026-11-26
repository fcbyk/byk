//! 文件作用域：`work@` / `global@` / `special@`。
//!
//! 前缀按唯一前缀匹配（`w@`、`wor@`、`work@` 等价）。命令位、精确语法、来源位
//! 与补全都调这里，避免各自 `strip_prefix` 后规则漂移。

/// 别名文件与特殊文件的作用域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// 工作目录里的别名文件
    Work,
    /// CLI 别名目录里的别名文件
    Global,
    /// 非别名的特殊文件（config / tap / shell）
    Special,
}

impl Scope {
    pub(crate) fn all() -> [Scope; 3] {
        [Scope::Work, Scope::Global, Scope::Special]
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Scope::Work => "work",
            Scope::Global => "global",
            Scope::Special => "special",
        }
    }

    /// 完整写法，用于拼 key 与提示文案。
    pub(crate) fn token(self) -> &'static str {
        match self {
            Scope::Work => "work@",
            Scope::Global => "global@",
            Scope::Special => "special@",
        }
    }

    pub(crate) fn is_global(self) -> bool {
        matches!(self, Scope::Global)
    }

    /// 按第一个 `@` 切分：`w@ops` → `(Work, "ops")`，`work@` → `(Work, "")`。
    ///
    /// `@` 左侧为空、未命中或命中多个作用域时返回 `None`，由调用方决定回退路径还是报错。
    pub(crate) fn split(token: &str) -> Option<(Scope, &str)> {
        let (head, rest) = token.split_once('@')?;
        let mut hit: Option<Scope> = None;
        for scope in Self::all() {
            if scope.as_str().starts_with(head) {
                if hit.is_some() {
                    return None;
                }
                hit = Some(scope);
            }
        }
        hit.map(|scope| (scope, rest))
    }

    /// 命令位的形状保护：`@` 左侧是非空 ASCII 字母、却没解析成作用域时返回那段。
    ///
    /// 路径（`./dir@2`）与带数字的名字不算作用域形态，交回普通查找，
    /// 免得把行内 `@` 报成一个奇怪的 `Unknown scope`。
    pub(crate) fn unknown_prefix(token: &str) -> Option<&str> {
        if Scope::split(token).is_some() {
            return None;
        }
        let (head, _) = token.split_once('@')?;
        (!head.is_empty() && head.bytes().all(|byte| byte.is_ascii_alphabetic())).then_some(head)
    }

    /// 提示用：`work@, global@, special@`。
    pub(crate) fn tokens() -> String {
        Scope::all()
            .iter()
            .map(|scope| scope.token())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_every_prefix_length() {
        for typed in ["w@ops", "wo@ops", "wor@ops", "work@ops"] {
            assert_eq!(Scope::split(typed), Some((Scope::Work, "ops")), "{typed}");
        }
        assert_eq!(Scope::split("g@ops"), Some((Scope::Global, "ops")));
        assert_eq!(Scope::split("s@config"), Some((Scope::Special, "config")));
    }

    #[test]
    fn keeps_the_rest_verbatim() {
        assert_eq!(Scope::split("work@"), Some((Scope::Work, "")));
        assert_eq!(Scope::split("work@.build"), Some((Scope::Work, ".build")));
        assert_eq!(
            Scope::split("special@config.list"),
            Some((Scope::Special, "config.list"))
        );
    }

    #[test]
    fn rejects_non_scope_heads() {
        assert_eq!(Scope::split("@ops"), None);
        assert_eq!(Scope::split("@@ops"), None);
        assert_eq!(Scope::split("ops"), None);
        assert_eq!(Scope::split("gla@ops"), None);
        assert_eq!(Scope::split("WORK@ops"), None);
        assert_eq!(Scope::split("hello@123"), None);
        assert_eq!(Scope::split("./dir@2"), None);
    }

    #[test]
    fn reports_only_scope_shaped_heads() {
        assert_eq!(Scope::unknown_prefix("gla@ops"), Some("gla"));
        assert_eq!(Scope::unknown_prefix("hello@123"), Some("hello"));
        assert_eq!(Scope::unknown_prefix("work@ops"), None);
        // 路径与带非字母的名字不是作用域形态
        assert_eq!(Scope::unknown_prefix("./dir@2"), None);
        assert_eq!(Scope::unknown_prefix("C:\\a@b"), None);
        assert_eq!(Scope::unknown_prefix("@ops"), None);
        assert_eq!(Scope::unknown_prefix("ops"), None);
    }

    #[test]
    fn tokens_lists_every_scope() {
        assert_eq!(Scope::tokens(), "work@, global@, special@");
    }
}
