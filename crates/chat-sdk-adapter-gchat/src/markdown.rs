//! Google Chat-specific format conversion.
//!
//! 1:1 port (in progress) of `packages/adapter-gchat/src/markdown.ts`.
//! Google Chat supports a subset of text formatting:
//!
//! - Bold: `*text*`
//! - Italic: `_text_`
//! - Strikethrough: `~text~`
//! - Monospace: `` `text` ``
//! - Code blocks: ```` ```text``` ````
//! - Links are auto-detected; custom links use `<url|label>`.
//!
//! Very similar to Slack's mrkdwn format. The Rust port reuses the
//! lookbehind-aware scanner pattern from `adapter-slack/src/format.rs`
//! to convert WhatsApp-style single markers to standard markdown
//! before parsing.

use chat_sdk_chat::markdown::{
    Node, ParseMarkdownError, parse_markdown, table_to_ascii, to_plain_text,
};

/// 1:1 port of upstream `class GoogleChatFormatConverter extends
/// BaseFormatConverter`.
#[derive(Debug, Default, Clone, Copy)]
pub struct GoogleChatFormatConverter;

impl GoogleChatFormatConverter {
    /// 1:1 with upstream `new GoogleChatFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse Google Chat text into mdast. 1:1 with upstream
    /// `toAst(gchatText)`: pre-process `*x*` -> `**x**` and
    /// `~x~` -> `~~x~~`, then parseMarkdown.
    pub fn to_ast(&self, gchat_text: &str) -> Result<Node, ParseMarkdownError> {
        let standard = from_gchat_format(gchat_text);
        parse_markdown(&standard)
    }

    /// Stringify mdast back to Google Chat format. 1:1 with
    /// upstream `fromAst(ast)`: recursive node-to-gchat walker.
    pub fn from_ast(&self, node: &Node) -> String {
        node_to_gchat(node, 0)
    }

    /// Plain-string postable.
    pub fn render_postable_string(&self, message: &str) -> String {
        message.to_string()
    }

    /// `{raw}` postable.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// `{markdown}` postable: parse + GChat-stringify.
    pub fn render_postable_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        let ast = parse_markdown(markdown)?;
        Ok(self.from_ast(&ast))
    }

    /// `{ast}` postable.
    pub fn render_postable_ast(&self, ast: &Node) -> String {
        self.from_ast(ast)
    }

    /// `fromMarkdown` shorthand: parse + stringify in one call.
    /// 1:1 with upstream's inherited `fromMarkdown(markdown)`.
    pub fn from_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        self.render_postable_markdown(markdown)
    }

    /// Plain-text extraction.
    pub fn extract_plain_text(&self, gchat_text: &str) -> String {
        match self.to_ast(gchat_text) {
            Ok(node) => to_plain_text(&node),
            Err(_) => gchat_text.to_string(),
        }
    }
}

/// Pre-process GChat -> standard format: `*x*` -> `**x**`,
/// `~x~` -> `~~x~~` with lookbehind-aware scanners (same pattern
/// as `adapter-slack/src/format.rs` and `adapter-whatsapp/src/
/// markdown.rs`).
fn from_gchat_format(text: &str) -> String {
    let s = upgrade_single_to_double(text, '*', "**");
    upgrade_single_to_double(&s, '~', "~~")
}

