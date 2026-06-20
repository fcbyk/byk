/// 插件缓存与执行。
///
/// 插件通过 `byk add` 安装，缓存到 plugins.json。
/// 执行时直接调用 `python -m <模块>` 透传参数。

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, exit};

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

/// 单个插件的包信息（install 时写入）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageInfo {
    /// pip 包名（来自 byk.json 的 install.name）
    pub name: String,
    /// 该插件注册的命令名列表
    pub commands: Vec<String>,
    /// 来源仓库：None = 中心仓库，Some("user/repo") = 社区仓库
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// 插件缓存（持久化到 cache/plugins.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCache {
    /// 已安装插件的命令列表
    pub commands: HashMap<String, PluginCommand>,
    /// Python 解释器路径（venv 内的 python）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python_executable: Option<String>,
    /// 插件 key → 包信息映射
    #[serde(default)]
    pub packages: HashMap<String, PackageInfo>,
}

// ---------------------------------------------------------------------------
// 空缓存
// ---------------------------------------------------------------------------

/// 构造空插件缓存（venv 不存在时使用）。
pub fn empty_plugin_cache() -> PluginCache {
    PluginCache {
        commands: HashMap::new(),
        python_executable: None,
        packages: HashMap::new(),
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
// 缓存加载
// ---------------------------------------------------------------------------

/// 读取插件缓存（从 plugins.json 直接读取，不做扫描或过期检测）。
///
/// - venv 不存在 → 返回空缓存
/// - 无缓存文件 → 返回空缓存
/// - 有缓存文件 → 直接返回
pub fn load_plugin_cache(cache_dir: &Path, venv_dir: &Path) -> PluginCache {
    if !venv_dir.is_dir() {
        return empty_plugin_cache();
    }

    let cache_file = cache_dir.join("plugins.json");
    json_io::read_json(&cache_file).unwrap_or_else(empty_plugin_cache)
}

// ---------------------------------------------------------------------------
// 命令执行
// ---------------------------------------------------------------------------

/// 将插件命令转发给 Python 执行。
///
/// 直接通过 `python -m <module> <args>` 调用。
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
