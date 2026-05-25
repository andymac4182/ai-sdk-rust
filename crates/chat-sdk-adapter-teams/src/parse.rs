//! Teams inbound-message parsing.
//!
//! 1:1 port of `TeamsAdapter#parseMessage(raw)` and the supporting
//! `parseTeamsMessage` / `normalizeMentions` / `createAttachment` /
//! `isMessageFromSelf` helpers from
//! `packages/adapter-teams/src/index.ts`.
//!
//! Upstream signature: `parseMessage(raw: unknown): Message<unknown>`.
//! The Rust port takes the raw JSON [`serde_json::Value`] (matching
//! upstream's `unknown`) and returns a cross-platform
//! [`chat_sdk_chat::message::Message`].
//!
//! Exposed as module-scope `pub fn` (rather than only as an adapter
//! method) so the upstream
//! `describe("parseMessage" / "isMessageFromSelf (via parseMessage)" /
//! "normalizeMentions (via parseMessage)")` cases can be exercised
//! without going through HTTP-call dispatch. Same pattern as
//! [`crate::lib::TeamsAdapter::parse_message`] (an inherent-method
//! wrapper) and the discord / whatsapp adapter parse modules.

use chat_sdk_chat::markdown::{Node, root};
use chat_sdk_chat::message::Message;
use chat_sdk_chat::types::{Attachment, AttachmentKind, Author, BotStatus, MessageMetadata};
use serde_json::Value;

use crate::markdown::TeamsFormatConverter;
use crate::thread_id::{TeamsThreadId, encode_thread_id as encode_upstream_thread_id};

/// Classify a Bot Framework attachment's MIME type. 1:1 with
/// upstream `createAttachment` switch: `image/*` -> `image`,
/// `video/*` -> `video`, `audio/*` -> `audio`, else (and undefined)
/// -> `file`.
pub fn teams_attachment_type(content_type: Option<&str>) -> AttachmentKind {
    match content_type {
        Some(c) if c.starts_with("image/") => AttachmentKind::Image,
        Some(c) if c.starts_with("video/") => AttachmentKind::Video,
        Some(c) if c.starts_with("audio/") => AttachmentKind::Audio,
        Some(_) | None => AttachmentKind::File,
    }
}