fn upgrade_single_to_double(text: &str, marker: char, double: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == marker {
            let prev = if i == 0 { '\0' } else { chars[i - 1] };
            let next = if i + 1 < chars.len() {
                chars[i + 1]
            } else {
                '\0'
            };
            // Skip part of double marker, escaped, or preceded by _.
            if prev == marker || next == marker || prev == '\\' || prev == '_' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != marker && chars[j] != '\n' {
                j += 1;
            }
            if j < chars.len() && chars[j] == marker && j > i + 1 {
                let after = if j + 1 < chars.len() {
                    chars[j + 1]
                } else {
                    '\0'
                };
                if after != marker {
                    out.push_str(double);
                    out.extend(&chars[i + 1..j]);
                    out.push_str(double);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Recursive node -> Google Chat text serializer. 1:1 with
/// upstream `nodeToGChat(node)`.
fn node_to_gchat(node: &Node, list_depth: usize) -> String {
    match node {
        Node::Root(root) => root
            .children
            .iter()
            .map(|c| node_to_gchat(c, list_depth))
            .collect::<Vec<_>>()
            .join("\n\n"),
        Node::Paragraph(p) => p
            .children
            .iter()
            .map(|c| node_to_gchat(c, list_depth))
            .collect::<Vec<_>>()
            .concat(),
        Node::Text(t) => t.value.clone(),
        Node::Strong(s) => {
            let content = s
                .children
                .iter()
                .map(|c| node_to_gchat(c, list_depth))
                .collect::<Vec<_>>()
                .concat();
            format!("*{content}*")
        }
        Node::Emphasis(e) => {
            let content = e
                .children
                .iter()
                .map(|c| node_to_gchat(c, list_depth))
                .collect::<Vec<_>>()
                .concat();
            format!("_{content}_")
        }
        Node::Delete(d) => {
            let content = d
                .children
                .iter()
                .map(|c| node_to_gchat(c, list_depth))
                .collect::<Vec<_>>()
                .concat();
            format!("~{content}~")
        }
        Node::InlineCode(c) => format!("`{}`", c.value),
        Node::Code(c) => format!("```\n{}\n```", c.value),
        Node::Link(l) => {
            let label = l
                .children
                .iter()
                .map(|c| node_to_gchat(c, list_depth))
                .collect::<Vec<_>>()
                .concat();
            if label == l.url || label.is_empty() {
                l.url.clone()
            } else {
                format!("<{}|{}>", l.url, label)
            }
        }
        Node::Blockquote(b) => b
            .children
            .iter()
            .map(|c| format!("> {}", node_to_gchat(c, list_depth)))
            .collect::<Vec<_>>()
            .join("\n"),
        Node::List(list) => render_list(list, list_depth, "•"),
        Node::ListItem(li) => li
            .children
            .iter()
            .map(|c| node_to_gchat(c, list_depth))
            .collect::<Vec<_>>()
            .concat(),
        Node::ThematicBreak(_) => "---".to_string(),
        Node::Break(_) => "\n".to_string(),
        Node::Table(t) => format!("```\n{}\n```", table_to_ascii(t)),
        Node::Heading(h) => {
            // No native heading; convert to bold text + content.
            let content = h
                .children
                .iter()
                .map(|c| node_to_gchat(c, list_depth))
                .collect::<Vec<_>>()
                .concat();
            format!("*{content}*")
        }
        Node::Html(h) => h.value.clone(),
        _ => to_plain_text(node),
    }
}

/// Render a list node with per-level indentation. Delegates to
/// the chat-sdk-chat `render_list` helper (1:1 with upstream
/// `BaseFormatConverter::renderList`) so that nested-list
/// indentation matches upstream's "2 spaces per depth level"
/// + continuation-line semantics. `bullet` is the unordered-list
/// glyph (GChat uses "•"); ordered lists use "<n>." prefixes.
fn render_list(list: &chat_sdk_chat::markdown::List, depth: usize, bullet: &str) -> String {
    chat_sdk_chat::markdown::render_list(
        list,
        depth,
        &|node| node_to_gchat(node, depth + 1),
        bullet,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- fromAst tests (subset of upstream 17 cases) ----------

    #[test]
    fn should_convert_bold() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("**bold text**").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("*bold text*"));
    }

    #[test]
    fn should_convert_italic() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("_italic text_").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("_italic text_"));
    }

    #[test]
    fn should_convert_strikethrough() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("~~strikethrough~~").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("~strikethrough~"));
    }

    #[test]
    fn should_preserve_inline_code() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("Use `const x = 1`").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("`const x = 1`"));
    }

    #[test]
    fn should_handle_code_blocks() {
        let c = GoogleChatFormatConverter::new();
        let input = "```\nconst x = 1;\n```";
        let ast = c.to_ast(input).unwrap();
        let output = c.from_ast(&ast);
        assert!(output.contains("```"));
        assert!(output.contains("const x = 1;"));
    }

    #[test]
    fn should_output_url_directly_when_link_text_matches_url() {
        let c = GoogleChatFormatConverter::new();
        let ast = c
            .to_ast("[https://example.com](https://example.com)")
            .unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn should_output_gchat_custom_link_when_text_differs() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("[click here](https://example.com)").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("<https://example.com|click here>"));
    }

    #[test]
    fn should_handle_blockquotes() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("> quoted text").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("> quoted text"));
    }

    #[test]
    fn should_handle_unordered_lists() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("- item 1\n- item 2").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("item 1"));
        assert!(result.contains("item 2"));
    }

    #[test]
    fn should_handle_ordered_lists() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("1. first\n2. second").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("1."));
        assert!(result.contains("2."));
    }

    #[test]
    fn should_handle_line_breaks() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("line one\nline two").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("line one"));
        assert!(result.contains("line two"));
    }

    #[test]
    fn should_handle_thematic_breaks() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("---").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("---"));
    }

    // ---------- nested list tests (5 deferred cases, slice 199) ----------

    #[test]
    fn should_indent_nested_unordered_lists() {
        let c = GoogleChatFormatConverter::new();
        let result = c
            .from_markdown("- parent\n  - child 1\n  - child 2")
            .unwrap();
        assert!(result.contains("• parent"));
        assert!(result.contains("  • child 1"));
        assert!(result.contains("  • child 2"));
    }

    #[test]
    fn should_indent_nested_ordered_lists() {
        let c = GoogleChatFormatConverter::new();
        let result = c
            .from_markdown("1. first\n   1. sub-first\n   2. sub-second\n2. second")
            .unwrap();
        assert!(result.contains("1. first"));
        assert!(result.contains("  1. sub-first"));
        assert!(result.contains("  2. sub-second"));
        assert!(result.contains("2. second"));
    }

    #[test]
    fn should_handle_deeply_nested_lists() {
        let c = GoogleChatFormatConverter::new();
        let result = c
            .from_markdown("- level 1\n  - level 2\n    - level 3")
            .unwrap();
        assert!(result.contains("• level 1"));
        assert!(result.contains("  • level 2"));
        assert!(result.contains("    • level 3"));
    }

    #[test]
    fn should_keep_sibling_items_at_same_indent() {
        let c = GoogleChatFormatConverter::new();
        let result = c.from_markdown("- item 1\n- item 2\n- item 3").unwrap();
        // Each "• " bullet starts at column 0 (no indent).
        assert!(result.contains("• item 1"));
        assert!(result.contains("• item 2"));
        assert!(result.contains("• item 3"));
        // No indented bullets at depth 1.
        assert!(!result.contains("  • item"));
    }

    #[test]
    fn should_handle_mixed_ordered_and_unordered_nesting() {
        let c = GoogleChatFormatConverter::new();
        let result = c
            .from_markdown("1. first\n   - sub a\n   - sub b\n2. second")
            .unwrap();
        assert!(result.contains("1. first"));
        assert!(result.contains("  • sub a"));
        assert!(result.contains("  • sub b"));
        assert!(result.contains("2. second"));
    }

    // ---------- toAst tests (subset) ----------

    #[test]
    fn should_parse_gchat_bold_to_ast() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("*bold*").unwrap();
        // *bold* should be promoted to **bold** then parsed as Strong.
        match &ast {
            Node::Root(r) => match &r.children[0] {
                Node::Paragraph(p) => assert!(matches!(p.children[0], Node::Strong(_))),
                other => panic!("expected paragraph, got {other:?}"),
            },
            _ => panic!("expected Root"),
        }
    }

    #[test]
    fn should_parse_gchat_strikethrough_to_ast() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("~strike~").unwrap();
        match &ast {
            Node::Root(r) => match &r.children[0] {
                Node::Paragraph(p) => assert!(matches!(p.children[0], Node::Delete(_))),
                other => panic!("expected paragraph with delete, got {other:?}"),
            },
            _ => panic!("expected Root"),
        }
    }

    #[test]
    fn should_parse_code_blocks() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("```\nfoo\n```").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    // ---------- extractPlainText ----------

    #[test]
    fn should_remove_formatting_markers() {
        let c = GoogleChatFormatConverter::new();
        let result = c.extract_plain_text("*bold* and _italic_");
        assert!(result.contains("bold"));
        assert!(result.contains("italic"));
        assert!(!result.contains('*'));
    }

    #[test]
    fn should_handle_empty_string() {
        let c = GoogleChatFormatConverter::new();
        assert_eq!(c.extract_plain_text(""), "");
    }

    #[test]
    fn should_handle_plain_text() {
        let c = GoogleChatFormatConverter::new();
        let result = c.extract_plain_text("just text");
        assert!(result.contains("just text"));
    }

    #[test]
    fn should_handle_inline_code_in_extract() {
        let c = GoogleChatFormatConverter::new();
        let result = c.extract_plain_text("Run `cargo test` to verify");
        assert!(result.contains("cargo test"));
    }

    // ---------- renderPostable ----------

    #[test]
    fn should_render_a_plain_string() {
        let c = GoogleChatFormatConverter::new();
        assert_eq!(c.render_postable_string("hello"), "hello");
    }

    #[test]
    fn should_render_a_raw_message() {
        let c = GoogleChatFormatConverter::new();
        assert_eq!(c.render_postable_raw("raw"), "raw");
    }

    #[test]
    fn should_render_a_markdown_message() {
        let c = GoogleChatFormatConverter::new();
        let result = c.render_postable_markdown("**bold**").unwrap();
        assert!(result.contains("*bold*"));
    }

    #[test]
    fn should_render_an_ast_message() {
        let c = GoogleChatFormatConverter::new();
        let ast = c.to_ast("hello").unwrap();
        let result = c.render_postable_ast(&ast);
        assert!(result.contains("hello"));
    }

    #[test]
    fn should_render_markdown_tables_as_code_blocks() {
        let c = GoogleChatFormatConverter::new();
        let result = c
            .render_postable_markdown("| Col1 | Col2 |\n|------|------|\n| a    | b    |")
            .unwrap();
        assert!(result.contains("```"));
        assert!(result.contains("Col1"));
        assert!(result.contains("Col2"));
    }
}
