//! `@@@config`：byk 的设置文件，落在 `~/.byk/config.json`。
//!
//! 语法和别名文件一致——`文件 动词 参数`。不用 `@@@config banner never` 那种位置式，
//! 是为了和 `@@ops` 保持同一个节奏，也给将来的 `edit` / `reset` 留位置。

mod keys;

use std::io::IsTerminal;
use std::path::Path;

use colored::Colorize;
use serde_json::{Map, Value};

use super::store;

use crate::model::PathLayout;
use crate::utils::display::get_display_width;
use crate::utils::io::write_stdout;

/// 帮助页顶部装饰框对应的键名。
const BANNER: &str = "banner";

/// 访问 GitHub 文件时走的通道。
const NET_GITHUB: &str = "net-github";

/// 当前配置的 GitHub 通道。
///
/// 下载路径上每一处远程请求都会问一次，所以这里不打印告警——设置文件坏了由
/// `@@@config` 自己提示，下载不该被一行警告打断。
pub(crate) fn channel(layout: &PathLayout) -> crate::net::Channel {
    let path = layout.config_path();
    let (object, warning) = store::read_object(&path);
    if warning.is_some() {
        return crate::net::Channel::default();
    }

    let Some(info) = keys::find(NET_GITHUB) else {
        return crate::net::Channel::default();
    };
    let (value, _) = effective(info, &object);
    crate::net::Channel::parse(&value)
}

/// 已注册的动词；顺序即帮助与错误提示顺序。
pub(crate) const VERBS: &[(&str, &str)] = &[
    ("list", "Show every setting"),
    ("get", "Print one value"),
    ("set", "Change one value"),
    ("unset", "Reset one value to its default"),
];

/// 补全用：设置名。
pub(crate) fn setting_names() -> Vec<&'static str> {
    keys::names()
}

/// 补全用：某个设置项的候选值；未知键没有候选。
pub(crate) fn setting_choices(key: &str) -> &'static [&'static str] {
    keys::find(key).map(|info| info.choices).unwrap_or(&[])
}

/// 补全说明：本文件的动词或设置名。
pub(crate) fn describe(candidate: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|(name, _)| *name == candidate)
        .map(|(_, description)| *description)
        .or_else(|| keys::find(candidate).map(|info| info.description))
}

const USAGE: &str = "byk @@@config [OPTIONS] [COMMAND] [ARGS]...";

pub(crate) fn run(args: &[String], layout: &PathLayout) -> Result<(), String> {
    let path = layout.config_path();

    let Some((verb, rest)) = args.split_first() else {
        // 裸调用看设置：对设置文件来说「给我看全部」才是高频动作，
        // 「我还能做什么」交给 `-h`。
        return print_table(&path);
    };

    match verb.as_str() {
        "-h" | "--help" => write_stdout(render_help(&path).as_bytes()),
        "list" => {
            expect_count(rest, 0, "list takes no arguments")?;
            print_table(&path)
        }
        "get" => {
            let key = single_argument(rest, "get takes one setting name")?;
            let value = read_value(&path, key)?;
            write_stdout(format!("{}\n", value).as_bytes())
        }
        "set" => {
            if rest.len() != 2 {
                return Err("set takes a setting name and a value".to_string());
            }
            let (key, value) = set_value(&path, &rest[0], &rest[1])?;
            eprintln!("{} = {}", key, value);
            Ok(())
        }
        "unset" => {
            let key = single_argument(rest, "unset takes one setting name")?;
            let key = unset_value(&path, key)?;
            eprintln!("Unset {}", key);
            Ok(())
        }
        other => Err(unknown_command(other)),
    }
}

/// 帮助页顶部的装饰框现在要不要画。
///
/// `auto` 的判据是 stdout 是否终端：`byk -h | head`、脚本里取帮助、编辑器插件调用，
/// 都不该被一个框打扰。诊断框（有文件出问题）不归这里管——那是提示不是装饰，
/// 由调用方保证照常打印。
pub(crate) fn banner_enabled(layout: &PathLayout) -> bool {
    let info = keys::find(BANNER).expect("banner is registered in keys.rs");
    let (object, warning) = store::read_object(&layout.config_path());
    print_warning(warning);

    let (value, warning) = effective(info, &object);
    print_warning(warning);

    allowed(&value, std::io::stdout().is_terminal())
}

/// 三态判定。只有 `auto` 看终端；`effective` 已经保证不会是别的值。
fn allowed(value: &str, is_terminal: bool) -> bool {
    match value {
        "always" => true,
        "never" => false,
        _ => is_terminal,
    }
}

