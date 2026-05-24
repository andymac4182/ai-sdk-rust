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
    //! `streaming-markdown.test.ts`. The 13 upstream tests that
    //! exercise `remend`-specific inline-marker healing are
    //! js-only-documented at the module header (no `remend`
    //! equivalent in the Rust workspace).
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
}
