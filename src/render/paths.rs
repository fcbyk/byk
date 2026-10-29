//! `--paths` 输出：byk 实际使用的两个目录。

use colored::Colorize;

use crate::core::paths::PathLayout;

/// 渲染 CLI 目录位置。
pub fn render(layout: &PathLayout) {
    println!();

    println!("{}: {}", "CLI Home".yellow(), layout.root_dir.display());
    println!("{}: {}", "Alias".yellow(), layout.alias_dir.display());

    println!();
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
