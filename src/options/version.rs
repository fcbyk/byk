//! `-v` 输出渲染：版本信息与安装位置。

use std::fs;

use colored::Colorize;

/// 渲染版本信息。
///
/// 首行格式固定为 `byk <版本> (<commit> <构建日期>) [<平台>]`。
pub fn run() -> i32 {
    println!(
        "byk {} ({} {}) [{}]",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_HASH"),
        env!("BUILD_DATE"),
        env!("PLATFORM"),
    );

    if let Ok(exe_path) = std::env::current_exe() {
        let mut line = format!(
            "{} {}",
            "installed at".dimmed(),
            exe_path.display().to_string().dimmed(),
        );

        if let Ok(meta) = fs::symlink_metadata(&exe_path)
            && meta.file_type().is_symlink()
            && let Ok(target) = fs::read_link(&exe_path)
        {
            line.push_str(&format!(" -> {}", target.display().to_string().dimmed()));
        }

        println!("{line}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_does_not_panic() {
        run();
    }
}
