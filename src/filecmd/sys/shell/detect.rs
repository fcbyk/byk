//! 探测当前 shell 与它的启动文件：父进程名优先，`$SHELL` 兜底。
//!
//! 不按操作系统猜——macOS 上 bash 一直在，Windows 的 Git Bash 也跑 zsh / bash。
//! 父进程名在 Unix 读 `/proc`（或 `ps`），Windows 走 Toolhelp 快照。

use std::env;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;

/// 补全生成器支持的 shell；名单与 `completion::generate` 保持一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    Zsh,
    Bash,
    Fish,
    Pwsh,
    /// Windows PowerShell 5.1（`powershell.exe`），profile 与 pwsh 分家。
    PowerShell51,
}

impl Shell {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Shell::Zsh => "zsh",
            Shell::Bash => "bash",
            Shell::Fish => "fish",
            Shell::Pwsh => "pwsh",
            Shell::PowerShell51 => "powershell",
        }
    }

    /// 从进程名或路径里认 shell。`sh` / `dash` 没有补全脚本，故意不认。
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match command_name(text) {
            "zsh" => Some(Shell::Zsh),
            "bash" => Some(Shell::Bash),
            "fish" => Some(Shell::Fish),
            "pwsh" => Some(Shell::Pwsh),
            "powershell" => Some(Shell::PowerShell51),
            _ => None,
        }
    }

    /// 启动文件：zsh 认 `$ZDOTDIR`，fish 跟 `$XDG_CONFIG_HOME`，bash 见 `bash_rc`。
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
            // pwsh 的 `$PROFILE` 随操作系统落点不同——这是 PowerShell 自己的规矩，
            // 不属于「按 OS 猜 shell」
            Shell::Pwsh => {
                if cfg!(windows) {
                    home.join("Documents")
                        .join("PowerShell")
                        .join("Microsoft.PowerShell_profile.ps1")
                } else {
                    xdg_config_home
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| home.join(".config"))
                        .join("powershell")
                        .join("Microsoft.PowerShell_profile.ps1")
                }
            }
            Shell::PowerShell51 => home
                .join("Documents")
                .join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
        }
    }
}

/// 进程名或路径里的命令名：`-zsh`、`/bin/zsh`、`bash-3.2` → `zsh` / `zsh` / `bash`。
pub(crate) fn command_name(text: &str) -> &str {
    let name = text
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(text)
        .trim_start_matches('-');
    name.split(['-', '.']).next().unwrap_or(name)
}

/// 登录 shell 读第一个存在的 `.bash_profile` → `.bash_login` → `.profile`。
///
/// `.bashrc` 排最后：只有它存在，说明用户走的是非登录交互 shell。
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

/// 探测结果。父进程认不出不算失败，交给 `$SHELL`；而 `$SHELL` 写着 `sh` 是真的不支持，
/// 两者要分开报，否则 sh 用户只看到一句「认不出」。
#[derive(Clone, Debug)]
pub(crate) enum Detection {
    Found(Detected),
    Unsupported { name: String, source: Source },
    Unknown,
}

/// 父进程优先：`exec bash` 之后 `$SHELL` 仍是登录 shell 的名字，父进程才是此刻在跑的。
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

/// Windows 没有 `/proc`，也没有 `ps`：Toolhelp 快照里按 PID 找父进程名。
#[cfg(windows)]
fn parent_shell() -> Option<Shell> {
    let entries = process_entries();
    let me = std::process::id();
    let parent = entries
        .iter()
        .find(|entry| entry.th32ProcessID == me)?
        .th32ParentProcessID;
    let name = entries
        .iter()
        .find(|entry| entry.th32ProcessID == parent)
        .map(process_name)?;
    Shell::parse(&name)
}

/// 一次快照里的全部进程条目；拿不到就是空，交给 `$SHELL`。
#[cfg(windows)]
fn process_entries() -> Vec<PROCESSENTRY32W> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
        loop {
            entries.push(entry);
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }
    unsafe { CloseHandle(snapshot) };
    entries
}

/// 进程可执行文件名；`szExeFile` 是定长 UTF-16，读到第一个 NUL 为止。
#[cfg(windows)]
fn process_name(entry: &PROCESSENTRY32W) -> String {
    let end = entry
        .szExeFile
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..end])
}

#[cfg(all(not(unix), not(windows)))]
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
            ("pwsh", Shell::Pwsh),
            ("pwsh-preview", Shell::Pwsh),
            ("pwsh.exe", Shell::Pwsh),
            ("powershell", Shell::PowerShell51),
            ("powershell.exe", Shell::PowerShell51),
            (r"C:\Program Files\PowerShell\7\pwsh.exe", Shell::Pwsh),
        ] {
            assert_eq!(Shell::parse(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn rejects_shells_without_a_completion_script() {
        for text in [
            "sh",
            "-sh",
            "dash",
            "",
            "/bin/sh",
            "python3",
            "powershell_ise",
        ] {
            assert_eq!(Shell::parse(text), None, "{text}");
        }
    }

    /// Windows 上父进程名来自 Toolhelp 快照：当前进程与它的父进程都得找得到。
    #[cfg(windows)]
    #[test]
    fn windows_reads_the_parent_process_name() {
        let entries = process_entries();
        let me = std::process::id();
        let parent = entries
            .iter()
            .find(|entry| entry.th32ProcessID == me)
            .map(|entry| entry.th32ParentProcessID)
            .expect("current process missing from the snapshot");
        let name = entries
            .iter()
            .find(|entry| entry.th32ProcessID == parent)
            .map(process_name)
            .expect("parent process missing from the snapshot");
        assert!(!name.is_empty());
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

    #[test]
    fn windows_powershell_profile_is_under_documents() {
        let home = Path::new("/home/coke");
        assert_eq!(
            Shell::PowerShell51.rc_path(home, None, None),
            PathBuf::from(
                "/home/coke/Documents/WindowsPowerShell/Microsoft.PowerShell_profile.ps1"
            )
        );
    }

    /// pwsh 的 `$PROFILE` 在 Unix 跟 XDG，Windows 在「文档」下。
    #[cfg(not(windows))]
    #[test]
    fn pwsh_profile_follows_xdg_on_unix() {
        let home = Path::new("/home/coke");
        assert_eq!(
            Shell::Pwsh.rc_path(home, None, None),
            PathBuf::from("/home/coke/.config/powershell/Microsoft.PowerShell_profile.ps1")
        );
        assert_eq!(
            Shell::Pwsh.rc_path(home, None, Some(Path::new("/etc/xdg"))),
            PathBuf::from("/etc/xdg/powershell/Microsoft.PowerShell_profile.ps1")
        );
    }

    #[cfg(windows)]
    #[test]
    fn pwsh_profile_is_under_documents_on_windows() {
        let home = Path::new(r"C:\Users\coke");
        assert_eq!(
            Shell::Pwsh.rc_path(home, None, None),
            PathBuf::from(r"C:\Users\coke\Documents\PowerShell\Microsoft.PowerShell_profile.ps1")
        );
    }

    /// bash 的逐级下降：不能凭空造 `.bash_profile` 把 `.profile` 屏蔽掉。
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
