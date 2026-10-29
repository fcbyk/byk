//! `-v` 输出渲染：版本信息与安装位置。

use colored::Colorize;

/// 渲染版本信息。
///
/// 首行格式固定为 `byk <版本> (<commit> <构建日期>) [<平台>]`。
pub fn render() {
    println!(
        "byk {} ({} {}) [{}]",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_HASH"),
        env!("BUILD_DATE"),
        env!("PLATFORM"),
    );

    if let Ok(exe_path) = std::env::current_exe() {
        println!(
            "{} {}",
            "installed at".dimmed(),
            exe_path.display().to_string().dimmed(),
        );
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_does_not_panic() {
        render();
    }
}
