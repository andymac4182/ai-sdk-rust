//! WhatsApp card text/plain-text renderer + callback-data codec.
//!
//! 1:1 port of the text-fallback and codec subset of
//! `packages/adapter-whatsapp/src/cards.ts`. The interactive-message
//! branch (`cardToWhatsApp` returning an interactive button payload)
//! depends on WhatsApp-specific JSON shapes and is deferred.
//!
//! The functions here render a [`CardElement`] as WhatsApp markdown
//! (`*bold*`, `_italic_`) or plain text, and round-trip
//! `chat:{a, v?}` JSON-in-string callback ids for interactive replies.

use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, CardChild, CardElement, FieldsElement, TextElement, TextStyle,
};

/// Callback-data prefix used to distinguish chat-sdk-encoded
/// payloads from legacy raw strings. 1:1 with upstream
/// `CALLBACK_DATA_PREFIX = "chat:"`.
pub const CALLBACK_DATA_PREFIX: &str = "chat:";

/// Decoded callback payload returned by
/// [`decode_whatsapp_callback_data`]. 1:1 with upstream's
/// `{ actionId, value }` return shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedWhatsAppCallbackData {
    pub action_id: String,
    pub value: Option<String>,
}

/// Encode a callback payload (action_id + optional value) as
/// `chat:{a, v?}` JSON-in-string. 1:1 with upstream
/// `encodeWhatsAppCallbackData(actionId, value?)`. Unlike Telegram,
/// WhatsApp has no hard limit enforced here (reply button IDs are
/// truncated/validated separately when building interactive
/// messages).
pub fn encode_whatsapp_callback_data(action_id: &str, value: Option<&str>) -> String {
    // Build a `BTreeMap`-stable order matching upstream's
    // `JSON.stringify({a, v?})` output ({"a":"...","v":"..."}).
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
/// with upstream `decodeWhatsAppCallbackData(data?)`:
///
/// - `None` / empty -> `{action_id: "whatsapp_callback", value:
///   None}` (the upstream default for "no payload present").
/// - Not starting with `"chat:"` -> legacy passthrough:
///   `{action_id: data, value: Some(data)}`.
/// - Starts with `"chat:"` but JSON is malformed or missing `a`
///   field -> same legacy passthrough.
/// - Well-formed `"chat:{...}"` -> the decoded action id + value.
pub fn decode_whatsapp_callback_data(data: Option<&str>) -> DecodedWhatsAppCallbackData {
    let Some(data) = data.filter(|s| !s.is_empty()) else {
        return DecodedWhatsAppCallbackData {
            action_id: "whatsapp_callback".to_string(),
            value: None,
        };
    };

    let Some(payload_json) = data.strip_prefix(CALLBACK_DATA_PREFIX) else {
        return DecodedWhatsAppCallbackData {
            action_id: data.to_string(),
            value: Some(data.to_string()),
        };
    };

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(payload_json)
        && let Some(a) = parsed.get("a").and_then(|v| v.as_str())
        && !a.is_empty()
    {
        let value = parsed.get("v").and_then(|v| v.as_str()).map(str::to_owned);
        return DecodedWhatsAppCallbackData {
            action_id: a.to_string(),
            value,
        };
    }

    DecodedWhatsAppCallbackData {
        action_id: data.to_string(),
        value: Some(data.to_string()),
    }
}

/// Render a [`CardElement`] as WhatsApp markdown text. 1:1 port of
/// upstream `cardToWhatsAppText(card)` - bold title, plain subtitle,
/// image URL on its own line, children rendered with WhatsApp
/// formatting (`*bold*`, `_italic_`, `[buttonLabel]` etc.).
pub fn card_to_whatsapp_text(card: &CardElement) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        lines.push(format!("*{}*", escape_whatsapp(title)));
    }
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        lines.push(escape_whatsapp(subtitle));
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

/// Plain-text fallback (no markdown formatting). 1:1 port of
/// upstream `cardToPlainText(card)`.
pub fn card_to_plain_text(card: &CardElement) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        parts.push(title.to_string());
    }
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        parts.push(subtitle.to_string());
    }

    for child in &card.children {
        if let Some(text) = child_to_plain_text(child) {
            parts.push(text);
        }
    }

    parts.join("\n")
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
    match t.style {
        Some(TextStyle::Bold) => vec![format!("*{}*", escape_whatsapp(&t.content))],
        Some(TextStyle::Muted) => vec![format!("_{}_", escape_whatsapp(&t.content))],
        _ => vec![escape_whatsapp(&t.content)],
    }
}

fn render_fields(fields: &FieldsElement) -> Vec<String> {
    fields
        .children
        .iter()
        .map(|f| {
            format!(
                "*{}:* {}",
                escape_whatsapp(&f.label),
                escape_whatsapp(&f.value)
            )
        })
        .collect()
}

fn render_actions(actions: &ActionsElement) -> Vec<String> {
    let pieces: Vec<String> = actions
        .children
        .iter()
        .map(|btn| match btn {
            ActionsChild::LinkButton(lb) => format!("{}: {}", escape_whatsapp(&lb.label), lb.url),
            ActionsChild::Button(b) => format!("[{}]", escape_whatsapp(&b.label)),
            ActionsChild::Select(s) => format!("[{}]", escape_whatsapp(&s.label)),
            ActionsChild::RadioSelect(rs) => format!("[{}]", escape_whatsapp(&rs.label)),
        })
        .collect();
    vec![pieces.join(" | ")]
}

