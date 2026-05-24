//! Modal form-dialog elements.
//!
//! 1:1 port (in progress) of `packages/chat/src/modals.ts`. Provides
//! data types + builder functions for modal dialogs that adapters
//! convert to platform-specific surfaces (Slack views, Teams task
//! modules, etc.).
//!
//! **What this slice ships (slice 33):**
//!
//! - Leaf interactive element structs: `TextInputElement`, `SelectElement`,
//!   `ExternalSelectElement`, `SelectOptionElement`, `RadioSelectElement`.
//! - Builders: [`text_input`], [`select`], [`select_option`],
//!   [`external_select`], [`radio_select`].
//!
//! Each element ships with a per-struct discriminator unit-enum (e.g.
//! `TextInputKind::TextInput` -> wire `"text_input"`) mirroring upstream's
//! `type` literal field. The future `ModalChild` discriminated union
//! will collapse these into a tagged enum without changing the
//! per-struct wire shape.
//!
//! **What remains for follow-up slices:**
//!
//! - `ModalElement` + `ModalOptions` + the `Modal` builder.
//! - `ModalChild` discriminated union + `VALID_MODAL_CHILD_TYPES`
//!   constant + `is_modal_element` + `filter_modal_children`.
//! - `fromReactModalElement` (JSX side, `js-only-documented`).
//!
//! Slice 33 lands these specifically because `cards::ActionsElement`
//! requires `SelectElement` and `RadioSelectElement` to be visible — see
//! the slice-32 cards module header for the deferred surface.

use crate::cards::{FieldsElement, TextElement};
use serde::{Deserialize, Serialize};

/// Set of `type` discriminator strings the [`ModalChild`] union accepts.
/// 1:1 port of upstream
/// `export const VALID_MODAL_CHILD_TYPES = ["text_input", "select", "external_select", "radio_select", "text", "fields"]`.
pub const VALID_MODAL_CHILD_TYPES: &[&str] = &[
    "text_input",
    "select",
    "external_select",
    "radio_select",
    "text",
    "fields",
];

/// Single-option payload for `SelectElement` / `RadioSelectElement` /
/// `ExternalSelectElement::initialOption`. 1:1 port of upstream
/// `interface SelectOptionElement`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelectOptionElement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub label: String,
    pub value: String,
}

/// Free-form text input. 1:1 port of upstream `interface TextInputElement`.
/// Discriminator `"text_input"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextInputElement {
    pub id: String,
    #[serde(
        rename = "initialValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_value: Option<String>,
    pub label: String,
    #[serde(rename = "maxLength", default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(rename = "type")]
    pub kind: TextInputKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TextInputKind {
    #[default]
    #[serde(rename = "text_input")]
    TextInput,
}

/// Single-select dropdown. 1:1 port of upstream `interface SelectElement`.
/// Discriminator `"select"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelectElement {
    pub id: String,
    #[serde(
        rename = "initialOption",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_option: Option<String>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    pub options: Vec<SelectOptionElement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(rename = "type")]
    pub kind: SelectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SelectKind {
    #[default]
    #[serde(rename = "select")]
    Select,
}

/// External-source single-select. 1:1 port of upstream
/// `interface ExternalSelectElement`. Discriminator `"external_select"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalSelectElement {
    pub id: String,
    #[serde(
        rename = "initialOption",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_option: Option<SelectOptionElement>,
    pub label: String,
    #[serde(
        rename = "minQueryLength",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_query_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(rename = "type")]
    pub kind: ExternalSelectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ExternalSelectKind {
    #[default]
    #[serde(rename = "external_select")]
    ExternalSelect,
}

/// Radio-button single-select. 1:1 port of upstream
/// `interface RadioSelectElement`. Discriminator `"radio_select"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RadioSelectElement {
    pub id: String,
    #[serde(
        rename = "initialOption",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_option: Option<String>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    pub options: Vec<SelectOptionElement>,
    #[serde(rename = "type")]
    pub kind: RadioSelectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RadioSelectKind {
    #[default]
    #[serde(rename = "radio_select")]
    RadioSelect,
}

/// Children allowed inside a [`ModalElement`]. 1:1 port of upstream
/// `export type ModalChild = TextInputElement | SelectElement |
/// ExternalSelectElement | RadioSelectElement | TextElement |
/// FieldsElement`. Modeled `#[serde(untagged)]` over the six variant
/// structs — each carries its own discriminator field, so serde
/// disambiguates without an outer wrapper. `From<T>` impls land for
/// every variant to keep call sites readable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModalChild {
    TextInput(TextInputElement),
    Select(SelectElement),
    ExternalSelect(ExternalSelectElement),
    RadioSelect(RadioSelectElement),
    Text(TextElement),
    Fields(FieldsElement),
}

