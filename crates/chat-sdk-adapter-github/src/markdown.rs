//! GitHub-specific format conversion.
//!
//! 1:1 port of `packages/adapter-github/src/markdown.ts`. GitHub
//! uses standard GitHub Flavored Markdown (GFM) so the converter
//! is mostly pass-through over chat-sdk-chat's
//! `parse_markdown` / `stringify_markdown` helpers. Per upstream
//! comments, the converter is GFM-passthrough; @mentions and
//! issue refs are already in their final form before/after a
//! round-trip.

use chat_sdk_chat::markdown::{
    Node, ParseMarkdownError, parse_markdown, stringify_markdown, to_plain_text,
};

/// 1:1 port of upstream `class GitHubFormatConverter extends
/// BaseFormatConverter`.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubFormatConverter;

impl GitHubFormatConverter {
    /// Construct a converter. 1:1 with upstream
    /// `new GitHubFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse a Markdown string into an mdast [`Node`]. 1:1 with
    /// upstream `toAst(markdown)`.
    pub fn to_ast(&self, markdown: &str) -> Result<Node, ParseMarkdownError> {
        parse_markdown(markdown)
    }

    /// Stringify an mdast [`Node`] back to Markdown. 1:1 with
    /// upstream `fromAst(ast)` - GitHub uses standard GFM so
    /// delegates straight to chat-sdk-chat's stringifier.
    pub fn from_ast(&self, node: &Node) -> String {
        stringify_markdown(node)
    }

    /// Extract plain text from a Markdown string. 1:1 with the
    /// upstream `BaseFormatConverter::extractPlainText` inherited
    /// behaviour (parse + `toString`).
    pub fn extract_plain_text(&self, markdown: &str) -> String {
        match parse_markdown(markdown) {
            Ok(node) => to_plain_text(&node),
            Err(_) => markdown.to_string(),
        }
    }

    /// Render a plain-string postable input. 1:1 with the
    /// `typeof message === "string"` branch of upstream
    /// `renderPostable`.
    pub fn render_postable_string(&self, message: &str) -> String {
        message.to_string()
    }

