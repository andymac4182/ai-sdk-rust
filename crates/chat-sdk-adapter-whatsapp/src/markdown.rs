//! WhatsApp markdown format conversion.
//!
//! 1:1 port of `packages/adapter-whatsapp/src/markdown.ts`. WhatsApp
//! uses a markdown-like format with single-char delimiters:
//!
//! - Bold: `*text*` (single asterisk, not double)
//! - Italic: `_text_`
//! - Strikethrough: `~text~` (single tilde, not double)
//! - Monospace: `` ```text``` ``
//!
//! @see <https://faq.whatsapp.com/539178204879377>
//!
//! Conversion strategy mirrors upstream:
//! - `to_ast(text)`: pre-process WhatsApp single-marker syntax to
//!   standard double-marker via lookbehind-aware scanners, then
//!   `parse_markdown`.
//! - `from_ast(ast)`: walk the AST to coerce unsupported variants
//!   (heading -> bold paragraph, thematic break -> `━━━` text,
//!   table -> code block), then `stringify_markdown_with` using
//!   the `_` emphasis + `-` bullet options that WhatsApp uses, and
//!   post-process the stringifier output to single-marker form.

use chat_sdk_chat::markdown::{
    Code, Node, ParseMarkdownError, StringifyMarkdownOptions, parse_markdown,
    stringify_markdown_with, table_to_ascii, to_plain_text, walk_ast,
};

/// 1:1 port of upstream `class WhatsAppFormatConverter extends
/// BaseFormatConverter`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WhatsAppFormatConverter;

impl WhatsAppFormatConverter {
    /// 1:1 with upstream `new WhatsAppFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse WhatsApp text into mdast. 1:1 with upstream
    /// `toAst(text)`: pre-process single-asterisk bold ->
    /// double-asterisk + single-tilde strike -> double-tilde,
    /// then `parseMarkdown`.
    pub fn to_ast(&self, whatsapp_text: &str) -> Result<Node, ParseMarkdownError> {
        let standard = from_whatsapp_format(whatsapp_text);
        parse_markdown(&standard)
    }

    /// Stringify mdast back to WhatsApp text. 1:1 with upstream
    /// `fromAst(ast)`:
    ///
    /// 1. Walk the AST and replace headings with bold-paragraph
    ///    equivalents (flattening any nested `Strong` to avoid
    ///    `***triple***`); thematic breaks with a `━━━` text
    ///    line; tables with code blocks containing the ASCII
    ///    table.
    /// 2. Stringify with `emphasis: '_'`, `bullet: '-'`.
    /// 3. Post-process: `**bold**` -> `*bold*`, `~~strike~~` ->
    ///    `~strike~`.
    pub fn from_ast(&self, ast: &Node) -> String {
        let transformed = walk_ast(ast.clone(), &mut whatsapp_node_visitor);
        let stringified = stringify_markdown_with(
            &transformed,
            &StringifyMarkdownOptions {
                emphasis: '_',
                bullet: '-',
            },
        );
        to_whatsapp_format(&stringified)
    }

    /// Plain-string postable.
    pub fn render_postable_string(&self, message: &str) -> String {
        message.to_string()
    }

    /// `{raw}` postable.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// `{markdown}` postable: parse standard markdown + stringify
    /// to WhatsApp format.
    pub fn render_postable_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        let ast = parse_markdown(markdown)?;
        Ok(self.from_ast(&ast))
    }

    /// `{ast}` postable.
    pub fn render_postable_ast(&self, ast: &Node) -> String {
        self.from_ast(ast)
    }

    /// Extract plain text from a WhatsApp message.
    pub fn extract_plain_text(&self, whatsapp_text: &str) -> String {
        match self.to_ast(whatsapp_text) {
            Ok(node) => to_plain_text(&node),
            Err(_) => whatsapp_text.to_string(),
        }
    }
}

