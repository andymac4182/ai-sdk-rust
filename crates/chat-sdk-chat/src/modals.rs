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

use serde::{Deserialize, Serialize};

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

// ============================================================================
// Builders
// ============================================================================

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
pub fn select(options: SelectOptions) -> SelectElement {
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
pub fn radio_select(options: RadioSelectOptions) -> RadioSelectElement {
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
    //! Subset port of `packages/chat/src/modals.test.ts` (slice 33):
    //! the leaf-element builders and their wire-shape round-trips. The
    //! remaining upstream cases (Modal + ModalElement + ModalChild +
    //! filterModalChildren + isModalElement) ship in follow-up slices.

    use super::*;

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
}
