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

use chat_sdk_chat::markdown::{Node, ParseMarkdownError, parse_markdown};

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

    // {markdown} and {ast} branches of renderPostable depend on
    // chat-sdk-chat's stringify_markdown which hasn't landed yet.
    // The fromAst describe-block tests (round-trip simple AST,
    // bold, links) similarly require stringify_markdown. Both are
    // deferred to a follow-up slice on chat-sdk-chat.

    // ---------- additive Rust-side ----------

    #[test]
    fn new_constructs_a_converter() {
        let _ = LinearFormatConverter::new();
        let _ = LinearFormatConverter::default();
    }
}