/// Walk-visitor that coerces unsupported variants. 1:1 with the
/// callback passed to upstream's `walkAst` in `fromAst`.
fn whatsapp_node_visitor(node: Node) -> Option<Node> {
    // Heading -> paragraph with a strong wrapper; flatten any
    // pre-existing strong child to avoid *** triple asterisks.
    if let Node::Heading(h) = &node {
        let mut flattened: Vec<Node> = Vec::with_capacity(h.children.len());
        for child in &h.children {
            if let Node::Strong(s) = child {
                flattened.extend(s.children.iter().cloned());
            } else {
                flattened.push(child.clone());
            }
        }
        let strong = chat_sdk_chat::markdown::strong(flattened);
        let para = chat_sdk_chat::markdown::paragraph(vec![Node::Strong(strong)]);
        return Some(Node::Paragraph(para));
    }
    // Thematic break -> paragraph with text "━━━".
    if let Node::ThematicBreak(_) = &node {
        let text = chat_sdk_chat::markdown::text("━━━");
        let para = chat_sdk_chat::markdown::paragraph(vec![Node::Text(text)]);
        return Some(Node::Paragraph(para));
    }
    // Table -> fenced code block containing ASCII table.
    if let Node::Table(t) = &node {
        let value = table_to_ascii(t);
        return Some(Node::Code(Code {
            value,
            lang: None,
            meta: None,
            position: None,
        }));
    }
    Some(node)
}

/// Post-process stringifier output: `**bold**` -> `*bold*`,
/// `~~strike~~` -> `~strike~`. 1:1 with upstream
/// `toWhatsAppFormat(text)`.
fn to_whatsapp_format(text: &str) -> String {
    let bold = replace_pair(text, "**");
    replace_pair(&bold, "~~")
}

/// Pre-process WhatsApp -> standard format: single `*x*` ->
/// `**x**`, single `~x~` -> `~~x~~`. 1:1 with upstream
/// `fromWhatsAppFormat(text)` which uses regex with lookbehind /
/// lookahead to skip double markers and across-newline matches.
fn from_whatsapp_format(text: &str) -> String {
    let bold = upgrade_single_to_double(text, '*', "**");
    upgrade_single_to_double(&bold, '~', "~~")
}

