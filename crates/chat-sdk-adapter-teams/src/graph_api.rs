//! Microsoft Graph API helpers for the Teams reader path.
//!
//! Partial 1:1 port of `packages/adapter-teams/src/graph-api.ts`.
//! The full `TeamsGraphReader` class is an HTTP-driven object (the
//! `chats.messages.list` / `teams.channels.messages.list` graph
//! endpoints are paginated and stateful), which lives behind a
//! `vi.fn()`-style mock infrastructure upstream and is enumerated as
//! js-only-documented per the slice 411 cross-cutting pattern.
//!
//! The three describe blocks that exercise *pure* shape-helpers
//! (`extractTextFromGraphMessage`, `extractCardTitle`,
//! `chatIdFromContext`) port 1:1 to the module-scope `pub fn`s
//! below — same pattern as
//! [`crate::parse::parse_teams_message`].
//!
//! The Graph API context is a tagged union upstream:
//!
//! ```text
//! type TeamsGraphContext =
//!   | { type: "dm"; graphChatId: string }
//!   | { type: "channel"; teamId: string; channelId: string };
//! ```
//!
//! The Rust port models this as [`TeamsGraphContext`] with two
//! variants. `None` (no context) means "no Graph metadata cached for
//! this conversation" — the helpers handle the `None` case.

use serde_json::Value;

/// Tagged union for Graph-context shape. 1:1 port of the upstream
/// `TeamsGraphContext` discriminator (`{ type: "dm" ... }` vs
/// `{ type: "channel" ... }`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TeamsGraphContext {
    /// Direct message context. Upstream stores the Graph API chat id
    /// (a synthesized id Teams uses for DM conversations) separately
    /// from the Bot Framework conversation id.
    Dm {
        /// Graph API chat id (e.g.
        /// `19:user-aad-id_bot-id@unq.gbl.spaces`).
        graph_chat_id: String,
    },
    /// Team channel context. Upstream stores the team id + channel
    /// id pair so the channel-level Graph endpoints can be called.
    Channel {
        /// Team id (`groupId` in the Graph API).
        team_id: String,
        /// Channel id (`channelId` in the Graph API).
        channel_id: String,
    },
}

/// Resolve the Graph API chat ID for a non-channel conversation.
/// 1:1 with upstream private `chatIdFromContext(context,
/// baseConversationId)`:
///
/// - When `context` is a DM, return `context.graphChatId`.
/// - Otherwise (no context, or channel context), return the raw
///   `base_conversation_id` (works for group chats and channels
///   where the conversation id is the right chat id).
pub fn chat_id_from_context(
    context: Option<&TeamsGraphContext>,
    base_conversation_id: &str,
) -> String {
    match context {
        Some(TeamsGraphContext::Dm { graph_chat_id }) => graph_chat_id.clone(),
        _ => base_conversation_id.to_string(),
    }
}

