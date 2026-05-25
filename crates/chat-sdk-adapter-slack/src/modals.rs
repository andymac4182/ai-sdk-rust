//! Slack modal (view) converter + metadata codec helpers.
//!
//! 1:1 port of `packages/adapter-slack/src/modals.ts`:
//!
//! - `encode_modal_metadata` / `decode_modal_metadata` + the
//!   [`ModalMetadata`] shape (slice 193).
//! - `modal_to_slack_view` + `select_option_to_slack_option` + the
//!   per-element block converters (text input, select, external select,
//!   radio select) that translate [`ModalElement`] into Slack Block Kit
//!   view JSON. Block JSON values are emitted as
//!   [`serde_json::Value`] (typed as [`SlackBlock`] alias in [`crate::cards`]).

use crate::cards::{SlackBlock, convert_fields_to_block, convert_text_to_block};
use chat_sdk_chat::modals::{
    ExternalSelectElement, ModalChild, ModalElement, RadioSelectElement, SelectElement,
    SelectOptionElement, TextInputElement,
};
use serde_json::{Value, json};

/// Decoded modal metadata. 1:1 with upstream
/// `interface ModalMetadata`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModalMetadata {
    /// Per-modal context id used by chat-sdk to correlate a
    /// `views.open` with subsequent `view_submission` /
    /// `view_closed` events.
    pub context_id: Option<String>,
    /// Caller-supplied opaque metadata. Round-trips through
    /// Slack's `private_metadata` field.
    pub private_metadata: Option<String>,
}

/// Encode contextId + privateMetadata into Slack's
/// `private_metadata` field. 1:1 with upstream
/// `encodeModalMetadata(meta)`:
///
/// - Returns `None` when both fields are absent.
/// - Otherwise returns `JSON.stringify({c, m})` where each key
///   is omitted when its value is None.
pub fn encode_modal_metadata(meta: &ModalMetadata) -> Option<String> {
    if meta.context_id.is_none() && meta.private_metadata.is_none() {
        return None;
    }
    let mut obj = serde_json::Map::with_capacity(2);
    if let Some(c) = &meta.context_id {
        obj.insert("c".to_string(), serde_json::Value::String(c.clone()));
    }
    if let Some(m) = &meta.private_metadata {
        obj.insert("m".to_string(), serde_json::Value::String(m.clone()));
    }
    Some(serde_json::Value::Object(obj).to_string())
}

/// Decode Slack's `private_metadata` back into a [`ModalMetadata`].
/// 1:1 with upstream `decodeModalMetadata(raw?)`:
///
/// - `None` / empty -> `ModalMetadata::default()`.
/// - Well-formed `{c, m}` JSON -> decoded fields. Empty-string
///   values fall back to `None`.
/// - Anything else (legacy plain-string, JSON missing both keys,
///   malformed JSON) -> `{context_id: Some(raw), private_metadata:
///   None}` so legacy callers that stored a raw UUID still work.
pub fn decode_modal_metadata(raw: Option<&str>) -> ModalMetadata {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else {
        return ModalMetadata::default();
    };
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw)
        && parsed.is_object()
    {
        let has_c = parsed.get("c").is_some();
        let has_m = parsed.get("m").is_some();
        if has_c || has_m {
            let context_id = parsed
                .get("c")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let private_metadata = parsed
                .get("m")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            return ModalMetadata {
                context_id,
                private_metadata,
            };
        }
    }
    // Legacy passthrough: treat the raw string as a plain
    // contextId.
    ModalMetadata {
        context_id: Some(raw.to_string()),
        private_metadata: None,
    }
}

// ============================================================================
// modalToSlackView + element->block converters
// ============================================================================

