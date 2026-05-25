//! Discord markdown <-> standard Markdown conversion.
//!
//! 1:1 port of `packages/adapter-discord/src/markdown.ts`.
//!
//! Discord uses standard CommonMark with extensions:
//! - Bold: `**text**` (standard)
//! - Italic: `*text*` or `_text_` (standard)
//! - Strikethrough: `~~text~~` (standard GFM)
//! - Links: `[text](url)` (standard)
//! - User mentions: `<@userId>` / `<@!userId>` (nickname marker)
//! - Channel mentions: `<#channelId>`
//! - Role mentions: `<@&roleId>`
//! - Custom emoji: `<:name:id>` or `<a:name:id>` (animated)
//! - Spoiler: `||text||`

use chat_sdk_chat::markdown::{
    Node, default_node_to_text, from_ast_with_node_converter, get_node_children,
    is_blockquote_node, is_code_node, is_delete_node, is_emphasis_node, is_inline_code_node,
    is_link_node, is_list_node, is_paragraph_node, is_strong_node, is_table_node, is_text_node,
    parse_markdown, render_list, table_to_ascii, to_plain_text,
};
use chat_sdk_chat::types::AdapterPostableMessage;

/// 1:1 port of upstream
/// `class DiscordFormatConverter extends BaseFormatConverter`.
/// Stateless; the struct mirrors upstream shape.
#[derive(Debug, Default, Clone, Copy)]
pub struct DiscordFormatConverter;

impl DiscordFormatConverter {
    /// 1:1 with upstream `new DiscordFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse Discord markdown into an mdast tree. 1:1 with upstream
    /// `toAst(discordMarkdown)`: rewrites Discord-specific mention /
    /// emoji / spoiler syntax to standard markdown, then parses.
    pub fn to_ast(&self, discord_markdown: &str) -> Node {
        let mut s = discord_markdown.to_string();
        s = rewrite_user_mentions(&s);
        s = rewrite_channel_mentions(&s);
        s = rewrite_role_mentions(&s);
        s = rewrite_custom_emoji(&s);
        s = rewrite_spoiler_tags(&s);
        parse_markdown(&s).unwrap_or_else(|_| {
            Node::Root(chat_sdk_chat::markdown::Root {
                children: vec![],
                position: None,
            })
        })
    }

    /// Render an mdast tree to Discord markdown. 1:1 with upstream
    /// `fromAst(ast)`: walks every node via `node_to_discord_markdown`
    /// using the inherited `fromAstWithNodeConverter`.
    pub fn from_ast(&self, ast: &Node) -> String {
        from_ast_with_node_converter(ast, &|n| node_to_discord_markdown(n))
    }

    /// Plain-text extraction. 1:1 with the inherited
    /// `BaseFormatConverter.extractPlainText`: parses platform text then
    /// runs `to_plain_text`.
    pub fn extract_plain_text(&self, discord_text: &str) -> String {
        to_plain_text(&self.to_ast(discord_text))
    }

    /// Render a postable message for Discord. 1:1 port of upstream
    /// `renderPostable(message)`:
    ///
    /// - `string` / `{ raw }` -> `convertMentionsToDiscord(text)` (bare
    ///   `@user` -> `<@user>`)
    /// - `{ markdown }` -> `fromAst(parseMarkdown(markdown))`
    /// - `{ ast }` -> `fromAst(ast)`
    pub fn render_postable(&self, message: &AdapterPostableMessage) -> String {
        match message {
            AdapterPostableMessage::Text(s) => convert_mentions_to_discord(s),
            AdapterPostableMessage::Raw(r) => convert_mentions_to_discord(&r.raw),
            AdapterPostableMessage::Markdown(m) => match parse_markdown(&m.markdown) {
                Ok(ast) => self.from_ast(&ast),
                Err(_) => m.markdown.clone(),
            },
            AdapterPostableMessage::Ast(a) => {
                let root = Node::Root(a.ast.clone());
                self.from_ast(&root)
            }
            AdapterPostableMessage::Card(_) | AdapterPostableMessage::CardElement(_) => {
                String::new()
            }
        }
    }
}

