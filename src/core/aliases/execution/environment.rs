//! 别名执行环境与 PATH 前缀。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::utils::path::{canonical, expand_home};

/// 把 `$paths` 里的相对条目按配置文件目录展开为绝对路径。
///
/// `build_alias_env` 仍会把相对条目按命令落地目录兜底解析，
/// 但在别名定义里写下的相对路径应当跟着配置文件走——否则从子目录
/// 调用 `byk` 时，这些目录会被静默跳过。
pub(super) fn absolutize_paths(paths: &[String], config_dir: Option<&Path>) -> Vec<String> {
    paths
        .iter()
        .map(|raw| {
            let expanded = expand_home(raw);
            if expanded.is_relative()
                && let Some(base) = config_dir
            {
                return base.join(&expanded).to_string_lossy().into_owned();
            }
            expanded.to_string_lossy().into_owned()
        })
        .collect()
}

/// 解析工作目录，支持 `~` 展开和基于配置文件目录的相对路径解析。
pub(super) fn resolve_working_dir(cwd: Option<&str>, base_dir: Option<&Path>) -> PathBuf {
    let raw = match cwd {
        Some(s) if !s.is_empty() => s,
        _ => return std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let path = expand_home(raw);

    // 相对路径以配置文件所在目录为基准解析
    if path.is_relative()
        && let Some(base) = base_dir
    {
        return canonical(&base.join(&path));
    }
    path
}

/// 计算需要前置到 PATH 的目录列表（`$paths` + `node_modules/.bin`）。
///
/// 与 `build_alias_env` 共用同一套前缀规则，保证 `byk __api__ resolve` 返回的
/// `paths` 与实际执行时的 PATH 完全一致。相对条目以 `working_dir` 为基准解析，
/// `~` 已展开，不存在的目录静默跳过。
pub(super) fn build_path_prefixes(working_dir: &Path, custom_paths: &[String]) -> Vec<String> {
    let mut prefixes: Vec<String> = Vec::new();

    // 解析并前置 $paths 条目（保持配置中的顺序，最前的优先级最高）
    for raw in custom_paths {
        let expanded = expand_home(raw);
        let resolved = if expanded.is_relative() {
            working_dir.join(&expanded)
        } else {
            expanded
        };
        if resolved.is_dir() {
            prefixes.push(resolved.to_string_lossy().into_owned());
        }
    }

    let node_bin = working_dir.join("node_modules").join(".bin");
    if node_bin.is_dir() {
        prefixes.push(node_bin.to_string_lossy().into_owned());
    }

    prefixes
}

/// 构造别名执行环境，将自定义路径和 node_modules/.bin 前置到 PATH。
///
/// `custom_paths` 来自别名文件的 `$paths` 字段，支持 `~` 展开和基于
/// `working_dir` 的相对路径解析。非字符串元素已在扫描阶段过滤。
pub(super) fn build_alias_env(
    working_dir: &Path,
    custom_paths: &[String],
) -> HashMap<String, String> {
    let mut env_map = HashMap::new();
    let existing_path = std::env::var("PATH").unwrap_or_default();

    let prefixes = build_path_prefixes(working_dir, custom_paths);

    if prefixes.is_empty() {
        env_map.insert("PATH".to_string(), existing_path);
    } else {
        #[cfg(windows)]
        let sep = ";";
        #[cfg(not(windows))]
        let sep = ":";
        let prepended = prefixes.join(sep);
        env_map.insert(
            "PATH".to_string(),
            format!("{}{}{}", prepended, sep, existing_path),
        );
    }
    env_map
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_cwd_none_returns_current_dir() {
        let result = resolve_working_dir(None, None);
        assert!(result.is_absolute());
    }

    #[test]
    fn resolve_cwd_empty_returns_current_dir() {
        let result = resolve_working_dir(Some(""), None);
        assert!(result.is_absolute());
    }

    #[test]
    fn resolve_cwd_absolute_path() {
        let result = resolve_working_dir(Some("/tmp/project"), None);
        assert_eq!(result, PathBuf::from("/tmp/project"));
    }

    #[test]
    fn resolve_cwd_relative_with_base_dir() {
        let base = PathBuf::from("/home/user/config");
        let result = resolve_working_dir(Some("project"), Some(&base));
        assert_eq!(result, PathBuf::from("/home/user/config/project"));
    }

    #[test]
    fn resolve_cwd_relative_with_parent_dir() {
        let base = PathBuf::from("/home/user/config");
        let result = resolve_working_dir(Some("../project"), Some(&base));
        // ../project 从 /home/user/config 出发 → /home/user/project
        // canonicalize 在测试环境中可能返回实际路径，检查以 project 结尾
        let s = result.to_string_lossy();
        assert!(s.ends_with("project") || s.ends_with("project/"));
    }

    #[test]
    fn resolve_cwd_relative_no_base_dir() {
        // 无 base_dir 时相对路径直接返回（不解析）
        let result = resolve_working_dir(Some("relative/path"), None);
        assert_eq!(result, PathBuf::from("relative/path"));
    }

    #[test]
    fn env_no_custom_paths_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let env = build_alias_env(dir.path(), &[]);
        let path = env.get("PATH").unwrap();
        let existing = std::env::var("PATH").unwrap_or_default();
        assert_eq!(path, &existing);
    }

    #[test]
    fn env_custom_paths_prepended() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();

        let env = build_alias_env(dir.path(), &[bin_dir.to_string_lossy().into_owned()]);
        let path = env.get("PATH").unwrap();
        assert!(path.starts_with(&bin_dir.to_string_lossy().into_owned()));
    }

    #[test]
    fn env_skips_nonexistent_custom_paths() {
        let dir = tempfile::tempdir().unwrap();
        let env = build_alias_env(
            dir.path(),
            &["/nonexistent/path/that/does/not/exist".to_string()],
        );
        let path = env.get("PATH").unwrap();
        assert!(!path.contains("/nonexistent/path"));
    }

    #[test]
    fn env_relative_custom_paths_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("scripts");
        std::fs::create_dir(&scripts).unwrap();

        let env = build_alias_env(dir.path(), &["scripts".to_string()]);
        let path = env.get("PATH").unwrap();
        assert!(path.contains(&scripts.to_string_lossy().into_owned()));
    }

    #[test]
    fn env_multiple_custom_paths_order_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();

        let env = build_alias_env(
            dir.path(),
            &[
                a.to_string_lossy().into_owned(),
                b.to_string_lossy().into_owned(),
            ],
        );
        let path = env.get("PATH").unwrap();
        let a_pos = path.find(&a.to_string_lossy().into_owned()).unwrap();
        let b_pos = path.find(&b.to_string_lossy().into_owned()).unwrap();
        assert!(a_pos < b_pos, "配置中排前的路径应更靠前");
    }

    #[test]
    fn env_node_modules_bin_appended_after_custom() {
        let dir = tempfile::tempdir().unwrap();
        let nm_bin = dir.path().join("node_modules").join(".bin");
        std::fs::create_dir_all(&nm_bin).unwrap();
        let custom = dir.path().join("custom");
        std::fs::create_dir(&custom).unwrap();

        let env = build_alias_env(dir.path(), &[custom.to_string_lossy().into_owned()]);
        let path = env.get("PATH").unwrap();
        let custom_pos = path.find(&custom.to_string_lossy().into_owned()).unwrap();
        let nm_pos = path.find(&nm_bin.to_string_lossy().into_owned()).unwrap();
        assert!(custom_pos < nm_pos, "$paths 应在 node_modules/.bin 之前");
    }
}
