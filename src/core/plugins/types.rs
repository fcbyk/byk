//! 平台常量（venv 内 bin / python 可执行文件名）。

/// venv 内 bin 目录名。
#[cfg(windows)]
pub const VENV_BIN: &str = "Scripts";
#[cfg(not(windows))]
pub const VENV_BIN: &str = "bin";

/// venv 内 Python 可执行文件名。
#[cfg(windows)]
pub const PYTHON_BIN: &str = "python.exe";
#[cfg(not(windows))]
pub const PYTHON_BIN: &str = "python";
