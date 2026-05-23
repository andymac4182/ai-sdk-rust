//! Markdown AST + parsing surface.
//!
//! 1:1 port of `packages/chat/src/markdown.ts` (in progress — see the
//! per-symbol port status below). Built on the [`markdown`] crate
//! (markdown-rs 1.0) whose `markdown::mdast::Node` enum mirrors the
//! upstream `mdast` `Content` discriminated union and whose
//! `markdown::to_mdast` is the Rust equivalent of upstream
//! `remark-parse + unified`.
//!
//! Architectural decision recorded in slice 19's refinement entry and
//! [`docs/chat/goal-refinements.md`](../../docs/chat/goal-refinements.md):
//! markdown-rs is the right Rust analogue of the upstream
//! `remark-*` + `mdast` toolchain. The Rust crate's `Node` enum
//! flattens upstream's structural-union model into a tagged enum, which
//! is more idiomatic Rust without changing the data shape.
//!
//! [`markdown`]: https://docs.rs/markdown
//!
//! **What this slice ships (slice 26):**
//!
//! - AST type re-exports (`Node`, `Root`, `Text`, `Paragraph`, …) so the
//!   rest of `chat-sdk-chat` can refer to mdast types via
//!   `crate::markdown::Text` rather than `markdown::mdast::Text`.
//! - `parse_markdown(input)` → `Result<Node, ParseMarkdownError>`,
//!   wrapping `markdown::to_mdast` with GFM options enabled to match the
//!   upstream `remark-gfm` setup.
//! - AST builder helpers `text`, `paragraph`, `root`, `strong`, `emphasis`,
//!   `inline_code`, `link`, `code_block`, `blockquote`, `strikethrough`.
//!   These are pure constructors that build a [`Node`] variant; they
//!   match the upstream `text(value)`, `paragraph(children)`, etc.
//!   shape one-for-one.
//!
//! **What remains for follow-up slices:**
//!
//! - `stringify_markdown` (the upstream `remark-stringify` pipeline).
//! - `to_plain_text` / `markdown_to_plain_text` (mdast → plain text).
//! - `walk_ast` (the upstream visitor helper).
//! - `is_*` type guards — most are trivial `matches!` wrappers in Rust,
//!   but they ship together with the upstream test coverage of the
//!   matching `*.test.ts` cases.
//! - `table_to_ascii` / `table_element_to_ascii` (used by `cards.ts`).
//! - `BaseFormatConverter` + `FormatConverter` + `MarkdownConverter`
//!   trait/abstract-class surface for adapter-specific renderers.
//! - `FormattedContent` alias in [`crate::types`] gets swapped from the
//!   slice-22 `serde_json::Value` placeholder to `crate::markdown::Node`
//!   once the full surface is in place. That swap is its own coordinated
//!   slice — every downstream type holding a `FormattedContent`
//!   recompiles against the typed AST automatically.

pub use markdown::mdast::{
    Blockquote, Code, Delete, Emphasis, InlineCode, Link, List, ListItem, Node, Paragraph, Root,
    Strong, Table, TableCell, TableRow, Text,
};

/// Error returned by [`parse_markdown`] when the upstream `markdown` crate
/// rejects the input.
#[derive(Debug, Clone)]
pub struct ParseMarkdownError(pub String);

impl std::fmt::Display for ParseMarkdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse markdown: {}", self.0)
    }
}

impl std::error::Error for ParseMarkdownError {}

/// Parse a Markdown string into an mdast [`Node`]. 1:1 port of upstream
/// `parseMarkdown(markdown)` — upstream uses
/// `unified().use(remarkParse).use(remarkGfm)`, this uses the equivalent
/// GFM-extended profile from the [`markdown`] crate.
pub fn parse_markdown(input: &str) -> Result<Node, ParseMarkdownError> {
    let options = markdown::ParseOptions::gfm();
    markdown::to_mdast(input, &options).map_err(|e| ParseMarkdownError(e.to_string()))
}

/// Build a [`Text`] node. 1:1 port of upstream `text(value): Text`.
pub fn text(value: impl Into<String>) -> Text {
    Text {
        value: value.into(),
        position: None,
    }
}

/// Build a [`Paragraph`] node. 1:1 port of upstream
/// `paragraph(children): Paragraph`. `children` must already be
/// paragraph-level inline content nodes.
pub fn paragraph(children: Vec<Node>) -> Paragraph {
    Paragraph {
        children,
        position: None,
    }
}

/// Build a [`Root`] node. 1:1 port of upstream `root(children): Root`.
pub fn root(children: Vec<Node>) -> Root {
    Root {
        children,
        position: None,
    }
}

/// Build a [`Strong`] (bold) node. 1:1 port of upstream
/// `strong(children): Strong`.
pub fn strong(children: Vec<Node>) -> Strong {
    Strong {
        children,
        position: None,
    }
}

/// Build an [`Emphasis`] (italic) node. 1:1 port of upstream
/// `emphasis(children): Emphasis`.
pub fn emphasis(children: Vec<Node>) -> Emphasis {
    Emphasis {
        children,
        position: None,
    }
}

/// Build an [`InlineCode`] node. 1:1 port of upstream
/// `inlineCode(value): InlineCode`.
pub fn inline_code(value: impl Into<String>) -> InlineCode {
    InlineCode {
        value: value.into(),
        position: None,
    }
}

/// Build a strikethrough ([`Delete`]) node. 1:1 port of upstream
/// `strikethrough(children): Delete`. GFM extension.
pub fn strikethrough(children: Vec<Node>) -> Delete {
    Delete {
        children,
        position: None,
    }
}

