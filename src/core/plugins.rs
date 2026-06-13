/// 插件缓存读取与自动刷新。
///
/// 读取 Python 插件层生成的 app.json 缓存文件。
/// 通过对比 site-packages 目录的 mtime 判断缓存是否失效，
/// 失效时 spawn Python 重建缓存（复用 Python 已有的扫描逻辑）。
/// Rust 层仅消费缓存产物，不负责插件扫描。

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, exit};
use std::time::UNIX_EPOCH;

use serde::Deserialize;

use crate::utils::json_io;

/// 单个插件命令的缓存条目。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginCommand {
    /// Python 模块路径，如 "byklansend.main:Plugin"
    #[allow(dead_code)]
    pub module: String,
    /// 命令描述
    pub description: String,
}

/// 插件缓存（对应 Python `_build_cache()` 产物）。
#[derive(Debug, Clone, Deserialize)]
pub struct PluginCache {
    /// 监控的目录路径 → mtime 时间戳（秒，`time.time()`）
    pub watched_mtimes: HashMap<String, f64>,
    /// 已安装插件的命令列表
    pub commands: HashMap<String, PluginCommand>,
    /// Python 解释器路径（由 Python 层写入）
    #[serde(default)]
    pub python_executable: Option<String>,
}

/// 对比缓存中的 mtime 与当前文件系统 mtime，判断缓存是否失效。
fn is_plugin_cache_stale(cached_mtimes: &HashMap<String, f64>) -> bool {
    for (path, cached_mtime) in cached_mtimes {
        match fs::metadata(path) {
            Ok(meta) => match meta.modified() {
                Ok(modified) => match modified.duration_since(UNIX_EPOCH) {
                    Ok(duration) => {
                        let current = duration.as_secs_f64();
                        if (current - cached_mtime).abs() > 0.001 {
                            return true;
                        }
                    }
                    Err(_) => return true,
                },
                Err(_) => return true,
            },
            Err(_) => return true,
        }
    }
    false
}