/// Find every `<marker><body><marker>` match where the marker is
/// `**` or `~~`, and replace with `<single><body><single>`.
fn replace_pair(text: &str, marker: &str) -> String {
    let single = marker.chars().next().unwrap_or('*').to_string();
    let chars: Vec<char> = text.chars().collect();
    let m: Vec<char> = marker.chars().collect();
    let mlen = m.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if i + mlen <= chars.len() && chars[i..i + mlen] == m[..] {
            let body_start = i + mlen;
            let mut j = body_start;
            while j + mlen <= chars.len() {
                if chars[j..j + mlen] == m[..] {
                    break;
                }
                j += 1;
            }
            if j + mlen <= chars.len() && chars[j..j + mlen] == m[..] && j > body_start {
                out.push_str(&single);
                out.extend(&chars[body_start..j]);
                out.push_str(&single);
                i = j + mlen;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Upgrade single-marker `*x*` -> `**x**` (or `~x~` -> `~~x~~`).
/// Mirrors upstream's lookbehind: ignore single markers that are
/// part of a doubled marker; ignore across-newline matches; body
/// must be non-empty and contain no newlines or the marker char.
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
            // Skip if part of a double marker.
            if prev == marker || next == marker {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != marker && chars[j] != '\n' {
                j += 1;
            }
            if j < chars.len() && chars[j] == marker && j > i + 1 {
                // Check trailing isn't double.
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

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::markdown::{paragraph, root, strong, text};

    // ---------- toAst (6 ported upstream cases) ----------

    #[test]
    fn should_parse_plain_text() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("Hello world").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_whatsapp_bold_as_standard_bold() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("*bold*").unwrap();
        // After fromWhatsAppFormat: "**bold**" -> parsed as
        // paragraph containing strong.
        match &ast {
            Node::Root(r) => match &r.children[0] {
                Node::Paragraph(p) => assert!(matches!(p.children[0], Node::Strong(_))),
                other => panic!("expected paragraph, got {other:?}"),
            },
            _ => panic!("expected Root"),
        }
    }

    #[test]
    fn should_parse_italic() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("_italic_").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_whatsapp_strikethrough_as_standard() {
        let c = WhatsAppFormatConverter::new();
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
    fn should_not_merge_bold_spans_across_newlines() {
        let c = WhatsAppFormatConverter::new();
        // "*line1\nline2*" should NOT merge into a bold span - the
        // single * shouldn't promote when the close-* is across a
        // newline.
        let ast = c.to_ast("*line1\nline2*").unwrap();
        // The standard markdown parser may or may not detect this as
        // anything; what matters is we didn't upgrade across the
        // newline. Re-stringifying should still contain "line1" and
        // "line2".
        let plain = to_plain_text(&ast);
        assert!(plain.contains("line1"));
        assert!(plain.contains("line2"));
    }

    #[test]
    fn should_parse_code_blocks() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("```\ncode here\n```").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_lists() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("- one\n- two").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    // ---------- fromAst conversion semantics ----------

    #[test]
    fn should_stringify_a_simple_ast() {
        let c = WhatsAppFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        assert_eq!(c.from_ast(&ast), "Hello world");
    }

    #[test]
    fn should_convert_standard_bold_to_whatsapp_bold() {
        let c = WhatsAppFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Strong(
            strong(vec![Node::Text(text("bold"))]),
        )]))]));
        assert_eq!(c.from_ast(&ast), "*bold*");
    }

    #[test]
    fn should_convert_standard_strikethrough_to_whatsapp_style() {
        use chat_sdk_chat::markdown::strikethrough;
        let c = WhatsAppFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Delete(
            strikethrough(vec![Node::Text(text("strike"))]),
        )]))]));
        assert_eq!(c.from_ast(&ast), "~strike~");
    }

    #[test]
    fn should_convert_standard_italic_to_whatsapp_underscore_italic() {
        use chat_sdk_chat::markdown::emphasis;
        let c = WhatsAppFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Emphasis(
            emphasis(vec![Node::Text(text("italic"))]),
        )]))]));
        assert_eq!(c.from_ast(&ast), "_italic_");
    }

    #[test]
    fn should_handle_bold_and_italic_together() {
        use chat_sdk_chat::markdown::emphasis;
        let c = WhatsAppFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![
            Node::Strong(strong(vec![Node::Text(text("bold"))])),
            Node::Text(text(" and ")),
            Node::Emphasis(emphasis(vec![Node::Text(text("italic"))])),
        ]))]));
        let result = c.from_ast(&ast);
        assert!(result.contains("*bold*"));
        assert!(result.contains("_italic_"));
    }

    #[test]
    fn should_convert_headings_to_bold_text() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("# Heading").unwrap();
        let result = c.from_ast(&ast);
        // Heading became bold paragraph -> *Heading*
        assert!(result.contains("*Heading*"));
    }

    #[test]
    fn should_flatten_bold_inside_headings_to_avoid_triple_asterisks() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("# **Already bold**").unwrap();
        let result = c.from_ast(&ast);
        assert!(!result.contains("***"));
        assert!(result.contains("Already bold"));
    }

    #[test]
    fn should_convert_thematic_breaks_to_text_separator() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("---").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("━━━"));
    }

    // ---------- renderPostable ----------

    #[test]
    fn should_render_plain_string() {
        let c = WhatsAppFormatConverter::new();
        assert_eq!(c.render_postable_string("hello"), "hello");
    }

    #[test]
    fn should_render_raw_message() {
        let c = WhatsAppFormatConverter::new();
        assert_eq!(c.render_postable_raw("raw text"), "raw text");
    }

    #[test]
    fn should_render_markdown_message() {
        let c = WhatsAppFormatConverter::new();
        let result = c.render_postable_markdown("**bold**").unwrap();
        // Re-rendered to WhatsApp format.
        assert_eq!(result, "*bold*");
    }

    #[test]
    fn should_render_ast_message() {
        let c = WhatsAppFormatConverter::new();
        let ast = c.to_ast("hello").unwrap();
        let result = c.render_postable_ast(&ast);
        assert!(result.contains("hello"));
    }

    // ---------- helper-function additive tests ----------

    #[test]
    fn from_whatsapp_format_upgrades_single_bold() {
        assert_eq!(from_whatsapp_format("*bold*"), "**bold**");
    }

    #[test]
    fn from_whatsapp_format_preserves_double_markers() {
        assert_eq!(from_whatsapp_format("**already**"), "**already**");
    }

    #[test]
    fn from_whatsapp_format_does_not_cross_newlines() {
        assert_eq!(
            from_whatsapp_format("*line1\nline2*"),
            "*line1\nline2*"
        );
    }

    #[test]
    fn to_whatsapp_format_collapses_double_bold() {
        assert_eq!(to_whatsapp_format("**bold**"), "*bold*");
    }

    #[test]
    fn to_whatsapp_format_collapses_double_strike() {
        assert_eq!(to_whatsapp_format("~~strike~~"), "~strike~");
    }
}
