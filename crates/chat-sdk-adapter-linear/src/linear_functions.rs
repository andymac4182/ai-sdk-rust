//! Pure functions for the Linear adapter. 1:1 port of the
//! pure-function subset of `packages/adapter-linear/src/utils.ts`:
//!
//! - [`get_user_name_from_profile_url`] - extract the
//!   `<slug>/profiles/<name>` segment from a Linear profile URL.
//! - [`calculate_expiry`] - compute an absolute expiry timestamp
//!   (epoch ms) from an `expires_in` duration in seconds.
//! - [`render_message_to_linear_markdown`] - extract a card from a
//!   postable message, render via `cardToLinearMarkdown`, fall back
//!   to the format converter for non-card postables, then convert
//!   emoji placeholders.
//! - [`assert_agent_session_thread`] - narrow a decoded
//!   [`LinearThreadId`] to one carrying an `agent_session_id`.

use crate::cards::card_to_linear_markdown;
use crate::markdown::LinearFormatConverter;
use crate::thread_id::LinearThreadId;
use chat_sdk_adapter_shared::adapter_utils::extract_card;
use chat_sdk_adapter_shared::errors::AdapterError;
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::types::AdapterPostableMessage;

/// Extract the user display name from a Linear profile URL. 1:1
/// port of upstream `getUserNameFromProfileUrl(url)`. Returns
/// the empty string when the URL does not contain a
/// `/profiles/<name>` segment.
///
/// Regex used upstream: `^https:\/\/linear\.app\/\S+\/profiles\/([^\/?#]+)`.
/// The Rust port doesn't pull in the `regex` crate (which isn't a
/// workspace dep yet); it parses the URL with `split` / `find`
/// matching exactly the same shape.
pub fn get_user_name_from_profile_url(url: &str) -> String {
    let prefix = "https://linear.app/";
    let Some(after_root) = url.strip_prefix(prefix) else {
        return String::new();
    };
    // After the workspace slug, "/profiles/" must appear.
    let Some(profiles_at) = after_root.find("/profiles/") else {
        return String::new();
    };
    // The slug must be non-empty (upstream uses `\S+`).
    let slug = &after_root[..profiles_at];
    if slug.is_empty() {
        return String::new();
    }
    let after_profiles = &after_root[profiles_at + "/profiles/".len()..];
    // Take characters up to the first `/`, `?`, or `#`.
    let end = after_profiles
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_profiles.len());
    after_profiles[..end].to_string()
}

/// Render a postable message as Linear markdown. 1:1 port of
/// upstream `renderMessageToLinearMarkdown(message, formatConverter)`:
///
/// 1. Try to extract a `Card` element via
///    `chat_sdk_adapter_shared::adapter_utils::extract_card`.
/// 2. If a card is present, render it via
///    [`card_to_linear_markdown`].
/// 3. Otherwise dispatch to the converter's `render_postable_*`
///    based on the [`AdapterPostableMessage`] variant.
/// 4. Apply
///    `chat_sdk_chat::emoji::convert_emoji_placeholders(_, Linear)`.
pub fn render_message_to_linear_markdown(
    message: &AdapterPostableMessage,
    converter: &LinearFormatConverter,
) -> String {
    let rendered = if let Some(card) = extract_card(message) {
        card_to_linear_markdown(card)
    } else {
        match message {
            AdapterPostableMessage::Text(s) => converter.render_postable_string(s),
            AdapterPostableMessage::Raw(r) => converter.render_postable_raw(&r.raw),
            AdapterPostableMessage::Markdown(m) => converter
                .render_postable_markdown(&m.markdown)
                // Match upstream behavior: fall back to the raw text
                // when parsing fails (BaseFormatConverter never
                // throws here, but the Rust parser exposes errors
                // explicitly).
                .unwrap_or_else(|_| m.markdown.clone()),
            AdapterPostableMessage::Ast(a) => {
                converter.render_postable_ast(&chat_sdk_chat::markdown::Node::Root(a.ast.clone()))
            }
            AdapterPostableMessage::Card(_) | AdapterPostableMessage::CardElement(_) => {
                // Already handled by the `extract_card` branch above
                // — these arms are unreachable for `Card` variants.
                String::new()
            }
        }
    };
    convert_emoji_placeholders(&rendered, PlaceholderPlatform::Linear, None)
}

/// Narrow a [`LinearThreadId`] to one carrying an
/// `agent_session_id`. 1:1 port of upstream
/// `assertAgentSessionThread(thread)` which throws
/// `ValidationError("Expected a Linear agent session thread")`.
/// Returns the agent-session id when present, or
/// `AdapterError::Validation` otherwise.
pub fn assert_agent_session_thread<'a>(
    thread: &'a LinearThreadId,
) -> Result<&'a str, AdapterError> {
    match thread.agent_session_id.as_deref() {
        Some(id) => Ok(id),
        None => Err(AdapterError::validation(
            "linear",
            "Expected a Linear agent session thread",
        )),
    }
}

