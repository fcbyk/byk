/// 别名缓存：持久化扫描结果与合并配置，避免每次启动重复 I/O 和计算。
///
/// 缓存 ~/.byk/cache/alias.json，通过文件 mtime 快照检测失效。
/// 模式与 node 和 plugins 缓存一致。

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::merge::build_merged_aliases;
use super::scan::{scan_alias_files, sort_alias_files};
use super::types::{AliasFile, MergedConfig};
use crate::utils::json_io;

// ---------------------------------------------------------------------------
// 缓存数据结构
// ---------------------------------------------------------------------------

/// 别名缓存（持久化到 alias.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasCache {
    /// 所有被扫描 .byk.json 文件的路径 → mtime (秒级)，用于失效检测。
    /// 本地 + 全局文件混合存储，CWD 变化自然导致快照不匹配。
    pub watched_mtimes: HashMap<String, u64>,
    /// 缓存时间戳
    pub scanned_at: u64,
    /// 排序后的 AliasFile 列表（供精确执行 @file.key 使用）
    pub files: Vec<AliasFile>,
    /// 深度合并后的配置树（供普通别名解析使用）
    pub merged: MergedConfig,
}

// ---------------------------------------------------------------------------
// mtime 快照
// ---------------------------------------------------------------------------

/// 扫描目录下所有 *.byk.json 文件，返回 (路径 → mtime) 映射。
pub(crate) fn get_watched_mtimes(dir: &Path) -> HashMap<String, u64> {
    let mut mtimes = HashMap::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return mtimes,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !file_name.ends_with(".byk.json") {
            continue;
        }
        if let Ok(meta) = fs::metadata(&path) {
            if let Ok(modified) = meta.modified() {
                if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                    mtimes.insert(
                        path.to_string_lossy().to_string(),
                        duration.as_secs(),
                    );
                }
            }
        }
    }
    mtimes
}

/// 构建本地 + 全局目录的完整 mtime 快照。
fn build_full_mtimes(cwd: &Path, global_dir: &Path) -> HashMap<String, u64> {
    let mut mtimes = get_watched_mtimes(cwd);
    mtimes.extend(get_watched_mtimes(global_dir));
    mtimes
}

// ---------------------------------------------------------------------------
// 缓存加载（主入口）
// ---------------------------------------------------------------------------

