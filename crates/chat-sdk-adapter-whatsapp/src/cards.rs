//! WhatsApp card renderer (text + interactive) + callback-data codec.
//!
//! 1:1 port of `packages/adapter-whatsapp/src/cards.ts`. Renders a
//! [`CardElement`] as WhatsApp markdown text, plain text, or a
//! [`WhatsAppCardResult`] union (interactive button message or text
//! fallback) for the WhatsApp Cloud API.

use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, ButtonElement, CardChild, CardElement, FieldsElement,
    TextElement, TextStyle,
};

/// Maximum number of reply buttons WhatsApp allows. 1:1 with upstream
/// `MAX_REPLY_BUTTONS = 3`.
const MAX_REPLY_BUTTONS: usize = 3;
/// Maximum character length for a WhatsApp button title. 1:1 with
/// upstream `MAX_BUTTON_TITLE_LENGTH = 20`.
const MAX_BUTTON_TITLE_LENGTH: usize = 20;
/// Maximum character length for the WhatsApp body text. 1:1 with
/// upstream `MAX_BODY_LENGTH = 1024`.
const MAX_BODY_LENGTH: usize = 1024;
/// Maximum character length for the WhatsApp header text. 1:1 with
/// upstream's inline `60` literal in the header builder.
const MAX_HEADER_LENGTH: usize = 60;

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

/// Result of converting a [`CardElement`] to a WhatsApp Cloud API
/// payload. 1:1 port of upstream
/// `type WhatsAppCardResult = { type: "interactive", interactive: ... }
/// | { type: "text", text: string }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatsAppCardResult {
    /// Interactive reply-button message. Used when the card has
    /// 1-3 action buttons that fit WhatsApp's constraints.
    Interactive(WhatsAppInteractiveMessage),
    /// Plain text fallback. Used when the card has no action buttons
    /// or only link-buttons (which WhatsApp can't render as replies).
    Text(String),
}

