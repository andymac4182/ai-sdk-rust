//! Telegram inline-keyboard renderer + callback-data codec.
//!
//! 1:1 port of `packages/adapter-telegram/src/cards.ts`.
//!
//! Telegram represents card actions as **inline keyboard buttons**
//! attached to a message via `reply_markup: {inline_keyboard:
//! [[button, ...], ...]}`. Each button is either an action button
//! (with `callback_data` payload routed back through a webhook) or
//! a link button (with `url`). The callback data is a tightly
//! size-limited string (64 bytes UTF-8) that must round-trip the
//! action id + optional value.

use chat_sdk_chat::cards::{ActionsChild, ActionsElement, CardChild, CardElement};

/// Callback-data prefix used to distinguish chat-sdk-encoded
/// payloads from legacy raw strings. 1:1 with upstream
/// `CALLBACK_DATA_PREFIX = "chat:"`.
pub const CALLBACK_DATA_PREFIX: &str = "chat:";

/// Telegram's hard 64-byte cap on callback_data. 1:1 with upstream
/// `TELEGRAM_CALLBACK_DATA_LIMIT_BYTES = 64`.
pub const TELEGRAM_CALLBACK_DATA_LIMIT_BYTES: usize = 64;

/// Decoded callback payload. 1:1 with upstream
/// `{ actionId, value }` return shape from
/// `decodeTelegramCallbackData`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedTelegramCallbackData {
    /// Action ID (the same `id` field that was on the button).
    pub action_id: String,
    /// Optional caller-supplied value.
    pub value: Option<String>,
}

/// Error returned when a callback payload exceeds Telegram's
/// 64-byte limit. 1:1 with upstream
/// `throw new ValidationError("telegram", "...max 64 bytes")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramCallbackTooLargeError {
    pub limit_bytes: usize,
    pub message: String,
}

impl std::fmt::Display for TelegramCallbackTooLargeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TelegramCallbackTooLargeError {}

/// Encode a callback payload (action_id + optional value) into the
/// `chat:{...}` JSON wire shape. 1:1 port of upstream
/// `encodeTelegramCallbackData(actionId, value?)`. Returns an
/// error when the resulting string is over 64 bytes (Telegram's
/// hard cap).
pub fn encode_telegram_callback_data(
    action_id: &str,
    value: Option<&str>,
) -> Result<String, TelegramCallbackTooLargeError> {
    let mut obj = serde_json::Map::with_capacity(2);
    obj.insert("a".to_string(), serde_json::Value::String(action_id.to_string()));
    if let Some(v) = value {
        obj.insert("v".to_string(), serde_json::Value::String(v.to_string()));
    }
    let json = serde_json::Value::Object(obj).to_string();
    let callback_data = format!("{CALLBACK_DATA_PREFIX}{json}");
    if callback_data.as_bytes().len() > TELEGRAM_CALLBACK_DATA_LIMIT_BYTES {
        return Err(TelegramCallbackTooLargeError {
            limit_bytes: TELEGRAM_CALLBACK_DATA_LIMIT_BYTES,
            message: format!(
                "Callback payload too large for Telegram (max {TELEGRAM_CALLBACK_DATA_LIMIT_BYTES} bytes)."
            ),
        });
    }
    Ok(callback_data)
}

/// Decode a callback payload string into action id + value. 1:1
/// port of upstream `decodeTelegramCallbackData(data?)`:
///
/// - `None` / empty -> `{action_id: "telegram_callback", value:
///   None}` (the upstream default for "no payload present").
/// - Not starting with `"chat:"` -> legacy passthrough:
///   `{action_id: data, value: Some(data)}`.
/// - Starts with `"chat:"` but JSON is malformed or missing the
///   `a` field -> same legacy passthrough.
/// - Well-formed `"chat:{...}"` -> the decoded action id + value.
pub fn decode_telegram_callback_data(data: Option<&str>) -> DecodedTelegramCallbackData {
    let Some(data) = data.filter(|s| !s.is_empty()) else {
        return DecodedTelegramCallbackData {
            action_id: "telegram_callback".to_string(),
            value: None,
        };
    };

    let Some(payload_json) = data.strip_prefix(CALLBACK_DATA_PREFIX) else {
        return DecodedTelegramCallbackData {
            action_id: data.to_string(),
            value: Some(data.to_string()),
        };
    };

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(payload_json)
        && let Some(a) = parsed.get("a").and_then(|v| v.as_str())
        && !a.is_empty()
    {
        let value = parsed
            .get("v")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        return DecodedTelegramCallbackData {
            action_id: a.to_string(),
            value,
        };
    }

    DecodedTelegramCallbackData {
        action_id: data.to_string(),
        value: Some(data.to_string()),
    }
}

