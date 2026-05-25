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
    '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!', '\\',
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

/// Like [`find_unescaped_positions`] but skips occurrences inside
/// fenced code blocks (```` ``` ````) or inline code spans
/// (`` ` ``). 1:1 port of upstream
/// `findUnescapedPositionsOutsideCode(text, marker)`. Returns
/// character-indices in `text`.
fn find_unescaped_positions_outside_code(text: &str, marker: char) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut positions: Vec<usize> = Vec::new();
    let mut in_fence = false;
    let mut in_inline = false;
    let mut backslashes = 0usize;

    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            backslashes += 1;
            i += 1;
            continue;
        }

        let escaped = backslashes % 2 == 1;
        backslashes = 0;

        if ch == '`' && !escaped {
            let is_triple = chars.get(i + 1) == Some(&'`') && chars.get(i + 2) == Some(&'`');
            if is_triple && !in_inline {
                in_fence = !in_fence;
                i += 3;
                continue;
            }
            if !in_fence {
                in_inline = !in_inline;
            }
            i += 1;
            continue;
        }

        if ch == marker && !escaped && !in_fence && !in_inline {
            positions.push(i);
        }
        i += 1;
    }

    positions
}

/// Drop any trailing characters that would produce invalid
/// MarkdownV2 after a length-based truncation: orphan trailing `\`,
/// unclosed entity delimiters (`*`, `_`, `~`, `` ` ``), or unmatched
/// `[`. 1:1 port of upstream private `trimToMarkdownV2SafeBoundary`.
fn trim_to_markdown_v2_safe_boundary(text: &str) -> String {
    let mut current: Vec<char> = text.chars().collect();
    let max_iterations = current.len() + 1;

    for _ in 0..max_iterations {
        let current_str: String = current.iter().collect();
        if ends_with_orphan_backslash(&current_str) {
            current.pop();
            continue;
        }

        let mut min_unsafe_position = current.len();

        for marker in ['*', '_', '~', '`'] {
            let positions = if marker == '`' {
                find_unescaped_positions(&current_str, marker)
            } else {
                find_unescaped_positions_outside_code(&current_str, marker)
            };
            if positions.len() % 2 == 1 {
                let last_unpaired = *positions.last().unwrap_or(&current.len());
                if last_unpaired < min_unsafe_position {
                    min_unsafe_position = last_unpaired;
                }
            }
        }

        let open_brackets = find_unescaped_positions_outside_code(&current_str, '[');
        let close_brackets = find_unescaped_positions_outside_code(&current_str, ']');
        if open_brackets.len() > close_brackets.len() {
            let last_open = *open_brackets.last().unwrap_or(&current.len());
            if last_open < min_unsafe_position {
                min_unsafe_position = last_open;
            }
        }

        if min_unsafe_position >= current.len() {
            return current.into_iter().collect();
        }

        current.truncate(min_unsafe_position);
    }

    current.into_iter().collect()
}

const MARKDOWN_V2_ELLIPSIS: &str = "\\.\\.\\.";
const PLAIN_ELLIPSIS: &str = "...";

/// Truncate a rendered string to `limit` characters, appending a
/// parse-mode-appropriate ellipsis. 1:1 port of upstream
/// `truncateForTelegram(text, limit, parseMode)`. For MarkdownV2 the
/// naive slice + "..." is unsafe (`.` is reserved + the slice can
/// leave orphan escape characters or cut through a paired entity),
/// so this uses an escaped ellipsis (`\.\.\.`) and trims back past
/// any unbalanced entity delimiter or orphan backslash before
/// appending.
pub fn truncate_for_telegram(text: &str, limit: usize, parse_mode: TelegramParseMode) -> String {
    let is_markdown_v2 = matches!(parse_mode, TelegramParseMode::MarkdownV2);
    let text_len = text.chars().count();

    if text_len <= limit {
        return if is_markdown_v2 {
            trim_to_markdown_v2_safe_boundary(text)
        } else {
            text.to_string()
        };
    }

    let ellipsis = if is_markdown_v2 {
        MARKDOWN_V2_ELLIPSIS
    } else {
        PLAIN_ELLIPSIS
    };
    let ellipsis_len = ellipsis.chars().count();

    let take = limit.saturating_sub(ellipsis_len);
    let slice: String = text.chars().take(take).collect();

    let slice = if is_markdown_v2 {
        trim_to_markdown_v2_safe_boundary(&slice)
    } else {
        slice
    };

    format!("{slice}{ellipsis}")
}