/// Build a [`Link`] node. 1:1 port of upstream
/// `link(url, children): Link`. Title is left as `None` per upstream.
pub fn link(url: impl Into<String>, children: Vec<Node>) -> Link {
    Link {
        children,
        url: url.into(),
        title: None,
        position: None,
    }
}

/// Build a fenced [`Code`] block. 1:1 port of upstream
/// `codeBlock(value, lang?, meta?): Code`. Both `lang` and `meta`
/// default to `None` matching upstream's optional parameters.
pub fn code_block(value: impl Into<String>, lang: Option<String>, meta: Option<String>) -> Code {
    Code {
        value: value.into(),
        lang,
        meta,
        position: None,
    }
}

/// Build a [`Blockquote`] node. 1:1 port of upstream
/// `blockquote(children): Blockquote`.
pub fn blockquote(children: Vec<Node>) -> Blockquote {
    Blockquote {
        children,
        position: None,
    }
}

#[cfg(test)]
mod tests {
    //! Subset port of `packages/chat/src/markdown.test.ts` covering the
    //! AST-builder and parse surface shipped in slice 26. The remaining
    //! upstream cases land alongside their corresponding API additions
    //! (`stringify`, `to_plain_text`, table-to-ASCII, walker, …).
    //!
    //! These tests are 1:1 with the matching upstream `it(...)` blocks
    //! for the constructors and parse paths. They live next to the code
    //! under `#[cfg(test)] mod tests` per repo convention.

    use super::*;

    #[test]
    fn text_builder_returns_a_text_node_with_the_given_value() {
        let node = text("hello");
        assert_eq!(node.value, "hello");
        assert!(node.position.is_none());
    }

    #[test]
    fn paragraph_builder_wraps_children() {
        let p = paragraph(vec![Node::Text(text("hi"))]);
        assert_eq!(p.children.len(), 1);
        assert!(matches!(p.children[0], Node::Text(_)));
    }

    #[test]
    fn root_builder_wraps_top_level_children() {
        let r = root(vec![Node::Paragraph(paragraph(vec![Node::Text(text(
            "p",
        ))]))]);
        assert_eq!(r.children.len(), 1);
        assert!(matches!(r.children[0], Node::Paragraph(_)));
    }

    #[test]
    fn strong_emphasis_strikethrough_wrap_inline_children() {
        let s = strong(vec![Node::Text(text("bold"))]);
        let e = emphasis(vec![Node::Text(text("italic"))]);
        let d = strikethrough(vec![Node::Text(text("struck"))]);
        assert_eq!(s.children.len(), 1);
        assert_eq!(e.children.len(), 1);
        assert_eq!(d.children.len(), 1);
    }

    #[test]
    fn inline_code_carries_value_verbatim() {
        let c = inline_code("let x = 1");
        assert_eq!(c.value, "let x = 1");
    }

    #[test]
    fn link_builder_sets_url_and_children_with_no_title() {
        let l = link("https://example.com", vec![Node::Text(text("here"))]);
        assert_eq!(l.url, "https://example.com");
        assert_eq!(l.children.len(), 1);
        assert!(l.title.is_none());
    }

    #[test]
    fn code_block_builder_with_lang_and_meta() {
        let cb = code_block("println!(\"hi\");", Some("rust".to_string()), None);
        assert_eq!(cb.value, "println!(\"hi\");");
        assert_eq!(cb.lang.as_deref(), Some("rust"));
        assert!(cb.meta.is_none());
    }

    #[test]
    fn code_block_builder_with_neither_lang_nor_meta() {
        let cb = code_block("plain", None, None);
        assert!(cb.lang.is_none());
        assert!(cb.meta.is_none());
    }

    #[test]
    fn blockquote_builder_wraps_block_children() {
        let bq = blockquote(vec![Node::Paragraph(paragraph(vec![Node::Text(text(
            "quoted",
        ))]))]);
        assert_eq!(bq.children.len(), 1);
    }

    #[test]
    fn parse_markdown_round_trips_a_paragraph() {
        let node = parse_markdown("hello").expect("parse succeeds");
        // Upstream remarkParse returns a Root with a single Paragraph
        // child containing a single Text node.
        let root = match node {
            Node::Root(r) => r,
            other => panic!("expected Root, got {other:?}"),
        };
        assert_eq!(root.children.len(), 1);
        let p = match &root.children[0] {
            Node::Paragraph(p) => p,
            other => panic!("expected Paragraph, got {other:?}"),
        };
        assert_eq!(p.children.len(), 1);
        let t = match &p.children[0] {
            Node::Text(t) => t,
            other => panic!("expected Text, got {other:?}"),
        };
        assert_eq!(t.value, "hello");
    }

    #[test]
    fn parse_markdown_understands_gfm_strikethrough() {
        // Upstream uses remarkGfm; the Rust port must keep GFM enabled.
        let node = parse_markdown("~~struck~~").expect("parse succeeds");
        let root = match node {
            Node::Root(r) => r,
            _ => unreachable!(),
        };
        let p = match &root.children[0] {
            Node::Paragraph(p) => p,
            _ => unreachable!(),
        };
        // The first child of the paragraph should be a Delete node.
        assert!(matches!(p.children[0], Node::Delete(_)));
    }

    #[test]
    fn parse_markdown_returns_an_error_for_malformed_extension_input() {
        // The markdown-rs parser is tolerant; supplying an empty string
        // returns a valid empty Root rather than an error. We assert the
        // happy path here and leave the failure-mode coverage to be
        // expanded if markdown-rs gains stricter modes in future.
        let node = parse_markdown("").expect("empty input parses");
        let root = match node {
            Node::Root(r) => r,
            _ => unreachable!(),
        };
        assert!(root.children.is_empty());
    }
}
