//! NPM Commands 渲染。
//!
//! 将 NpmPackageInfo 列表转为对齐的终端展示行并输出。

use colored::Colorize;

use crate::core::node::NpmPackageInfo;
use crate::utils::display;

/// 渲染 NPM Commands 区块到终端。
pub fn render(packages: &[NpmPackageInfo]) {
    let lines = format_lines(packages);
    if lines.is_empty() {
        return;
    }

    println!();
    println!("{}", "NPM Commands:".green().bold());
    for (bin_names, line) in &lines {
        let rest = &line[2 + bin_names.len()..];
        print!("  {}", bin_names.cyan().bold());
        println!("{}", rest);
    }
}

/// 将 NPM 包信息格式化为对齐的展示行。
///
/// 行格式: "  {bin_names}{padding}  {name}@{version}"
fn format_lines(packages: &[NpmPackageInfo]) -> Vec<(String, String)> {
    if packages.is_empty() {
        return Vec::new();
    }

    let entries: Vec<(String, String)> = packages
        .iter()
        .map(|pkg| {
            let bin_names = pkg.bins.join(", ");
            let name_ver = format!("{}@{}", pkg.name, pkg.version);
            (bin_names, name_ver)
        })
        .collect();

    display::align_kv_pairs(&entries, "  ")
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, bins: Vec<&str>) -> NpmPackageInfo {
        NpmPackageInfo {
            name: name.into(),
            version: version.into(),
            bins: bins.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn npm_format_lines_empty() {
        assert!(format_lines(&[]).is_empty());
    }

    #[test]
    fn npm_format_lines_single_package_single_bin() {
        let packages = vec![pkg("eslint", "8.50.0", vec!["eslint"])];
        let result = format_lines(&packages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "eslint");
        // 格式: "  eslint  eslint@8.50.0"
        assert_eq!(result[0].1, "  eslint  eslint@8.50.0");
    }

    #[test]
    fn npm_format_lines_single_package_multiple_bins() {
        let packages = vec![pkg(
            "typescript",
            "5.3.0",
            vec!["tsc", "tsserver"],
        )];
        let result = format_lines(&packages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "tsc, tsserver");
        assert!(result[0].1.contains("typescript@5.3.0"));
    }

    #[test]
    fn npm_format_lines_multiple_packages_aligned() {
        let packages = vec![
            pkg("pkg-a", "1.0.0", vec!["a"]),
            pkg("pkg-longname", "2.0.0", vec!["longcmd"]),
        ];
        let result = format_lines(&packages);
        assert_eq!(result.len(), 2);
        // "a" 应该补齐到和 "longcmd" 一样的宽度
        assert!(result[0].1.starts_with("  a"));
        assert!(result[0].1.contains("pkg-a@1.0.0"));
        assert!(result[1].1.contains("pkg-longname@2.0.0"));
    }
}