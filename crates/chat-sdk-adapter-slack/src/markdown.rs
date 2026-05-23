//! Slack mrkdwn <-> standard Markdown conversion.
//!
//! 1:1 port (in progress) of `packages/adapter-slack/src/markdown.ts`.
//! This slice ports the `to_ast` / `from_ast` / `to_markdown` /
//! `extract_plain_text` / plain `render_postable_string,raw`
//! subset. The `toSlackPayload` + `toResponseUrlText` paths
//! require Slack-specific payload types and follow in later
//! slices, alongside the full `nodeToMrkdwn` walker.

use chat_sdk_chat::markdown::{
    Node, ParseMarkdownError, parse_markdown, stringify_markdown, to_plain_text,
};

use crate::format::{
    link_bare_slack_mentions, markdown_bold_to_slack_mrkdwn, slack_mrkdwn_to_markdown,
};

/// 1:1 port of upstream
/// `class SlackFormatConverter extends BaseFormatConverter`.
/// Stateless; the struct mirrors upstream shape.
#[derive(Debug, Default, Clone, Copy)]
pub struct SlackFormatConverter;

impl SlackFormatConverter {
    /// 1:1 with upstream `new SlackFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse Slack mrkdwn into an mdast Node. 1:1 with upstream
    /// `toAst(mrkdwn)`: normalises Slack-specific mention / link /
    /// bold / strike syntax to standard markdown via the
    /// [`slack_mrkdwn_to_markdown`] scanner, then parseMarkdown.
    pub fn to_ast(&self, mrkdwn: &str) -> Result<Node, ParseMarkdownError> {
        let normalised = slack_mrkdwn_to_markdown(mrkdwn);
        parse_markdown(&normalised)
    }

    /// Stringify mdast as standard Markdown. 1:1 with upstream
    /// `fromAst(ast)` (Slack accepts standard markdown via
    /// `markdown_text` block).
    pub fn from_ast(&self, ast: &Node) -> String {
        stringify_markdown(ast)
    }

    /// Convenience: parse Slack mrkdwn and stringify to standard
    /// Markdown in one call. 1:1 with the inherited
    /// `BaseFormatConverter.toMarkdown(platformText)` which is
    /// `stringifyMarkdown(toAst(platformText))`.
    pub fn to_markdown(&self, slack_text: &str) -> String {
        match self.to_ast(slack_text) {
            Ok(ast) => self.from_ast(&ast),
            Err(_) => slack_text.to_string(),
        }
    }

    /// Plain-text extraction. 1:1 with the inherited
    /// `extractPlainText`.
    pub fn extract_plain_text(&self, slack_text: &str) -> String {
        match self.to_ast(slack_text) {
            Ok(node) => to_plain_text(&node),
            Err(_) => slack_text.to_string(),
        }
    }

    /// `string` postable: route to plain `text` (no markdown
    /// conversion). 1:1 with the upstream "preserves literal
    /// markdown chars" branch. Bare `@mentions` are still
    /// rewritten to Slack's `<@user>` form via
    /// [`finalize_plain`].
    pub fn render_postable_string(&self, message: &str) -> String {
        finalize_plain(message)
    }

    /// `{raw}` postable: same routing as string. 1:1 with
    /// upstream.
    pub fn render_postable_raw(&self, raw: &str) -> String {
        finalize_plain(raw)
    }
}

/// Finalize a plain-text Slack message. 1:1 with upstream's
/// `this.finalize(text)` for string / raw branches in
/// `toSlackPayload`: rewrites bare `@USER` mentions to
/// `<@USER>` so Slack renders them as proper mentions.
fn finalize_plain(text: &str) -> String {
    link_bare_slack_mentions(text)
}