/// WhatsApp Cloud API interactive message shape. 1:1 port of
/// upstream `interface WhatsAppInteractiveMessage`. Only the
/// `"button"` interactive type is produced by this renderer; list
/// and product-list interactive types are out of scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppInteractiveMessage {
    pub interactive_type: String,
    pub header: Option<WhatsAppInteractiveHeader>,
    pub body: WhatsAppInteractiveBody,
    pub action: WhatsAppInteractiveAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppInteractiveHeader {
    pub header_type: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppInteractiveBody {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppInteractiveAction {
    pub buttons: Vec<WhatsAppReplyButton>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppReplyButton {
    pub button_type: String,
    pub reply: WhatsAppReplyButtonReply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatsAppReplyButtonReply {
    pub id: String,
    pub title: String,
}

/// Convert a [`CardElement`] to a WhatsApp message payload. 1:1 port
/// of upstream `cardToWhatsApp(card)`. If the card has 1-3 reply
/// buttons that fit WhatsApp's constraints, returns an interactive
/// button message; otherwise falls back to plain text via
/// [`card_to_whatsapp_text`].
pub fn card_to_whatsapp(card: &CardElement) -> WhatsAppCardResult {
    let actions = find_actions(&card.children);
    let action_buttons = actions.and_then(extract_reply_buttons);

    if let Some(buttons) = action_buttons.filter(|b| !b.is_empty()) {
        let body_text = build_body_text(card);
        let header = card
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|title| WhatsAppInteractiveHeader {
                header_type: "text".to_string(),
                text: truncate_chars(title, MAX_HEADER_LENGTH),
            });
        let body_source = if body_text.is_empty() {
            "Please choose an option".to_string()
        } else {
            body_text
        };
        let body = WhatsAppInteractiveBody {
            text: truncate_chars(&body_source, MAX_BODY_LENGTH),
        };
        let action_buttons: Vec<WhatsAppReplyButton> = buttons
            .into_iter()
            .map(|btn| WhatsAppReplyButton {
                button_type: "reply".to_string(),
                reply: WhatsAppReplyButtonReply {
                    id: encode_whatsapp_callback_data(&btn.id, btn.value.as_deref()),
                    title: truncate_chars(&btn.label, MAX_BUTTON_TITLE_LENGTH),
                },
            })
            .collect();
        return WhatsAppCardResult::Interactive(WhatsAppInteractiveMessage {
            interactive_type: "button".to_string(),
            header,
            body,
            action: WhatsAppInteractiveAction {
                buttons: action_buttons,
            },
        });
    }

    WhatsAppCardResult::Text(card_to_whatsapp_text(card))
}

/// Find the first [`ActionsElement`] in a list of children,
/// recursing into [`CardChild::Section`]s. 1:1 port of upstream
/// `findActions`.
fn find_actions(children: &[CardChild]) -> Option<&ActionsElement> {
    for child in children {
        match child {
            CardChild::Actions(a) => return Some(a),
            CardChild::Section(s) => {
                if let Some(nested) = find_actions(&s.children) {
                    return Some(nested);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract reply buttons from an [`ActionsElement`], limited to the
/// first [`MAX_REPLY_BUTTONS`]. 1:1 port of upstream
/// `extractReplyButtons`: only `"button"` children with a non-empty
/// `id` are kept (link-buttons / select / radio_select are skipped).
fn extract_reply_buttons(actions: &ActionsElement) -> Option<Vec<ButtonElement>> {
    let mut buttons: Vec<ButtonElement> = Vec::new();
    for child in &actions.children {
        if let ActionsChild::Button(b) = child
            && !b.id.is_empty()
        {
            buttons.push(b.clone());
        }
    }
    if buttons.is_empty() {
        return None;
    }
    buttons.truncate(MAX_REPLY_BUTTONS);
    Some(buttons)
}

/// Build interactive body text from card subtitle + non-actions
/// children. 1:1 port of upstream `buildBodyText(card)`.
fn build_body_text(card: &CardElement) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        parts.push(subtitle.to_string());
    }
    for child in &card.children {
        if matches!(child, CardChild::Actions(_)) {
            continue;
        }
        if let Some(text) = child_to_plain_text(child) {
            parts.push(text);
        }
    }
    parts.join("\n")
}

/// Truncate `text` to at most `max_length` chars (counting Unicode
/// scalars to match JS `string.length` semantics for the BMP).
/// Appends `…` (U+2026) as an ellipsis. 1:1 port of upstream
/// `truncate(text, maxLength)`.
fn truncate_chars(text: &str, max_length: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_length {
        return text.to_string();
    }
    let mut out: String = chars[..max_length - 1].iter().collect();
    out.push('\u{2026}');
    out
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
        // Link and Table are intentionally not rendered in WhatsApp's
        // text-fallback mode. 1:1 with upstream `default: return []`
        // in `cards.ts renderChild(child)`.
        CardChild::Link(_) | CardChild::Table(_) => vec![],
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
        // Image / Divider / Link / Table contribute no plain text.
        // 1:1 with upstream `default: return null` in `cards.ts
        // childToPlainText(child)`.
        CardChild::Image(_) | CardChild::Divider(_) | CardChild::Link(_) | CardChild::Table(_) => {
            None
        }
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

    // ---------- cardToWhatsAppText (10 upstream cases) ----------

    use chat_sdk_chat::cards::{
        ActionsKind, ButtonElement, ButtonKind, ButtonStyle, DividerElement, DividerKind,
        ImageElement, ImageKind, LinkButtonElement, LinkButtonKind, SectionElement, SectionKind,
    };

    fn text_child(content: &str, style: Option<TextStyle>) -> CardChild {
        CardChild::Text(TextElement {
            content: content.to_string(),
            style,
            kind: TextKind::Text,
        })
    }

    #[test]
    fn cardtotext_should_render_simple_card_with_title() {
        let c = card(Some("Hello World"), None, vec![]);
        assert_eq!(card_to_whatsapp_text(&c), "*Hello World*");
    }

    #[test]
    fn cardtotext_should_render_card_with_title_and_subtitle() {
        let c = card(Some("Order #1234"), Some("Status update"), vec![]);
        assert_eq!(card_to_whatsapp_text(&c), "*Order #1234*\nStatus update");
    }

    #[test]
    fn cardtotext_should_render_card_with_text_content() {
        let c = card(
            Some("Notification"),
            None,
            vec![text_child("Your order has been shipped!", None)],
        );
        assert_eq!(
            card_to_whatsapp_text(&c),
            "*Notification*\n\nYour order has been shipped!"
        );
    }

    #[test]
    fn cardtotext_should_render_card_with_fields_using_whatsapp_bold() {
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
        let result = card_to_whatsapp_text(&c);
        assert!(result.contains("*Order ID:* 12345"), "got: {result}");
        assert!(result.contains("*Status:* Shipped"), "got: {result}");
    }

    #[test]
    fn cardtotext_should_render_card_with_link_buttons_as_text_with_urls() {
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
        let result = card_to_whatsapp_text(&c);
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
    fn cardtotext_should_render_card_with_action_buttons_as_bracketed_text() {
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
        let result = card_to_whatsapp_text(&c);
        assert!(result.contains("[Approve]"), "got: {result}");
        assert!(result.contains("[Reject]"), "got: {result}");
    }

    #[test]
    fn cardtotext_should_render_card_with_image_url() {
        let c = card(
            Some("Image Card"),
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/image.png".to_string(),
                alt: Some("Example image".to_string()),
                kind: ImageKind::Image,
            })],
        );
        let result = card_to_whatsapp_text(&c);
        assert!(
            result.contains("Example image: https://example.com/image.png"),
            "got: {result}"
        );
    }

    #[test]
    fn cardtotext_should_render_card_with_divider() {
        let c = card(
            None,
            None,
            vec![
                text_child("Before", None),
                CardChild::Divider(DividerElement {
                    kind: DividerKind::Divider,
                }),
                text_child("After", None),
            ],
        );
        let result = card_to_whatsapp_text(&c);
        assert!(result.contains("---"), "got: {result}");
    }

    #[test]
    fn cardtotext_should_render_card_with_section() {
        let c = card(
            None,
            None,
            vec![CardChild::Section(SectionElement {
                children: vec![text_child("Section content", None)],
                kind: SectionKind::Section,
            })],
        );
        let result = card_to_whatsapp_text(&c);
        assert!(result.contains("Section content"), "got: {result}");
    }

    #[test]
    fn cardtotext_should_handle_text_with_different_styles() {
        let c = card(
            None,
            None,
            vec![
                text_child("Normal text", None),
                text_child("Bold text", Some(TextStyle::Bold)),
                text_child("Muted text", Some(TextStyle::Muted)),
            ],
        );
        let result = card_to_whatsapp_text(&c);
        assert!(result.contains("Normal text"), "got: {result}");
        assert!(result.contains("*Bold text*"), "got: {result}");
        assert!(result.contains("_Muted text_"), "got: {result}");
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

    // ---------- cardToWhatsApp (5 upstream cases) ----------

    fn button(id: &str, label: &str) -> ActionsChild {
        ActionsChild::Button(ButtonElement {
            action_type: None,
            callback_url: None,
            disabled: None,
            id: id.to_string(),
            label: label.to_string(),
            style: None,
            kind: ButtonKind::Button,
            value: None,
        })
    }

    #[test]
    fn cardtowhatsapp_should_produce_interactive_message_for_card_with_1_to_3_buttons() {
        let c = card(
            Some("Choose an action"),
            None,
            vec![
                CardChild::Text(TextElement {
                    content: "What would you like to do?".to_string(),
                    style: None,
                    kind: TextKind::Text,
                }),
                CardChild::Actions(ActionsElement {
                    children: vec![button("btn_yes", "Yes"), button("btn_no", "No")],
                    kind: ActionsKind::Actions,
                }),
            ],
        );
        let r = card_to_whatsapp(&c);
        match r {
            WhatsAppCardResult::Interactive(m) => {
                assert_eq!(m.interactive_type, "button");
                assert_eq!(
                    m.header.as_ref().map(|h| h.text.as_str()),
                    Some("Choose an action")
                );
                assert_eq!(m.action.buttons.len(), 2);
                assert_eq!(
                    m.action.buttons[0].reply.id,
                    encode_whatsapp_callback_data("btn_yes", None)
                );
                assert_eq!(
                    m.action.buttons[1].reply.id,
                    encode_whatsapp_callback_data("btn_no", None)
                );
            }
            other => panic!("expected interactive, got {other:?}"),
        }
    }

    #[test]
    fn cardtowhatsapp_should_truncate_to_first_3_buttons_when_more_than_3_are_provided() {
        let c = card(
            Some("Too many buttons"),
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    button("btn_1", "One"),
                    button("btn_2", "Two"),
                    button("btn_3", "Three"),
                    button("btn_4", "Four"),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let r = card_to_whatsapp(&c);
        match r {
            WhatsAppCardResult::Interactive(m) => {
                assert_eq!(m.action.buttons.len(), 3);
            }
            other => panic!("expected interactive, got {other:?}"),
        }
    }

    #[test]
    fn cardtowhatsapp_should_fall_back_to_text_for_link_only_buttons() {
        let c = card(
            Some("Links only"),
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::LinkButton(LinkButtonElement {
                    label: "Visit".to_string(),
                    style: None,
                    kind: LinkButtonKind::LinkButton,
                    url: "https://example.com".to_string(),
                })],
                kind: ActionsKind::Actions,
            })],
        );
        assert!(matches!(card_to_whatsapp(&c), WhatsAppCardResult::Text(_)));
    }

    #[test]
    fn cardtowhatsapp_should_fall_back_to_text_for_cards_without_actions() {
        let c = card(
            Some("Info only"),
            None,
            vec![CardChild::Text(TextElement {
                content: "Just some info".to_string(),
                style: None,
                kind: TextKind::Text,
            })],
        );
        assert!(matches!(card_to_whatsapp(&c), WhatsAppCardResult::Text(_)));
    }

    #[test]
    fn cardtowhatsapp_should_truncate_long_button_titles_to_20_chars() {
        let c = card(
            None,
            None,
            vec![
                CardChild::Text(TextElement {
                    content: "Choose".to_string(),
                    style: None,
                    kind: TextKind::Text,
                }),
                CardChild::Actions(ActionsElement {
                    children: vec![button(
                        "btn_long",
                        "This is a very long button title that exceeds the limit",
                    )],
                    kind: ActionsKind::Actions,
                }),
            ],
        );
        let r = card_to_whatsapp(&c);
        match r {
            WhatsAppCardResult::Interactive(m) => {
                let title_len = m.action.buttons[0].reply.title.chars().count();
                assert!(title_len <= 20, "title too long: {title_len}");
            }
            other => panic!("expected interactive, got {other:?}"),
        }
    }
}