fn child_to_plain_text(child: &CardChild) -> Option<String> {
    match child {
        CardChild::Text(t) => Some(t.content.clone()),
        CardChild::Fields(f) => Some(
            f.children
                .iter()
                .map(|fld| format!("{}: {}", fld.label, fld.value))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        CardChild::Actions(_) => None,
        CardChild::Section(s) => {
            let pieces: Vec<String> = s.children.iter().filter_map(child_to_plain_text).collect();
            if pieces.is_empty() {
                None
            } else {
                Some(pieces.join("\n"))
            }
        }
        _ => None,
    }
}

/// Escape WhatsApp markdown-significant characters. 1:1 with
/// upstream `escapeWhatsApp(text)`: backslash, asterisk,
/// underscore, tilde, backtick.
fn escape_whatsapp(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '*' => out.push_str("\\*"),
            '_' => out.push_str("\\_"),
            '~' => out.push_str("\\~"),
            '`' => out.push_str("\\`"),
            other => out.push(other),
        }
    }
    out
}

/// Append `s` as a JSON string literal to `out`, matching
/// `JSON.stringify(s)` for ASCII strings. Used by
/// `encode_whatsapp_callback_data` to mimic JavaScript's exact byte
/// sequence (avoids whitespace insertion that `serde_json::Value`
/// would introduce via `to_string`).
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
    use chat_sdk_chat::cards::{CardKind, FieldElement, FieldKind, FieldsKind, TextKind};

    fn card(title: Option<&str>, subtitle: Option<&str>, children: Vec<CardChild>) -> CardElement {
        CardElement {
            title: title.map(str::to_owned),
            subtitle: subtitle.map(str::to_owned),
            image_url: None,
            kind: CardKind::Card,
            children,
        }
    }

    // ---------- encodeWhatsAppCallbackData (2 cases) ----------

    #[test]
    fn should_encode_action_id_only() {
        let result = encode_whatsapp_callback_data("my_action", None);
        assert_eq!(result, r#"chat:{"a":"my_action"}"#);
    }

    #[test]
    fn should_encode_action_id_and_value() {
        let result = encode_whatsapp_callback_data("my_action", Some("some_value"));
        assert_eq!(result, r#"chat:{"a":"my_action","v":"some_value"}"#);
    }

    // ---------- decodeWhatsAppCallbackData (5 cases) ----------

    #[test]
    fn should_decode_encoded_callback_data() {
        let encoded = encode_whatsapp_callback_data("my_action", Some("some_value"));
        let result = decode_whatsapp_callback_data(Some(&encoded));
        assert_eq!(result.action_id, "my_action");
        assert_eq!(result.value.as_deref(), Some("some_value"));
    }

    #[test]
    fn should_decode_action_id_without_value() {
        let encoded = encode_whatsapp_callback_data("my_action", None);
        let result = decode_whatsapp_callback_data(Some(&encoded));
        assert_eq!(result.action_id, "my_action");
        assert!(result.value.is_none());
    }

    #[test]
    fn should_handle_non_prefixed_data_as_passthrough() {
        let result = decode_whatsapp_callback_data(Some("raw_id"));
        assert_eq!(result.action_id, "raw_id");
        assert_eq!(result.value.as_deref(), Some("raw_id"));
    }

    #[test]
    fn should_handle_undefined_data() {
        let result = decode_whatsapp_callback_data(None);
        assert_eq!(result.action_id, "whatsapp_callback");
        assert!(result.value.is_none());
    }

    #[test]
    fn should_handle_malformed_json_after_prefix() {
        let result = decode_whatsapp_callback_data(Some("chat:not-json"));
        assert_eq!(result.action_id, "chat:not-json");
        assert_eq!(result.value.as_deref(), Some("chat:not-json"));
    }

    // ---------- cardToPlainText (1 case) ----------

    #[test]
    fn should_generate_plain_text_from_card() {
        let c = card(
            Some("Hello"),
            Some("World"),
            vec![
                CardChild::Text(TextElement {
                    content: "Some content".to_string(),
                    style: None,
                    kind: TextKind::Text,
                }),
                CardChild::Fields(FieldsElement {
                    children: vec![FieldElement {
                        label: "Key".to_string(),
                        value: "Value".to_string(),
                        kind: FieldKind::Field,
                    }],
                    kind: FieldsKind::Fields,
                }),
            ],
        );
        let result = card_to_plain_text(&c);
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
        assert!(result.contains("Some content"));
        assert!(result.contains("Key: Value"));
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn card_to_whatsapp_text_uses_single_asterisk_bold_for_title() {
        let c = card(Some("Order"), None, vec![]);
        assert_eq!(card_to_whatsapp_text(&c), "*Order*");
    }

    #[test]
    fn escape_whatsapp_escapes_asterisks_underscores_tildes_backticks() {
        assert_eq!(
            escape_whatsapp(r"hi * _ ~ ` \ test"),
            r"hi \* \_ \~ \` \\ test"
        );
    }
}