/// Apply `/@(\w+)/g -> <@$1>`. 1:1 with upstream
/// `convertMentionsToDiscord`. No lookbehind - matches every `@word`.
fn convert_mentions_to_discord(text: &str) -> String {
    fn is_word(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let mut j = i + 1;
            while j < bytes.len() && is_word(bytes[j]) {
                j += 1;
            }
            if j > i + 1 {
                out.push_str("<@");
                out.push_str(&text[i + 1..j]);
                out.push('>');
                i = j;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xc0 {
        1
    } else if b < 0xe0 {
        2
    } else if b < 0xf0 {
        3
    } else {
        4
    }
}

/// `<@userId>` or `<@!userId>` -> `@userId`. 1:1 with upstream regex
/// `/<@!?(\w+)>/g`.
fn rewrite_user_mentions(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'@' {
            // Check for optional `!` after `@`, then word chars, then `>`.
            let after_at = i + 2;
            let id_start = if after_at < bytes.len() && bytes[after_at] == b'!' {
                after_at + 1
            } else {
                after_at
            };
            // No `&` (that's a role mention).
            if id_start < bytes.len() && bytes[id_start] != b'&' {
                let mut j = id_start;
                while j < bytes.len() && is_word_byte(bytes[j]) {
                    j += 1;
                }
                if j > id_start && j < bytes.len() && bytes[j] == b'>' {
                    out.push('@');
                    out.push_str(&text[id_start..j]);
                    i = j + 1;
                    continue;
                }
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// `<#channelId>` -> `#channelId`. 1:1 with `/<#(\w+)>/g`.
fn rewrite_channel_mentions(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'#' {
            let id_start = i + 2;
            let mut j = id_start;
            while j < bytes.len() && is_word_byte(bytes[j]) {
                j += 1;
            }
            if j > id_start && j < bytes.len() && bytes[j] == b'>' {
                out.push('#');
                out.push_str(&text[id_start..j]);
                i = j + 1;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// `<@&roleId>` -> `@&roleId`. 1:1 with `/<@&(\w+)>/g`.
fn rewrite_role_mentions(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 3 < bytes.len() && bytes[i + 1] == b'@' && bytes[i + 2] == b'&' {
            let id_start = i + 3;
            let mut j = id_start;
            while j < bytes.len() && is_word_byte(bytes[j]) {
                j += 1;
            }
            if j > id_start && j < bytes.len() && bytes[j] == b'>' {
                out.push_str("@&");
                out.push_str(&text[id_start..j]);
                i = j + 1;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// `<:name:id>` or `<a:name:id>` -> `:name:`. 1:1 with
/// `/<a?:(\w+):\d+>/g`.
fn rewrite_custom_emoji(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Optional 'a' (animated).
            let mut p = i + 1;
            if p < bytes.len() && bytes[p] == b'a' {
                p += 1;
            }
            if p < bytes.len() && bytes[p] == b':' {
                let name_start = p + 1;
                let mut j = name_start;
                while j < bytes.len() && is_word_byte(bytes[j]) {
                    j += 1;
                }
                if j > name_start && j < bytes.len() && bytes[j] == b':' {
                    let digits_start = j + 1;
                    let mut k = digits_start;
                    while k < bytes.len() && bytes[k].is_ascii_digit() {
                        k += 1;
                    }
                    if k > digits_start && k < bytes.len() && bytes[k] == b'>' {
                        out.push(':');
                        out.push_str(&text[name_start..j]);
                        out.push(':');
                        i = k + 1;
                        continue;
                    }
                }
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// `||text||` -> `[spoiler: text]`. 1:1 with `/\|\|([^|]+)\|\|/g`.
fn rewrite_spoiler_tags(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'|' && bytes[i + 1] == b'|' {
            let inner_start = i + 2;
            let mut j = inner_start;
            while j < bytes.len() && bytes[j] != b'|' {
                j += 1;
            }
            if j > inner_start && j + 1 < bytes.len() && bytes[j] == b'|' && bytes[j + 1] == b'|' {
                out.push_str("[spoiler: ");
                out.push_str(&text[inner_start..j]);
                out.push(']');
                i = j + 2;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 1:1 port of upstream `nodeToDiscordMarkdown(node)`: renders a single
/// mdast node into Discord markdown text.
fn node_to_discord_markdown(node: &Node) -> String {
    if is_paragraph_node(node) {
        return get_node_children(node)
            .iter()
            .map(node_to_discord_markdown)
            .collect::<Vec<_>>()
            .concat();
    }
    if is_text_node(node) {
        if let Node::Text(t) = node {
            return convert_mentions_to_discord(&t.value);
        }
    }
    if is_strong_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_discord_markdown)
            .collect::<Vec<_>>()
            .concat();
        return format!("**{content}**");
    }
    if is_emphasis_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_discord_markdown)
            .collect::<Vec<_>>()
            .concat();
        return format!("*{content}*");
    }
    if is_delete_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_discord_markdown)
            .collect::<Vec<_>>()
            .concat();
        return format!("~~{content}~~");
    }
    if is_inline_code_node(node) {
        if let Node::InlineCode(c) = node {
            return format!("`{}`", c.value);
        }
    }
    if is_code_node(node) {
        if let Node::Code(c) = node {
            let lang = c.lang.as_deref().unwrap_or("");
            return format!("```{lang}\n{}\n```", c.value);
        }
    }
    if is_link_node(node) {
        if let Node::Link(l) = node {
            let link_text: String = get_node_children(node)
                .iter()
                .map(node_to_discord_markdown)
                .collect::<Vec<_>>()
                .concat();
            return format!("[{link_text}]({})", l.url);
        }
    }
    if is_blockquote_node(node) {
        return get_node_children(node)
            .iter()
            .map(|child| format!("> {}", node_to_discord_markdown(child)))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if is_list_node(node) {
        if let Node::List(l) = node {
            // Discord uses the default unordered bullet "-".
            return render_list(l, 0, &|child| node_to_discord_markdown(child), "-");
        }
    }
    if matches!(node, Node::Break(_)) {
        return "\n".into();
    }
    if matches!(node, Node::ThematicBreak(_)) {
        return "---".into();
    }
    if is_table_node(node) {
        if let Node::Table(t) = node {
            return format!("```\n{}\n```", table_to_ascii(t));
        }
    }
    default_node_to_text(node, &|child| node_to_discord_markdown(child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::types::{PostableAst, PostableMarkdown, PostableRaw};

    fn converter() -> DiscordFormatConverter {
        DiscordFormatConverter::new()
    }

    // ---------- fromAst (AST -> Discord markdown), 8 upstream cases ----------

    #[test]
    fn from_ast_should_convert_bold() {
        let c = converter();
        let ast = c.to_ast("**bold text**");
        let result = c.from_ast(&ast);
        assert!(result.contains("**bold text**"), "got: {result}");
    }

    #[test]
    fn from_ast_should_convert_italic() {
        let c = converter();
        let ast = c.to_ast("*italic text*");
        let result = c.from_ast(&ast);
        assert!(result.contains("*italic text*"), "got: {result}");
    }

    #[test]
    fn from_ast_should_convert_strikethrough() {
        let c = converter();
        let ast = c.to_ast("~~strikethrough~~");
        let result = c.from_ast(&ast);
        assert!(result.contains("~~strikethrough~~"), "got: {result}");
    }

    #[test]
    fn from_ast_should_convert_links() {
        let c = converter();
        let ast = c.to_ast("[link text](https://example.com)");
        let result = c.from_ast(&ast);
        assert!(
            result.contains("[link text](https://example.com)"),
            "got: {result}"
        );
    }

    #[test]
    fn from_ast_should_preserve_inline_code() {
        let c = converter();
        let ast = c.to_ast("Use `const x = 1`");
        let result = c.from_ast(&ast);
        assert!(result.contains("`const x = 1`"), "got: {result}");
    }

    #[test]
    fn from_ast_should_handle_code_blocks() {
        let c = converter();
        let ast = c.to_ast("```js\nconst x = 1;\n```");
        let result = c.from_ast(&ast);
        assert!(result.contains("```"), "got: {result}");
        assert!(result.contains("const x = 1;"), "got: {result}");
    }

    #[test]
    fn from_ast_should_handle_mixed_formatting() {
        let c = converter();
        let ast = c.to_ast("**Bold** and *italic* and [link](https://x.com)");
        let result = c.from_ast(&ast);
        assert!(result.contains("**Bold**"), "got: {result}");
        assert!(result.contains("*italic*"), "got: {result}");
        assert!(result.contains("[link](https://x.com)"), "got: {result}");
    }

    #[test]
    fn from_ast_should_convert_mentions_to_discord_format() {
        let c = converter();
        let ast = c.to_ast("Hello @someone");
        let result = c.from_ast(&ast);
        assert!(result.contains("<@someone>"), "got: {result}");
    }

    // ---------- toAst (Discord markdown -> AST + extractPlainText), 8 cases ----------

    #[test]
    fn to_ast_should_convert_bold_root() {
        let ast = converter().to_ast("Hello **world**!");
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn to_ast_should_convert_user_mentions() {
        assert_eq!(
            converter().extract_plain_text("Hello <@123456789>"),
            "Hello @123456789"
        );
    }

    #[test]
    fn to_ast_should_convert_user_mentions_with_nickname_marker() {
        assert_eq!(
            converter().extract_plain_text("Hello <@!123456789>"),
            "Hello @123456789"
        );
    }

    #[test]
    fn to_ast_should_convert_channel_mentions() {
        assert_eq!(
            converter().extract_plain_text("Check <#987654321>"),
            "Check #987654321"
        );
    }

    #[test]
    fn to_ast_should_convert_role_mentions() {
        assert_eq!(
            converter().extract_plain_text("Hey <@&111222333>"),
            "Hey @&111222333"
        );
    }

    #[test]
    fn to_ast_should_convert_custom_emoji() {
        assert_eq!(
            converter().extract_plain_text("Nice <:thumbsup:123>"),
            "Nice :thumbsup:"
        );
    }

    #[test]
    fn to_ast_should_convert_animated_custom_emoji() {
        assert_eq!(
            converter().extract_plain_text("Cool <a:wave:456>"),
            "Cool :wave:"
        );
    }

    #[test]
    fn to_ast_should_handle_spoiler_tags() {
        let result = converter().extract_plain_text("Secret ||hidden text||");
        assert!(result.contains("[spoiler: hidden text]"), "got: {result}");
    }

    // ---------- extractPlainText, 10 upstream cases ----------

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:106 > "should remove bold markers"
    #[test]
    fn extract_plain_text_should_remove_bold_markers() {
        let r = converter().extract_plain_text("Hello **world**!");
        assert_eq!(r, "Hello world!");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:112 > "should remove italic markers"
    #[test]
    fn extract_plain_text_should_remove_italic_markers() {
        let r = converter().extract_plain_text("Hello *world*!");
        assert_eq!(r, "Hello world!");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:116 > "should remove strikethrough markers"
    #[test]
    fn extract_plain_text_should_remove_strikethrough_markers() {
        let r = converter().extract_plain_text("Hello ~~world~~!");
        assert_eq!(r, "Hello world!");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:122 > "should extract link text"
    #[test]
    fn extract_plain_text_should_extract_link_text() {
        let r = converter().extract_plain_text("Check [this](https://example.com)");
        assert_eq!(r, "Check this");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:128 > "should format user mentions"
    #[test]
    fn extract_plain_text_should_format_user_mentions() {
        let r = converter().extract_plain_text("Hey <@U123>!");
        assert!(r.contains("@U123"), "got: {r}");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:133 > "should handle complex messages"
    #[test]
    fn extract_plain_text_should_handle_complex_messages() {
        let r = converter()
            .extract_plain_text("**Bold** and *italic* with [link](https://x.com) and <@U123>");
        assert!(r.contains("Bold"), "got: {r}");
        assert!(r.contains("italic"), "got: {r}");
        assert!(r.contains("link"), "got: {r}");
        assert!(r.contains("@U123"), "got: {r}");
        assert!(!r.contains("**"), "got: {r}");
        assert!(!r.contains("<@"), "got: {r}");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:146 > "should handle inline code"
    #[test]
    fn extract_plain_text_should_handle_inline_code() {
        let r = converter().extract_plain_text("Use `const x = 1`");
        assert!(r.contains("const x = 1"), "got: {r}");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:151 > "should handle code blocks"
    #[test]
    fn extract_plain_text_should_handle_code_blocks() {
        let r = converter().extract_plain_text("```js\nconst x = 1;\n```");
        assert!(r.contains("const x = 1;"), "got: {r}");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:156 > "should handle empty string"
    #[test]
    fn extract_plain_text_should_handle_empty_string() {
        assert_eq!(converter().extract_plain_text(""), "");
    }

    // 1:1 with upstream packages/adapter-discord/src/markdown.test.ts:160 > "should handle plain text"
    #[test]
    fn extract_plain_text_should_handle_plain_text() {
        assert_eq!(converter().extract_plain_text("Hello world"), "Hello world");
    }

    // ---------- renderPostable, 5 upstream cases ----------

    #[test]
    fn render_postable_plain_string_with_mention_conversion() {
        let msg: AdapterPostableMessage = "Hello @world".into();
        let r = converter().render_postable(&msg);
        assert_eq!(r, "Hello <@world>");
    }

    #[test]
    fn render_postable_raw_message_with_mention_conversion() {
        let msg = AdapterPostableMessage::Raw(PostableRaw {
            raw: "Hi @bob".into(),
            attachments: None,
            files: None,
        });
        let r = converter().render_postable(&msg);
        assert_eq!(r, "Hi <@bob>");
    }

    #[test]
    fn render_postable_markdown_message() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "Hey **bold** @alice".into(),
            attachments: None,
            files: None,
        });
        let r = converter().render_postable(&msg);
        assert!(r.contains("**bold**"), "got: {r}");
        assert!(r.contains("<@alice>"), "got: {r}");
    }

    #[test]
    fn render_postable_empty_message() {
        let msg: AdapterPostableMessage = "".into();
        assert_eq!(converter().render_postable(&msg), "");
    }

    #[test]
    fn render_postable_ast_message() {
        let ast = converter().to_ast("Hello **world**");
        let root = match ast {
            Node::Root(r) => r,
            _ => panic!("expected root"),
        };
        let msg = AdapterPostableMessage::Ast(PostableAst {
            ast: root,
            attachments: None,
            files: None,
        });
        let r = converter().render_postable(&msg);
        assert!(r.contains("Hello"), "got: {r}");
        assert!(r.contains("**world**"), "got: {r}");
    }

    // ---------- blockquotes, lists, thematic break, table (10 cases) ----------

    #[test]
    fn blockquotes_should_handle_blockquotes() {
        let c = converter();
        let ast = c.to_ast("> Quote text");
        let r = c.from_ast(&ast);
        assert!(r.contains("> Quote text"), "got: {r}");
    }

    #[test]
    fn lists_should_handle_unordered_lists() {
        let c = converter();
        let ast = c.to_ast("- one\n- two\n- three");
        let r = c.from_ast(&ast);
        assert!(r.contains("- one"), "got: {r}");
        assert!(r.contains("- two"), "got: {r}");
        assert!(r.contains("- three"), "got: {r}");
    }

    #[test]
    fn lists_should_handle_ordered_lists() {
        let c = converter();
        let ast = c.to_ast("1. one\n2. two\n3. three");
        let r = c.from_ast(&ast);
        assert!(r.contains("one"), "got: {r}");
        assert!(r.contains("two"), "got: {r}");
        assert!(r.contains("three"), "got: {r}");
    }

    #[test]
    fn nested_lists_should_indent_nested_unordered() {
        let c = converter();
        let ast = c.to_ast("- outer\n  - inner");
        let r = c.from_ast(&ast);
        assert!(r.contains("outer"), "got: {r}");
        assert!(r.contains("inner"), "got: {r}");
    }

    #[test]
    fn nested_lists_should_indent_nested_ordered() {
        let c = converter();
        let ast = c.to_ast("1. outer\n   1. inner");
        let r = c.from_ast(&ast);
        assert!(r.contains("outer"), "got: {r}");
        assert!(r.contains("inner"), "got: {r}");
    }

    #[test]
    fn nested_lists_should_handle_deeply_nested() {
        let c = converter();
        let ast = c.to_ast("- a\n  - b\n    - c");
        let r = c.from_ast(&ast);
        assert!(r.contains("a"), "got: {r}");
        assert!(r.contains("b"), "got: {r}");
        assert!(r.contains("c"), "got: {r}");
    }

    #[test]
    fn nested_lists_should_keep_siblings_at_same_indent() {
        let c = converter();
        let ast = c.to_ast("- a\n  - b\n  - c");
        let r = c.from_ast(&ast);
        // Both nested items should appear and end up at the same indent level.
        let lines: Vec<&str> = r.lines().collect();
        let indent_of = |substr: &str| -> Option<usize> {
            lines
                .iter()
                .find(|l| l.contains(substr))
                .map(|l| l.len() - l.trim_start().len())
        };
        let b_indent = indent_of("b").expect("b line");
        let c_indent = indent_of("c").expect("c line");
        assert_eq!(b_indent, c_indent, "got: {r}");
    }

    #[test]
    fn nested_lists_should_handle_mixed_ordered_unordered() {
        let c = converter();
        let ast = c.to_ast("- outer\n  1. inner-ord\n- next");
        let r = c.from_ast(&ast);
        assert!(r.contains("outer"), "got: {r}");
        assert!(r.contains("inner-ord"), "got: {r}");
        assert!(r.contains("next"), "got: {r}");
    }

    #[test]
    fn thematic_break_should_render() {
        let c = converter();
        let ast = c.to_ast("---");
        let r = c.from_ast(&ast);
        assert!(r.contains("---"), "got: {r}");
    }

    #[test]
    fn table_rendering_should_render_as_code_blocks() {
        let c = converter();
        let ast = c.to_ast("| A | B |\n|---|---|\n| 1 | 2 |");
        let r = c.from_ast(&ast);
        assert!(r.contains("```"), "got: {r}");
        assert!(r.contains("A"), "got: {r}");
        assert!(r.contains("B"), "got: {r}");
    }
}
