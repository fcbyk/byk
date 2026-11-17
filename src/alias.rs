//! 别名调用：精确语法与合并空间统一查找。

use crate::execution;
use crate::model::PathLayout;
use crate::resolve;
use crate::resolve::LookupFailure;

/// 处理一条别名命令，返回退出码。
pub fn run(command_name: &str, args: &[String], layout: &PathLayout) -> i32 {
    match resolve::lookup(layout, command_name) {
        Ok(hit) => {
            execution::execute_alias(&hit.resolved, args, &hit.display_source);
            0
        }
        Err(failure) => report(failure),
    }
}

/// 输出查找失败原因，返回退出码 1。
fn report(failure: LookupFailure) -> i32 {
    match failure {
        LookupFailure::FileNotFound { file_key } => {
            eprintln!("Alias file not found: {}", file_key);
        }
        LookupFailure::FileUnavailable { file_key, reason } => {
            eprintln!("Alias file cannot be loaded: {}", file_key);
            eprintln!("  {}", reason);
            eprintln!("Run `byk --check` for details.");
        }
        LookupFailure::ReservedField {
            file_key,
            alias_key,
        } => {
            eprintln!(
                "Not an alias (reserved $ field): {}.{}",
                file_key, alias_key
            );
        }
        LookupFailure::KeyNotFound {
            file_key,
            alias_key,
        } => {
            eprintln!("Alias key not found in file {}: {}", file_key, alias_key);
        }
        LookupFailure::AliasNotFound { key, suggestions } => {
            let prefix = if key.starts_with('-') {
                "Unrecognized command, alias, or option"
            } else {
                "Unrecognized command or alias"
            };
            if suggestions.is_empty() {
                eprintln!("{}: {}", prefix, key);
            } else {
                eprintln!(
                    "{}: {}\nDid you mean:\n  {}",
                    prefix,
                    key,
                    suggestions.join("\n  ")
                );
            }
        }
    }
    1
}
