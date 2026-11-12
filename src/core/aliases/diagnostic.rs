//! 别名文件扫描诊断：收集现有检查发现的问题，供 banner 汇总与 `--check` 展示。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 诊断级别；`Error` 表示文件或别名不可用，`Warning` 表示仍可用但可疑。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Error,
    Warning,
}

/// 一条扫描诊断；`path` 始终是触发扫描的那个别名文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: Level,
    pub path: PathBuf,
    pub message: String,
}

impl Diagnostic {
    pub fn error(path: &Path, message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    pub fn warning(path: &Path, message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

/// 一次扫描的汇总：扫描了多少文件、发现了哪些问题。
#[derive(Clone, Debug, Default)]
pub struct DiagnosticReport {
    pub scanned: usize,
    pub items: Vec<Diagnostic>,
}

impl DiagnosticReport {
    pub fn is_clean(&self) -> bool {
        self.items.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.level == Level::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.level == Level::Warning)
            .count()
    }

    /// 至少有一条诊断的不同文件数。
    pub fn problem_file_count(&self) -> usize {
        self.items
            .iter()
            .map(|d| d.path.as_path())
            .collect::<HashSet<_>>()
            .len()
    }
}