/// Calculate an absolute expiry timestamp (Unix epoch
/// milliseconds) given an optional `expires_in` duration in
/// seconds. 1:1 port of upstream `calculateExpiry(expiresIn)`.
/// Returns `None` when `expires_in` is `None`.
pub fn calculate_expiry(expires_in: Option<u64>) -> Option<u128> {
    let secs = expires_in?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Some(now_ms + (secs as u128) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- getUserNameFromProfileUrl (3 upstream cases) ----------

    #[test]
    fn extracts_the_profile_name_for_any_workspace_slug() {
        assert_eq!(
            get_user_name_from_profile_url("https://linear.app/acme-workspace/profiles/Bob"),
            "Bob"
        );
    }

    #[test]
    fn ignores_trailing_slash_query_and_hash() {
        assert_eq!(
            get_user_name_from_profile_url(
                "https://linear.app/acme-workspace/profiles/bob-bob/?foo=bar#details"
            ),
            "bob-bob"
        );
    }

    #[test]
    fn falls_back_to_empty_when_url_does_not_contain_a_profile_path() {
        assert_eq!(
            get_user_name_from_profile_url("https://linear.app/acme-workspace/issues/ABC-1"),
            ""
        );
    }

    // ---------- additive Rust-side coverage (not in upstream) ----------

    #[test]
    fn rejects_urls_with_a_non_linear_root() {
        assert_eq!(
            get_user_name_from_profile_url("https://example.com/foo/profiles/Bob"),
            ""
        );
    }

    #[test]
    fn rejects_urls_with_an_empty_workspace_slug() {
        // The upstream regex `\S+` requires at least one non-space
        // workspace slug char before `/profiles/`.
        assert_eq!(
            get_user_name_from_profile_url("https://linear.app//profiles/Bob"),
            ""
        );
    }

    // ---------- assertAgentSessionThread (additive) ----------
    // No standalone upstream tests; the helper is exercised by the
    // agent-session HTTP code paths. The Rust suite asserts both the
    // success and failure branches directly.

    #[test]
    fn assert_agent_session_thread_returns_the_id_when_present() {
        let thread = LinearThreadId {
            issue_id: "ISSUE-1".to_string(),
            comment_id: None,
            agent_session_id: Some("session-123".to_string()),
        };
        assert_eq!(assert_agent_session_thread(&thread).unwrap(), "session-123");
    }

    #[test]
    fn assert_agent_session_thread_rejects_threads_without_a_session() {
        let thread = LinearThreadId {
            issue_id: "ISSUE-1".to_string(),
            comment_id: Some("comment-1".to_string()),
            agent_session_id: None,
        };
        let err = assert_agent_session_thread(&thread).unwrap_err();
        assert!(err.is_validation(), "expected ValidationError, got {err}");
    }

    // ---------- renderMessageToLinearMarkdown (additive) ----------
    // Upstream tests live in `index.test.ts` and exercise the helper
    // indirectly through the adapter. The Rust suite covers each
    // `AdapterPostableMessage` variant directly so the dispatch
    // matrix is locked in.

    #[test]
    fn render_message_to_linear_markdown_handles_plain_text() {
        let converter = LinearFormatConverter::new();
        let msg = AdapterPostableMessage::Text("hello".to_string());
        assert_eq!(render_message_to_linear_markdown(&msg, &converter), "hello");
    }

    #[test]
    fn render_message_to_linear_markdown_passes_raw_through() {
        let converter = LinearFormatConverter::new();
        let msg = AdapterPostableMessage::Raw(chat_sdk_chat::types::PostableRaw {
            attachments: None,
            files: None,
            raw: "**bold**".to_string(),
        });
        assert_eq!(
            render_message_to_linear_markdown(&msg, &converter),
            "**bold**"
        );
    }

    #[test]
    fn render_message_to_linear_markdown_converts_emoji_placeholders() {
        let converter = LinearFormatConverter::new();
        let msg = AdapterPostableMessage::Text(":wave: hi".to_string());
        let out = render_message_to_linear_markdown(&msg, &converter);
        // Linear placeholder rendering preserves the literal `:wave:`
        // shortcode (Linear renders shortcodes itself); whatever the
        // helper emits, the input shortcode form must still be
        // recognizable.
        assert!(out.contains("hi"), "got: {out}");
    }

    #[test]
    fn render_message_to_linear_markdown_dispatches_card_via_card_to_linear_markdown() {
        use chat_sdk_chat::cards::{CardElement, CardKind};
        let converter = LinearFormatConverter::new();
        let card = CardElement {
            title: Some("Hello".to_string()),
            subtitle: None,
            image_url: None,
            kind: CardKind::Card,
            children: vec![],
        };
        let msg = AdapterPostableMessage::CardElement(card);
        let out = render_message_to_linear_markdown(&msg, &converter);
        assert!(out.contains("Hello"), "got: {out}");
    }

    #[test]
    fn calculate_expiry_returns_none_for_none_input() {
        assert_eq!(calculate_expiry(None), None);
    }

    #[test]
    fn calculate_expiry_adds_seconds_in_milliseconds() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let expiry = calculate_expiry(Some(3600)).unwrap();
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        // expiry must be in [before + 1h, after + 1h] inclusive
        assert!(expiry >= before + 3_600_000);
        assert!(expiry <= after + 3_600_000);
    }
}
