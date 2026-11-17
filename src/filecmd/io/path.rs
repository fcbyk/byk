//! 文件命令 token 的解析与目标文件定位。

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::PathLayout;
use crate::resolve::validate_filename;

use crate::filecmd::spec::{FileScope, invalid_stem_message, split_prefix};

/// `__api__ file` 的返回内容：key 对应的 scope 与目标文件路径。
pub(crate) struct FileAddress {
    pub scope: FileScope,
    pub path: PathBuf,
}

/// 文件命令的目标文件名：`<stem>.byk.json`，空 stem 为 `.byk.json`。
pub(crate) fn target_path(base: &Path, stem: &str) -> PathBuf {
    if stem.is_empty() {
        base.join(".byk.json")
    } else {
        base.join(format!("{}.byk.json", stem))
    }
}

/// 按 scope 解析文件命令的目标路径。
pub(crate) fn resolve_target(
    scope: FileScope,
    stem: &str,
    layout: &PathLayout,
) -> Result<PathBuf, String> {
    match scope {
        FileScope::Local => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("Failed to resolve current directory: {}", e))?;
            Ok(target_path(&cwd, stem))
        }
        FileScope::Global => Ok(target_path(&layout.alias_dir, stem)),
    }
}

/// 解析文件 key（`@name` / `@@name` / `@` / `@@`）为目标地址。
///
/// 与 `..path` 共用文件名校验和 scope 选择；不读取文件，也不要求文件存在。
pub(crate) fn address_for_key(key: &str, layout: &PathLayout) -> Result<FileAddress, String> {
    let Some((prefix, stem)) = split_prefix(key) else {
        return Err(format!(
            "Invalid file key: {:?}\nHint: expected @<name> or @@<name>",
            key
        ));
    };
    if !validate_filename(stem) {
        return Err(invalid_stem_message(stem));
    }
    let scope = if prefix == "@@" {
        FileScope::Global
    } else {
        FileScope::Local
    };
    let path = resolve_target(scope, stem, layout)?;
    Ok(FileAddress { scope, path })
}

