//! 终端显示工具函数。
//!
//! 提供 CJK 兼容的显示宽度计算、对齐、换行等通用渲染能力。
//! 对应 Python `infra/display.py`。

use unicode_width::UnicodeWidthStr;

/// 获取字符串的终端显示宽度（CJK 兼容）。
pub fn get_display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// 将文本按指定显示宽度补齐空格。
///
/// 如果 text 显示宽度已经 >= target_width，返回原文本。
pub fn pad_to_width(text: &str, target_width: usize) -> String {
    let current = get_display_width(text);
    if current >= target_width {
        text.to_string()
    } else {
        format!("{}{}", text, " ".repeat(target_width - current))
    }
}

/// 将键值对列表按 CJK 显示宽度对齐，返回 (键, 格式化行)。
///
/// 行格式: "{prefix}{key}{padding}  {value}"
///
/// prefix 通常为空字符串或两个空格缩进。
pub fn align_kv_pairs(
    entries: &[(String, String)],
    prefix: &str,
) -> Vec<(String, String)> {
    if entries.is_empty() {
        return Vec::new();
    }

    let max_key_width = entries
        .iter()
        .map(|(k, _)| get_display_width(k))
        .max()
        .unwrap_or(0);

    entries
        .iter()
        .map(|(k, v)| {
            let padded_key = pad_to_width(k, max_key_width);
            let line = format!("{}{}  {}", prefix, padded_key, v);
            (k.clone(), line)
        })
        .collect()
}

/// 获取终端宽度，无法获取时返回 80。
pub fn get_terminal_width() -> u16 {
    use terminal_size::{terminal_size, Width};
    if let Some((Width(w), _)) = terminal_size() {
        w
    } else {
        80
    }
}