/// 构造空插件缓存（~/.byk 不存在时使用，避免不必要的 bykpy spawn）。
pub fn empty_plugin_cache() -> PluginCache {
    PluginCache {
        watched_mtimes: HashMap::new(),
        commands: HashMap::new(),
        python_executable: None,
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::UNIX_EPOCH;

    /// 获取指定路径的当前 mtime（秒，f64）。
    fn file_mtime(path: &Path) -> f64 {
        fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    // ==================== is_plugin_cache_stale ====================

    #[test]
    fn cache_stale_empty_cache_not_stale() {
        // 空缓存没有要检查的路径，始终返回 false
        assert!(!is_plugin_cache_stale(&HashMap::new()));
    }

    #[test]
    fn cache_stale_nonexistent_path() {
        let mut mtimes = HashMap::new();
        mtimes.insert("/nonexistent/path/for/test".into(), 1.0);
        assert!(is_plugin_cache_stale(&mtimes));
    }

    #[test]
    fn cache_stale_existing_path_with_matching_mtime() {
        let dir = std::env::temp_dir().join("fcbyk_test_plugin");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test.txt");
        fs::write(&file_path, "content").unwrap();

        // 立即获取 mtime
        let mtime = file_mtime(&file_path);
        let mut mtimes = HashMap::new();
        mtimes.insert(file_path.to_string_lossy().to_string(), mtime);

        // mtime 差值应在 1ms 以内，判定为不陈旧
        assert!(!is_plugin_cache_stale(&mtimes));

        // 清理
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_stale_mismatched_mtime() {
        let dir = std::env::temp_dir().join("fcbyk_test_plugin2");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test2.txt");
        fs::write(&file_path, "content").unwrap();

        // 使用一个远小于实际 mtime 的值，应判定为陈旧
        let mut mtimes = HashMap::new();
        mtimes.insert(file_path.to_string_lossy().to_string(), 0.0); // epoch
        assert!(is_plugin_cache_stale(&mtimes));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_stale_multiple_paths_one_mismatch() {
        let dir = std::env::temp_dir().join("fcbyk_test_plugin3");
        fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        fs::write(&f1, "a").unwrap();
        fs::write(&f2, "b").unwrap();

        let m1 = file_mtime(&f1);
        let mut mtimes = HashMap::new();
        mtimes.insert(f1.to_string_lossy().to_string(), m1); // 匹配
        mtimes.insert(f2.to_string_lossy().to_string(), 0.0); // 不匹配 → stale

        assert!(is_plugin_cache_stale(&mtimes));

        let _ = fs::remove_dir_all(&dir);
    }
}

/// 获取 Python 解释器路径。
///
/// 优先级：
/// 1. 环境变量 `BYK_PYTHON`
/// 2. 缓存文件中的 `python_executable`
/// 3. 平台默认：Windows `python`，Unix `python3`
pub(crate) fn get_python_executable(cache_dir: &Path) -> String {
    // 1. 检查环境变量
    if let Ok(exe) = std::env::var("BYK_PYTHON") {
        return exe;
    }

    // 2. 尝试从缓存文件读取
    let cache_file = cache_dir.join("app.json");
    if let Some(data) = json_io::read_json::<PluginCache>(&cache_file) {
        if let Some(exe) = data.python_executable {
            return exe;
        }
    }

    // 3. 平台默认值
    #[cfg(windows)]
    { "python".to_string() }
    #[cfg(not(windows))]
    { "python3".to_string() }
}

/// 调用 Python 重建插件缓存。
///
/// 返回 `true` 表示扫描成功，`false` 表示 bykpy 运行时不可用。
fn refresh_plugin_cache(cache_dir: &Path) -> bool {
    let python_exe = get_python_executable(cache_dir);

    // 先快速检查 bykpy 是否可导入（避免 spawn 注定失败的扫描进程）
    let check = Command::new(&python_exe)
        .args(["-c", "import bykpy"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if !check.map(|s| s.success()).unwrap_or(false) {
        return false;
    }

    let status = Command::new(&python_exe)
        .args(["-m", "bykpy", "--scan-plugins"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    status.map(|s| s.success()).unwrap_or(false)
}

/// 读取插件缓存，失效时自动调用 Python 重建。
///
/// 无缓存 → spawn Python 写入后重读。
/// 缓存过期 → spawn Python 刷新后重读。
/// 缓存有效 → 直接返回。
/// bykpy 不可用时 → 删除过期缓存，返回空 PluginCache。
pub fn load_plugin_cache(cache_dir: &Path) -> PluginCache {
    let cache_file = cache_dir.join("app.json");
    let empty_cache = || PluginCache {
        watched_mtimes: HashMap::new(),
        commands: HashMap::new(),
        python_executable: None,
    };

    let data: Option<PluginCache> = json_io::read_json(&cache_file);

    match data {
        None => {
            if !refresh_plugin_cache(cache_dir) {
                return empty_cache();
            }
            json_io::read_json(&cache_file).unwrap_or_else(empty_cache)
        }
        Some(cached) => {
            if is_plugin_cache_stale(&cached.watched_mtimes) {
                if refresh_plugin_cache(cache_dir) {
                    json_io::read_json(&cache_file).unwrap_or(cached)
                } else {
                    // bykpy 不存在 → 删除过期缓存，返回空，反正任何插件命令也执行不了
                    let _ = fs::remove_file(&cache_file);
                    empty_cache()
                }
            } else {
                cached
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 命令执行
// ---------------------------------------------------------------------------

/// 将插件命令转发给 Python 执行。
///
/// Rust 无需知道插件属于哪个模块，直接通过 `python3 -m bykpy <cmd> <args>`
/// 走 Python 现有的 Click 路由（含懒加载、错误处理）。
pub fn execute_plugin_command(cmd_name: &str, cmd_args: &[String], cache_dir: &Path) {
    let python_exe = get_python_executable(cache_dir);
    let status = Command::new(python_exe)
        .arg("-m")
        .arg("bykpy")
        .arg(cmd_name)
        .args(cmd_args)
        .status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to start Python runtime: {}", e);
            exit(1);
        }
    }
}
