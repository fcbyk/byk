//! 保留变量展开：`$python` / `$venv` / `$byk`。
//!
//! 别名 `$cmd` / `$cwd` / `$paths` 中的保留变量由 byk 在执行前展开为
//! 具体路径——shell 永远看不到 `$` 变量（`$python` 直接进终端在 sh 中是
//! 空变量、在 cmd 中是普通字符，必败，故必须由 byk 代为展开）。
//!
//! 展开规则：
//! - 仅 `$python` / `$venv` / `$byk` 三个整词命中，替换为对应路径；
//! - 未命中的 `$` token（`$HOME`、`$pythonfoo`、`${args}` 等）原样保留，
//!   交由 shell 自行处理，不破坏既有用法；
//! - 替换值含空白时自动包双引号，避免被 shell 拆分；
//! - `$python` 指向的 venv 解释器不存在时返回错误（友好文案）。
//!
//! 取值全部为纯推导（venv 按约定结构拼接，不读任何状态文件），
//! 与插件命令使用同一个 venv 约定。
//!
//! 如需要资产路径，在 `$var` 中自行定义即可（如 `"TOOLS": "$byk/tools"`）。

use std::path::{Path, PathBuf};

use colored::Colorize;

use crate::core::paths::PathLayout;
use crate::core::pip::{PYTHON_BIN, VENV_BIN};

/// 变量展开上下文：保留变量的取值来源。
#[derive(Debug, Clone)]
pub struct VarContext {
    /// byk 根目录（`~/.byk`）→ `$byk`
    pub byk_root: PathBuf,
    /// venv bin 目录 → `$venv`
    pub venv_bin: PathBuf,
    /// venv Python 解释器 → `$python`
    pub venv_python: PathBuf,
}

impl VarContext {
    /// 从目录布局构建。
    pub fn from_layout(layout: &PathLayout) -> Self {
        let venv_bin = layout.venv_dir.join(VENV_BIN);
        Self {
            byk_root: layout.root_dir.clone(),
            venv_bin: venv_bin.clone(),
            venv_python: venv_bin.join(PYTHON_BIN),
        }
    }
}

/// venv 缺失时的友好报错（替代 shell 的 "command not found" 乱码）。
fn missing_venv_message() -> String {
    format!(
        "{} '$python' requires the byk Python environment (venv), but it does not exist yet.\n  It is created automatically when you install a Python plugin: {}",
        "Error:".red(),
        "byk add <user/repo>".bold()
    )
}

/// 将路径写入输出；含空白时包双引号，避免被 shell 拆分。
fn push_quoted(out: &mut String, path: &Path) {
    let s = path.to_string_lossy();
    if s.contains(char::is_whitespace) {
        out.push('"');
        out.push_str(&s);
        out.push('"');
    } else {
        out.push_str(&s);
    }
}