/// Finalize a markdown Slack message. 1:1 with upstream's
/// `this.finalize(text)` for `markdown` / `ast` branches in
/// `toSlackPayload`: applies `markdownBoldToSlackMrkdwn` on top
/// of the bare-mention rewrite so `**bold**` -> `*bold*` for
/// Slack's mrkdwn renderer.
#[allow(dead_code)]
fn finalize_markdown(text: &str) -> String {
    let with_mentions = link_bare_slack_mentions(text);
    markdown_bold_to_slack_mrkdwn(&with_mentions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter() -> SlackFormatConverter {
        SlackFormatConverter::new()
    }

    // ---------- toMarkdown (mrkdwn -> markdown), 7 upstream cases ----------

    #[test]
    fn should_convert_bold() {
        let result = converter().to_markdown("Some *bold* text");
        assert!(result.contains("**bold**"), "got: {result}");
    }

    #[test]
    fn should_convert_strikethrough() {
        let result = converter().to_markdown("Some ~deleted~ text");
        assert!(result.contains("~~deleted~~"), "got: {result}");
    }

    #[test]
    fn should_convert_links_with_text() {
        let result = converter().to_markdown("Visit <https://example.com|our site>");
        assert!(
            result.contains("[our site](https://example.com)"),
            "got: {result}"
        );
    }

    #[test]
    fn should_convert_bare_links() {
        let result = converter().to_markdown("Visit <https://example.com>");
        assert!(result.contains("https://example.com"), "got: {result}");
        // Should NOT contain the angle brackets.
        assert!(!result.contains("<https://"), "got: {result}");
    }

    #[test]
    fn should_convert_user_mentions() {
        let result = converter().to_markdown("Hey <@U12345|alice>");
        assert!(result.contains("@alice"), "got: {result}");
    }

    #[test]
    fn should_convert_channel_mentions() {
        let result = converter().to_markdown("In <#C12345|general>");
        assert!(result.contains("#general"), "got: {result}");
    }

    #[test]
    fn should_convert_bare_channel_id_mentions() {
        let result = converter().to_markdown("In <#C12345>");
        assert!(result.contains("#C12345"), "got: {result}");
    }

    // ---------- additive Rust-side ----------

    #[test]
    fn to_ast_passes_plain_text_through_unchanged() {
        let ast = converter().to_ast("Hello world").unwrap();
        assert!(matches!(ast, Node::Root(_)));
    }

    #[test]
    fn from_ast_round_trips_simple_markdown() {
        let c = converter();
        let ast = c.to_ast("**already standard**").unwrap();
        let result = c.from_ast(&ast);
        assert!(result.contains("**already standard**"), "got: {result}");
    }

    #[test]
    fn extract_plain_text_strips_slack_markup() {
        let result = converter().extract_plain_text("*bold* and ~strike~");
        assert!(result.contains("bold"));
        assert!(result.contains("strike"));
        assert!(!result.contains('*'));
        assert!(!result.contains('~'));
    }

    #[test]
    fn render_postable_string_rewrites_bare_mentions_but_keeps_literal_markdown_chars() {
        // From upstream "preserves literal markdown chars" test.
        let result = converter().render_postable_string("Some *literal* text @U12345");
        assert!(result.contains("*literal*"), "literal markdown preserved");
        assert!(result.contains("<@U12345>"), "bare mention rewritten");
    }

    #[test]
    fn render_postable_raw_passthrough_preserves_markdown_chars() {
        let result = converter().render_postable_raw("Some *literal* @U12345");
        assert!(result.contains("*literal*"));
        assert!(result.contains("<@U12345>"));
    }

    #[test]
    fn finalize_markdown_helper_collapses_bold_and_rewrites_mentions() {
        assert_eq!(finalize_markdown("**bold** @U12345"), "*bold* <@U12345>");
    }

    // ---------- toPlainText, 5 upstream cases ----------
    // Upstream `toPlainText` is a deprecated alias for `extractPlainText`,
    // so these cases exercise our [`SlackFormatConverter::extract_plain_text`].

    #[test]
    fn to_plain_text_should_remove_bold_markers() {
        assert_eq!(
            converter().extract_plain_text("Hello *world*!"),
            "Hello world!"
        );
    }

    #[test]
    fn to_plain_text_should_remove_italic_markers() {
        assert_eq!(
            converter().extract_plain_text("Hello _world_!"),
            "Hello world!"
        );
    }

    #[test]
    fn to_plain_text_should_extract_link_text() {
        assert_eq!(
            converter().extract_plain_text("Check <https://example.com|this>"),
            "Check this"
        );
    }

    #[test]
    fn to_plain_text_should_format_user_mentions() {
        let result = converter().extract_plain_text("Hey <@U123>!");
        assert!(result.contains("@U123"), "got: {result}");
    }

    #[test]
    fn to_plain_text_should_handle_complex_messages() {
        let input = "*Bold* and _italic_ with <https://x.com|link> and <@U123|user>";
        let result = converter().extract_plain_text(input);
        assert!(result.contains("Bold"), "got: {result}");
        assert!(result.contains("italic"), "got: {result}");
        assert!(result.contains("link"), "got: {result}");
        assert!(result.contains("user"), "got: {result}");
        assert!(!result.contains('*'), "got: {result}");
        assert!(!result.contains('<'), "got: {result}");
    }
}
