//! 探测「当前是哪个 shell」以及它的启动文件。
//!
//! 不按操作系统猜。macOS 默认 zsh，但 `/bin/bash` 一直在；Windows 上的 Git Bash
//! 同样是 zsh / bash 的家。所以规则只认两件事——父进程名与 `$SHELL`——同一套代码
//! 在任何平台都成立，平台差异只体现在「补全生成器支持谁」。
//!
//! 不给 `switch`：指向该由环境决定，只在命令行里指名也不行。探测结果就是唯一的
//! 目标——显示的那个文件、真正被写的那个文件、用户眼前这个终端，三者必然一致。

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 补全生成器支持的 shell；名单与 `completion::generate` 保持一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    Zsh,
    Bash,
    Fish,
}

impl Shell {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Shell::Zsh => "zsh",
            Shell::Bash => "bash",
            Shell::Fish => "fish",
        }
    }

    /// 从进程名或路径里认 shell：`-zsh`、`/bin/zsh`、`bash-3.2` 都算。
    ///
    /// `sh` / `dash` 故意不认：它们既没有 `compdef` 也没有 `COMP_WORDS`，
    /// 认下来只会产出装不上的补全。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match command_name(text) {
            "zsh" => Some(Shell::Zsh),
            "bash" => Some(Shell::Bash),
            "fish" => Some(Shell::Fish),
            _ => None,
        }
    }

    /// 启动文件。
    ///
    /// - zsh 认 `$ZDOTDIR`：设了它的人，`.zshrc` 就不在家目录；
    /// - bash 挑第一个「登录 shell 真的会读」的既有文件；只有 `.bashrc` 时说明
    ///   此人跑的是非登录交互 shell（终端直接开的就是这类），那 `.bashrc` 才对；
    ///   一个既有文件都没有才落到 `.bash_profile`——此时没有可被屏蔽的文件，
    ///   建它是安全的；
    /// - fish 跟着 `$XDG_CONFIG_HOME`。
    pub(crate) fn rc_path(
        self,
        home: &Path,
        zdotdir: Option<&Path>,
        xdg_config_home: Option<&Path>,
    ) -> PathBuf {
        match self {
            Shell::Zsh => zdotdir.unwrap_or(home).join(".zshrc"),
            Shell::Bash => bash_rc(home),
            Shell::Fish => xdg_config_home
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".config"))
                .join("fish")
                .join("config.fish"),
        }
    }
}

/// 进程名或路径里真正的命令名：`-zsh`、`/bin/zsh`、`bash-3.2`、`pwsh.exe`
/// 都归一成 `zsh` / `zsh` / `bash` / `pwsh`。报错时用它，比整条路径好读。
pub(crate) fn command_name(text: &str) -> &str {
    let name = text
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(text)
        .trim_start_matches('-');
    name.split(['-', '.']).next().unwrap_or(name)
}

/// bash 登录时按 `.bash_profile` → `.bash_login` → `.profile` 的顺序读第一个存在的。
///
/// `.bashrc` 排在最后：它只被非登录交互 shell 读取，所以只有在前面三个都不存在、
/// 而它存在时才说明用户走的是那条路（Linux 的终端大多如此）。
fn bash_rc(home: &Path) -> PathBuf {
    for name in [".bash_profile", ".bash_login", ".profile", ".bashrc"] {
        let candidate = home.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    home.join(".bash_profile")
}

/// 探测结果的来源。显示出来，用户才知道它选得对不对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Parent,
    Environment,
}

