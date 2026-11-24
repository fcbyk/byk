//! `@@@shell`：当前 shell 的启动文件（`~/.zshrc` 之类）。
//!
//! 目标按父进程与 `$SHELL` 探测，并且只改它——没有 `switch`，也不接受
//! `add completion bash` 这样的指名：给 bash 装补全，就在 bash 里跑 byk。

mod detect;
mod rc;

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use colored::Colorize;

use detect::{Detected, Detection, Shell, Source};
use rc::State;

use crate::model::PathLayout;
use crate::utils::display::get_display_width;
use crate::utils::io::write_stdout;

/// 已注册的动词；顺序即帮助与错误提示顺序。
pub(crate) const VERBS: &[(&str, &str)] = &[
    ("add", "Enable shell completion"),
    ("remove", "Disable shell completion"),
];

/// `add` / `remove` 的对象。v1 只有一个，但位置留着：`path` 迟早要来。
const ITEMS: &[(&str, &str)] = &[("completion", "Shell completion")];

// shell 由环境决定，多写一个词就会被拒绝，所以用法行照实说。
const USAGE: &str = "byk @@@shell [OPTIONS] [COMMAND] [ITEM]";

pub(crate) fn run(args: &[String], _layout: &PathLayout) -> Result<(), String> {
    let Some((verb, rest)) = args.split_first() else {
        // 裸调用 = `-h`：这一域只有两条动词，单开一屏状态反而多敲一次
        return help();
    };

    match verb.as_str() {
        "-h" | "--help" => help(),
        "add" => modify(rest, Action::Add),
        "remove" => modify(rest, Action::Remove),
        other => Err(unknown_command(other)),
    }
}

/// 一屏说清：哪个 shell、改哪个文件、装没装，以及能做什么。没有文件可改时只有状态几行。
fn help() -> Result<(), String> {
    write_stdout(render_screen().as_bytes())
}

fn render_screen() -> String {
    match status_block() {
        Ok(screen) => screen.render(),
        // 读不了不等于没有：用法照给，但把原因写在头顶，不能装作读过了
        Err(error) => render_help(&format!(
            "\n{}\n",
            format!("Status unavailable: {error}").dimmed()
        )),
    }
}

pub(crate) fn items() -> Vec<&'static str> {
    ITEMS.iter().map(|(name, _)| *name).collect()
}

pub(crate) fn describe(candidate: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|(name, _)| *name == candidate)
        .map(|(_, description)| *description)
        .or_else(|| {
            ITEMS
                .iter()
                .find(|(name, _)| *name == candidate)
                .map(|(_, description)| *description)
        })
}

#[derive(Clone, Copy)]
enum Action {
    Add,
    Remove,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Add => "add",
            Action::Remove => "remove",
        }
    }
}

/// 状态那几行，外加一件事：这一屏该不该接用法。
enum Screen {
    /// 有文件可改：状态之后还有 `add` / `remove` 可用。
    Modifiable(String),
    /// 没有文件：`add` / `remove` 都做不成，用法到此为止。
    NoFile(String),
}

impl Screen {
    fn render(self) -> String {
        match self {
            Screen::Modifiable(status) => render_help(&status),
            Screen::NoFile(status) => status,
        }
    }
}

fn status_block() -> Result<Screen, String> {
    match target()? {
        Target::File { path, detected } => {
            let content = rc::read(&path)?;
            let state = content
                .as_deref()
                .map(rc::inspect)
                .unwrap_or(State::Missing);
            Ok(Screen::Modifiable(render_status(&path, detected, &state)))
        }
        Target::NoFile(missing) => Ok(Screen::NoFile(render_no_file(&missing))),
    }
}