    /// Render a `{raw}` postable input verbatim.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// Render a `{markdown}` postable input. Upstream calls
    /// `fromMarkdown(markdown)` which (in
    /// `BaseFormatConverter`) parses and re-stringifies for
    /// normalisation.
    pub fn render_postable_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        let ast = parse_markdown(markdown)?;
        Ok(stringify_markdown(&ast))
    }

    /// Render an `{ast}` postable input.
    pub fn render_postable_ast(&self, ast: &Node) -> String {
        stringify_markdown(ast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::markdown::{Paragraph, Root, Strong, Text, paragraph, root, strong, text};

    // ---------- toAst (6 ported upstream cases) ----------

    #[test]
    fn should_parse_plain_text() {
        let converter = GitHubFormatConverter::new();
        let ast = converter.to_ast("Hello world").unwrap();
        match &ast {
            Node::Root(r) => assert_eq!(r.children.len(), 1),
            _ => panic!("expected Root, got {ast:?}"),
        }
    }

    #[test]
    fn should_parse_bold_text() {
        let converter = GitHubFormatConverter::new();
        let ast = converter.to_ast("**bold text**").unwrap();
        match &ast {
            Node::Root(r) => match &r.children[0] {
                Node::Paragraph(_) => {}
                other => panic!("expected paragraph child, got {other:?}"),
            },
            _ => panic!("expected Root"),
        }
    }

    #[test]
    fn should_parse_at_mentions() {
        let converter = GitHubFormatConverter::new();
        // Upstream `extractPlainText("Hey @username, check this out")`
        // returns text containing @username.
        let text = converter.extract_plain_text("Hey @username, check this out");
        assert!(text.contains("@username"));
    }

    #[test]
    fn should_parse_code_blocks() {
        let converter = GitHubFormatConverter::new();
        let ast = converter
            .to_ast("```javascript\nconsole.log('hello');\n```")
            .unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_links() {
        let converter = GitHubFormatConverter::new();
        let ast = converter
            .to_ast("[link text](https://example.com)")
            .unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_strikethrough() {
        let converter = GitHubFormatConverter::new();
        let ast = converter.to_ast("~~deleted~~").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    // ---------- fromAst (3 ported upstream cases) ----------

    #[test]
    fn should_render_plain_text() {
        let converter = GitHubFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        assert_eq!(converter.from_ast(&ast), "Hello world");
    }

    #[test]
    fn should_render_bold_text() {
        let converter = GitHubFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Strong(
            strong(vec![Node::Text(text("bold"))]),
        )]))]));
        assert_eq!(converter.from_ast(&ast), "**bold**");
    }

    #[test]
    fn should_render_italic_text() {
        use chat_sdk_chat::markdown::emphasis;
        let converter = GitHubFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![
            Node::Emphasis(emphasis(vec![Node::Text(text("italic"))])),
        ]))]));
        assert_eq!(converter.from_ast(&ast), "*italic*");
    }

    // ---------- extractPlainText (3 ported upstream cases) ----------

    #[test]
    fn should_extract_text_from_markdown() {
        let converter = GitHubFormatConverter::new();
        // Upstream expects exactly "bold and italic". The Rust
        // to_plain_text mirrors mdast-util-to-string's concat
        // behavior; round-trip yields the same.
        let result = converter.extract_plain_text("**bold** and _italic_");
        assert_eq!(result, "bold and italic");
    }

    #[test]
    fn should_preserve_at_mentions() {
        let converter = GitHubFormatConverter::new();
        let result = converter.extract_plain_text("Hey @user, **thanks**!");
        assert!(result.contains("@user"));
        assert!(result.contains("thanks"));
    }

    #[test]
    fn should_extract_text_from_code_blocks() {
        let converter = GitHubFormatConverter::new();
        let result = converter.extract_plain_text("```\ncode\n```");
        assert!(result.contains("code"));
    }

    // ---------- renderPostable (4 ported upstream cases) ----------

    #[test]
    fn should_render_string_directly() {
        let converter = GitHubFormatConverter::new();
        assert_eq!(
            converter.render_postable_string("Hello world"),
            "Hello world"
        );
    }

    #[test]
    fn should_render_raw_message() {
        let converter = GitHubFormatConverter::new();
        assert_eq!(converter.render_postable_raw("Raw content"), "Raw content");
    }

    #[test]
    fn should_render_markdown_message() {
        let converter = GitHubFormatConverter::new();
        let result = converter.render_postable_markdown("**bold**").unwrap();
        assert_eq!(result, "**bold**");
    }

    #[test]
    fn should_render_ast_message() {
        let converter = GitHubFormatConverter::new();
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("AST content"),
        )]))]));
        assert_eq!(converter.render_postable_ast(&ast), "AST content");
    }

    // ---------- roundtrip (2 ported upstream cases) ----------

    #[test]
    fn should_roundtrip_simple_text() {
        let converter = GitHubFormatConverter::new();
        let original = "Hello world";
        let ast = converter.to_ast(original).unwrap();
        let result = converter.from_ast(&ast);
        assert_eq!(result.trim(), original);
    }

    #[test]
    fn should_roundtrip_markdown_with_formatting() {
        let converter = GitHubFormatConverter::new();
        let original = "**bold** and *italic*";
        let ast = converter.to_ast(original).unwrap();
        let result = converter.from_ast(&ast);
        assert!(result.contains("bold"));
        assert!(result.contains("italic"));
    }

    // Silence unused-import warnings on Paragraph / Root / Strong /
    // Text - they're only used via the constructor helpers above.
    #[allow(dead_code)]
    fn _unused_imports(_p: Paragraph, _r: Root, _s: Strong, _t: Text) {}
}
