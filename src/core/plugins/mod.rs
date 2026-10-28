//! 插件安装：把声明 `$add` 的 *.byk.json 装成全局插件。
//!
//! 流程：读原始 JSON → 校验 `$add` 存在 → `$var` 阶段一替换
//! → 装 pip 依赖 → 下载 assets 资源 → 写副本到 `~/.byk/alias/<stem>.byk.json`。
//!
//! 入口：`install_from_file`（由 `byk add @xxx` / `byk add @@xxx` 调用）。

use std::fs;
use std::path::Path;

use colored::Colorize;

use crate::core::add::alias_file_name;
use crate::core::aliases;
use crate::core::assets;
use crate::core::paths::PathLayout;
use crate::core::pip;

/// 副本保留的文件级字段。
const FILE_LEVEL_FIELDS: &[&str] = &[
    "$priority", "$cwd", "$interactive", "$paths", "$add", "$var",
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
    crate::core::vars::preprocess_vars(&mut substituted);

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