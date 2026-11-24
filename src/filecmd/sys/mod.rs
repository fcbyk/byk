//! 系统文件命令路由：`@@@<name> <args...>`。
//!
//! `@@@` 与 `@` / `@@` 共用同一个认领点（`filecmd::spec::parse`），但语义不同：
//! 它指向 CLI Home 里的文件（`~/.byk/<name>.json`），既不参与别名查找，
//! 也没有链接、依赖这些概念。
//!
//! 这一层只认名字。名字之后的语法——有没有动词、`-h` 算什么、参数怎么解——
//! 全归各文件自己定，不强行统一：`config` 是键值表，将来别的文件未必是。

pub(crate) mod config;

use crate::model::PathLayout;

/// 已注册的系统文件；顺序即错误提示与补全顺序。
///
/// 表而不是裸 `match`：未知名单、补全、将来的帮助都要同一份名单，
/// 各写一遍迟早会不一致。
const FILES: &[(&str, &str)] = &[("config", "byk settings")];

pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(name, _)| *name)
}

/// 分发一次系统文件调用；返回的错误已是可以直接打印的完整文案。
pub(crate) fn run(stem: &str, args: &[String], layout: &PathLayout) -> Result<(), String> {
    match resolve_name(stem)? {
        "config" => config::run(args, layout),
        other => unreachable!("system file {other:?} is registered but not dispatched"),
    }
}

/// 把 `@@@` 后面那段文字换成注册表里的文件身份。
fn resolve_name(stem: &str) -> Result<&'static str, String> {
    if stem.is_empty() {
        return Err(format!(
            "Missing system file name\nAvailable: {}",
            available()
        ));
    }
    // 点号在这里是明确的误用信号：`@@@config.set` 是想写 `@@@config set ...`，
    // 不能掉进别名查找，否则用户等到的是 "alias not found"。
    if stem.contains('.') {
        return Err(format!(
            "Invalid system file: {:?}\nHint: system files take arguments after a space, e.g. `byk @@@config banner never`",
            stem
        ));
    }
    FILES
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(name, _)| *name)
        .ok_or_else(|| {
            format!(
                "Unknown system file: {:?}\nAvailable: {}",
                stem,
                available()
            )
        })
}

fn available() -> String {
    names()
        .map(|name| format!("@@@{}", name))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_registered_name() {
        assert_eq!(resolve_name("config").unwrap(), "config");
    }

    #[test]
    fn rejects_empty_name_with_available_list() {
        let err = resolve_name("").unwrap_err();
        assert!(err.starts_with("Missing system file name"), "{err}");
        assert!(err.contains("@@@config"), "{err}");
    }

    #[test]
    fn rejects_unknown_name_with_available_list() {
        let err = resolve_name("confg").unwrap_err();
        assert!(err.starts_with("Unknown system file: \"confg\""), "{err}");
        assert!(err.contains("Available: @@@config"), "{err}");
    }

    #[test]
    fn dotted_name_gets_a_rewrite_hint() {
        let err = resolve_name("config.set").unwrap_err();
        assert!(
            err.starts_with("Invalid system file: \"config.set\""),
            "{err}"
        );
        assert!(err.contains("byk @@@config banner never"), "{err}");
    }
}