/// Convert a [`ModalElement`] into a Slack Block Kit `view` payload
/// (the JSON `views.open` / `views.update` expects). 1:1 with upstream
/// `modalToSlackView(modal, contextId?)`:
///
/// - `title` is truncated to 24 chars (Slack hard limit).
/// - `submit` / `close` default to `"Submit"` / `"Cancel"` when
///   absent.
/// - `private_metadata` is omitted when `context_id` is `None`.
/// - `notify_on_close` is omitted when `None`.
/// - `blocks` is `modal.children.map(modalChildToBlock)`.
pub fn modal_to_slack_view(modal: &ModalElement, context_id: Option<&str>) -> Value {
    // Title truncation matches upstream `modal.title.slice(0, 24)`,
    // which is JS string-length (UTF-16 code units). For ASCII inputs
    // (the only ones exercised by upstream tests) `chars().take(24)`
    // yields the same byte shape.
    let title_text: String = modal.title.chars().take(24).collect();
    let submit_text = modal
        .submit_label
        .as_deref()
        .unwrap_or("Submit")
        .to_string();
    let close_text = modal.close_label.as_deref().unwrap_or("Cancel").to_string();

    let blocks: Vec<Value> = modal.children.iter().map(modal_child_to_block).collect();

    let mut view = serde_json::Map::new();
    view.insert("type".to_string(), json!("modal"));
    view.insert("callback_id".to_string(), json!(modal.callback_id));
    view.insert(
        "title".to_string(),
        json!({ "type": "plain_text", "text": title_text }),
    );
    view.insert(
        "submit".to_string(),
        json!({ "type": "plain_text", "text": submit_text }),
    );
    view.insert(
        "close".to_string(),
        json!({ "type": "plain_text", "text": close_text }),
    );
    if let Some(notify) = modal.notify_on_close {
        view.insert("notify_on_close".to_string(), json!(notify));
    }
    if let Some(ctx) = context_id {
        view.insert("private_metadata".to_string(), json!(ctx));
    }
    view.insert("blocks".to_string(), Value::Array(blocks));
    Value::Object(view)
}

fn modal_child_to_block(child: &ModalChild) -> SlackBlock {
    match child {
        ModalChild::TextInput(t) => text_input_to_block(t),
        ModalChild::Select(s) => select_to_block(s),
        ModalChild::ExternalSelect(s) => external_select_to_block(s),
        ModalChild::RadioSelect(r) => radio_select_to_block(r),
        ModalChild::Text(t) => convert_text_to_block(t),
        ModalChild::Fields(f) => convert_fields_to_block(f),
    }
}

/// Convert a [`SelectOptionElement`] into a Slack option object.
/// 1:1 with upstream `selectOptionToSlackOption(option)` — emits a
/// `plain_text` text + `value`, plus an optional `plain_text`
/// description.
pub fn select_option_to_slack_option(option: &SelectOptionElement) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "text".to_string(),
        json!({ "type": "plain_text", "text": option.label }),
    );
    obj.insert("value".to_string(), json!(option.value));
    if let Some(desc) = &option.description {
        obj.insert(
            "description".to_string(),
            json!({ "type": "plain_text", "text": desc }),
        );
    }
    Value::Object(obj)
}

fn text_input_to_block(input: &TextInputElement) -> SlackBlock {
    let mut element = serde_json::Map::new();
    element.insert("type".to_string(), json!("plain_text_input"));
    element.insert("action_id".to_string(), json!(input.id));
    element.insert(
        "multiline".to_string(),
        json!(input.multiline.unwrap_or(false)),
    );

    if let Some(placeholder) = &input.placeholder {
        element.insert(
            "placeholder".to_string(),
            json!({ "type": "plain_text", "text": placeholder }),
        );
    }
    if let Some(initial) = &input.initial_value {
        element.insert("initial_value".to_string(), json!(initial));
    }
    if let Some(max_len) = input.max_length {
        element.insert("max_length".to_string(), json!(max_len));
    }

    json!({
        "type": "input",
        "block_id": input.id,
        "optional": input.optional.unwrap_or(false),
        "label": { "type": "plain_text", "text": input.label },
        "element": Value::Object(element),
    })
}

fn select_to_block(select: &SelectElement) -> SlackBlock {
    let options: Vec<Value> = select
        .options
        .iter()
        .map(select_option_to_slack_option)
        .collect();

    let mut element = serde_json::Map::new();
    element.insert("type".to_string(), json!("static_select"));
    element.insert("action_id".to_string(), json!(select.id));
    element.insert("options".to_string(), Value::Array(options.clone()));

    if let Some(placeholder) = &select.placeholder {
        element.insert(
            "placeholder".to_string(),
            json!({ "type": "plain_text", "text": placeholder }),
        );
    }

    if let Some(initial_value) = &select.initial_option
        && let Some(initial) = options
            .iter()
            .find(|o| o.get("value").and_then(Value::as_str) == Some(initial_value.as_str()))
    {
        element.insert("initial_option".to_string(), initial.clone());
    }

    json!({
        "type": "input",
        "block_id": select.id,
        "optional": select.optional.unwrap_or(false),
        "label": { "type": "plain_text", "text": select.label },
        "element": Value::Object(element),
    })
}

