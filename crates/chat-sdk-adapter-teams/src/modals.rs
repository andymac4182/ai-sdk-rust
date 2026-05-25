//! Teams dialog (task module) converter.
//!
//! 1:1 port of `packages/adapter-teams/src/modals.ts`:
//!
//! - [`modal_to_adaptive_card`] - Convert a `ModalElement` to an
//!   Adaptive Card JSON envelope for Teams task modules.
//! - [`parse_dialog_submit_values`] - Extract user input values
//!   from an Action.Submit data payload (stripping internal keys).
//! - [`modal_response_to_task_module_response`] - Convert a
//!   `ModalResponse`-shape value into the Teams `TaskModuleResponse`
//!   JSON shape.
//!
//! The upstream `ModalResponse` type lives in `chat` and is not yet
//! ported to `chat_sdk_chat` as a typed Rust enum. The Rust port
//! introduces a local [`ModalResponse`] enum modeled on the upstream
//! discriminated-union (`close | update | push | errors`); when the
//! cross-platform port lands it can be migrated to the shared type
//! without changing the wire shape.

use chat_sdk_adapter_shared::card_utils::{PlatformName, map_button_style};
use chat_sdk_chat::cards::{ButtonStyle, FieldsElement, TextElement, TextStyle};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::modals::{
    ModalChild, ModalElement, RadioSelectElement, SelectElement, TextInputElement,
};
use serde_json::{Map, Value, json};

/// 1:1 with upstream private `ADAPTIVE_CARD_SCHEMA =
/// "http://adaptivecards.io/schemas/adaptive-card.json"`.
pub const ADAPTIVE_CARD_SCHEMA: &str = "http://adaptivecards.io/schemas/adaptive-card.json";

/// 1:1 with upstream private `ADAPTIVE_CARD_VERSION = "1.4"`.
pub const ADAPTIVE_CARD_VERSION: &str = "1.4";

fn convert_emoji(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Teams, None)
}

/// Convert a [`ModalElement`] to an Adaptive Card JSON envelope for
/// use inside a Teams task module. 1:1 port of upstream
/// `modalToAdaptiveCard(modal, contextId, callbackId)`.
///
/// - Walks `modal.children` through
///   [`modal_child_to_adaptive_elements`] which dispatches on the
///   six `ModalChild` variants (text_input / select / radio_select /
///   text / fields, plus `external_select` which throws upstream).
/// - Emits a card-level `actions: [SubmitAction]` with the submit
///   data carrying `__contextId` + `__callbackId` so the round-trip
///   from the user's submit back to the handler retains the context.
/// - `submitLabel` defaults to `"Submit"` (matches upstream).
/// - The "primary" submit button gets the Teams-flavored style from
///   [`map_button_style`] which returns `"positive"` (`Option<&str>`).
pub fn modal_to_adaptive_card(modal: &ModalElement, context_id: &str, callback_id: &str) -> Value {
    let mut body: Vec<Value> = Vec::new();
    for child in &modal.children {
        body.extend(modal_child_to_adaptive_elements(child));
    }

    let submit_data = json!({
        "__contextId": context_id,
        "__callbackId": callback_id,
    });

    let title = modal
        .submit_label
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("Submit");

    let mut submit = json!({
        "type": "Action.Submit",
        "title": title,
        "data": submit_data,
    });
    if let Some(style) = map_button_style(Some(ButtonStyle::Primary), PlatformName::Teams) {
        submit["style"] = Value::String(style.to_string());
    }

    json!({
        "type": "AdaptiveCard",
        "$schema": ADAPTIVE_CARD_SCHEMA,
        "version": ADAPTIVE_CARD_VERSION,
        "body": body,
        "actions": [submit],
    })
}

/// Dispatch a [`ModalChild`] to its Adaptive Card body elements.
/// 1:1 with upstream `modalChildToAdaptiveElements(child)`.
///
/// `external_select` is unsupported (upstream throws); the Rust
/// port currently returns an empty vector to keep the converter
/// total — an adopter that needs the throwing behavior can pre-walk
/// children. (Slice 411 enumerates the upstream throw shape as
/// js-only-documented since the Rust port surfaces it differently.)
pub fn modal_child_to_adaptive_elements(child: &ModalChild) -> Vec<Value> {
    match child {
        ModalChild::TextInput(t) => vec![text_input_to_adaptive(t)],
        ModalChild::Select(s) => vec![select_to_adaptive(s)],
        ModalChild::RadioSelect(r) => vec![radio_select_to_adaptive(r)],
        ModalChild::Text(t) => vec![text_to_adaptive(t)],
        ModalChild::Fields(f) => vec![fields_to_adaptive(f)],
        ModalChild::ExternalSelect(_) => Vec::new(),
    }
}

