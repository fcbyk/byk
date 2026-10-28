//! `byk add` 子命令逻辑。
//!
//! 包含：
//! - 内置功能初始化（comp）
//! - 从本地别名文件安装（@xxx）
//! - 从全局别名文件安装（@@xxx）
//! - 插件安装核心逻辑：校验 `$add` → `$var` 替换 → pip 安装 → assets 下载 → 写副本
//!
//! 入口：`install_from_file` → `install_from_content`。

pub(crate) mod assets;
pub(crate) mod download;
pub(crate) mod pip;

use colored::Colorize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::aliases;
use crate::core::paths::PathLayout;
use crate::core::vars;
use crate::utils::shell;

// ---------------------------------------------------------------------------
// --add comp
// ---------------------------------------------------------------------------

/// 初始化 shell 补全。
///
/// 行为与 install.sh 保持一致：
/// - 从 `$SHELL` 检测当前 shell（zsh / bash）
/// - 在对应的 rc 文件中追加 `source <(byk completion <shell>)` 行
/// - 幂等：已配置则跳过
pub fn init_completion() {
    let (_, shell_name) = match shell::detect_shell() {
        Some(s) => s,
        None => {
            let shell_val = std::env::var("SHELL").unwrap_or_default();
            eprintln!(
                "{} {} {}",
                "Unsupported shell:".red(),
                shell_val.dimmed(),
                "(supported: zsh, bash)".dimmed()
            );
            return;
        }
    };

    let rc_path = match shell::rc_path() {
        Some(p) => p,
        None => {
            eprintln!("{}", "Cannot determine home directory.".red());
            return;
        }
    };

    let line = shell::completion_line(shell_name);

    // 读取 rc 文件检测是否已配置
    let content = fs::read_to_string(&rc_path).unwrap_or_default();
    if content.contains(&line) {
        println!(
            "Shell completion already configured in {}",
            rc_path.display().to_string().dimmed()
        );
        return;
    }

    // 追加到 rc 文件
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&rc_path)
        .unwrap_or_else(|e| {
            eprintln!("Failed to open {}: {}", rc_path.display(), e);
            std::process::exit(1);
        });

    // 确保从新行开始
    if !content.is_empty() && !content.ends_with('\n') {
        let _ = writeln!(file);
    }

    writeln!(file, "\n# byk shell completion\n{}", line).unwrap_or_else(|e| {
        eprintln!("Failed to write to {}: {}", rc_path.display(), e);
        std::process::exit(1);
    });

    println!(
        "+ Shell completion configured in {}",
        rc_path.display().to_string().dimmed()
    );
    println!(
        "  Restart your shell or run: {} {}",
        "source".dimmed(),
        rc_path.display().to_string().dimmed()
    );
}

// ---------------------------------------------------------------------------
// --add @xxx（当前目录）
// ---------------------------------------------------------------------------

/// `byk add @xxx`：读取当前目录 `xxx.byk.json` 的 `$add` 字段并安装。
pub fn install_from_local(name: &str, layout: &PathLayout) {
    let stem = &name[1..]; // strip '@'
    if stem.is_empty() {
        eprintln!(
            "{} invalid argument: {} (expected @<name>)",
            "Error:".red(),
            name.bold()
        );
        std::process::exit(1);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(alias_file_name(stem));

    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }

    let file_key = format!("@{}", stem);
    install_from_file(&path, &file_key, stem, layout);
}

// ---------------------------------------------------------------------------
// --add @@xxx（全局别名目录）
// ---------------------------------------------------------------------------

/// `byk add @@xxx`：读取 `~/.byk/alias/xxx.byk.json` 的 `$add` 字段并安装。
pub fn install_from_global(name: &str, layout: &PathLayout) {
    let stem = &name[2..]; // strip '@@'
    if stem.is_empty() {
        eprintln!(
            "{} invalid argument: {} (expected @@<name>)",
            "Error:".red(),
            name.bold()
        );
        std::process::exit(1);
    }

    let path = layout.alias_dir.join(alias_file_name(stem));

    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }

    let file_key = format!("@@{}", stem);
    install_from_file(&path, &file_key, stem, layout);
}

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------

/// stem → 文件名。空 stem（匿名 `@` / `@@`）→ `.byk.json`。
pub(crate) fn alias_file_name(stem: &str) -> String {
    if stem.is_empty() {
        ".byk.json".to_string()
    } else {
        format!("{}.byk.json", stem)
    }
}

// ---------------------------------------------------------------------------
// 插件安装核心逻辑
// ---------------------------------------------------------------------------

/// 副本保留的文件级字段。
const FILE_LEVEL_FIELDS: &[&str] = &[
    "$priority", "$cwd", "$interactive", "$paths", "$add", "$remove", "$var",
];

