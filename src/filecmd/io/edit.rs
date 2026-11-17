//! 文件命令共用的读写流程：存在性、symlink/目录保护、JSON 校验与落盘。

use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::filecmd::io::path::resolve_target;
use crate::filecmd::spec::FileScope;
use crate::model::PathLayout;
use crate::utils::json;

/// 读取对象根文件，交给 `edit` 修改后统一写回。
///
/// `action` 用于链接文件报错（`set` / `remove`）；`missing_hint` 追加到
/// “File not found” 之后。
pub(crate) fn object<T>(
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

    let mut root = json::read_value(&path)?;

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

    json::write_value(&path, &root)?;

    Ok((path, value))
}
