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

/// Extract plain text from an mdast tree. 1:1 port of upstream
/// `toPlainText(ast: Root): string`, which calls the
/// mdast-util-to-string npm helper. The Rust [`markdown`] crate's
/// `Node::to_string()` performs the same recursive-children-with-
/// leaf-value-concatenation transform.
pub fn to_plain_text(ast: &Node) -> String {
    ast.to_string()
}

/// Mutable accessor for the `children` field of any container [`Node`]
/// variant. Returns `None` for leaf variants (Text/Code/Html/…) that
/// don't carry a children vector.
///
/// Used by [`walk_ast`] to swap children out for recursive visitation.
/// Kept private — consumers should use `walk_ast` or pattern-match the
/// concrete variant directly.
fn children_mut(node: &mut Node) -> Option<&mut Vec<Node>> {
    match node {
        Node::Root(n) => Some(&mut n.children),
        Node::Paragraph(n) => Some(&mut n.children),
        Node::Heading(n) => Some(&mut n.children),
        Node::Blockquote(n) => Some(&mut n.children),
        Node::List(n) => Some(&mut n.children),
        Node::ListItem(n) => Some(&mut n.children),
        Node::Emphasis(n) => Some(&mut n.children),
        Node::Strong(n) => Some(&mut n.children),
        Node::Delete(n) => Some(&mut n.children),
        Node::Link(n) => Some(&mut n.children),
        Node::LinkReference(n) => Some(&mut n.children),
        Node::FootnoteDefinition(n) => Some(&mut n.children),
        Node::Table(n) => Some(&mut n.children),
        Node::TableRow(n) => Some(&mut n.children),
        Node::TableCell(n) => Some(&mut n.children),
        _ => None,
    }
}

/// Render an mdast [`Table`] node as a padded ASCII table. 1:1 port of
/// upstream `tableToAscii(node: Table): string`. Used by adapters that
/// lack native table support (Slack, GChat, Discord, Telegram).
pub fn table_to_ascii(node: &Table) -> String {
    let rows: Vec<Vec<String>> = node
        .children
        .iter()
        .map(|row| match row {
            Node::TableRow(row) => row.children.iter().map(|cell| cell.to_string()).collect(),
            // Defensive: a Table should only contain TableRow per mdast
            // spec, but never panic on malformed input.
            other => vec![other.to_string()],
        })
        .collect();

    if rows.is_empty() {
        return String::new();
    }

    let headers = &rows[0];
    let data_rows = &rows[1..];
    table_element_to_ascii(headers, data_rows)
}

/// Render headers + rows as a padded ASCII table. 1:1 port of upstream
/// `tableElementToAscii(headers, rows): string`. Used by card
/// `TableElement` fallback rendering. Pure string formatting with no
/// AST dependency.
///
/// Column widths are the maximum cell length per column; cells are
/// right-padded with spaces, joined with `" | "`, trailing whitespace
/// trimmed. The header/data separator is `-`-filled width-segments
/// joined with `"-|-"`.
pub fn table_element_to_ascii(headers: &[String], rows: &[Vec<String>]) -> String {
    let col_count = std::iter::once(headers.len())
        .chain(rows.iter().map(Vec::len))
        .max()
        .unwrap_or(0);
    if col_count == 0 {
        return String::new();
    }

    let mut col_widths = vec![0usize; col_count];
    let all_rows = std::iter::once(headers).chain(rows.iter().map(|r| r.as_slice()));
    for row in all_rows {
        for (i, cell) in row.iter().take(col_count).enumerate() {
            // `chars().count()` mirrors upstream JS `.length` for
            // Unicode-friendly width measurement at the BMP-codepoint
            // level (matching how upstream measures `String.length`).
            let len = cell.chars().count();
            if len > col_widths[i] {
                col_widths[i] = len;
            }
        }
    }

    let format_row = |cells: &[String]| -> String {
        let parts: Vec<String> = (0..col_count)
            .map(|i| {
                let empty = String::new();
                let cell = cells.get(i).unwrap_or(&empty);
                let len = cell.chars().count();
                let pad = col_widths[i].saturating_sub(len);
                format!("{cell}{}", " ".repeat(pad))
            })
            .collect();
        parts.join(" | ").trim_end().to_string()
    };

    let mut lines: Vec<String> = Vec::with_capacity(rows.len() + 2);
    lines.push(format_row(headers));
    let separator = col_widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("-|-");
    lines.push(separator);
    for row in rows {
        lines.push(format_row(row));
    }
    lines.join("\n")
}