/// 从 JSON 内容安装为全局插件。
///
/// 只有声明 `$add` 的文件才能被安装；否则报错，不装依赖、不下载资源。
///
/// `display_label` 用于日志/错误消息中标识来源（文件路径）。
/// `source_file_dir` 用于解析本地 assets 相对路径（文件所在目录）。
fn install_from_content(
    content: &str,
    display_label: &str,
    file_key: &str,
    stem: &str,
    layout: &PathLayout,
    source_file_dir: Option<&Path>,
) {
    let original: serde_json::Value = serde_json::from_str(content).unwrap_or_else(|e| {
        eprintln!(
            "{} invalid JSON in {}: {}",
            "Error:".red(),
            display_label,
            e
        );
        std::process::exit(1);
    });

    // $add 块：只有声明 $add 的文件才能被安装
    let add_block = original.get("$add").filter(|v| v.is_object());
    if add_block.is_none() {
        eprintln!(
            "{} {} is not a plugin (missing {}).",
            "Error:".red(),
            display_label.bold(),
            "$add".bold()
        );
        eprintln!(
            "  Add {} block with {} and/or {} to make it installable.",
            "$add".dimmed(),
            "pip".dimmed(),
            "assets".dimmed()
        );
        std::process::exit(1);
    }

    // 阶段一变量替换（下载用，副本仍保留原始 $var 声明）
    let mut substituted = original.clone();
    vars::preprocess_vars(&mut substituted);

    // 提取 pip（从 $add.pip）
    let packages = substituted
        .get("$add")
        .and_then(|v| v.get("pip"))
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();
    let assets_block = substituted
        .get("$add")
        .and_then(|v| v.get("assets"))
        .cloned()
        .filter(|v| v.is_object());

    // 装 pip
    if !packages.is_empty() {
        println!(
            "{} Installing pip dependencies from {}",
            "==>".cyan().bold(),
            display_label.bold()
        );
        pip::install_packages(&packages, layout, "byk add");
        println!(
            "{} installed {} package(s)",
            "Successfully".green(),
            packages.len()
        );
    }

    // 下载 assets：传递 source_file_path 作为本地文件解析基准
    // download_block 内部通过 .parent() 获取目录，故传完整路径
    let assets_ctx_path = source_file_dir
        .map(|d| d.join(alias_file_name(stem)))
        .unwrap_or_else(|| layout.alias_dir.join(alias_file_name(stem)));
    if let Some(block) = &assets_block {
        assets::download_block(block, &assets_ctx_path, file_key, layout);
    }

    // 组装副本：文件级字段（含 $add）+ 全部别名
    let obj = original.as_object().cloned().unwrap_or_default();
    let mut result = serde_json::Map::new();
    for field in FILE_LEVEL_FIELDS {
        if let Some(v) = obj.get(*field) {
            result.insert((*field).to_string(), v.clone());
        }
    }
    for (k, v) in &obj {
        if !k.starts_with('$') {
            result.insert(k.clone(), v.clone());
        }
    }

    let dest = layout.alias_dir.join(alias_file_name(stem));
    // ~/.byk/alias/ 是内部目录，静默创建（不向用户报告）
    if let Err(e) = fs::create_dir_all(&layout.alias_dir) {
        eprintln!(
            "{} failed to create {}: {}",
            "Error:".red(),
            layout.alias_dir.display(),
            e
        );
        std::process::exit(1);
    }
    let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(result))
        .unwrap_or_else(|e| {
            eprintln!("{} failed to serialize: {}", "Error:".red(), e);
            std::process::exit(1);
        });
    fs::write(&dest, pretty).unwrap_or_else(|e| {
        eprintln!("{} failed to write {}: {}", "Error:".red(), dest.display(), e);
        std::process::exit(1);
    });

    println!(
        "{} installed plugin to {}",
        "Successfully".green(),
        dest.display().to_string().dimmed()
    );
}

/// 从文件路径安装为全局插件。读取文件后委托给 [`install_from_content`]。
pub(crate) fn install_from_file(
    path: &Path,
    file_key: &str,
    stem: &str,
    layout: &PathLayout,
) {
    let content = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{} failed to read {}: {}", "Error:".red(), path.display(), e);
        std::process::exit(1);
    });
    let display_label = path.display().to_string();
    let source_dir = path.parent();
    install_from_content(&content, &display_label, file_key, stem, layout, source_dir);
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_file_name_forms() {
        assert_eq!(alias_file_name("demo"), "demo.byk.json");
        assert_eq!(alias_file_name(""), ".byk.json");
    }
}