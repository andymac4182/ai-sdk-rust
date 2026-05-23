//! Slack mrkdwn <-> standard Markdown conversion.
//!
//! 1:1 port (in progress) of `packages/adapter-slack/src/markdown.ts`.
//! This slice ports the `to_ast` / `from_ast` / `to_markdown` /
//! `extract_plain_text` / plain `render_postable_string,raw`
//! subset. The `toSlackPayload` + `toResponseUrlText` paths
//! require Slack-specific payload types and follow in later
//! slices, alongside the full `nodeToMrkdwn` walker.

use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::markdown::{
    Node, ParseMarkdownError, default_node_to_text, from_ast_with_node_converter,
    get_node_children, is_blockquote_node, is_code_node, is_delete_node, is_emphasis_node,
    is_inline_code_node, is_link_node, is_list_node, is_paragraph_node, is_strong_node,
    is_table_node, is_text_node, parse_markdown, render_list, stringify_markdown, table_to_ascii,
    to_plain_text,
};
use chat_sdk_chat::types::AdapterPostableMessage;

use crate::format::{markdown_bold_to_slack_mrkdwn, slack_mrkdwn_to_markdown};

/// 1:1 port of upstream `BARE_MENTION_REGEX = /(?<![<\w])@(\w+)/g` from
/// `adapter-slack/src/markdown.ts`. Rewrites `@word` to `<@word>` whenever
/// the preceding char isn't `<` (avoids double-wrapping `<@U123>`) and
/// isn't a word char (preserves `user@example.com` and `domain@host`).
///
/// More permissive than [`link_bare_slack_mentions`] which only matches
/// Slack ID shapes (`U` / `W` + digits).
fn rewrite_bare_mentions(text: &str) -> String {
    fn is_word(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let prev_ok = i == 0 || (bytes[i - 1] != b'<' && !is_word(bytes[i - 1]));
            if prev_ok {
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
        }
        // Push this char and advance by its utf-8 length.
        let ch_len = if bytes[i] < 0x80 {
            1
        } else if bytes[i] < 0xc0 {
            1
        } else if bytes[i] < 0xe0 {
            2
        } else if bytes[i] < 0xf0 {
            3
        } else {
            4
        };
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// 1:1 port of upstream
/// `type SlackTextPayload = { text: string } | { markdown_text: string }`.
/// Slack's chat.postMessage accepts either `text` (legacy mrkdwn-ish, up to
/// ~40k chars) or `markdown_text` (native CommonMark render, ~12k cap,
/// mutually exclusive with `text` / `blocks`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackTextPayload {
    /// `{ text: string }` — plain text branch.
    Text(String),
    /// `{ markdown_text: string }` — Slack-rendered markdown branch.
    MarkdownText(String),
}

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

    /// Render an mdast tree to Slack's legacy mrkdwn text. 1:1 port of
    /// upstream `private astToMrkdwn(ast)` which calls
    /// `fromAstWithNodeConverter` with `nodeToMrkdwn` as the per-node
    /// converter.
    fn ast_to_mrkdwn(&self, ast: &Node) -> String {
        from_ast_with_node_converter(ast, &|n| node_to_mrkdwn(n))
    }

    /// Build text for Slack `response_url` payloads. 1:1 port of upstream
    /// `toResponseUrlText(message)`:
    ///
    /// - `string` / `{ raw }` -> `finalize_plain(text)` (same as plain
    ///   `toSlackPayload`)
    /// - `{ markdown }` -> parse + `ast_to_mrkdwn` + emoji placeholders
    /// - `{ ast }` -> `ast_to_mrkdwn` + emoji placeholders
    ///
    /// Slack rejects `markdown_text` on `response_url` (returns
    /// `no_text`), so markdown / AST messages are rendered to Slack's
    /// legacy mrkdwn format for this surface.
    pub fn to_response_url_text(&self, message: &AdapterPostableMessage) -> String {
        match message {
            AdapterPostableMessage::Text(s) => finalize_plain(s),
            AdapterPostableMessage::Raw(r) => finalize_plain(&r.raw),
            AdapterPostableMessage::Markdown(m) => {
                let ast = match parse_markdown(&m.markdown) {
                    Ok(node) => node,
                    Err(_) => return finalize_plain(&m.markdown),
                };
                let mrkdwn = self.ast_to_mrkdwn(&ast);
                convert_emoji_placeholders(&mrkdwn, PlaceholderPlatform::Slack, None)
            }
            AdapterPostableMessage::Ast(a) => {
                let root = Node::Root(a.ast.clone());
                let mrkdwn = self.ast_to_mrkdwn(&root);
                convert_emoji_placeholders(&mrkdwn, PlaceholderPlatform::Slack, None)
            }
            AdapterPostableMessage::Card(_) | AdapterPostableMessage::CardElement(_) => {
                String::new()
            }
        }
    }

    /// Build the Slack API payload fields for a message. 1:1 port of
    /// upstream `toSlackPayload(message)`:
    ///
    /// - `string` / `{ raw }` → `{ text }` (plain — preserves literal
    ///   `*`, `_`, etc.)
    /// - `{ markdown }` / `{ ast }` → `{ markdown_text }` (Slack renders
    ///   natively)
    ///
    /// Bare `@user` mentions are rewritten to `<@user>` and
    /// `{{emoji:NAME}}` placeholders are normalized for Slack in all
    /// branches via [`finalize_plain`].
    pub fn to_slack_payload(&self, message: &AdapterPostableMessage) -> SlackTextPayload {
        match message {
            AdapterPostableMessage::Text(s) => SlackTextPayload::Text(finalize_plain(s)),
            AdapterPostableMessage::Raw(r) => SlackTextPayload::Text(finalize_plain(&r.raw)),
            AdapterPostableMessage::Markdown(m) => {
                SlackTextPayload::MarkdownText(finalize_plain(&m.markdown))
            }
            AdapterPostableMessage::Ast(a) => {
                let root = Node::Root(a.ast.clone());
                let md = stringify_markdown(&root);
                SlackTextPayload::MarkdownText(finalize_plain(&md))
            }
            // Card / CardElement variants don't route through this method
            // upstream (they're rendered via toSlackBlocks); upstream returns
            // `{ text: "" }` for the fall-through, so do the same.
            AdapterPostableMessage::Card(_) | AdapterPostableMessage::CardElement(_) => {
                SlackTextPayload::Text(String::new())
            }
        }
    }
}

