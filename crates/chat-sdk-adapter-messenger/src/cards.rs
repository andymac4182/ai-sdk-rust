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
        CardChild::Link(l) => vec![format!("{}: {}", l.label, l.url)],
        CardChild::Table(t) => render_table(t),
    }
}

/// Render a [`TableElement`] as plain pipe-separated rows. 1:1 with
/// upstream's text-fallback table handling: header row joined by
/// `" | "`, then each body row likewise. No alignment, no separator
/// line - Messenger's text branch is intentionally minimal.
fn render_table(t: &chat_sdk_chat::cards::TableElement) -> Vec<String> {
    let mut lines = Vec::with_capacity(t.rows.len() + 1);
    if !t.headers.is_empty() {
        lines.push(t.headers.join(" | "));
    }
    for row in &t.rows {
        lines.push(row.join(" | "));
    }
    lines
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

    // ---------- text fallback rendering (13 upstream cases) ----------

    use chat_sdk_chat::cards::{
        ActionsKind, ButtonElement, ButtonKind, ButtonStyle, CardKind, DividerElement, DividerKind,
        FieldElement, FieldKind, FieldsKind, ImageElement, ImageKind, LinkButtonElement,
        LinkButtonKind, LinkElement, LinkKind, SectionElement, SectionKind, TableElement,
        TableKind, TextKind,
    };

    fn card(title: Option<&str>, subtitle: Option<&str>, children: Vec<CardChild>) -> CardElement {
        CardElement {
            title: title.map(str::to_owned),
            subtitle: subtitle.map(str::to_owned),
            image_url: None,
            kind: CardKind::Card,
            children,
        }
    }

    fn text_child(content: &str) -> CardChild {
        CardChild::Text(TextElement {
            content: content.to_string(),
            style: None,
            kind: TextKind::Text,
        })
    }

    #[test]
    fn renders_simple_card_with_title() {
        let c = card(Some("Hello World"), None, vec![]);
        assert_eq!(card_to_messenger_text(&c), "Hello World");
    }

    #[test]
    fn renders_card_with_title_and_subtitle() {
        let c = card(Some("Order #1234"), Some("Status update"), vec![]);
        assert_eq!(card_to_messenger_text(&c), "Order #1234\nStatus update");
    }

    #[test]
    fn renders_card_with_text_content() {
        let c = card(
            Some("Notification"),
            None,
            vec![text_child("Your order has been shipped!")],
        );
        assert_eq!(
            card_to_messenger_text(&c),
            "Notification\n\nYour order has been shipped!"
        );
    }

    #[test]
    fn renders_card_with_fields() {
        let c = card(
            Some("Order Details"),
            None,
            vec![CardChild::Fields(FieldsElement {
                children: vec![
                    FieldElement {
                        label: "Order ID".to_string(),
                        value: "12345".to_string(),
                        kind: FieldKind::Field,
                    },
                    FieldElement {
                        label: "Status".to_string(),
                        value: "Shipped".to_string(),
                        kind: FieldKind::Field,
                    },
                ],
                kind: FieldsKind::Fields,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(result.contains("Order ID: 12345"), "got: {result}");
        assert!(result.contains("Status: Shipped"), "got: {result}");
    }

    #[test]
    fn renders_card_with_link_buttons_as_text_with_urls() {
        let c = card(
            Some("Actions"),
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::LinkButton(LinkButtonElement {
                        label: "Track Order".to_string(),
                        style: None,
                        kind: LinkButtonKind::LinkButton,
                        url: "https://example.com/track".to_string(),
                    }),
                    ActionsChild::LinkButton(LinkButtonElement {
                        label: "Get Help".to_string(),
                        style: None,
                        kind: LinkButtonKind::LinkButton,
                        url: "https://example.com/help".to_string(),
                    }),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(
            result.contains("Track Order: https://example.com/track"),
            "got: {result}"
        );
        assert!(
            result.contains("Get Help: https://example.com/help"),
            "got: {result}"
        );
    }

    #[test]
    fn renders_card_with_action_buttons_as_bracketed_text() {
        let c = card(
            Some("Approve?"),
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(ButtonElement {
                        action_type: None,
                        callback_url: None,
                        disabled: None,
                        id: "approve".to_string(),
                        label: "Approve".to_string(),
                        style: Some(ButtonStyle::Primary),
                        kind: ButtonKind::Button,
                        value: None,
                    }),
                    ActionsChild::Button(ButtonElement {
                        action_type: None,
                        callback_url: None,
                        disabled: None,
                        id: "reject".to_string(),
                        label: "Reject".to_string(),
                        style: Some(ButtonStyle::Danger),
                        kind: ButtonKind::Button,
                        value: None,
                    }),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(result.contains("[Approve]"), "got: {result}");
        assert!(result.contains("[Reject]"), "got: {result}");
    }

    #[test]
    fn renders_card_with_inline_image() {
        let c = card(
            Some("Image Card"),
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/image.png".to_string(),
                alt: Some("Example image".to_string()),
                kind: ImageKind::Image,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(
            result.contains("Example image: https://example.com/image.png"),
            "got: {result}"
        );
    }

    #[test]
    fn renders_image_url_without_alt_text() {
        let c = card(
            None,
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/photo.jpg".to_string(),
                alt: None,
                kind: ImageKind::Image,
            })],
        );
        assert_eq!(card_to_messenger_text(&c), "https://example.com/photo.jpg");
    }

    #[test]
    fn renders_card_with_divider() {
        let c = card(
            None,
            None,
            vec![
                text_child("Before"),
                CardChild::Divider(DividerElement {
                    kind: DividerKind::Divider,
                }),
                text_child("After"),
            ],
        );
        let result = card_to_messenger_text(&c);
        assert!(result.contains("---"), "got: {result}");
    }

    #[test]
    fn renders_card_with_section() {
        let c = card(
            None,
            None,
            vec![CardChild::Section(SectionElement {
                children: vec![text_child("Section content")],
                kind: SectionKind::Section,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(result.contains("Section content"), "got: {result}");
    }

    #[test]
    fn renders_card_with_link_element() {
        let c = card(
            None,
            None,
            vec![CardChild::Link(LinkElement {
                url: "https://example.com".to_string(),
                label: "Example Link".to_string(),
                kind: LinkKind::Link,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(
            result.contains("Example Link: https://example.com"),
            "got: {result}"
        );
    }

    #[test]
    fn renders_card_with_table() {
        let c = card(
            None,
            None,
            vec![CardChild::Table(TableElement {
                align: None,
                headers: vec!["Name".to_string(), "Age".to_string()],
                rows: vec![
                    vec!["Alice".to_string(), "30".to_string()],
                    vec!["Bob".to_string(), "25".to_string()],
                ],
                kind: TableKind::Table,
            })],
        );
        let result = card_to_messenger_text(&c);
        assert!(result.contains("Name | Age"), "got: {result}");
        assert!(result.contains("Alice | 30"), "got: {result}");
        assert!(result.contains("Bob | 25"), "got: {result}");
    }

    #[test]
    fn renders_card_image_url_header() {
        let card = CardElement {
            title: Some("Card with Header Image".to_string()),
            subtitle: None,
            image_url: Some("https://example.com/header.png".to_string()),
            kind: CardKind::Card,
            children: vec![],
        };
        let result = card_to_messenger_text(&card);
        assert!(
            result.contains("https://example.com/header.png"),
            "got: {result}"
        );
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
