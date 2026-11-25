//! 系统文件命令路由：`sys@<name> <args...>`。
//!
//! `local@` / `global@` 指向别名文件，`sys@` 指向别名之外的任何文件（byk 自己的配置、
//! 用户的启动文件）。这一层只认名字，之后的语法——有无动词、`-h` 算什么——归各文件自己定。

pub(crate) mod config;
pub(crate) mod shell;
pub(crate) mod store;
pub(crate) mod tap;

use crate::model::{PathLayout, Scope};

/// 已注册的系统文件；顺序即错误提示与补全顺序。
///
/// 表而不是裸 `match`：未知名单、补全、将来的帮助都要同一份名单，
/// 各写一遍迟早会不一致。
const FILES: &[(&str, &str)] = &[
    ("config", "byk settings"),
    ("tap", "tap repositories"),
    ("shell", "shell startup file"),
];

pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(name, _)| *name)
}

/// 把 `sys@name` 换成注册表里的名字；未注册或只是 `sys@` 时返回 `None`。
pub(crate) fn parse_token(token: &str) -> Option<&'static str> {
    let (scope, stem) = Scope::split(token)?;
    if scope != Scope::Sys {
        return None;
    }
    FILES
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(name, _)| *name)
}

/// 某个系统文件的动词表。
///
/// 补全向这里要名单而不是另抄一份：动词是各文件自己定的，抄一份迟早会漏。
pub(crate) fn verbs(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "config" => config::VERBS,
        "tap" => tap::VERBS,
        "shell" => shell::VERBS,
        _ => &[],
    }
}

/// 补全说明：系统文件名本身（`sys@config` → `byk settings`）。
pub(crate) fn describe_file(token: &str) -> Option<&'static str> {
    let name = parse_token(token)?;
    FILES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, description)| *description)
}

/// 补全说明：`name` 文件下的候选（动词、设置名）。
///
/// 说明归各文件自己管：`config` 与 `tap` 都有 `list`，含义并不相同，
/// 一张全局表查不出正确答案。
pub(crate) fn describe(name: &str, candidate: &str) -> Option<&'static str> {
    match name {
        "config" => config::describe(candidate),
        "tap" => tap::describe(candidate),
        "shell" => shell::describe(candidate),
        _ => None,
    }
}

/// 分发一次系统文件调用；返回的错误已是可以直接打印的完整文案。
pub(crate) fn run(stem: &str, args: &[String], layout: &PathLayout) -> Result<(), String> {
    match resolve_name(stem)? {
        "config" => config::run(args, layout),
        "tap" => tap::run(args, layout),
        "shell" => shell::run(args, layout),
        other => unreachable!("system file {other:?} is registered but not dispatched"),
    }
}

/// 把 `sys@` 后面那段文字换成注册表里的文件身份。
fn resolve_name(stem: &str) -> Result<&'static str, String> {
    if stem.is_empty() {
        return Err(format!(
            "Missing system file name\nAvailable: {}",
            available()
        ));
    }
    // 点号在这里是明确的误用信号：`sys@config.set` 是想写 `sys@config set ...`，
    // 不能掉进别名查找，否则用户等到的是 "alias not found"。
    if stem.contains('.') {
        return Err(format!(
            "Invalid system file: {:?}\nHint: system files take arguments after a space, e.g. `byk sys@config banner never`",
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
        .map(|name| format!("{}{}", Scope::Sys.token(), name))
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
        assert!(err.contains("sys@config"), "{err}");
    }

    #[test]
    fn rejects_unknown_name_with_available_list() {
        let err = resolve_name("confg").unwrap_err();
        assert!(err.starts_with("Unknown system file: \"confg\""), "{err}");
        assert!(err.contains("Available: sys@config"), "{err}");
    }

    #[test]
    fn dotted_name_gets_a_rewrite_hint() {
        let err = resolve_name("config.set").unwrap_err();
        assert!(
            err.starts_with("Invalid system file: \"config.set\""),
            "{err}"
        );
        assert!(err.contains("byk sys@config banner never"), "{err}");
    }
}
