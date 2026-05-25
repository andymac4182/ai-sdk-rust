//! Teams markdown <-> standard Markdown conversion.
//!
//! 1:1 port of `packages/adapter-teams/src/markdown.ts`. Teams
//! supports both standard markdown and a subset of HTML for inline
//! formatting (`<b>`, `<strong>`, `<i>`, `<em>`, `<s>`, `<strike>`,
//! `<a>`, `<code>`, `<pre>`) plus mention tags (`<at>name</at>`).
//! The port pre-processes Teams HTML into standard markdown before
//! parsing, then walks the AST to emit Teams-flavored markdown.

use chat_sdk_adapter_shared::card_utils::escape_table_cell;
use chat_sdk_chat::markdown::{
    Node, ParseMarkdownError, default_node_to_text, from_ast_with_node_converter,
    get_node_children, is_blockquote_node, is_code_node, is_delete_node, is_emphasis_node,
    is_inline_code_node, is_link_node, is_list_node, is_paragraph_node, is_strong_node,
    is_table_node, is_text_node, parse_markdown, render_list, to_plain_text,
};
use chat_sdk_chat::types::AdapterPostableMessage;

/// 1:1 port of upstream
/// `class TeamsFormatConverter extends BaseFormatConverter`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TeamsFormatConverter;

impl TeamsFormatConverter {
    /// 1:1 with upstream `new TeamsFormatConverter()`.
    pub fn new() -> Self {
        Self
    }

    /// Parse Teams text (HTML or markdown) into mdast. 1:1 with
    /// upstream `toAst(teamsText)`: rewrites Teams HTML / mention
    /// tags into standard markdown, decodes entities, then parses.
    pub fn to_ast(&self, teams_text: &str) -> Node {
        let s = teams_html_to_markdown(teams_text);
        parse_markdown(&s).unwrap_or_else(|_| {
            Node::Root(chat_sdk_chat::markdown::Root {
                children: vec![],
                position: None,
            })
        })
    }

    /// Render an mdast tree as Teams-flavored markdown. 1:1 with
    /// upstream `fromAst(ast)`: standard markdown with
    /// `<at>name</at>` rewrites in Text nodes and GFM tables.
    pub fn from_ast(&self, ast: &Node) -> String {
        from_ast_with_node_converter(ast, &|n| node_to_teams(n))
    }

    /// Plain-text extraction. 1:1 with the inherited
    /// `BaseFormatConverter.extractPlainText`.
    pub fn extract_plain_text(&self, teams_text: &str) -> String {
        to_plain_text(&self.to_ast(teams_text))
    }

    /// `fromMarkdown` shorthand: parse markdown then render as Teams.
    /// 1:1 with the inherited `BaseFormatConverter.fromMarkdown(markdown)`.
    pub fn from_markdown(&self, markdown: &str) -> Result<String, ParseMarkdownError> {
        let ast = parse_markdown(markdown)?;
        Ok(self.from_ast(&ast))
    }

    /// Render a postable message for Teams. 1:1 port of upstream
    /// `renderPostable(message)`.
    pub fn render_postable(&self, message: &AdapterPostableMessage) -> String {
        match message {
            AdapterPostableMessage::Text(s) => convert_mentions_to_teams(s),
            AdapterPostableMessage::Raw(r) => convert_mentions_to_teams(&r.raw),
            AdapterPostableMessage::Markdown(m) => match parse_markdown(&m.markdown) {
                Ok(ast) => self.from_ast(&ast),
                Err(_) => m.markdown.clone(),
            },
            AdapterPostableMessage::Ast(a) => {
                let root = Node::Root(a.ast.clone());
                self.from_ast(&root)
            }
            AdapterPostableMessage::Card(_) | AdapterPostableMessage::CardElement(_) => {
                String::new()
            }
        }
    }
}

