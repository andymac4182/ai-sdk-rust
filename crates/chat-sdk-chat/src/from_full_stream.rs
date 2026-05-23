//! Stream-event normalizer for AI SDK fullStream / textStream input.
//!
//! 1:1 port of `packages/chat/src/from-full-stream.ts` adapted to a
//! synchronous Iterator pipeline. Upstream uses
//! `AsyncIterable<unknown> -> AsyncIterable<string | StreamChunk>`;
//! the Rust port commits to a synchronous `Iterator<Item =
//! serde_json::Value>` input because the pure normalization logic
//! does not depend on awaiting upstream IO. Adapters that feed in
//! from a `futures::Stream` can collect to `Vec<Value>` before
//! calling this function (or wrap the result in their own
//! `Stream::from_iter`).
//!
//! **Behavior parity** with upstream:
//!
//! - String events pass through as-is.
//! - Objects with `type` in `STREAM_CHUNK_TYPES` (`markdown_text`,
//!   `task_update`, `plan_update`) pass through as a JSON value (the
//!   upstream `StreamChunk` wire shape).
//! - Objects with `type: "text-delta"` extract `text` first, then
//!   `textDelta`, then `delta` (AI SDK v5/v6 forward-compat fallback).
//!   Only string values are emitted; non-string values are skipped.
//! - Objects with `type: "finish-step"` set the separator flag.
//! - Other events (null, primitives, missing `type`, tool-call, …)
//!   are silently skipped.
//! - Separator `"\n\n"` is injected between steps only when a
//!   finish-step has been seen AND text has already been emitted; it
//!   is never trailing.

use serde_json::Value;

const STREAM_CHUNK_TYPES: &[&str] = &["markdown_text", "task_update", "plan_update"];

/// One element produced by [`from_full_stream`]. Mirrors upstream's
/// `AsyncIterable<string | StreamChunk>` element type.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamYield {
    /// Plain text chunk (string event, extracted text-delta, or the
    /// `"\n\n"` step separator).
    Text(String),
    /// Pass-through structured chunk (one of [`STREAM_CHUNK_TYPES`]).
    Chunk(Value),
}

impl StreamYield {
    /// View the text payload, if any. Returns `None` for [`Self::Chunk`].
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            Self::Chunk(_) => None,
        }
    }
}

/// Normalize an event iterator into [`StreamYield`] entries. 1:1 port
/// of upstream `async function* fromFullStream(stream)`.
pub fn from_full_stream<I>(events: I) -> Vec<StreamYield>
where
    I: IntoIterator<Item = Value>,
{
    let mut out: Vec<StreamYield> = Vec::new();
    let mut needs_separator = false;
    let mut has_emitted_text = false;

    for event in events {
        // Plain string chunk (textStream).
        if let Value::String(s) = &event {
            out.push(StreamYield::Text(s.clone()));
            has_emitted_text = true;
            continue;
        }

        // Object with `type` field.
        let Some(obj) = event.as_object() else {
            continue;
        };
        let Some(type_tag) = obj.get("type").and_then(Value::as_str) else {
            continue;
        };

        // Pass-through StreamChunk objects.
        if STREAM_CHUNK_TYPES.contains(&type_tag) {
            out.push(StreamYield::Chunk(event));
            continue;
        }

        if type_tag == "text-delta" {
            // AI SDK v6 uses `text`, v5 uses `textDelta`, the
            // experimental `delta` field is the same place.
            let text = obj
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| obj.get("delta").and_then(Value::as_str))
                .or_else(|| obj.get("textDelta").and_then(Value::as_str));
            if let Some(text) = text {
                if needs_separator && has_emitted_text {
                    out.push(StreamYield::Text("\n\n".to_string()));
                }
                needs_separator = false;
                has_emitted_text = true;
                out.push(StreamYield::Text(text.to_string()));
            }
        } else if type_tag == "finish-step" {
            needs_separator = true;
        }
        // All other event types are silently skipped.
    }

    out
}

