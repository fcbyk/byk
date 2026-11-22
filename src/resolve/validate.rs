//! 扫描期校验：字段类型、`$mode` 取值与 `$deps` 声明。
//!
//! 校验只产出诊断，不改动别名数据；依赖装没装属于 `deps status`，
//! 不属于配置检查。

use std::path::Path;

use crate::deps;
use crate::model::build_file_key;
use crate::resolve::{Diagnostic, Level, is_valid_clone, is_valid_private};
use crate::utils::path::dir_of;

/// 记录一条归属于扫描文件的诊断；链接目标的内部问题补上目标路径。
pub(super) fn push_doc_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    level: Level,
    owner: &Path,
    doc: &Path,
    message: String,
) {
    let message = if doc != owner {
        format!("{} (in {})", message, doc.display())
    } else {
        message
    };
    diagnostics.push(Diagnostic {
        level,
        path: owner.to_path_buf(),
        message,
    });
}

/// `$deps` 的扫描期校验：局部文件拒绝；全局文件校验声明本身——能不能解析、
/// 落点是否冲突、相对 `src` 有没有 base。只报告，不改动别名数据，也不看装没装：
/// 依赖装没装是运行时状态，属于 `deps status`，不属于配置检查。
pub(super) fn validate_deps_declaration(
    value: serde_json::Value,
    stem: &str,
    is_global: bool,
    root_dir: &Path,
    doc: &Path,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let declaration = match deps::parse_deps(&value) {
        Ok(decl) => decl,
        Err(message) => {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!("$deps ignored: {message}"),
            );
            return Vec::new();
        }
    };

    let key = build_file_key(stem, is_global);
    let deps_dir = deps::deps_dir(&key, is_global, Path::new(&dir_of(owner)), root_dir);
    if let Err(message) = deps::plan_entries(&declaration, &deps_dir, &key) {
        push_doc_diagnostic(
            diagnostics,
            Level::Warning,
            owner,
            doc,
            format!("$deps ignored: {message}"),
        );
        return Vec::new();
    }

    let mut paths = Vec::new();
    if !declaration.pip.is_empty() {
        paths.push(deps::venv_bin(&deps_dir).to_string_lossy().into_owned());
    }
    if !declaration.npm.is_empty() {
        paths.push(
            deps::node_modules_bin(&deps_dir)
                .to_string_lossy()
                .into_owned(),
        );
    }
    paths
}

/// 校验别名树里的字段类型；这些错误会被后续解析静默丢弃，所以必须在这里报告。
///
/// 在 `filter_invalid_keys` 之前调用，否则错误类型已经被过滤掉，无法诊断。
pub(super) fn validate_aliases(
    object: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    doc: &Path,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (key, value) in object {
        if key.starts_with('$') {
            continue;
        }
        let serde_json::Value::Object(inner) = value else {
            continue;
        };
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };
        let is_alias = inner.contains_key("$cmd");

        if is_alias {
            if let Some(cmd) = inner.get("$cmd")
                && !cmd.is_string()
            {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Error,
                    owner,
                    doc,
                    format!("alias \"{}\": $cmd must be a string; alias ignored", path),
                );
            }
            if let Some(cwd) = inner.get("$cwd")
                && !cwd.is_string()
            {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Error,
                    owner,
                    doc,
                    format!("alias \"{}\": $cwd must be a string; alias ignored", path),
                );
            }
            if let Some(description) = inner.get("$description")
                && !description.is_string()
            {
                push_doc_diagnostic(
                    diagnostics,
                    Level::Error,
                    owner,
                    doc,
                    format!(
                        "alias \"{}\": $description must be a string; alias ignored",
                        path
                    ),
                );
            }
        } else if let Some(cwd) = inner.get("$cwd")
            && !cwd.is_string()
        {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!("group \"{}\": $cwd must be a string; ignored", path),
            );
        }

        if let Some(private) = inner.get("$private")
            && !is_valid_private(private)
        {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!(
                    "\"{}\": $private must be a boolean; {} is treated as false",
                    path, private
                ),
            );
        }

        if let Some(clone) = inner.get("$clone")
            && !is_valid_clone(clone)
        {
            push_doc_diagnostic(
                diagnostics,
                Level::Warning,
                owner,
                doc,
                format!(
                    "\"{}\": $clone must be a boolean; {} is treated as true",
                    path, clone
                ),
            );
        }

        if let Some(raw) = inner.get("$paths") {
            validate_paths_value(raw, &path, is_alias, doc, owner, diagnostics);
        }

        validate_aliases(inner, &path, doc, owner, diagnostics);
    }
}

/// 分组 / 别名级 `$paths` 只认字符串数组；写错时告警并按「没写」继续继承。
fn validate_paths_value(
    raw: &serde_json::Value,
    path: &str,
    is_alias: bool,
    doc: &Path,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let label = if is_alias { "alias" } else { "group" };
    let Some(items) = raw.as_array() else {
        push_doc_diagnostic(
            diagnostics,
            Level::Warning,
            owner,
            doc,
            format!(
                "{} \"{}\": $paths must be an array of strings; ignored",
                label, path
            ),
        );
        return;
    };

    let dropped = items.iter().filter(|item| item.as_str().is_none()).count();
    if dropped > 0 {
        push_doc_diagnostic(
            diagnostics,
            Level::Warning,
            owner,
            doc,
            format!(
                "{} \"{}\": $paths contains {} non-string {}; ignored",
                label,
                path,
                dropped,
                if dropped == 1 { "entry" } else { "entries" }
            ),
        );
    }
}

/// `$mode` 只认三个值；写错时告警并回落 `normal`。
///
/// 必须在这里喊一声：把它拼成 `"interactiv"` 会让一条本该确认的危险命令
/// 直接执行，是「配置写了没生效」里后果最重的一种。
///
/// 只走 object：数组在 `filter_invalid_keys` 里本来就会被整段丢掉，
/// 里面的 `$mode` 属于死配置，不值得为它再递归一层。
pub(super) fn warn_invalid_modes(
    value: &serde_json::Value,
    doc: &Path,
    owner: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let serde_json::Value::Object(obj) = value else {
        return;
    };

    if let Some(raw) = obj.get("$mode")
        && !matches!(
            raw.as_str(),
            Some("normal") | Some("interactive") | Some("silent")
        )
    {
        push_doc_diagnostic(
            diagnostics,
            Level::Warning,
            owner,
            doc,
            format!(
                "$mode must be one of normal|interactive|silent, got {} (treated as normal)",
                raw
            ),
        );
    }

    for child in obj.values() {
        warn_invalid_modes(child, doc, owner, diagnostics);
    }
}