fn print_table(path: &Path) -> Result<(), String> {
    let (object, warning) = store::read_object(path);
    print_warning(warning);

    let (rows, warnings) = rows(&object);
    for warning in warnings {
        print_warning(Some(warning));
    }

    write_stdout(render_table(&rows, path).as_bytes())
}

/// 读一个键的有效值：文件里有就用文件里的，否则用默认值。
fn read_value(path: &Path, key: &str) -> Result<String, String> {
    let info = keys::find(key).ok_or_else(|| unknown_setting(key))?;

    let (object, warning) = store::read_object(path);
    print_warning(warning);

    let (value, warning) = effective(info, &object);
    print_warning(warning);
    Ok(value)
}

/// 校验并写入一个键；返回规范化后的键名与值。
fn set_value(path: &Path, key: &str, value: &str) -> Result<(String, String), String> {
    let info = keys::find(key).ok_or_else(|| unknown_setting(key))?;
    if !info.accepts(value) {
        return Err(format!(
            "Invalid value for {}: {:?}\nExpected: {}",
            info.key,
            value,
            info.choices.join(", ")
        ));
    }

    let (mut object, warning) = store::read_object(path);
    print_warning(warning);
    object.insert(info.key.to_string(), Value::String(value.to_string()));
    store::write_object(path, &object)?;

    Ok((info.key.to_string(), value.to_string()))
}

/// 删掉一个键，回到默认值；没设过就不动磁盘。
fn unset_value(path: &Path, key: &str) -> Result<String, String> {
    let info = keys::find(key).ok_or_else(|| unknown_setting(key))?;

    let (mut object, warning) = store::read_object(path);
    print_warning(warning);
    if object.remove(info.key).is_some() {
        store::write_object(path, &object)?;
    }

    Ok(info.key.to_string())
}

/// 表格里的一行。
struct Row {
    key: &'static str,
    value: String,
    /// 值来自默认值而不是文件；表格里标 `(default)`。
    is_default: bool,
    description: &'static str,
}

impl Row {
    /// 补白用的纯文本标签：`value` 加可能存在的标记。
    fn label(&self) -> String {
        if self.is_default {
            format!("{} (default)", self.value)
        } else {
            self.value.clone()
        }
    }
}

/// 按注册表算每一行的有效值；顺便收集「文件里值不合法」的警告。
fn rows(object: &Map<String, Value>) -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::with_capacity(keys::SETTINGS.len());
    let mut warnings = Vec::new();

    for info in keys::SETTINGS {
        let (value, warning) = effective(info, object);
        if let Some(warning) = warning {
            warnings.push(warning);
        }
        // 文件里有这个键、且值合法，才算「用户设过」
        let is_default = !matches!(
            object.get(info.key),
            Some(Value::String(text)) if info.accepts(text)
        );
        rows.push(Row {
            key: info.key,
            value,
            is_default,
            description: info.description,
        });
    }

    (rows, warnings)
}

/// 文件里的值优先；不是字符串或不在候选内时退回默认值，并给一行警告。
fn effective(info: &keys::SettingInfo, object: &Map<String, Value>) -> (String, Option<String>) {
    match object.get(info.key) {
        None => (info.default.to_string(), None),
        Some(Value::String(text)) if info.accepts(text) => (text.clone(), None),
        Some(other) => (
            info.default.to_string(),
            Some(format!(
                "Warning: invalid value for {}: {}; using default ({})",
                info.key, other, info.default
            )),
        ),
    }
}

fn render_table(rows: &[Row], path: &Path) -> String {
    let key_width = rows
        .iter()
        .map(|row| get_display_width(row.key))
        .max()
        .unwrap_or(0);
    let label_width = rows
        .iter()
        .map(|row| get_display_width(&row.label()))
        .max()
        .unwrap_or(0);

    let mut output = String::from("\n");
    output.push_str(&format!(
        "{} {}\n\n",
        "FilePath:".green().bold(),
        path.display()
    ));

    for row in rows {
        let key_padding = " ".repeat(key_width - get_display_width(row.key));
        let label_padding = " ".repeat(label_width - get_display_width(&row.label()));
        let shown = if row.is_default {
            format!("{}{}", row.value, " (default)".dimmed())
        } else {
            row.value.clone()
        };
        output.push_str(&format!(
            "  {}{}  {}{}  {}\n",
            row.key.cyan().bold(),
            key_padding,
            shown,
            label_padding,
            row.description
        ));
    }

    output
}