fn text_input_to_adaptive(input: &TextInputElement) -> Value {
    let mut obj = json!({
        "type": "Input.Text",
        "id": input.id,
        "label": convert_emoji(&input.label),
        "isMultiline": input.multiline.unwrap_or(false),
        "isRequired": !input.optional.unwrap_or(false),
    });
    if let Some(ph) = &input.placeholder {
        obj["placeholder"] = Value::String(ph.clone());
    }
    if let Some(v) = &input.initial_value {
        obj["value"] = Value::String(v.clone());
    }
    if let Some(m) = input.max_length {
        obj["maxLength"] = Value::Number(m.into());
    }
    obj
}

fn select_to_adaptive(select: &SelectElement) -> Value {
    let choices: Vec<Value> = select
        .options
        .iter()
        .map(|o| json!({ "title": convert_emoji(&o.label), "value": o.value }))
        .collect();
    let mut obj = json!({
        "type": "Input.ChoiceSet",
        "id": select.id,
        "label": convert_emoji(&select.label),
        "style": "compact",
        "isRequired": !select.optional.unwrap_or(false),
        "choices": choices,
    });
    if let Some(ph) = &select.placeholder {
        obj["placeholder"] = Value::String(ph.clone());
    }
    if let Some(v) = &select.initial_option {
        obj["value"] = Value::String(v.clone());
    }
    obj
}

fn radio_select_to_adaptive(radio: &RadioSelectElement) -> Value {
    let choices: Vec<Value> = radio
        .options
        .iter()
        .map(|o| json!({ "title": convert_emoji(&o.label), "value": o.value }))
        .collect();
    let mut obj = json!({
        "type": "Input.ChoiceSet",
        "id": radio.id,
        "label": convert_emoji(&radio.label),
        "style": "expanded",
        "isRequired": !radio.optional.unwrap_or(false),
        "choices": choices,
    });
    if let Some(v) = &radio.initial_option {
        obj["value"] = Value::String(v.clone());
    }
    obj
}

fn text_to_adaptive(text: &TextElement) -> Value {
    let mut obj = json!({
        "type": "TextBlock",
        "text": convert_emoji(&text.content),
        "wrap": true,
    });
    match text.style {
        Some(TextStyle::Bold) => {
            obj["weight"] = Value::String("Bolder".to_string());
        }
        Some(TextStyle::Muted) => {
            obj["isSubtle"] = Value::Bool(true);
        }
        _ => {}
    }
    obj
}

fn fields_to_adaptive(fields: &FieldsElement) -> Value {
    let facts: Vec<Value> = fields
        .children
        .iter()
        .map(|f| {
            json!({
                "title": convert_emoji(&f.label),
                "value": convert_emoji(&f.value),
            })
        })
        .collect();
    json!({ "type": "FactSet", "facts": facts })
}

/// Decoded shape returned by [`parse_dialog_submit_values`]. 1:1
/// with upstream `interface DialogSubmitValues { callbackId,
/// contextId, values }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogSubmitValues {
    /// `__callbackId` from the submit payload, when present.
    pub callback_id: Option<String>,
    /// `__contextId` from the submit payload, when present.
    pub context_id: Option<String>,
    /// All other string-valued keys, with the three internal keys
    /// (`__contextId`, `__callbackId`, `msteams`) stripped.
    pub values: Map<String, Value>,
}

