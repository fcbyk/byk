/// CLI 信息格式化渲染（--info 选项）。

use colored::Colorize;
use std::process::Command;

use crate::core::paths::PathLayout;
use crate::core::plugins;

/// 渲染 CLI 完整信息：banner + 路径 + Python 环境。
pub fn render_all(layout: &PathLayout) {
    crate::render::banner::render();
    render_paths(layout);
    println!();
    render_py(layout);
}

/// 显示 CLI 目录路径。
pub fn render_paths(layout: &PathLayout) {
    let items: [(&str, &std::path::Path); 5] = [
        ("CLI Home", layout.root_dir.as_path()),
        ("Alias Directory", layout.alias_dir.as_path()),
        ("Logs Directory", layout.logs_dir.as_path()),
        ("Node Packages", layout.node_pkgs_dir.as_path()),
        ("Python venv", layout.venv_dir.as_path()),
    ];

    for (label, path) in &items {
        let status = if path.exists() {
            String::new()
        } else {
            format!("  {}", "(not created)".red())
        };
        println!("{}: {}{}", label.yellow(), path.display(), status);
    }
}

/// 渲染 Python 环境信息。
pub fn render_py(layout: &PathLayout) {
    let python_exe = plugins::get_python_executable(&layout.cache_dir);
    let cache_file = layout.cache_dir.join("app.json");
    let is_env = std::env::var("BYK_PYTHON").is_ok();

    // 未初始化检测：无 BYK_PYTHON 且 app.json 不存在
    if !is_env && !cache_file.exists() {
        println!("{}", "Python plugin system not initialized.".yellow());
        println!();
        println!("  {}   {}", "$ byk init py".dimmed(), "(global)".dimmed());
        println!("  {}   {}", "$ byk init py-v".dimmed(), "(venv, recommended)".dimmed());
        return;
    }

    // Python 解释器路径
    println!("{}: {}", "Python".yellow(), python_exe);

    // 尝试获取 Python 版本
    if let Ok(output) = Command::new(&python_exe)
        .args(["--version"])
        .output()
    {
        let version = String::from_utf8_lossy(&output.stdout);
        let version = version.trim();
        if !version.is_empty() {
            println!("{}: {}", "Version".yellow(), version);
        }
    }

    // 缓存文件路径
    println!("{}: {}", "Cache".yellow(), cache_file.display());

    // 来源提示
    if is_env {
        println!("{}:  {}", "Source".yellow(), "BYK_PYTHON env var".dimmed());
    } else {
        println!("{}:  {}", "Source".yellow(), "Cache file (app.json)".dimmed());
    }
}
