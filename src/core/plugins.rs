/// 插件发现、缓存与执行。
///
/// 扫描虚拟环境的 site-packages 目录，查找每个包根目录下的 byk.json 清单文件。
/// 扫描结果缓存至 cache/plugins.json，通过对比 site-packages 目录的 mtime
/// 判断缓存是否失效。执行时直接调用 `python -m <模块>` 透传参数。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::utils::json_io;

// ---------------------------------------------------------------------------
// 平台常量
// ---------------------------------------------------------------------------

/// venv 内 bin 目录名。
#[cfg(windows)]
const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
const VENV_BIN: &str = "bin";

/// venv 内 Python 可执行文件名。
#[cfg(windows)]
const PYTHON_BIN: &str = "python.exe";
#[cfg(not(windows))]
const PYTHON_BIN: &str = "python";

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单个插件命令的缓存条目。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginCommand {
    /// Python 模块路径，如 "hello.one"
    pub module: String,
    /// 命令描述
    pub description: String,
}

/// 插件缓存（持久化到 cache/plugins.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCache {
    /// 监控的目录路径 → mtime 时间戳（秒，f64）
    pub watched_mtimes: HashMap<String, f64>,
    /// 扫描时间戳
    #[serde(default)]
    pub scanned_at: f64,
    /// 已安装插件的命令列表
    pub commands: HashMap<String, PluginCommand>,
    /// Python 解释器路径（venv 内的 python）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python_executable: Option<String>,
}

// ---------------------------------------------------------------------------
// 空缓存
// ---------------------------------------------------------------------------

/// 构造空插件缓存（venv 不存在时使用）。
pub fn empty_plugin_cache() -> PluginCache {
    PluginCache {
        watched_mtimes: HashMap::new(),
        scanned_at: 0.0,
        commands: HashMap::new(),
        python_executable: None,
    }
}

// ---------------------------------------------------------------------------
// Python 解释器路径
// ---------------------------------------------------------------------------

