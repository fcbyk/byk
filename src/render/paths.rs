//! `--paths` 输出：CLI 用到的目录与解释器位置。

use colored::Colorize;

use crate::core::paths::PathLayout;
use crate::core::pip::{PYTHON_BIN, VENV_BIN};

/// 渲染 CLI 路径和文件位置。
pub fn render(layout: &PathLayout) {
    println!();

    let python_exe = layout.venv_dir.join(VENV_BIN).join(PYTHON_BIN);

    println!("{}: {}", "Python".yellow(), python_exe.display());
    println!("{}", "-".repeat(29).dimmed());
    println!("{}: {}", "CLI Home".yellow(), layout.root_dir.display());
    println!("{}: {}", "Cache".yellow(), layout.cache_dir.display());
    println!("{}: {}", "Alias".yellow(), layout.alias_dir.display());
    println!("{}: {}", "Plugins".yellow(), layout.plugins_dir.display());
    println!("{}: {}", "Python Venv".yellow(), layout.py_venv_dir.display());

    println!();
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_does_not_panic() {
        let layout = PathLayout::new();
        render(&layout);
    }
}