impl From<TextInputElement> for ModalChild {
    fn from(value: TextInputElement) -> Self {
        Self::TextInput(value)
    }
}
impl From<SelectElement> for ModalChild {
    fn from(value: SelectElement) -> Self {
        Self::Select(value)
    }
}
impl From<ExternalSelectElement> for ModalChild {
    fn from(value: ExternalSelectElement) -> Self {
        Self::ExternalSelect(value)
    }
}
impl From<RadioSelectElement> for ModalChild {
    fn from(value: RadioSelectElement) -> Self {
        Self::RadioSelect(value)
    }
}
impl From<TextElement> for ModalChild {
    fn from(value: TextElement) -> Self {
        Self::Text(value)
    }
}
impl From<FieldsElement> for ModalChild {
    fn from(value: FieldsElement) -> Self {
        Self::Fields(value)
    }
}

/// Root modal element. 1:1 port of upstream `interface ModalElement`.
/// Discriminator `"modal"`. `title` and `callback_id` are required;
/// every other field is optional and elided from the wire when None.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModalElement {
    #[serde(rename = "callbackId")]
    pub callback_id: String,
    /// URL to POST form values to when this modal is submitted.
    #[serde(
        rename = "callbackUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub callback_url: Option<String>,
    pub children: Vec<ModalChild>,
    #[serde(
        rename = "closeLabel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub close_label: Option<String>,
    #[serde(
        rename = "notifyOnClose",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub notify_on_close: Option<bool>,
    /// Arbitrary string carried through the modal lifecycle (e.g. a
    /// serialized context JSON blob).
    #[serde(
        rename = "privateMetadata",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub private_metadata: Option<String>,
    #[serde(
        rename = "submitLabel",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub submit_label: Option<String>,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: ModalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ModalKind {
    #[default]
    #[serde(rename = "modal")]
    Modal,
}

/// Type guard for [`ModalElement`]. 1:1 port of upstream
/// `isModalElement(value: unknown): value is ModalElement`.
pub fn is_modal_element(value: &serde_json::Value) -> bool {
    value
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == "modal")
}

/// Read the `type` discriminator off any JSON object. Returns `None`
/// when the input isn't an object or carries no `type` field. Mirrors
/// upstream's inline `value.type` access used by the renderer's
/// `switch (child.type)` dispatch on modal children.
pub fn modal_child_kind(value: &serde_json::Value) -> Option<&str> {
    value.get("type").and_then(serde_json::Value::as_str)
}

/// Predicate: does the JSON value look like a [`TextInputElement`]?
/// 1:1 with upstream's inline `child.type === "text_input"` check.
pub fn is_text_input_element(value: &serde_json::Value) -> bool {
    modal_child_kind(value) == Some("text_input")
}

/// Predicate: does the JSON value look like a [`SelectElement`]? 1:1
/// with upstream's inline `child.type === "select"` check.
pub fn is_select_element(value: &serde_json::Value) -> bool {
    modal_child_kind(value) == Some("select")
}

/// Predicate: does the JSON value look like an
/// [`ExternalSelectElement`]? 1:1 with upstream's inline
/// `child.type === "external_select"` check.
pub fn is_external_select_element(value: &serde_json::Value) -> bool {
    modal_child_kind(value) == Some("external_select")
}

/// Predicate: does the JSON value look like a [`RadioSelectElement`]?
/// 1:1 with upstream's inline `child.type === "radio_select"` check.
pub fn is_radio_select_element(value: &serde_json::Value) -> bool {
    modal_child_kind(value) == Some("radio_select")
}

/// Predicate: does the JSON value look like a modal-level
/// [`TextElement`]? 1:1 with upstream's inline `child.type === "text"`
/// check used by the modal renderer.
pub fn is_modal_text_element(value: &serde_json::Value) -> bool {
    modal_child_kind(value) == Some("text")
}

/// Predicate: does the JSON value look like a [`FieldsElement`]? 1:1
/// with upstream's inline `child.type === "fields"` check.
pub fn is_modal_fields_element(value: &serde_json::Value) -> bool {
    modal_child_kind(value) == Some("fields")
}

