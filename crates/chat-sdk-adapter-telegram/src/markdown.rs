//! Telegram MarkdownV2 escape + scanning helpers.
//!
//! 1:1 port of the pure-function subset of
//! `packages/adapter-telegram/src/markdown.ts`. The full
//! `TelegramFormatConverter` (AST <-> MarkdownV2) is deferred
//! until `stringify_markdown` lands in chat-sdk-chat; until then
//! this module covers the helpers that already function
//! standalone: `escape_markdown_v2`, `find_unescaped_positions`,
//! `ends_with_orphan_backslash`, `to_bot_api_parse_mode`, and
//! the Telegram length-limit constants.

/// Internal parse mode. 1:1 with upstream
/// `type TelegramParseMode = "MarkdownV2" | "plain"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TelegramParseMode {
    /// Telegram MarkdownV2 (escaped, parsed by the Bot API).
    MarkdownV2,
    /// Plain text, no entity parsing. `parse_mode` field is
    /// omitted from the Bot API request.
    Plain,
}

/// Maximum length of a Telegram text message body in characters.
/// 1:1 with upstream `TELEGRAM_MESSAGE_LIMIT = 4096`.
pub const TELEGRAM_MESSAGE_LIMIT: usize = 4096;

/// Maximum length of a media caption in characters. 1:1 with
/// upstream `TELEGRAM_CAPTION_LIMIT = 1024`.
pub const TELEGRAM_CAPTION_LIMIT: usize = 1024;

/// MarkdownV2 special characters that must be backslash-escaped
/// when appearing outside of an entity. 1:1 with upstream's
/// `MARKDOWNV2_SPECIAL_CHARS = /([_*[\]()~`>#+\-=|{}.!\\])/g`.
const MARKDOWNV2_SPECIAL_CHARS: &[char] = &[
    '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    '\\',
];

/// Translate the internal parse mode to the Bot API wire value.
/// Returns `None` for plain so the `parse_mode` field is omitted.
/// 1:1 port of upstream `toBotApiParseMode(mode)`.
pub fn to_bot_api_parse_mode(mode: TelegramParseMode) -> Option<&'static str> {
    match mode {
        TelegramParseMode::MarkdownV2 => Some("MarkdownV2"),
        TelegramParseMode::Plain => None,
    }
}