/// 加载别名缓存，失效时自动重建。
///
/// - `cache_file`: ~/.byk/cache/alias.json 路径
/// - `cwd`: 当前工作目录
/// - `global_dir`: ~/.byk/alias/ 目录路径
///
/// 返回 (合并配置, 文件列表)。
pub fn load_alias_cache(
    cache_file: &Path,
    cwd: &Path,
    global_dir: &Path,
) -> (MergedConfig, Vec<AliasFile>) {
    let current_mtimes = build_full_mtimes(cwd, global_dir);

    // 尝试读取缓存
    let cached: Option<AliasCache> = json_io::read_json(cache_file);

    if let Some(cache) = cached {
        if cache.watched_mtimes == current_mtimes {
            // 缓存命中，直接返回
            return (cache.merged, cache.files);
        }
        // 缓存失效，继续重建
    }

    // 无缓存或缓存失效 → 完整重建
    let mut files: Vec<AliasFile> = Vec::new();
    files.extend(scan_alias_files(cwd, false));
    files.extend(scan_alias_files(global_dir, true));
    sort_alias_files(&mut files);
    let merged = build_merged_aliases(&files);

    let scanned_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let new_cache = AliasCache {
        watched_mtimes: current_mtimes,
        scanned_at,
        files: files.clone(),
        merged,
    };

    json_io::write_json(cache_file, &new_cache);

    (new_cache.merged, new_cache.files)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

    // ==================== get_watched_mtimes ====================

    #[test]
    fn mtimes_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mtimes = get_watched_mtimes(dir.path());
        assert!(mtimes.is_empty());
    }

    #[test]
    fn mtimes_detects_byk_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        fs::write(&file, r#"{"hello": "echo hi"}"#).unwrap();

        let mtimes = get_watched_mtimes(dir.path());
        assert_eq!(mtimes.len(), 1);
        let key = file.to_string_lossy().to_string();
        assert!(mtimes.contains_key(&key));
    }

    #[test]
    fn mtimes_ignores_non_byk_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        fs::write(dir.path().join("other.json"), "{}").unwrap();
        fs::write(dir.path().join("nope.byk.txt"), "{}").unwrap();

        let mtimes = get_watched_mtimes(dir.path());
        assert!(mtimes.is_empty());
    }

    #[test]
    fn mtimes_detects_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        fs::write(&file, r#"{"hello": "echo hi"}"#).unwrap();

        let before = get_watched_mtimes(dir.path());

        // 等待确保 mtime 变化
        thread::sleep(Duration::from_secs(1));
        fs::write(&file, r#"{"hello": "echo changed"}"#).unwrap();

        let after = get_watched_mtimes(dir.path());
        assert_ne!(before, after, "文件修改后 mtime 快照应变化");
    }

    #[test]
    fn mtimes_detects_file_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.byk.json");
        fs::write(&file, r#"{"hello": "echo hi"}"#).unwrap();

        let before = get_watched_mtimes(dir.path());
        fs::remove_file(&file).unwrap();
        let after = get_watched_mtimes(dir.path());

        assert_ne!(before, after, "文件删除后 mtime 快照应变化");
    }

    // ==================== load_alias_cache 集成测试 ====================

    #[test]
    fn cache_hit_avoids_rescan() {
        let temp = tempfile::tempdir().unwrap();
        let global_dir = temp.path().join("alias");
        let cache_dir = temp.path().join("cache");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        // 写入一个别名文件
        fs::write(
            global_dir.join("test.byk.json"),
            r#"{"greet": "echo hello"}"#,
        )
        .unwrap();

        let cache_file = cache_dir.join("alias.json");

        // 首次加载 → 构建缓存
        let (merged1, files1) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(merged1.contains_key("greet"));
        assert!(!files1.is_empty());
        assert!(cache_file.exists(), "首次加载应写入缓存文件");

        // 二次加载 → 缓存命中
        let (merged2, files2) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert_eq!(merged2.len(), merged1.len());
        assert_eq!(files2.len(), files1.len());
    }

    #[test]
    fn cache_stale_on_mtime_change() {
        let temp = tempfile::tempdir().unwrap();
        let global_dir = temp.path().join("alias");
        let cache_dir = temp.path().join("cache");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        let alias_file = global_dir.join("test.byk.json");
        fs::write(&alias_file, r#"{"greet": "echo hello"}"#).unwrap();

        let cache_file = cache_dir.join("alias.json");

        // 首次加载
        let (merged1, _) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(merged1.contains_key("greet"));

        // 修改文件（等待 mtime 变化）
        thread::sleep(Duration::from_secs(1));
        fs::write(&alias_file, r#"{"farewell": "echo bye"}"#).unwrap();

        // 再次加载 → 应检测到变化并重建
        let (merged2, _) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(
            !merged2.contains_key("greet"),
            "旧别名不应出现在重建后的缓存中"
        );
        assert!(merged2.contains_key("farewell"));
    }

    #[test]
    fn cache_stale_on_file_added() {
        let temp = tempfile::tempdir().unwrap();
        let global_dir = temp.path().join("alias");
        let cache_dir = temp.path().join("cache");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        fs::write(
            global_dir.join("one.byk.json"),
            r#"{"first": "echo one"}"#,
        )
        .unwrap();

        let cache_file = cache_dir.join("alias.json");

        // 首次加载
        let (merged1, _) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(merged1.contains_key("first"));

        // 新增文件
        fs::write(
            global_dir.join("two.byk.json"),
            r#"{"second": "echo two"}"#,
        )
        .unwrap();

        // 再次加载 → mtime 快照多了新文件，应重建
        let (merged2, _) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(merged2.contains_key("first"));
        assert!(merged2.contains_key("second"));
    }

    #[test]
    fn cache_stale_on_file_removed() {
        let temp = tempfile::tempdir().unwrap();
        let global_dir = temp.path().join("alias");
        let cache_dir = temp.path().join("cache");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        let file = global_dir.join("remove_me.byk.json");
        fs::write(&file, r#"{"temp": "echo temp"}"#).unwrap();

        let cache_file = cache_dir.join("alias.json");

        // 首次加载
        let (merged1, _) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(merged1.contains_key("temp"));

        // 删除文件
        fs::remove_file(&file).unwrap();

        // 再次加载 → mtime 快照少了文件，应重建
        let (merged2, _) = load_alias_cache(&cache_file, temp.path(), &global_dir);
        assert!(
            !merged2.contains_key("temp"),
            "已删除文件的别名不应出现"
        );
    }

    #[test]
    fn cache_cwd_change_triggers_rebuild() {
        let temp = tempfile::tempdir().unwrap();
        let global_dir = temp.path().join("alias");
        let cache_dir = temp.path().join("cache");

        let cwd_a = temp.path().join("project_a");
        let cwd_b = temp.path().join("project_b");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&cwd_a).unwrap();
        fs::create_dir_all(&cwd_b).unwrap();

        // 项目 A 有本地别名
        fs::write(
            cwd_a.join(".byk.json"),
            r#"{"build_a": "cargo build"}"#,
        )
        .unwrap();

        // 项目 B 有本地别名
        fs::write(
            cwd_b.join(".byk.json"),
            r#"{"build_b": "npm run build"}"#,
        )
        .unwrap();

        let cache_file = cache_dir.join("alias.json");

        // 在项目 A 加载
        let (merged_a, _) = load_alias_cache(&cache_file, &cwd_a, &global_dir);
        assert!(merged_a.contains_key("build_a"));
        assert!(!merged_a.contains_key("build_b"));

        // 切换到项目 B 加载 → CWD 不同，mtime 快照不匹配，应重建
        let (merged_b, _) = load_alias_cache(&cache_file, &cwd_b, &global_dir);
        assert!(!merged_b.contains_key("build_a"));
        assert!(merged_b.contains_key("build_b"));
    }
}