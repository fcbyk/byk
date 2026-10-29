//! 保留变量展开：`$byk` / `$workdir`。
//!
//! 别名 `$cmd` / `$cwd` / `$paths` 中的保留变量由 byk 在执行前展开为
//! 具体路径——shell 永远看不到 `$` 变量（`$byk` 直接进终端在 sh 中是
//! 空变量、在 cmd 中是普通字符，必败，故必须由 byk 代为展开）。
//!
//! 展开规则：
//! - 仅 `$byk` / `$workdir` 整词命中，替换为对应路径；
//! - 未命中的 `$` token（`$HOME`、`$bykfoo`、`${args}` 等）原样保留，
//!   交由 shell 自行处理，不破坏既有用法；
//! - 替换值含空白时自动包双引号，避免被 shell 拆分。
//!
//! 其他路径一律在 `$var` 中自行定义（如 `"TOOLS": "$byk/tools"`）。

use std::path::{Path, PathBuf};

use crate::core::paths::PathLayout;

/// 变量展开上下文：保留变量的取值来源。
#[derive(Debug, Clone)]
pub struct VarContext {
    /// byk 根目录（`~/.byk`）→ `$byk`
    pub byk_root: PathBuf,
    /// 执行命令时的工作目录 → `$workdir`
    pub workdir: PathBuf,
}

impl VarContext {
    /// 从目录布局构建（workdir 取当前工作目录）。
    pub fn from_layout(layout: &PathLayout) -> Self {
        Self {
            byk_root: layout.root_dir.clone(),
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
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
pub fn expand_variables(cmd: &str, ctx: &VarContext) -> String {
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
            "byk" => Some(&ctx.byk_root),
            "workdir" => Some(&ctx.workdir),
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

    out
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(dir: &Path) -> VarContext {
        VarContext {
            byk_root: dir.join("home").join(".byk"),
            workdir: dir.join("work"),
        }
    }

    #[test]
    fn expands_byk_root() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("echo $byk", &ctx),
            format!("echo {}", ctx.byk_root.display())
        );
    }

    #[test]
    fn expands_workdir() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("pwd $workdir", &ctx),
            format!("pwd {}", ctx.workdir.display())
        );
    }

    #[test]
    fn combined_path_expands() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("ls $workdir/out", &ctx),
            format!("ls {}/out", ctx.workdir.display())
        );
    }

    #[test]
    fn unknown_dollar_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("echo $HOME $PATH", &ctx),
            "echo $HOME $PATH"
        );
    }

    #[test]
    fn word_boundary_no_partial_match() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        // $bykfoo 不是 $byk，必须原样保留
        assert_eq!(expand_variables("echo $bykfoo", &ctx), "echo $bykfoo");
    }

    #[test]
    fn brace_placeholders_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("echo ${args} ${0}", &ctx),
            "echo ${args} ${0}"
        );
    }

    #[test]
    fn quotes_value_with_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = VarContext {
            byk_root: dir.path().join("my home").join(".byk"),
            workdir: dir.path().join("my work"),
        };
        assert_eq!(
            expand_variables("ls $byk", &ctx),
            format!("ls \"{}\"", ctx.byk_root.display())
        );
    }

    #[test]
    fn multiple_occurrences_all_expanded() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("$byk and $byk", &ctx),
            format!("{} and {}", ctx.byk_root.display(), ctx.byk_root.display())
        );
    }

    #[test]
    fn no_variables_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(expand_variables("echo hi", &ctx), "echo hi");
    }

    #[test]
    fn dollar_at_end_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(expand_variables("echo $", &ctx), "echo $");
    }

    #[test]
    fn cjk_text_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_ctx(dir.path());
        assert_eq!(
            expand_variables("echo 中文 $byk", &ctx),
            format!("echo 中文 {}", ctx.byk_root.display())
        );
    }

}
