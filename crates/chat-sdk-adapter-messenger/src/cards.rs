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
    ActionsChild, ActionsElement, ButtonElement, CardChild, CardElement, FieldsElement,
    LinkButtonElement, TextElement,
};

/// Callback-data prefix used to distinguish chat-sdk-encoded
/// payloads from legacy raw strings. 1:1 with upstream
/// `CALLBACK_DATA_PREFIX = "chat:"`.
pub const CALLBACK_DATA_PREFIX: &str = "chat:";

/// Maximum number of buttons Messenger allows per template. 1:1
/// with upstream `MAX_BUTTONS = 3`.
pub const MAX_BUTTONS: usize = 3;

/// Maximum character length for a button title. 1:1 with upstream
/// `MAX_BUTTON_TITLE_LENGTH = 20`.
pub const MAX_BUTTON_TITLE_LENGTH: usize = 20;

/// Maximum character length for subtitle in Generic Template. 1:1
/// with upstream `MAX_SUBTITLE_LENGTH = 80`.
pub const MAX_SUBTITLE_LENGTH: usize = 80;

/// Maximum character length for text in Button Template. 1:1 with
/// upstream `MAX_BUTTON_TEMPLATE_TEXT_LENGTH = 640`.
pub const MAX_BUTTON_TEMPLATE_TEXT_LENGTH: usize = 640;

/// Maximum character length for title in Generic Template. 1:1
/// with upstream `MAX_TITLE_LENGTH = 80`.
pub const MAX_TITLE_LENGTH: usize = 80;

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

/// Messenger button shape. 1:1 with upstream `MessengerButton` —
/// `postback` for interactive buttons (carrying an opaque
/// callback-data string), `web_url` for link buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessengerButton {
    Postback { title: String, payload: String },
    WebUrl { title: String, url: String },
}

/// Generic Template element. 1:1 with upstream
/// `MessengerGenericElement`. `subtitle` and `image_url` are
/// optional; `buttons` is the variable-length button payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessengerGenericElement {
    pub title: String,
    pub subtitle: Option<String>,
    pub image_url: Option<String>,
    pub buttons: Vec<MessengerButton>,
}

/// Messenger template payload. 1:1 with the upstream discriminated
/// union on `template_type`: `"generic"` carries an `elements`
/// array; `"button"` carries flat `text` + `buttons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessengerTemplatePayload {
    Generic { elements: Vec<MessengerGenericElement> },
    Button { text: String, buttons: Vec<MessengerButton> },
}

/// Result of [`card_to_messenger`]. 1:1 with upstream's
/// `MessengerCardResult` union: either a text fallback or a
/// template payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessengerCardResult {
    Text { text: String },
    Template { payload: MessengerTemplatePayload },
}

/// 1:1 port of upstream `cardToMessenger(card)`. Returns a template
/// payload when the card's actions fit Messenger's constraints
/// (≤ [`MAX_BUTTONS`] buttons; each button label ≤
/// [`MAX_BUTTON_TITLE_LENGTH`] chars) AND the children contain no
/// unsupported elements (tables and Select / RadioSelect inside
/// Actions force the text fallback); otherwise returns the text
/// fallback.
///
/// When templated, the renderer picks the Generic Template if the
/// card has a `title` or `imageUrl`, otherwise the Button Template
/// for body-text + buttons cards.
pub fn card_to_messenger(card: &CardElement) -> MessengerCardResult {
    if has_unsupported_elements(&card.children) {
        return MessengerCardResult::Text {
            text: card_to_messenger_text(card),
        };
    }

    let actions = find_actions(&card.children);
    let buttons = actions.and_then(extract_buttons);

    if let Some(buttons) = buttons {
        if !buttons.is_empty() && buttons.len() <= MAX_BUTTONS {
            let all_fit = buttons
                .iter()
                .all(|b| button_title(b).chars().count() <= MAX_BUTTON_TITLE_LENGTH);
            if all_fit {
                if card.title.as_deref().is_some_and(|t| !t.is_empty())
                    || card.image_url.as_deref().is_some_and(|u| !u.is_empty())
                {
                    return MessengerCardResult::Template {
                        payload: build_generic_template(card, buttons),
                    };
                }
                let body = build_body_text(card);
                if !body.is_empty() {
                    return MessengerCardResult::Template {
                        payload: build_button_template(body, buttons),
                    };
                }
            }
        }
    }

    MessengerCardResult::Text {
        text: card_to_messenger_text(card),
    }
}