/// 1:1 port of upstream private `nodeToMrkdwn(node)`: renders a single
/// mdast node to Slack's legacy mrkdwn format. Used by `astToMrkdwn`
/// (and thus `toResponseUrlText`).
fn node_to_mrkdwn(node: &Node) -> String {
    if is_paragraph_node(node) {
        return get_node_children(node)
            .iter()
            .map(node_to_mrkdwn)
            .collect::<Vec<_>>()
            .concat();
    }
    if is_text_node(node) {
        if let Node::Text(t) = node {
            return rewrite_bare_mentions(&t.value);
        }
    }
    if is_strong_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_mrkdwn)
            .collect::<Vec<_>>()
            .concat();
        return format!("*{content}*");
    }
    if is_emphasis_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_mrkdwn)
            .collect::<Vec<_>>()
            .concat();
        return format!("_{content}_");
    }
    if is_delete_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_mrkdwn)
            .collect::<Vec<_>>()
            .concat();
        return format!("~{content}~");
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
                .map(node_to_mrkdwn)
                .collect::<Vec<_>>()
                .concat();
            return format!("<{}|{link_text}>", l.url);
        }
    }
    if is_blockquote_node(node) {
        return get_node_children(node)
            .iter()
            .map(|child| format!("> {}", node_to_mrkdwn(child)))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if is_list_node(node) {
        if let Node::List(l) = node {
            return render_list(l, 0, &|child| node_to_mrkdwn(child), "•");
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
    default_node_to_text(node, &|child| node_to_mrkdwn(child))
}

/// Finalize a Slack message text. 1:1 with upstream's private
/// `this.finalize(text)`: rewrites bare `@USER` mentions to `<@USER>`
/// then converts `{{emoji:NAME}}` placeholders to Slack's `:name:`
/// shortcode form. Used by both the plain-text and markdown branches
/// of `to_slack_payload` (upstream applies the same finalize to both).
fn finalize_plain(text: &str) -> String {
    let with_mentions = rewrite_bare_mentions(text);
    convert_emoji_placeholders(&with_mentions, PlaceholderPlatform::Slack, None)
}

/// Finalize text for the legacy Slack mrkdwn surface (e.g.
/// `response_url` payloads). Adds `markdownBoldToSlackMrkdwn` on top of
/// [`finalize_plain`] so `**bold**` -> `*bold*` for Slack's mrkdwn
/// renderer. Used by the upcoming `to_response_url_text` markdown / ast
/// branches.
#[allow(dead_code)]
fn finalize_markdown(text: &str) -> String {
    let with_mentions_and_emoji = finalize_plain(text);
    markdown_bold_to_slack_mrkdwn(&with_mentions_and_emoji)
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

    // ---------- toResponseUrlText, 2 upstream cases ----------

    #[test]
    fn to_response_url_text_renders_markdown_to_slack_mrkdwn_text() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "**Bold** and [link](https://example.com)".into(),
            attachments: None,
            files: None,
        });
        let result = converter().to_response_url_text(&msg);
        assert_eq!(result, "*Bold* and <https://example.com|link>");
    }

    #[test]
    fn to_response_url_text_renders_markdown_tables_as_ascii_code_blocks() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "| A | B |\n|---|---|\n| 1 | 2 |".into(),
            attachments: None,
            files: None,
        });
        let result = converter().to_response_url_text(&msg);
        assert!(result.contains("```\n"), "got: {result}");
    }

    // ---------- toSlackPayload (routing), 5 upstream cases ----------

    use chat_sdk_chat::markdown::{
        AlignKind, Node, Paragraph, Root, Strong, Table, TableCell, TableRow, Text, paragraph,
        root, strong, text,
    };
    use chat_sdk_chat::types::PostableAst;

    #[test]
    fn to_slack_payload_routes_plain_strings_to_text_preserves_literal_markdown_chars() {
        let msg: AdapterPostableMessage = "Use *foo* literally".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(p, SlackTextPayload::Text("Use *foo* literally".into()));
    }

    #[test]
    fn to_slack_payload_routes_raw_strings_to_text() {
        let msg = AdapterPostableMessage::Raw(PostableRaw {
            raw: "*already mrkdwn*".into(),
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        assert_eq!(p, SlackTextPayload::Text("*already mrkdwn*".into()));
    }

    #[test]
    fn to_slack_payload_routes_markdown_to_markdown_text() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "## Heading\n\n- a\n- b".into(),
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::MarkdownText("## Heading\n\n- a\n- b".into())
        );
    }

    #[test]
    fn to_slack_payload_routes_ast_to_markdown_text_via_stringify_markdown() {
        // Upstream: root({ paragraph([strong([text("bold")])] )}).
        let ast: Root = root(vec![Node::Paragraph(paragraph(vec![Node::Strong(
            strong(vec![Node::Text(text("bold"))]),
        )]))]);
        let msg = AdapterPostableMessage::Ast(PostableAst {
            ast,
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        match p {
            SlackTextPayload::MarkdownText(s) => {
                assert!(s.contains("**bold**"), "got: {s}");
            }
            other => panic!("expected MarkdownText, got {other:?}"),
        }
    }

    #[test]
    fn to_slack_payload_preserves_tables_when_rendering_ast_to_markdown_text() {
        // Upstream: root with one table { rows of [cell(A), cell(B)] and [cell(1), cell(2)] }.
        let make_cell = |v: &str| -> Node {
            Node::TableCell(TableCell {
                children: vec![Node::Text(text(v))],
                position: None,
            })
        };
        let make_row = |a: &str, b: &str| -> Node {
            Node::TableRow(TableRow {
                children: vec![make_cell(a), make_cell(b)],
                position: None,
            })
        };
        let ast: Root = root(vec![Node::Table(Table {
            children: vec![make_row("A", "B"), make_row("1", "2")],
            align: vec![AlignKind::None, AlignKind::None],
            position: None,
        })]);
        let msg = AdapterPostableMessage::Ast(PostableAst {
            ast,
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        match p {
            SlackTextPayload::MarkdownText(s) => {
                assert!(s.contains("| A | B |"), "got: {s}");
                assert!(s.contains("| 1 | 2 |"), "got: {s}");
            }
            other => panic!("expected MarkdownText, got {other:?}"),
        }
    }

    // Silence "unused import" warnings if any constructor isn't referenced.
    #[allow(dead_code)]
    fn _ast_helper_refs() -> (Paragraph, Strong, Text) {
        (paragraph(vec![]), strong(vec![]), text(""))
    }

    // ---------- mentions / toSlackPayload, 7 upstream cases ----------

    use chat_sdk_chat::types::{PostableMarkdown, PostableRaw};

    fn payload_text(p: &SlackTextPayload) -> &str {
        match p {
            SlackTextPayload::Text(s) => s.as_str(),
            SlackTextPayload::MarkdownText(s) => s.as_str(),
        }
    }

    #[test]
    fn mentions_does_not_double_wrap_existing_slack_user_mentions_in_plain_strings() {
        let msg: AdapterPostableMessage = "Hey <@U12345>. Please select".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::Text("Hey <@U12345>. Please select".into())
        );
    }

    #[test]
    fn mentions_does_not_double_wrap_existing_mentions_in_markdown() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "Hey <@U12345>. Please select".into(),
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::MarkdownText("Hey <@U12345>. Please select".into())
        );
    }

    #[test]
    fn mentions_rewrites_bare_mentions_in_plain_strings() {
        let msg: AdapterPostableMessage = "Hey @george. Please select".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::Text("Hey <@george>. Please select".into())
        );
    }

    #[test]
    fn mentions_rewrites_bare_mentions_in_markdown() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "Hey @george. Please select".into(),
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::MarkdownText("Hey <@george>. Please select".into())
        );
    }

    #[test]
    fn mentions_does_not_mangle_email_addresses_in_plain_strings() {
        let msg: AdapterPostableMessage = "Contact user@example.com for help".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::Text("Contact user@example.com for help".into())
        );
    }

    #[test]
    fn mentions_does_not_mangle_mailto_links() {
        let msg: AdapterPostableMessage = "Email <mailto:user@example.com>".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(
            p,
            SlackTextPayload::Text("Email <mailto:user@example.com>".into())
        );
    }

    #[test]
    fn mentions_converts_mentions_adjacent_to_non_word_punctuation() {
        let msg: AdapterPostableMessage = "(cc @george, @anne)".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(p, SlackTextPayload::Text("(cc <@george>, <@anne>)".into()));
    }

    // ---------- additive: raw + ast routing, emoji finalize ----------

    #[test]
    fn to_slack_payload_raw_routes_to_text_branch_with_finalize() {
        let msg = AdapterPostableMessage::Raw(PostableRaw {
            raw: "Hey @U1".into(),
            attachments: None,
            files: None,
        });
        let p = converter().to_slack_payload(&msg);
        assert_eq!(p, SlackTextPayload::Text("Hey <@U1>".into()));
    }

    #[test]
    fn to_slack_payload_finalize_converts_emoji_placeholders_for_slack() {
        let msg: AdapterPostableMessage = "{{emoji:smile}} hi".into();
        let p = converter().to_slack_payload(&msg);
        assert_eq!(payload_text(&p), ":smile: hi");
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
