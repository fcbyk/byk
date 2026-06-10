/// NPM Commands 扫描与缓存。
///
/// 扫描 ~/.byk/node-pkgs 下主动安装的 npm 包，提取可执行命令。
/// 缓存至 cache/node-pkg.json，node-pkgs/package.json 或 node_modules
/// mtime 变化时自动重建。

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, exit};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::paths::PathLayout;
use crate::utils::json_io;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单个 npm 包的扫描信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmPackageInfo {
    pub name: String,
    pub version: String,
    pub bins: Vec<String>,
}

/// NPM 缓存数据结构（持久化到 node-pkg.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePkgCache {
    /// 关键文件/目录的 mtime，用于缓存失效检测
    pub watched_mtimes: HashMap<String, u64>,
    /// 扫描时间戳
    pub scanned_at: u64,
    /// 扫描到的包列表
    pub packages: Vec<NpmPackageInfo>,
    /// bin 命令名 → 包名 的映射
    pub bin_map: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// 扫描逻辑
// ---------------------------------------------------------------------------

/// 扫描 node-pkgs 目录下的 npm 包。
///
/// 读取 package.json 的 dependencies 字段，遍历每个包的 bin 信息。
/// 若目录不存在或无 dependencies，返回空列表。
pub fn scan_npm_packages(node_pkgs_dir: &Path) -> Vec<NpmPackageInfo> {
    let pkg_json = node_pkgs_dir.join("package.json");
    if !pkg_json.is_file() {
        return Vec::new();
    }

    let root_data: serde_json::Value = match read_json_file(&pkg_json) {
        Some(v) => v,
        None => {
            eprintln!("Warning: failed to read {}", pkg_json.display());
            return Vec::new();
        }
    };

    let dependencies = match root_data.get("dependencies") {
        Some(serde_json::Value::Object(deps)) => deps.clone(),
        _ => return Vec::new(),
    };

    if dependencies.is_empty() {
        return Vec::new();
    }

    let node_modules = node_pkgs_dir.join("node_modules");
    let mut result: Vec<NpmPackageInfo> = Vec::new();

    for (pkg_name, _version) in &dependencies {
        let pkg_dir = node_modules.join(pkg_name);
        let pkg_pkg_json = pkg_dir.join("package.json");

        if !pkg_pkg_json.is_file() {
            eprintln!("Debug: package.json for {} not found: {}", pkg_name, pkg_pkg_json.display());
            continue;
        }

        let pkg_data: serde_json::Value = match read_json_file(&pkg_pkg_json) {
            Some(v) => v,
            None => {
                eprintln!("Warning: failed to read {}", pkg_pkg_json.display());
                continue;
            }
        };

        let version = pkg_data
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let bins = extract_bins(&pkg_data, pkg_name);

        if !bins.is_empty() {
            result.push(NpmPackageInfo {
                name: pkg_name.clone(),
                version,
                bins,
            });
        }
    }

    result
}

/// 从 package.json 的 bin 字段提取命令名列表。
///
/// bin 字段有两种格式：
/// - 字符串：只有一个命令，命令名为包名（scoped 取 / 后部分，如 @antfu/ni → ni）
/// - 对象：key 为命令名
fn extract_bins(pkg_data: &serde_json::Value, pkg_name: &str) -> Vec<String> {
    let bin_field = match pkg_data.get("bin") {
        Some(v) => v,
        None => return Vec::new(),
    };

    match bin_field {
        serde_json::Value::String(_) => {
            // 字符串形式：命令名 = 包名（scoped 取 scope 后部分）
            vec![bin_name_from_package(pkg_name)]
        }
        serde_json::Value::Object(obj) => {
            // 对象形式：取所有 key
            obj.keys().cloned().collect()
        }
        _ => Vec::new(),
    }
}

/// 从包名推断 bin 命令名。
///
/// 对于 scoped 包（@scope/name），返回 scope 后面的部分。
fn bin_name_from_package(pkg_name: &str) -> String {
    pkg_name.split('/').last().unwrap_or(pkg_name).to_string()
}