impl Source {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Source::Parent => "from parent process",
            Source::Environment => "from $SHELL",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Detected {
    pub(crate) shell: Shell,
    pub(crate) source: Source,
}

/// 探测结果。
///
/// 父进程认不出**不算**失败：`python`、`cron`、GUI 启动器都不是 shell，这时才轮到
/// `$SHELL` 发言。但 `$SHELL` 写着 `/bin/sh` 就是另一回事了——那是真的「这个 shell
/// 没有生成器」，要和「认不出来」分开报，否则 sh 用户永远只看到 `Cannot detect`，
/// 而他真正该知道的是「补全生成器只覆盖 zsh / bash / fish」。
#[derive(Clone, Debug)]
pub(crate) enum Detection {
    Found(Detected),
    Unsupported { name: String, source: Source },
    Unknown,
}

/// 父进程优先，`$SHELL` 兜底。
///
/// `$SHELL` 是登录 shell：用户 `exec bash` 之后它仍然写着 zsh。父进程才是此刻
/// 真正在跑的那个。取不到父进程（Windows、受限沙箱）才退回环境变量。
pub(crate) fn detect() -> Detection {
    if let Some(shell) = parent_shell() {
        return Detection::Found(Detected {
            shell,
            source: Source::Parent,
        });
    }

    let Some(raw) = env::var("SHELL").ok().filter(|value| !value.is_empty()) else {
        return Detection::Unknown;
    };

    match Shell::parse(&raw) {
        Some(shell) => Detection::Found(Detected {
            shell,
            source: Source::Environment,
        }),
        None => Detection::Unsupported {
            name: command_name(&raw).to_string(),
            source: Source::Environment,
        },
    }
}

#[cfg(unix)]
fn parent_shell() -> Option<Shell> {
    let parent = unsafe { libc::getppid() };

    // Linux 先读 /proc：不创建进程，也没有 `ps` 缺失的可能
    if let Ok(text) = std::fs::read_to_string(format!("/proc/{parent}/comm"))
        && let Some(shell) = Shell::parse(text.trim())
    {
        return Some(shell);
    }

    // macOS 没有 /proc，只能问 ps；受限环境里它可能被拒绝，那就交给 $SHELL
    let output = Command::new("ps")
        .args(["-p", &parent.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Shell::parse(String::from_utf8_lossy(&output.stdout).trim())
}

#[cfg(not(unix))]
fn parent_shell() -> Option<Shell> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_process_names_and_paths() {
        for (text, expected) in [
            ("zsh", Shell::Zsh),
            ("-zsh", Shell::Zsh),
            ("/bin/zsh", Shell::Zsh),
            ("/usr/local/bin/fish", Shell::Fish),
            ("-bash", Shell::Bash),
            ("bash-3.2", Shell::Bash),
        ] {
            assert_eq!(Shell::parse(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn rejects_shells_without_a_completion_script() {
        for text in ["sh", "-sh", "dash", "", "/bin/sh", "python3", "pwsh"] {
            assert_eq!(Shell::parse(text), None, "{text}");
        }
    }

    #[test]
    fn zsh_honours_zdotdir() {
        let home = Path::new("/home/coke");
        assert_eq!(
            Shell::Zsh.rc_path(home, None, None),
            PathBuf::from("/home/coke/.zshrc")
        );
        assert_eq!(
            Shell::Zsh.rc_path(home, Some(Path::new("/home/coke/zdot")), None),
            PathBuf::from("/home/coke/zdot/.zshrc")
        );
    }

    #[test]
    fn fish_honours_xdg_config_home() {
        let home = Path::new("/home/coke");
        assert_eq!(
            Shell::Fish.rc_path(home, None, None),
            PathBuf::from("/home/coke/.config/fish/config.fish")
        );
        assert_eq!(
            Shell::Fish.rc_path(home, None, Some(Path::new("/etc/xdg"))),
            PathBuf::from("/etc/xdg/fish/config.fish")
        );
    }

    /// bash 的三级下降：不能凭空造 `.bash_profile` 把 `.profile` 屏蔽掉。
    #[test]
    fn bash_prefers_the_file_a_login_shell_reads() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();

        assert_eq!(bash_rc(home), home.join(".bash_profile"));

        std::fs::write(home.join(".profile"), "x").unwrap();
        assert_eq!(bash_rc(home), home.join(".profile"));

        std::fs::write(home.join(".bash_login"), "x").unwrap();
        assert_eq!(bash_rc(home), home.join(".bash_login"));

        std::fs::write(home.join(".bash_profile"), "x").unwrap();
        assert_eq!(bash_rc(home), home.join(".bash_profile"));

        // `.bashrc` 只被非登录交互 shell 读；前面三个都没有时，它才说明用户走那条路
        std::fs::remove_file(home.join(".bash_profile")).unwrap();
        std::fs::remove_file(home.join(".bash_login")).unwrap();
        std::fs::remove_file(home.join(".profile")).unwrap();
        std::fs::write(home.join(".bashrc"), "x").unwrap();
        assert_eq!(bash_rc(home), home.join(".bashrc"));

        // 一个都不存在才新建 `.bash_profile`：没有可被屏蔽的文件，建它安全
        std::fs::remove_file(home.join(".bashrc")).unwrap();
        assert_eq!(bash_rc(home), home.join(".bash_profile"));
    }
}
