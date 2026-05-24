//! Linear-specific format conversion.
//!
//! 1:1 port (in progress) of `packages/adapter-linear/src/markdown.ts`.
//! Linear comments use standard GitHub-flavored Markdown so the
//! upstream converter is mostly pass-through over the chat-sdk
//! parseMarkdown / stringifyMarkdown helpers.
//!
//! This slice covers the `toAst` path + the plain-string +
//! `{raw}` `render_postable` branches. The `fromAst` and
//! `{markdown}`/`{ast}` `render_postable` branches depend on
//! `chat_sdk_chat::markdown::stringify_markdown`, which has not
//! landed yet; they're deferred to a follow-up slice on
//! chat-sdk-chat.

use chat_sdk_chat::markdown::{Node, ParseMarkdownError, parse_markdown, stringify_markdown};

/// 1:1 port of upstream `class LinearFormatConverter extends
/// BaseFormatConverter`. The converter is stateless; the methods
/// could be free functions, but the `struct` mirrors the upstream
/// shape and leaves room for state (e.g. emoji resolver) when
/// chat-sdk-chat's `BaseFormatConverter` lands.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinearFormatConverter;

impl LinearFormatConverter {
    /// Construct a converter. 1:1 with upstream
    /// `new LinearFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse a Markdown string into an mdast [`Node`]. 1:1 with
    /// upstream `toAst(markdown)`. Linear uses standard GFM so this
    /// delegates to [`chat_sdk_chat::markdown::parse_markdown`].
    pub fn to_ast(&self, markdown: &str) -> Result<Node, ParseMarkdownError> {
        parse_markdown(markdown)
    }

    /// Stringify an mdast [`Node`] back to standard Markdown. 1:1
    /// with upstream `fromAst(ast)`. Linear uses standard GFM so
    /// this delegates to
    /// [`chat_sdk_chat::markdown::stringify_markdown`].
    pub fn from_ast(&self, node: &Node) -> String {
        stringify_markdown(node)
    }

    /// Render a "postable" input that's already a plain string.
    /// 1:1 with the `typeof message === "string"` branch of
    /// upstream `renderPostable(message)`.
    pub fn render_postable_string(&self, message: &str) -> String {
        message.to_string()
    }

    /// Render a `{raw}` postable input verbatim. 1:1 with the
    /// `"raw" in message` branch of upstream `renderPostable`.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// Render a `{markdown}` postable input. 1:1 with the
    /// `"markdown" in message` branch of upstream
    /// `renderPostable`. The upstream `fromMarkdown(markdown)`
    /// helper parses + stringifies via remark; the Rust port
    /// does the same to normalise the input.
    pub fn render_postable_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        let ast = parse_markdown(markdown)?;
        Ok(stringify_markdown(&ast))
    }

    /// Render an `{ast}` postable input. 1:1 with the
    /// `"ast" in message` branch of upstream `renderPostable`.
    pub fn render_postable_ast(&self, ast: &Node) -> String {
        stringify_markdown(ast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- toAst (6 ported upstream cases) ----------

    #[test]
    fn should_parse_plain_text() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("Hello world").unwrap();
        assert!(matches!(ast, Node::Root(_)));
        if let Node::Root(root) = ast {
            assert!(!root.children.is_empty());
        }
    }

    #[test]
    fn should_parse_markdown_with_bold() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("**bold text**").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_markdown_with_italic() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("_italic text_").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_markdown_with_links() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("[Link](https://example.com)").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_markdown_with_code_blocks() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("```\ncode\n```").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn should_parse_markdown_with_lists() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("- item 1\n- item 2\n- item 3").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    // ---------- renderPostable (2 of 4 ported upstream cases) ----------

    #[test]
    fn should_render_a_plain_string() {
        let converter = LinearFormatConverter::new();
        let result = converter.render_postable_string("Hello world");
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn should_render_a_raw_message() {
        let converter = LinearFormatConverter::new();
        let result = converter.render_postable_raw("raw content");
        assert_eq!(result, "raw content");
    }

    // ---------- fromAst (3 ported upstream cases) ----------

    #[test]
    fn should_stringify_a_simple_ast() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("Hello world").unwrap();
        let result = converter.from_ast(&ast);
        assert!(result.contains("Hello world"));
    }

    #[test]
    fn should_round_trip_bold_text() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("**bold text**").unwrap();
        let result = converter.from_ast(&ast);
        assert!(result.contains("**bold text**"));
    }

    #[test]
    fn should_round_trip_links() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("[Link](https://example.com)").unwrap();
        let result = converter.from_ast(&ast);
        assert!(result.contains("[Link](https://example.com)"));
    }

    // ---------- renderPostable (2 ported upstream cases for {markdown}/{ast}) ----------

    #[test]
    fn should_render_a_markdown_message() {
        let converter = LinearFormatConverter::new();
        let result = converter.render_postable_markdown("**bold** text").unwrap();
        assert!(result.contains("bold"));
    }

    #[test]
    fn should_render_an_ast_message() {
        let converter = LinearFormatConverter::new();
        let ast = converter.to_ast("Hello from AST").unwrap();
        let result = converter.render_postable_ast(&ast);
        assert!(result.contains("Hello from AST"));
    }

    // ---------- additive Rust-side ----------

    #[test]
    fn new_constructs_a_converter() {
        let _ = LinearFormatConverter::new();
        let _ = LinearFormatConverter::default();
    }
}
