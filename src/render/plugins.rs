/// Plugin Commands 渲染。
///
/// 将 CmdState 中的命令列表转为对齐的终端展示行并输出。

#[cfg(test)]
use crate::core::plugins::CmdState;
#[cfg(test)]
use crate::utils::display;

/// 将插件命令格式化为对齐的展示行。
///
/// 行格式: "  {name}{padding}  {description}"
#[cfg(test)]
fn format_lines(state: &CmdState) -> Vec<(String, String)> {
    if state.commands.is_empty() {
        return Vec::new();
    }

    let mut entries: Vec<(String, String)> = state
        .commands
        .iter()
        .map(|(name, cmd)| (name.clone(), cmd.description.clone()))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    display::align_kv_pairs(&entries, "  ")
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::core::plugins::PluginCommand;

    fn plugin_command(target: &str, description: &str) -> PluginCommand {
        PluginCommand {
            behavior: "py-m".to_string(),
            target: target.into(),
            description: description.into(),
        }
    }

    fn make_state(commands: Vec<(&str, &str, &str)>) -> CmdState {
        let map: HashMap<String, PluginCommand> = commands
            .into_iter()
            .map(|(name, target, desc)| (name.into(), plugin_command(target, desc)))
            .collect();
        CmdState {
            commands: map,
            python_executable: None,
        }
    }

    #[test]
    fn plugin_format_lines_empty() {
        let state = make_state(vec![]);
        assert!(format_lines(&state).is_empty());
    }

    #[test]
    fn plugin_format_lines_single_command() {
        let state = make_state(vec![("send", "byklansend.main:Plugin", "Send messages")]);
        let result = format_lines(&state);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "send");
        assert_eq!(result[0].1, "  send  Send messages");
    }

    #[test]
    fn plugin_format_lines_sorted_by_name() {
        let state = make_state(vec![
            ("zzz", "mod3:Plugin", "Last"),
            ("aaa", "mod1:Plugin", "First"),
            ("mmm", "mod2:Plugin", "Middle"),
        ]);
        let result = format_lines(&state);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "aaa");
        assert_eq!(result[1].0, "mmm");
        assert_eq!(result[2].0, "zzz");
    }

    #[test]
    fn plugin_format_lines_key_alignment() {
        let state = make_state(vec![
            ("verylongcommand", "m:Plugin", "Has a long name"),
            ("x", "m:Plugin", "Short"),
        ]);
        let result = format_lines(&state);
        assert_eq!(result.len(), 2);
        // 排序后 "verylongcommand" 在前，"x" 在后
        // "x" 应补齐到和 "verylongcommand" 相同的宽度
        let short = result.iter().find(|(k, _)| k == "x").unwrap();
        let long = result.iter().find(|(k, _)| k == "verylongcommand").unwrap();
        assert!(short.1.contains("Short"));
        assert!(long.1.contains("Has a long name"));
        // 对齐后的描述起始位置应该一致
        let short_desc_pos = short.1.find("Short").unwrap();
        let long_desc_pos = long.1.find("Has a long name").unwrap();
        assert_eq!(short_desc_pos, long_desc_pos);
    }
}