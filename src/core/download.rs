//! 下载 / 解压 / 建目录 共享原语。
//!
//! 由 `$assets` 下载（`byk add assets` / `byk add <file>` / 远程分发）复用。

use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

use colored::Colorize;

/// 将 raw.githubusercontent.com URL 转换为 jsDelivr CDN URL。
///
/// raw:  https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}
/// cdn:  https://cdn.jsdelivr.net/gh/{owner}/{repo}@{branch}/{path}
pub(crate) fn to_jsdelivr_url(raw_url: &str) -> String {
    let prefix = "https://raw.githubusercontent.com/";
    if let Some(rest) = raw_url.strip_prefix(prefix) {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        if parts.len() >= 3 {
            let owner = parts[0];
            let repo = parts[1];
            let branch_and_path = parts[2];
            let (branch, path) = branch_and_path
                .split_once('/')
                .unwrap_or((branch_and_path, ""));
            return format!(
                "https://cdn.jsdelivr.net/gh/{}/{}@{}/{}",
                owner, repo, branch, path
            );
        }
    }
    raw_url.to_string()
}

/// 下载文件到 dest（进度条写 stderr）。
pub(crate) fn download_script(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if response.status() != 200 {
        return Err(format!(
            "HTTP {} when downloading {}",
            response.status(),
            url
        ));
    }

    // 从响应头获取文件总大小（可能不存在）
    let total_size: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("Failed to create file {}: {}", dest.display(), e))?;

    let mut buf = [0u8; 8192];
    let mut downloaded: u64 = 0;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download failed: {}", e))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Failed to write file: {}", e))?;
        downloaded += n as u64;

        // 每 64KB 刷新一次进度条，避免刷屏
        if downloaded % 65536 < 8192 || downloaded == total_size.unwrap_or(0) {
            if let Some(total) = total_size {
                if total > 0 {
                    let pct = (downloaded * 100).checked_div(total).unwrap_or(0);
                    let bar_width: usize = 30;
                    let filled = (pct as usize * bar_width / 100).min(bar_width);
                    let bar = "#".repeat(filled) + &" ".repeat(bar_width - filled);
                    eprint!(
                        "\r  [{bar}] {:>3}%  ({}/{})\x1b[K",
                        pct,
                        format_size(downloaded),
                        format_size(total)
                    );
                }
            } else {
                eprint!("\r  Downloaded {}\x1b[K", format_size(downloaded));
            }
        }
    }

    // 清除进度行，输出完成提示
    let final_size = total_size.unwrap_or(downloaded);
    eprint!("\r\x1b[K");
    eprintln!("  Downloaded ({})", format_size(final_size));

    Ok(())
}

/// 将字节数格式化为人类可读的大小字符串。
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{}B", bytes)
    } else {
        format!("{:.1}{}", size, UNITS[idx])
    }
}

/// 解压压缩包到目标目录。
pub(crate) fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    if !dest.exists()
        && let Err(e) = std::fs::create_dir_all(dest)
    {
        return Err(format!(
            "failed to create directory {}: {}",
            dest.display(),
            e
        ));
    }

    let status = Command::new("tar")
        .args(["-xf", archive.to_str().unwrap(), "-C", dest.to_str().unwrap()])
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!(
            "tar extract failed with exit code {}",
            s.code().unwrap_or(1)
        )),
        Err(e) => Err(format!("failed to run tar: {}", e)),
    }
}

/// 创建目录，打印操作信息。
pub(crate) fn ensure_dir(path: &Path, label: &str) {
    if path.exists() {
        println!(
            "  {} {} {}",
            "+".dimmed(),
            label.dimmed(),
            "(exists)".dimmed()
        );
    } else {
        std::fs::create_dir_all(path).unwrap_or_else(|e| {
            eprintln!("Failed to create {}: {}", label, e);
        });
        println!("  {} {}", "+".green(), label.dimmed());
    }
}