/// 一次 `add` / `remove`。目标 shell 一律是运行 byk 的那个。
fn modify(args: &[String], action: Action) -> Result<(), String> {
    let Some((item, rest)) = args.split_first() else {
        return Err(format!("Missing item\nAvailable: {}", items().join(", ")));
    };
    // 前缀也要先校验：拼错了就不该去探测环境、读文件
    resolve_item(item)?;
    if let Some(extra) = rest.first() {
        return Err(no_shell_argument(action, extra));
    }
    let (path, shell) = match target()? {
        Target::File { path, detected } => (path, detected.shell),
        // 状态里这只是一句说明；真去改却什么都没改，才算失败
        Target::NoFile(missing) => {
            return Err(format!("{}\n{}", missing.headline(), missing.advice()));
        }
    };

    let current = rc::read(&path)?;

    match action {
        Action::Add => {
            let text = current.as_deref().unwrap_or_default();
            let Some(updated) = rc::add(text, shell.name()) else {
                let state = rc::inspect(text);
                eprintln!(
                    "Shell completion is already enabled in {} ({})",
                    path.display(),
                    state.location().unwrap_or_default()
                );
                if state.is_outdated() {
                    eprintln!(
                        "Hint: `byk @@@shell remove completion` then `add` upgrades it to a marked block"
                    );
                }
                return Ok(());
            };

            let created = current.is_none();
            rc::write(&path, &updated)?;
            if created {
                eprintln!("Created {}", path.display());
            }
            eprintln!("Enabled {} completion in {}", shell.name(), path.display());
            eprintln!(
                "Run `source {}` or restart your shell to pick it up",
                path.display()
            );
            Ok(())
        }
        Action::Remove => {
            let Some(updated) = current.as_deref().and_then(rc::remove) else {
                eprintln!("Shell completion is not enabled in {}", path.display());
                return Ok(());
            };

            rc::write(&path, &updated)?;
            eprintln!("Disabled {} completion in {}", shell.name(), path.display());
            Ok(())
        }
    }
}

/// 这次要改的东西：一个启动文件，或者「没有文件」——后者是正常结果，不是错误。
enum Target {
    File { path: PathBuf, detected: Detected },
    NoFile(NoFile),
}

/// 为什么没有文件可改。
enum NoFile {
    /// 认出了 shell，但它不在补全生成器的覆盖范围里。
    Unsupported { name: String, source: Source },
    /// 连跑的是哪个 shell 都不知道（Windows 的 cmd / PowerShell，受限环境）。
    Unknown,
}

impl NoFile {
    /// 「没有文件」那一句。状态里不出现，`add` / `remove` 用它开头。
    fn headline(&self) -> String {
        match self {
            NoFile::Unsupported { name, source } => {
                format!("No startup file for {name} ({})", source.label())
            }
            NoFile::Unknown => "No startup file: cannot tell which shell is running".to_string(),
        }
    }

    /// 为什么没有、或者该怎么办。状态里单独一行，报错里是第二行。
    fn advice(&self) -> &'static str {
        match self {
            NoFile::Unsupported { .. } => {
                "Only zsh, bash and fish have a startup file byk can edit"
            }
            NoFile::Unknown => "Only zsh, bash and fish are supported; run byk from one of them",
        }
    }

    /// 状态表里的 `shell` 行。
    fn shell_column(&self) -> String {
        match self {
            NoFile::Unsupported { name, source } => format!("{name} ({})", source.label()),
            NoFile::Unknown => "unknown".to_string(),
        }
    }
}

/// 当前 shell 的启动文件；没有就说清楚为什么没有。
fn target() -> Result<Target, String> {
    match detect::detect() {
        Detection::Found(found) => {
            let home =
                dirs::home_dir().ok_or_else(|| "Cannot find your home directory".to_string())?;
            let zdotdir = non_empty_var("ZDOTDIR").map(PathBuf::from);
            let xdg_config_home = non_empty_var("XDG_CONFIG_HOME").map(PathBuf::from);
            let path = found
                .shell
                .rc_path(&home, zdotdir.as_deref(), xdg_config_home.as_deref());

            Ok(Target::File {
                path,
                detected: found,
            })
        }
        Detection::Unsupported { name, source } => {
            Ok(Target::NoFile(NoFile::Unsupported { name, source }))
        }
        Detection::Unknown => Ok(Target::NoFile(NoFile::Unknown)),
    }
}

