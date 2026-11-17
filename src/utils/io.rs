//! 通用标准输出辅助。

use std::io::Write;

/// 写入并刷新 stdout。
pub fn write_stdout(content: &[u8]) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(content)
        .and_then(|_| stdout.flush())
        .map_err(|e| format!("Failed to write output: {}", e))
}
