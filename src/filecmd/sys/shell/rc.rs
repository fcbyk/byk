//! 启动文件里的「byk 补全」块。
//!
//! 这是 `sys@` 里唯一改**别人**文件的对象：只增删标记块、其余字节原样保留；符号链接
//! 写到目标（`temp + rename` 会把链接换成普通文件）；也认 `install.sh` 早期写的裸行。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 块的首尾。用完整句子而不是 `# >>> ... >>>`：rc 文件是给别人读的。
pub(crate) const BEGIN: &str =
    "# The following lines have been added by byk to enable byk cli completions.";
pub(crate) const END: &str = "# End of byk cli completions";

/// 文件里现在的状态。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// 没有补全，文件可能压根不存在。
    Missing,
    /// 标记块；行号从 1 开始，给人看。
    Block {
        start: usize,
        end: usize,
        /// 只找到开头没找到结尾：用户删过结尾行，或者手抄漏了。
        closed: bool,
    },
    /// `install.sh` 写的裸行，没有标记。
    Legacy { line: usize },
}

impl State {
    /// 补全在文件里的位置；没有补全时返回 `None`。拆出来是因为状态表要整句、
    /// `add` 的提示只要括号里那半句。
    pub(crate) fn location(&self) -> Option<String> {
        match self {
            State::Missing => None,
            State::Legacy { line } => Some(format!("line {line}, no markers")),
            State::Block {
                start,
                closed: false,
                ..
            } => Some(format!("line {start}, block not closed")),
            State::Block {
                start,
                end,
                closed: true,
                ..
            } if start == end => Some(format!("line {start}")),
            State::Block {
                start,
                end,
                closed: true,
                ..
            } => Some(format!("lines {start}-{end}")),
        }
    }

    /// 表格里那一行的取值。
    pub(crate) fn label(&self) -> String {
        match self.location() {
            Some(location) => format!("installed ({location})"),
            None => "not installed".to_string(),
        }
    }

    /// 装是装了，但没有标记：`add` 该劝一次「摘了重装」。
    pub(crate) fn is_outdated(&self) -> bool {
        matches!(self, State::Legacy { .. })
    }
}

/// 补全块的内容。
pub(crate) fn snippet(shell: &str) -> String {
    // PowerShell 的块不是 POSIX 语法：检查命令用 Get-Command，求值用 Invoke-Expression；
    // 外部命令的输出是行数组，得先 Out-String 聚成一份脚本文本
    let body = match shell {
        "pwsh" | "powershell" => format!(
            "if (Get-Command byk -ErrorAction SilentlyContinue) {{ byk __api__ completion {shell} | Out-String | Invoke-Expression }}"
        ),
        _ => format!(
            "if command -v byk >/dev/null 2>&1; then source <(byk __api__ completion {shell}); fi"
        ),
    };
    format!("{BEGIN}\n{body}\n{END}")
}

/// 看一眼文件里有没有补全。
pub(crate) fn inspect(text: &str) -> State {
    let lines = split_lines(text);
    let mut begin = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = trim(line);
        if trimmed == BEGIN {
            begin.get_or_insert(index);
            continue;
        }
        if trimmed == END
            && let Some(start) = begin
        {
            return State::Block {
                start: start + 1,
                end: index + 1,
                closed: true,
            };
        }
    }

    // 只有开头：报未闭合，删的时候一路删到文件末尾，别把标记留在文件里
    if let Some(start) = begin {
        return State::Block {
            start: start + 1,
            end: lines.len(),
            closed: false,
        };
    }

    for (index, line) in lines.iter().enumerate() {
        if is_legacy(line) {
            return State::Legacy { line: index + 1 };
        }
    }

    State::Missing
}

/// 追加补全块；已经装过（块或旧裸行）时返回 `None`，让调用方报「已装」。
pub(crate) fn add(text: &str, shell: &str) -> Option<String> {
    if inspect(text) != State::Missing {
        return None;
    }
    Some(append(text, &snippet(shell)))
}

/// 摘掉补全块或旧裸行；本来就没有时返回 `None`。
pub(crate) fn remove(text: &str) -> Option<String> {
    let (start, end) = match inspect(text) {
        State::Missing => return None,
        State::Block { start, end, .. } => (start, end),
        State::Legacy { line } => (line, line),
    };

    let lines = split_lines(text);
    if start == 0 || start > lines.len() {
        return None;
    }

    // 块上方那个空行是 `append` 加的，跟着一起删；add/remove 来回跑不留残渣
    let mut first = start - 1;
    if first > 0 && is_blank(lines[first - 1]) {
        first -= 1;
    }
    let last = end.min(lines.len());

    let mut output = String::with_capacity(text.len());
    for line in &lines[..first] {
        output.push_str(line);
    }
    for line in &lines[last..] {
        output.push_str(line);
    }
    Some(output)
}

/// 读文件；不存在是正常状态，不是错误。
pub(crate) fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Failed to read {}: {}", path.display(), error)),
    }
}