/// Extract user input values from an Action.Submit data payload,
/// stripping out internal keys (`__contextId`, `__callbackId`,
/// `msteams`). 1:1 with upstream
/// `parseDialogSubmitValues(data)`.
pub fn parse_dialog_submit_values(data: Option<&Value>) -> DialogSubmitValues {
    let Some(obj) = data.and_then(Value::as_object) else {
        return DialogSubmitValues::default();
    };

    let context_id = obj
        .get("__contextId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let callback_id = obj
        .get("__callbackId")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut values = Map::new();
    for (key, val) in obj {
        if key == "__contextId" || key == "__callbackId" || key == "msteams" {
            continue;
        }
        if val.is_string() {
            values.insert(key.clone(), val.clone());
        }
    }

    DialogSubmitValues {
        callback_id,
        context_id,
        values,
    }
}

/// 1:1 with upstream's `ModalResponse` discriminated union. The
/// upstream type is sourced from `chat`; the Rust port keeps a local
/// definition until the cross-platform port lands so the converter
/// has the correct shape to dispatch on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalResponse {
    /// `{ action: "close" }` — closes the dialog with empty body.
    Close,
    /// `{ action: "update", modal }` — re-render the dialog with
    /// the updated modal.
    Update(ModalElement),
    /// `{ action: "push", modal }` — Teams has no dialog stacking,
    /// so upstream warns and falls back to `update`. The Rust port
    /// surfaces the warning via the optional `logger` callback when
    /// provided.
    Push(ModalElement),
    /// `{ action: "errors", errors }` — render an error card.
    Errors(Map<String, Value>),
}

/// 1:1 with upstream `TaskModuleResponse` envelope. Modeled as a
/// thin alias over [`serde_json::Value`] because the upstream type
/// lives in `@microsoft/teams.api` and the Rust port has no Teams
/// SDK dependency. The shape is well-defined by upstream's
/// `buildContinueResponse` body construction.
pub type TaskModuleResponse = Value;

/// 1:1 port of upstream
/// `modalResponseToTaskModuleResponse(response, logger, contextId)`.
///
/// - `None` -> `None` (signals "close dialog" via empty HTTP body).
/// - `Close` -> `None`.
/// - `Update(modal)` -> `task: { type: "continue", value: ... }`.
/// - `Push(modal)` -> same as `Update`, with a logger warning.
/// - `Errors(map)` -> a render-shape error card with one
///   `**field**: msg` `TextBlock` per error.
///
/// The `logger` callback (when present) is invoked for the `Push`
/// branch with a one-argument message — the upstream second-arg
/// metadata object is omitted because the Rust port has no
/// `Logger` trait yet (per `port-chat-sdk.md` slice 447).
pub fn modal_response_to_task_module_response<F: FnOnce(&str)>(
    response: Option<&ModalResponse>,
    logger: Option<F>,
    context_id: &str,
) -> Option<TaskModuleResponse> {
    let response = response?;
    match response {
        ModalResponse::Close => None,
        ModalResponse::Update(modal) => Some(build_continue_response(modal, context_id)),
        ModalResponse::Push(modal) => {
            if let Some(log) = logger {
                log("Teams does not support dialog stacking (push). Falling back to update.");
            }
            Some(build_continue_response(modal, context_id))
        }
        ModalResponse::Errors(errors) => Some(build_errors_response(errors)),
    }
}

fn build_continue_response(modal: &ModalElement, context_id: &str) -> TaskModuleResponse {
    let card = modal_to_adaptive_card(modal, context_id, &modal.callback_id);
    json!({
        "task": {
            "type": "continue",
            "value": {
                "title": modal.title,
                "card": {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": card,
                }
            }
        }
    })
}