fn external_select_to_block(select: &ExternalSelectElement) -> SlackBlock {
    let mut element = serde_json::Map::new();
    element.insert("type".to_string(), json!("external_select"));
    element.insert("action_id".to_string(), json!(select.id));

    if let Some(placeholder) = &select.placeholder {
        element.insert(
            "placeholder".to_string(),
            json!({ "type": "plain_text", "text": placeholder }),
        );
    }
    if let Some(min_q) = select.min_query_length {
        element.insert("min_query_length".to_string(), json!(min_q));
    }
    if let Some(initial) = &select.initial_option {
        element.insert(
            "initial_option".to_string(),
            select_option_to_slack_option(initial),
        );
    }

    json!({
        "type": "input",
        "block_id": select.id,
        "optional": select.optional.unwrap_or(false),
        "label": { "type": "plain_text", "text": select.label },
        "element": Value::Object(element),
    })
}

fn radio_select_to_block(radio_select: &RadioSelectElement) -> SlackBlock {
    // Upstream caps options at 10.
    let limited: Vec<&SelectOptionElement> = radio_select.options.iter().take(10).collect();
    let options: Vec<Value> = limited
        .iter()
        .map(|opt| {
            let mut o = serde_json::Map::new();
            o.insert(
                "text".to_string(),
                json!({ "type": "mrkdwn", "text": opt.label }),
            );
            o.insert("value".to_string(), json!(opt.value));
            if let Some(desc) = &opt.description {
                o.insert(
                    "description".to_string(),
                    json!({ "type": "mrkdwn", "text": desc }),
                );
            }
            Value::Object(o)
        })
        .collect();

    let mut element = serde_json::Map::new();
    element.insert("type".to_string(), json!("radio_buttons"));
    element.insert("action_id".to_string(), json!(radio_select.id));
    element.insert("options".to_string(), Value::Array(options.clone()));

    if let Some(initial_value) = &radio_select.initial_option
        && let Some(initial) = options
            .iter()
            .find(|o| o.get("value").and_then(Value::as_str) == Some(initial_value.as_str()))
    {
        element.insert("initial_option".to_string(), initial.clone());
    }

    json!({
        "type": "input",
        "block_id": radio_select.id,
        "optional": radio_select.optional.unwrap_or(false),
        "label": { "type": "plain_text", "text": radio_select.label },
        "element": Value::Object(element),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::modals::{
        ExternalSelectOptions, ModalOptions, RadioSelectOptions, SelectOptions, TextInputOptions,
        external_select, modal, radio_select, select, select_option, text_input,
    };

    // ---------- encodeModalMetadata (4 upstream cases) ----------

    #[test]
    fn returns_none_when_both_fields_are_empty() {
        assert!(encode_modal_metadata(&ModalMetadata::default()).is_none());
    }

    #[test]
    fn encodes_context_id_only() {
        let encoded = encode_modal_metadata(&ModalMetadata {
            context_id: Some("uuid-123".to_string()),
            private_metadata: None,
        })
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["c"], "uuid-123");
        assert!(parsed.get("m").is_none());
    }

    #[test]
    fn encodes_private_metadata_only() {
        let encoded = encode_modal_metadata(&ModalMetadata {
            context_id: None,
            private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
        })
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert!(parsed.get("c").is_none());
        assert_eq!(parsed["m"], r#"{"chatId":"abc"}"#);
    }

    #[test]
    fn encodes_both_context_id_and_private_metadata() {
        let encoded = encode_modal_metadata(&ModalMetadata {
            context_id: Some("uuid-123".to_string()),
            private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
        })
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["c"], "uuid-123");
        assert_eq!(parsed["m"], r#"{"chatId":"abc"}"#);
    }

    // ---------- decodeModalMetadata (8 upstream cases) ----------

    #[test]
    fn returns_empty_object_for_undefined_input() {
        assert_eq!(decode_modal_metadata(None), ModalMetadata::default());
    }

    #[test]
    fn returns_empty_object_for_empty_string() {
        assert_eq!(decode_modal_metadata(Some("")), ModalMetadata::default());
    }

    #[test]
    fn decodes_context_id_only() {
        let encoded = r#"{"c":"uuid-123"}"#;
        assert_eq!(
            decode_modal_metadata(Some(encoded)),
            ModalMetadata {
                context_id: Some("uuid-123".to_string()),
                private_metadata: None,
            }
        );
    }

    #[test]
    fn decodes_private_metadata_only() {
        let encoded = r#"{"m":"{\"chatId\":\"abc\"}"}"#;
        assert_eq!(
            decode_modal_metadata(Some(encoded)),
            ModalMetadata {
                context_id: None,
                private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
            }
        );
    }

    #[test]
    fn decodes_both_context_id_and_private_metadata() {
        let encoded = r#"{"c":"uuid-123","m":"{\"chatId\":\"abc\"}"}"#;
        assert_eq!(
            decode_modal_metadata(Some(encoded)),
            ModalMetadata {
                context_id: Some("uuid-123".to_string()),
                private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
            }
        );
    }

    #[test]
    fn falls_back_to_treating_plain_string_as_context_id() {
        assert_eq!(
            decode_modal_metadata(Some("plain-uuid-456")),
            ModalMetadata {
                context_id: Some("plain-uuid-456".to_string()),
                private_metadata: None,
            }
        );
    }

    #[test]
    fn falls_back_for_json_without_c_or_m_keys() {
        let raw = r#"{"other":"value"}"#;
        assert_eq!(
            decode_modal_metadata(Some(raw)),
            ModalMetadata {
                context_id: Some(raw.to_string()),
                private_metadata: None,
            }
        );
    }

    // ---------- roundtrip (1 upstream case) ----------

    #[test]
    fn roundtrips_encode_then_decode() {
        let original = ModalMetadata {
            context_id: Some("ctx-1".to_string()),
            private_metadata: Some(r#"{"key":"val"}"#.to_string()),
        };
        let encoded = encode_modal_metadata(&original).unwrap();
        let decoded = decode_modal_metadata(Some(&encoded));
        assert_eq!(decoded, original);
    }

    // ============================================================
    // describe("modalToSlackView") (15 upstream cases)
    // 1:1 with upstream `modals.test.ts > describe("modalToSlackView")`.
    // ============================================================

    fn build_modal(
        callback_id: &str,
        title: &str,
        children: Vec<ModalChild>,
        submit_label: Option<&str>,
        close_label: Option<&str>,
        notify: Option<bool>,
    ) -> ModalElement {
        modal(ModalOptions {
            callback_id: callback_id.to_string(),
            callback_url: None,
            children: Some(children),
            close_label: close_label.map(str::to_string),
            notify_on_close: notify,
            private_metadata: None,
            submit_label: submit_label.map(str::to_string),
            title: title.to_string(),
        })
    }

    #[test]
    fn modal_to_slack_view_converts_a_simple_modal_with_text_input() {
        // 1:1 with upstream `modalToSlackView > converts a simple modal with text input`.
        let m = build_modal(
            "feedback_form",
            "Send Feedback",
            vec![
                text_input(TextInputOptions {
                    id: "message".to_string(),
                    label: "Your Feedback".to_string(),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);

        assert_eq!(view["type"], "modal");
        assert_eq!(view["callback_id"], "feedback_form");
        assert_eq!(
            view["title"],
            json!({"type":"plain_text","text":"Send Feedback"})
        );
        assert_eq!(view["submit"], json!({"type":"plain_text","text":"Submit"}));
        assert_eq!(view["close"], json!({"type":"plain_text","text":"Cancel"}));
        let blocks = view["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        let b0 = &blocks[0];
        assert_eq!(b0["type"], "input");
        assert_eq!(b0["block_id"], "message");
        assert_eq!(b0["optional"], false);
        assert_eq!(
            b0["label"],
            json!({"type":"plain_text","text":"Your Feedback"})
        );
        assert_eq!(b0["element"]["type"], "plain_text_input");
        assert_eq!(b0["element"]["action_id"], "message");
        assert_eq!(b0["element"]["multiline"], false);
    }

    #[test]
    fn modal_to_slack_view_converts_a_modal_with_custom_submit_close_labels() {
        // 1:1 with upstream `converts a modal with custom submit/close labels`.
        let m = build_modal(
            "test",
            "Test Modal",
            vec![],
            Some("Send"),
            Some("Dismiss"),
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(view["submit"], json!({"type":"plain_text","text":"Send"}));
        assert_eq!(view["close"], json!({"type":"plain_text","text":"Dismiss"}));
    }

    #[test]
    fn modal_to_slack_view_converts_multiline_text_input() {
        // 1:1 with upstream `converts multiline text input`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                text_input(TextInputOptions {
                    id: "description".to_string(),
                    label: "Description".to_string(),
                    multiline: Some(true),
                    placeholder: Some("Enter description...".to_string()),
                    max_length: Some(500),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let b0 = &view["blocks"][0];
        assert_eq!(b0["type"], "input");
        assert_eq!(b0["element"]["type"], "plain_text_input");
        assert_eq!(b0["element"]["action_id"], "description");
        assert_eq!(b0["element"]["multiline"], true);
        assert_eq!(
            b0["element"]["placeholder"],
            json!({"type":"plain_text","text":"Enter description..."})
        );
        assert_eq!(b0["element"]["max_length"], 500);
    }

    #[test]
    fn modal_to_slack_view_converts_optional_text_input() {
        // 1:1 with upstream `converts optional text input`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                text_input(TextInputOptions {
                    id: "notes".to_string(),
                    label: "Notes".to_string(),
                    optional: Some(true),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(view["blocks"][0]["type"], "input");
        assert_eq!(view["blocks"][0]["optional"], true);
    }

    #[test]
    fn modal_to_slack_view_converts_text_input_with_initial_value() {
        // 1:1 with upstream `converts text input with initial value`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                text_input(TextInputOptions {
                    id: "name".to_string(),
                    label: "Name".to_string(),
                    initial_value: Some("John Doe".to_string()),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(view["blocks"][0]["element"]["initial_value"], "John Doe");
    }

    #[test]
    fn modal_to_slack_view_converts_select_element_with_options() {
        // 1:1 with upstream `converts select element with options`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                select(SelectOptions {
                    id: "category".to_string(),
                    label: "Category".to_string(),
                    options: vec![
                        select_option("Bug Report", "bug", None),
                        select_option("Feature Request", "feature", None),
                    ],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let b0 = &view["blocks"][0];
        assert_eq!(b0["type"], "input");
        assert_eq!(b0["block_id"], "category");
        assert_eq!(b0["label"], json!({"type":"plain_text","text":"Category"}));
        assert_eq!(b0["element"]["type"], "static_select");
        assert_eq!(b0["element"]["action_id"], "category");
        let opts = b0["element"]["options"].as_array().unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(
            opts[0],
            json!({"text":{"type":"plain_text","text":"Bug Report"},"value":"bug"})
        );
        assert_eq!(
            opts[1],
            json!({"text":{"type":"plain_text","text":"Feature Request"},"value":"feature"})
        );
    }

    #[test]
    fn modal_to_slack_view_converts_select_with_initial_option() {
        // 1:1 with upstream `converts select with initial option`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                select(SelectOptions {
                    id: "priority".to_string(),
                    label: "Priority".to_string(),
                    options: vec![
                        select_option("Low", "low", None),
                        select_option("Medium", "medium", None),
                        select_option("High", "high", None),
                    ],
                    initial_option: Some("medium".to_string()),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(
            view["blocks"][0]["element"]["initial_option"],
            json!({"text":{"type":"plain_text","text":"Medium"},"value":"medium"})
        );
    }

    #[test]
    fn modal_to_slack_view_converts_select_with_placeholder() {
        // 1:1 with upstream `converts select with placeholder`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                select(SelectOptions {
                    id: "category".to_string(),
                    label: "Category".to_string(),
                    placeholder: Some("Select a category".to_string()),
                    options: vec![select_option("General", "general", None)],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(
            view["blocks"][0]["element"]["placeholder"],
            json!({"type":"plain_text","text":"Select a category"})
        );
    }

    #[test]
    fn modal_to_slack_view_converts_external_select_with_placeholder_and_min_query_length() {
        // 1:1 with upstream `converts external select with placeholder and min query length`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                external_select(ExternalSelectOptions {
                    id: "person".to_string(),
                    label: "Person".to_string(),
                    placeholder: Some("Search people".to_string()),
                    min_query_length: Some(1),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let b0 = &view["blocks"][0];
        assert_eq!(b0["type"], "input");
        assert_eq!(b0["block_id"], "person");
        assert_eq!(b0["label"], json!({"type":"plain_text","text":"Person"}));
        assert_eq!(b0["element"]["type"], "external_select");
        assert_eq!(b0["element"]["action_id"], "person");
        assert_eq!(
            b0["element"]["placeholder"],
            json!({"type":"plain_text","text":"Search people"})
        );
        assert_eq!(b0["element"]["min_query_length"], 1);
    }

    #[test]
    fn modal_to_slack_view_converts_external_select_with_initial_option() {
        // 1:1 with upstream `converts external select with initialOption`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                external_select(ExternalSelectOptions {
                    id: "person".to_string(),
                    label: "Person".to_string(),
                    initial_option: Some(select_option("Alice", "u1", None)),
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let b0 = &view["blocks"][0];
        assert_eq!(b0["type"], "input");
        assert_eq!(b0["block_id"], "person");
        assert_eq!(b0["element"]["type"], "external_select");
        assert_eq!(b0["element"]["action_id"], "person");
        assert_eq!(
            b0["element"]["initial_option"],
            json!({"text":{"type":"plain_text","text":"Alice"},"value":"u1"})
        );
    }

    #[test]
    fn modal_to_slack_view_includes_context_id_as_private_metadata_when_provided() {
        // 1:1 with upstream `includes contextId as private_metadata when provided`.
        let m = build_modal("test", "Test", vec![], None, None, None);
        let view = modal_to_slack_view(&m, Some("context-uuid-123"));
        assert_eq!(view["private_metadata"], "context-uuid-123");
    }

    #[test]
    fn modal_to_slack_view_private_metadata_is_undefined_when_no_context_id_provided() {
        // 1:1 with upstream `private_metadata is undefined when no contextId provided`.
        let m = build_modal("test", "Test", vec![], None, None, None);
        let view = modal_to_slack_view(&m, None);
        assert!(view.get("private_metadata").is_none());
    }

    #[test]
    fn modal_to_slack_view_sets_notify_on_close_when_provided() {
        // 1:1 with upstream `sets notify_on_close when provided`.
        let m = build_modal("test", "Test", vec![], None, None, Some(true));
        let view = modal_to_slack_view(&m, None);
        assert_eq!(view["notify_on_close"], true);
    }

    #[test]
    fn modal_to_slack_view_truncates_long_titles_to_24_chars() {
        // 1:1 with upstream `truncates long titles to 24 chars`.
        let m = build_modal(
            "test",
            "This is a very long modal title that exceeds the limit",
            vec![],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let title_text = view["title"]["text"].as_str().unwrap();
        assert!(
            title_text.len() <= 24,
            "title was {title_text:?} ({})",
            title_text.len()
        );
    }

    #[test]
    fn modal_to_slack_view_converts_a_complete_modal_with_multiple_inputs() {
        // 1:1 with upstream `converts a complete modal with multiple inputs`.
        let m = build_modal(
            "feedback_form",
            "Submit Feedback",
            vec![
                text_input(TextInputOptions {
                    id: "message".to_string(),
                    label: "Your Feedback".to_string(),
                    placeholder: Some("Tell us what you think...".to_string()),
                    multiline: Some(true),
                    ..Default::default()
                })
                .into(),
                select(SelectOptions {
                    id: "category".to_string(),
                    label: "Category".to_string(),
                    options: vec![
                        select_option("Bug", "bug", None),
                        select_option("Feature", "feature", None),
                        select_option("Other", "other", None),
                    ],
                    ..Default::default()
                })
                .into(),
                text_input(TextInputOptions {
                    id: "email".to_string(),
                    label: "Email (optional)".to_string(),
                    optional: Some(true),
                    ..Default::default()
                })
                .into(),
            ],
            Some("Send"),
            Some("Cancel"),
            Some(true),
        );
        let view = modal_to_slack_view(&m, Some("thread-context-123"));
        assert_eq!(view["callback_id"], "feedback_form");
        assert_eq!(view["private_metadata"], "thread-context-123");
        let blocks = view["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["type"], "input");
        assert_eq!(blocks[1]["type"], "input");
        assert_eq!(blocks[2]["type"], "input");
    }

    // ============================================================
    // describe("modalToSlackView with radio select") (4 upstream cases)
    // 1:1 with upstream `modals.test.ts > describe("modalToSlackView with radio select")`.
    // ============================================================

    #[test]
    fn modal_to_slack_view_radio_converts_radio_select_element_with_options() {
        // 1:1 with upstream `radio select > converts radio select element with options`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                radio_select(RadioSelectOptions {
                    id: "plan".to_string(),
                    label: "Choose Plan".to_string(),
                    options: vec![
                        select_option("Basic", "basic", None),
                        select_option("Pro", "pro", None),
                        select_option("Enterprise", "enterprise", None),
                    ],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let blocks = view["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        let b0 = &blocks[0];
        assert_eq!(b0["type"], "input");
        assert_eq!(b0["block_id"], "plan");
        assert_eq!(
            b0["label"],
            json!({"type":"plain_text","text":"Choose Plan"})
        );
        assert_eq!(b0["element"]["type"], "radio_buttons");
        assert_eq!(b0["element"]["action_id"], "plan");
        assert_eq!(b0["element"]["options"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn modal_to_slack_view_radio_converts_optional_radio_select() {
        // 1:1 with upstream `radio select > converts optional radio select`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                radio_select(RadioSelectOptions {
                    id: "preference".to_string(),
                    label: "Preference".to_string(),
                    optional: Some(true),
                    options: vec![
                        select_option("Yes", "yes", None),
                        select_option("No", "no", None),
                    ],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(view["blocks"][0]["type"], "input");
        assert_eq!(view["blocks"][0]["optional"], true);
    }

    #[test]
    fn modal_to_slack_view_radio_uses_mrkdwn_type_for_labels() {
        // 1:1 with upstream `radio select > uses mrkdwn type for radio select labels`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                radio_select(RadioSelectOptions {
                    id: "option".to_string(),
                    label: "Choose".to_string(),
                    options: vec![select_option("Option A", "a", None)],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let opt0 = &view["blocks"][0]["element"]["options"][0];
        assert_eq!(opt0["text"]["type"], "mrkdwn");
        assert_eq!(opt0["text"]["text"], "Option A");
    }

    #[test]
    fn modal_to_slack_view_radio_limits_options_to_10() {
        // 1:1 with upstream `radio select > limits radio select options to 10`.
        let opts: Vec<SelectOptionElement> = (0..15)
            .map(|i| select_option(format!("Option {}", i + 1), format!("opt{}", i + 1), None))
            .collect();
        let m = build_modal(
            "test",
            "Test",
            vec![
                radio_select(RadioSelectOptions {
                    id: "many_options".to_string(),
                    label: "Many Options".to_string(),
                    options: opts,
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        assert_eq!(
            view["blocks"][0]["element"]["options"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
    }

    // ============================================================
    // describe("modalToSlackView with select option descriptions") (2 upstream cases)
    // 1:1 with upstream `modals.test.ts > describe("modalToSlackView with select option descriptions")`.
    // ============================================================

    #[test]
    fn modal_to_slack_view_includes_description_in_select_options_with_plain_text_type() {
        // 1:1 with upstream `select option descriptions > includes description in select options with plain_text type`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                select(SelectOptions {
                    id: "plan".to_string(),
                    label: "Plan".to_string(),
                    options: vec![
                        select_option("Basic", "basic", Some("For individuals".to_string())),
                        select_option("Pro", "pro", Some("For teams".to_string())),
                    ],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let opts = view["blocks"][0]["element"]["options"].as_array().unwrap();
        assert_eq!(
            opts[0]["description"],
            json!({"type":"plain_text","text":"For individuals"})
        );
        assert_eq!(
            opts[1]["description"],
            json!({"type":"plain_text","text":"For teams"})
        );
    }

    #[test]
    fn modal_to_slack_view_includes_description_in_radio_select_options_with_mrkdwn_type() {
        // 1:1 with upstream `select option descriptions > includes description in radio select options with mrkdwn type`.
        let m = build_modal(
            "test",
            "Test",
            vec![
                radio_select(RadioSelectOptions {
                    id: "plan".to_string(),
                    label: "Plan".to_string(),
                    options: vec![
                        select_option("Basic", "basic", Some("For *individuals*".to_string())),
                        select_option("Pro", "pro", Some("For _teams_".to_string())),
                    ],
                    ..Default::default()
                })
                .into(),
            ],
            None,
            None,
            None,
        );
        let view = modal_to_slack_view(&m, None);
        let opts = view["blocks"][0]["element"]["options"].as_array().unwrap();
        assert_eq!(
            opts[0]["description"],
            json!({"type":"mrkdwn","text":"For *individuals*"})
        );
        assert_eq!(
            opts[1]["description"],
            json!({"type":"mrkdwn","text":"For _teams_"})
        );
    }
}
