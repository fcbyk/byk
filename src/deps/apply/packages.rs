//! Python 与 npm 包依赖的隔离安装和状态查询。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::install::first_stderr;
use crate::deps::spec::{NODE_MODULES, VENV_DIR, venv_bin};

// ── pip：专属目录里的 Python 虚拟环境 ──────────────────────────────────

/// 创建 venv 用的系统解释器：Unix 是 `python3`，Windows 是 `python`。
pub(crate) fn system_python() -> &'static str {
    #[cfg(windows)]
    {
        "python"
    }
    #[cfg(not(windows))]
    {
        "python3"
    }
}

/// venv 内的 python 可执行文件路径。
pub(crate) fn venv_python(venv: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        venv.join("Scripts").join("python.exe")
    }
    #[cfg(not(windows))]
    {
        venv.join("bin").join("python")
    }
}

/// 专属目录下的 venv 根目录。
pub(crate) fn venv_dir(deps_dir: &Path) -> PathBuf {
    deps_dir.join(VENV_DIR)
}

/// 在专属目录里建 venv 并 `pip install` 声明的包。
///
/// venv 只在缺失时创建（幂等）；`pip install` 每次执行（v1：不判断是否已装，
/// 依赖 pip 自身的收敛能力，先把链路跑通）。
pub(crate) fn install_pip(deps_dir: &Path, packages: &[String]) -> Result<(), String> {
    fs::create_dir_all(deps_dir)
        .map_err(|error| format!("cannot create {}: {error}", deps_dir.display()))?;

    let venv = venv_dir(deps_dir);
    if !venv_bin(deps_dir).is_dir() {
        eprintln!(
            "Creating a Python virtual environment in {}:",
            venv.display()
        );
        let output = Command::new(system_python())
            .args(["-m", "venv"])
            .arg(&venv)
            .output()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => format!(
                    "{} is required to create a Python virtual environment\nHint: install Python 3 and retry",
                    system_python()
                ),
                _ => format!("cannot run \"{}\": {error}", system_python()),
            })?;
        if !output.status.success() {
            return Err(format!(
                "cannot create the virtual environment: \"{}\" {}",
                system_python(),
                first_stderr(&output.stderr)
            ));
        }
    }

    let python = venv_python(&venv);
    eprintln!("Installing Python packages into {}:", venv.display());
    let status = Command::new(&python)
        .args(["-m", "pip", "install"])
        .args(packages)
        .status()
        .map_err(|error| format!("cannot run \"{}\": {error}", python.display()))?;
    if !status.success() {
        return Err(format!(
            "pip install failed (exit {})",
            status.code().unwrap_or(1)
        ));
    }
    Ok(())
}

/// 把 pip 包名规范成 PEP 503 形式：小写，`_` / `.` / `-` 统一成 `-`。
fn normalize_pip_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '_' | '.' => '-',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// 从 PEP 508 规格里抠出包名（丢弃 extras、版本约束与环境标记）。
///
/// 名字由字母数字与 `.` `-` `_` 组成，遇到 `[` / `>=` / `;` 等即结束。
/// 结果经过 [`normalize_pip_name`] 规范化，便于和 `pip list` 比对。
pub(crate) fn pip_package_name(spec: &str) -> String {
    let raw: String = spec
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    normalize_pip_name(&raw)
}

/// venv 里已安装的包名集合（规范化）。venv 不存在或查询失败时返回空集合。
pub(crate) fn installed_pip_packages(deps_dir: &Path) -> Vec<String> {
    let python = venv_python(&venv_dir(deps_dir));
    if !python.is_file() {
        return Vec::new();
    }

    let output = match Command::new(&python)
        .args(["-m", "pip", "list", "--format=json"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let Ok(value) = serde_json::from_slice::<Value>(&output.stdout) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(normalize_pip_name)
        .collect()
}

// ── npm：专属目录里的 node_modules ─────────────────────────────────────

/// 在专属目录里 `npm install` 声明的包。
///
/// 不建虚拟环境：npm 直接把包装进专属目录的 node_modules，并在根下生成
/// package.json / package-lock.json。主动写固定 name 的 package.json，避免
/// npm 拿目录名（如 `@hello`）当包名。幂等交给 npm（v1：每次直接执行）。
pub(crate) fn install_npm(deps_dir: &Path, packages: &[String]) -> Result<(), String> {
    fs::create_dir_all(deps_dir)
        .map_err(|error| format!("cannot create {}: {error}", deps_dir.display()))?;
    ensure_package_json(deps_dir)?;

    eprintln!("Installing npm packages into {}:", deps_dir.display());
    let status = Command::new("npm")
        .args(["install"])
        .args(packages)
        .current_dir(deps_dir)
        .status()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                "npm is required to install npm packages\nHint: install Node.js and retry"
                    .to_string()
            }
            _ => format!("cannot run \"npm\": {error}"),
        })?;
    if !status.success() {
        return Err(format!(
            "npm install failed (exit {})",
            status.code().unwrap_or(1)
        ));
    }
    Ok(())
}

/// 专属目录下没有 package.json 时写一个固定 name 的最小版本，`private` 防止误发。
fn ensure_package_json(deps_dir: &Path) -> Result<(), String> {
    let path = deps_dir.join("package.json");
    if path.exists() {
        return Ok(());
    }
    fs::write(
        &path,
        "{\n  \"name\": \"byk-deps\",\n  \"private\": true\n}\n",
    )
    .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// 从 npm 规格里抠出包名（丢弃版本约束），形如 `chalk` 或 `@scope/pkg`。
///
/// node_modules 里的目录名就是包名本身，直接拿它拼路径判断装没装。
pub(crate) fn npm_package_name(spec: &str) -> String {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix('@') {
        // scope 包：@scope/pkg[@version]
        let (scope, rest) = rest.split_once('/').unwrap_or((rest, ""));
        let name = rest.split('@').next().unwrap_or("");
        format!("@{scope}/{name}")
    } else {
        spec.split('@').next().unwrap_or("").to_string()
    }
}

/// npm 包是否已装：看 node_modules 下有没有对应的包目录。
pub(crate) fn npm_package_installed(deps_dir: &Path, spec: &str) -> bool {
    deps_dir
        .join(NODE_MODULES)
        .join(npm_package_name(spec))
        .is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn npm_package_name_strips_version() {
        assert_eq!(npm_package_name("chalk"), "chalk");
        assert_eq!(npm_package_name("commander@^12"), "commander");
        assert_eq!(npm_package_name("  padded@1.2.3  "), "padded");
    }

    #[test]
    fn npm_package_name_handles_scoped() {
        assert_eq!(npm_package_name("@scope/pkg"), "@scope/pkg");
        assert_eq!(npm_package_name("@scope/pkg@1.0"), "@scope/pkg");
    }

    #[test]
    fn pip_package_name_strips_version_and_extras() {
        assert_eq!(pip_package_name("fcbyk-cli"), "fcbyk-cli");
        assert_eq!(pip_package_name("click>=8.0"), "click");
        assert_eq!(pip_package_name("requests[socks]>=2.0"), "requests");
        assert_eq!(pip_package_name("pkg; python_version < '3.8'"), "pkg");
        assert_eq!(pip_package_name("  padded==1.2.3  "), "padded");
    }

    #[test]
    fn pip_package_name_normalizes_separators_and_case() {
        assert_eq!(pip_package_name("PyYAML"), "pyyaml");
        assert_eq!(pip_package_name("Foo.Bar"), "foo-bar");
        assert_eq!(pip_package_name("foo_bar"), "foo-bar");
    }
}
