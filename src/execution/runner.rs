//! Shell 进程启动与平台差异处理。

use std::path::Path;
use std::process::{Command, exit};

use super::chain::ENV_ALIAS_STACK;
use super::environment::build_alias_env;
use super::placeholder::parse_alias_arguments_with_mapping;

/// 为 Windows 构造执行别名的 `cmd` 进程。
///
/// 必须用 `raw_arg` 手工拼接命令行，不能写成 `.arg("/C").arg(cmd)`：
/// `arg()` 会按 MSVCRT 规则给整条参数加引号、并把内部的 `"` 转义成 `\"`，
/// 而 cmd.exe 不认反斜杠转义，会把 `\"` 当成字面上的「反斜杠 + 引号」，
/// 于是 `mkdir "%USERPROFILE%\a b"` 这类带引号的命令一律报
/// 「文件名、目录名或卷标语法不正确」(ERROR_INVALID_NAME)。
///
/// `/S` 的作用是让 cmd 只剥掉最外层的一对引号、其余内容原样执行，
/// 这样命令内部的引号才能完整保留。
#[cfg(windows)]
pub(super) fn windows_cmd(final_command: &str) -> Command {
    use std::os::windows::process::CommandExt;

    let mut command = Command::new("cmd");
    command
        .raw_arg("/S")
        .raw_arg("/C")
        .raw_arg(format!("\"{}\"", final_command));
    command
}

/// 执行最终命令（平台适配：Windows 用 cmd /C，Unix 用 sh -c）。
///
/// `alias_stack` 是当前的调用链（逗号分隔的 `file.key`），必须随环境下发：
/// 子进程里的 byk 靠它认出「我正在被自己调用」。走环境变量而不是命令行参数，
/// 是因为 `$cmd` 是任意 shell 字符串，命令行参数根本传不进去。
pub(super) fn run_command(
    final_command: &str,
    working_dir: &Path,
    custom_paths: &[String],
    alias_stack: &str,
) -> ! {
    let mut env_map = build_alias_env(working_dir, custom_paths);
    env_map.insert(ENV_ALIAS_STACK.to_string(), alias_stack.to_string());

    #[cfg(windows)]
    let final_command = convert_unix_path_for_cmd(final_command);

    #[cfg(not(windows))]
    let final_command = final_command.to_string();

    #[cfg(windows)]
    let status = windows_cmd(&final_command)
        .current_dir(working_dir)
        .envs(&env_map)
        .status();

    #[cfg(not(windows))]
    let status = Command::new("sh")
        .arg("-c")
        .arg(&final_command)
        .current_dir(working_dir)
        .envs(&env_map)
        .status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to execute alias: {}", e);
            exit(1);
        }
    }
}

/// 非交互执行：解析占位符后直接启动进程。
pub(super) fn execute_direct_impl(
    command: &str,
    args: &[String],
    working_dir: &Path,
    custom_paths: &[String],
    announce: bool,
    alias_stack: &str,
) -> ! {
    let (final_command, _) = parse_alias_arguments_with_mapping(command, args, &[]);
    if announce {
        eprintln!("> {}\n", final_command);
    }
    run_command(&final_command, working_dir, custom_paths, alias_stack);
}

/// 将命令中的 Unix 风格 `./` 前缀转为 Windows `.\`，使 cmd 可识别。
/// 仅转换非路径部分的前缀（如 `./pip` → `.\pip`）。
#[cfg(windows)]
pub(super) fn convert_unix_path_for_cmd(command: &str) -> String {
    // 只转换命令开头的 ./（如 ./pip install → .\pip install）
    if let Some(stripped) = command.strip_prefix("./") {
        return format!(".\\{}", stripped);
    }
    command.to_string()
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

    /// 回归测试：命令内部的引号必须原样抵达 cmd。
    ///
    /// 旧实现用 `.arg("/C").arg(cmd)`，Rust 会把内部引号转义成 `\"`，
    /// cmd 收到字面反斜杠，`echo "A B"` 会变成 `echo \"A B\"`。
    #[cfg(windows)]
    #[test]
    fn windows_cmd_preserves_inner_quotes() {
        let out = windows_cmd(r#"echo "A B""#).output().unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "\"A B\"");
    }

    /// 回归测试：含空格的带引号路径不会被拆开。
    #[cfg(windows)]
    #[test]
    fn windows_cmd_keeps_quoted_path_intact() {
        let out = windows_cmd(r#"if exist "C:\Program Files" echo FOUND"#)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "cmd failed: {}", stdout);
        assert_eq!(stdout.trim(), "FOUND");
    }
}