/// Predicate: does the JSON value have one of
/// [`VALID_MODAL_CHILD_TYPES`]? 1:1 with the upstream inline
/// `VALID_MODAL_CHILD_TYPES.includes(child.type)` filter inside
/// [`filter_modal_children`]; exposed so adapter callsites can apply
/// the same check without going through the JSON->struct path.
pub fn is_valid_modal_child(value: &serde_json::Value) -> bool {
    modal_child_kind(value).is_some_and(|s| VALID_MODAL_CHILD_TYPES.contains(&s))
}

/// Filter a heterogeneous array of JSON values down to the subset that
/// parse as [`ModalChild`] (i.e. their `type` field is in
/// [`VALID_MODAL_CHILD_TYPES`]). 1:1 port of upstream
/// `filterModalChildren(children: unknown[]): ModalChild[]`.
///
/// Upstream `console.warn`s when any child is dropped; the Rust port
/// surfaces the same information via the returned tuple's `dropped`
/// count so callers can log or surface a warning through their own
/// [`crate::logger::Logger`] instance instead of an SDK-side hard-coded
/// `console.warn`.
///
/// Returns `(kept, dropped_count)` where `kept` is the validated
/// children and `dropped_count` is the number of inputs whose `type`
/// was not in [`VALID_MODAL_CHILD_TYPES`] (or which weren't JSON
/// objects).
pub fn filter_modal_children(children: &[serde_json::Value]) -> (Vec<ModalChild>, usize) {
    let mut kept: Vec<ModalChild> = Vec::new();
    let mut dropped = 0usize;
    for child in children {
        let valid = child
            .get("type")
            .and_then(|v| v.as_str())
            .is_some_and(|s| VALID_MODAL_CHILD_TYPES.contains(&s));
        if !valid {
            dropped += 1;
            continue;
        }
        match serde_json::from_value::<ModalChild>(child.clone()) {
            Ok(modal_child) => kept.push(modal_child),
            Err(_) => dropped += 1,
        }
    }
    (kept, dropped)
}

// ============================================================================
// Builders
// ============================================================================

/// Options for [`modal`]. Mirrors upstream `interface ModalOptions`.
#[derive(Debug, Default, Clone)]
pub struct ModalOptions {
    pub callback_id: String,
    pub callback_url: Option<String>,
    pub children: Option<Vec<ModalChild>>,
    pub close_label: Option<String>,
    pub notify_on_close: Option<bool>,
    pub private_metadata: Option<String>,
    pub submit_label: Option<String>,
    pub title: String,
}

/// 1:1 port of upstream `Modal(options): ModalElement`. The upstream
/// `children ?? []` default lands here as
/// `options.children.unwrap_or_default()`.
pub fn modal(options: ModalOptions) -> ModalElement {
    ModalElement {
        callback_id: options.callback_id,
        callback_url: options.callback_url,
        children: options.children.unwrap_or_default(),
        close_label: options.close_label,
        notify_on_close: options.notify_on_close,
        private_metadata: options.private_metadata,
        submit_label: options.submit_label,
        title: options.title,
        kind: ModalKind::Modal,
    }
}

/// Options for [`text_input`]. Mirrors upstream `interface TextInputOptions`.
#[derive(Debug, Default, Clone)]
pub struct TextInputOptions {
    pub id: String,
    pub initial_value: Option<String>,
    pub label: String,
    pub max_length: Option<u32>,
    pub multiline: Option<bool>,
    pub optional: Option<bool>,
    pub placeholder: Option<String>,
}

/// 1:1 port of upstream `TextInput(options): TextInputElement`.
pub fn text_input(options: TextInputOptions) -> TextInputElement {
    TextInputElement {
        id: options.id,
        initial_value: options.initial_value,
        label: options.label,
        max_length: options.max_length,
        multiline: options.multiline,
        optional: options.optional,
        placeholder: options.placeholder,
        kind: TextInputKind::TextInput,
    }
}

/// Options for [`select`]. Mirrors upstream `interface SelectOptions`.
#[derive(Debug, Default, Clone)]
pub struct SelectOptions {
    pub id: String,
    pub initial_option: Option<String>,
    pub label: String,
    pub optional: Option<bool>,
    pub options: Vec<SelectOptionElement>,
    pub placeholder: Option<String>,
}

