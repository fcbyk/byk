//! 文件命令共用的读写流程：存在性、symlink/目录保护、JSON 校验与落盘。

use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::{Map, Value};

use super::{FileScope, resolve_target};
use crate::core::paths::PathLayout;

/// 读取对象根文件，交给 `edit` 修改后统一写回。
///
/// `action` 用于链接文件报错（`set` / `remove`）；`missing_hint` 追加到
/// “File not found” 之后。
pub(super) fn object<T>(
    scope: FileScope,
    stem: &str,
    layout: &PathLayout,
    missing_hint: Option<String>,
    action: &str,
    edit: impl FnOnce(&mut Map<String, Value>) -> Result<T, String>,
) -> Result<(PathBuf, T), String> {
    let path = resolve_target(scope, stem, layout)?;

    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut message = format!("File not found: {}", path.display());
            if let Some(hint) = missing_hint {
                message.push('\n');
                message.push_str(&hint);
            }
            return Err(message);
        }
        Err(e) => return Err(format!("Failed to inspect {}: {}", path.display(), e)),
    };

    if metadata.file_type().is_symlink() {
        return Err(format!("Refusing to rewrite symlink: {}", path.display()));
    }
    if metadata.file_type().is_dir() {
        return Err(format!("Refusing to edit directory: {}", path.display()));
    }

    let text = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let mut root: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

    let value = {
        let object = match &mut root {
            Value::Object(object) => object,
            Value::Array(_) => {
                return Err(format!(
                    "Cannot {} aliases in a link file: {}\nHint: edit the link target directly",
                    action,
                    path.display()
                ));
            }
            _ => return Err(format!("Not an alias file: {}", path.display())),
        };
        edit(object)?
    };

    let mut output = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to encode {}: {}", path.display(), e))?;
    output.push('\n');
    fs::write(&path, output).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    Ok((path, value))
}
