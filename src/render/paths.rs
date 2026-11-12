//! `--paths` 输出：byk 实际使用的目录，以及两处目录下的别名文件。

use std::path::PathBuf;

use colored::Colorize;

use crate::core::aliases;
use crate::core::paths::PathLayout;

/// 文件分组之间的分隔线；不带标题，作用域靠 key 的 `@` / `@@` 前缀区分。
const SEPARATOR: &str = "---------------------------";

/// 渲染 CLI 目录与别名文件位置。
pub fn render(layout: &PathLayout) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let global = aliases::list_alias_files(&layout.alias_dir, true);
    let local = aliases::list_alias_files(&cwd, false);

    println!();
    println!("{}: {}", "CLI Home".yellow(), layout.root_dir.display());
    println!("{}: {}", "Alias".yellow(), layout.alias_dir.display());
    println!("{}: {}", "Workdir".yellow(), cwd.display());

    render_files(&global);
    render_files(&local);
    println!();
}

/// 输出一组文件；空组直接跳过，避免没有文件时留下孤立的分隔线。
fn render_files(files: &[(String, PathBuf)]) {
    if files.is_empty() {
        return;
    }

    println!("{}", SEPARATOR.dimmed());
    for (key, path) in files {
        println!("{}: {}", key.yellow(), path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_does_not_panic() {
        let layout = PathLayout::new();
        render(&layout);
    }
}