/// 1:1 port of upstream `Select(options): SelectElement`.
///
/// **Validation:** panics when `options.options` is empty, matching
/// upstream's `if (!options.options.length) throw new Error("Select
/// requires at least one option")`. An empty select is structurally
/// invalid; adapter renderers assume at least one option.
pub fn select(options: SelectOptions) -> SelectElement {
    assert!(
        !options.options.is_empty(),
        "Select requires at least one option"
    );
    SelectElement {
        id: options.id,
        initial_option: options.initial_option,
        label: options.label,
        optional: options.optional,
        options: options.options,
        placeholder: options.placeholder,
        kind: SelectKind::Select,
    }
}

/// Build a single `SelectOptionElement`. 1:1 port of upstream
/// `SelectOption({ label, value, description? }): SelectOptionElement`.
pub fn select_option(
    label: impl Into<String>,
    value: impl Into<String>,
    description: Option<String>,
) -> SelectOptionElement {
    SelectOptionElement {
        description,
        label: label.into(),
        value: value.into(),
    }
}

/// Options for [`external_select`]. Mirrors upstream
/// `interface ExternalSelectOptions`.
#[derive(Debug, Default, Clone)]
pub struct ExternalSelectOptions {
    pub id: String,
    pub initial_option: Option<SelectOptionElement>,
    pub label: String,
    pub min_query_length: Option<u32>,
    pub optional: Option<bool>,
    pub placeholder: Option<String>,
}

/// 1:1 port of upstream `ExternalSelect(options): ExternalSelectElement`.
pub fn external_select(options: ExternalSelectOptions) -> ExternalSelectElement {
    ExternalSelectElement {
        id: options.id,
        initial_option: options.initial_option,
        label: options.label,
        min_query_length: options.min_query_length,
        optional: options.optional,
        placeholder: options.placeholder,
        kind: ExternalSelectKind::ExternalSelect,
    }
}

/// Options for [`radio_select`]. Mirrors upstream
/// `interface RadioSelectOptions`.
#[derive(Debug, Default, Clone)]
pub struct RadioSelectOptions {
    pub id: String,
    pub initial_option: Option<String>,
    pub label: String,
    pub optional: Option<bool>,
    pub options: Vec<SelectOptionElement>,
}

/// 1:1 port of upstream `RadioSelect(options): RadioSelectElement`.
///
/// **Validation:** panics when `options.options` is empty, matching
/// upstream's `if (!options.options.length) throw new Error("RadioSelect
/// requires at least one option")`. A radio group with zero options
/// is structurally invalid.
pub fn radio_select(options: RadioSelectOptions) -> RadioSelectElement {
    assert!(
        !options.options.is_empty(),
        "RadioSelect requires at least one option"
    );
    RadioSelectElement {
        id: options.id,
        initial_option: options.initial_option,
        label: options.label,
        optional: options.optional,
        options: options.options,
        kind: RadioSelectKind::RadioSelect,
    }
}

#[cfg(test)]
mod tests {
    //! Subset port of `packages/chat/src/modals.test.ts`:
    //! the leaf-element builders + Modal + ModalElement + ModalChild +
    //! filterModalChildren + isModalElement.
    //!
    //! **1:1 portable coverage**: 20 of 29 upstream cases mapped 1:1
    //! plus 5 additive Rust coverage tests.
    //!
    //! **9 JSX-runtime cases are `js-only-documented`** per the
    //! slice-380 type-system-impossible pattern — upstream's
    //! `fromReactModalElement` helper converts React JSX elements
    //! into ModalElement instances. The Rust port has no React /
    //! JSX runtime so these cases are unreachable by construction:
    //!
    //! - `fromReactModalElement > should convert a Modal react element`
    //! - `fromReactModalElement > should convert a TextInput react element`
    //! - `fromReactModalElement > should convert a Select react element with children`
    //! - `fromReactModalElement > should convert an ExternalSelect react element`
    //! - `fromReactModalElement > should convert a RadioSelect react element`
    //! - `fromReactModalElement > should return null for non-react, non-modal elements`
    //! - `fromReactModalElement > should pass through plain modal elements`
    //! - `fromReactModalElement > should pass through plain modal children`
    //! - `fromReactModalElement > should handle unknown component by extracting children`
    //! - `fromReactModalElement > should return null for unknown component without children`

    use super::*;