/// Rewrite bare `@word` mentions as `<at>word</at>`. 1:1 with upstream
/// `convertMentionsToTeams`. Matches `/@(\w+)/g` (no lookbehind).
fn convert_mentions_to_teams(text: &str) -> String {
    fn is_word(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let mut j = i + 1;
            while j < bytes.len() && is_word(bytes[j]) {
                j += 1;
            }
            if j > i + 1 {
                out.push_str("<at>");
                out.push_str(&text[i + 1..j]);
                out.push_str("</at>");
                i = j;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xc0 {
        1
    } else if b < 0xe0 {
        2
    } else if b < 0xf0 {
        3
    } else {
        4
    }
}

/// Pre-process Teams text: convert HTML tags + entities to
/// standard markdown. 1:1 with upstream `toAst` rewrite chain.
fn teams_html_to_markdown(input: &str) -> String {
    // 1. `<at>Name</at>` -> `@Name` (case-insensitive).
    let s = rewrite_tag(input, "at", |inner| format!("@{inner}"));
    // 2. `<b>` / `<strong>` -> `**text**`.
    let s = rewrite_tag(&s, "b", |inner| format!("**{inner}**"));
    let s = rewrite_tag(&s, "strong", |inner| format!("**{inner}**"));
    // 3. `<i>` / `<em>` -> `_text_`.
    let s = rewrite_tag(&s, "i", |inner| format!("_{inner}_"));
    let s = rewrite_tag(&s, "em", |inner| format!("_{inner}_"));
    // 4. `<s>` / `<strike>` -> `~~text~~`.
    let s = rewrite_tag(&s, "s", |inner| format!("~~{inner}~~"));
    let s = rewrite_tag(&s, "strike", |inner| format!("~~{inner}~~"));
    // 5. `<a href="url">text</a>` -> `[text](url)`.
    let s = rewrite_anchor(&s);
    // 6. `<code>text</code>` -> `` `text` ``.
    let s = rewrite_tag(&s, "code", |inner| format!("`{inner}`"));
    // 7. `<pre>text</pre>` -> fenced code block.
    let s = rewrite_tag(&s, "pre", |inner| format!("```\n{inner}\n```"));
    // 8. Strip any remaining HTML tags (loop to handle nested/reconstructed).
    let mut s = s;
    loop {
        let stripped = strip_html_tags(&s);
        if stripped == s {
            break;
        }
        s = stripped;
    }
    // 9. Decode HTML entities (single pass, no double-unescape).
    decode_html_entities(&s)
}

/// Strip every `<...>` tag from `s`. Single-pass byte scan.
fn strip_html_tags(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Skip until matching `>`.
            if let Some(rel) = s[i + 1..].find('>') {
                i = i + 1 + rel + 1;
                continue;
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// Decode the 5 HTML entities upstream supports. 1:1 with the
/// `entityMap` lookup in `toAst`.
fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find('&') {
        out.push_str(&rest[..idx]);
        let tail = &rest[idx..];
        let replaced = if let Some(stripped) = tail.strip_prefix("&lt;") {
            out.push('<');
            stripped
        } else if let Some(stripped) = tail.strip_prefix("&gt;") {
            out.push('>');
            stripped
        } else if let Some(stripped) = tail.strip_prefix("&amp;") {
            out.push('&');
            stripped
        } else if let Some(stripped) = tail.strip_prefix("&quot;") {
            out.push('"');
            stripped
        } else if let Some(stripped) = tail.strip_prefix("&#39;") {
            out.push('\'');
            stripped
        } else {
            out.push('&');
            &tail[1..]
        };
        rest = replaced;
    }
    out.push_str(rest);
    out
}

/// Find every `<TAG>inner</TAG>` pair (case-insensitive on the tag
/// name) and replace with `replacer(inner)`. Doesn't recurse - the
/// caller invokes us per tag-type so order matches upstream's
/// chained `.replace(/<b>...<\/b>/gi, ...)` calls.
fn rewrite_tag(input: &str, tag: &str, mut replacer: impl FnMut(&str) -> String) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<'
            && i + tag.len() + 1 < bytes.len()
            && bytes[i + tag.len() + 1] == b'>'
            && input[i + 1..].len() >= tag.len()
            && input[i + 1..i + 1 + tag.len()].eq_ignore_ascii_case(tag)
        {
            // Look for matching closing tag.
            let close = format!("</{tag}>");
            let inner_start = i + 1 + tag.len() + 1;
            if let Some(close_rel) = case_insensitive_find(&input[inner_start..], &close) {
                let inner_end = inner_start + close_rel;
                let inner = &input[inner_start..inner_end];
                // Skip if inner contains `<` (not a leaf tag) - the
                // upstream regex `[^<]+` would also reject this.
                if !inner.contains('<') {
                    out.push_str(&replacer(inner));
                    i = inner_end + close.len();
                    continue;
                }
            }
        }
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// Rewrite `<a href="url" ...>text</a>` -> `[text](url)`. Matches
/// upstream's `/<a[^>]+href="([^"]+)"[^>]*>([^<]+)<\/a>/gi`.
fn rewrite_anchor(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let Some(open_idx) = find_open_anchor(rest) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..open_idx]);
        let tail = &rest[open_idx..];
        // Find `>`.
        let Some(gt) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        // Extract href.
        let attrs = &tail[..gt];
        let Some(href) = extract_href(attrs) else {
            out.push_str(&tail[..=gt]);
            rest = &tail[gt + 1..];
            continue;
        };
        let after_open = &tail[gt + 1..];
        // Find `</a>`.
        let Some(close_rel) = case_insensitive_find(after_open, "</a>") else {
            out.push_str(tail);
            return out;
        };
        let inner = &after_open[..close_rel];
        if inner.contains('<') {
            // upstream `[^<]+` rejects.
            out.push_str(&tail[..gt + 1]);
            rest = after_open;
            continue;
        }
        out.push_str(&format!("[{inner}]({href})"));
        rest = &after_open[close_rel + 4..];
    }
}

