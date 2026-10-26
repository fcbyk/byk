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
use crate::core::download;
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
/// 找不到主文件时回退到 `.byk.json` 的 `$ref` 映射。
pub fn install(target: Option<&str>, layout: &PathLayout, cdn: bool) {
    let stem = match target {
        Some(t) if !t.is_empty() => t,
        _ => {
            crate::render::add::render_alias();
            return;
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(alias_file_name(stem));

    if path.is_file() {
        // 直接命中
        install_from_file(&path, &format!("@{}", stem), stem, layout, cdn, None);
        return;
    }

    // 回退：当前目录 .byk.json 的 $ref
    let anon_path = cwd.join(".byk.json");
    if !anon_path.is_file() {
        eprintln!("{} alias file not found: {}", "Error:".red(), stem.bold());
        eprintln!(
            "  Expected {} or $ref entry in {}",
            path.display().to_string().dimmed(),
            anon_path.display().to_string().dimmed()
        );
        std::process::exit(1);
    }

    let anon_content = match fs::read_to_string(&anon_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} failed to read {}: {}", "Error:".red(), anon_path.display(), e);
            std::process::exit(1);
        }
    };
    let anon_json: serde_json::Value = match serde_json::from_str(&anon_content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} invalid JSON in {}: {}", "Error:".red(), anon_path.display(), e);
            std::process::exit(1);
        }
    };

    let ref_value = anon_json
        .get("$ref")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get(stem))
        .and_then(|v| v.as_str());

    match ref_value {
        Some(v) if v.starts_with("http://") || v.starts_with("https://") => {
            // 绝对 URL → 按需转 CDN 后下载到临时文件再安装
            let download_url = if cdn {
                download::to_jsdelivr_url(v)
            } else {
                v.to_string()
            };
            println!(
                "{} Resolved {} via $ref → {}",
                "==>".cyan().bold(),
                stem.bold(),
                download_url.dimmed()
            );
            let temp_dir = layout.cache_dir.join("tmp-remote");
            if let Err(e) = fs::create_dir_all(&temp_dir) {
                eprintln!("{} failed to create {}: {}", "Error:".red(), temp_dir.display(), e);
                std::process::exit(1);
            }
            let temp_file = temp_dir.join(format!("{}.byk.json", stem));
            if let Err(e) = download::download_script(&download_url, &temp_file) {
                eprintln!("{} failed to download {}: {}", "Error:".red(), download_url, e);
                std::process::exit(1);
            }
            let remote_base = download_url.rsplit_once('/')
                .map(|(dir, _)| format!("{}/", dir))
                .unwrap_or_else(|| download_url.clone());
            install_from_file(&temp_file, &format!("@@{}", stem), stem, layout, cdn, Some(&remote_base));
        }
        Some(v) => {
            // 相对路径 → 相对 .byk.json 所在目录
            let resolved = cwd.join(v);
            if !resolved.is_file() {
                eprintln!(
                    "{} $ref target not found: {} → {}",
                    "Error:".red(),
                    v.bold(),
                    resolved.display().to_string().dimmed()
                );
                std::process::exit(1);
            }
            println!(
                "{} Resolved {} via $ref → {}",
                "==>".cyan().bold(),
                stem.bold(),
                resolved.display().to_string().dimmed()
            );
            let file_key = format!("@{}", stem);
            install_from_file(&resolved, &file_key, stem, layout, cdn, None);
        }
        None => {
            eprintln!("{} alias file not found: {}", "Error:".red(), stem.bold());
            eprintln!(
                "  Expected {} or $ref entry in {}",
                path.display().to_string().dimmed(),
                anon_path.display().to_string().dimmed()
            );
            std::process::exit(1);
        }
    }
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