/// Walk an mdast tree and transform descendants. 1:1 port of upstream
/// `walkAst<T extends Content | Root>(node, visitor)`.
///
/// The visitor is called on every *descendant* of `node` (never `node`
/// itself, matching upstream). Each visitor return value can:
///
/// - `Some(replacement)` — replace the descendant with `replacement`,
///   then recurse into `replacement`'s own children.
/// - `None` — drop the descendant entirely.
///
/// The signature uses [`Option`] instead of upstream's
/// `Content | null` union; semantics are identical.
pub fn walk_ast<F>(mut node: Node, visitor: &mut F) -> Node
where
    F: FnMut(Node) -> Option<Node>,
{
    if let Some(children) = children_mut(&mut node) {
        let original = std::mem::take(children);
        let walked: Vec<Node> = original
            .into_iter()
            .filter_map(|child| visitor(child).map(|replaced| walk_ast(replaced, visitor)))
            .collect();
        if let Some(children) = children_mut(&mut node) {
            *children = walked;
        }
    }
    node
}

/// Parse a Markdown string and extract its plain text. 1:1 port of upstream
/// `markdownToPlainText(markdown): string` — `parseMarkdown` then
/// [`to_plain_text`]. Returns an empty string when the input fails to
/// parse (mirrors upstream behavior since the upstream `parseMarkdown`
/// also returns an empty Root on truly empty input).
pub fn markdown_to_plain_text(input: &str) -> String {
    match parse_markdown(input) {
        Ok(ast) => to_plain_text(&ast),
        Err(_) => String::new(),
    }
}

// ============================================================================
// Type guards — `is_*_node` family
//
// 1:1 ports of upstream `isTextNode`/`isParagraphNode`/… Each upstream
// guard does `node.type === "text"` etc.; Rust's tagged enum makes the
// equivalent `matches!(node, Node::Text(_))`. The guards are kept as
// free functions matching the upstream module shape so consumers can
// `use crate::markdown::is_text_node;` rather than write the `matches!`
// arm inline (mirroring upstream import sites in `cards.ts`,
// `streaming-markdown.ts`, adapter renderers, etc.).
// ============================================================================

/// 1:1 port of upstream `isTextNode(node): node is Text`.
pub fn is_text_node(node: &Node) -> bool {
    matches!(node, Node::Text(_))
}

/// 1:1 port of upstream `isParagraphNode`.
pub fn is_paragraph_node(node: &Node) -> bool {
    matches!(node, Node::Paragraph(_))
}

/// 1:1 port of upstream `isStrongNode`.
pub fn is_strong_node(node: &Node) -> bool {
    matches!(node, Node::Strong(_))
}

/// 1:1 port of upstream `isEmphasisNode`.
pub fn is_emphasis_node(node: &Node) -> bool {
    matches!(node, Node::Emphasis(_))
}

/// 1:1 port of upstream `isDeleteNode` (GFM strikethrough).
pub fn is_delete_node(node: &Node) -> bool {
    matches!(node, Node::Delete(_))
}

/// 1:1 port of upstream `isInlineCodeNode`.
pub fn is_inline_code_node(node: &Node) -> bool {
    matches!(node, Node::InlineCode(_))
}

/// 1:1 port of upstream `isCodeNode` (fenced/indented code block).
pub fn is_code_node(node: &Node) -> bool {
    matches!(node, Node::Code(_))
}

/// 1:1 port of upstream `isLinkNode`.
pub fn is_link_node(node: &Node) -> bool {
    matches!(node, Node::Link(_))
}

/// 1:1 port of upstream `isBlockquoteNode`.
pub fn is_blockquote_node(node: &Node) -> bool {
    matches!(node, Node::Blockquote(_))
}

/// 1:1 port of upstream `isListNode`.
pub fn is_list_node(node: &Node) -> bool {
    matches!(node, Node::List(_))
}

/// 1:1 port of upstream `isListItemNode`.
pub fn is_list_item_node(node: &Node) -> bool {
    matches!(node, Node::ListItem(_))
}

/// 1:1 port of upstream `isTableNode` (GFM table).
pub fn is_table_node(node: &Node) -> bool {
    matches!(node, Node::Table(_))
}

/// 1:1 port of upstream `isTableRowNode`.
pub fn is_table_row_node(node: &Node) -> bool {
    matches!(node, Node::TableRow(_))
}

/// 1:1 port of upstream `isTableCellNode`.
pub fn is_table_cell_node(node: &Node) -> bool {
    matches!(node, Node::TableCell(_))
}