fn find_open_anchor(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'<'
            && (bytes[i + 1] == b'a' || bytes[i + 1] == b'A')
            && (bytes[i + 2] == b' '
                || bytes[i + 2] == b'\t'
                || bytes[i + 2] == b'>'
                || bytes[i + 2] == b'/')
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn extract_href(attrs: &str) -> Option<&str> {
    let i = attrs.find("href=\"")?;
    let after = &attrs[i + 6..];
    let end = after.find('"')?;
    Some(&after[..end])
}

fn case_insensitive_find(haystack: &str, needle: &str) -> Option<usize> {
    let needle_lower = needle.to_ascii_lowercase();
    let hb = haystack.as_bytes();
    let nb = needle_lower.as_bytes();
    if hb.len() < nb.len() {
        return None;
    }
    for i in 0..=hb.len() - nb.len() {
        if haystack[i..i + nb.len()].eq_ignore_ascii_case(needle) {
            return Some(i);
        }
    }
    None
}

/// 1:1 port of upstream `nodeToTeams(node)` walker.
fn node_to_teams(node: &Node) -> String {
    if is_paragraph_node(node) {
        return get_node_children(node)
            .iter()
            .map(node_to_teams)
            .collect::<Vec<_>>()
            .concat();
    }
    if is_text_node(node) {
        if let Node::Text(t) = node {
            return convert_mentions_to_teams(&t.value);
        }
    }
    if is_strong_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_teams)
            .collect::<Vec<_>>()
            .concat();
        return format!("**{content}**");
    }
    if is_emphasis_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_teams)
            .collect::<Vec<_>>()
            .concat();
        return format!("_{content}_");
    }
    if is_delete_node(node) {
        let content: String = get_node_children(node)
            .iter()
            .map(node_to_teams)
            .collect::<Vec<_>>()
            .concat();
        return format!("~~{content}~~");
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
                .map(node_to_teams)
                .collect::<Vec<_>>()
                .concat();
            return format!("[{link_text}]({})", l.url);
        }
    }
    if is_blockquote_node(node) {
        return get_node_children(node)
            .iter()
            .map(|child| format!("> {}", node_to_teams(child)))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if is_list_node(node) {
        if let Node::List(l) = node {
            return render_list(l, 0, &|child| node_to_teams(child), "-");
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
            return table_to_gfm(t);
        }
    }
    default_node_to_text(node, &|child| node_to_teams(child))
}

