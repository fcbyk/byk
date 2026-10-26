//! 插件安装：把声明 `$plugin: true` 的 *.byk.json 装成全局插件。
//!
//! 流程：读原始 JSON → 校验 `$plugin` 开关 → `$var` 阶段一替换
//! → 装 `$pip`/`$pip-keep` → 下载 `$assets` → 写副本到 `~/.byk/alias/<stem>.byk.json`。
//!
//! 入口：`install`（本地 `<file>`）、`install_from_path`（`-f`）、
//! `install_remote`（远程分发，见 remote.rs）。

mod remote;

use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;

use crate::core::add::alias_file_name;
use crate::core::aliases;
use crate::core::assets;
use crate::core::paths::PathLayout;
use crate::core::pip;

pub use remote::install_remote;

/// 副本保留的文件级字段。`$plugin` 是文件持久身份，保留在副本中。
const FILE_LEVEL_FIELDS: &[&str] = &[
    "$priority", "$cwd", "$interactive", "$paths", "$pip", "$pip-keep", "$assets", "$var", "$plugin",
];

/// 从文件路径安装为全局插件（本地 / 远程共用）。
///
/// 只有声明 `$plugin: true` 的文件才能被安装；否则报错，不装依赖、不下载资源。
fn install_from_file(
    path: &Path,
    file_key: &str,
    stem: &str,
    layout: &PathLayout,
    cdn: bool,
    remote_base: Option<&str>,
) {
    // 读原始 JSON
    let content = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{} failed to read {}: {}", "Error:".red(), path.display(), e);
        std::process::exit(1);
    });
    let original: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!(
            "{} invalid JSON in {}: {}",
            "Error:".red(),
            path.display(),
            e
        );
        std::process::exit(1);
    });

    // $plugin 开关：只有声明为插件的文件才能被安装
    let is_plugin = original
        .get("$plugin")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_plugin {
        eprintln!(
            "{} {} is not a plugin (missing {}).",
            "Error:".red(),
            path.display().to_string().bold(),
            "$plugin: true".bold()
        );
        eprintln!(
            "  Add {} to install it, or use {} / {} for deps/resources only.",
            "$plugin: true".dimmed(),
            "byk add pip".bold(),
            "byk add assets".bold()
        );
        std::process::exit(1);
    }

    // 阶段一变量替换（下载用，副本仍保留原始 $var 声明）
    let mut substituted = original.clone();
    crate::core::vars::preprocess_vars(&mut substituted);

    // 提取 pip / pip-keep / assets（替换后）
    let packages = substituted
        .get("$pip")
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();
    let packages_keep = substituted
        .get("$pip-keep")
        .and_then(aliases::parse_pip_field)
        .unwrap_or_default();
    let assets_block = substituted
        .get("$assets")
        .cloned()
        .filter(|v| v.is_object());

    // 装 pip（$pip + $pip-keep 目前行为一致，均安装到 venv）
    if !packages.is_empty() || !packages_keep.is_empty() {
        println!(
            "{} Installing pip dependencies from {}",
            "==>".cyan().bold(),
            path.display().to_string().bold()
        );
        if !packages.is_empty() {
            pip::install_packages(&packages, layout, "byk add");
        }
        if !packages_keep.is_empty() {
            pip::install_packages(&packages_keep, layout, "byk add");
        }
        println!(
            "{} installed {} package(s)",
            "Successfully".green(),
            packages.len() + packages_keep.len()
        );
    }

    // 下载 assets
    if let Some(block) = &assets_block {
        assets::download_block(block, path, file_key, layout, cdn, remote_base);
    }

    // 组装副本：文件级字段（含 $plugin）+ 全部别名
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

/// `byk add <file>`：安装当前目录的 `<file>.byk.json` 到全局目录。
pub fn install(target: Option<&str>, layout: &PathLayout, cdn: bool) {
    let stem = match target {
        Some(t) if !t.is_empty() => t,
        _ => {
            crate::render::add::render_alias();
            return;
        }
    };

    // 解析本地文件（当前目录）
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(alias_file_name(stem));
    if !path.is_file() {
        eprintln!("{} alias file not found: {}", "Error:".red(), stem.bold());
        eprintln!(
            "  Expected {} in the current directory.",
            path.display().to_string().dimmed()
        );
        std::process::exit(1);
    }

    install_from_file(&path, &format!("@{}", stem), stem, layout, cdn, None);
}

/// `byk add -f <path>`：安装指定路径的 *.byk.json 文件（本地）。
pub fn install_from_path(path_str: &str, layout: &PathLayout, cdn: bool) {
    let path = PathBuf::from(path_str);
    if !path.is_file() {
        eprintln!(
            "{} alias file not found: {}",
            "Error:".red(),
            path.display().to_string().bold()
        );
        std::process::exit(1);
    }
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let stem = file_name.strip_suffix(".byk.json").unwrap_or(file_name);
    if stem.is_empty() {
        eprintln!("{} invalid alias file: {}", "Error:".red(), path_str.bold());
        std::process::exit(1);
    }
    install_from_file(&path, &format!("@{}", stem), stem, layout, cdn, None);
}