/// An empty inline keyboard payload. 1:1 with upstream
/// `emptyTelegramInlineKeyboard() -> { inline_keyboard: [] }`.
/// Used by `editMessageText` to remove existing keyboards.
pub fn empty_telegram_inline_keyboard() -> serde_json::Value {
    serde_json::json!({ "inline_keyboard": [] })
}

/// Build a Telegram inline keyboard from a [`CardElement`]'s
/// actions. 1:1 port of upstream `cardToTelegramInlineKeyboard`.
/// Returns `None` when the card has no actions (so callers can
/// omit `reply_markup` entirely).
pub fn card_to_telegram_inline_keyboard(card: &CardElement) -> Option<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    collect_inline_keyboard_rows(&card.children, &mut rows);
    if rows.is_empty() {
        return None;
    }
    Some(serde_json::json!({ "inline_keyboard": rows }))
}

fn collect_inline_keyboard_rows(children: &[CardChild], rows: &mut Vec<serde_json::Value>) {
    for child in children {
        match child {
            CardChild::Actions(a) => {
                if let Some(row) = to_inline_keyboard_row(a) {
                    rows.push(row);
                }
            }
            CardChild::Section(s) => {
                collect_inline_keyboard_rows(&s.children, rows);
            }
            _ => {}
        }
    }
}