    #[test]
    fn modal_builder_emits_required_fields_with_camelcase_wire() {
        let elem = modal(ModalOptions {
            callback_id: "form_1".to_string(),
            callback_url: Some("https://example.com/submit".to_string()),
            close_label: Some("Cancel".to_string()),
            notify_on_close: Some(true),
            private_metadata: Some(r#"{"ctx":"x"}"#.to_string()),
            submit_label: Some("Submit".to_string()),
            title: "Confirm".to_string(),
            children: Some(vec![
                text_input(TextInputOptions {
                    id: "name".to_string(),
                    label: "Name".to_string(),
                    ..Default::default()
                })
                .into(),
            ]),
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"modal\""));
        assert!(json.contains("\"callbackId\":\"form_1\""));
        assert!(json.contains("\"callbackUrl\":\"https://example.com/submit\""));
        assert!(json.contains("\"closeLabel\":\"Cancel\""));
        assert!(json.contains("\"notifyOnClose\":true"));
        assert!(json.contains("\"privateMetadata\":\"{\\\"ctx\\\":\\\"x\\\"}\""));
        assert!(json.contains("\"submitLabel\":\"Submit\""));
        assert!(json.contains("\"title\":\"Confirm\""));
        // The TextInput child should round-trip through the untagged union.
        let back: ModalElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn modal_builder_minimum_shape_emits_only_required_fields() {
        let elem = modal(ModalOptions {
            callback_id: "id".to_string(),
            title: "T".to_string(),
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(!json.contains("callbackUrl"));
        assert!(!json.contains("closeLabel"));
        assert!(!json.contains("notifyOnClose"));
        assert!(!json.contains("privateMetadata"));
        assert!(!json.contains("submitLabel"));
        // children: [] (empty array) is always emitted because the
        // upstream type requires it.
        assert!(json.contains("\"children\":[]"));
        assert!(json.contains("\"callbackId\":\"id\""));
        assert!(json.contains("\"title\":\"T\""));
    }

    #[test]
    fn modal_child_untagged_round_trip_handles_every_variant() {
        use crate::cards::{field, fields, text as cards_text};
        let payload: Vec<ModalChild> = vec![
            text_input(TextInputOptions {
                id: "name".to_string(),
                label: "Name".to_string(),
                ..Default::default()
            })
            .into(),
            select(SelectOptions {
                id: "s".to_string(),
                label: "S".to_string(),
                options: vec![select_option("A", "a", None)],
                ..Default::default()
            })
            .into(),
            external_select(ExternalSelectOptions {
                id: "es".to_string(),
                label: "ES".to_string(),
                ..Default::default()
            })
            .into(),
            radio_select(RadioSelectOptions {
                id: "rs".to_string(),
                label: "RS".to_string(),
                options: vec![select_option("X", "x", None)],
                ..Default::default()
            })
            .into(),
            cards_text("hello", None).into(),
            fields(vec![field("k", "v")]).into(),
        ];
        let json = serde_json::to_string(&payload).unwrap();
        let back: Vec<ModalChild> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), payload.len());
        assert!(matches!(back[0], ModalChild::TextInput(_)));
        assert!(matches!(back[1], ModalChild::Select(_)));
        assert!(matches!(back[2], ModalChild::ExternalSelect(_)));
        assert!(matches!(back[3], ModalChild::RadioSelect(_)));
        assert!(matches!(back[4], ModalChild::Text(_)));
        assert!(matches!(back[5], ModalChild::Fields(_)));
    }

    #[test]
    fn is_modal_element_distinguishes_modals_from_other_shapes() {
        let modal_json = serde_json::to_value(modal(ModalOptions {
            callback_id: "id".to_string(),
            title: "T".to_string(),
            ..Default::default()
        }))
        .unwrap();
        assert!(is_modal_element(&modal_json));

        let text_input_json = serde_json::to_value(text_input(TextInputOptions {
            id: "id".to_string(),
            label: "Label".to_string(),
            ..Default::default()
        }))
        .unwrap();
        assert!(!is_modal_element(&text_input_json));

        assert!(!is_modal_element(&serde_json::json!({"foo": "bar"})));
        assert!(!is_modal_element(&serde_json::Value::Null));
    }

    #[test]
    fn filter_modal_children_filters_non_object_items() {
        // 1:1 with upstream `filterModalChildren(["string", null, 42] as unknown[])`.
        let input = vec![
            serde_json::Value::String("string".to_string()),
            serde_json::Value::Null,
            serde_json::Value::Number(42.into()),
        ];
        let (kept, dropped) = filter_modal_children(&input);
        assert_eq!(kept.len(), 0);
        assert_eq!(dropped, 3);
    }

    #[test]
    fn filter_modal_children_drops_invalid_children_and_reports_count() {
        let input = vec![
            serde_json::to_value(text_input(TextInputOptions {
                id: "k".to_string(),
                label: "L".to_string(),
                ..Default::default()
            }))
            .unwrap(),
            // Invalid: card element type isn't in VALID_MODAL_CHILD_TYPES.
            serde_json::json!({"type": "button", "id": "x", "label": "x"}),
            // Invalid: no type field.
            serde_json::json!({"foo": "bar"}),
            // Invalid: not an object.
            serde_json::Value::Null,
            // Valid: select.
            serde_json::to_value(select(SelectOptions {
                id: "s".to_string(),
                label: "S".to_string(),
                options: vec![select_option("a", "a", None)],
                ..Default::default()
            }))
            .unwrap(),
        ];
        let (kept, dropped) = filter_modal_children(&input);
        assert_eq!(kept.len(), 2);
        assert_eq!(dropped, 3);
        assert!(matches!(kept[0], ModalChild::TextInput(_)));
        assert!(matches!(kept[1], ModalChild::Select(_)));
    }

    #[test]
    fn valid_modal_child_types_matches_upstream_constant() {
        assert_eq!(
            VALID_MODAL_CHILD_TYPES,
            &[
                "text_input",
                "select",
                "external_select",
                "radio_select",
                "text",
                "fields"
            ]
        );
    }

    #[test]
    fn select_option_round_trips_with_optional_description() {
        let with_desc = select_option("Display", "value", Some("Show me".to_string()));
        let json = serde_json::to_string(&with_desc).unwrap();
        assert!(json.contains("\"description\":\"Show me\""));
        assert!(json.contains("\"label\":\"Display\""));
        assert!(json.contains("\"value\":\"value\""));
        let back: SelectOptionElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, with_desc);

        let no_desc = select_option("L", "v", None);
        let json = serde_json::to_string(&no_desc).unwrap();
        assert!(!json.contains("description"));
    }

    #[test]
    fn text_input_builder_emits_discriminator_and_camelcase_fields() {
        let elem = text_input(TextInputOptions {
            id: "name".to_string(),
            initial_value: Some("Ada".to_string()),
            label: "Your name".to_string(),
            max_length: Some(64),
            multiline: Some(false),
            optional: Some(true),
            placeholder: Some("Type here...".to_string()),
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"text_input\""));
        assert!(json.contains("\"initialValue\":\"Ada\""));
        assert!(json.contains("\"maxLength\":64"));
        assert!(json.contains("\"multiline\":false"));
        let back: TextInputElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn text_input_builder_omits_unset_fields() {
        let elem = text_input(TextInputOptions {
            id: "id".to_string(),
            label: "label".to_string(),
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(!json.contains("initialValue"));
        assert!(!json.contains("maxLength"));
        assert!(!json.contains("multiline"));
        assert!(!json.contains("optional"));
        assert!(!json.contains("placeholder"));
    }

    #[test]
    fn select_builder_serializes_options_array_and_discriminator() {
        let elem = select(SelectOptions {
            id: "prio".to_string(),
            label: "Priority".to_string(),
            options: vec![
                select_option("Low", "low", None),
                select_option("High", "high", None),
            ],
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"select\""));
        assert!(json.contains("\"options\":["));
        assert!(json.contains("\"value\":\"low\""));
        assert!(json.contains("\"value\":\"high\""));
    }

    #[test]
    fn external_select_builder_emits_external_select_discriminator() {
        let elem = external_select(ExternalSelectOptions {
            id: "user".to_string(),
            label: "User".to_string(),
            min_query_length: Some(3),
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"external_select\""));
        assert!(json.contains("\"minQueryLength\":3"));
    }

    #[test]
    fn external_select_initial_option_is_a_select_option_element_not_a_string() {
        // Critical distinction from SelectElement.initial_option (String):
        // ExternalSelectElement.initial_option is a full SelectOptionElement
        // because the option value is server-side resolved.
        let elem = external_select(ExternalSelectOptions {
            id: "id".to_string(),
            label: "label".to_string(),
            initial_option: Some(select_option(
                "Resolved",
                "U_123",
                Some("Cached label".to_string()),
            )),
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"initialOption\":{"));
        assert!(json.contains("\"label\":\"Resolved\""));
        assert!(json.contains("\"value\":\"U_123\""));
        assert!(json.contains("\"description\":\"Cached label\""));
    }

    #[test]
    fn radio_select_builder_emits_radio_select_discriminator() {
        let elem = radio_select(RadioSelectOptions {
            id: "status".to_string(),
            label: "Status".to_string(),
            initial_option: Some("open".to_string()),
            options: vec![
                select_option("Open", "open", None),
                select_option("Closed", "closed", None),
            ],
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"radio_select\""));
        assert!(json.contains("\"initialOption\":\"open\""));
        assert!(json.contains("\"value\":\"open\""));
    }

    // ---------- additional 1:1 cases (slice 51) ----------

    #[test]
    fn modal_builder_accepts_children() {
        let m = modal(ModalOptions {
            callback_id: "m1".to_string(),
            title: "T".to_string(),
            children: Some(vec![
                text_input(TextInputOptions {
                    id: "name".to_string(),
                    label: "Name".to_string(),
                    ..Default::default()
                })
                .into(),
            ]),
            ..Default::default()
        });
        assert_eq!(m.children.len(), 1);
        assert!(matches!(m.children[0], ModalChild::TextInput(_)));
    }

    #[test]
    fn text_input_builder_accepts_optional_fields() {
        let elem = text_input(TextInputOptions {
            id: "bio".to_string(),
            label: "Bio".to_string(),
            placeholder: Some("Tell us about yourself".to_string()),
            initial_value: Some("Hi".to_string()),
            multiline: Some(true),
            optional: Some(true),
            max_length: Some(500),
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"placeholder\":\"Tell us about yourself\""));
        assert!(json.contains("\"initialValue\":\"Hi\""));
        assert!(json.contains("\"multiline\":true"));
        assert!(json.contains("\"optional\":true"));
        assert!(json.contains("\"maxLength\":500"));
    }

    #[test]
    #[should_panic(expected = "Select requires at least one option")]
    fn select_builder_panics_on_empty_options() {
        select(SelectOptions {
            id: "s1".to_string(),
            label: "Pick".to_string(),
            options: vec![],
            ..Default::default()
        });
    }

    #[test]
    fn select_builder_accepts_optional_fields() {
        let elem = select(SelectOptions {
            id: "s1".to_string(),
            label: "Pick".to_string(),
            placeholder: Some("Choose".to_string()),
            initial_option: Some("a".to_string()),
            optional: Some(true),
            options: vec![select_option("a", "a", None)],
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"placeholder\":\"Choose\""));
        assert!(json.contains("\"initialOption\":\"a\""));
        assert!(json.contains("\"optional\":true"));
    }

    #[test]
    fn external_select_builder_accepts_optional_fields() {
        let elem = external_select(ExternalSelectOptions {
            id: "es1".to_string(),
            label: "Search".to_string(),
            placeholder: Some("Type to search".to_string()),
            initial_option: Some(select_option("Initial", "init", None)),
            min_query_length: Some(3),
            optional: Some(false),
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"placeholder\":\"Type to search\""));
        assert!(json.contains("\"minQueryLength\":3"));
        assert!(json.contains("\"initialOption\""));
    }

    #[test]
    fn select_option_with_label_and_value_only_omits_description() {
        let opt = select_option("Yes", "yes", None);
        let json = serde_json::to_string(&opt).unwrap();
        assert_eq!(json, "{\"label\":\"Yes\",\"value\":\"yes\"}");
    }

    #[test]
    fn select_option_with_description_includes_it() {
        let opt = select_option("Yes", "yes", Some("Confirm".to_string()));
        let json = serde_json::to_string(&opt).unwrap();
        assert!(json.contains("\"description\":\"Confirm\""));
    }

    #[test]
    #[should_panic(expected = "RadioSelect requires at least one option")]
    fn radio_select_builder_panics_on_empty_options() {
        radio_select(RadioSelectOptions {
            id: "r1".to_string(),
            label: "Choose".to_string(),
            options: vec![],
            ..Default::default()
        });
    }

    #[test]
    fn is_modal_element_returns_true_for_modal_elements() {
        let m = modal(ModalOptions {
            callback_id: "m1".to_string(),
            title: "T".to_string(),
            ..Default::default()
        });
        let value = serde_json::to_value(&m).unwrap();
        assert!(is_modal_element(&value));
    }

    #[test]
    fn is_modal_element_returns_false_for_non_modal_objects() {
        assert!(!is_modal_element(&serde_json::json!({"type": "text"})));
        assert!(!is_modal_element(&serde_json::json!({})));
        assert!(!is_modal_element(&serde_json::json!("not-an-object")));
        assert!(!is_modal_element(&serde_json::json!(null)));
    }

    #[test]
    fn filter_modal_children_keeps_every_valid_modal_child_type() {
        let input = vec![
            serde_json::to_value(text_input(TextInputOptions {
                id: "a".to_string(),
                label: "A".to_string(),
                ..Default::default()
            }))
            .unwrap(),
            serde_json::to_value(select(SelectOptions {
                id: "b".to_string(),
                label: "B".to_string(),
                options: vec![select_option("x", "x", None)],
                ..Default::default()
            }))
            .unwrap(),
            serde_json::to_value(external_select(ExternalSelectOptions {
                id: "c".to_string(),
                label: "C".to_string(),
                ..Default::default()
            }))
            .unwrap(),
            serde_json::to_value(radio_select(RadioSelectOptions {
                id: "d".to_string(),
                label: "D".to_string(),
                options: vec![select_option("y", "y", None)],
                ..Default::default()
            }))
            .unwrap(),
        ];
        let (kept, dropped) = filter_modal_children(&input);
        assert_eq!(kept.len(), 4);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn filter_modal_children_drops_non_object_items() {
        let valid = serde_json::to_value(text_input(TextInputOptions {
            id: "a".to_string(),
            label: "A".to_string(),
            ..Default::default()
        }))
        .unwrap();
        let input = vec![
            serde_json::json!("string"),
            serde_json::json!(42),
            serde_json::json!(null),
            valid,
        ];
        let (kept, dropped) = filter_modal_children(&input);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 3);
    }

    // ---------- slice 115: modal_child_kind + per-element predicates ----------

    #[test]
    fn modal_child_kind_reads_the_type_field() {
        assert_eq!(
            modal_child_kind(&serde_json::json!({"type": "text_input"})),
            Some("text_input")
        );
        assert_eq!(
            modal_child_kind(&serde_json::json!({"type": "fields"})),
            Some("fields")
        );
        assert!(modal_child_kind(&serde_json::json!({})).is_none());
        assert!(modal_child_kind(&serde_json::json!(null)).is_none());
        assert!(modal_child_kind(&serde_json::json!("text")).is_none());
    }

    #[test]
    fn is_text_input_element_matches_only_text_input_types() {
        assert!(is_text_input_element(
            &serde_json::json!({"type": "text_input"})
        ));
        assert!(!is_text_input_element(&serde_json::json!({"type": "text"})));
        assert!(!is_text_input_element(&serde_json::json!({})));
    }

    #[test]
    fn is_select_element_matches_only_select_types() {
        assert!(is_select_element(&serde_json::json!({"type": "select"})));
        assert!(!is_select_element(
            &serde_json::json!({"type": "external_select"})
        ));
        assert!(!is_select_element(
            &serde_json::json!({"type": "radio_select"})
        ));
    }

    #[test]
    fn is_external_select_element_only_matches_external_select() {
        assert!(is_external_select_element(
            &serde_json::json!({"type": "external_select"})
        ));
        assert!(!is_external_select_element(
            &serde_json::json!({"type": "select"})
        ));
    }

    #[test]
    fn is_radio_select_element_only_matches_radio_select() {
        assert!(is_radio_select_element(
            &serde_json::json!({"type": "radio_select"})
        ));
        assert!(!is_radio_select_element(
            &serde_json::json!({"type": "select"})
        ));
    }

    #[test]
    fn is_modal_text_element_only_matches_text_type() {
        assert!(is_modal_text_element(&serde_json::json!({"type": "text"})));
        // The card-level text is a SEPARATE element kind in cards.rs;
        // here we only care about the modal-children list.
        assert!(!is_modal_text_element(
            &serde_json::json!({"type": "text_input"})
        ));
    }

    #[test]
    fn is_modal_fields_element_only_matches_fields_type() {
        assert!(is_modal_fields_element(
            &serde_json::json!({"type": "fields"})
        ));
        assert!(!is_modal_fields_element(
            &serde_json::json!({"type": "field"})
        ));
    }

    #[test]
    fn is_valid_modal_child_accepts_every_type_in_the_constant() {
        for t in VALID_MODAL_CHILD_TYPES {
            assert!(is_valid_modal_child(&serde_json::json!({"type": t})));
        }
    }

    #[test]
    fn is_valid_modal_child_rejects_unknown_types_and_non_objects() {
        assert!(!is_valid_modal_child(&serde_json::json!({"type": "card"})));
        assert!(!is_valid_modal_child(&serde_json::json!({})));
        assert!(!is_valid_modal_child(&serde_json::json!(null)));
        assert!(!is_valid_modal_child(&serde_json::json!("text")));
    }
}
