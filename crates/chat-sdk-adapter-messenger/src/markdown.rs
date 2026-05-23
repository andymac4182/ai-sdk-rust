//! Messenger format conversion.
//!
//! 1:1 port of `packages/adapter-messenger/src/markdown.ts`.
//! Messenger has no inline markdown — `MessengerFormatConverter`
//! just delegates to chat-sdk-chat's parse / stringify helpers
//! and emits raw text on the wire.

use chat_sdk_chat::markdown::{
    Node, ParseMarkdownError, parse_markdown, stringify_markdown, to_plain_text,
};

/// 1:1 port of upstream `class MessengerFormatConverter extends
/// BaseFormatConverter`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MessengerFormatConverter;

impl MessengerFormatConverter {
    /// 1:1 with upstream `new MessengerFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse text into mdast. 1:1 with upstream `toAst(text)`.
    pub fn to_ast(&self, text: &str) -> Result<Node, ParseMarkdownError> {
        parse_markdown(text)
    }

    /// Stringify mdast back to text. 1:1 with upstream
    /// `fromAst(ast)` which calls `stringifyMarkdown(ast).trim()`.
    pub fn from_ast(&self, node: &Node) -> String {
        stringify_markdown(node)
    }

    /// Plain-string passthrough.
    pub fn render_postable_string(&self, message: &str) -> String {
        message.to_string()
    }

    /// `{raw}` postable passthrough.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        raw.to_string()
    }

    /// `{markdown}` postable: parse + stringify normalisation.
    pub fn render_postable_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        let ast = parse_markdown(markdown)?;
        Ok(stringify_markdown(&ast))
    }

    /// `{ast}` postable.
    pub fn render_postable_ast(&self, ast: &Node) -> String {
        stringify_markdown(ast)
    }

    /// Extract plain text from a Markdown string. 1:1 with
    /// upstream `extractPlainText(markdown)`.
    pub fn extract_plain_text(&self, markdown: &str) -> String {
        match parse_markdown(markdown) {
            Ok(node) => to_plain_text(&node),
            Err(_) => markdown.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- toAst (3 upstream cases) ----------

    #[test]
    fn parses_plain_text() {
        let c = MessengerFormatConverter::new();
        let ast = c.to_ast("Hello world").unwrap();
        match &ast {
            Node::Root(r) => assert!(!r.children.is_empty()),
            _ => panic!("expected Root, got {ast:?}"),
        }
    }

    #[test]
    fn parses_markdown_bold() {
        let c = MessengerFormatConverter::new();
        let ast = c.to_ast("**bold**").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn handles_empty_text() {
        let c = MessengerFormatConverter::new();
        let ast = c.to_ast("").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    // ---------- fromAst (2 upstream cases) ----------

    #[test]
    fn roundtrips_plain_text() {
        let c = MessengerFormatConverter::new();
        let text = "Hello world";
        let ast = c.to_ast(text).unwrap();
        let result = c.from_ast(&ast);
        assert_eq!(result, text);
    }

    #[test]
    fn roundtrips_markdown_formatting() {
        let c = MessengerFormatConverter::new();
        let text = "**bold** and *italic*";
        let ast = c.to_ast(text).unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("bold"));
        assert!(result.contains("italic"));
    }

    // ---------- renderPostable (4 upstream cases) ----------

    #[test]
    fn renders_string_messages() {
        let c = MessengerFormatConverter::new();
        assert_eq!(c.render_postable_string("hello"), "hello");
    }

    #[test]
    fn renders_raw_messages() {
        let c = MessengerFormatConverter::new();
        assert_eq!(c.render_postable_raw("raw text"), "raw text");
    }

    #[test]
    fn renders_markdown_messages() {
        let c = MessengerFormatConverter::new();
        let result = c.render_postable_markdown("**bold**").unwrap();
        assert!(result.contains("bold"));
    }

    #[test]
    fn renders_ast_messages() {
        let c = MessengerFormatConverter::new();
        let ast = c.to_ast("hello from ast").unwrap();
        let result = c.render_postable_ast(&ast);
        assert!(result.contains("hello from ast"));
    }

    // The upstream "throws on invalid postable message shapes" case
    // tests `converter.renderPostable({unknown: "value"} as never)`,
    // which would dispatch through BaseFormatConverter::renderPostable
    // and throw `TypeError("Unknown postable message shape")`. In Rust
    // the dispatcher is the public `render_postable_*` methods - each
    // takes the matching argument and is type-checked at compile time,
    // so there's no runtime "unknown shape" path. The compile-time
    // rejection is the Rust equivalent of the upstream throw.
    //
    // Documented here so the test-floor inventory matches: this case
    // is "structurally enforced by Rust's type system" rather than a
    // runtime test.

    // ---------- extractPlainText (1 upstream case) ----------

    #[test]
    fn extracts_plain_text_from_markdown() {
        let c = MessengerFormatConverter::new();
        let result = c.extract_plain_text("**bold** text");
        assert!(result.contains("bold"));
        assert!(result.contains("text"));
    }
}