fn render_help(path: &Path) -> String {
    let commands: Vec<(String, &'static str)> = VERBS
        .iter()
        .map(|(name, description)| ((*name).to_string(), *description))
        .collect();

    let mut output = String::from("\n");
    output.push_str(&format!(
        "{} {}\n",
        "FilePath:".green().bold(),
        path.display()
    ));
    output.push_str(&format!("{} {}\n\n", "Usage:".green().bold(), USAGE));
    output.push_str(&render_section(
        "Options:",
        &[("--help, -h".to_string(), "Show this help")],
    ));
    output.push('\n');
    output.push_str(&render_section("Commands:", &commands));
    output.push('\n');
    output
}

fn render_section(title: &str, entries: &[(String, &'static str)]) -> String {
    let width = entries
        .iter()
        .map(|(name, _)| get_display_width(name))
        .max()
        .unwrap_or(0);

    let mut output = format!("{}\n", title.green().bold());
    for (name, description) in entries {
        let padding = " ".repeat(width - get_display_width(name));
        output.push_str(&format!(
            "  {}{}  {}\n",
            name.cyan().bold(),
            padding,
            description
        ));
    }
    output
}

fn unknown_command(command: &str) -> String {
    format!(
        "Unknown command: {:?}\nAvailable: {}",
        command,
        VERBS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn unknown_setting(key: &str) -> String {
    format!(
        "Unknown setting: {:?}\nAvailable: {}",
        key,
        keys::names().join(", ")
    )
}

fn single_argument<'a>(args: &'a [String], message: &str) -> Result<&'a str, String> {
    expect_count(args, 1, message)?;
    Ok(&args[0])
}

fn expect_count(args: &[String], expected: usize, message: &str) -> Result<(), String> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

fn print_warning(warning: Option<String>) {
    if let Some(warning) = warning {
        eprintln!("{}", warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn path() -> PathBuf {
        PathBuf::from("/root/.byk/config.json")
    }

    /// 去除 ANSI 转义码，方便断言。
    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c == '\x1b' {
                chars.next();
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&inner) = chars.peek() {
                        chars.next();
                        if inner.is_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(c);
                chars.next();
            }
        }
        result
    }

    #[test]
    fn table_marks_default_values() {
        let (rows, warnings) = rows(&object(json!({})));
        assert!(warnings.is_empty());

        let text = strip_ansi(&render_table(&rows, &path()));
        assert!(
            text.contains("  banner      auto (default)    Show the banner in help output"),
            "{text}"
        );
        assert!(
            text.contains("  net-github  direct (default)  How byk reaches GitHub files"),
            "{text}"
        );
    }

    #[test]
    fn table_shows_configured_values_without_the_marker() {
        let (rows, warnings) = rows(&object(json!({"banner": "never", "net-github": "ghproxy"})));
        assert!(warnings.is_empty());

        let text = strip_ansi(&render_table(&rows, &path()));
        assert!(text.contains("banner") && text.contains("never"), "{text}");
        assert!(
            text.contains("net-github") && text.contains("ghproxy"),
            "{text}"
        );
        assert!(!text.contains("(default)"), "{text}");
    }

    #[test]
    fn invalid_stored_value_falls_back_and_warns() {
        let (rows, warnings) = rows(&object(json!({"banner": "nvr"})));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("Warning: invalid value for banner: \"nvr\""));
        assert!(warnings[0].contains("using default (auto)"));

        // 值是坏的，但表格仍要显示可用的值
        let text = strip_ansi(&render_table(&rows, &path()));
        assert!(
            text.contains("banner") && text.contains("auto (default)"),
            "{text}"
        );
    }

    #[test]
    fn non_string_value_is_treated_as_unset() {
        let (rows, warnings) = rows(&object(json!({"banner": false})));

        assert_eq!(warnings.len(), 1);
        assert!(rows[0].is_default);
    }

    #[test]
    fn help_lists_every_command() {
        let text = strip_ansi(&render_help(&path()));
        assert!(text.contains("Usage: byk @@@config [OPTIONS] [COMMAND] [ARGS]..."));
        assert!(text.contains("  list   Show every setting"));
        assert!(text.contains("  get    Print one value"));
        assert!(text.contains("  set    Change one value"));
        assert!(text.contains("  unset  Reset one value to its default"));
        assert!(text.contains("  --help, -h  Show this help"));
    }

    #[test]
    fn unknown_command_lists_verbs() {
        let message = unknown_command("delete");
        assert_eq!(
            message,
            "Unknown command: \"delete\"\nAvailable: list, get, set, unset"
        );
    }

    #[test]
    fn unknown_setting_lists_keys() {
        let message = unknown_setting("bannner");
        assert_eq!(
            message,
            "Unknown setting: \"bannner\"\nAvailable: banner, net-github"
        );
    }

    #[test]
    fn rejects_values_outside_the_registry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");

        let err = set_value(&path, "banner", "nvr").unwrap_err();

        assert_eq!(
            err,
            "Invalid value for banner: \"nvr\"\nExpected: auto, always, never"
        );
        // 校验失败不落盘，也不该顺手把目录建出来
        assert!(!path.exists());
    }

    #[test]
    fn channel_defaults_to_direct_and_reads_ghproxy() {
        let temp = tempfile::tempdir().unwrap();
        let layout = PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        };

        assert_eq!(channel(&layout), crate::net::Channel::Direct);

        set_value(&layout.config_path(), "net-github", "ghproxy").unwrap();
        assert_eq!(channel(&layout), crate::net::Channel::Ghproxy);

        // 值是坏的：退回默认值，下载不该被打断
        std::fs::write(layout.config_path(), r#"{"net-github": 42}"#).unwrap();
        assert_eq!(channel(&layout), crate::net::Channel::Direct);
    }

    #[test]
    fn set_then_get_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");

        set_value(&path, "banner", "never").unwrap();

        assert_eq!(read_value(&path, "banner").unwrap(), "never");
    }

    #[test]
    fn get_falls_back_to_the_default() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");

        assert_eq!(read_value(&path, "banner").unwrap(), "auto");
    }

    #[test]
    fn unset_restores_the_default() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");

        set_value(&path, "banner", "always").unwrap();
        unset_value(&path, "banner").unwrap();

        assert_eq!(read_value(&path, "banner").unwrap(), "auto");
    }

    #[test]
    fn unset_leaves_a_missing_file_alone() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".byk").join("config.json");

        unset_value(&path, "banner").unwrap();

        // 没设过就什么都不用做，别顺手把 CLI Home 建出来
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn set_keeps_unknown_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        std::fs::write(&path, r#"{"from":"a newer byk"}"#).unwrap();

        set_value(&path, "banner", "never").unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("a newer byk"), "{text}");
    }

    fn temp_layout(temp: &tempfile::TempDir) -> PathLayout {
        PathLayout {
            root_dir: temp.path().to_path_buf(),
            alias_dir: temp.path().join("alias"),
        }
    }

    #[test]
    fn banner_auto_follows_the_terminal() {
        assert!(allowed("auto", true));
        assert!(!allowed("auto", false));
        assert!(allowed("always", false));
        assert!(!allowed("never", true));
    }

    #[test]
    fn banner_setting_is_read_from_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = temp_layout(&temp);
        std::fs::create_dir_all(&layout.root_dir).unwrap();

        std::fs::write(layout.config_path(), r#"{"banner":"never"}"#).unwrap();
        assert!(!banner_enabled(&layout));

        std::fs::write(layout.config_path(), r#"{"banner":"always"}"#).unwrap();
        assert!(banner_enabled(&layout));
    }

    #[test]
    fn rejects_bad_arity_before_touching_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let layout = temp_layout(&temp);

        assert_eq!(
            run(&["list".to_string(), "extra".to_string()], &layout).unwrap_err(),
            "list takes no arguments"
        );
        assert_eq!(
            run(&["get".to_string()], &layout).unwrap_err(),
            "get takes one setting name"
        );
        assert_eq!(
            run(&["unset".to_string()], &layout).unwrap_err(),
            "unset takes one setting name"
        );
        assert_eq!(
            run(&["set".to_string(), "banner".to_string()], &layout).unwrap_err(),
            "set takes a setting name and a value"
        );
    }

    #[test]
    fn rejects_unknown_commands_and_settings() {
        let temp = tempfile::tempdir().unwrap();
        let layout = temp_layout(&temp);

        assert!(
            run(&["delete".to_string()], &layout)
                .unwrap_err()
                .starts_with("Unknown command: \"delete\"")
        );
        assert!(
            run(&["get".to_string(), "bannner".to_string()], &layout)
                .unwrap_err()
                .starts_with("Unknown setting: \"bannner\"")
        );
    }
}
