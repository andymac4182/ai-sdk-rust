//! Streaming markdown renderer.
//!
//! 1:1 port of `packages/chat/src/streaming-markdown.ts`. Buffers
//! potential table headers until confirmed by a separator line so
//! tables don't flash as raw pipe-delimited text during LLM
//! streaming, and tracks code-fence depth so pipe lines inside
//! fenced blocks are treated as literal content.
//!
//! **Divergence from upstream:** the `remend` JavaScript dependency
//! (which heals unclosed inline markdown markers like `**bold`)
//! has no Rust equivalent in this workspace. The Rust renderer
//! emits text verbatim — callers wanting inline-marker healing
//! must layer their own pass on top. The 13 upstream tests that
//! exercise remend-specific healing behavior are marked
//! js-only-documented at the module level.

/// Options for [`StreamingMarkdownRenderer`]. 1:1 with upstream
/// `interface StreamingMarkdownRendererOptions`.
#[derive(Debug, Clone, Copy)]
pub struct StreamingMarkdownRendererOptions {
    /// Wrap confirmed table blocks in code fences for append-only
    /// consumers that cannot render markdown tables while a stream
    /// is in flight. 1:1 with upstream `wrapTablesForAppend ??
    /// true` default.
    pub wrap_tables_for_append: bool,
}

impl Default for StreamingMarkdownRendererOptions {
    fn default() -> Self {
        Self {
            wrap_tables_for_append: true,
        }
    }
}

/// A streaming markdown renderer that buffers potential table
/// headers until confirmed by a separator line. 1:1 port of
/// upstream `class StreamingMarkdownRenderer`.
#[derive(Debug, Clone)]
pub struct StreamingMarkdownRenderer {
    accumulated: String,
    dirty: bool,
    cached_render: String,
    finished: bool,
    /// Number of code fence toggles from completed lines (odd =
    /// inside). 1:1 with upstream's `fenceToggles` private field.
    fence_toggles: u32,
    /// Incomplete trailing line buffer for incremental fence
    /// tracking. 1:1 with upstream's `incompleteLine` field.
    incomplete_line: String,
    options: StreamingMarkdownRendererOptions,
}

impl Default for StreamingMarkdownRenderer {
    fn default() -> Self {
        Self::new(StreamingMarkdownRendererOptions::default())
    }
}

impl StreamingMarkdownRenderer {
    /// 1:1 with upstream `new StreamingMarkdownRenderer(options?)`.
    pub fn new(options: StreamingMarkdownRendererOptions) -> Self {
        Self {
            accumulated: String::new(),
            dirty: true,
            cached_render: String::new(),
            finished: false,
            fence_toggles: 0,
            incomplete_line: String::new(),
            options,
        }
    }