/// 原子写回，并且写到链接的目标上。
pub(crate) fn write(path: &Path, text: &str) -> Result<(), String> {
    let target = target_of(path)?;

    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    // rename 出来的新文件权限是 644；dotfiles 里的 rc 常是 600，得跟着原来走
    let permissions = fs::metadata(&target).ok().map(|meta| meta.permissions());
    let temp = temp_path(&target);

    fs::write(&temp, text).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Failed to write {}: {}", temp.display(), e)
    })?;

    if let Some(permissions) = permissions
        && let Err(error) = fs::set_permissions(&temp, permissions)
    {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Failed to set permissions on {}: {}",
            temp.display(),
            error
        ));
    }

    // Windows 的 rename 不能覆盖已存在的目标（与 store::write_object 同一处理）
    #[cfg(windows)]
    if target.exists() {
        let _ = fs::remove_file(&target);
    }

    fs::rename(&temp, &target).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Failed to write {}: {}", target.display(), e)
    })
}

/// 链接要改的真实文件；不是链接就还是它自己。必须 `canonicalize`，否则 `rename`
/// 会把 dotfiles 里的链接换成普通文件，而且不会有任何提示。
fn target_of(path: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path).map_err(|e| {
            format!(
                "Failed to resolve symlink {}: {}\nHint: the link points at a file that does not exist",
                path.display(),
                e
            )
        }),
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(format!("Failed to inspect {}: {}", path.display(), error)),
    }
}

/// 链接指向的绝对路径；不是链接时返回 `None`。
pub(crate) fn link_target(path: &Path) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    fs::canonicalize(path).ok()
}

fn append(text: &str, block: &str) -> String {
    // 跟着文件自己的行尾走：CRLF 的 rc 里混进 LF 块，diff 会很难看
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let body = block.replace('\n', eol);

    let mut output = text.to_string();
    if output.is_empty() {
        return format!("{body}{eol}");
    }
    if !output.ends_with('\n') {
        output.push_str(eol);
    }
    // 与上一段隔开一行，但已经有了就不再添
    let blank = format!("{eol}{eol}");
    if !output.ends_with(&blank) {
        output.push_str(eol);
    }
    output.push_str(&body);
    output.push_str(eol);
    output
}

/// 按行切，每行带着自己的行尾符——重建时才能逐字节还原，不把 CRLF 洗成 LF。
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split_inclusive('\n').collect()
}

fn trim(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r').trim()
}

fn is_blank(line: &str) -> bool {
    trim(line).is_empty()
}