/// Escape text for use in normal MarkdownV2 context (outside of
/// entities). 1:1 port of upstream
/// `escapeMarkdownV2(text)`.
pub fn escape_markdown_v2(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if MARKDOWNV2_SPECIAL_CHARS.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Return char-indices (0-based, byte-aligned for ASCII inputs;
/// byte-position for unicode) of every occurrence of `marker` in
/// `text` that is NOT preceded by an odd number of backslashes.
/// 1:1 port of upstream
/// `findUnescapedPositions(text, marker)`. Upstream operates on
/// UTF-16 string indices via `text[i]`; the Rust port mirrors
/// that on Unicode scalars (the upstream function is only used
/// on ASCII markers and short ASCII strings — no surrogate-pair
/// observed callsites).
pub fn find_unescaped_positions(text: &str, marker: char) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut positions: Vec<usize> = Vec::new();
    for (i, &ch) in chars.iter().enumerate() {
        if ch != marker {
            continue;
        }
        let mut backslashes = 0usize;
        let mut j = i as isize - 1;
        while j >= 0 && chars[j as usize] == '\\' {
            backslashes += 1;
            j -= 1;
        }
        if backslashes % 2 == 0 {
            positions.push(i);
        }
    }
    positions
}

/// Whether `text` ends with an odd number of trailing backslashes
/// (i.e. an "orphan" `\` that would escape whatever follows).
/// 1:1 port of upstream `endsWithOrphanBackslash(text)`.
pub fn ends_with_orphan_backslash(text: &str) -> bool {
    let mut trailing = 0usize;
    for ch in text.chars().rev() {
        if ch == '\\' {
            trailing += 1;
        } else {
            break;
        }
    }
    trailing % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- escapeMarkdownV2 (4 + 19 char cases) ----------

    #[test]
    fn escape_markdown_v2_leaves_non_special_ascii_untouched() {
        assert_eq!(escape_markdown_v2("Hello world 123"), "Hello world 123");
    }

    #[test]
    fn escape_markdown_v2_leaves_unicode_characters_untouched() {
        assert_eq!(escape_markdown_v2("café — €50"), "café — €50");
    }

    #[test]
    fn escape_markdown_v2_escapes_multiple_special_characters_in_one_string() {
        assert_eq!(escape_markdown_v2("a.b!c(d)"), "a\\.b\\!c\\(d\\)");
    }

    #[test]
    fn escape_markdown_v2_handles_empty_input() {
        assert_eq!(escape_markdown_v2(""), "");
    }

    #[test]
    fn escape_markdown_v2_escapes_every_special_character() {
        // 1:1 with upstream's `for (const char of MARKDOWNV2_SPECIAL_CHARS)
        // it("escapes the special character ...")` parametric loop.
        for &ch in MARKDOWNV2_SPECIAL_CHARS {
            let input = format!("a{ch}b");
            let expected = format!("a\\{ch}b");
            assert_eq!(
                escape_markdown_v2(&input),
                expected,
                "for char {ch:?}"
            );
        }
    }

    // ---------- findUnescapedPositions (4 cases) ----------

    #[test]
    fn find_unescaped_positions_finds_unescaped_markers() {
        assert_eq!(find_unescaped_positions("*a*", '*'), vec![0, 2]);
    }

    #[test]
    fn find_unescaped_positions_ignores_escaped_markers() {
        // "\\*a*" - JS string with 4 chars: \, *, a, *
        // The * at index 1 IS escaped; the * at index 3 is not.
        assert_eq!(find_unescaped_positions("\\*a*", '*'), vec![3]);
    }

    #[test]
    fn find_unescaped_positions_handles_double_backslash_before_marker() {
        // "\\\\*" - JS string with 3 chars: \, \, *
        // Two backslashes is an escaped backslash; the * at index 2 is
        // NOT escaped.
        assert_eq!(find_unescaped_positions("\\\\*", '*'), vec![2]);
    }

    #[test]
    fn find_unescaped_positions_returns_empty_for_no_markers() {
        assert_eq!(find_unescaped_positions("hello", '*'), Vec::<usize>::new());
    }

    // ---------- endsWithOrphanBackslash (5 cases) ----------

    #[test]
    fn ends_with_orphan_backslash_returns_true_for_single_trailing_backslash() {
        assert!(ends_with_orphan_backslash("abc\\"));
    }

    #[test]
    fn ends_with_orphan_backslash_returns_false_for_double_trailing_backslash() {
        assert!(!ends_with_orphan_backslash("abc\\\\"));
    }

    #[test]
    fn ends_with_orphan_backslash_returns_true_for_triple_trailing_backslash() {
        assert!(ends_with_orphan_backslash("abc\\\\\\"));
    }

    #[test]
    fn ends_with_orphan_backslash_returns_false_for_no_trailing_backslash() {
        assert!(!ends_with_orphan_backslash("abc"));
    }

    #[test]
    fn ends_with_orphan_backslash_returns_false_for_empty_string() {
        assert!(!ends_with_orphan_backslash(""));
    }

    // ---------- additive Rust-side ----------

    #[test]
    fn to_bot_api_parse_mode_maps_markdown_v2_and_drops_plain() {
        assert_eq!(
            to_bot_api_parse_mode(TelegramParseMode::MarkdownV2),
            Some("MarkdownV2")
        );
        assert_eq!(to_bot_api_parse_mode(TelegramParseMode::Plain), None);
    }

    #[test]
    fn length_limits_match_upstream() {
        assert_eq!(TELEGRAM_MESSAGE_LIMIT, 4096);
        assert_eq!(TELEGRAM_CAPTION_LIMIT, 1024);
    }
}