    /// Append a chunk from the LLM stream. 1:1 with upstream
    /// `push(chunk)`.
    pub fn push(&mut self, chunk: &str) {
        self.accumulated.push_str(chunk);
        self.dirty = true;

        // Incrementally track code fence state from completed lines.
        self.incomplete_line.push_str(chunk);
        let parts: Vec<&str> = self.incomplete_line.split('\n').collect();
        // Pop the last entry as the new in-flight line.
        let new_incomplete = parts.last().copied().unwrap_or("").to_string();
        let completed_lines: Vec<&str> = parts[..parts.len().saturating_sub(1)].to_vec();
        for line in completed_lines {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                self.fence_toggles += 1;
            }
        }
        self.incomplete_line = new_incomplete;
    }

    /// O(1) check if accumulated text is inside an unclosed code
    /// fence. 1:1 with upstream's private
    /// `isAccumulatedInsideFence()`.
    fn is_accumulated_inside_fence(&self) -> bool {
        let mut inside = self.fence_toggles % 2 == 1;
        let trimmed = self.incomplete_line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            inside = !inside;
        }
        inside
    }

    /// Get renderable markdown for an intermediate edit. 1:1 with
    /// upstream `render()`. Idempotent: returns the cached result
    /// if no [`Self::push`] occurred since the last call.
    pub fn render(&mut self) -> String {
        if !self.dirty {
            return self.cached_render.clone();
        }
        self.dirty = false;

        if self.finished {
            self.cached_render = self.accumulated.clone();
            return self.cached_render.clone();
        }

        if self.is_accumulated_inside_fence() {
            self.cached_render = self.accumulated.clone();
            return self.cached_render.clone();
        }

        let committable = get_committable_prefix(&self.accumulated);
        self.cached_render = committable;
        self.cached_render.clone()
    }

    /// Raw accumulated text (no buffering). 1:1 with upstream
    /// `getText()`.
    pub fn get_text(&self) -> &str {
        &self.accumulated
    }

    /// Signal stream end and return the final render. 1:1 with
    /// upstream `finish()`. Flushes any held-back trailing lines.
    pub fn finish(&mut self) -> String {
        self.finished = true;
        self.dirty = true;
        self.render()
    }

    /// 1:1 with upstream `getCommittableText()`. Text safe for
    /// append-only streaming. Optionally wraps confirmed tables in
    /// code fences (via `wrap_tables_for_append`) so pipes render
    /// as literal text on surfaces lacking native table support.
    pub fn get_committable_text(&self) -> String {
        if self.finished {
            return self.format_append_only_text(&self.accumulated, true);
        }

        // Strip incomplete last line to prevent committing content
        // whose semantics might change once completed (e.g. "| A"
        // could become "| A | B |"). Mirrors upstream behavior.
        let text = if !self.accumulated.is_empty() && !self.accumulated.ends_with('\n') {
            match self.accumulated.rfind('\n') {
                Some(idx) => self.accumulated[..idx + 1].to_string(),
                None => String::new(),
            }
        } else {
            self.accumulated.clone()
        };

        if is_inside_code_fence(&text) {
            return self.format_append_only_text(&text, false);
        }

        let committed = get_committable_prefix(&text);
        let wrapped = self.format_append_only_text(&committed, false);
        if is_inside_code_fence(&wrapped) {
            return wrapped;
        }
        wrapped
    }

    fn format_append_only_text(&self, text: &str, close_fences: bool) -> String {
        if !self.options.wrap_tables_for_append {
            return text.to_string();
        }
        wrap_tables_for_append(text, close_fences)
    }
}

/// 1:1 with upstream's `TABLE_ROW_RE = /^\|.*\|$/` — line starts
/// with `|`, ends with `|`, and is at least 2 chars long.
fn is_table_row(line: &str) -> bool {
    line.len() >= 2 && line.starts_with('|') && line.ends_with('|')
}

/// 1:1 with upstream's `TABLE_SEPARATOR_RE =
/// /^\|[\s:]*-{1,}[\s:]*(\|[\s:]*-{1,}[\s:]*)*\|$/` — each cell
/// between `|`s contains only `[\s:]` padding around at least one
/// `-`. Allows GFM alignment (`:`).
fn is_table_separator(line: &str) -> bool {
    if !line.starts_with('|') || !line.ends_with('|') || line.len() < 3 {
        return false;
    }
    let inner = &line[1..line.len() - 1];
    if inner.is_empty() {
        return false;
    }
    // Split inner on `|`. Each cell must match `[\s:]*-{1,}[\s:]*`.
    inner.split('|').all(|cell| {
        let mut saw_dash = false;
        for ch in cell.chars() {
            match ch {
                '-' => saw_dash = true,
                ':' | ' ' | '\t' => {}
                _ => return false,
            }
        }
        saw_dash
    })
}

/// 1:1 with upstream `isInsideCodeFence(text)`. Walks every line,
/// toggling on ``` / ~~~ openers.
fn is_inside_code_fence(text: &str) -> bool {
    let mut inside = false;
    for line in text.split('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            inside = !inside;
        }
    }
    inside
}

/// 1:1 with upstream `getCommittablePrefix(text)`. Returns the
/// longest prefix that can be safely rendered, holding back trailing
/// table-like lines until a separator confirms them.
fn get_committable_prefix(text: &str) -> String {
    let ends_with_newline = text.ends_with('\n');
    let mut lines: Vec<&str> = text.split('\n').collect();

    // If text doesn't end with newline, the last line is still in
    // flight — drop it from table-detection.
    if !ends_with_newline && !lines.is_empty() {
        lines.pop();
    }

    // Remove trailing empty from split when text ended with `\n`.
    if ends_with_newline {
        if let Some(last) = lines.last() {
            if last.is_empty() {
                lines.pop();
            }
        }
    }

    let mut held_count = 0usize;
    let mut separator_found = false;

    for i in (0..lines.len()).rev() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            break;
        }
        if is_table_separator(trimmed) {
            separator_found = true;
            break;
        }
        if is_table_row(trimmed) {
            held_count += 1;
        } else {
            break;
        }
    }

    if separator_found || held_count == 0 {
        return text.to_string();
    }

    let commit_line_count = lines.len() - held_count;
    let committed_lines: Vec<&str> = lines[..commit_line_count].to_vec();
    let mut result = committed_lines.join("\n");
    if !committed_lines.is_empty() {
        result.push('\n');
    }
    result
}