/// Escape text inside a code block. 1:1 with upstream
/// `escapeCodeBlock(text)`: only `` ` `` and `\` need escaping.
pub fn escape_code_block(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '`' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Escape text inside a link URL. 1:1 with upstream
/// `escapeLinkUrl(text)`: only `)` and `\` need escaping.
pub fn escape_link_url(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == ')' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Render an mdast node as Telegram MarkdownV2 text. 1:1 port of
/// upstream `renderMarkdownV2(node)`. Table/TableRow/TableCell
/// nodes are not supported - the caller (`from_ast`) preprocesses
/// them into code blocks before this function sees them.
pub fn render_markdown_v2(node: &chat_sdk_chat::markdown::Node) -> String {
    use chat_sdk_chat::markdown::Node;
    match node {
        Node::Root(root) => root
            .children
            .iter()
            .map(render_markdown_v2)
            .collect::<Vec<_>>()
            .join("\n\n"),
        Node::Paragraph(p) => p
            .children
            .iter()
            .map(render_markdown_v2)
            .collect::<Vec<_>>()
            .concat(),
        Node::Text(t) => escape_markdown_v2(&t.value),
        Node::Strong(s) => format!(
            "*{}*",
            s.children
                .iter()
                .map(render_markdown_v2)
                .collect::<Vec<_>>()
                .concat()
        ),
        Node::Emphasis(e) => format!(
            "_{}_",
            e.children
                .iter()
                .map(render_markdown_v2)
                .collect::<Vec<_>>()
                .concat()
        ),
        Node::Delete(d) => format!(
            "~{}~",
            d.children
                .iter()
                .map(render_markdown_v2)
                .collect::<Vec<_>>()
                .concat()
        ),
        Node::InlineCode(c) => format!("`{}`", escape_code_block(&c.value)),
        Node::Code(c) => {
            let lang = c.lang.as_deref().unwrap_or("");
            let val = escape_code_block(&c.value);
            format!("```{lang}\n{val}\n```")
        }
        Node::Link(l) => {
            let label = l
                .children
                .iter()
                .map(render_markdown_v2)
                .collect::<Vec<_>>()
                .concat();
            format!("[{label}]({})", escape_link_url(&l.url))
        }
        Node::Blockquote(b) => {
            let inner = b
                .children
                .iter()
                .map(render_markdown_v2)
                .collect::<Vec<_>>()
                .join("\n");
            inner
                .split('\n')
                .map(|line| format!(">{line}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Node::List(list) => list
            .children
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let content = if let Node::ListItem(li) = item {
                    li.children
                        .iter()
                        .map(render_markdown_v2)
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    render_markdown_v2(item)
                };
                if list.ordered {
                    format!("{} {content}", escape_markdown_v2(&format!("{}.", i + 1)))
                } else {
                    format!("\\- {content}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Node::ListItem(li) => li
            .children
            .iter()
            .map(render_markdown_v2)
            .collect::<Vec<_>>()
            .join("\n"),
        Node::Heading(h) => {
            let text = h
                .children
                .iter()
                .map(render_markdown_v2)
                .collect::<Vec<_>>()
                .concat();
            format!("*{text}*")
        }
        Node::ThematicBreak(_) => escape_markdown_v2("———"),
        Node::Break(_) => "\n".to_string(),
        Node::Image(img) => {
            let alt = escape_markdown_v2(&img.alt);
            let url = escape_link_url(&img.url);
            format!("[{alt}]({url})")
        }
        Node::Html(h) => escape_markdown_v2(&h.value),
        Node::LinkReference(lr) => {
            if !lr.children.is_empty() {
                lr.children
                    .iter()
                    .map(render_markdown_v2)
                    .collect::<Vec<_>>()
                    .concat()
            } else {
                escape_markdown_v2(&lr.identifier)
            }
        }
        Node::ImageReference(ir) => escape_markdown_v2(&ir.alt),
        Node::FootnoteReference(fr) => escape_markdown_v2(&format!("[^{}]", fr.identifier)),
        // Definition / FootnoteDefinition / Yaml / Toml: no visible
        // output upstream.
        Node::Definition(_) | Node::FootnoteDefinition(_) | Node::Yaml(_) | Node::Toml(_) => {
            String::new()
        }
        // Tables shouldn't reach the renderer; from_ast preprocesses
        // them into code blocks. Defensive fallback returns the ascii
        // table.
        Node::Table(t) => format!("```\n{}\n```", chat_sdk_chat::markdown::table_to_ascii(t)),
        Node::TableRow(_) | Node::TableCell(_) => String::new(),
        _ => String::new(),
    }
}

/// 1:1 port of upstream
/// `class TelegramFormatConverter extends BaseFormatConverter`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TelegramFormatConverter;

impl TelegramFormatConverter {
    /// 1:1 with upstream `new TelegramFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse Telegram text into mdast. 1:1 with upstream
    /// `toAst(text)`.
    pub fn to_ast(
        &self,
        text: &str,
    ) -> Result<chat_sdk_chat::markdown::Node, chat_sdk_chat::markdown::ParseMarkdownError> {
        chat_sdk_chat::markdown::parse_markdown(text)
    }

    /// Stringify mdast as Telegram MarkdownV2. 1:1 with upstream
    /// `fromAst(ast)`: preprocess Table nodes -> Code blocks, then
    /// `renderMarkdownV2`, then `.trim()`.
    pub fn from_ast(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        use chat_sdk_chat::markdown::{Code, Node, walk_ast};
        let transformed = walk_ast(ast.clone(), &mut |node: Node| -> Option<Node> {
            if let Node::Table(t) = &node {
                return Some(Node::Code(Code {
                    value: chat_sdk_chat::markdown::table_to_ascii(t),
                    lang: None,
                    meta: None,
                    position: None,
                }));
            }
            Some(node)
        });
        let s = render_markdown_v2(&transformed);
        s.trim().to_string()
    }

    /// Plain-string postable.
    pub fn render_postable_string(&self, message: &str) -> String {
        message.to_string()
    }

    /// `{raw}` postable.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// `{markdown}` postable: parse and Telegram-format.
    pub fn render_postable_markdown(
        &self,
        markdown: &str,
    ) -> Result<String, chat_sdk_chat::markdown::ParseMarkdownError> {
        let ast = self.to_ast(markdown)?;
        Ok(self.from_ast(&ast))
    }

    /// `{ast}` postable.
    pub fn render_postable_ast(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        self.from_ast(ast)
    }

    /// Extract plain text from Telegram MarkdownV2 input. 1:1 with upstream
    /// `BaseFormatConverter.extractPlainText`: `toPlainText(this.toAst(text))`.
    /// Falls back to the raw input when parsing fails (mirrors upstream's
    /// "AST construction never throws for these inputs" assumption while
    /// remaining defensive on the Rust side).
    pub fn extract_plain_text(&self, text: &str) -> String {
        match self.to_ast(text) {
            Ok(node) => chat_sdk_chat::markdown::to_plain_text(&node),
            Err(_) => text.to_string(),
        }
    }
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
            assert_eq!(escape_markdown_v2(&input), expected, "for char {ch:?}");
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

    // ---------- truncateForTelegram (9 upstream cases) ----------

    #[test]
    fn truncate_for_telegram_returns_text_unchanged_when_under_limit() {
        assert_eq!(
            truncate_for_telegram("hello", 100, TelegramParseMode::Plain),
            "hello"
        );
    }

    #[test]
    fn truncate_for_telegram_truncates_plain_text_with_literal_ellipsis() {
        let result = truncate_for_telegram(&"a".repeat(200), 100, TelegramParseMode::Plain);
        assert_eq!(result.chars().count(), 100);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_for_telegram_truncates_markdown_v2_with_escaped_ellipsis() {
        let result = truncate_for_telegram(&"a".repeat(200), 100, TelegramParseMode::MarkdownV2);
        assert!(result.chars().count() <= 100);
        assert!(result.ends_with("\\.\\.\\."));
    }

    #[test]
    fn truncate_for_telegram_strips_orphan_backslash_before_ellipsis() {
        let input = format!("{}\\{}", "a".repeat(90), "b".repeat(50));
        let result = truncate_for_telegram(&input, 100, TelegramParseMode::MarkdownV2);
        let before_ellipsis = result.replace("\\.\\.\\.", "");
        assert!(!ends_with_orphan_backslash(&before_ellipsis));
        assert!(result.ends_with("\\.\\.\\."));
    }

    #[test]
    fn truncate_for_telegram_strips_unclosed_bold_before_ellipsis() {
        let input = format!("{}*{}", "a".repeat(80), "b".repeat(100));
        let result = truncate_for_telegram(&input, 100, TelegramParseMode::MarkdownV2);
        let before_ellipsis = result.replace("\\.\\.\\.", "");
        let stars = before_ellipsis.chars().filter(|&c| c == '*').count();
        assert_eq!(stars % 2, 0);
    }

    #[test]
    fn truncate_for_telegram_handles_input_that_is_all_special_chars() {
        let input = ".".repeat(200);
        let rendered = escape_markdown_v2(&input);
        let result = truncate_for_telegram(&rendered, 100, TelegramParseMode::MarkdownV2);
        assert!(result.chars().count() <= 100);
        assert!(result.ends_with("\\.\\.\\."));
    }

    #[test]
    fn truncate_for_telegram_strips_unpaired_entity_markers_when_under_limit() {
        let input = "Hello *world* _italic and bold *bold*";
        let result = truncate_for_telegram(input, 4096, TelegramParseMode::MarkdownV2);
        let underscores = result.chars().filter(|&c| c == '_').count();
        assert_eq!(underscores % 2, 0);
    }

    #[test]
    fn truncate_for_telegram_preserves_code_fences_with_literal_asterisks() {
        let input = "```python\nprint(*args, **kwargs)\n```";
        let result = truncate_for_telegram(input, 4096, TelegramParseMode::MarkdownV2);
        assert_eq!(result, input);
    }

    #[test]
    fn truncate_for_telegram_does_not_modify_plain_parse_mode_messages() {
        let input = "Hello *world* _unclosed";
        let result = truncate_for_telegram(input, 4096, TelegramParseMode::Plain);
        assert_eq!(result, input);
    }

    #[test]
    fn truncate_for_telegram_preserves_balanced_markdown_v2_under_the_limit() {
        let input = "*bold* _italic_ ~strike~ `code`";
        let result = truncate_for_telegram(input, 4096, TelegramParseMode::MarkdownV2);
        assert_eq!(result, input);
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

    // ---------- TelegramFormatConverter.fromAst - inline formatting (8 cases) ----------

    fn rt(input: &str) -> String {
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast(input).unwrap();
        c.from_ast(&ast)
    }

    #[test]
    fn passes_plain_text_through_unchanged() {
        assert_eq!(rt("Hello world"), "Hello world");
    }

    #[test]
    fn renders_bold_with_single_asterisks() {
        assert_eq!(rt("**bold text**"), "*bold text*");
    }

    #[test]
    fn renders_italic_with_underscores() {
        assert_eq!(rt("*italic text*"), "_italic text_");
    }

    #[test]
    fn renders_strikethrough_with_single_tilde() {
        assert_eq!(rt("~~strikethrough~~"), "~strikethrough~");
    }

    #[test]
    fn escapes_special_chars_inside_bold() {
        assert_eq!(rt("**Note: important!**"), "*Note: important\\!*");
    }

    #[test]
    fn escapes_special_chars_inside_italic() {
        assert_eq!(rt("*price: $50.*"), "_price: $50\\._");
    }

    #[test]
    fn preserves_inline_code_content_verbatim() {
        assert!(rt("Use `const x = 1`").contains("`const x = 1`"));
    }

    #[test]
    fn escapes_only_backtick_and_backslash_inside_inline_code() {
        assert!(rt("Use `foo.bar!` here").contains("`foo.bar!`"));
    }

    // ---------- code blocks (3 cases) ----------

    #[test]
    fn wraps_code_blocks_with_triple_backticks_and_language() {
        let s = rt("```js\nconst x = 1;\n```");
        assert!(s.contains("```js"));
        assert!(s.contains("const x = 1;"));
        assert!(s.ends_with("```"));
    }

    #[test]
    fn escapes_only_backtick_and_backslash_inside_fenced_code() {
        let s = rt("```\nfoo.bar! + (test) = [ok]\n```");
        // Normal-text specials must NOT be escaped inside code blocks.
        assert!(s.contains("foo.bar! + (test) = [ok]"));
    }

    #[test]
    fn escapes_backslash_inside_fenced_code() {
        let s = rt("```\npath\\\\to\\\\file\n```");
        // Each `\\` source becomes `\\\\` after escape (each \ -> \\).
        assert!(s.contains("\\\\"));
    }

    // ---------- links (1 case) ----------

    #[test]
    fn renders_inline_links() {
        assert_eq!(
            rt("[click](https://example.com)"),
            "[click](https://example.com)"
        );
    }

    // ---------- renderPostable (6 upstream cases) ----------
    //
    // 1:1 with upstream `packages/adapter-telegram/src/markdown.test.ts`
    // `describe("renderPostable")` (L354-389). The Rust port exposes
    // the underlying postable variants as four typed methods
    // (`render_postable_string` / `render_postable_raw` /
    // `render_postable_markdown` / `render_postable_ast`); the
    // upstream `renderPostable(postable)` dispatches on the
    // `{raw|markdown|ast|string}` discriminator at runtime.

    #[test]
    fn render_postable_string_passthrough() {
        // 1:1 with upstream markdown.test.ts:355 > "returns a plain string as-is"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.render_postable_string("Hello world"), "Hello world");
    }

    #[test]
    fn render_postable_returns_an_empty_string_unchanged() {
        // 1:1 with upstream markdown.test.ts:359 > "returns an empty string unchanged"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.render_postable_string(""), "");
    }

    #[test]
    fn render_postable_raw_passthrough() {
        // 1:1 with upstream markdown.test.ts:363 > "returns a raw message directly"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.render_postable_raw("raw content"), "raw content");
    }

    #[test]
    fn render_postable_markdown_converts_to_v2() {
        // 1:1 with upstream markdown.test.ts:369 > "renders a markdown message as MarkdownV2"
        let c = TelegramFormatConverter::new();
        let result = c.render_postable_markdown("**bold** and *italic*").unwrap();
        assert!(result.contains("*bold*"), "got: {result}");
        assert!(result.contains("_italic_"), "got: {result}");
    }

    #[test]
    fn render_postable_ast_converts_to_v2() {
        // 1:1 with upstream markdown.test.ts:377 > "renders an AST message"
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast("Hello from AST").unwrap();
        assert!(c.render_postable_ast(&ast).contains("Hello from AST"));
    }

    #[test]
    fn render_postable_renders_a_markdown_table_as_a_code_block() {
        // 1:1 with upstream markdown.test.ts:382 > "renders a markdown table as a code block"
        let c = TelegramFormatConverter::new();
        let result = c
            .render_postable_markdown("| A | B |\n| --- | --- |\n| 1 | 2 |")
            .unwrap();
        assert!(result.contains("```"), "got: {result}");
        assert!(result.contains("A"), "got: {result}");
    }

    // ---------- links and images (4 upstream cases) ----------

    #[test]
    fn escapes_only_paren_and_backslash_inside_url() {
        let input = "[label](https://example.com/path)";
        assert_eq!(rt(input), "[label](https://example.com/path)");
    }

    #[test]
    fn escapes_special_chars_inside_link_label_text() {
        let output = rt("[hello!](https://example.com)");
        assert_eq!(output, "[hello\\!](https://example.com)");
    }

    #[test]
    fn renders_an_image_as_a_link_to_the_source() {
        let output = rt("![alt text](https://example.com/pic.png)");
        assert!(output.contains("alt text"), "got: {output}");
        assert!(
            output.contains("https://example.com/pic.png"),
            "got: {output}"
        );
    }

    // ---------- block structures (6 upstream cases) ----------

    #[test]
    fn renders_headings_as_bold_all_levels() {
        // 1:1 with upstream markdown.test.ts:176 > "renders headings as bold (all levels)"
        for level in 1..=6 {
            let hashes = "#".repeat(level);
            let output = rt(&format!("{hashes} Title"));
            assert_eq!(output, "*Title*", "level {level}");
        }
    }

    #[test]
    fn renders_unordered_lists_with_escaped_dashes() {
        // 1:1 with upstream markdown.test.ts:184 > "renders unordered lists with escaped dashes"
        let output = rt("- one\n- two");
        assert!(output.contains("\\- one"), "got: {output}");
        assert!(output.contains("\\- two"), "got: {output}");
    }

    #[test]
    fn renders_ordered_lists_with_escaped_periods() {
        // 1:1 with upstream markdown.test.ts:190 > "renders ordered lists with escaped periods"
        let output = rt("1. first\n2. second");
        assert!(output.contains("1\\. first"), "got: {output}");
        assert!(output.contains("2\\. second"), "got: {output}");
    }

    #[test]
    fn renders_blockquotes_with_gt_prefix_per_line() {
        // 1:1 with upstream markdown.test.ts:196 > "renders blockquotes with > prefix per line"
        let output = rt("> quoted text");
        assert!(output.contains(">quoted text"), "got: {output}");
    }

    #[test]
    fn renders_thematic_break_as_escaped_em_dashes() {
        // 1:1 with upstream markdown.test.ts:202 > "renders thematic break as escaped em-dashes"
        assert_eq!(rt("---"), "———");
    }

    #[test]
    fn converts_tables_to_ascii_code_blocks_and_drops_pipe_syntax() {
        // 1:1 with upstream markdown.test.ts:206 > "converts tables to ASCII code blocks and drops pipe syntax"
        let output = rt("| Name | Age |\n|------|-----|\n| Alice | 30 |");
        assert!(output.contains("```"), "got: {output}");
        assert!(output.contains("Name"), "got: {output}");
        assert!(output.contains("Alice"), "got: {output}");
        // Mirrors upstream `TABLE_PIPE_PATTERN = /\|.*Name.*\|/` —
        // assert no line still has `Name` between two `|` chars.
        let leaked = output.lines().any(|line| {
            line.find('|').is_some_and(|first| {
                let rest = &line[first + 1..];
                let name_idx = rest.find("Name");
                name_idx.is_some_and(|i| rest[i + "Name".len()..].contains('|'))
            })
        });
        assert!(!leaked, "raw pipe syntax leaked: {output}");
    }

    // ---------- nested formatting (3 upstream cases) ----------

    #[test]
    fn renders_bold_containing_italic() {
        let output = rt("**bold _italic_**");
        assert!(output.contains('*'), "got: {output}");
        assert!(output.contains("_italic_"), "got: {output}");
    }

    #[test]
    fn renders_link_containing_inline_code() {
        let output = rt("[`code` link](https://example.com)");
        assert!(output.contains("`code`"), "got: {output}");
        assert!(output.contains("https://example.com"), "got: {output}");
    }

    #[test]
    fn renders_list_containing_bold() {
        let output = rt("- **important** one\n- plain two");
        assert!(output.contains("*important*"), "got: {output}");
        assert!(output.contains("plain two"), "got: {output}");
    }

    // ---------- edge cases (4 upstream cases) ----------

    #[test]
    fn handles_empty_input() {
        assert_eq!(rt(""), "");
    }

    #[test]
    fn handles_whitespace_only_input() {
        assert_eq!(rt("   "), "");
    }

    #[test]
    fn trims_trailing_whitespace() {
        let output = rt("Hello\n\n");
        assert!(!output.ends_with('\n'), "got: {output:?}");
    }

    #[test]
    fn escapes_html_input_literally_rather_than_interpreting_it() {
        // Telegram MarkdownV2 has no HTML support; raw HTML must not crash
        // and must not appear as `<b>` in output.
        let output = rt("<b>hi</b>");
        assert!(!output.contains("<b>"), "got: {output}");
    }

    // ---------- corpus validity invariants (3 upstream cases) ----------

    const LLM_CORPUS: &str = "# Trip Summary: Morocco\n\nHere's your **personalized** 7-day itinerary. Price: $2,450 per person (all-inclusive)!\n\n## Day 1 — Arrival in Marrakech\n\n- Airport pickup at 14:30\n- Check-in at *Riad El Fenn* (4-star)\n- Welcome dinner: [La Mamounia](https://www.mamounia.com/restaurants)\n\n> Tip: bring cash — not every souk accepts cards.\n\n## Day 2 — Atlas Mountains\n\n1. 08:00 breakfast\n2. 09:00 departure (2h drive)\n3. Hike to Toubkal base camp\n\nPack: `sunscreen`, `hiking boots`, *layers* (temperatures drop ~10°C).\n\n```bash\n# Exchange rate check\ncurl 'https://api.rates.io/MAD' | jq '.rate'\n```\n\n| Day | Activity | Cost |\n|-----|----------|------|\n| 1 | Arrival | $200 |\n| 2 | Atlas | $350 |\n\n---\n\n~~Previous version priced at $2,800~~. New total: **$2,450**.";

    #[test]
    fn corpus_produces_non_empty_output_covering_every_structural_element() {
        let output = rt(LLM_CORPUS);
        assert!(output.contains("*Trip Summary"), "got: {output}");
        assert!(output.contains("\\- Airport pickup"), "got: {output}");
        assert!(output.contains("1\\. 08:00 breakfast"), "got: {output}");
        assert!(output.contains("_Riad El Fenn_"), "got: {output}");
        assert!(
            output.contains("[La Mamounia](https://www.mamounia.com/restaurants)"),
            "got: {output}"
        );
        assert!(output.contains(">Tip:"), "got: {output}");
        assert!(output.contains("```"), "got: {output}");
        assert!(output.contains("~Previous version"), "got: {output}");
        assert!(output.contains("———"), "got: {output}");
    }

    #[test]
    fn corpus_escapes_every_in_text_markdown_v2_special_outside_code_and_link_urls() {
        let output = rt(LLM_CORPUS);
        // Strip code blocks (```...```), inline code (`...`), and link URLs
        // (the content between `](` and `)`).
        let stripped = strip_code_blocks_inline_and_link_urls(&output);

        // For each text-only special char, any occurrence in stripped text
        // must be preceded by a backslash. 1:1 with upstream's regex sweep.
        for ch in ['+', '=', '{', '}', '.', '!', '|'] {
            let chars: Vec<char> = stripped.chars().collect();
            for (i, &c) in chars.iter().enumerate() {
                if c != ch {
                    continue;
                }
                let prev = if i == 0 { '\0' } else { chars[i - 1] };
                assert_eq!(
                    prev, '\\',
                    "found unescaped {ch:?} at position {i} in {stripped:?}"
                );
            }
        }
    }

    #[test]
    fn corpus_preserves_code_block_contents_verbatim_no_over_escaping() {
        // 1:1 with upstream markdown.test.ts:342 > "preserves code block contents verbatim (no over-escaping)"
        let output = rt(LLM_CORPUS);
        // Find the bash fenced code block.
        let start = output.find("```bash").expect("bash fence open");
        let after_open = &output[start + "```bash".len()..];
        // Skip the newline after the language tag.
        let body_start = after_open.find('\n').expect("newline after ```bash") + 1;
        let body_start_abs = start + "```bash".len() + body_start;
        let body_end_rel = output[body_start_abs..]
            .find("```")
            .expect("bash fence close");
        let code_content = &output[body_start_abs..body_start_abs + body_end_rel];
        // These symbols must appear literally - MarkdownV2 only
        // escapes ` and \ inside code blocks.
        assert!(code_content.contains('\''), "got: {code_content}");
        assert!(code_content.contains('|'), "got: {code_content}");
        assert!(code_content.contains('.'), "got: {code_content}");
    }

    /// Test helper: strip ```…``` fenced blocks, `…` inline-code, and
    /// link-URL bodies (`](…)` -> `]()`) from MarkdownV2 output. 1:1
    /// with the upstream regex stripping in the corpus test.
    fn strip_code_blocks_inline_and_link_urls(text: &str) -> String {
        // 1. Strip fenced code blocks.
        let mut out = String::new();
        let mut rest = text;
        loop {
            match rest.find("```") {
                None => {
                    out.push_str(rest);
                    break;
                }
                Some(i) => {
                    out.push_str(&rest[..i]);
                    let after_open = &rest[i + 3..];
                    match after_open.find("```") {
                        None => {
                            // Unterminated - keep rest as-is.
                            out.push_str(&rest[i..]);
                            break;
                        }
                        Some(j) => {
                            rest = &after_open[j + 3..];
                        }
                    }
                }
            }
        }

        // 2. Strip inline code `…`.
        let mut tmp = String::new();
        let bytes = out.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                if let Some(rel) = out[i + 1..].find('`') {
                    i = i + 1 + rel + 1;
                    continue;
                }
            }
            tmp.push(bytes[i] as char);
            i += 1;
        }

        // 3. Replace link-url bodies: `](…)` -> `]()`. Scan for `](`
        // sequences and skip to matching `)`.
        let mut out2 = String::new();
        let chars: Vec<char> = tmp.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == ']' && chars[i + 1] == '(' {
                out2.push_str("](");
                // Skip until matching close paren (non-escaped).
                let mut j = i + 2;
                while j < chars.len() {
                    if chars[j] == ')' && (j == 0 || chars[j - 1] != '\\') {
                        break;
                    }
                    j += 1;
                }
                out2.push(')');
                i = j + 1;
                continue;
            }
            out2.push(chars[i]);
            i += 1;
        }
        out2
    }

    // ---------- toAst (4 upstream cases) ----------
    //
    // 1:1 with upstream `packages/adapter-telegram/src/markdown.test.ts`
    // `describe("toAst")` (L391-415). Each case asserts the parsed
    // root carries `type === "root"` and has at least one child.

    fn assert_root_with_children(ast: &chat_sdk_chat::markdown::Node) {
        let chat_sdk_chat::markdown::Node::Root(root) = ast else {
            panic!("expected Root node, got {ast:?}");
        };
        assert!(!root.children.is_empty(), "expected non-empty children");
    }

    #[test]
    fn to_ast_parses_plain_text() {
        // 1:1 with upstream markdown.test.ts:392 > "parses plain text"
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast("Hello world").unwrap();
        assert_root_with_children(&ast);
    }

    #[test]
    fn to_ast_parses_bold() {
        // 1:1 with upstream markdown.test.ts:398 > "parses bold"
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast("**bold**").unwrap();
        assert_root_with_children(&ast);
    }

    #[test]
    fn to_ast_parses_italic() {
        // 1:1 with upstream markdown.test.ts:404 > "parses italic"
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast("*italic*").unwrap();
        assert_root_with_children(&ast);
    }

    #[test]
    fn to_ast_parses_inline_code() {
        // 1:1 with upstream markdown.test.ts:410 > "parses inline code"
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast("`code`").unwrap();
        assert_root_with_children(&ast);
    }

    // ---------- escape helpers ----------

    #[test]
    fn escape_code_block_handles_backtick_and_backslash() {
        assert_eq!(escape_code_block("a`b\\c"), "a\\`b\\\\c");
    }

    #[test]
    fn escape_link_url_handles_paren_and_backslash() {
        assert_eq!(escape_link_url("a)b\\c"), "a\\)b\\\\c");
    }

    // ---------- extractPlainText (9 upstream cases) ----------
    //
    // Ported 1:1 from upstream
    // `packages/adapter-telegram/src/markdown.test.ts`
    // `describe("extractPlainText")` (lines 417-470). The upstream method is
    // inherited from `BaseFormatConverter` and defined as
    // `toPlainText(this.toAst(text))`. The Rust port mirrors that via
    // `chat_sdk_chat::markdown::to_plain_text` over our `to_ast` output.

    #[test]
    fn extract_plain_text_strips_bold_markers() {
        // 1:1 with upstream markdown.test.ts:418 > "strips bold markers"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.extract_plain_text("Hello **world**!"), "Hello world!");
    }

    #[test]
    fn extract_plain_text_strips_italic_markers() {
        // 1:1 with upstream markdown.test.ts:424 > "strips italic markers"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.extract_plain_text("Hello *world*!"), "Hello world!");
    }

    #[test]
    fn extract_plain_text_strips_strikethrough_markers() {
        // 1:1 with upstream markdown.test.ts:428 > "strips strikethrough markers"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.extract_plain_text("Hello ~~world~~!"), "Hello world!");
    }

    #[test]
    fn extract_plain_text_extracts_link_text() {
        // 1:1 with upstream markdown.test.ts:434 > "extracts link text"
        let c = TelegramFormatConverter::new();
        assert_eq!(
            c.extract_plain_text("Check [this](https://example.com)"),
            "Check this"
        );
    }

    #[test]
    fn extract_plain_text_preserves_inline_code_content() {
        // 1:1 with upstream markdown.test.ts:440 > "preserves inline code content"
        let c = TelegramFormatConverter::new();
        assert!(
            c.extract_plain_text("Use `const x = 1`")
                .contains("const x = 1")
        );
    }

    #[test]
    fn extract_plain_text_preserves_code_block_content() {
        // 1:1 with upstream markdown.test.ts:446 > "preserves code block content"
        let c = TelegramFormatConverter::new();
        assert!(
            c.extract_plain_text("```js\nconst x = 1;\n```")
                .contains("const x = 1;")
        );
    }

    #[test]
    fn extract_plain_text_returns_plain_text_unchanged() {
        // 1:1 with upstream markdown.test.ts:452 > "returns plain text unchanged"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.extract_plain_text("Hello world"), "Hello world");
    }

    #[test]
    fn extract_plain_text_returns_empty_string_unchanged() {
        // 1:1 with upstream markdown.test.ts:456 > "returns empty string unchanged"
        let c = TelegramFormatConverter::new();
        assert_eq!(c.extract_plain_text(""), "");
    }

    #[test]
    fn extract_plain_text_strips_all_formatting_from_complex_input() {
        // 1:1 with upstream markdown.test.ts:460 > "strips all formatting from complex input"
        let c = TelegramFormatConverter::new();
        let result = c.extract_plain_text("**Bold** and *italic* with [link](https://x.com)");
        assert!(result.contains("Bold"));
        assert!(result.contains("italic"));
        assert!(result.contains("link"));
        assert!(!result.contains("**"));
        assert!(!result.contains("]("));
    }
}