/// Render a mdast Table as a GFM pipe-syntax table. 1:1 with
/// upstream `tableToGfm(node)`.
fn table_to_gfm(table: &chat_sdk_chat::markdown::Table) -> String {
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(table.children.len());
    for row in &table.children {
        if let Node::TableRow(tr) = row {
            let mut cells: Vec<String> = Vec::with_capacity(tr.children.len());
            for cell in &tr.children {
                let content: String = get_node_children(cell)
                    .iter()
                    .map(node_to_teams)
                    .collect::<Vec<_>>()
                    .concat();
                cells.push(content);
            }
            rows.push(cells);
        }
    }
    if rows.is_empty() {
        return String::new();
    }
    let mut lines = Vec::with_capacity(rows.len() + 1);
    let header: Vec<String> = rows[0].iter().map(|c| escape_table_cell(c)).collect();
    lines.push(format!("| {} |", header.join(" | ")));
    let separators: Vec<&str> = rows[0].iter().map(|_| "---").collect();
    lines.push(format!("| {} |", separators.join(" | ")));
    for row in rows.iter().skip(1) {
        let cells: Vec<String> = row.iter().map(|c| escape_table_cell(c)).collect();
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::types::{PostableAst, PostableMarkdown, PostableRaw};

    fn c() -> TeamsFormatConverter {
        TeamsFormatConverter::new()
    }

    fn rt(input: &str) -> String {
        let conv = c();
        let ast = conv.to_ast(input);
        conv.from_ast(&ast)
    }

    // ---------- fromAst (AST -> Teams format), 16 upstream cases ----------

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:8 > "should convert bold"
    #[test]
    fn from_ast_should_convert_bold() {
        let ast = c().to_ast("**bold text**");
        let result = c().from_ast(&ast);
        assert!(result.contains("**bold text**"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:14 > "should convert italic"
    #[test]
    fn from_ast_should_convert_italic() {
        let ast = c().to_ast("_italic text_");
        let result = c().from_ast(&ast);
        assert!(result.contains("_italic text_"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:20 > "should convert strikethrough"
    #[test]
    fn from_ast_should_convert_strikethrough() {
        let ast = c().to_ast("~~strikethrough~~");
        let result = c().from_ast(&ast);
        assert!(result.contains("~~strikethrough~~"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:26 > "should preserve inline code"
    #[test]
    fn from_ast_should_preserve_inline_code() {
        let ast = c().to_ast("Use `const x = 1`");
        let result = c().from_ast(&ast);
        assert!(result.contains("`const x = 1`"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:32 > "should handle code blocks"
    #[test]
    fn from_ast_should_handle_code_blocks() {
        let input = "```js\nconst x = 1;\n```";
        let ast = c().to_ast(input);
        let output = c().from_ast(&ast);
        assert!(output.contains("```"), "got: {output}");
        assert!(output.contains("const x = 1;"), "got: {output}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:40 > "should convert links to markdown format"
    #[test]
    fn from_ast_should_convert_links_to_markdown_format() {
        let ast = c().to_ast("[link text](https://example.com)");
        let result = c().from_ast(&ast);
        assert!(
            result.contains("[link text](https://example.com)"),
            "got: {result}"
        );
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:46 > "should handle blockquotes"
    #[test]
    fn from_ast_should_handle_blockquotes() {
        let ast = c().to_ast("> quoted text");
        let result = c().from_ast(&ast);
        assert!(result.contains("> quoted text"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:52 > "should handle unordered lists"
    #[test]
    fn from_ast_should_handle_unordered_lists() {
        let ast = c().to_ast("- item 1\n- item 2");
        let result = c().from_ast(&ast);
        assert!(result.contains("- item 1"), "got: {result}");
        assert!(result.contains("- item 2"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:59 > "should handle ordered lists"
    #[test]
    fn from_ast_should_handle_ordered_lists() {
        let ast = c().to_ast("1. first\n2. second");
        let result = c().from_ast(&ast);
        assert!(result.contains("1."), "got: {result}");
        assert!(result.contains("2."), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:66 > "should indent nested unordered lists"
    #[test]
    fn from_ast_should_indent_nested_unordered_lists() {
        let result = c()
            .from_markdown("- parent\n  - child 1\n  - child 2")
            .expect("from_markdown");
        assert_eq!(result, "- parent\n  - child 1\n  - child 2");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:73 > "should indent nested ordered lists"
    #[test]
    fn from_ast_should_indent_nested_ordered_lists() {
        let result = c()
            .from_markdown("1. first\n   1. sub-first\n   2. sub-second\n2. second")
            .expect("from_markdown");
        assert!(result.contains("1. first"), "got: {result}");
        assert!(result.contains("  1. sub-first"), "got: {result}");
        assert!(result.contains("  2. sub-second"), "got: {result}");
        assert!(result.contains("2. second"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:83 > "should handle deeply nested lists"
    #[test]
    fn from_ast_should_handle_deeply_nested_lists() {
        let result = c()
            .from_markdown("- level 1\n  - level 2\n    - level 3")
            .expect("from_markdown");
        assert!(result.contains("- level 1"), "got: {result}");
        assert!(result.contains("  - level 2"), "got: {result}");
        assert!(result.contains("    - level 3"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:92 > "should keep sibling items at the same indent level"
    #[test]
    fn from_ast_should_keep_sibling_items_at_the_same_indent_level() {
        let result = c()
            .from_markdown("- item 1\n- item 2\n- item 3")
            .expect("from_markdown");
        assert_eq!(result, "- item 1\n- item 2\n- item 3");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:97 > "should handle mixed ordered and unordered nesting"
    #[test]
    fn from_ast_should_handle_mixed_ordered_and_unordered_nesting() {
        let result = c()
            .from_markdown("1. first\n   - sub a\n   - sub b\n2. second")
            .expect("from_markdown");
        assert!(result.contains("1. first"), "got: {result}");
        assert!(result.contains("  - sub a"), "got: {result}");
        assert!(result.contains("  - sub b"), "got: {result}");
        assert!(result.contains("2. second"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:107 > "should convert @mentions to <at>mention</at>"
    #[test]
    fn from_ast_should_convert_at_mentions_to_at_tag() {
        let ast = c().to_ast("Hello @someone");
        let result = c().from_ast(&ast);
        assert!(result.contains("<at>someone</at>"), "got: {result}");
    }

    // 1:1 with upstream packages/adapter-teams/src/markdown.test.ts:113 > "should handle thematic breaks"
    #[test]
    fn from_ast_should_handle_thematic_breaks() {
        let ast = c().to_ast("text\n\n---\n\nmore");
        let result = c().from_ast(&ast);
        assert!(result.contains("---"), "got: {result}");
    }

    // ---------- toAst (Teams HTML -> AST), 11 upstream cases ----------

    #[test]
    fn to_ast_should_convert_at_mentions_to_at_signs() {
        let r = c().extract_plain_text("<at>Alice</at> hi");
        assert!(r.contains("@Alice"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_b_tags_to_bold() {
        let r = c().from_ast(&c().to_ast("This is <b>bold</b>"));
        assert!(r.contains("**bold**"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_strong_tags_to_bold() {
        let r = c().from_ast(&c().to_ast("This is <strong>bold</strong>"));
        assert!(r.contains("**bold**"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_i_tags_to_italic() {
        let r = c().from_ast(&c().to_ast("This is <i>italic</i>"));
        assert!(r.contains("_italic_"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_em_tags_to_italic() {
        let r = c().from_ast(&c().to_ast("This is <em>italic</em>"));
        assert!(r.contains("_italic_"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_s_tags_to_strikethrough() {
        let r = c().from_ast(&c().to_ast("This is <s>struck</s>"));
        assert!(r.contains("~~struck~~"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_a_tags_to_links() {
        let r = c().from_ast(&c().to_ast("<a href=\"https://x.com\">link</a>"));
        assert!(r.contains("[link](https://x.com)"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_code_tags_to_inline_code() {
        let r = c().from_ast(&c().to_ast("Use <code>x</code>"));
        assert!(r.contains("`x`"), "got: {r}");
    }

    #[test]
    fn to_ast_should_convert_pre_tags_to_code_blocks() {
        let r = c().from_ast(&c().to_ast("<pre>const x = 1;</pre>"));
        assert!(r.contains("```"), "got: {r}");
        assert!(r.contains("const x = 1;"), "got: {r}");
    }

    #[test]
    fn to_ast_should_strip_remaining_html_tags() {
        let r = c().extract_plain_text("<div>hello</div>");
        assert!(r.contains("hello"), "got: {r}");
        assert!(!r.contains("<div"), "got: {r}");
    }

    #[test]
    fn to_ast_should_decode_html_entities() {
        let r = c().extract_plain_text("a &amp; b &lt;c&gt; &quot;d&quot; &#39;e&#39;");
        assert!(r.contains("a & b"), "got: {r}");
        assert!(r.contains("<c>"), "got: {r}");
        assert!(r.contains("\"d\""), "got: {r}");
        assert!(r.contains("'e'"), "got: {r}");
    }

    // ---------- renderPostable (5 upstream cases) ----------

    #[test]
    fn render_postable_at_mentions_in_plain_strings() {
        let msg: AdapterPostableMessage = "Hello @bob".into();
        let r = c().render_postable(&msg);
        assert_eq!(r, "Hello <at>bob</at>");
    }

    #[test]
    fn render_postable_at_mentions_in_raw_messages() {
        let msg = AdapterPostableMessage::Raw(PostableRaw {
            raw: "Hi @sue".into(),
            attachments: None,
            files: None,
        });
        let r = c().render_postable(&msg);
        assert_eq!(r, "Hi <at>sue</at>");
    }

    #[test]
    fn render_postable_markdown_messages() {
        let msg = AdapterPostableMessage::Markdown(PostableMarkdown {
            markdown: "Hey **bold** @alice".into(),
            attachments: None,
            files: None,
        });
        let r = c().render_postable(&msg);
        assert!(r.contains("**bold**"), "got: {r}");
        assert!(r.contains("<at>alice</at>"), "got: {r}");
    }

    #[test]
    fn render_postable_ast_messages() {
        let ast = c().to_ast("Hello **world**");
        let root = match ast {
            Node::Root(r) => r,
            _ => panic!("expected root"),
        };
        let msg = AdapterPostableMessage::Ast(PostableAst {
            ast: root,
            attachments: None,
            files: None,
        });
        let r = c().render_postable(&msg);
        assert!(r.contains("Hello"));
        assert!(r.contains("**world**"));
    }

    #[test]
    fn render_postable_empty_message() {
        let msg: AdapterPostableMessage = "".into();
        assert_eq!(c().render_postable(&msg), "");
    }

    // ---------- extractPlainText (5 upstream cases) ----------

    #[test]
    fn extract_plain_text_should_remove_bold_markers() {
        assert_eq!(c().extract_plain_text("Hello **world**!"), "Hello world!");
    }

    #[test]
    fn extract_plain_text_should_remove_italic_markers() {
        assert_eq!(c().extract_plain_text("Hello _world_!"), "Hello world!");
    }

    #[test]
    fn extract_plain_text_should_handle_empty_string() {
        assert_eq!(c().extract_plain_text(""), "");
    }

    #[test]
    fn extract_plain_text_should_handle_plain_text() {
        assert_eq!(c().extract_plain_text("Just plain text"), "Just plain text");
    }

    #[test]
    fn extract_plain_text_should_handle_inline_code() {
        let r = c().extract_plain_text("Use `code`");
        assert_eq!(r, "Use code");
    }

    // ---------- table rendering (2 upstream cases) ----------

    #[test]
    fn table_should_render_markdown_tables_as_gfm_tables() {
        let r = rt("| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(r.contains("| A | B |"), "got: {r}");
        assert!(r.contains("| --- | --- |"), "got: {r}");
        assert!(r.contains("| 1 | 2 |"), "got: {r}");
    }

    #[test]
    fn table_should_render_tables_with_pipe_syntax() {
        let r = rt("| H1 | H2 |\n|---|---|\n| d1 | d2 |\n| d3 | d4 |");
        assert!(r.contains("|"), "got: {r}");
        assert!(r.contains("H1"));
        assert!(r.contains("d3"));
    }
}