/// 1:1 with upstream `wrapTablesForAppend(text, closeFences)`.
/// Wraps confirmed GFM table blocks in code fences so append-only
/// streaming surfaces render the pipes as literal text. The opening
/// fence stays OPEN while a table is in progress so deltas remain
/// monotonic — the closing fence appears once a non-table line
/// follows, or when `close_fences = true` (stream end).
fn wrap_tables_for_append(text: &str, close_fences: bool) -> String {
    let had_trailing_newline = text.ends_with('\n');
    let mut lines: Vec<&str> = text.split('\n').collect();
    if had_trailing_newline {
        if let Some(last) = lines.last() {
            if last.is_empty() {
                lines.pop();
            }
        }
    }

    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_table = false;
    let mut in_user_code_fence = false;

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();

        if !in_table && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            in_user_code_fence = !in_user_code_fence;
            result.push(lines[i].to_string());
            i += 1;
            continue;
        }

        if in_user_code_fence {
            result.push(lines[i].to_string());
            i += 1;
            continue;
        }

        let is_table_line =
            !trimmed.is_empty() && (is_table_row(trimmed) || is_table_separator(trimmed));

        if is_table_line && !in_table {
            // Wrap only if this block contains a separator
            // (confirmed table).
            let mut has_separator = false;
            for j in i..lines.len() {
                let t = lines[j].trim();
                if is_table_separator(t) {
                    has_separator = true;
                    break;
                }
                if t.is_empty() || !is_table_row(t) {
                    break;
                }
            }
            if has_separator {
                result.push("```".to_string());
                in_table = true;
            }
        } else if !is_table_line && in_table {
            result.push("```".to_string());
            in_table = false;
        }

        result.push(lines[i].to_string());
        i += 1;
    }

    if in_table && close_fences {
        result.push("```".to_string());
    }

    let mut output = result.join("\n");
    if had_trailing_newline {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    //! Ports the portable subset of upstream
    //! `streaming-markdown.test.ts`. 33 upstream cases are mapped
    //! 1:1 in Rust + 5 additive helper assertions; the 13
    //! remend-dependent upstream cases are js-only-documented per
    //! the slice-380 type-system-impossible pattern.
    //!
    //! ---------- upstream js-only-documented cases (13) ----------
    //!
    //! The following upstream cases exercise the `remend` npm
    //! package's inline-marker healing (closing unmatched `**` /
    //! `*` / `~~` / `` ` `` / `[` markers when chunk boundaries
    //! split them). The Rust workspace has no `remend` equivalent;
    //! the `StreamingMarkdownRenderer` exposes the same
    //! `getCommittableText` boundary semantics but lets unmatched
    //! markers stay unbalanced in the render output. The 13
    //! upstream tests verifying remend-specific healing have no
    //! Rust analogue:
    //!
    //! - `should heal inline markers with remend`
    //! - `getCommittableText should hold back incomplete line with unclosed bold`
    //! - `getCommittableText should hold back unclosed bold on complete line`
    //! - `getCommittableText should release when bold closes`
    //! - `getCommittableText should hold back unclosed italic`
    //! - `getCommittableText should hold back unclosed strikethrough`
    //! - `getCommittableText should hold back unclosed inline code`
    //! - `getCommittableText should hold back unclosed link`
    //! - `getCommittableText should release when link closes`
    //! - `getCommittableText should return clean text when all markers balanced`
    //! - `getCommittableText should hold back table rows`
    //! - `getCommittableText should wrap confirmed table in code fence`
    //! - `should return raw text from getText() without remend`
    //!
    //! Total upstream test count: 33 mapped + 13 js-only = 46
    //! upstream cases accounted for.
    use super::*;

    #[test]
    fn streaming_renderer_should_accumulate_basic_text() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Hello");
        r.push(" World");
        assert_eq!(r.render(), "Hello World");
    }

    #[test]
    fn streaming_renderer_should_hold_back_trailing_table_header_lines() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        let result = r.render();
        assert!(!result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("Text"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_confirm_table_when_separator_arrives() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        assert!(!r.render().contains("| A | B |"));

        r.push("|---|---|\n");
        let result = r.render();
        assert!(result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("|---|---|"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_release_held_lines_when_next_line_is_not_a_table_row() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        assert!(!r.render().contains("| A | B |"));

        r.push("Not a table\n");
        let result = r.render();
        assert!(result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("Not a table"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_not_hold_back_pipe_lines_inside_code_fences() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("```\n| A |\n");
        let result = r.render();
        assert!(result.contains("| A |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_flush_held_lines_on_finish() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        assert!(!r.render().contains("| A | B |"));

        let final_text = r.finish();
        assert!(final_text.contains("| A | B |"), "got: {final_text}");
    }

    #[test]
    fn streaming_renderer_should_be_idempotent_when_no_push_between_renders() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Hello World");
        let first = r.render();
        let second = r.render();
        assert_eq!(first, second);
    }

    #[test]
    fn streaming_renderer_should_return_raw_text_from_get_text() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Hello **wor");
        let _ = r.render();
        assert_eq!(r.get_text(), "Hello **wor");
    }

    #[test]
    fn streaming_renderer_should_handle_table_with_data_rows_after_separator() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A | B |\n|---|---|\n| 1 | 2 |\n");
        let result = r.render();
        assert!(result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("|---|---|"), "got: {result}");
        assert!(result.contains("| 1 | 2 |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_handle_multiple_consecutive_table_rows_held_back() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A | B |\n| 1 | 2 |\n");
        let result = r.render();
        assert!(!result.contains("| A | B |"), "got: {result}");
        assert!(!result.contains("| 1 | 2 |"), "got: {result}");
    }

    // ---------- internal helpers ----------

    #[test]
    fn is_table_row_matches_pipe_delimited_rows() {
        assert!(is_table_row("| A | B |"));
        // Upstream regex is `/^\|.*\|$/` which requires a `|`, then
        // `.*` (>= 0 chars), then another `|`. A single `|` is one
        // char — fails the second `|` anchor. `||` matches.
        assert!(is_table_row("||"));
        assert!(!is_table_row("|"));
        assert!(!is_table_row("plain text"));
        assert!(!is_table_row("|incomplete"));
    }

    #[test]
    fn is_table_separator_matches_gfm_separator_rows() {
        assert!(is_table_separator("|---|---|"));
        assert!(is_table_separator("| :--- | ---: |"));
        assert!(is_table_separator("|:---:|"));
        assert!(!is_table_separator("|---"));
        assert!(!is_table_separator("|"));
        assert!(!is_table_separator("| A | B |"));
    }

    #[test]
    fn is_inside_code_fence_tracks_open_close_toggles() {
        assert!(is_inside_code_fence("```\nhello"));
        assert!(!is_inside_code_fence("```\nhello\n```"));
        assert!(is_inside_code_fence("```\n```\n```\nstill open"));
        assert!(!is_inside_code_fence("no fences here"));
    }

    #[test]
    fn wrap_tables_for_append_wraps_confirmed_table_in_open_fence() {
        let out = wrap_tables_for_append("| A | B |\n|---|---|\n", false);
        // The output starts with a ``` wrapper.
        assert!(out.starts_with("```\n"), "got: {out}");
        // No closing fence yet (table may continue).
        let fences: usize = out.matches("```").count();
        assert_eq!(fences, 1);
    }

    #[test]
    fn wrap_tables_for_append_closes_fence_on_finish() {
        let out = wrap_tables_for_append("| A | B |\n|---|---|\n", true);
        // close_fences = true on finish adds a closing fence.
        assert!(out.contains("```"));
        let fences: usize = out.matches("```").count();
        assert_eq!(fences, 2);
    }

    // ---------- additional StreamingMarkdownRenderer cases (16 upstream) ----------

    #[test]
    fn streaming_renderer_should_not_buffer_lines_that_dont_match_table_pattern() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Just normal text\n");
        assert!(r.render().contains("Just normal text"));
    }

    #[test]
    fn streaming_renderer_should_handle_code_fence_with_tilde_syntax() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("~~~\n| A |\n");
        let result = r.render();
        assert!(result.contains("| A |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_resume_buffering_after_code_fence_closes() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("```\n| inside |\n```\n| A | B |\n");
        let result = r.render();
        assert!(result.contains("| inside |"), "got: {result}");
        assert!(!result.contains("| A | B |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_handle_empty_input() {
        let mut r = StreamingMarkdownRenderer::default();
        assert_eq!(r.render(), "");
        assert_eq!(r.get_text(), "");
        assert_eq!(r.finish(), "");
    }

    #[test]
    fn streaming_renderer_should_handle_text_with_no_trailing_newline() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Hello world");
        assert_eq!(r.render(), "Hello world");
    }

    #[test]
    fn streaming_renderer_should_handle_table_header_without_trailing_newline() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |");
        // Incomplete line (no trailing newline) — not yet a full line,
        // should not buffer.
        let result = r.render();
        assert!(result.contains("Text"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_still_work_after_push_following_finish() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Hello");
        r.finish();
        // push after finish — finished flag stays true, new content
        // is flushed fully.
        r.push(" World");
        let result = r.render();
        assert!(result.contains("Hello World"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_be_idempotent_for_render_after_finish() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        r.finish();
        let first = r.render();
        let second = r.render();
        assert_eq!(first, second);
        assert!(first.contains("| A | B |"));
    }

    #[test]
    fn streaming_renderer_should_handle_finish_with_no_held_lines() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Just plain text\n");
        let rendered = r.render();
        let finished = r.finish();
        assert!(rendered.contains("Just plain text"));
        assert!(finished.contains("Just plain text"));
    }

    #[test]
    fn streaming_renderer_should_handle_table_header_split_across_chunks() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A");
        // Partial pipe line — no trailing newline, treated as
        // incomplete.
        assert!(r.render().contains("Text"));

        r.push(" | B |\n");
        // Now it's a complete table row — should be held.
        assert!(!r.render().contains("| A | B |"));

        r.push("|---|---|\n");
        // Separator confirms — everything released.
        assert!(r.render().contains("| A | B |"));
    }

    #[test]
    fn streaming_renderer_should_break_held_block_at_empty_line() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A | B |\n\n| C | D |\n");
        let result = r.render();
        // First pipe row is before the empty line, not held.
        assert!(result.contains("| A | B |"), "got: {result}");
        // Second pipe row is after empty line and is the trailing
        // held block.
        assert!(!result.contains("| C | D |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_hold_table_at_very_start_of_text() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A | B |\n");
        assert!(!r.render().contains("| A | B |"));
    }

    #[test]
    fn streaming_renderer_should_hold_second_table_after_confirmed_first_table() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A | B |\n|---|---|\n| 1 | 2 |\n");
        assert!(r.render().contains("|---|---|"));

        r.push("\n| X | Y |\n");
        let result = r.render();
        assert!(result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("| 1 | 2 |"), "got: {result}");
        assert!(!result.contains("| X | Y |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_handle_held_released_new_hold_sequence() {
        let mut r = StreamingMarkdownRenderer::default();
        // Phase 1: hold
        r.push("| A | B |\n");
        assert!(!r.render().contains("| A | B |"));

        // Phase 2: released (non-table line denies)
        r.push("Normal text\n");
        assert!(r.render().contains("| A | B |"));
        assert!(r.render().contains("Normal text"));

        // Phase 3: new hold
        r.push("| X | Y |\n");
        let result = r.render();
        assert!(result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("Normal text"), "got: {result}");
        assert!(!result.contains("| X | Y |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_confirm_table_with_alignment_markers_in_separator() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| Left | Center | Right |\n");
        assert!(!r.render().contains("| Left |"));

        r.push("|:---|:---:|---:|\n");
        let result = r.render();
        assert!(
            result.contains("| Left | Center | Right |"),
            "got: {result}"
        );
        assert!(result.contains("|:---|:---:|---:|"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_not_hold_data_rows_after_confirmed_separator() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A | B |\n|---|---|\n");
        assert!(r.render().contains("|---|---|"));

        r.push("| 1 | 2 |\n");
        let result = r.render();
        assert!(result.contains("| 1 | 2 |"), "got: {result}");
    }

    #[test]
    fn streaming_renderer_should_handle_multiple_push_calls_before_single_render() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("| A ");
        r.push("| B |\n");
        r.push("|---|---|\n");
        r.push("| 1 | 2 |\n");
        let result = r.render();
        assert!(result.contains("| A | B |"), "got: {result}");
        assert!(result.contains("|---|---|"), "got: {result}");
        assert!(result.contains("| 1 | 2 |"), "got: {result}");
    }

    // ---------- getCommittableText (5 portable upstream cases) ----------

    #[test]
    fn get_committable_text_should_hold_back_table_rows() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        let committable = r.get_committable_text();
        assert!(!committable.contains("| A | B |"), "got: {committable}");
        assert!(committable.contains("Text"), "got: {committable}");
    }

    #[test]
    fn get_committable_text_should_wrap_confirmed_table_in_code_fence() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n|---|---|\n| 1 | 2 |\n");
        let committable = r.get_committable_text();
        assert!(committable.contains("```"), "got: {committable}");
        assert!(committable.contains("| A | B |"), "got: {committable}");
        assert!(committable.contains("| 1 | 2 |"), "got: {committable}");
        assert!(committable.contains("Text"), "got: {committable}");
    }

    #[test]
    fn get_committable_text_should_not_buffer_inside_code_fence() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("```\n| A |\n");
        assert!(r.get_committable_text().contains("| A |"));
    }

    #[test]
    fn get_committable_text_should_return_full_text_after_finish() {
        let mut r = StreamingMarkdownRenderer::default();
        r.push("Text\n\n| A | B |\n");
        assert!(!r.get_committable_text().contains("| A | B |"));
        r.finish();
        assert!(r.get_committable_text().contains("| A | B |"));
    }

    // ---------- append-only delta simulation (6 portable cases) ----------
    // 1:1 with upstream's `simulateAppendStream` helper + the
    // table/wrap-toggle/monotonic/end-of-stream cases. Inline-marker
    // healing cases (Hello **wor / unclosed-bold / etc.) depend on
    // remend and are js-only-documented at the module header.

    /// 1:1 with upstream's `simulateAppendStream(chunks, options?)`
    /// test helper. Returns `(appended_text, deltas, final_text)`.
    fn simulate_append_stream(
        chunks: &[&str],
        options: Option<StreamingMarkdownRendererOptions>,
    ) -> (String, Vec<String>, String) {
        let mut r = StreamingMarkdownRenderer::new(options.unwrap_or_default());
        let mut last_appended = String::new();
        let mut deltas: Vec<String> = Vec::new();

        for chunk in chunks {
            r.push(chunk);
            let committable = r.get_committable_text();
            let delta = committable[last_appended.len()..].to_string();
            if !delta.is_empty() {
                deltas.push(delta);
                last_appended = committable;
            }
        }

        r.finish();
        let final_committable = r.get_committable_text();
        let final_delta = final_committable[last_appended.len()..].to_string();
        if !final_delta.is_empty() {
            deltas.push(final_delta);
        }

        let final_text = r.get_text().to_string();
        let appended_text = deltas.join("");
        (appended_text, deltas, final_text)
    }

    #[test]
    fn append_only_plain_text_streams_without_modification() {
        let (appended, _, _) = simulate_append_stream(&["Hello ", "World", "!\n"], None);
        assert_eq!(appended, "Hello World!\n");
    }

    #[test]
    fn append_only_table_is_wrapped_in_code_fence() {
        let (appended, _, _) = simulate_append_stream(
            &[
                "Intro\n\n",
                "| A | B |\n",
                "|---|---|\n",
                "| 1 | 2 |\n",
                "| 3 | 4 |\n",
                "\nAfter table\n",
            ],
            None,
        );
        assert!(appended.contains("```\n| A | B |"), "got: {appended}");
        assert!(appended.contains("| 1 | 2 |"));
        assert!(appended.contains("| 3 | 4 |"));
        assert!(appended.contains("```\n\nAfter table"));
        // Intro should appear before the code fence.
        assert!(appended.find("Intro").unwrap() < appended.find("```").unwrap());
    }

    #[test]
    fn append_only_table_can_stream_without_code_fence_when_wrapping_disabled() {
        let (appended, _, _) = simulate_append_stream(
            &[
                "Intro\n\n",
                "| A | B |\n",
                "|---|---|\n",
                "| 1 | 2 |\n",
                "| 3 | 4 |\n",
                "\nAfter table\n",
            ],
            Some(StreamingMarkdownRendererOptions {
                wrap_tables_for_append: false,
            }),
        );
        assert!(appended.contains("| A | B |"));
        assert!(appended.contains("| 1 | 2 |"));
        assert!(appended.contains("| 3 | 4 |"));
        assert!(appended.contains("After table"));
        assert!(!appended.contains("```"));
    }

    #[test]
    fn append_only_table_at_end_of_stream_is_flushed_on_finish() {
        let (appended, deltas, _) = simulate_append_stream(
            &["Text\n\n", "| A | B |\n", "|---|---|\n", "| 1 | 2 |\n"],
            None,
        );
        assert!(appended.contains("| A | B |"));
        assert!(appended.contains("```"));
        assert!(!deltas.is_empty());
    }

    #[test]
    fn append_only_concatenated_deltas_are_monotonic() {
        // Core invariant: every committable is a prefix-extension of
        // the previous one.
        let mut r = StreamingMarkdownRenderer::default();
        let mut last_appended = String::new();
        let mut deltas: Vec<String> = Vec::new();
        let chunks = [
            "Hello world\n",
            "\n",
            "| A | B |\n",
            "| - | - |\n",
            "| 1 | 2 |\n",
            "\nDone\n",
        ];
        for chunk in chunks {
            r.push(chunk);
            let committable = r.get_committable_text();
            assert!(
                committable.starts_with(&last_appended),
                "monotonicity broken: committable={committable:?} last_appended={last_appended:?}"
            );
            let delta = committable[last_appended.len()..].to_string();
            if !delta.is_empty() {
                deltas.push(delta);
                last_appended = committable;
            }
        }
        r.finish();
        let final_committable = r.get_committable_text();
        assert!(final_committable.starts_with(&last_appended));
        let final_delta = final_committable[last_appended.len()..].to_string();
        if !final_delta.is_empty() {
            deltas.push(final_delta);
        }
        assert_eq!(deltas.join(""), final_committable);
    }

    #[test]
    fn append_only_real_world_table_streams_correctly() {
        let header = "| ID | Name | Department | Age | Salary | City | Join Date |\n";
        let sep = "| - | - | - | - | - | - | - |\n";
        let rows = [
            "| 1 | Alice Johnson | Engineering | 28 | $75,000 | New York | 2021-03-15 |\n",
            "| 2 | Bob Smith | Marketing | 35 | $68,000 | Los Angeles | 2019-07-22 |\n",
            "| 3 | Carol Davis | Finance | 31 | $82,000 | Chicago | 2021-01-10 |\n",
        ];
        let mut chunks: Vec<&str> = vec!["Here's a table:\n\n", header, sep];
        chunks.extend(rows.iter().copied());

        let (appended, _, final_text) = simulate_append_stream(&chunks, None);
        assert!(appended.contains("Alice Johnson"), "got: {appended}");
        assert!(appended.contains("Bob Smith"));
        assert!(appended.contains("Carol Davis"));
        assert!(appended.contains("```"));
        assert!(appended.contains("Join Date"));
        assert!(!appended.contains("JoinJoin"));
        assert!(final_text.contains("Alice Johnson"));
        assert!(final_text.contains("| 3 |"));
    }

    #[test]
    fn streaming_renderer_should_render_real_world_table_with_single_dash_separators_progressively()
    {
        let mut r = StreamingMarkdownRenderer::default();

        r.push("Here's a table with 20 rows of sample data:\n\n");
        assert!(r.render().contains("Here's a table"));

        r.push("| ID | Name | Department | Age | Salary | City | Join Date | Status |\n");
        let result = r.render();
        assert!(!result.contains("| ID |"), "got: {result}");
        assert!(result.contains("Here's a table"));

        r.push("| - | - | - | - | - | - | - | - |\n");
        let result = r.render();
        assert!(result.contains("| ID |"), "got: {result}");
        assert!(result.contains("| - |"), "got: {result}");

        r.push(
            "| 1 | Sarah Johnson | Engineering | 32 | $95,000 | Seattle | 2019-03-15 | Active |\n",
        );
        let result = r.render();
        assert!(result.contains("Sarah Johnson"), "got: {result}");

        r.push("| 2 | Michael");
        let result = r.render();
        // Complete rows still visible, partial line excluded from
        // table detection.
        assert!(result.contains("Sarah Johnson"), "got: {result}");

        r.push(" Chen | Marketing | 28 | $72,000 | Austin | 2020-07-22 | Active |\n");
        let result = r.render();
        assert!(result.contains("Michael Chen"), "got: {result}");
    }
}
