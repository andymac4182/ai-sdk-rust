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

    // ---------- renderPostable ----------

    #[test]
    fn render_postable_string_passthrough() {
        let c = TelegramFormatConverter::new();
        assert_eq!(c.render_postable_string("hi"), "hi");
    }

    #[test]
    fn render_postable_raw_passthrough() {
        let c = TelegramFormatConverter::new();
        assert_eq!(c.render_postable_raw("raw"), "raw");
    }

    #[test]
    fn render_postable_markdown_converts_to_v2() {
        let c = TelegramFormatConverter::new();
        let result = c.render_postable_markdown("**bold**").unwrap();
        assert_eq!(result, "*bold*");
    }

    #[test]
    fn render_postable_ast_converts_to_v2() {
        let c = TelegramFormatConverter::new();
        let ast = c.to_ast("**bold**").unwrap();
        assert_eq!(c.render_postable_ast(&ast), "*bold*");
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
        for level in 1..=6 {
            let hashes = "#".repeat(level);
            let output = rt(&format!("{hashes} Title"));
            assert_eq!(output, "*Title*", "level {level}");
        }
    }

    #[test]
    fn renders_unordered_lists_with_escaped_dashes() {
        let output = rt("- one\n- two");
        assert!(output.contains("\\- one"), "got: {output}");
        assert!(output.contains("\\- two"), "got: {output}");
    }

    #[test]
    fn renders_ordered_lists_with_escaped_periods() {
        let output = rt("1. first\n2. second");
        assert!(output.contains("1\\. first"), "got: {output}");
        assert!(output.contains("2\\. second"), "got: {output}");
    }

    #[test]
    fn renders_blockquotes_with_gt_prefix_per_line() {
        let output = rt("> quoted text");
        assert!(output.contains(">quoted text"), "got: {output}");
    }

    #[test]
    fn renders_thematic_break_as_escaped_em_dashes() {
        assert_eq!(rt("---"), "———");
    }

    #[test]
    fn converts_tables_to_ascii_code_blocks_and_drops_pipe_syntax() {
        let output = rt("| Name | Age |\n|------|-----|\n| Alice | 30 |");
        assert!(output.contains("```"), "got: {output}");
        assert!(output.contains("Name"), "got: {output}");
        assert!(output.contains("Alice"), "got: {output}");
        // Should not contain raw pipe-table syntax (a `|` adjacent to text).
        assert!(
            !output.contains("| Name "),
            "raw pipe syntax leaked: {output}"
        );
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

    // ---------- escape helpers ----------

    #[test]
    fn escape_code_block_handles_backtick_and_backslash() {
        assert_eq!(escape_code_block("a`b\\c"), "a\\`b\\\\c");
    }

    #[test]
    fn escape_link_url_handles_paren_and_backslash() {
        assert_eq!(escape_link_url("a)b\\c"), "a\\)b\\\\c");
    }
}