fn to_inline_keyboard_row(actions: &ActionsElement) -> Option<serde_json::Value> {
    let mut row: Vec<serde_json::Value> = Vec::new();
    for action in &actions.children {
        match action {
            ActionsChild::Button(b) => {
                // Upstream wraps label through convertEmojiPlaceholders
                // first. The Rust port currently passes the label
                // through unchanged (deferred until the emoji-resolver
                // for Telegram lands).
                if let Ok(callback) = encode_telegram_callback_data(&b.id, b.value.as_deref()) {
                    row.push(serde_json::json!({
                        "text": b.label,
                        "callback_data": callback,
                    }));
                }
            }
            ActionsChild::LinkButton(lb) => {
                row.push(serde_json::json!({
                    "text": lb.label,
                    "url": lb.url,
                }));
            }
            // Select/RadioSelect aren't representable as Telegram
            // inline keyboard rows - upstream silently drops them.
            ActionsChild::Select(_) | ActionsChild::RadioSelect(_) => {}
        }
    }
    if row.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{
        ActionsKind, ButtonElement, ButtonKind, CardKind, LinkButtonElement, LinkButtonKind,
        SectionElement, SectionKind, TextElement, TextKind,
    };

    fn card_with_children(children: Vec<CardChild>) -> CardElement {
        CardElement {
            title: None,
            subtitle: None,
            image_url: None,
            kind: CardKind::Card,
            children,
        }
    }

    fn button(id: &str, label: &str) -> ActionsChild {
        ActionsChild::Button(ButtonElement {
            id: id.to_string(),
            label: label.to_string(),
            action_type: None,
            callback_url: None,
            disabled: None,
            style: None,
            value: None,
            kind: ButtonKind::Button,
        })
    }

    fn link_button(label: &str, url: &str) -> ActionsChild {
        ActionsChild::LinkButton(LinkButtonElement {
            label: label.to_string(),
            url: url.to_string(),
            style: None,
            kind: LinkButtonKind::LinkButton,
        })
    }

    // ---------- cardToTelegramInlineKeyboard (3 cases) ----------

    #[test]
    fn returns_none_when_card_has_no_actions() {
        let card = card_with_children(vec![CardChild::Text(TextElement {
            content: "hi".to_string(),
            style: None,
            kind: TextKind::Text,
        })]);
        assert!(card_to_telegram_inline_keyboard(&card).is_none());
    }

    #[test]
    fn converts_multiple_actions_blocks_into_multiple_keyboard_rows() {
        let card = card_with_children(vec![
            CardChild::Actions(ActionsElement {
                kind: ActionsKind::Actions,
                children: vec![button("a", "A"), button("b", "B")],
            }),
            CardChild::Section(SectionElement {
                kind: SectionKind::Section,
                children: vec![CardChild::Actions(ActionsElement {
                    kind: ActionsKind::Actions,
                    children: vec![link_button("Docs", "https://chat-sdk.dev")],
                })],
            }),
        ]);
        let kb = card_to_telegram_inline_keyboard(&card).unwrap();
        let rows = kb["inline_keyboard"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        // First row: two action buttons (callback_data).
        let r0 = rows[0].as_array().unwrap();
        assert_eq!(r0.len(), 2);
        assert_eq!(r0[0]["text"], "A");
        assert!(r0[0]["callback_data"].as_str().unwrap().starts_with("chat:"));
        assert_eq!(r0[1]["text"], "B");
        // Second row: one link button.
        let r1 = rows[1].as_array().unwrap();
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0]["text"], "Docs");
        assert_eq!(r1[0]["url"], "https://chat-sdk.dev");
    }

    #[test]
    fn ignores_unsupported_action_controls() {
        use chat_sdk_chat::modals::{SelectElement, SelectKind, SelectOptionElement};
        let card = card_with_children(vec![CardChild::Actions(ActionsElement {
            kind: ActionsKind::Actions,
            children: vec![ActionsChild::Select(SelectElement {
                id: "priority".to_string(),
                label: "Priority".to_string(),
                initial_option: None,
                optional: None,
                options: vec![SelectOptionElement {
                    label: "High".to_string(),
                    value: "high".to_string(),
                    description: None,
                }],
                placeholder: None,
                kind: SelectKind::Select,
            })],
        })]);
        assert!(card_to_telegram_inline_keyboard(&card).is_none());
    }

    // ---------- callback payload encoding (5 cases) ----------

    #[test]
    fn encodes_and_decodes_callback_payload_with_value() {
        let encoded =
            encode_telegram_callback_data("approve", Some("request-123")).unwrap();
        let decoded = decode_telegram_callback_data(Some(&encoded));
        assert_eq!(
            decoded,
            DecodedTelegramCallbackData {
                action_id: "approve".to_string(),
                value: Some("request-123".to_string()),
            }
        );
    }

    #[test]
    fn decodes_empty_callback_payload_with_telegram_callback_fallback() {
        let decoded = decode_telegram_callback_data(None);
        assert_eq!(
            decoded,
            DecodedTelegramCallbackData {
                action_id: "telegram_callback".to_string(),
                value: None,
            }
        );
    }

    #[test]
    fn falls_back_to_raw_payload_for_malformed_encoded_data() {
        let decoded = decode_telegram_callback_data(Some("chat:{not-json"));
        assert_eq!(
            decoded,
            DecodedTelegramCallbackData {
                action_id: "chat:{not-json".to_string(),
                value: Some("chat:{not-json".to_string()),
            }
        );
    }

    #[test]
    fn falls_back_to_raw_payload_for_non_encoded_callbacks() {
        let decoded = decode_telegram_callback_data(Some("legacy_action"));
        assert_eq!(
            decoded,
            DecodedTelegramCallbackData {
                action_id: "legacy_action".to_string(),
                value: Some("legacy_action".to_string()),
            }
        );
    }

    #[test]
    fn throws_when_callback_payload_exceeds_telegram_limit() {
        let very_long = "x".repeat(200);
        let err = encode_telegram_callback_data(&very_long, None).unwrap_err();
        assert!(err.message.contains("max 64 bytes"));
    }

    // ---------- emptyTelegramInlineKeyboard (1 case) ----------

    #[test]
    fn returns_an_empty_keyboard() {
        assert_eq!(
            empty_telegram_inline_keyboard(),
            serde_json::json!({ "inline_keyboard": [] })
        );
    }
}