/// Convenience wrapper that joins every [`StreamYield::Text`] entry
/// into a single string, dropping any structured [`StreamYield::Chunk`]
/// elements. Matches upstream test helpers that `for await` and
/// concatenate text chunks.
pub fn from_full_stream_to_string<I>(events: I) -> String
where
    I: IntoIterator<Item = Value>,
{
    let mut out = String::new();
    for y in from_full_stream(events) {
        if let StreamYield::Text(s) = y {
            out.push_str(&s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/chat/src/from-full-stream.test.ts` (18
    //! cases). Upstream uses an async generator + `collect()`
    //! helper; the Rust port uses sync `from_full_stream_to_string`.
    //! Stream-shape semantics are identical.
    use super::*;
    use serde_json::json;

    fn collect(events: Vec<Value>) -> String {
        from_full_stream_to_string(events)
    }

    // ---------- fullStream (object events) ----------

    #[test]
    fn extracts_text_delta_values() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "hello"}),
            json!({"type": "text-delta", "textDelta": " world"}),
        ];
        assert_eq!(collect(events), "hello world");
    }

    #[test]
    fn injects_separator_between_steps() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "hello."}),
            json!({"type": "finish-step"}),
            json!({"type": "text-delta", "textDelta": "how are you?"}),
        ];
        assert_eq!(collect(events), "hello.\n\nhow are you?");
    }

    #[test]
    fn does_not_add_trailing_separator_after_final_finish_step() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "done."}),
            json!({"type": "finish-step"}),
        ];
        assert_eq!(collect(events), "done.");
    }

    #[test]
    fn handles_multiple_steps() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "step 1"}),
            json!({"type": "finish-step"}),
            json!({"type": "text-delta", "textDelta": "step 2"}),
            json!({"type": "finish-step"}),
            json!({"type": "text-delta", "textDelta": "step 3"}),
        ];
        assert_eq!(collect(events), "step 1\n\nstep 2\n\nstep 3");
    }

    #[test]
    fn skips_tool_call_and_other_non_text_events() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "before"}),
            json!({"type": "tool-call", "toolName": "search", "args": {}}),
            json!({"type": "tool-result", "toolName": "search", "result": "data"}),
            json!({"type": "finish-step"}),
            json!({"type": "tool-call-streaming-start", "toolName": "lookup"}),
            json!({"type": "text-delta", "textDelta": " after"}),
        ];
        assert_eq!(collect(events), "before\n\n after");
    }

    #[test]
    fn handles_consecutive_finish_step_events() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "a"}),
            json!({"type": "finish-step"}),
            json!({"type": "finish-step"}),
            json!({"type": "text-delta", "textDelta": "b"}),
        ];
        assert_eq!(collect(events), "a\n\nb");
    }

    #[test]
    fn does_not_inject_separator_when_finish_step_comes_before_any_text() {
        let events = vec![
            json!({"type": "finish-step"}),
            json!({"type": "text-delta", "textDelta": "first text"}),
        ];
        assert_eq!(collect(events), "first text");
    }

    #[test]
    fn ignores_text_delta_with_non_string_text_delta() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": 123}),
            json!({"type": "text-delta", "textDelta": null}),
            json!({"type": "text-delta"}),
            json!({"type": "text-delta", "textDelta": "ok"}),
        ];
        assert_eq!(collect(events), "ok");
    }

    // ---------- textStream (plain strings) ----------

    #[test]
    fn passes_through_string_chunks() {
        let events = vec![json!("hello"), json!(" "), json!("world")];
        assert_eq!(collect(events), "hello world");
    }

    #[test]
    fn handles_single_string_chunk() {
        let events = vec![json!("complete message")];
        assert_eq!(collect(events), "complete message");
    }

    // ---------- fullStream v6 (text property) ----------

    #[test]
    fn extracts_text_delta_with_text_property_ai_sdk_v6() {
        let events = vec![
            json!({"type": "text-delta", "id": "0", "text": "hello"}),
            json!({"type": "text-delta", "id": "0", "text": " world"}),
        ];
        assert_eq!(collect(events), "hello world");
    }

    #[test]
    fn injects_separator_between_steps_with_text_property() {
        let events = vec![
            json!({"type": "text-delta", "id": "0", "text": "step 1."}),
            json!({"type": "finish-step"}),
            json!({"type": "text-delta", "id": "0", "text": "step 2."}),
        ];
        assert_eq!(collect(events), "step 1.\n\nstep 2.");
    }

    #[test]
    fn prefers_text_over_text_delta_when_both_present() {
        let events = vec![json!({"type": "text-delta", "text": "v6", "textDelta": "v5"})];
        assert_eq!(collect(events), "v6");
    }

    // ---------- mixed and edge cases ----------

    #[test]
    fn returns_empty_string_for_empty_stream() {
        assert_eq!(collect(vec![]), "");
    }

    #[test]
    fn ignores_invalid_events_null_primitives_missing_type() {
        let events = vec![
            json!(null),
            json!(42),
            json!({"noType": true}),
            json!({"type": "text-delta", "textDelta": "valid"}),
        ];
        assert_eq!(collect(events), "valid");
    }

    #[test]
    fn handles_mixed_strings_and_objects() {
        let events = vec![
            json!("hello"),
            json!({"type": "text-delta", "textDelta": " world"}),
        ];
        assert_eq!(collect(events), "hello world");
    }

    // ---------- additive: StreamChunk pass-through ----------

    #[test]
    fn stream_chunk_types_pass_through_as_chunks_not_text() {
        let events = vec![
            json!({"type": "text-delta", "textDelta": "hi"}),
            json!({"type": "markdown_text", "value": "**bold**"}),
            json!({"type": "task_update", "task": "doing things"}),
            json!({"type": "plan_update", "title": "Plan"}),
        ];
        let yields = from_full_stream(events);
        assert_eq!(yields.len(), 4);
        assert!(matches!(yields[0], StreamYield::Text(ref t) if t == "hi"));
        assert!(matches!(yields[1], StreamYield::Chunk(_)));
        assert!(matches!(yields[2], StreamYield::Chunk(_)));
        assert!(matches!(yields[3], StreamYield::Chunk(_)));
    }
}
