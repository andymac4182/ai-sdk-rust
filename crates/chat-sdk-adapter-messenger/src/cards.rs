//! Messenger card text renderer + callback-data codec.
//!
//! 1:1 port of the text-fallback + codec subset of
//! `packages/adapter-messenger/src/cards.ts`. Template-message
//! rendering (`cardToMessenger` returning a Generic/Button
//! Template payload) depends on Messenger-specific JSON shapes
//! and is deferred to a follow-up slice.
//!
//! Messenger postback callback ids use the same shared
//! `chat:{a, v?}` JSON-in-string convention as the Telegram /
//! WhatsApp ports.

use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, CardChild, CardElement, FieldsElement, TextElement,
};

/// Callback-data prefix used to distinguish chat-sdk-encoded
/// payloads from legacy raw strings. 1:1 with upstream
/// `CALLBACK_DATA_PREFIX = "chat:"`.
pub const CALLBACK_DATA_PREFIX: &str = "chat:";

/// Decoded callback payload returned by
/// [`decode_messenger_callback_data`]. 1:1 with upstream's
/// `{ actionId, value }` return shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedMessengerCallbackData {
    pub action_id: String,
    pub value: Option<String>,
}

/// Encode a callback payload (action_id + optional value) as
/// `chat:{a, v?}` JSON-in-string. 1:1 with upstream
/// `encodeMessengerCallbackData(actionId, value?)`. Mirrors the
/// JavaScript `JSON.stringify` byte sequence exactly (stable key
/// order: `{"a":...,"v":...}`).
pub fn encode_messenger_callback_data(action_id: &str, value: Option<&str>) -> String {
    let mut json = String::from("{\"a\":");
    write_json_string(&mut json, action_id);
    if let Some(v) = value {
        json.push_str(",\"v\":");
        write_json_string(&mut json, v);
    }
    json.push('}');
    format!("{CALLBACK_DATA_PREFIX}{json}")
}

/// Decode a callback payload string into action id + value. 1:1
/// port of upstream `decodeMessengerCallbackData(data?)`:
///
/// - `None` / empty -> `{action_id: "messenger_callback", value:
///   None}`.
/// - Not starting with `"chat:"` -> legacy passthrough:
///   `{action_id: data, value: Some(data)}`.
/// - Starts with `"chat:"` but JSON malformed or missing `a` ->
///   passthrough.
/// - Well-formed `"chat:{...}"` -> decoded action id + value.
pub fn decode_messenger_callback_data(data: Option<&str>) -> DecodedMessengerCallbackData {
    let Some(data) = data.filter(|s| !s.is_empty()) else {
        return DecodedMessengerCallbackData {
            action_id: "messenger_callback".to_string(),
            value: None,
        };
    };

    let Some(payload_json) = data.strip_prefix(CALLBACK_DATA_PREFIX) else {
        return DecodedMessengerCallbackData {
            action_id: data.to_string(),
            value: Some(data.to_string()),
        };
    };

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(payload_json)
        && let Some(a) = parsed.get("a").and_then(|v| v.as_str())
        && !a.is_empty()
    {
        let value = parsed.get("v").and_then(|v| v.as_str()).map(str::to_owned);
        return DecodedMessengerCallbackData {
            action_id: a.to_string(),
            value,
        };
    }

    DecodedMessengerCallbackData {
        action_id: data.to_string(),
        value: Some(data.to_string()),
    }
}

/// Render a [`CardElement`] as Messenger text. 1:1 port of
/// upstream `cardToMessengerText(card)` - plain title (no
/// emphasis), plain subtitle, image URL on its own line,
/// children rendered with `[ButtonLabel]` etc. and no markdown
/// emphasis (Messenger has no inline markdown).
pub fn card_to_messenger_text(card: &CardElement) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        lines.push(title.to_string());
    }
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        lines.push(subtitle.to_string());
    }

    let has_header = card.title.as_deref().filter(|t| !t.is_empty()).is_some()
        || card.subtitle.as_deref().filter(|s| !s.is_empty()).is_some();
    if has_header && !card.children.is_empty() {
        lines.push(String::new());
    }

    if let Some(image_url) = card.image_url.as_deref().filter(|u| !u.is_empty()) {
        lines.push(image_url.to_string());
        lines.push(String::new());
    }

    let last = card.children.len().saturating_sub(1);
    for (i, child) in card.children.iter().enumerate() {
        let child_lines = render_child(child);
        if !child_lines.is_empty() {
            lines.extend(child_lines);
            if i < last {
                lines.push(String::new());
            }
        }
    }

    lines.join("\n")
}

