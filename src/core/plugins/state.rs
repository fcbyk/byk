//! 插件状态加载与持久化。
//!
//! 从 plugins.cmd.json 和 plugins.pkg.json 读写插件状态。

use std::collections::HashMap;
use std::path::Path;

use super::types::*;
use crate::utils::json_io;

// ---------------------------------------------------------------------------
// 空状态
// ---------------------------------------------------------------------------

/// 构造空命令状态。
pub fn empty_cmd_state() -> CmdState {
    CmdState {
        commands: HashMap::new(),
        python_executable: None,
    }
}

/// 构造空包状态。
pub fn empty_pkg_state() -> PkgState {
    HashMap::new()
}

// ---------------------------------------------------------------------------
// Python 解释器路径
// ---------------------------------------------------------------------------

/// 获取 Python 解释器路径。
///
/// 优先级：
/// 1. plugins.cmd.json 中的 `python_executable`
/// 2. 如果 venv 存在 → `venv/bin/python`
pub(crate) fn get_python_executable(plugins_dir: &Path, venv_dir: &Path) -> String {
    let cmd_file = plugins_dir.join("plugins.cmd.json");
    if let Some(data) = json_io::read_json::<CmdState>(&cmd_file)
        && let Some(exe) = data.python_executable {
            return exe;
        }

    let venv_python = venv_dir.join(VENV_BIN).join(PYTHON_BIN);
    venv_python.to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// 状态加载
// ---------------------------------------------------------------------------

/// 读取命令状态（从 plugins.cmd.json）。
///
/// - venv 不存在 → 返回空状态
/// - 无状态文件 → 返回空状态
/// - 有状态文件 → 直接返回
pub fn load_plugin_state(plugins_dir: &Path, venv_dir: &Path) -> CmdState {
    if !venv_dir.is_dir() {
        return empty_cmd_state();
    }

    let cmd_file = plugins_dir.join("plugins.cmd.json");
    json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state)
}

/// 读取包状态（从 plugins.pkg.json）。
pub fn load_pkg_state(plugins_dir: &Path) -> PkgState {
    let pkg_file = plugins_dir.join("plugins.pkg.json");
    json_io::read_json(&pkg_file).unwrap_or_else(empty_pkg_state)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ==================== empty_cmd_state ====================

    #[test]
    fn empty_cmd_state_is_empty() {
        let state = empty_cmd_state();
        assert!(state.commands.is_empty());
        assert!(state.python_executable.is_none());
    }

    // ==================== empty_pkg_state ====================

    #[test]
    fn empty_pkg_state_is_empty() {
        let state = empty_pkg_state();
        assert!(state.is_empty());
    }

    // ==================== get_python_executable ====================

    #[test]
    fn get_python_fallback_to_venv_path() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state");
        let _ = std::fs::create_dir_all(&tmp);
        let plugins_dir = tmp.join("plugins");
        // No plugins.cmd.json → fallback to venv path
        let venv_dir = tmp.join(".venv");
        let result = get_python_executable(&plugins_dir, &venv_dir);
        assert!(result.contains("python") || result.contains("python.exe"));
        assert!(result.contains(".venv"));
    }

    #[test]
    fn get_python_from_cmd_state_file() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state2");
        let _ = std::fs::create_dir_all(&tmp);
        let plugins_dir = tmp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let cmd_file = plugins_dir.join("plugins.cmd.json");
        let state = CmdState {
            commands: HashMap::new(),
            python_executable: Some("/custom/python".to_string()),
        };
        crate::utils::json_io::write_json(&cmd_file, &state);

        let venv_dir = tmp.join(".venv");
        let result = get_python_executable(&plugins_dir, &venv_dir);
        assert_eq!(result, "/custom/python");
    }

    // ==================== load_plugin_state ====================

    #[test]
    fn load_plugin_state_venv_not_exists() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state3");
        let plugins_dir = tmp.join("plugins");
        let venv_dir = tmp.join("nonexistent_venv");
        let state = load_plugin_state(&plugins_dir, &venv_dir);
        assert!(state.commands.is_empty());
    }

    #[test]
    fn load_plugin_state_no_file() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state4");
        let _ = std::fs::create_dir_all(&tmp);
        let plugins_dir = tmp.join("plugins");
        let venv_dir = tmp.join(".venv");
        std::fs::create_dir_all(&venv_dir).unwrap();

        let state = load_plugin_state(&plugins_dir, &venv_dir);
        assert!(state.commands.is_empty());
    }

    #[test]
    fn load_plugin_state_with_data() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state5");
        let _ = std::fs::create_dir_all(&tmp);
        let plugins_dir = tmp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let venv_dir = tmp.join(".venv");
        std::fs::create_dir_all(&venv_dir).unwrap();

        let cmd_file = plugins_dir.join("plugins.cmd.json");
        let mut commands = HashMap::new();
        commands.insert(
            "test-cmd".to_string(),
            PluginCommand {
                cmd_type: "py-module".to_string(),
                entry: "mymod".to_string(),
                desc: "test desc".to_string(),
            },
        );
        let state = CmdState {
            commands,
            python_executable: None,
        };
        crate::utils::json_io::write_json(&cmd_file, &state);

        let loaded = load_plugin_state(&plugins_dir, &venv_dir);
        assert!(loaded.commands.contains_key("test-cmd"));
        assert_eq!(
            loaded.commands.get("test-cmd").unwrap().cmd_type,
            "py-module"
        );
    }

    // ==================== load_pkg_state ====================

    #[test]
    fn load_pkg_state_empty() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state6");
        let plugins_dir = tmp.join("plugins");
        let state = load_pkg_state(&plugins_dir);
        assert!(state.is_empty());
    }

    #[test]
    fn load_pkg_state_with_data() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state7");
        let _ = std::fs::create_dir_all(&tmp);
        let plugins_dir = tmp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let pkg_file = plugins_dir.join("plugins.pkg.json");
        let mut pkg_state: PkgState = HashMap::new();
        pkg_state.insert(
            "myplugin".to_string(),
            PkgEntry {
                source: Some("user/repo".to_string()),
                pip: Some(vec!["requests".to_string()]),
                scripts: vec![],
                bins: vec![],
                bins_tar: vec![],
                commands: vec!["run".to_string()],
            },
        );
        crate::utils::json_io::write_json(&pkg_file, &pkg_state);

        let loaded = load_pkg_state(&plugins_dir);
        assert!(loaded.contains_key("myplugin"));
        let entry = loaded.get("myplugin").unwrap();
        assert_eq!(entry.source, Some("user/repo".to_string()));
        assert_eq!(entry.pip, Some(vec!["requests".to_string()]));
        assert_eq!(entry.commands, vec!["run"]);
    }

    #[test]
    fn load_pkg_state_invalid_json_returns_empty() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state8");
        let _ = std::fs::create_dir_all(&tmp);
        let plugins_dir = tmp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let pkg_file = plugins_dir.join("plugins.pkg.json");
        std::fs::write(&pkg_file, "not valid json").unwrap();

        let state = load_pkg_state(&plugins_dir);
        assert!(state.is_empty());
    }
}