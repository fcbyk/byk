/// 别名渲染。
///
/// 将合并后的别名配置转为对齐的终端展示行并输出，
/// 包含 CJK 显示宽度对齐和终端宽度感知的自动换行。

use colored::Colorize;
use std::path::{Path, PathBuf};

use crate::core::aliases::{self, MergedConfig};
use crate::utils::display;

/// 渲染 Aliases 区块到终端。
pub fn render(merged: &MergedConfig, alias_dir: &Path) {
    let lines = format_lines(merged, alias_dir);
    if lines.is_empty() {
        return;
    }

    println!();
    println!("{}", "Aliases:".green().bold());

    for (name, line) in &lines {
        if name.is_empty() {
            println!("{}", line);
        } else {
            let rest = &line[name.len()..];
            print!("  {}", name.cyan().bold());
            println!("{}", rest);
        }
    }
}

/// 将合并后的别名配置格式化为对齐的展示行。
fn format_lines(merged: &MergedConfig, alias_dir: &Path) -> Vec<(String, String)> {
    let mut paths = aliases::collect_merged_paths(merged, "");
    paths.sort();

    let entries: Vec<(String, String)> = paths
        .iter()
        .filter_map(|path| {
            let resolved = aliases::resolve_merged_alias(merged, path)?;
            let definition = aliases::to_alias_definition(&resolved.value)?;
            // cwd 相对路径以别名来源文件所在目录为基准解析，
            // 无 source_path 时回退到 ~/.byk/alias/
            let base_dir = resolved
                .source_path
                .as_deref()
                .unwrap_or(alias_dir);
            let suffix = definition
                .cwd
                .as_ref()
                .map(|c| format!(" ({})", resolve_cwd_display(c, base_dir)))
                .unwrap_or_default();
            let display_command = display::escape_for_display(&definition.command);
            Some((path.clone(), format!("{}{}", display_command, suffix)))
        })
        .collect();

    if entries.is_empty() {
        return Vec::new();
    }

    let aligned = display::align_kv_pairs(&entries, "");

    let terminal_width = display::get_terminal_width() as usize;
    let indent = 2;
    let separator = "  ";
    let max_key_width = entries
        .iter()
        .map(|(k, _)| display::get_display_width(k))
        .max()
        .unwrap_or(0);
    let name_and_sep_width = max_key_width + separator.len();
    let max_command_width = if terminal_width > indent + name_and_sep_width {
        terminal_width - indent - name_and_sep_width
    } else {
        usize::MAX
    };

    let mut lines: Vec<(String, String)> = Vec::new();
    for ((path, command), (_, aligned_line)) in entries.iter().zip(aligned.iter()) {
        if max_command_width == usize::MAX
            || display::get_display_width(aligned_line) <= terminal_width - indent
        {
            lines.push((path.clone(), aligned_line.clone()));
        } else {
            let wrapped = display::wrap_text(command, max_command_width);
            if wrapped.is_empty() {
                continue;
            }
            let padded_key = display::pad_to_width(path, max_key_width);
            let first_line = format!("{}  {}", padded_key, wrapped[0]);
            lines.push((path.clone(), first_line));
            for cmd_line in &wrapped[1..] {
                let indented = format!("{}{}", " ".repeat(indent + name_and_sep_width), cmd_line);
                lines.push((String::new(), indented));
            }
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// 路径显示
// ---------------------------------------------------------------------------

/// 将 alias 文件中存储的 cwd 路径解析为直观的显示路径。
///
/// 相对路径以 `base_dir`（来源文件所在目录）为基准 resolve，
/// 绝对路径直接使用。手动消除 `.` 和 `..` 组件（不依赖文件系统，
/// 路径不存在也能正确计算）。若在 `$HOME` 下则替换为 `~` 前缀。
fn resolve_cwd_display(cwd: &str, base_dir: &Path) -> String {
    let cwd_path = Path::new(cwd);
    let resolved = if cwd_path.is_absolute() {
        cwd_path.to_path_buf()
    } else {
        normalize(&base_dir.join(cwd_path))
    };
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = resolved.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    resolved.display().to_string()
}

/// 手动标准化路径，消除 `.` 和 `..` 组件。
/// 不依赖文件系统，路径不存在也能正确计算。
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => { /* skip "." */ }
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use crate::core::aliases::{ResolvedAlias, MergedNode};

    /// 测试用的虚拟 alias 目录。
    const TEST_ALIAS_DIR: &str = "/tmp";

    fn make_resolved_alias(cmd: &str) -> ResolvedAlias {
        ResolvedAlias {
            value: aliases::AliasValue::Str(cmd.into()),
            source: "@test".into(),
            source_path: None,
        }
    }

    fn make_resolved_alias_with_cwd(cmd: &str, cwd: &str) -> ResolvedAlias {
        ResolvedAlias {
            value: aliases::AliasValue::Meta {
                cmd: cmd.into(),
                cwd: Some(cwd.into()),
                interactive: None,
            },
            source: "@test".into(),
            source_path: None,
        }
    }

    /// 插入一个顶层别名。
    fn insert_alias(merged: &mut MergedConfig, key: &str, cmd: &str) {
        let mut node = MergedNode::default();
        node.alias = Some(make_resolved_alias(cmd));
        merged.insert(key.into(), node);
    }

    /// 插入嵌套别名，如 `ns.cmd`。
    fn insert_nested_alias(merged: &mut MergedConfig, ns: &str, key: &str, cmd: &str) {
        let entry = merged.entry(ns.into()).or_default();
        let mut node = MergedNode::default();
        node.alias = Some(make_resolved_alias(cmd));
        entry.children.insert(key.into(), node);
    }

    #[test]
    fn alias_format_lines_empty() {
        let merged: MergedConfig = HashMap::new();
        assert!(format_lines(&merged, Path::new(TEST_ALIAS_DIR)).is_empty());
    }

    #[test]
    fn alias_format_lines_single_alias() {
        let mut merged: MergedConfig = HashMap::new();
        insert_alias(&mut merged, "greet", "echo hello");
        let result = format_lines(&merged, Path::new(TEST_ALIAS_DIR));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "greet");
        // format: "greet  echo hello" (no prefix in format_lines)
        assert_eq!(result[0].1, "greet  echo hello");
    }

    #[test]
    fn alias_format_lines_multiple_sorted() {
        let mut merged: MergedConfig = HashMap::new();
        insert_alias(&mut merged, "zzz", "cmd-z");
        insert_alias(&mut merged, "aaa", "cmd-a");
        insert_alias(&mut merged, "mmm", "cmd-m");
        let result = format_lines(&merged, Path::new(TEST_ALIAS_DIR));
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "aaa");
        assert_eq!(result[1].0, "mmm");
        assert_eq!(result[2].0, "zzz");
    }

    #[test]
    fn alias_format_lines_nested() {
        let mut merged: MergedConfig = HashMap::new();
        insert_nested_alias(&mut merged, "ns", "cmd", "echo nested");
        let result = format_lines(&merged, Path::new(TEST_ALIAS_DIR));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "ns.cmd");
        assert!(result[0].1.contains("echo nested"));
    }

    #[test]
    fn alias_format_lines_with_cwd_suffix() {
        let mut merged: MergedConfig = HashMap::new();
        let mut node = MergedNode::default();
        node.alias = Some(make_resolved_alias_with_cwd("npm run build", "/path/to/project"));
        merged.insert("build".into(), node);
        let result = format_lines(&merged, Path::new(TEST_ALIAS_DIR));
        assert_eq!(result.len(), 1);
        assert!(result[0].1.contains("npm run build"));
        assert!(result[0].1.contains("(/path/to/project)"));
    }

    #[test]
    fn alias_format_lines_key_aligned() {
        let mut merged: MergedConfig = HashMap::new();
        insert_alias(&mut merged, "a", "cmd-a");
        insert_alias(&mut merged, "verylongkey", "cmd-long");
        let result = format_lines(&merged, Path::new(TEST_ALIAS_DIR));
        // "a" 应补齐到和 "verylongkey" 相同的宽度
        assert_eq!(result.len(), 2);
        let a_line = result.iter().find(|(k, _)| k == "a").unwrap();
        let long_line = result.iter().find(|(k, _)| k == "verylongkey").unwrap();
        // 两行的对齐部分（key + 2 空格）应该等宽
        let a_prefix_len = a_line.1.find("cmd-a").unwrap();
        let long_prefix_len = long_line.1.find("cmd-long").unwrap();
        assert_eq!(a_prefix_len, long_prefix_len);
    }
}
