//! `deps` 的 verb/选项解析。

use crate::filecmd::spec::subcommand_names;

#[derive(Debug)]
pub(super) enum Action {
    Install { force: bool },
    Uninstall { yes: bool },
    Status,
}

pub(super) fn parse_args(args: &[String]) -> Result<Action, String> {
    let Some((verb, rest)) = args.split_first() else {
        return Ok(Action::Status);
    };

    let candidates: Vec<&str> = subcommand_names()
        .filter(|name| name.starts_with(verb.as_str()))
        .collect();
    let name = match candidates.as_slice() {
        [name] => *name,
        [] => {
            return Err(format!(
                "Unknown deps subcommand: {:?}\nAvailable: {}",
                verb,
                subcommand_names().collect::<Vec<_>>().join(", ")
            ));
        }
        _ => {
            return Err(format!(
                "Ambiguous deps subcommand: {:?}\nAvailable: {}",
                verb,
                candidates.join(", ")
            ));
        }
    };

    match name {
        // 安装不做二次确认：`--yes` 已经没有意义，直接说不支持比静默忽略好。
        "install" => match rest {
            [] => Ok(Action::Install { force: false }),
            [flag] if flag == "--force" => Ok(Action::Install { force: true }),
            [flag, ..] => Err(format!(
                "Unknown option: {}\nHint: deps install runs without confirmation; the only option is --force",
                flag
            )),
        },
        "uninstall" => match rest {
            [] => Ok(Action::Uninstall { yes: false }),
            [flag] if flag == "--yes" => Ok(Action::Uninstall { yes: true }),
            _ => Err("deps uninstall accepts only --yes".to_string()),
        },
        _ => match rest {
            [] => Ok(Action::Status),
            _ => Err("deps status takes no options".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn action(values: &[&str]) -> Action {
        parse_args(&args(values)).unwrap()
    }

    #[test]
    fn parse_args_accepts_full_and_short_verbs() {
        assert!(matches!(
            action(&["install"]),
            Action::Install { force: false }
        ));
        assert!(matches!(action(&["i"]), Action::Install { force: false }));
        assert!(matches!(action(&["in"]), Action::Install { force: false }));
        assert!(matches!(action(&["u"]), Action::Uninstall { yes: false }));
        assert!(matches!(action(&["uni"]), Action::Uninstall { yes: false }));
        assert!(matches!(
            action(&["uninstall", "--yes"]),
            Action::Uninstall { yes: true }
        ));
        assert!(matches!(action(&["s"]), Action::Status));
        assert!(matches!(
            action(&["i", "--force"]),
            Action::Install { force: true }
        ));
    }

    #[test]
    fn bare_deps_means_status() {
        assert!(matches!(action(&[]), Action::Status));
        assert!(matches!(action(&["status"]), Action::Status));
    }

    #[test]
    fn parse_args_rejects_unknown_extra_and_status_options() {
        assert!(
            parse_args(&args(&["upgrade"]))
                .unwrap_err()
                .contains("Unknown deps subcommand")
        );

        // 安装不再确认，`--yes` 就是非法选项
        assert_eq!(
            parse_args(&args(&["install", "--yes"])).unwrap_err(),
            "Unknown option: --yes\nHint: deps install runs without confirmation; the only option is --force"
        );

        assert_eq!(
            parse_args(&args(&["uninstall", "--yes", "x"])).unwrap_err(),
            "deps uninstall accepts only --yes"
        );

        assert_eq!(
            parse_args(&args(&["status", "--yes"])).unwrap_err(),
            "deps status takes no options"
        );
    }
}
