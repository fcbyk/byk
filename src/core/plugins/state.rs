#![allow(dead_code)] // 临时：byk.json 插件系统（install_plugin）待 npm 迁移后整体删除

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

/// 读取命令状态（从 plugins.cmd.json），并迁移旧版条目。
///
/// 迁移规则（幂等，旧格式 → 新格式）：
/// - `pip-bin` → `bin`，entry 指向 venv bin 目录（Windows 为 Scripts）
/// - `bin` 的插件相对 entry（旧格式，无 token）→ 拼接为绝对路径
///
/// 新格式：bin 类型 entry 一律为 `$this` / `$venv` 在安装期解析出的绝对路径。
///
/// 刻意不检查 venv 是否存在：`bin` / `node` 类型插件命令与 Python 环境无关，
/// venv 被删除后这些命令仍应可用。
/// 依赖 Python 的命令类型（python-m / python）在执行时
/// 按需检查解释器是否存在（见 execute.rs）。
pub fn load_plugin_state(plugins_dir: &Path, venv_dir: &Path) -> CmdState {
    let cmd_file = plugins_dir.join("plugins.cmd.json");
    let mut state: CmdState = json_io::read_json(&cmd_file).unwrap_or_else(empty_cmd_state);
    migrate_cmd_state(&mut state, plugins_dir, venv_dir);
    state
}