/// 空字符串等同于没设：`ZDOTDIR=` 展开出来的路径不能是相对目录。
fn non_empty_var(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn render_status(path: &Path, detected: Detected, state: &State) -> String {
    let mut fields = vec![
        (
            "Shell:",
            format!("{} ({})", detected.shell.name(), detected.source.label()),
            Tone::Plain,
        ),
        ("FilePath:", path.display().to_string(), Tone::Plain),
    ];

    // dotfiles 里 `.zshrc` 常是链接；改之前先让用户看见真正落到哪儿
    if let Some(target) = rc::link_target(path) {
        fields.push(("Target:", target.display().to_string(), Tone::Plain));
    }

    let tone = if *state == State::Missing {
        Tone::Warn
    } else {
        Tone::Good
    };
    fields.push(("Completion:", state.label(), tone));

    // 不再补一句「用 add 装上」：紧跟着的 `Commands:` 就是答案
    format!("\n{}", render_fields(&fields))
}

/// cmd / PowerShell / `sh` 之类：只有「没有文件」和为什么，上下各留一个空行。
fn render_no_file(missing: &NoFile) -> String {
    let fields = [
        ("Shell:", missing.shell_column(), Tone::Plain),
        ("FilePath:", "(none)".to_string(), Tone::Plain),
        ("Completion:", "not available".to_string(), Tone::Warn),
    ];

    // 原因紧贴字段，不另起一段：它说的是 `Completion: not available` 这一行
    format!(
        "\n{}{}\n\n",
        render_fields(&fields),
        missing.advice().dimmed()
    )
}

#[derive(Clone, Copy)]
enum Tone {
    Plain,
    /// 已装、可用
    Good,
    /// 没装，或者压根用不了
    Warn,
}

/// 同一级的一组「标签: 值」。不把取值补齐到同一列——路径长短差得远，补出来的空白比信息还多。
fn render_fields(fields: &[(&'static str, String, Tone)]) -> String {
    let mut output = String::new();
    for (label, value, tone) in fields {
        let shown = match tone {
            Tone::Plain => value.clone(),
            Tone::Good => value.green().to_string(),
            Tone::Warn => value.yellow().to_string(),
        };
        output.push_str(&format!("{} {}\n", label.green().bold(), shown));
    }

    output
}

/// `status` 是头顶那几行；空串表示没读出来，只有用法。
fn render_help(status: &str) -> String {
    let commands: Vec<(String, &'static str)> = VERBS
        .iter()
        .map(|(name, description)| ((*name).to_string(), *description))
        .collect();
    let items: Vec<(String, &'static str)> = ITEMS
        .iter()
        .map(|(name, description)| ((*name).to_string(), *description))
        .collect();

    // 状态块自己以换行结尾，这个空行就是它与 `Usage:` 的分隔；读不出来时也以空行开头
    let mut output = String::from(status);
    output.push('\n');
    output.push_str(&format!("{} {}\n\n", "Usage:".green().bold(), USAGE));
    output.push_str(&render_section(
        "Options:",
        &[("--help, -h".to_string(), "Show this help")],
    ));
    output.push('\n');
    output.push_str(&render_section("Commands:", &commands));
    output.push('\n');
    output.push_str(&render_section("Items:", &items));
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
        output.push_str(&format!(
            "  {}{}  {}\n",
            name.cyan().bold(),
            " ".repeat(width - get_display_width(name)),
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

/// 对象名允许只写唯一前缀（`comp`）。两个对象都能接上就报歧义，不替用户挑一个。
fn resolve_item(candidate: &str) -> Result<&'static str, String> {
    if candidate.is_empty() {
        return Err(unknown_item(candidate));
    }

    let hits: Vec<&'static str> = ITEMS
        .iter()
        .filter(|(name, _)| name.starts_with(candidate))
        .map(|(name, _)| *name)
        .collect();

    match hits.as_slice() {
        [only] => Ok(only),
        [] => Err(unknown_item(candidate)),
        several => Err(format!(
            "Ambiguous item: {:?}\nMatches: {}",
            candidate,
            several.join(", ")
        )),
    }
}

fn unknown_item(item: &str) -> String {
    format!(
        "Unknown item: {:?}\nAvailable: {}",
        item,
        items().join(", ")
    )
}

/// 指名 shell 是明确不支持的用法，所以既要拒绝，也要把它换成能照着做的那一句。
fn no_shell_argument(action: Action, given: &str) -> String {
    let base = format!(
        "`{}` takes no shell argument: it always updates the startup file of the shell running byk",
        action.verb()
    );
    match Shell::parse(given) {
        Some(shell) => format!(
            "{base}\nHint: run `byk @@@shell {} completion` from {} to set up {}",
            action.verb(),
            shell.name(),
            shell.name()
        ),
        None => format!("{base}\nHint: drop {given:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(text: &str) -> String {
        let mut result = String::new();
        let mut chars = text.chars().peekable();
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

    fn detected(shell: Shell, source: Source) -> Detected {
        Detected { shell, source }
    }

    #[test]
    fn status_puts_the_shell_the_file_and_the_state_on_one_level() {
        let text = strip_ansi(&render_status(
            Path::new("/home/coke/.zshrc"),
            detected(Shell::Zsh, Source::Parent),
            &State::Block {
                start: 12,
                end: 14,
                closed: true,
            },
        ));

        // 三个标签同一级，冒号后一个空格就是内容
        assert_eq!(
            text,
            "\nShell: zsh (from parent process)\n\
             FilePath: /home/coke/.zshrc\n\
             Completion: installed (lines 12-14)\n"
        );
    }

    #[test]
    fn status_puts_the_link_target_in_the_same_block() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("zshrc");
        std::fs::write(&real, "x\n").unwrap();
        let link = temp.path().join(".zshrc");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();

        #[cfg(unix)]
        {
            let text = strip_ansi(&render_status(
                &link,
                detected(Shell::Zsh, Source::Parent),
                &State::Missing,
            ));

            assert!(text.contains("FilePath: /"), "{text}");
            assert!(text.contains("Target: /"), "{text}");
            // 链接目标单列一行，不再为对齐去凑空白
            assert!(text.contains("Completion: not installed"), "{text}");
        }
    }

    #[test]
    fn status_marks_a_missing_block_without_repeating_the_help() {
        let text = strip_ansi(&render_status(
            Path::new("/home/coke/.zshrc"),
            detected(Shell::Zsh, Source::Environment),
            &State::Missing,
        ));

        assert!(text.contains("Shell: zsh (from $SHELL)"), "{text}");
        assert!(text.contains("Completion: not installed"), "{text}");
        // 「怎么装上」交给紧跟着的 `Commands:`，这里不再多一句
        assert!(!text.contains("Hint:"), "{text}");
    }

    #[test]
    fn status_without_a_file_names_the_shell_and_why() {
        let text = strip_ansi(&render_no_file(&NoFile::Unsupported {
            name: "pwsh".to_string(),
            source: Source::Environment,
        }));

        // 上下各一个空行：这一屏虽短，也是独立的一屏
        assert_eq!(
            text,
            "\nShell: pwsh (from $SHELL)\n\
             FilePath: (none)\n\
             Completion: not available\n\
             Only zsh, bash and fish have a startup file byk can edit\n\n"
        );
    }

    #[test]
    fn status_without_a_file_admits_an_unknown_shell() {
        let text = strip_ansi(&render_no_file(&NoFile::Unknown));

        assert!(text.contains("Shell: unknown"), "{text}");
        assert!(text.contains("FilePath: (none)"), "{text}");
        assert!(text.contains("run byk from one of them"), "{text}");
    }

    #[test]
    fn modifying_without_a_file_fails_with_the_same_sentences() {
        let missing = NoFile::Unsupported {
            name: "pwsh".to_string(),
            source: Source::Parent,
        };
        assert_eq!(
            missing.headline(),
            "No startup file for pwsh (from parent process)"
        );
        assert_eq!(
            NoFile::Unknown.headline(),
            "No startup file: cannot tell which shell is running"
        );
        assert_eq!(
            NoFile::Unknown.advice(),
            "Only zsh, bash and fish are supported; run byk from one of them"
        );
    }

    #[test]
    fn takes_a_unique_item_prefix() {
        for candidate in ["c", "comp", "complet", "completion"] {
            assert_eq!(resolve_item(candidate), Ok("completion"), "{candidate}");
        }
        assert_eq!(
            resolve_item("path").unwrap_err(),
            "Unknown item: \"path\"\nAvailable: completion"
        );
        // 空词不算前缀：`add ""` 不该被当成全匹配
        assert_eq!(
            resolve_item("").unwrap_err(),
            "Unknown item: \"\"\nAvailable: completion"
        );
    }

    #[test]
    fn help_lists_verbs_and_items() {
        let text = strip_ansi(&render_help(""));
        assert!(text.contains("Usage: byk @@@shell [OPTIONS] [COMMAND] [ITEM]"));
        assert!(text.contains("  add     Enable shell completion"));
        assert!(text.contains("  remove  Disable shell completion"));
        assert!(text.contains("  completion  Shell completion"));
    }

    #[test]
    fn help_opens_with_the_state_it_would_touch() {
        let status = render_status(
            Path::new("/home/coke/.zshrc"),
            detected(Shell::Zsh, Source::Parent),
            &State::Block {
                start: 12,
                end: 14,
                closed: true,
            },
        );
        let text = strip_ansi(&render_help(&status));

        // 状态在前、用法定在后，中间只隔一个空行
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], "Shell: zsh (from parent process)");
        assert_eq!(lines[3], "Completion: installed (lines 12-14)");
        assert_eq!(lines[4], "");
        assert_eq!(lines[5], "Usage: byk @@@shell [OPTIONS] [COMMAND] [ITEM]");
    }

    #[test]
    fn without_a_file_the_screen_stops_after_the_state() {
        let text = strip_ansi(&Screen::NoFile(render_no_file(&NoFile::Unknown)).render());

        assert!(text.contains("Shell: unknown"), "{text}");
        assert!(text.contains("FilePath: (none)"), "{text}");
        assert!(text.contains("run byk from one of them"), "{text}");
        // 没有文件时 `add` / `remove` 都做不成：不列一份用不上的清单
        assert!(!text.contains("Usage:"), "{text}");
        assert!(!text.contains("Commands:"), "{text}");
    }

    #[test]
    fn rejects_unknown_commands() {
        let layout = PathLayout::new();
        assert_eq!(
            run(&["switch".to_string()], &layout).unwrap_err(),
            "Unknown command: \"switch\"\nAvailable: add, remove"
        );
    }

    #[test]
    fn rejects_unknown_items() {
        let layout = PathLayout::new();
        assert_eq!(
            run(&["add".to_string(), "path".to_string()], &layout).unwrap_err(),
            "Unknown item: \"path\"\nAvailable: completion"
        );
        assert_eq!(
            run(&["add".to_string()], &layout).unwrap_err(),
            "Missing item\nAvailable: completion"
        );
    }

    #[test]
    fn rejects_a_shell_argument_and_says_where_to_run_it() {
        let layout = PathLayout::new();
        assert_eq!(
            run(
                &[
                    "add".to_string(),
                    "completion".to_string(),
                    "bash".to_string()
                ],
                &layout
            )
            .unwrap_err(),
            "`add` takes no shell argument: it always updates the startup file of the shell \
             running byk\nHint: run `byk @@@shell add completion` from bash to set up bash"
        );

        // 认不出来的词给不出「去 bash 里跑」这种建议，就只说多余
        assert_eq!(
            run(
                &[
                    "remove".to_string(),
                    "completion".to_string(),
                    "extra".to_string()
                ],
                &layout
            )
            .unwrap_err(),
            "`remove` takes no shell argument: it always updates the startup file of the shell \
             running byk\nHint: drop \"extra\""
        );
    }

    #[test]
    fn describes_its_own_candidates() {
        assert_eq!(describe("add"), Some("Enable shell completion"));
        assert_eq!(describe("completion"), Some("Shell completion"));
        assert_eq!(describe("zsh"), None);
    }
}
