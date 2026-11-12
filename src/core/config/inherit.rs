//! 继承属性应用：把分组/文件级的 `$cwd` / `$mode` 填进未显式指定的别名。

use super::types::{AliasValue, Mode};

/// 对未指定 $cwd / $mode 的别名应用继承。
///
/// 三者都用 `or`：别名自己写了就以自己为准（包括 `Mode::Normal`，
/// 它表示「这条就要普通」，能覆盖文件级的 `interactive` / `silent`），
/// 只有 `None` 才落回继承值。
pub(crate) fn apply_inherited(
    av: AliasValue,
    inherited_cwd: Option<&str>,
    inherited_mode: Option<Mode>,
) -> AliasValue {
    match av {
        AliasValue::Meta {
            cmd,
            cwd,
            mode,
            description,
            recursive,
        } => AliasValue::Meta {
            cmd,
            // 别名自己的 $cwd 优先，缺省才落回继承值
            cwd: cwd.or_else(|| inherited_cwd.map(String::from)),
            mode: mode.or(inherited_mode),
            description, // $description / $recursive 不继承，直接透传
            recursive,
        },
        AliasValue::Str(cmd) => {
            // `Mode::Normal` 继承值就是默认态，没必要为此把纯字符串别名升级成 Meta
            if inherited_cwd.is_some() || inherited_mode.is_some_and(|m| m != Mode::Normal) {
                AliasValue::Meta {
                    cmd,
                    cwd: inherited_cwd.map(String::from),
                    mode: inherited_mode,
                    description: None,
                    recursive: None,
                }
            } else {
                AliasValue::Str(cmd)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherit_str_no_inherited_stays_str() {
        let av = AliasValue::Str("echo hi".into());
        let result = apply_inherited(av, None, None);
        assert!(matches!(result, AliasValue::Str(s) if s == "echo hi"));
    }

    #[test]
    fn inherit_str_with_normal_mode_stays_str() {
        // Normal 继承值就是默认态，不该把字符串别名无谓升级成 Meta
        let av = AliasValue::Str("build".into());
        let result = apply_inherited(av, None, Some(Mode::Normal));
        assert!(matches!(result, AliasValue::Str(s) if s == "build"));
    }

    #[test]
    fn inherit_str_with_cwd_becomes_meta() {
        let av = AliasValue::Str("build".into());
        let result = apply_inherited(av, Some("/app"), None);
        match result {
            AliasValue::Meta { cmd, cwd, mode, .. } => {
                assert_eq!(cmd, "build");
                assert_eq!(cwd, Some("/app".into()));
                assert_eq!(mode, None);
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_str_with_interactive_becomes_meta() {
        let av = AliasValue::Str("run".into());
        let result = apply_inherited(av, None, Some(Mode::Interactive));
        match result {
            AliasValue::Meta { cmd, mode, .. } => {
                assert_eq!(cmd, "run");
                assert_eq!(mode, Some(Mode::Interactive));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_str_with_silent_becomes_meta() {
        let av = AliasValue::Str("build".into());
        let result = apply_inherited(av, None, Some(Mode::Silent));
        match result {
            AliasValue::Meta { cmd, mode, .. } => {
                assert_eq!(cmd, "build");
                assert_eq!(mode, Some(Mode::Silent));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_keeps_own_cwd() {
        let av = AliasValue::Meta {
            cmd: "test".into(),
            cwd: Some("/own".into()),
            mode: None,
            description: None,
            recursive: None,
        };
        let result = apply_inherited(av, Some("/inherit"), None);
        match result {
            AliasValue::Meta { cwd, .. } => {
                assert_eq!(cwd, Some("/own".into())); // 自己的优先
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_fills_missing_cwd() {
        let av = AliasValue::Meta {
            cmd: "test".into(),
            cwd: None,
            mode: None,
            description: None,
            recursive: None,
        };
        let result = apply_inherited(av, Some("/inherit"), None);
        match result {
            AliasValue::Meta { cwd, .. } => {
                assert_eq!(cwd, Some("/inherit".into()));
            }
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_fills_missing_mode() {
        let av = AliasValue::Meta {
            cmd: "ask".into(),
            cwd: None,
            mode: None,
            description: None,
            recursive: None,
        };
        let result = apply_inherited(av, None, Some(Mode::Interactive));
        match result {
            AliasValue::Meta { mode, .. } => assert_eq!(mode, Some(Mode::Interactive)),
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_own_mode_wins() {
        // 文件级交互，别名写 silent：一个字就够，不必再关掉另一个开关
        let av = AliasValue::Meta {
            cmd: "backup".into(),
            cwd: None,
            mode: Some(Mode::Silent),
            description: None,
            recursive: None,
        };
        let result = apply_inherited(av, None, Some(Mode::Interactive));
        match result {
            AliasValue::Meta { mode, .. } => assert_eq!(mode, Some(Mode::Silent)),
            _ => panic!("Expected Meta"),
        }
    }

    #[test]
    fn inherit_meta_normal_overrides_inherited_mode() {
        // 文件级静默，别名写 normal 就该恢复提示
        let av = AliasValue::Meta {
            cmd: "deploy".into(),
            cwd: None,
            mode: Some(Mode::Normal),
            description: None,
            recursive: None,
        };
        let result = apply_inherited(av, None, Some(Mode::Silent));
        match result {
            AliasValue::Meta { mode, .. } => assert_eq!(mode, Some(Mode::Normal)),
            _ => panic!("Expected Meta"),
        }
    }
}