/// 目标不存在时的错误；另一 scope 有同名文件时给出切换提示。
pub(crate) fn not_found_message(
    scope: FileScope,
    stem: &str,
    target: &Path,
    layout: &PathLayout,
    verb: &str,
) -> String {
    let mut message = format!("File not found: {}", target.display());

    let other = match scope {
        FileScope::Local => target_path(&layout.alias_dir, stem),
        FileScope::Global => match std::env::current_dir() {
            Ok(cwd) => target_path(&cwd, stem),
            Err(_) => return message,
        },
    };

    if other != target && fs::symlink_metadata(&other).is_ok() {
        let (label, prefix) = match scope {
            FileScope::Local => ("global", "@@"),
            FileScope::Global => ("local", "@"),
        };
        message.push_str(&format!(
            "\nHint: a {} file exists at {}; use {}{}..{}",
            label,
            other.display(),
            prefix,
            stem,
            verb
        ));
    }

    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filecmd::spec::{
        FileCommand, FileCommandError, ParseResult, Verb, claims, describe, parse,
    };

    fn parse_ok(token: &str) -> FileCommand {
        match parse(token) {
            ParseResult::Command(command) => command,
            other => panic!("expected command, got {:?}", other),
        }
    }

    fn parse_err(token: &str) -> FileCommandError {
        match parse(token) {
            ParseResult::Error(err) => err,
            other => panic!("expected error, got {:?}", other),
        }
    }

    #[test]
    fn parse_global_init() {
        assert_eq!(
            parse_ok("@@ops..init"),
            FileCommand {
                scope: FileScope::Global,
                stem: "ops".into(),
                verb: Verb::Init,
            }
        );
    }

    #[test]
    fn parse_local_init() {
        assert_eq!(
            parse_ok("@ops..init"),
            FileCommand {
                scope: FileScope::Local,
                stem: "ops".into(),
                verb: Verb::Init,
            }
        );
    }

    #[test]
    fn parse_anonymous_files() {
        assert_eq!(parse_ok("@@..init").stem, "");
        assert_eq!(parse_ok("@..init").scope, FileScope::Local);
    }

    #[test]
    fn parse_exact_alias_is_not_a_file_command() {
        assert_eq!(parse("@@ops.init"), ParseResult::NotFileCommand);
        assert_eq!(parse("@ops.init.prod"), ParseResult::NotFileCommand);
        assert_eq!(parse("ops.init"), ParseResult::NotFileCommand);
    }

    #[test]
    fn parse_missing_verb() {
        assert_eq!(parse_err("@@ops.."), FileCommandError::MissingVerb);
        assert_eq!(parse_err("@@.."), FileCommandError::MissingVerb);
    }

    #[test]
    fn parse_invalid_verb() {
        assert_eq!(
            parse_err("@@ops...init"),
            FileCommandError::InvalidVerb(".init".into())
        );
        assert_eq!(
            parse_err("@@ops..init.prod"),
            FileCommandError::InvalidVerb("init.prod".into())
        );
    }

    #[test]
    fn parse_unknown_verb() {
        assert_eq!(
            parse_err("@@ops..explode"),
            FileCommandError::UnknownVerb("explode".into())
        );
    }

    #[test]
    fn parse_invalid_stem() {
        assert_eq!(
            parse_err("@@a/b..init"),
            FileCommandError::InvalidStem("a/b".into())
        );
        assert_eq!(
            parse_err("@@a.b..init"),
            FileCommandError::InvalidStem("a.b".into())
        );
        assert_eq!(
            parse_err("@@a b..init"),
            FileCommandError::InvalidStem("a b".into())
        );
        assert_eq!(
            parse_err("@@@ops..init"),
            FileCommandError::InvalidStem("@ops".into())
        );
    }

    #[test]
    fn claims_file_command_namespace() {
        assert!(claims("@@ops..init"));
        assert!(claims("@@ops.."));
        assert!(claims("@@ops..explode"));
        assert!(!claims("@@ops.init"));
        assert!(!claims("normal"));
    }

    #[test]
    fn describe_file_command_verbs() {
        assert_eq!(describe("@@ops..init"), Some("Create an alias file"));
        assert_eq!(describe("@ops..destroy"), Some("Remove the alias file"));
        assert_eq!(
            describe("@@ops..clear"),
            Some("Reset this file to an empty object")
        );
        assert_eq!(describe("@@ops..set"), Some("Create or update an alias"));
        assert_eq!(describe("@@ops..remove"), Some("Remove an alias or group"));
        assert_eq!(describe("@@..json"), Some("Show the alias file JSON"));
        assert_eq!(describe("@@ops..path"), Some("Print the absolute path"));
        assert_eq!(describe("@@ops..rename"), Some("Rename the alias file"));
        assert_eq!(
            describe("@@ops..clone"),
            Some("Copy a local or remote alias file into this file")
        );
        assert_eq!(
            describe("@@ops..link"),
            Some("Point this file at another alias file")
        );
        assert_eq!(describe("@@ops..help"), Some("Show this help"));
        assert_eq!(describe("@@ops.init"), None);
        assert_eq!(describe("@ops"), None);
    }

    #[test]
    fn parse_global_destroy() {
        assert_eq!(
            parse_ok("@@ops..destroy"),
            FileCommand {
                scope: FileScope::Global,
                stem: "ops".into(),
                verb: Verb::Destroy,
            }
        );
    }

    #[test]
    fn target_path_named_and_anonymous() {
        assert_eq!(
            target_path(Path::new("/base"), "ops"),
            PathBuf::from("/base/ops.byk.json")
        );
        assert_eq!(
            target_path(Path::new("/base"), ""),
            PathBuf::from("/base/.byk.json")
        );
    }
}