fn button_title(b: &MessengerButton) -> &str {
    match b {
        MessengerButton::Postback { title, .. } | MessengerButton::WebUrl { title, .. } => title,
    }
}

/// 1:1 with upstream `hasUnsupportedElements(children)`. Returns
/// `true` for any descendant `Table`, or for any Actions block
/// containing `Select` / `RadioSelect` children.
fn has_unsupported_elements(children: &[CardChild]) -> bool {
    for child in children {
        match child {
            CardChild::Table(_) => return true,
            CardChild::Section(s) if has_unsupported_elements(&s.children) => return true,
            CardChild::Actions(a) => {
                if a.children.iter().any(|c| {
                    matches!(c, ActionsChild::Select(_) | ActionsChild::RadioSelect(_))
                }) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// 1:1 with upstream `findActions(children)`. Recurses into
/// `Section` children. Returns the first `ActionsElement` found.
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

/// 1:1 with upstream `extractButtons(actions)`. Filters out non-
/// button children; truncates to [`MAX_BUTTONS`] (first 3). Returns
/// `None` when no buttons survive the filter (upstream returns
/// `null` in that case).
fn extract_buttons(actions: &ActionsElement) -> Option<Vec<MessengerButton>> {
    let mut buttons: Vec<MessengerButton> = Vec::new();
    for child in &actions.children {
        match child {
            ActionsChild::Button(b) if !b.id.is_empty() => buttons.push(convert_button(b)),
            ActionsChild::LinkButton(b) => buttons.push(convert_link_button(b)),
            _ => {}
        }
    }
    if buttons.is_empty() {
        return None;
    }
    if buttons.len() > MAX_BUTTONS {
        buttons.truncate(MAX_BUTTONS);
    }
    Some(buttons)
}

fn convert_button(button: &ButtonElement) -> MessengerButton {
    MessengerButton::Postback {
        title: truncate(&button.label, MAX_BUTTON_TITLE_LENGTH),
        payload: encode_messenger_callback_data(&button.id, button.value.as_deref()),
    }
}

fn convert_link_button(button: &LinkButtonElement) -> MessengerButton {
    MessengerButton::WebUrl {
        title: truncate(&button.label, MAX_BUTTON_TITLE_LENGTH),
        url: button.url.clone(),
    }
}

/// 1:1 with upstream `buildGenericTemplate(card, buttons)`. The
/// `subtitle` field is set when the card has its own subtitle, OR
/// when the card has a title AND non-empty body text (in which case
/// the body text becomes the subtitle). Title falls back to the
/// body text, then to `"Menu"` if neither is present.
fn build_generic_template(card: &CardElement, buttons: Vec<MessengerButton>) -> MessengerTemplatePayload {
    let body_text = build_body_text(card);
    let raw_title = card
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if !body_text.is_empty() {
                body_text.clone()
            } else {
                "Menu".to_string()
            }
        });

    let subtitle = if let Some(sub) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        Some(truncate(sub, MAX_SUBTITLE_LENGTH))
    } else if card.title.as_deref().is_some_and(|t| !t.is_empty()) && !body_text.is_empty() {
        Some(truncate(&body_text, MAX_SUBTITLE_LENGTH))
    } else {
        None
    };

    let image_url = card.image_url.as_deref().filter(|u| !u.is_empty()).map(str::to_owned);

    MessengerTemplatePayload::Generic {
        elements: vec![MessengerGenericElement {
            title: truncate(&raw_title, MAX_TITLE_LENGTH),
            subtitle,
            image_url,
            buttons,
        }],
    }
}

/// 1:1 with upstream `buildButtonTemplate(text, buttons)`.
fn build_button_template(text: String, buttons: Vec<MessengerButton>) -> MessengerTemplatePayload {
    MessengerTemplatePayload::Button {
        text: truncate(&text, MAX_BUTTON_TEMPLATE_TEXT_LENGTH),
        buttons,
    }
}

/// 1:1 with upstream `buildBodyText(card)`. Builds text from the
/// card's non-action children using [`child_to_plain_text`].
fn build_body_text(card: &CardElement) -> String {
    let mut parts: Vec<String> = Vec::new();
    for child in &card.children {
        if matches!(child, CardChild::Actions(_)) {
            continue;
        }
        if let Some(text) = child_to_plain_text(child) {
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("\n")
}

/// 1:1 with upstream private `childToPlainText(child)`. Used by
/// `buildBodyText` (not the public text-fallback path which uses
/// [`render_child`] above). Sections are recursively flattened;
/// `Actions` returns `None`.
fn child_to_plain_text(child: &CardChild) -> Option<String> {
    match child {
        CardChild::Text(t) => Some(t.content.clone()),
        CardChild::Fields(f) => {
            let lines: Vec<String> = f
                .children
                .iter()
                .map(|fld| format!("{}: {}", fld.label, fld.value))
                .collect();
            Some(lines.join("\n"))
        }
        CardChild::Actions(_) => None,
        CardChild::Section(s) => {
            let parts: Vec<String> = s.children.iter().filter_map(child_to_plain_text).collect();
            if parts.is_empty() { None } else { Some(parts.join("\n")) }
        }
        CardChild::Link(l) => Some(format!("{}: {}", l.label, l.url)),
        _ => None,
    }
}

/// 1:1 with upstream private `truncate(text, maxLength)`. Counts
/// characters (Unicode scalar values), not bytes. Appends `\u{2026}`
/// (horizontal-ellipsis) when truncating; the ellipsis itself
/// counts toward `max_length`.
fn truncate(text: &str, max_length: usize) -> String {
    if text.chars().count() <= max_length {
        return text.to_string();
    }
    if max_length == 0 {
        return String::new();
    }
    let mut s: String = text.chars().take(max_length - 1).collect();
    s.push('\u{2026}');
    s
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

    // ---------- Messenger template limit constants ----------
    // 1:1 with upstream's private `MAX_*` consts in cards.ts.
    // Exposed at module scope (rather than private as upstream) so
    // the limits can be referenced + asserted by callers / tests
    // without re-declaring them.

    #[test]
    fn messenger_template_limits_match_upstream() {
        assert_eq!(MAX_BUTTONS, 3);
        assert_eq!(MAX_BUTTON_TITLE_LENGTH, 20);
        assert_eq!(MAX_SUBTITLE_LENGTH, 80);
        assert_eq!(MAX_BUTTON_TEMPLATE_TEXT_LENGTH, 640);
        assert_eq!(MAX_TITLE_LENGTH, 80);
    }

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

    // ---------- cardToMessenger Generic Template (5 cases) ----------
    // 1:1 with upstream `cards.test.ts > describe("template
    // conversion") > describe("Generic Template")`. Covers the 5
    // portable Generic-Template cases — Button Template + constraint
    // handling + callback-data integration tests land in follow-up
    // slices.

    fn button_action(id: &str, label: &str) -> ActionsChild {
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

    fn link_button_action(url: &str, label: &str) -> ActionsChild {
        ActionsChild::LinkButton(LinkButtonElement {
            label: label.to_string(),
            style: None,
            kind: LinkButtonKind::LinkButton,
            url: url.to_string(),
        })
    }

    #[test]
    fn generic_template_for_card_with_title_and_buttons() {
        let c = CardElement {
            title: Some("Choose an action".to_string()),
            subtitle: None,
            image_url: None,
            kind: CardKind::Card,
            children: vec![
                text_child("What would you like to do?"),
                CardChild::Actions(ActionsElement {
                    children: vec![button_action("btn_yes", "Yes"), button_action("btn_no", "No")],
                    kind: ActionsKind::Actions,
                }),
            ],
        };
        let result = card_to_messenger(&c);
        match result {
            MessengerCardResult::Template {
                payload: MessengerTemplatePayload::Generic { elements },
            } => {
                assert_eq!(elements.len(), 1);
                assert_eq!(elements[0].title, "Choose an action");
                assert_eq!(elements[0].buttons.len(), 2);
                match &elements[0].buttons[0] {
                    MessengerButton::Postback { title, .. } => assert_eq!(title, "Yes"),
                    other => panic!("expected Postback, got {other:?}"),
                }
            }
            other => panic!("expected Generic Template, got {other:?}"),
        }
    }

    #[test]
    fn generic_template_for_card_with_image_url() {
        let c = CardElement {
            title: Some("Product".to_string()),
            subtitle: None,
            image_url: Some("https://example.com/product.jpg".to_string()),
            kind: CardKind::Card,
            children: vec![CardChild::Actions(ActionsElement {
                children: vec![button_action("buy", "Buy Now")],
                kind: ActionsKind::Actions,
            })],
        };
        let result = card_to_messenger(&c);
        match result {
            MessengerCardResult::Template {
                payload: MessengerTemplatePayload::Generic { elements },
            } => {
                assert_eq!(
                    elements[0].image_url.as_deref(),
                    Some("https://example.com/product.jpg")
                );
            }
            other => panic!("expected Generic Template, got {other:?}"),
        }
    }

    #[test]
    fn generic_template_includes_subtitle() {
        let c = CardElement {
            title: Some("Order #123".to_string()),
            subtitle: Some("Your order is ready".to_string()),
            image_url: None,
            kind: CardKind::Card,
            children: vec![CardChild::Actions(ActionsElement {
                children: vec![button_action("view", "View")],
                kind: ActionsKind::Actions,
            })],
        };
        let result = card_to_messenger(&c);
        match result {
            MessengerCardResult::Template {
                payload: MessengerTemplatePayload::Generic { elements },
            } => {
                assert_eq!(elements[0].subtitle.as_deref(), Some("Your order is ready"));
            }
            other => panic!("expected Generic Template, got {other:?}"),
        }
    }

    #[test]
    fn generic_template_supports_link_buttons_as_web_url() {
        let c = CardElement {
            title: Some("Resources".to_string()),
            subtitle: None,
            image_url: None,
            kind: CardKind::Card,
            children: vec![CardChild::Actions(ActionsElement {
                children: vec![link_button_action("https://example.com/docs", "View Docs")],
                kind: ActionsKind::Actions,
            })],
        };
        let result = card_to_messenger(&c);
        match result {
            MessengerCardResult::Template {
                payload: MessengerTemplatePayload::Generic { elements },
            } => match &elements[0].buttons[0] {
                MessengerButton::WebUrl { url, .. } => {
                    assert_eq!(url, "https://example.com/docs")
                }
                other => panic!("expected WebUrl, got {other:?}"),
            },
            other => panic!("expected Generic Template, got {other:?}"),
        }
    }

    #[test]
    fn generic_template_mixes_postback_and_web_url_buttons() {
        let c = CardElement {
            title: Some("Options".to_string()),
            subtitle: None,
            image_url: None,
            kind: CardKind::Card,
            children: vec![CardChild::Actions(ActionsElement {
                children: vec![
                    button_action("action1", "Do Action"),
                    link_button_action("https://example.com", "Learn More"),
                ],
                kind: ActionsKind::Actions,
            })],
        };
        let result = card_to_messenger(&c);
        match result {
            MessengerCardResult::Template {
                payload: MessengerTemplatePayload::Generic { elements },
            } => {
                assert_eq!(elements[0].buttons.len(), 2);
                assert!(matches!(elements[0].buttons[0], MessengerButton::Postback { .. }));
                assert!(matches!(elements[0].buttons[1], MessengerButton::WebUrl { .. }));
            }
            other => panic!("expected Generic Template, got {other:?}"),
        }
    }
}