fn build_errors_response(errors: &Map<String, Value>) -> TaskModuleResponse {
    let mut body: Vec<Value> = Vec::new();
    body.push(json!({
        "type": "TextBlock",
        "text": "Please fix the following errors:",
        "weight": "Bolder",
        "wrap": true,
    }));
    for (field, msg) in errors {
        let msg_str = msg.as_str().unwrap_or("");
        body.push(json!({
            "type": "TextBlock",
            "text": format!("**{field}**: {msg_str}"),
            "wrap": true,
            "color": "Attention",
        }));
    }

    let card = json!({
        "type": "AdaptiveCard",
        "$schema": ADAPTIVE_CARD_SCHEMA,
        "version": ADAPTIVE_CARD_VERSION,
        "body": body,
    });

    json!({
        "task": {
            "type": "continue",
            "value": {
                "title": "Validation Error",
                "card": {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": card,
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{FieldElement, FieldKind, FieldsKind, TextKind};
    use chat_sdk_chat::modals::{
        ModalKind, RadioSelectKind, SelectKind, SelectOptionElement, TextInputKind,
    };

    fn make_modal(submit_label: Option<&str>, children: Vec<ModalChild>) -> ModalElement {
        ModalElement {
            callback_id: "cb-1".to_string(),
            callback_url: None,
            children,
            close_label: None,
            notify_on_close: None,
            private_metadata: None,
            submit_label: submit_label.map(str::to_string),
            title: "Test Modal".to_string(),
            kind: ModalKind::default(),
        }
    }

    fn text_input(id: &str, label: &str, placeholder: Option<&str>) -> TextInputElement {
        TextInputElement {
            id: id.to_string(),
            initial_value: None,
            label: label.to_string(),
            max_length: None,
            multiline: None,
            optional: None,
            placeholder: placeholder.map(str::to_string),
            kind: TextInputKind::default(),
        }
    }

    fn opt(label: &str, value: &str) -> SelectOptionElement {
        SelectOptionElement {
            label: label.to_string(),
            value: value.to_string(),
            description: None,
        }
    }

    // ==================================================================
    // describe("modalToAdaptiveCard") — 4 upstream cases.
    // ==================================================================

    // 1:1 with upstream modals.test.ts:43 > "produces a valid Adaptive Card structure".
    #[test]
    fn modal_to_adaptive_card_produces_a_valid_adaptive_card_structure() {
        let card = modal_to_adaptive_card(&make_modal(None, Vec::new()), "ctx-1", "cb-1");
        assert_eq!(card["type"], "AdaptiveCard");
        assert_eq!(card["$schema"], ADAPTIVE_CARD_SCHEMA);
        assert_eq!(card["version"], ADAPTIVE_CARD_VERSION);
        assert!(card["body"].is_array());
    }

    // 1:1 with upstream modals.test.ts:54 > "includes contextId and callbackId in submit action data".
    #[test]
    fn modal_to_adaptive_card_includes_context_id_and_callback_id_in_submit_action_data() {
        let card = modal_to_adaptive_card(&make_modal(None, Vec::new()), "ctx-1", "cb-1");
        let actions = card["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        let action = &actions[0];
        assert_eq!(action["data"]["__contextId"], "ctx-1");
        assert_eq!(action["data"]["__callbackId"], "cb-1");
    }

    // 1:1 with upstream modals.test.ts:63 > "uses custom submitLabel when provided".
    #[test]
    fn modal_to_adaptive_card_uses_custom_submit_label_when_provided() {
        let card = modal_to_adaptive_card(&make_modal(Some("Send it"), Vec::new()), "ctx", "cb");
        assert_eq!(card["actions"][0]["title"], "Send it");
    }

    // 1:1 with upstream modals.test.ts:71 > "defaults submitLabel to 'Submit'".
    #[test]
    fn modal_to_adaptive_card_defaults_submit_label_to_submit() {
        let card = modal_to_adaptive_card(&make_modal(None, Vec::new()), "ctx", "cb");
        assert_eq!(card["actions"][0]["title"], "Submit");
    }

    // ==================================================================
    // describe("modal child element conversion") — 5 upstream cases.
    // ==================================================================

    // 1:1 with upstream modals.test.ts:84 > "converts text_input to TextInput".
    #[test]
    fn modal_child_conversion_converts_text_input_to_text_input() {
        let modal = make_modal(
            None,
            vec![text_input("name", "Your Name", Some("Enter name")).into()],
        );
        let card = modal_to_adaptive_card(&modal, "ctx", "cb");
        let body = card["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Input.Text");
        assert_eq!(body[0]["id"], "name");
        assert_eq!(body[0]["label"], "Your Name");
        assert_eq!(body[0]["placeholder"], "Enter name");
        assert_eq!(body[0]["isRequired"], true);
        assert_eq!(body[0]["isMultiline"], false);
    }

    // 1:1 with upstream modals.test.ts:107 > "converts select to ChoiceSetInput with compact style".
    #[test]
    fn modal_child_conversion_converts_select_to_compact_choice_set_input() {
        let select = SelectElement {
            id: "color".to_string(),
            initial_option: None,
            label: "Favorite Color".to_string(),
            optional: None,
            options: vec![opt("Red", "red"), opt("Blue", "blue")],
            placeholder: None,
            kind: SelectKind::default(),
        };
        let modal = make_modal(None, vec![select.into()]);
        let card = modal_to_adaptive_card(&modal, "ctx", "cb");
        let body = card["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Input.ChoiceSet");
        assert_eq!(body[0]["id"], "color");
        assert_eq!(body[0]["label"], "Favorite Color");
        assert_eq!(body[0]["style"], "compact");
        assert_eq!(body[0]["isRequired"], true);
        let choices = body[0]["choices"].as_array().unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0]["title"], "Red");
        assert_eq!(choices[0]["value"], "red");
    }

    // 1:1 with upstream modals.test.ts:137 > "converts radio_select to ChoiceSetInput with expanded style".
    #[test]
    fn modal_child_conversion_converts_radio_select_to_expanded_choice_set_input() {
        let radio = RadioSelectElement {
            id: "size".to_string(),
            initial_option: None,
            label: "Size".to_string(),
            optional: None,
            options: vec![opt("Small", "sm"), opt("Large", "lg")],
            kind: RadioSelectKind::default(),
        };
        let modal = make_modal(None, vec![radio.into()]);
        let card = modal_to_adaptive_card(&modal, "ctx", "cb");
        let body = card["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Input.ChoiceSet");
        assert_eq!(body[0]["id"], "size");
        assert_eq!(body[0]["label"], "Size");
        assert_eq!(body[0]["style"], "expanded");
        assert_eq!(body[0]["isRequired"], true);
    }

    // 1:1 with upstream modals.test.ts:162 > "converts text to TextBlock with style support".
    #[test]
    fn modal_child_conversion_converts_text_to_text_block_with_style_support() {
        let modal = make_modal(
            None,
            vec![
                TextElement {
                    content: "Hello".to_string(),
                    style: None,
                    kind: TextKind::default(),
                }
                .into(),
                TextElement {
                    content: "Bold text".to_string(),
                    style: Some(TextStyle::Bold),
                    kind: TextKind::default(),
                }
                .into(),
                TextElement {
                    content: "Muted text".to_string(),
                    style: Some(TextStyle::Muted),
                    kind: TextKind::default(),
                }
                .into(),
            ],
        );
        let card = modal_to_adaptive_card(&modal, "ctx", "cb");
        let body = card["body"].as_array().unwrap();
        assert_eq!(body.len(), 3);
        assert_eq!(body[0]["type"], "TextBlock");
        assert_eq!(body[0]["text"], "Hello");
        assert_eq!(body[0]["wrap"], true);
        assert_eq!(body[1]["weight"], "Bolder");
        assert_eq!(body[2]["isSubtle"], true);
    }

    // 1:1 with upstream modals.test.ts:188 > "converts fields to FactSet".
    #[test]
    fn modal_child_conversion_converts_fields_to_fact_set() {
        let fields = FieldsElement {
            children: vec![
                FieldElement {
                    label: "Name".to_string(),
                    value: "Alice".to_string(),
                    kind: FieldKind::default(),
                },
                FieldElement {
                    label: "Role".to_string(),
                    value: "Engineer".to_string(),
                    kind: FieldKind::default(),
                },
            ],
            kind: FieldsKind::default(),
        };
        let modal = make_modal(None, vec![fields.into()]);
        let card = modal_to_adaptive_card(&modal, "ctx", "cb");
        let body = card["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "FactSet");
        let facts = body[0]["facts"].as_array().unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0]["title"], "Name");
        assert_eq!(facts[0]["value"], "Alice");
    }

    // ==================================================================
    // describe("parseDialogSubmitValues") — 2 upstream cases.
    // ==================================================================

    // 1:1 with upstream modals.test.ts:214 > "extracts callbackId, contextId, and user values".
    #[test]
    fn parse_dialog_submit_values_extracts_callback_id_context_id_and_user_values() {
        let data = json!({
            "__contextId": "ctx-1",
            "__callbackId": "cb-1",
            "msteams": { "some": "data" },
            "name": "Alice",
            "color": "blue",
        });
        let result = parse_dialog_submit_values(Some(&data));
        assert_eq!(result.context_id.as_deref(), Some("ctx-1"));
        assert_eq!(result.callback_id.as_deref(), Some("cb-1"));
        assert_eq!(
            result.values.get("name").and_then(Value::as_str),
            Some("Alice")
        );
        assert_eq!(
            result.values.get("color").and_then(Value::as_str),
            Some("blue")
        );
        assert_eq!(result.values.len(), 2);
    }

    // 1:1 with upstream modals.test.ts:228 > "returns empty result for undefined data".
    #[test]
    fn parse_dialog_submit_values_returns_empty_result_for_undefined_data() {
        let result = parse_dialog_submit_values(None);
        assert!(result.context_id.is_none());
        assert!(result.callback_id.is_none());
        assert!(result.values.is_empty());
    }

    // ==================================================================
    // describe("modalResponseToTaskModuleResponse") — 5 upstream cases.
    // ==================================================================

    type NoopLogger = fn(&str);

    // 1:1 with upstream modals.test.ts:242 > "returns undefined for undefined response".
    #[test]
    fn modal_response_to_task_module_response_returns_undefined_for_undefined_response() {
        let result = modal_response_to_task_module_response(None, None::<NoopLogger>, "ctx");
        assert!(result.is_none());
    }

    // 1:1 with upstream modals.test.ts:246 > "returns undefined for close action".
    #[test]
    fn modal_response_to_task_module_response_returns_undefined_for_close_action() {
        let result = modal_response_to_task_module_response(
            Some(&ModalResponse::Close),
            None::<NoopLogger>,
            "ctx",
        );
        assert!(result.is_none());
    }

    // 1:1 with upstream modals.test.ts:251 > "returns continue response for update action".
    #[test]
    fn modal_response_to_task_module_response_returns_continue_response_for_update_action() {
        let mut modal = make_modal(None, Vec::new());
        modal.title = "Updated".to_string();
        let result = modal_response_to_task_module_response(
            Some(&ModalResponse::Update(modal)),
            None::<NoopLogger>,
            "ctx-1",
        )
        .expect("result is Some");
        assert_eq!(result["task"]["type"], "continue");
        assert_eq!(result["task"]["value"]["title"], "Updated");
        assert_eq!(
            result["task"]["value"]["card"]["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );
    }

    // 1:1 with upstream modals.test.ts:272 > "falls back to continue and warns for push action".
    #[test]
    fn modal_response_to_task_module_response_falls_back_to_continue_and_warns_for_push_action() {
        use std::cell::RefCell;
        let mut modal = make_modal(None, Vec::new());
        modal.title = "Pushed".to_string();
        let calls = RefCell::new(Vec::<String>::new());
        let logger = |msg: &str| calls.borrow_mut().push(msg.to_string());
        let result = modal_response_to_task_module_response(
            Some(&ModalResponse::Push(modal)),
            Some(logger),
            "ctx-1",
        )
        .expect("result is Some");
        assert_eq!(result["task"]["type"], "continue");
        let recorded = calls.borrow();
        assert_eq!(recorded.len(), 1);
        assert!(
            recorded[0].contains("does not support dialog stacking"),
            "got: {}",
            recorded[0]
        );
    }

    // 1:1 with upstream modals.test.ts:287 > "returns error card for errors action".
    #[test]
    fn modal_response_to_task_module_response_returns_error_card_for_errors_action() {
        let mut errors = Map::new();
        errors.insert("name".to_string(), Value::String("Required".to_string()));
        errors.insert(
            "email".to_string(),
            Value::String("Invalid format".to_string()),
        );
        let result = modal_response_to_task_module_response(
            Some(&ModalResponse::Errors(errors)),
            None::<NoopLogger>,
            "ctx",
        )
        .expect("result is Some");
        assert_eq!(result["task"]["type"], "continue");
        assert_eq!(result["task"]["value"]["title"], "Validation Error");
        assert_eq!(
            result["task"]["value"]["card"]["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );
        let body = result["task"]["value"]["card"]["content"]["body"]
            .as_array()
            .unwrap();
        assert!(body.len() >= 3);
        assert!(
            body[0]["text"].as_str().unwrap().contains("Please fix"),
            "got: {}",
            body[0]["text"]
        );
    }
}