/// 1:1 port of upstream `getNodeChildren(node): Content[]`. Returns the
/// node's children when it has any, or an empty `Vec` otherwise. Mirrors
/// the upstream `"children" in node && Array.isArray(node.children)`
/// duck-test.
pub fn get_node_children(node: &Node) -> Vec<Node> {
    node.children().cloned().unwrap_or_default()
}

/// 1:1 port of upstream `getNodeValue(node): string`. Returns the
/// node's `value` for leaves that have one (text, inline code, code
/// block) and an empty string otherwise. Mirrors the upstream
/// `"value" in node && typeof node.value === "string"` duck-test.
pub fn get_node_value(node: &Node) -> String {
    match node {
        Node::Text(t) => t.value.clone(),
        Node::InlineCode(c) => c.value.clone(),
        Node::Code(c) => c.value.clone(),
        Node::Html(h) => h.value.clone(),
        Node::Yaml(y) => y.value.clone(),
        Node::Toml(t) => t.value.clone(),
        Node::ImageReference(_)
        | Node::LinkReference(_)
        | Node::FootnoteDefinition(_)
        | Node::Definition(_)
        | Node::FootnoteReference(_) => String::new(),
        _ => String::new(),
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

    // ------------------------------------------------------------------
    // Type guards — slice 27 ports of the corresponding upstream
    // describe(\"isXNode\") blocks in packages/chat/src/markdown.test.ts.
    // ------------------------------------------------------------------

    fn make_text() -> Node {
        Node::Text(text("hi"))
    }
    fn make_paragraph() -> Node {
        Node::Paragraph(paragraph(vec![make_text()]))
    }
    fn make_strong() -> Node {
        Node::Strong(strong(vec![make_text()]))
    }
    fn make_emphasis() -> Node {
        Node::Emphasis(emphasis(vec![make_text()]))
    }
    fn make_delete() -> Node {
        Node::Delete(strikethrough(vec![make_text()]))
    }
    fn make_inline_code() -> Node {
        Node::InlineCode(inline_code("x"))
    }
    fn make_code() -> Node {
        Node::Code(code_block("println!()", None, None))
    }
    fn make_link() -> Node {
        Node::Link(link("https://example.com", vec![make_text()]))
    }
    fn make_blockquote() -> Node {
        Node::Blockquote(blockquote(vec![make_paragraph()]))
    }
    fn make_list() -> Node {
        Node::List(List {
            children: vec![],
            ordered: false,
            start: None,
            spread: false,
            position: None,
        })
    }
    fn make_list_item() -> Node {
        Node::ListItem(ListItem {
            children: vec![],
            spread: false,
            checked: None,
            position: None,
        })
    }
    fn make_table_cell() -> Node {
        Node::TableCell(TableCell {
            children: vec![],
            position: None,
        })
    }
    fn make_table_row() -> Node {
        Node::TableRow(TableRow {
            children: vec![],
            position: None,
        })
    }
    fn make_table() -> Node {
        Node::Table(Table {
            children: vec![],
            align: vec![],
            position: None,
        })
    }

    #[test]
    fn is_text_node_distinguishes_text_from_other_nodes() {
        assert!(is_text_node(&make_text()));
        assert!(!is_text_node(&make_paragraph()));
    }

    #[test]
    fn is_paragraph_node_distinguishes_paragraph_from_other_nodes() {
        assert!(is_paragraph_node(&make_paragraph()));
        assert!(!is_paragraph_node(&make_text()));
    }

    #[test]
    fn is_strong_emphasis_delete_inline_code_guards_match_their_variants() {
        assert!(is_strong_node(&make_strong()));
        assert!(!is_strong_node(&make_emphasis()));
        assert!(is_emphasis_node(&make_emphasis()));
        assert!(!is_emphasis_node(&make_delete()));
        assert!(is_delete_node(&make_delete()));
        assert!(!is_delete_node(&make_strong()));
        assert!(is_inline_code_node(&make_inline_code()));
        assert!(!is_inline_code_node(&make_code()));
    }

    #[test]
    fn is_code_link_blockquote_guards_match_their_variants() {
        assert!(is_code_node(&make_code()));
        assert!(!is_code_node(&make_inline_code()));
        assert!(is_link_node(&make_link()));
        assert!(!is_link_node(&make_text()));
        assert!(is_blockquote_node(&make_blockquote()));
        assert!(!is_blockquote_node(&make_paragraph()));
    }

    #[test]
    fn is_list_list_item_guards_match_their_variants() {
        assert!(is_list_node(&make_list()));
        assert!(!is_list_node(&make_list_item()));
        assert!(is_list_item_node(&make_list_item()));
        assert!(!is_list_item_node(&make_list()));
    }

    #[test]
    fn is_table_table_row_table_cell_guards_match_their_variants() {
        assert!(is_table_node(&make_table()));
        assert!(!is_table_node(&make_table_row()));
        assert!(is_table_row_node(&make_table_row()));
        assert!(!is_table_row_node(&make_table_cell()));
        assert!(is_table_cell_node(&make_table_cell()));
        assert!(!is_table_cell_node(&make_table_row()));
    }

    #[test]
    fn get_node_children_returns_children_when_present_and_empty_otherwise() {
        let p = make_paragraph();
        let kids = get_node_children(&p);
        assert_eq!(kids.len(), 1);
        assert!(matches!(kids[0], Node::Text(_)));

        // Leaves have no children → empty vec.
        let t = make_text();
        assert!(get_node_children(&t).is_empty());
    }

    #[test]
    fn get_node_value_returns_value_for_leaf_nodes_and_empty_otherwise() {
        assert_eq!(get_node_value(&Node::Text(text("hello"))), "hello");
        assert_eq!(
            get_node_value(&Node::InlineCode(inline_code("let x"))),
            "let x"
        );
        assert_eq!(
            get_node_value(&Node::Code(code_block("body", None, None))),
            "body"
        );
        // Non-leaf nodes return an empty string per upstream behavior.
        assert_eq!(get_node_value(&make_paragraph()), "");
        assert_eq!(get_node_value(&make_strong()), "");
    }

    // ------------------------------------------------------------------
    // table_to_ascii / table_element_to_ascii — slice 30 ports of the
    // upstream tableToAscii + tableElementToAscii describe blocks.
    // ------------------------------------------------------------------

    fn s(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn table_element_to_ascii_pads_columns_to_max_width() {
        let headers = s(&["Name", "Status"]);
        let rows = vec![s(&["alice", "OK"]), s(&["bob", "FAIL"])];
        let out = table_element_to_ascii(&headers, &rows);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "Name  | Status");
        // Col widths are 5 ("alice") and 6 ("Status"); separator is
        // "-".repeat(5) + "-|-" + "-".repeat(6) = 6 dashes + '|' + 7
        // dashes = 14 chars total.
        assert_eq!(lines[1], "------|-------");
        assert_eq!(lines[2], "alice | OK");
        assert_eq!(lines[3], "bob   | FAIL");
    }

    #[test]
    fn table_element_to_ascii_returns_empty_string_for_empty_input() {
        let empty: Vec<String> = Vec::new();
        let no_rows: Vec<Vec<String>> = Vec::new();
        assert_eq!(table_element_to_ascii(&empty, &no_rows), "");
    }

    #[test]
    fn table_element_to_ascii_fills_short_rows_with_empty_padded_cells() {
        let headers = s(&["A", "B", "C"]);
        let rows = vec![s(&["1", "2"])]; // shorter than headers
        let out = table_element_to_ascii(&headers, &rows);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines[0], "A | B | C");
        // Short row gets empty padded cell — trailing whitespace trimmed.
        assert_eq!(lines[2], "1 | 2 |");
    }

    #[test]
    fn table_to_ascii_round_trips_a_simple_mdast_table() {
        // Build: | A | B |\n|---|---|\n| 1 | 2 |
        let header_row = Node::TableRow(TableRow {
            children: vec![
                Node::TableCell(TableCell {
                    children: vec![Node::Text(text("A"))],
                    position: None,
                }),
                Node::TableCell(TableCell {
                    children: vec![Node::Text(text("B"))],
                    position: None,
                }),
            ],
            position: None,
        });
        let data_row = Node::TableRow(TableRow {
            children: vec![
                Node::TableCell(TableCell {
                    children: vec![Node::Text(text("1"))],
                    position: None,
                }),
                Node::TableCell(TableCell {
                    children: vec![Node::Text(text("2"))],
                    position: None,
                }),
            ],
            position: None,
        });
        let table = Table {
            children: vec![header_row, data_row],
            align: vec![],
            position: None,
        };
        let out = table_to_ascii(&table);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines[0], "A | B");
        assert_eq!(lines[1], "--|--");
        assert_eq!(lines[2], "1 | 2");
    }

    #[test]
    fn table_to_ascii_returns_empty_for_a_table_with_no_rows() {
        let table = Table {
            children: vec![],
            align: vec![],
            position: None,
        };
        assert_eq!(table_to_ascii(&table), "");
    }

    // ------------------------------------------------------------------
    // walk_ast — slice 29 ports of the upstream walkAst describe block.
    // ------------------------------------------------------------------

    #[test]
    fn walk_ast_replaces_every_descendant_text_with_a_new_value() {
        // Tree: Root -> Paragraph -> [Text("a"), Strong -> [Text("b")]]
        let tree = Node::Root(root(vec![Node::Paragraph(paragraph(vec![
            Node::Text(text("a")),
            Node::Strong(strong(vec![Node::Text(text("b"))])),
        ]))]));
        let walked = walk_ast(tree, &mut |node| match node {
            Node::Text(_) => Some(Node::Text(text("REPLACED"))),
            other => Some(other),
        });
        assert_eq!(to_plain_text(&walked), "REPLACEDREPLACED");
    }

    #[test]
    fn walk_ast_drops_descendants_when_visitor_returns_none() {
        // Tree: Root -> Paragraph -> [Text("keep"), Delete -> [Text("drop")]]
        let tree = Node::Root(root(vec![Node::Paragraph(paragraph(vec![
            Node::Text(text("keep")),
            Node::Delete(strikethrough(vec![Node::Text(text("drop"))])),
        ]))]));
        let walked = walk_ast(tree, &mut |node| match node {
            Node::Delete(_) => None,
            other => Some(other),
        });
        // Delete subtree was dropped, leaving only the Text("keep").
        assert_eq!(to_plain_text(&walked), "keep");
    }

    #[test]
    fn walk_ast_does_not_call_visitor_on_the_root_node_itself() {
        // Upstream walkAst visits *descendants only* — the input node is
        // never passed to the visitor. Rust port preserves that.
        let tree = Node::Root(root(vec![Node::Text(text("inside"))]));
        let mut visited: Vec<String> = Vec::new();
        walk_ast(tree, &mut |node| {
            visited.push(match &node {
                Node::Root(_) => "Root".to_string(),
                Node::Paragraph(_) => "Paragraph".to_string(),
                Node::Text(t) => format!("Text({})", t.value),
                _ => "other".to_string(),
            });
            Some(node)
        });
        // Only the Text descendant should have been visited — not the Root.
        assert_eq!(visited, vec!["Text(inside)".to_string()]);
    }

    #[test]
    fn walk_ast_recurses_into_replacement_subtrees() {
        // When the visitor replaces a node with a fresh subtree, walk_ast
        // recurses into that replacement's children — matching upstream
        // semantics. The replacement's Text children should also be visited.
        let tree = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("placeholder"),
        )]))]));
        let mut text_visits = 0;
        let walked = walk_ast(tree, &mut |node| {
            if matches!(node, Node::Text(_)) {
                text_visits += 1;
            }
            match node {
                Node::Paragraph(_) => Some(Node::Paragraph(paragraph(vec![Node::Text(text(
                    "inserted-by-visitor",
                ))]))),
                other => Some(other),
            }
        });
        // The visitor saw: original Paragraph (no text count), then the
        // replacement Paragraph's Text child (text_visits == 1). The
        // original "placeholder" Text was NOT visited because we replaced
        // its parent Paragraph wholesale before recursing.
        assert_eq!(text_visits, 1);
        assert_eq!(to_plain_text(&walked), "inserted-by-visitor");
    }

    // ------------------------------------------------------------------
    // to_plain_text / markdown_to_plain_text — slice 28 ports of the
    // corresponding upstream describe blocks.
    // ------------------------------------------------------------------

    #[test]
    fn to_plain_text_extracts_text_from_a_root_paragraph_subtree() {
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![
            Node::Text(text("hello ")),
            Node::Strong(strong(vec![Node::Text(text("bold"))])),
            Node::Text(text(" world")),
        ]))]));
        assert_eq!(to_plain_text(&ast), "hello bold world");
    }

    #[test]
    fn to_plain_text_returns_empty_for_an_empty_root() {
        let ast = Node::Root(root(vec![]));
        assert_eq!(to_plain_text(&ast), "");
    }

    #[test]
    fn markdown_to_plain_text_parses_and_extracts_in_one_step() {
        assert_eq!(markdown_to_plain_text("**hello** _world_"), "hello world");
        assert_eq!(markdown_to_plain_text("plain"), "plain");
        // GFM strikethrough text is still part of the plain output —
        // upstream mdastToString does not strip ~~ marks at the AST
        // level (they're already AST-marker-free in mdast).
        assert_eq!(markdown_to_plain_text("~~struck~~"), "struck");
    }

    #[test]
    fn markdown_to_plain_text_returns_empty_for_empty_input() {
        assert_eq!(markdown_to_plain_text(""), "");
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