/// 获取 Python 解释器路径。
///
/// 优先级：
/// 1. 缓存文件（plugins.json）中的 `python_executable`
/// 2. 如果 venv 存在 → `venv/bin/python`
pub(crate) fn get_python_executable(cache_dir: &Path, venv_dir: &Path) -> String {
    let cache_file = cache_dir.join("plugins.json");
    if let Some(data) = json_io::read_json::<PluginCache>(&cache_file) {
        if let Some(exe) = data.python_executable {
            return exe;
        }
    }

    let venv_python = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    venv_python.to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// 扫描逻辑
// ---------------------------------------------------------------------------

/// 通过 venv 内的 Python 获取 site-packages 路径。
///
/// 等价于：`venv/bin/python -c "import site; print(site.getsitepackages()[0])"`
fn get_venv_site_packages(venv_dir: &Path) -> Option<PathBuf> {
    let python = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    let output = Command::new(&python)
        .args(["-c", "import site; print(site.getsitepackages()[0])"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path_str.is_empty() {
        return None;
    }

    Some(PathBuf::from(path_str))
}

/// 获取路径的 mtime（秒，f64）。
fn path_mtime(path: &Path) -> Option<f64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
}

/// 扫描 site-packages 目录下的 byk.json 文件。
///
/// 遍历 site-packages 下的每个子目录，跳过 .dist-info、__pycache__、
/// 以点开头的目录，检查其根目录是否存在 byk.json。
/// 存在则读取并展开 commands。
fn scan_plugins_from_site_packages(
    site_packages: &Path,
) -> HashMap<String, PluginCommand> {
    let mut commands: HashMap<String, PluginCommand> = HashMap::new();

    let entries = match fs::read_dir(site_packages) {
        Ok(e) => e,
        Err(_) => return commands,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // 跳过特殊目录
        if name_str.starts_with('.') || name_str == "__pycache__" || name_str.ends_with(".dist-info")
        {
            continue;
        }

        let byk_json = path.join("byk.json");
        if !byk_json.is_file() {
            continue;
        }

        // 解析 byk.json
        let parsed: Option<HashMap<String, serde_json::Value>> =
            json_io::read_json(&byk_json);
        if let Some(entries) = parsed {
            for (cmd_name, cmd_value) in entries {
                let module = cmd_value
                    .get("module")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // module 为空 → 跳过
                if module.is_empty() {
                    continue;
                }
                let description = cmd_value
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                commands.insert(
                    cmd_name,
                    PluginCommand {
                        module: module.to_string(),
                        description,
                    },
                );
            }
        }
    }

    commands
}

// ---------------------------------------------------------------------------
// 缓存构建
// ---------------------------------------------------------------------------

/// 扫描并构建完整插件缓存。
fn scan_and_build_cache(venv_dir: &Path) -> Option<PluginCache> {
    let site_packages = get_venv_site_packages(venv_dir)?;
    let mtime = path_mtime(&site_packages)?;

    let commands = scan_plugins_from_site_packages(&site_packages);

    let python_exe = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    let scanned_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let mut watched_mtimes = HashMap::new();
    watched_mtimes.insert(site_packages.to_string_lossy().to_string(), mtime);

    Some(PluginCache {
        watched_mtimes,
        scanned_at,
        commands,
        python_executable: Some(python_exe.to_string_lossy().to_string()),
    })
}

// ---------------------------------------------------------------------------
// 缓存失效检测
// ---------------------------------------------------------------------------

/// 对比缓存中的 mtime 与当前文件系统 mtime，判断缓存是否失效。
fn is_plugin_cache_stale(cached_mtimes: &HashMap<String, f64>) -> bool {
    for (path, cached_mtime) in cached_mtimes {
        match path_mtime(Path::new(path)) {
            Some(current) => {
                if (current - cached_mtime).abs() > 0.001 {
                    return true;
                }
            }
            None => return true,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 缓存加载（主入口）
// ---------------------------------------------------------------------------

/// 读取插件缓存，失效时自动重建。
///
/// - venv 不存在 → 返回空缓存，不触发任何扫描
/// - 无缓存文件 → 扫描并写入 plugins.json
/// - 缓存过期 → 重建并写入
/// - 缓存有效 → 直接返回
pub fn load_plugin_cache(cache_dir: &Path, venv_dir: &Path) -> PluginCache {
    if !venv_dir.is_dir() {
        return empty_plugin_cache();
    }

    let cache_file = cache_dir.join("plugins.json");

    let data: Option<PluginCache> = json_io::read_json(&cache_file);

    match data {
        None => {
            // 无缓存 → 扫描并写入
            if let Some(cache) = scan_and_build_cache(venv_dir) {
                json_io::write_json(&cache_file, &cache);
                cache
            } else {
                empty_plugin_cache()
            }
        }
        Some(cached) => {
            if is_plugin_cache_stale(&cached.watched_mtimes) {
                // 缓存过期 → 重建
                if let Some(cache) = scan_and_build_cache(venv_dir) {
                    json_io::write_json(&cache_file, &cache);
                    cache
                } else {
                    // 重建失败 → 返回旧缓存（兜底）
                    cached
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
/// 直接通过 `python -m <module> <args>` 调用，不再经过 bykpy 中间层。
pub fn execute_plugin_command(
    cmd_name: &str,
    cmd_args: &[String],
    cache_dir: &Path,
    venv_dir: &Path,
    plugin_cache: &PluginCache,
) {
    let python_exe = get_python_executable(cache_dir, venv_dir);

    let module = match plugin_cache.commands.get(cmd_name) {
        Some(cmd) => &cmd.module,
        None => {
            eprintln!(
                "Internal error: command '{}' not found in plugin cache",
                cmd_name
            );
            exit(1);
        }
    };

    let status = Command::new(&python_exe)
        .arg("-m")
        .arg(module)
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

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 获取指定路径的当前 mtime（秒，f64）。
    fn file_mtime(path: &Path) -> f64 {
        path_mtime(path).unwrap_or(0.0)
    }

    // ==================== is_plugin_cache_stale ====================

    #[test]
    fn cache_stale_empty_cache_not_stale() {
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

        let mtime = file_mtime(&file_path);
        let mut mtimes = HashMap::new();
        mtimes.insert(file_path.to_string_lossy().to_string(), mtime);

        assert!(!is_plugin_cache_stale(&mtimes));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_stale_mismatched_mtime() {
        let dir = std::env::temp_dir().join("fcbyk_test_plugin2");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test2.txt");
        fs::write(&file_path, "content").unwrap();

        let mut mtimes = HashMap::new();
        mtimes.insert(file_path.to_string_lossy().to_string(), 0.0);
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
        mtimes.insert(f1.to_string_lossy().to_string(), m1);
        mtimes.insert(f2.to_string_lossy().to_string(), 0.0);

        assert!(is_plugin_cache_stale(&mtimes));

        let _ = fs::remove_dir_all(&dir);
    }

    // ==================== scan_plugins ====================

    #[test]
    fn scan_empty_directory() {
        let dir = std::env::temp_dir().join("fcbyk_test_scan_empty");
        fs::create_dir_all(&dir).unwrap();
        let result = scan_plugins_from_site_packages(&dir);
        assert!(result.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_skips_dist_info() {
        let dir = std::env::temp_dir().join("fcbyk_test_scan_distinfo");
        fs::create_dir_all(dir.join("some_pkg-1.0.dist-info")).unwrap();
        let result = scan_plugins_from_site_packages(&dir);
        assert!(result.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_skips_dot_dirs() {
        let dir = std::env::temp_dir().join("fcbyk_test_scan_dot");
        fs::create_dir_all(dir.join(".hidden_pkg")).unwrap();
        let result = scan_plugins_from_site_packages(&dir);
        assert!(result.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_skips_pycache() {
        let dir = std::env::temp_dir().join("fcbyk_test_scan_pycache");
        fs::create_dir_all(dir.join("__pycache__")).unwrap();
        let result = scan_plugins_from_site_packages(&dir);
        assert!(result.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_skips_empty_module() {
        let dir = std::env::temp_dir().join("fcbyk_test_scan_empty_module");
        let pkg_dir = dir.join("myplugin");
        fs::create_dir_all(&pkg_dir).unwrap();
        // module 为空 → 应跳过
        let byk_json = serde_json::json!({
            "badcmd": {
                "module": "",
                "description": "this should be skipped"
            },
            "goodcmd": {
                "module": "myplugin.main",
                "description": "this should be kept"
            }
        });
        fs::write(
            pkg_dir.join("byk.json"),
            serde_json::to_string_pretty(&byk_json).unwrap(),
        )
        .unwrap();

        let result = scan_plugins_from_site_packages(&dir);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("goodcmd"));
        assert!(!result.contains_key("badcmd"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_finds_byk_json() {
        let dir = std::env::temp_dir().join("fcbyk_test_scan_found");
        let pkg_dir = dir.join("myplugin");
        fs::create_dir_all(&pkg_dir).unwrap();
        let byk_json = serde_json::json!({
            "hello": {
                "module": "myplugin.hello",
                "description": "Say hello"
            }
        });
        fs::write(
            pkg_dir.join("byk.json"),
            serde_json::to_string_pretty(&byk_json).unwrap(),
        )
        .unwrap();

        let result = scan_plugins_from_site_packages(&dir);
        assert_eq!(result.len(), 1);
        let cmd = result.get("hello").unwrap();
        assert_eq!(cmd.module, "myplugin.hello");
        assert_eq!(cmd.description, "Say hello");
        let _ = fs::remove_dir_all(&dir);
    }
}
