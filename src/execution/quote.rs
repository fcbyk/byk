//! 参数保真：把用户给的参数拼进命令模板时，按目标 shell 的规则包裹。
//!
//! 别名的尾随参数最终要交给 `sh -c`（Windows 上是 `cmd /C`）解释。直接
//! `join(" ")` 等于把参数边界交还给 shell：带空格的值被切成两个词、空串
//! 直接消失、`;` 变成命令分隔符。在插入点逐值包裹，边界就落在引号里。

/// 无需包裹即可安全通过 sh 的 ASCII 字节。
///
/// 刻意收紧：`~` `*` `?` `[` `$` `#` `!` 都不在内——它们要么会被 shell 展开，
/// 要么在词首有特殊含义，而参数是「用户已经让 shell 处理过一次的成品」，
/// 不该再被展开第二遍。
#[cfg(not(windows))]
fn is_plain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_' | b'.' | b'/' | b':' | b'=' | b'@' | b'+' | b','
        )
}

/// 单引号包裹（POSIX）。
///
/// 单引号内没有任何转义，唯一的麻烦是值自身带的 `'`：惯例上先收尾、写出
/// 转义后的一个 `'`、再重新开引号（`'\''`），因此结果总能还原成原值。
#[cfg(not(windows))]
pub(crate) fn quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.bytes().all(is_plain_byte) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// 双引号包裹（cmd）：引号内部用 `""` 表示一个字面引号。
///
/// Windows 侧做不到 100% 等价：`%VAR%` 在引号里照样被 cmd 展开，而命令行上
/// 没有可靠的转义写法（`%%` 只在批处理文件里有效）。除此之外空格、`&`、
/// `|`、`(` 都被引号挡住，比裸拼好得多。
#[cfg(windows)]
pub(crate) fn quote(value: &str) -> String {
    fn is_plain_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'/' | b':' | b'\\' | b'=' | b'@' | b'+'
            )
    }

    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value.bytes().all(is_plain_byte) {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// 把一段参数逐个包裹后拼起来；空切片得到空串。
pub(crate) fn join(values: &[String]) -> String {
    values
        .iter()
        .map(|v| quote(v))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 折叠引号外的连续空白，并去掉首尾空白。
///
/// 模板里取不到值的占位符会原地留下缺口（`cp ${0} ${9}` → `cp src `），
/// 这里把缺口收成一个空格。引号内原样保留——参数里的连续空白、制表符、
/// 换行都是用户的原文，不能碰。
pub(crate) fn normalize_spaces(command: &str) -> String {
    #[derive(PartialEq)]
    enum Mode {
        Plain,
        Single,
        Double,
    }

    let mut out = String::with_capacity(command.len());
    let mut mode = Mode::Plain;
    let mut pending_space = false;

    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match mode {
            Mode::Single => {
                out.push(c);
                if c == '\'' {
                    mode = Mode::Plain;
                }
            }
            Mode::Double => {
                out.push(c);
                match c {
                    '"' => mode = Mode::Plain,
                    // 双引号里反斜杠仍会转义下一个字符
                    '\\' => {
                        if let Some(next) = chars.next() {
                            out.push(next);
                        }
                    }
                    _ => {}
                }
            }
            Mode::Plain => match c {
                '\'' => {
                    flush_space(&mut out, &mut pending_space);
                    mode = Mode::Single;
                    out.push(c);
                }
                '"' => {
                    flush_space(&mut out, &mut pending_space);
                    mode = Mode::Double;
                    out.push(c);
                }
                '\\' => {
                    flush_space(&mut out, &mut pending_space);
                    out.push(c);
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                c if c.is_whitespace() => pending_space = true,
                c => {
                    flush_space(&mut out, &mut pending_space);
                    out.push(c);
                }
            },
        }
    }

    out
}

/// 在写下一个可见字符前补上被折叠的空格；串首不补。
fn flush_space(out: &mut String, pending: &mut bool) {
    if *pending && !out.is_empty() {
        out.push(' ');
    }
    *pending = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn plain_values_stay_bare() {
        assert_eq!(quote("hello"), "hello");
        assert_eq!(quote("--verbose"), "--verbose");
        assert_eq!(quote("./target/debug/byk"), "./target/debug/byk");
    }

    #[cfg(not(windows))]
    #[test]
    fn space_is_wrapped() {
        assert_eq!(quote("my file.json"), "'my file.json'");
    }

    #[cfg(not(windows))]
    #[test]
    fn empty_value_is_kept() {
        assert_eq!(quote(""), "''");
    }

    #[cfg(not(windows))]
    #[test]
    fn metacharacters_are_wrapped() {
        assert_eq!(quote("a; echo INJECTED"), "'a; echo INJECTED'");
        assert_eq!(quote("a|b"), "'a|b'");
        assert_eq!(quote("$(whoami)"), "'$(whoami)'");
    }

    #[cfg(not(windows))]
    #[test]
    fn quote_inside_value_is_escaped() {
        assert_eq!(quote("it's"), r#"'it'\''s'"#);
    }

    #[cfg(not(windows))]
    #[test]
    fn glob_and_backslash_stay_literal() {
        assert_eq!(quote("*.json"), "'*.json'");
        assert_eq!(quote(r"C:\tmp\x"), r"'C:\tmp\x'");
        assert_eq!(quote("~/x"), "'~/x'");
    }

    #[test]
    fn join_wraps_each_value() {
        let values = vec!["a b".to_string(), "c".to_string()];
        let expected = if cfg!(windows) {
            "\"a b\" c"
        } else {
            "'a b' c"
        };
        assert_eq!(join(&values), expected);
    }

    #[test]
    fn join_empty_is_empty() {
        assert_eq!(join(&[]), "");
    }

    #[test]
    fn normalize_collapses_plain_whitespace() {
        assert_eq!(normalize_spaces("echo   hello"), "echo hello");
        assert_eq!(normalize_spaces("  echo hello  "), "echo hello");
        assert_eq!(normalize_spaces("echo\t\thello\n"), "echo hello");
    }

    #[test]
    fn normalize_keeps_quoted_whitespace() {
        assert_eq!(normalize_spaces("echo 'a  b'"), "echo 'a  b'");
        assert_eq!(normalize_spaces("echo \"a  b\""), "echo \"a  b\"");
    }

    #[test]
    fn normalize_handles_escaped_quote() {
        assert_eq!(normalize_spaces(r"echo 'a'\''b  c'"), r"echo 'a'\''b  c'");
    }

    #[test]
    fn normalize_empty() {
        assert_eq!(normalize_spaces(""), "");
        assert_eq!(normalize_spaces("   "), "");
    }
}