/// `install.sh` 写过的裸行；判定与 `uninstall.sh` 的正则同源。标记行要先排除，
/// 否则未闭合的块会被当成裸行。
fn is_legacy(line: &str) -> bool {
    let text = trim(line);
    if text.is_empty() || [BEGIN, END].contains(&text) {
        return false;
    }
    text.contains("byk") && (text.contains("__api__ completion") || text.contains("byk completion"))
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZSH: &str =
        "if command -v byk >/dev/null 2>&1; then source <(byk __api__ completion zsh); fi";
    const ZSH_LEGACY: &str =
        "if command -v byk >/dev/null 2>&1; then source <(byk completion zsh); fi";

    #[test]
    fn missing_on_empty_file() {
        assert_eq!(inspect(""), State::Missing);
        assert_eq!(inspect("export FOO=1\n"), State::Missing);
    }

    #[test]
    fn finds_the_block_with_line_numbers() {
        let text = format!("line1\n\n{BEGIN}\n{ZSH}\n{END}\n");
        assert_eq!(
            inspect(&text),
            State::Block {
                start: 3,
                end: 5,
                closed: true,
            }
        );
        assert_eq!(inspect(&text).label(), "installed (lines 3-5)");
    }

    #[test]
    fn finds_a_single_line_block() {
        let text = format!("{BEGIN}\n{END}\n");
        assert_eq!(
            inspect(&text),
            State::Block {
                start: 1,
                end: 2,
                closed: true,
            }
        );
    }

    #[test]
    fn reports_an_unclosed_block() {
        let text = format!("export A=1\n{BEGIN}\n{ZSH}\n");
        let state = inspect(&text);
        assert_eq!(
            state,
            State::Block {
                start: 2,
                end: 3,
                closed: false,
            }
        );
        assert_eq!(state.label(), "installed (line 2, block not closed)");
    }

    #[test]
    fn the_markers_read_like_prose() {
        // 写在别人的 rc 里，得看得出是谁加的、到哪儿结束
        assert_eq!(
            snippet("zsh"),
            "# The following lines have been added by byk to enable byk cli completions.\n\
             if command -v byk >/dev/null 2>&1; then source <(byk __api__ completion zsh); fi\n\
             # End of byk cli completions"
        );
    }

    #[test]
    fn powershell_snippet_uses_powershell_syntax() {
        assert_eq!(
            snippet("pwsh"),
            "# The following lines have been added by byk to enable byk cli completions.\n\
             if (Get-Command byk -ErrorAction SilentlyContinue) { byk __api__ completion pwsh | Out-String | Invoke-Expression }\n\
             # End of byk cli completions"
        );
        assert!(
            snippet("powershell")
                .contains("completion powershell | Out-String | Invoke-Expression")
        );
    }

    #[test]
    fn recognises_the_install_script_line() {
        let state = inspect(&format!("export A=1\n{ZSH}\n"));
        assert_eq!(state, State::Legacy { line: 2 });
        assert_eq!(state.label(), "installed (line 2, no markers)");

        // 老版本 install.sh 写的是不带 __api__ 的形式
        assert_eq!(
            inspect(&format!("{ZSH_LEGACY}\n")),
            State::Legacy { line: 1 }
        );
    }

    #[test]
    fn markers_are_not_mistaken_for_a_legacy_line() {
        // 只有开头没有结尾时，不能让开头的标记行自己被当成裸行
        assert_eq!(
            inspect(&format!("{BEGIN}\n")),
            State::Block {
                start: 1,
                end: 1,
                closed: false,
            }
        );
    }

    #[test]
    fn add_appends_a_block_after_a_blank_line() {
        let text = add("export A=1\n", "zsh").unwrap();
        assert_eq!(text, format!("export A=1\n\n{BEGIN}\n{ZSH}\n{END}\n"));
    }

    #[test]
    fn add_does_not_double_the_blank_line() {
        let text = add("export A=1\n\n", "zsh").unwrap();
        assert_eq!(text, format!("export A=1\n\n{BEGIN}\n{ZSH}\n{END}\n"));
    }

    #[test]
    fn add_fixes_a_missing_final_newline() {
        let text = add("export A=1", "zsh").unwrap();
        assert_eq!(text, format!("export A=1\n\n{BEGIN}\n{ZSH}\n{END}\n"));
    }

    #[test]
    fn add_creates_the_block_in_an_empty_file() {
        assert_eq!(
            add("", "fish").unwrap(),
            format!(
                "{BEGIN}\n{}\n{END}\n",
                snippet("fish").lines().nth(1).unwrap()
            )
        );
    }

    #[test]
    fn add_is_idempotent() {
        let once = add("", "zsh").unwrap();
        assert!(add(&once, "zsh").is_none());
        assert!(add(&format!("{ZSH}\n"), "zsh").is_none());
    }

    #[test]
    fn add_keeps_crlf_files_crlf() {
        let text = add("export A=1\r\n", "zsh").unwrap();
        assert!(!text.replace("\r\n", "").contains('\n'), "{text:?}");
        assert!(text.ends_with("\r\n"));
    }

    #[test]
    fn remove_takes_the_block_and_its_blank_line() {
        let text = format!("export A=1\n\n{BEGIN}\n{ZSH}\n{END}\n");
        assert_eq!(remove(&text).unwrap(), "export A=1\n");
    }

    #[test]
    fn remove_round_trips_with_add() {
        let original = "export A=1\nexport B=2\n";
        let with_block = add(original, "zsh").unwrap();
        assert_eq!(remove(&with_block).unwrap(), original);
    }

    #[test]
    fn remove_takes_the_legacy_line() {
        assert_eq!(
            remove(&format!("export A=1\n{ZSH}\nexport B=2\n")).unwrap(),
            "export A=1\nexport B=2\n"
        );
    }

    #[test]
    fn remove_cleans_an_unclosed_block_to_the_end() {
        let text = format!("export A=1\n\n{BEGIN}\n{ZSH}\n");
        assert_eq!(remove(&text).unwrap(), "export A=1\n");
    }

    #[test]
    fn remove_is_a_no_op_when_nothing_is_there() {
        assert!(remove("").is_none());
        assert!(remove("export A=1\n").is_none());
    }

    #[test]
    fn remove_leaves_the_rest_byte_for_byte() {
        // 只删紧邻块的那一个空行（`append` 加的），更上面的空行是用户的，留着
        let text = format!("  spaced  \n\n\n{BEGIN}\n{ZSH}\n{END}\n# tail\n");
        assert_eq!(remove(&text).unwrap(), "  spaced  \n\n# tail\n");
    }

    #[cfg(unix)]
    #[test]
    fn write_goes_through_a_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let real_dir = temp.path().join("dotfiles");
        std::fs::create_dir_all(&real_dir).unwrap();
        let real = real_dir.join("zshrc");
        std::fs::write(&real, "export A=1\n").unwrap();

        let link = temp.path().join(".zshrc");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write(&link, "export B=2\n").unwrap();

        // 链接还在，改的是目标
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "export B=2\n");
        assert_eq!(
            link_target(&link),
            Some(std::fs::canonicalize(&real).unwrap())
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_keeps_the_original_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".zshrc");
        std::fs::write(&path, "x\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        write(&path, "y\n").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn write_leaves_no_temp_file_behind() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".zshrc");

        write(&path, "x\n").unwrap();

        assert!(!temp_path(&path).exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x\n");
    }

    #[test]
    fn write_creates_missing_parents() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".config").join("fish").join("config.fish");

        write(&path, "x\n").unwrap();

        assert!(path.is_file());
    }

    #[test]
    fn read_treats_a_missing_file_as_empty() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(read(&temp.path().join(".zshrc")).unwrap(), None);
    }
}