/// 就地迁移旧版命令状态（幂等）：
/// `pip-bin` 合并进 `bin`，bin 的 entry 统一为绝对路径。
fn migrate_cmd_state(state: &mut CmdState, plugins_dir: &Path, venv_dir: &Path) {
    let venv_bin = venv_dir.join(VENV_BIN);
    for cmd in state.commands.values_mut() {
        match cmd.cmd_type.as_str() {
            // pip-bin 已合并进 bin：旧 entry 是 venv bin 下的裸文件名
            "pip-bin" => {
                cmd.cmd_type = "bin".to_string();
                cmd.entry = venv_bin.join(&cmd.entry).to_string_lossy().into_owned();
            }
            // 旧 bin entry 是相对于 plugins/ 的路径（含 plugin_key 前缀）
            "bin" if !Path::new(&cmd.entry).is_absolute() => {
                cmd.entry = plugins_dir
                    .join(&cmd.entry)
                    .to_string_lossy()
                    .into_owned();
            }
            _ => {}
        }
    }
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
    fn load_plugin_state_without_venv_still_loads_commands() {
        // 回归测试：venv 不存在时（如 `byk remove py` 之后），
        // bin 类型插件命令仍必须从 plugins.cmd.json 加载出来
        let tmp = std::env::temp_dir().join("fcbyk_test_state3");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let plugins_dir = tmp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        let cmd_file = plugins_dir.join("plugins.cmd.json");
        let mut commands = HashMap::new();
        commands.insert(
            "lansend".to_string(),
            PluginCommand {
                cmd_type: "bin".to_string(),
                entry: "lansend/lansend".to_string(),
                desc: "局域网文件共享".to_string(),
                args: None,
            },
        );
        let state = CmdState {
            commands,
            python_executable: None,
        };
        crate::utils::json_io::write_json(&cmd_file, &state);

        // venv 目录不存在
        let venv_dir = tmp.join("py-venv").join(".venv");
        let loaded = load_plugin_state(&plugins_dir, &venv_dir);
        assert!(loaded.commands.contains_key("lansend"));
        let cmd = loaded.commands.get("lansend").unwrap();
        assert_eq!(cmd.cmd_type, "bin");
        // 旧格式插件相对 entry → 绝对路径
        assert_eq!(
            cmd.entry,
            plugins_dir.join("lansend/lansend").to_string_lossy()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_plugin_state_no_file() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state4");
        let plugins_dir = tmp.join("plugins");
        let venv_dir = tmp.join("py-venv").join(".venv");

        let state = load_plugin_state(&plugins_dir, &venv_dir);
        assert!(state.commands.is_empty());
    }

    #[test]
    fn load_plugin_state_with_data() {
        let tmp = std::env::temp_dir().join("fcbyk_test_state5");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let plugins_dir = tmp.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        let venv_dir = tmp.join("py-venv").join(".venv");

        let cmd_file = plugins_dir.join("plugins.cmd.json");
        let mut commands = HashMap::new();
        commands.insert(
            "test-cmd".to_string(),
            PluginCommand {
                cmd_type: "python-m".to_string(),
                entry: "mymod".to_string(),
                desc: "test desc".to_string(),
                args: None,
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
            "python-m"
        );
        // 非 bin 类型 entry 不受迁移影响
        assert_eq!(loaded.commands.get("test-cmd").unwrap().entry, "mymod");
    }

    // ==================== migrate_cmd_state ====================

    fn cmd_entry(state: &CmdState, name: &str) -> (String, String) {
        let cmd = state.commands.get(name).unwrap();
        (cmd.cmd_type.clone(), cmd.entry.clone())
    }

    #[test]
    fn migrate_pip_bin_to_bin_with_venv_path() {
        // 旧 pip-bin：entry 是 venv bin 下的裸文件名 → bin + venv bin 绝对路径
        let mut state = CmdState {
            commands: HashMap::new(),
            python_executable: None,
        };
        state.commands.insert(
            "hello".to_string(),
            PluginCommand {
                cmd_type: "pip-bin".to_string(),
                entry: "hello".to_string(),
                desc: String::new(),
                args: None,
            },
        );
        let plugins_dir = Path::new("/home/u/.byk/plugins");
        let venv_dir = Path::new("/home/u/.byk/py-venv/.venv");

        migrate_cmd_state(&mut state, plugins_dir, venv_dir);

        let (cmd_type, entry) = cmd_entry(&state, "hello");
        assert_eq!(cmd_type, "bin");
        assert_eq!(
            entry,
            venv_dir.join(VENV_BIN).join("hello").to_string_lossy()
        );
    }

    #[test]
    fn migrate_relative_bin_entry_to_absolute() {
        // 旧 bin：entry 是 plugins/ 下的相对路径 → 绝对路径
        let mut state = CmdState {
            commands: HashMap::new(),
            python_executable: None,
        };
        state.commands.insert(
            "tool".to_string(),
            PluginCommand {
                cmd_type: "bin".to_string(),
                entry: "my-tool/bin/my-tool".to_string(),
                desc: String::new(),
                args: None,
            },
        );
        let plugins_dir = Path::new("/home/u/.byk/plugins");
        let venv_dir = Path::new("/home/u/.byk/py-venv/.venv");

        migrate_cmd_state(&mut state, plugins_dir, venv_dir);

        let (cmd_type, entry) = cmd_entry(&state, "tool");
        assert_eq!(cmd_type, "bin");
        assert_eq!(
            entry,
            plugins_dir.join("my-tool/bin/my-tool").to_string_lossy()
        );
    }

    #[test]
    fn migrate_keeps_absolute_bin_entry() {
        // 新格式（安装期已解析为绝对路径）不受影响；幂等
        let mut state = CmdState {
            commands: HashMap::new(),
            python_executable: None,
        };
        state.commands.insert(
            "tool".to_string(),
            PluginCommand {
                cmd_type: "bin".to_string(),
                entry: "/home/u/.byk/py-venv/.venv/bin/tool".to_string(),
                desc: String::new(),
                args: None,
            },
        );
        let plugins_dir = Path::new("/home/u/.byk/plugins");
        let venv_dir = Path::new("/home/u/.byk/py-venv/.venv");

        migrate_cmd_state(&mut state, plugins_dir, venv_dir);
        migrate_cmd_state(&mut state, plugins_dir, venv_dir);

        let (cmd_type, entry) = cmd_entry(&state, "tool");
        assert_eq!(cmd_type, "bin");
        assert_eq!(entry, "/home/u/.byk/py-venv/.venv/bin/tool");
    }

    #[test]
    fn migrate_ignores_non_bin_types() {
        // python / python-m / node 的 entry 语义不变
        let mut state = CmdState {
            commands: HashMap::new(),
            python_executable: None,
        };
        for (name, cmd_type, entry) in [
            ("a", "python", "myplugin/runner.py"),
            ("b", "python-m", "mymod.cli"),
            ("c", "node", "myplugin/main.js"),
        ] {
            state.commands.insert(
                name.to_string(),
                PluginCommand {
                    cmd_type: cmd_type.to_string(),
                    entry: entry.to_string(),
                    desc: String::new(),
                    args: None,
                },
            );
        }
        let plugins_dir = Path::new("/home/u/.byk/plugins");
        let venv_dir = Path::new("/home/u/.byk/py-venv/.venv");

        migrate_cmd_state(&mut state, plugins_dir, venv_dir);

        assert_eq!(cmd_entry(&state, "a").1, "myplugin/runner.py");
        assert_eq!(cmd_entry(&state, "b").1, "mymod.cli");
        assert_eq!(cmd_entry(&state, "c").1, "myplugin/main.js");
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
                pip_keep: None,
                assets: vec![],
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