fn render_child(child: &CardChild) -> Vec<String> {
    match child {
        CardChild::Text(t) => render_text(t),
        CardChild::Fields(f) => render_fields(f),
        CardChild::Actions(a) => render_actions(a),
        CardChild::Section(s) => s.children.iter().flat_map(render_child).collect(),
        CardChild::Image(img) => {
            if let Some(alt) = img.alt.as_deref().filter(|a| !a.is_empty()) {
                vec![format!("{}: {}", alt, img.url)]
            } else {
                vec![img.url.clone()]
            }
        }
        CardChild::Divider(_) => vec!["---".to_string()],
        _ => vec![],
    }
}

fn render_text(t: &TextElement) -> Vec<String> {
    // Messenger has no inline markdown; render the text raw.
    vec![t.content.clone()]
}

fn render_fields(fields: &FieldsElement) -> Vec<String> {
    fields
        .children
        .iter()
        .map(|f| format!("{}: {}", f.label, f.value))
        .collect()
}

fn render_actions(actions: &ActionsElement) -> Vec<String> {
    let pieces: Vec<String> = actions
        .children
        .iter()
        .map(|btn| match btn {
            ActionsChild::LinkButton(lb) => format!("{}: {}", lb.label, lb.url),
            ActionsChild::Button(b) => format!("[{}]", b.label),
            ActionsChild::Select(s) => format!("[{}]", s.label),
            ActionsChild::RadioSelect(rs) => format!("[{}]", rs.label),
        })
        .collect();
    vec![pieces.join(" | ")]
}

fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- encodeMessengerCallbackData (3 cases) ----------

    #[test]
    fn encodes_action_id_only() {
        assert_eq!(
            encode_messenger_callback_data("my_action", None),
            r#"chat:{"a":"my_action"}"#
        );
    }

    #[test]
    fn encodes_action_id_and_value() {
        assert_eq!(
            encode_messenger_callback_data("my_action", Some("some_value")),
            r#"chat:{"a":"my_action","v":"some_value"}"#
        );
    }

    #[test]
    fn handles_special_characters_in_action_id() {
        assert_eq!(
            encode_messenger_callback_data("action:with:colons", None),
            r#"chat:{"a":"action:with:colons"}"#
        );
    }

    // ---------- decodeMessengerCallbackData (7 cases) ----------

    #[test]
    fn decodes_encoded_callback_data_with_value() {
        let encoded = encode_messenger_callback_data("my_action", Some("some_value"));
        let decoded = decode_messenger_callback_data(Some(&encoded));
        assert_eq!(decoded.action_id, "my_action");
        assert_eq!(decoded.value.as_deref(), Some("some_value"));
    }

    #[test]
    fn decodes_action_id_without_value() {
        let encoded = encode_messenger_callback_data("my_action", None);
        let decoded = decode_messenger_callback_data(Some(&encoded));
        assert_eq!(decoded.action_id, "my_action");
        assert!(decoded.value.is_none());
    }

    #[test]
    fn handles_non_prefixed_data_as_passthrough_legacy_support() {
        let decoded = decode_messenger_callback_data(Some("raw_payload"));
        assert_eq!(decoded.action_id, "raw_payload");
        assert_eq!(decoded.value.as_deref(), Some("raw_payload"));
    }

    #[test]
    fn handles_undefined_data() {
        let decoded = decode_messenger_callback_data(None);
        assert_eq!(decoded.action_id, "messenger_callback");
        assert!(decoded.value.is_none());
    }

    #[test]
    fn handles_malformed_json_after_prefix() {
        let decoded = decode_messenger_callback_data(Some("chat:not-valid-json"));
        assert_eq!(decoded.action_id, "chat:not-valid-json");
        assert_eq!(decoded.value.as_deref(), Some("chat:not-valid-json"));
    }

    #[test]
    fn handles_empty_string_as_missing_data() {
        let decoded = decode_messenger_callback_data(Some(""));
        assert_eq!(decoded.action_id, "messenger_callback");
        assert!(decoded.value.is_none());
    }

    #[test]
    fn roundtrips_encode_decode() {
        let action_id = "test_action";
        let value = "test_value";
        let encoded = encode_messenger_callback_data(action_id, Some(value));
        let decoded = decode_messenger_callback_data(Some(&encoded));
        assert_eq!(decoded.action_id, action_id);
        assert_eq!(decoded.value.as_deref(), Some(value));
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn card_to_messenger_text_renders_title_and_text() {
        use chat_sdk_chat::cards::{CardKind, TextKind};
        let card = CardElement {
            title: Some("Hello".to_string()),
            subtitle: Some("World".to_string()),
            image_url: None,
            kind: CardKind::Card,
            children: vec![CardChild::Text(TextElement {
                content: "Some content".to_string(),
                style: None,
                kind: TextKind::Text,
            })],
        };
        let result = card_to_messenger_text(&card);
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
        assert!(result.contains("Some content"));
    }
}