/// Predicate: is the activity's `from.id` this bot? 1:1 with
/// upstream protected `isMessageFromSelf(activity)`.
///
/// - Returns `false` when `from.id` is missing or empty, or when
///   `app_id` is empty.
/// - Returns `true` when `from.id === app_id`.
/// - Returns `true` when `from.id` ends with `":app_id"` (Teams
///   prefixes bot ids as `28:<appId>` or `29:<appId>` in activities).
pub fn is_message_from_self(activity: &Value, app_id: &str) -> bool {
    let Some(from_id) = activity
        .get("from")
        .and_then(|v| v.get("id"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    if app_id.is_empty() || from_id.is_empty() {
        return false;
    }
    if from_id == app_id {
        return true;
    }
    let suffix = format!(":{app_id}");
    from_id.ends_with(&suffix)
}

/// Normalize raw activity text by trimming leading/trailing
/// whitespace. 1:1 with upstream protected `normalizeMentions(text)`.
///
/// Upstream's helper is a placeholder for richer mention-rewriting;
/// for now it only calls `.trim()`.
pub fn normalize_mentions(text: &str) -> String {
    text.trim().to_string()
}

/// 1:1 with upstream `TeamsAdapter.parseMessage(raw)` -> protected
/// `parseTeamsMessage(activity, threadId)`:
///
/// - Encodes the thread id as the upstream-shape
///   `teams:<b64(conv)>:<b64(serviceUrl)>` via
///   [`crate::thread_id::encode_thread_id`].
/// - `text` is `activity.text || ""`, then
///   [`normalize_mentions`]-trimmed, then both
///   - `Message.text` = `TeamsFormatConverter::extract_plain_text`
///   - `Message.formatted` = `TeamsFormatConverter::to_ast` (coerced
///     into a `Root`).
/// - `author.user_id` / `user_name` / `full_name` default to
///   `"unknown"` when missing (matches upstream `|| "unknown"`).
/// - `author.is_me` = [`is_message_from_self`] result.
/// - `metadata.date_sent` is the activity's `timestamp` (kept as the
///   raw string, matching upstream `new Date(activity.timestamp)`
///   ISO-string round-trip).
/// - `metadata.edited` = `false` (upstream always sets `false` here).
/// - `attachments` filter mirrors upstream exactly: drops the
///   `application/vnd.microsoft.card.adaptive` contentType, drops
///   `text/html` without a `contentUrl`, keeps everything else.
pub fn parse_teams_message(activity: &Value, app_id: &str) -> Message {
    let raw_text = activity.get("text").and_then(Value::as_str).unwrap_or("");
    let normalized = normalize_mentions(raw_text);

    let converter = TeamsFormatConverter::new();
    let text = converter.extract_plain_text(&normalized);

    let formatted_ast = converter.to_ast(&normalized);
    let formatted_root = match formatted_ast {
        Node::Root(r) => r,
        other => root(vec![other]),
    };

    let conversation_id = activity
        .get("conversation")
        .and_then(|v| v.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let service_url = activity
        .get("serviceUrl")
        .and_then(Value::as_str)
        .unwrap_or("");
    let thread_id = encode_upstream_thread_id(&TeamsThreadId {
        conversation_id: conversation_id.to_string(),
        service_url: service_url.to_string(),
    });

    let from = activity.get("from");
    let user_id = from
        .and_then(|f| f.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let user_name = from
        .and_then(|f| f.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let is_me = is_message_from_self(activity, app_id);

    let author = Author {
        user_id,
        user_name: user_name.clone(),
        full_name: user_name,
        // Upstream sets `is_bot: false` always — Teams SDK doesn't
        // expose role directly so the upstream comment is `we check
        // isMe instead`. Preserved here verbatim.
        is_bot: BotStatus::FALSE,
        is_me,
    };

    let metadata = MessageMetadata {
        date_sent: activity
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        edited: false,
        edited_at: None,
    };

    let attachments = activity
        .get("attachments")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(build_attachment_from_activity)
                .collect()
        })
        .unwrap_or_default();

    let id = activity
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    Message::new(
        id,
        thread_id,
        text,
        formatted_root,
        activity.clone(),
        author,
        metadata,
        attachments,
    )
}

/// Internal: build an `Attachment` from a single Bot Framework
/// attachment JSON object. Returns `None` for the two upstream
/// filter cases:
///
/// - `contentType === "application/vnd.microsoft.card.adaptive"`
/// - `contentType === "text/html" && !contentUrl`
fn build_attachment_from_activity(att: &Value) -> Option<Attachment> {
    let content_type = att.get("contentType").and_then(Value::as_str);
    let content_url = att.get("contentUrl").and_then(Value::as_str);

    if content_type == Some("application/vnd.microsoft.card.adaptive") {
        return None;
    }
    if content_type == Some("text/html") && content_url.is_none() {
        return None;
    }

    let kind = teams_attachment_type(content_type);
    let name = att.get("name").and_then(Value::as_str).map(str::to_string);
    let url = content_url.map(str::to_string);
    let mime_type = content_type.map(str::to_string);

    Some(Attachment {
        kind,
        url,
        name,
        mime_type,
        data: None,
        fetch_metadata: None,
        size: None,
        width: None,
        height: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(extra: serde_json::Value) -> Value {
        let mut base = serde_json::json!({
            "type": "message",
            "id": "msg-1",
            "conversation": { "id": "19:abc@thread.tacv2" },
            "serviceUrl": "https://smba.trafficmanager.net/teams/",
        });
        let base_obj = base.as_object_mut().unwrap();
        if let Some(extra_obj) = extra.as_object() {
            for (k, v) in extra_obj {
                base_obj.insert(k.clone(), v.clone());
            }
        }
        base
    }

    // ==================================================================
    // describe("isMessageFromSelf (via parseMessage)") — 4 upstream
    // cases ported as is_message_from_self predicate cases. The
    // parse_teams_message integration in each case mirrors the
    // upstream `parseMessage(...).author.isMe` shape.
    // ==================================================================

    // 1:1 with upstream index.test.ts:329 > "should detect exact match of appId".
    #[test]
    fn is_message_from_self_detects_exact_match_of_app_id() {
        let act = activity(serde_json::json!({
            "id": "msg-1",
            "text": "Hello",
            "from": { "id": "abc123-def456", "name": "Bot" },
        }));
        let msg = parse_teams_message(&act, "abc123-def456");
        assert!(msg.author.is_me);
    }

    // 1:1 with upstream index.test.ts:349 > "should detect Teams-prefixed bot ID (28:appId)".
    #[test]
    fn is_message_from_self_detects_teams_prefixed_bot_id() {
        let act = activity(serde_json::json!({
            "id": "msg-2",
            "text": "Hello",
            "from": { "id": "28:abc123-def456", "name": "Bot" },
        }));
        let msg = parse_teams_message(&act, "abc123-def456");
        assert!(msg.author.is_me);
    }

    // 1:1 with upstream index.test.ts:369 > "should not detect unrelated user as self".
    #[test]
    fn is_message_from_self_does_not_detect_unrelated_user_as_self() {
        let act = activity(serde_json::json!({
            "id": "msg-3",
            "text": "Hello",
            "from": { "id": "user-xyz", "name": "User" },
        }));
        let msg = parse_teams_message(&act, "abc123-def456");
        assert!(!msg.author.is_me);
    }

    // 1:1 with upstream index.test.ts:389 > "should return false when from.id is undefined".
    #[test]
    fn is_message_from_self_returns_false_when_from_id_is_undefined() {
        let act = activity(serde_json::json!({
            "id": "msg-4",
            "text": "Hello",
            "from": { "name": "Unknown" },
        }));
        let msg = parse_teams_message(&act, "abc123");
        assert!(!msg.author.is_me);
    }

    // ==================================================================
    // describe("parseMessage") — 7 upstream cases
    // ==================================================================

    // 1:1 with upstream index.test.ts:415 > "should parse basic text message".
    #[test]
    fn parse_message_should_parse_basic_text_message() {
        let act = activity(serde_json::json!({
            "id": "msg-100",
            "text": "Hello world",
            "from": { "id": "user-1", "name": "Alice", "role": "user" },
            "timestamp": "2024-01-01T00:00:00.000Z",
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert_eq!(msg.id, "msg-100");
        assert!(msg.text.contains("Hello world"));
        assert_eq!(msg.author.user_id, "user-1");
        assert_eq!(msg.author.user_name, "Alice");
        assert!(!msg.author.is_me);
    }

    // 1:1 with upstream index.test.ts:440 > "should handle missing text gracefully".
    #[test]
    fn parse_message_should_handle_missing_text_gracefully() {
        let act = activity(serde_json::json!({
            "id": "msg-102",
            "from": { "id": "user-1", "name": "Alice" },
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert_eq!(msg.text, "");
    }

    // 1:1 with upstream index.test.ts:459 > "should handle missing from fields gracefully".
    #[test]
    fn parse_message_should_handle_missing_from_fields_gracefully() {
        let act = activity(serde_json::json!({
            "id": "msg-103",
            "text": "test",
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert_eq!(msg.author.user_id, "unknown");
        assert_eq!(msg.author.user_name, "unknown");
    }

    // 1:1 with upstream index.test.ts:479 > "should filter out adaptive card attachments".
    #[test]
    fn parse_message_should_filter_out_adaptive_card_attachments() {
        let act = activity(serde_json::json!({
            "id": "msg-104",
            "text": "test",
            "from": { "id": "user-1", "name": "Alice" },
            "attachments": [
                {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": {}
                },
                {
                    "contentType": "image/png",
                    "contentUrl": "https://example.com/image.png",
                    "name": "screenshot.png"
                }
            ],
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(msg.attachments[0].name.as_deref(), Some("screenshot.png"));
    }

    // 1:1 with upstream index.test.ts:512 > "should filter out text/html attachments without contentUrl".
    #[test]
    fn parse_message_should_filter_out_text_html_attachments_without_content_url() {
        let act = activity(serde_json::json!({
            "id": "msg-105",
            "text": "test",
            "from": { "id": "user-1", "name": "Alice" },
            "attachments": [
                {
                    "contentType": "text/html",
                    "content": "<p>Formatted version</p>"
                }
            ],
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert_eq!(msg.attachments.len(), 0);
    }

    // 1:1 with upstream index.test.ts:538 > "should classify attachment types by contentType".
    #[test]
    fn parse_message_should_classify_attachment_types_by_content_type() {
        let act = activity(serde_json::json!({
            "id": "msg-106",
            "text": "test",
            "from": { "id": "user-1", "name": "Alice" },
            "attachments": [
                { "contentType": "image/jpeg", "contentUrl": "https://example.com/photo.jpg", "name": "photo.jpg" },
                { "contentType": "video/mp4", "contentUrl": "https://example.com/video.mp4", "name": "video.mp4" },
                { "contentType": "audio/mpeg", "contentUrl": "https://example.com/audio.mp3", "name": "audio.mp3" },
                { "contentType": "application/pdf", "contentUrl": "https://example.com/doc.pdf", "name": "doc.pdf" }
            ],
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert_eq!(msg.attachments.len(), 4);
        assert_eq!(msg.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(msg.attachments[1].kind, AttachmentKind::Video);
        assert_eq!(msg.attachments[2].kind, AttachmentKind::Audio);
        assert_eq!(msg.attachments[3].kind, AttachmentKind::File);
    }

    // 1:1 with upstream index.test.ts:584 > "should set metadata.edited to false for new messages".
    #[test]
    fn parse_message_should_set_metadata_edited_to_false_for_new_messages() {
        let act = activity(serde_json::json!({
            "id": "msg-107",
            "text": "test",
            "from": { "id": "user-1", "name": "Alice" },
            "timestamp": "2024-06-01T12:00:00Z",
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert!(!msg.metadata.edited);
        assert_eq!(msg.metadata.date_sent, "2024-06-01T12:00:00Z");
    }

    // ==================================================================
    // describe("normalizeMentions (via parseMessage)") — 1 upstream case
    // ==================================================================

    // 1:1 with upstream index.test.ts:614 > "should trim whitespace from text".
    #[test]
    fn normalize_mentions_should_trim_whitespace_from_text() {
        let act = activity(serde_json::json!({
            "id": "msg-200",
            "text": "  Hello world  ",
            "from": { "id": "user-1", "name": "Alice" },
        }));
        let msg = parse_teams_message(&act, "test-app");
        assert!(!msg.text.starts_with(char::is_whitespace));
        assert!(!msg.text.ends_with(char::is_whitespace));
    }
}