/// 构建 bin 命令名 → 包名 的映射，用于命令路由。
pub fn build_bin_map(packages: &[NpmPackageInfo]) -> HashMap<String, String> {
    let mut bin_map = HashMap::new();
    for pkg in packages {
        for bin_name in &pkg.bins {
            bin_map.insert(bin_name.clone(), pkg.name.clone());
        }
    }
    bin_map
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ==================== bin_name_from_package ====================

    #[test]
    fn bin_name_simple_package() {
        assert_eq!(bin_name_from_package("eslint"), "eslint");
    }

    #[test]
    fn bin_name_scoped_package() {
        assert_eq!(bin_name_from_package("@antfu/ni"), "ni");
        assert_eq!(bin_name_from_package("@babel/core"), "core");
        assert_eq!(bin_name_from_package("@scope/pkg"), "pkg");
    }

    #[test]
    fn bin_name_scoped_nested() {
        // split('/') 取最后一段
        assert_eq!(bin_name_from_package("@a/b/c"), "c");
    }

    // ==================== extract_bins ====================

    #[test]
    fn extract_bins_missing_field() {
        let data = json!({"name": "test"});
        assert!(extract_bins(&data, "test").is_empty());
    }

    #[test]
    fn extract_bins_string_format() {
        // bin 为字符串：命令名为包名（scoped 取 scope 后部分）
        let data = json!({"bin": "cli.js"});
        assert_eq!(extract_bins(&data, "mypkg"), vec!["mypkg"]);
    }

    #[test]
    fn extract_bins_string_format_scoped() {
        let data = json!({"bin": "cli.js"});
        assert_eq!(extract_bins(&data, "@scope/mypkg"), vec!["mypkg"]);
    }

    #[test]
    fn extract_bins_object_format() {
        let data = json!({"bin": {"cmd1": "a.js", "cmd2": "b.js"}});
        let mut result = extract_bins(&data, "pkg");
        result.sort();
        assert_eq!(result, vec!["cmd1", "cmd2"]);
    }

    #[test]
    fn extract_bins_object_single_key() {
        let data = json!({"bin": {"tsc": "./bin/tsc"}});
        assert_eq!(extract_bins(&data, "typescript"), vec!["tsc"]);
    }

    #[test]
    fn extract_bins_wrong_type() {
        let data = json!({"bin": 42});
        assert!(extract_bins(&data, "pkg").is_empty());
    }

    #[test]
    fn extract_bins_null_bin() {
        let data = json!({"bin": null});
        assert!(extract_bins(&data, "pkg").is_empty());
    }

    // ==================== build_bin_map ====================

    #[test]
    fn build_bin_map_empty() {
        assert!(build_bin_map(&[]).is_empty());
    }

    #[test]
    fn build_bin_map_single_package() {
        let packages = vec![NpmPackageInfo {
            name: "eslint".into(),
            version: "8.0.0".into(),
            bins: vec!["eslint".into()],
        }];
        let map = build_bin_map(&packages);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("eslint").unwrap(), "eslint");
    }

    #[test]
    fn build_bin_map_multiple_bins() {
        let packages = vec![NpmPackageInfo {
            name: "typescript".into(),
            version: "5.0.0".into(),
            bins: vec!["tsc".into(), "tsserver".into()],
        }];
        let map = build_bin_map(&packages);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("tsc").unwrap(), "typescript");
        assert_eq!(map.get("tsserver").unwrap(), "typescript");
    }

    #[test]
    fn build_bin_map_multiple_packages() {
        let packages = vec![
            NpmPackageInfo {
                name: "eslint".into(),
                version: "8.0.0".into(),
                bins: vec!["eslint".into()],
            },
            NpmPackageInfo {
                name: "prettier".into(),
                version: "3.0.0".into(),
                bins: vec!["prettier".into()],
            },
        ];
        let map = build_bin_map(&packages);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("eslint").unwrap(), "eslint");
        assert_eq!(map.get("prettier").unwrap(), "prettier");
    }
}

// ---------------------------------------------------------------------------
// 缓存：mtime 检测 + node-pkg.json 持久化
// ---------------------------------------------------------------------------

/// 收集 node-pkgs 目录下关键文件的 mtime，用于缓存失效检测。
fn get_watched_mtimes(node_pkgs_dir: &Path) -> HashMap<String, u64> {
    let mut mtimes = HashMap::new();
    let targets = [
        node_pkgs_dir.join("package.json"),
        node_pkgs_dir.join("node_modules"),
    ];
    for p in &targets {
        if let Ok(meta) = fs::metadata(p) {
            if let Ok(modified) = meta.modified() {
                if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                    mtimes.insert(p.to_string_lossy().to_string(), duration.as_secs());
                }
            }
        }
    }
    mtimes
}

/// 对比当前 mtime 与缓存，判断缓存是否失效。
fn is_cache_stale(cached_mtimes: &HashMap<String, u64>, node_pkgs_dir: &Path) -> bool {
    let current = get_watched_mtimes(node_pkgs_dir);
    &current != cached_mtimes
}

/// 构建完整缓存数据结构。
fn build_cache(node_pkgs_dir: &Path) -> NodePkgCache {
    let packages = scan_npm_packages(node_pkgs_dir);
    let bin_map = build_bin_map(&packages);
    let scanned_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    NodePkgCache {
        watched_mtimes: get_watched_mtimes(node_pkgs_dir),
        scanned_at,
        packages,
        bin_map,
    }
}

/// 读取 NPM Commands 缓存文件，失效时自动重建。
///
/// - `cache_file`: ~/.byk/cache/node-pkg.json 路径
/// - `node_pkgs_dir`: ~/.byk/node-pkgs 目录路径
pub fn load_npm_cache(cache_file: &Path, node_pkgs_dir: &Path) -> NodePkgCache {
    let data: Option<NodePkgCache> = json_io::read_json(cache_file);

    match data {
        None => {
            // 无缓存，构建新缓存
            let new_cache = build_cache(node_pkgs_dir);
            json_io::write_json(cache_file, &new_cache);
            new_cache
        }
        Some(cached) => {
            // 检查是否失效
            if is_cache_stale(&cached.watched_mtimes, node_pkgs_dir) {
                let new_cache = build_cache(node_pkgs_dir);
                json_io::write_json(cache_file, &new_cache);
                new_cache
            } else {
                cached
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 内部工具函数
// ---------------------------------------------------------------------------

/// 读取并解析 JSON 文件。
fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

// ---------------------------------------------------------------------------
// 命令执行
// ---------------------------------------------------------------------------

/// 执行 NPM 命令，将 node-pkgs/node_modules/.bin 前置到 PATH 中。
pub fn execute_npm_command(cmd_name: &str, cmd_args: &[String], layout: &PathLayout) {
    let bin_dir = layout.node_pkgs_dir.join("node_modules").join(".bin");

    let mut path_env = bin_dir.to_string_lossy().to_string();
    if let Ok(existing_path) = std::env::var("PATH") {
        path_env = format!("{}:{}", path_env, existing_path);
    }

    let status = Command::new(cmd_name)
        .args(cmd_args)
        .env("PATH", &path_env)
        .status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to execute NPM command: {} - {}", cmd_name, e);
            exit(1);
        }
    }
}