/// Find a heuristic "title" TextBlock inside an Adaptive Card body.
/// 1:1 with upstream `extractCardTitle(card)`:
///
/// - Returns `None` for `null`/`undefined`, non-objects, missing
///   `body`, or empty `body` arrays.
/// - First pass: returns the `text` of the first `TextBlock` with
///   `weight === "bolder"` OR `size === "large"` OR
///   `size === "extraLarge"`.
/// - Fallback: returns the `text` of the first `TextBlock`
///   irrespective of weight/size.
/// - Returns `None` when no `TextBlock` has a string `text`.
pub fn extract_card_title(card: &Value) -> Option<String> {
    if !card.is_object() {
        return None;
    }
    let body = card.get("body").and_then(Value::as_array)?;
    if body.is_empty() {
        return None;
    }

    // First pass — styled title.
    for element in body {
        if element.get("type").and_then(Value::as_str) != Some("TextBlock") {
            continue;
        }
        let weight = element.get("weight").and_then(Value::as_str);
        let size = element.get("size").and_then(Value::as_str);
        let is_styled =
            weight == Some("bolder") || size == Some("large") || size == Some("extraLarge");
        if is_styled {
            if let Some(text) = element.get("text").and_then(Value::as_str) {
                return Some(text.to_string());
            }
        }
    }

    // Fallback — first TextBlock.
    for element in body {
        if element.get("type").and_then(Value::as_str) != Some("TextBlock") {
            continue;
        }
        if let Some(text) = element.get("text").and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    None
}

/// Extract a plain-text representation of a Graph API chat message.
/// 1:1 with upstream `extractTextFromGraphMessage(msg)`:
///
/// - `body.contentType === "text"` -> return `body.content || ""`.
/// - HTML body: strip every `<...>` tag pair via a single-pass byte
///   scanner (no regex), then trim.
/// - When stripped HTML is empty AND the message has an adaptive
///   card attachment, parse the `content` JSON and:
///   - return `extract_card_title(card)` if found, else
///   - return `"[Card]"` (also returned on JSON parse failure).
/// - Otherwise return the trimmed (HTML-stripped) text.
pub fn extract_text_from_graph_message(msg: &Value) -> String {
    let body = msg.get("body");
    let content_type = body
        .and_then(|b| b.get("contentType"))
        .and_then(Value::as_str);
    let content = body
        .and_then(|b| b.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");

    if content_type == Some("text") {
        return content.to_string();
    }

    let text = strip_html_tags(content).trim().to_string();
    if !text.is_empty() {
        return text;
    }

    if let Some(atts) = msg.get("attachments").and_then(Value::as_array) {
        for att in atts {
            if att.get("contentType").and_then(Value::as_str)
                == Some("application/vnd.microsoft.card.adaptive")
            {
                let raw_content = att.get("content").and_then(Value::as_str).unwrap_or("{}");
                match serde_json::from_str::<Value>(raw_content) {
                    Ok(card) => {
                        if let Some(title) = extract_card_title(&card) {
                            return title;
                        }
                        return "[Card]".to_string();
                    }
                    Err(_) => return "[Card]".to_string(),
                }
            }
        }
    }

    text
}

/// Single-pass byte scanner that removes every `<...>` HTML tag.
/// Mirrors upstream's inline loop in `extractTextFromGraphMessage`.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================================================================
    // describe("extractTextFromGraphMessage") — 6 upstream cases.
    // ==================================================================

    // 1:1 with upstream graph-api.test.ts:18 > "should extract plain text content".
    #[test]
    fn extract_text_from_graph_message_should_extract_plain_text_content() {
        let msg = serde_json::json!({
            "id": "1",
            "body": { "content": "Hello world", "contentType": "text" },
        });
        assert_eq!(extract_text_from_graph_message(&msg), "Hello world");
    }

    // 1:1 with upstream graph-api.test.ts:29 > "should strip HTML tags from html content".
    #[test]
    fn extract_text_from_graph_message_should_strip_html_tags_from_html_content() {
        let msg = serde_json::json!({
            "id": "1",
            "body": { "content": "<p>Hello <b>world</b></p>", "contentType": "html" },
        });
        assert_eq!(extract_text_from_graph_message(&msg), "Hello world");
    }

    // 1:1 with upstream graph-api.test.ts:43 > "should return empty string for missing body".
    #[test]
    fn extract_text_from_graph_message_should_return_empty_string_for_missing_body() {
        let msg = serde_json::json!({ "id": "1" });
        assert_eq!(extract_text_from_graph_message(&msg), "");
    }

    // 1:1 with upstream graph-api.test.ts:49 > "should return '[Card]' for adaptive card without title".
    #[test]
    fn extract_text_from_graph_message_should_return_card_for_adaptive_card_without_title() {
        let card = serde_json::to_string(&serde_json::json!({
            "type": "AdaptiveCard", "body": []
        }))
        .unwrap();
        let msg = serde_json::json!({
            "id": "1",
            "body": { "content": "", "contentType": "html" },
            "attachments": [
                {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": card
                }
            ],
        });
        assert_eq!(extract_text_from_graph_message(&msg), "[Card]");
    }

    // 1:1 with upstream graph-api.test.ts:64 > "should extract card title from bolder TextBlock".
    #[test]
    fn extract_text_from_graph_message_should_extract_card_title_from_bolder_text_block() {
        let card = serde_json::to_string(&serde_json::json!({
            "type": "AdaptiveCard",
            "body": [
                { "type": "TextBlock", "text": "My Card Title", "weight": "bolder" },
                { "type": "TextBlock", "text": "Some description" }
            ]
        }))
        .unwrap();
        let msg = serde_json::json!({
            "id": "1",
            "body": { "content": "", "contentType": "html" },
            "attachments": [
                {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": card
                }
            ],
        });
        assert_eq!(extract_text_from_graph_message(&msg), "My Card Title");
    }

    // 1:1 with upstream graph-api.test.ts:87 > "should return '[Card]' for invalid JSON in card content".
    #[test]
    fn extract_text_from_graph_message_should_return_card_for_invalid_json() {
        let msg = serde_json::json!({
            "id": "1",
            "body": { "content": "", "contentType": "html" },
            "attachments": [
                {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": "not valid json"
                }
            ],
        });
        assert_eq!(extract_text_from_graph_message(&msg), "[Card]");
    }

    // ==================================================================
    // describe("extractCardTitle") — 6 upstream cases.
    // ==================================================================

    // 1:1 with upstream graph-api.test.ts:104 > "should return null for null/undefined".
    #[test]
    fn extract_card_title_should_return_null_for_null_undefined() {
        assert!(extract_card_title(&Value::Null).is_none());
        // serde_json has no `undefined`; the Value::Null case covers both.
    }

    // 1:1 with upstream graph-api.test.ts:110 > "should return null for non-object values".
    #[test]
    fn extract_card_title_should_return_null_for_non_object_values() {
        assert!(extract_card_title(&Value::String("string".into())).is_none());
        assert!(extract_card_title(&serde_json::json!(42)).is_none());
    }

    // 1:1 with upstream graph-api.test.ts:116 > "should return null for empty body".
    #[test]
    fn extract_card_title_should_return_null_for_empty_body() {
        assert!(extract_card_title(&serde_json::json!({ "body": [] })).is_none());
    }

    // 1:1 with upstream graph-api.test.ts:121 > "should find title with weight: bolder".
    #[test]
    fn extract_card_title_should_find_title_with_weight_bolder() {
        let card = serde_json::json!({
            "body": [
                { "type": "TextBlock", "text": "Title", "weight": "bolder" },
                { "type": "TextBlock", "text": "Description" }
            ]
        });
        assert_eq!(extract_card_title(&card).as_deref(), Some("Title"));
    }

    // 1:1 with upstream graph-api.test.ts:132 > "should find title with size: large".
    #[test]
    fn extract_card_title_should_find_title_with_size_large() {
        let card = serde_json::json!({
            "body": [
                { "type": "TextBlock", "text": "Big Title", "size": "large" },
                { "type": "TextBlock", "text": "Description" }
            ]
        });
        assert_eq!(extract_card_title(&card).as_deref(), Some("Big Title"));
    }

    // 1:1 with upstream graph-api.test.ts:143 > "should fallback to first TextBlock when no styled title found".
    #[test]
    fn extract_card_title_should_fallback_to_first_text_block_when_no_styled_title_found() {
        let card = serde_json::json!({
            "body": [
                { "type": "TextBlock", "text": "First block" },
                { "type": "TextBlock", "text": "Second block" }
            ]
        });
        assert_eq!(extract_card_title(&card).as_deref(), Some("First block"));
    }

    // ==================================================================
    // describe("chatIdFromContext") — 3 upstream cases.
    // ==================================================================

    // 1:1 with upstream graph-api.test.ts:156 > "should use graphChatId from DM context".
    #[test]
    fn chat_id_from_context_should_use_graph_chat_id_from_dm_context() {
        let ctx = TeamsGraphContext::Dm {
            graph_chat_id: "19:user-aad-id_bot-id@unq.gbl.spaces".to_string(),
        };
        assert_eq!(
            chat_id_from_context(Some(&ctx), "a:opaque-conversation-id"),
            "19:user-aad-id_bot-id@unq.gbl.spaces"
        );
    }

    // 1:1 with upstream graph-api.test.ts:166 > "should use raw conversation ID when no context".
    #[test]
    fn chat_id_from_context_should_use_raw_conversation_id_when_no_context() {
        assert_eq!(
            chat_id_from_context(None, "19:group-chat@thread.v2"),
            "19:group-chat@thread.v2"
        );
    }

    // 1:1 with upstream graph-api.test.ts:176 > "should use raw conversation ID for channel context".
    #[test]
    fn chat_id_from_context_should_use_raw_conversation_id_for_channel_context() {
        let ctx = TeamsGraphContext::Channel {
            team_id: "team-id".to_string(),
            channel_id: "channel-id".to_string(),
        };
        assert_eq!(
            chat_id_from_context(Some(&ctx), "19:channel@thread.tacv2"),
            "19:channel@thread.tacv2"
        );
    }
}