/// 展开 `cmd` 中的保留变量，返回展开后的命令字符串。
///
/// 单遍扫描：`$` 后跟标识符整词匹配保留集，命中则替换；
/// 未命中（含 `${...}` 占位符——`$` 后是 `{`，标识符为空）原样保留。
pub fn expand_variables(cmd: &str, ctx: &VarContext) -> Result<String, String> {
    let mut out = String::with_capacity(cmd.len());
    let mut rest = cmd;

    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];

        // 读取 $ 后的标识符（空标识符 = ${...} 等，直接透传）
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let ident = &after[..end];

        let value: Option<&Path> = match ident {
            "python" => {
                if !ctx.venv_python.is_file() {
                    return Err(missing_venv_message());
                }
                Some(&ctx.venv_python)
            }
            "venv" => Some(&ctx.venv_bin),
            "byk" => Some(&ctx.byk_root),
            _ => None,
        };

        match value {
            Some(path) => {
                push_quoted(&mut out, path);
                rest = &after[end..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);

    Ok(out)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 构造上下文。`with_python` 控制 venv python 文件是否存在
    /// （`$python` 展开前做 is_file 检查，需要真实文件）。
    fn make_ctx(dir: &Path, with_python: bool) -> VarContext {
        let venv_bin = dir.join(VENV_BIN);
        let venv_python = venv_bin.join(PYTHON_BIN);
        if with_python {
            std::fs::create_dir_all(&venv_bin).unwrap();
            std::fs::write(&venv_python, "").unwrap();
        }
        VarContext {
            byk_root: dir.join("home").join(".byk"),
            venv_bin,
            venv_python,
        }
    }

    #[test]
    fn expands_byk_root() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        let result = expand_variables("echo $byk", &ctx).unwrap();
        assert_eq!(result, format!("echo {}", ctx.byk_root.display()));
    }

    #[test]
    fn expands_venv_bin() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        let result = expand_variables("ls $venv", &ctx).unwrap();
        assert_eq!(result, format!("ls {}", ctx.venv_bin.display()));
    }

    #[test]
    fn expands_python_when_venv_exists() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), true);
        let result = expand_variables("$python -V", &ctx).unwrap();
        assert_eq!(result, format!("{} -V", ctx.venv_python.display()));
    }

    #[test]
    fn python_missing_venv_errors() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        let err = expand_variables("$python -V", &ctx).unwrap_err();
        assert!(err.contains("venv"), "错误文案应提示 venv: {}", err);
    }

    #[test]
    fn unknown_dollar_passthrough() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        // $HOME / $PATH 等未命中变量原样透传，交由 shell 处理
        let result = expand_variables("echo $HOME $PATH", &ctx).unwrap();
        assert_eq!(result, "echo $HOME $PATH");
    }

    #[test]
    fn word_boundary_no_partial_match() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        // $pythonfoo 不是 $python，必须原样保留
        let result = expand_variables("echo $pythonfoo", &ctx).unwrap();
        assert_eq!(result, "echo $pythonfoo");
    }

    #[test]
    fn brace_placeholders_untouched() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        // ${...} 系统占位符：$ 后是 {，标识符为空，透传
        let result = expand_variables("echo ${args} ${0}", &ctx).unwrap();
        assert_eq!(result, "echo ${args} ${0}");
    }

    #[test]
    fn quotes_value_with_whitespace() {
        let dir = tempdir().unwrap();
        // 在临时目录内构造带空格的 venv 路径，验证自动引号
        let spaced = dir.path().join("my home");
        let venv_bin = spaced.join(".byk").join("py-venv").join(".venv").join("bin");
        let venv_python = venv_bin.join(PYTHON_BIN);
        std::fs::create_dir_all(&venv_bin).unwrap();
        std::fs::write(&venv_python, "").unwrap();
        let ctx = VarContext {
            byk_root: spaced.join(".byk"),
            venv_bin,
            venv_python,
        };
        let result = expand_variables("$python -m x", &ctx).unwrap();
        assert_eq!(result, format!("\"{}\" -m x", ctx.venv_python.display()));
    }

    #[test]
    fn multiple_occurrences_all_expanded() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        let result = expand_variables("$byk and $byk", &ctx).unwrap();
        assert_eq!(
            result,
            format!("{} and {}", ctx.byk_root.display(), ctx.byk_root.display())
        );
    }

    #[test]
    fn path_composition() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        // $byk/plugins/... 变量后接路径片段，标识符在 / 处截断
        let result = expand_variables("$python $byk/plugins/jiahao/jiahao.py", &ctx).unwrap_err();
        // venv python 不存在 → 报错（$python 先出现）
        assert!(result.contains("venv"));

        let dir2 = tempdir().unwrap();
        let ctx2 = make_ctx(dir2.path(), true);
        let result = expand_variables("$python $byk/plugins/jiahao/jiahao.py", &ctx2).unwrap();
        assert_eq!(
            result,
            format!(
                "{} {}/plugins/jiahao/jiahao.py",
                ctx2.venv_python.display(),
                ctx2.byk_root.display()
            )
        );
    }

    #[test]
    fn no_variables_unchanged() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        assert_eq!(expand_variables("echo hi", &ctx).unwrap(), "echo hi");
    }

    #[test]
    fn dollar_at_end_unchanged() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        assert_eq!(expand_variables("echo $", &ctx).unwrap(), "echo $");
    }

    #[test]
    fn cjk_text_preserved() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path(), false);
        let result = expand_variables("echo 中文 $byk", &ctx).unwrap();
        assert_eq!(
            result,
            format!("echo 中文 {}", ctx.byk_root.display())
        );
    }
}