/// 将文本按指定显示宽度换行，保持单词完整性。
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in &words {
        if current.is_empty() {
            current = word.to_string();
        } else {
            let candidate = format!("{} {}", current, word);
            if get_display_width(&candidate) <= max_width {
                current = candidate;
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// 将控制字符转为转义表示，避免破坏终端渲染。
///
/// 执行时用原始字符串，仅显示时调用此函数。
pub fn escape_for_display(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== get_display_width ====================

    #[test]
    fn display_width_ascii() {
        assert_eq!(get_display_width("hello"), 5);
        assert_eq!(get_display_width("a b c"), 5);
    }

    #[test]
    fn display_width_empty() {
        assert_eq!(get_display_width(""), 0);
    }

    #[test]
    fn display_width_cjk() {
        // 每个中文字符宽度为 2
        assert_eq!(get_display_width("你好"), 4);
        assert_eq!(get_display_width("中文测试"), 8);
    }

    #[test]
    fn display_width_mixed() {
        // "hello你好" = 5 + 4 = 9
        assert_eq!(get_display_width("hello你好"), 9);
    }

    #[test]
    fn display_width_emoji() {
        // emoji 宽度通常为 2
        assert_eq!(get_display_width("🚀"), 2);
    }

    // ==================== pad_to_width ====================

    #[test]
    fn pad_shorter_text() {
        // "hi" 宽度 2，目标 6，补 4 空格
        assert_eq!(pad_to_width("hi", 6), "hi    ");
    }

    #[test]
    fn pad_exact_width() {
        assert_eq!(pad_to_width("hello", 5), "hello");
    }

    #[test]
    fn pad_longer_text_no_truncation() {
        // text 宽度已超过目标，返回原文本
        assert_eq!(pad_to_width("hello world", 5), "hello world");
    }

    #[test]
    fn pad_empty_text() {
        assert_eq!(pad_to_width("", 4), "    ");
    }

    #[test]
    fn pad_target_zero() {
        assert_eq!(pad_to_width("abc", 0), "abc");
    }

    #[test]
    fn pad_cjk_text() {
        // "你好" 宽度 4，目标 6，补 2 空格
        assert_eq!(pad_to_width("你好", 6), "你好  ");
    }

    // ==================== align_kv_pairs ====================

    #[test]
    fn align_empty_entries() {
        let result = align_kv_pairs(&[], "");
        assert!(result.is_empty());
    }

    #[test]
    fn align_single_entry_no_prefix() {
        let entries = vec![("key".to_string(), "value".to_string())];
        let result = align_kv_pairs(&entries, "");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "key");
        assert_eq!(result[0].1, "key  value");
    }

    #[test]
    fn align_multiple_entries_aligned() {
        let entries = vec![
            ("a".to_string(), "first".to_string()),
            ("longkey".to_string(), "second".to_string()),
        ];
        let result = align_kv_pairs(&entries, "");
        assert_eq!(result.len(), 2);
        // "a" 补到宽度 7（"longkey" 的宽度）
        assert_eq!(result[0].1, "a        first");
        assert_eq!(result[1].1, "longkey  second");
    }

    #[test]
    fn align_with_prefix() {
        let entries = vec![("k".to_string(), "v".to_string())];
        let result = align_kv_pairs(&entries, "  ");
        assert_eq!(result[0].1, "  k  v");
    }

    #[test]
    fn align_cjk_keys_aligned_correctly() {
        // "设置" 宽度 4，"用户名" 宽度 6
        let entries = vec![
            ("设置".to_string(), "config".to_string()),
            ("用户名".to_string(), "user".to_string()),
        ];
        let result = align_kv_pairs(&entries, "");
        // max_key_width = 6 ("用户名")，"设置" 需要补 2 空格
        assert_eq!(result[0].1, "设置    config");
        assert_eq!(result[1].1, "用户名  user");
    }

    // ==================== get_terminal_width ====================

    #[test]
    fn terminal_width_is_positive() {
        // 无论是否检测到终端，返回值都应该 > 0
        assert!(get_terminal_width() > 0);
    }

    // ==================== wrap_text ====================

    #[test]
    fn wrap_zero_max_width_returns_original() {
        assert_eq!(wrap_text("hello world", 0), vec!["hello world"]);
    }

    #[test]
    fn wrap_empty_text_returns_empty_vec() {
        assert_eq!(wrap_text("", 10), Vec::<String>::new());
    }

    #[test]
    fn wrap_whitespace_only() {
        assert_eq!(wrap_text("   ", 10), Vec::<String>::new());
    }

    #[test]
    fn wrap_single_word_under_width() {
        assert_eq!(wrap_text("hello", 10), vec!["hello"]);
    }

    #[test]
    fn wrap_single_word_equals_width() {
        assert_eq!(wrap_text("hello", 5), vec!["hello"]);
    }

    #[test]
    fn wrap_single_word_longer_than_width() {
        // 单个词超过宽度，保持完整不切分
        assert_eq!(wrap_text("hello world", 4), vec!["hello", "world"]);
    }

    #[test]
    fn wrap_fits_in_one_line() {
        assert_eq!(wrap_text("hello world rust", 20), vec!["hello world rust"]);
    }

    #[test]
    fn wrap_splits_across_lines() {
        // "hello world rust": hello(5) + 空格 + world(5) = 11, 再加 rust(4) 超出 10
        assert_eq!(wrap_text("hello world rust", 10), vec!["hello", "world rust"]);
    }

    #[test]
    fn wrap_exact_boundary() {
        // "abc def" 宽度 7，宽度 7 刚好
        assert_eq!(wrap_text("abc def", 7), vec!["abc def"]);
        // "abc de" 宽度 6，但 "abc def" 宽度 7 超出 6
        assert_eq!(wrap_text("abc def", 6), vec!["abc", "def"]);
    }

    #[test]
    fn wrap_multiple_lines() {
        assert_eq!(
            wrap_text("a b c d e f g h i j", 5),
            vec!["a b c", "d e f", "g h i", "j"]
        );
    }

    #[test]
    fn wrap_cjk_words() {
        // 中文字符之间无空格，每个字符视为一个 "词"（但 split_whitespace 不切中文）
        // 实际上 split_whitespace 按空白分割，所以纯中文无空格会作为整体
        let result = wrap_text("你好世界", 2);
        assert_eq!(result, vec!["你好世界"]);
    }

    // ==================== escape_for_display ====================

    #[test]
    fn escape_no_special_chars() {
        assert_eq!(escape_for_display("hello world"), "hello world");
    }

    #[test]
    fn escape_empty() {
        assert_eq!(escape_for_display(""), "");
    }

    #[test]
    fn escape_newline() {
        assert_eq!(escape_for_display("a\nb"), "a\\nb");
    }

    #[test]
    fn escape_carriage_return() {
        assert_eq!(escape_for_display("a\rb"), "a\\rb");
    }

    #[test]
    fn escape_tab() {
        assert_eq!(escape_for_display("a\tb"), "a\\tb");
    }

    #[test]
    fn escape_backslash() {
        // 反斜杠先转义，结果正确
        assert_eq!(escape_for_display("a\\b"), "a\\\\b");
    }

    #[test]
    fn escape_multiple() {
        assert_eq!(escape_for_display("a\nb\tc\rd\\e"), "a\\nb\\tc\\rd\\\\e");
    }

    #[test]
    fn escape_double_backslash_already_escaped_like() {
        assert_eq!(escape_for_display("\\\\"), "\\\\\\\\");
    }
}
