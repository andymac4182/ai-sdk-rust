//! Card elements for cross-platform rich messaging.
//!
//! 1:1 port (in progress) of `packages/chat/src/cards.ts`. Provides
//! data types + builder functions for rich cards that adapters convert
//! to platform-specific formats (Slack Block Kit, Teams Adaptive Cards,
//! Google Chat Card v2).
//!
//! **What this slice ships (slice 32):**
//!
//! - Style enums: `ButtonStyle`, `TextStyle`, `TableAlignment`,
//!   `ButtonActionType`.
//! - Leaf element structs: `ButtonElement`, `LinkButtonElement`,
//!   `TextElement`, `ImageElement`, `DividerElement`, `LinkElement`,
//!   `FieldElement`, `FieldsElement`, `TableElement`.
//! - Builders: [`button`], [`link_button`], [`text`], [`card_text`],
//!   [`image`], [`divider`], [`field`], [`fields`], [`table`],
//!   [`card_link`].
//!
//! Each element type has a discriminator string in the upstream wire
//! shape (e.g. `"button"`, `"text"`, …) carried as a `#[serde(tag = "type",
//! rename = "<discriminator>")]`-style enum-ish unit field. We surface
//! that here via per-struct `#[serde(tag = "type", rename_all = "lowercase")]`
//! variants once the CardChild union ships; for now each struct has a
//! `kind: <discriminator>` constant accessor so test assertions can
//! verify the wire shape.
//!
//! **What remains for follow-up slices:**
//!
//! - `ActionsElement` (children of which include `SelectElement` and
//!   `RadioSelectElement` from `modals` — both blocked).
//! - `SectionElement` (children: `CardChild[]` — needs `ActionsElement`).
//! - `CardChild` discriminated union and `CardElement` + `CardOptions`
//!   + the `Card` builder + `is_card_element` type guard.
//! - JSX-side card helpers (`cardChildToFallbackText`, `fromReactElement`,
//!   `isCardElement` from the JSX runtime — `js-only-documented`).

use crate::modals::{RadioSelectElement, SelectElement};
use serde::{Deserialize, Serialize};

/// Button style. 1:1 port of upstream
/// `export type ButtonStyle = "primary" | "danger" | "default"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ButtonStyle {
    Primary,
    Danger,
    Default,
}

/// Text style. 1:1 port of upstream
/// `export type TextStyle = "plain" | "bold" | "muted"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextStyle {
    Plain,
    Bold,
    Muted,
}

/// Column alignment for a [`TableElement`]. 1:1 port of upstream
/// `export type TableAlignment = "left" | "center" | "right"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableAlignment {
    Left,
    Center,
    Right,
}

/// Action kind on a [`ButtonElement`]. 1:1 port of upstream
/// `"action" | "modal"` literal union on
/// [`ButtonElement::action_type`]. The upstream default is `Action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ButtonActionType {
    Action,
    Modal,
}

/// Button for interactive actions. 1:1 port of upstream
/// `interface ButtonElement`. The `type: "button"` discriminator is
/// represented via the [`Self::kind`] constant accessor — the future
/// `CardChild` discriminated-union port will move the discriminator to
/// a `#[serde(tag = "type")]` enum variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ButtonElement {
    /// Whether this button triggers a regular action or opens a modal
    /// dialog. Upstream default: [`ButtonActionType::Action`].
    #[serde(
        rename = "actionType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub action_type: Option<ButtonActionType>,
    /// URL to POST action data to when clicked.
    #[serde(
        rename = "callbackUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub callback_url: Option<String>,
    /// Whether the button is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Unique action ID for callback routing.
    pub id: String,
    /// Button label text.
    pub label: String,
    /// Visual style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ButtonStyle>,
    /// Discriminator. Always `"button"` on the wire.
    #[serde(rename = "type")]
    pub kind: ButtonKind,
    /// Optional payload value sent with the action callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Discriminator tag for [`ButtonElement`]. Single-variant unit-like
/// enum so serde emits the upstream literal `"button"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ButtonKind {
    #[default]
    #[serde(rename = "button")]
    Button,
}

/// Link button that opens a URL. 1:1 port of upstream
/// `interface LinkButtonElement`. Discriminator `"link-button"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LinkButtonElement {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ButtonStyle>,
    #[serde(rename = "type")]
    pub kind: LinkButtonKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LinkButtonKind {
    #[default]
    #[serde(rename = "link-button")]
    LinkButton,
}

/// Text content. 1:1 port of upstream `interface TextElement`.
/// Discriminator `"text"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextElement {
    /// Text content (markdown is interpreted by some platform adapters).
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TextStyle>,
    #[serde(rename = "type")]
    pub kind: TextKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TextKind {
    #[default]
    #[serde(rename = "text")]
    Text,
}

/// Image. 1:1 port of upstream `interface ImageElement`. Discriminator
/// `"image"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImageElement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(rename = "type")]
    pub kind: ImageKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ImageKind {
    #[default]
    #[serde(rename = "image")]
    Image,
}

/// Visual separator. 1:1 port of upstream `interface DividerElement`.
/// Discriminator `"divider"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DividerElement {
    #[serde(rename = "type")]
    pub kind: DividerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DividerKind {
    #[default]
    #[serde(rename = "divider")]
    Divider,
}

/// Inline hyperlink. 1:1 port of upstream `interface LinkElement`.
/// Discriminator `"link"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LinkElement {
    pub label: String,
    #[serde(rename = "type")]
    pub kind: LinkKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LinkKind {
    #[default]
    #[serde(rename = "link")]
    Link,
}

/// Key/value display field. 1:1 port of upstream `interface FieldElement`.
/// Discriminator `"field"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldElement {
    pub label: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FieldKind {
    #[default]
    #[serde(rename = "field")]
    Field,
}

/// Multi-column field layout. 1:1 port of upstream
/// `interface FieldsElement`. Discriminator `"fields"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldsElement {
    pub children: Vec<FieldElement>,
    #[serde(rename = "type")]
    pub kind: FieldsKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FieldsKind {
    #[default]
    #[serde(rename = "fields")]
    Fields,
}

/// Structured data table. 1:1 port of upstream `interface TableElement`.
/// Discriminator `"table"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableElement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Vec<TableAlignment>>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    #[serde(rename = "type")]
    pub kind: TableKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TableKind {
    #[default]
    #[serde(rename = "table")]
    Table,
}

/// Section container for grouping card children. 1:1 port of upstream
/// `interface SectionElement`. Discriminator `"section"`. Children are
/// drawn from [`CardChild`] (any card child type, including nested
/// `Section`s).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SectionElement {
    pub children: Vec<CardChild>,
    #[serde(rename = "type")]
    pub kind: SectionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SectionKind {
    #[default]
    #[serde(rename = "section")]
    Section,
}

/// Children of a [`CardElement`] or [`SectionElement`]. 1:1 port of
/// upstream `export type CardChild = TextElement | ImageElement |
/// DividerElement | ActionsElement | SectionElement | FieldsElement |
/// LinkElement | TableElement`.
///
/// Modeled `#[serde(untagged)]` over the eight element structs — each
/// carries its own discriminator string field, so serde can disambiguate
/// from the JSON without an outer wrapper. The variant order in this
/// enum is the upstream variant order; serde's untagged matcher tries
/// variants in declaration order so the most specific shapes (with
/// required non-overlapping fields) ship first.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CardChild {
    Text(TextElement),
    Image(ImageElement),
    Divider(DividerElement),
    Actions(ActionsElement),
    Section(SectionElement),
    Fields(FieldsElement),
    Link(LinkElement),
    Table(TableElement),
}

impl From<TextElement> for CardChild {
    fn from(value: TextElement) -> Self {
        Self::Text(value)
    }
}
impl From<ImageElement> for CardChild {
    fn from(value: ImageElement) -> Self {
        Self::Image(value)
    }
}
impl From<DividerElement> for CardChild {
    fn from(value: DividerElement) -> Self {
        Self::Divider(value)
    }
}
impl From<ActionsElement> for CardChild {
    fn from(value: ActionsElement) -> Self {
        Self::Actions(value)
    }
}
impl From<SectionElement> for CardChild {
    fn from(value: SectionElement) -> Self {
        Self::Section(value)
    }
}
impl From<FieldsElement> for CardChild {
    fn from(value: FieldsElement) -> Self {
        Self::Fields(value)
    }
}
impl From<LinkElement> for CardChild {
    fn from(value: LinkElement) -> Self {
        Self::Link(value)
    }
}
impl From<TableElement> for CardChild {
    fn from(value: TableElement) -> Self {
        Self::Table(value)
    }
}

/// Root card element. 1:1 port of upstream `interface CardElement`.
/// Discriminator `"card"`. `title`/`subtitle`/`image_url` are optional;
/// `children` is required (an empty `Vec` is fine).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardElement {
    pub children: Vec<CardChild>,
    #[serde(rename = "imageUrl", default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: CardKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CardKind {
    #[default]
    #[serde(rename = "card")]
    Card,
}

/// Type guard for [`CardElement`]. 1:1 port of upstream
/// `isCardElement(value: unknown): value is CardElement`. Returns
/// `true` when the JSON value is an object whose `type` field equals
/// `"card"`.
pub fn is_card_element(value: &serde_json::Value) -> bool {
    value
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == "card")
}

/// Children of an [`ActionsElement`]. 1:1 port of upstream's
/// `(ButtonElement | LinkButtonElement | SelectElement | RadioSelectElement)[]`
/// children-type union.
///
/// Modeled as `#[serde(untagged)]` over the four element structs because
/// each carries its own discriminator (`type: "button"`/`"link-button"`/
/// `"select"`/`"radio_select"`) — serde can disambiguate variants by that
/// inner `type` field without an outer wrapper, preserving the upstream
/// JSON shape exactly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActionsChild {
    Button(ButtonElement),
    LinkButton(LinkButtonElement),
    Select(SelectElement),
    RadioSelect(RadioSelectElement),
}

impl From<ButtonElement> for ActionsChild {
    fn from(value: ButtonElement) -> Self {
        Self::Button(value)
    }
}

impl From<LinkButtonElement> for ActionsChild {
    fn from(value: LinkButtonElement) -> Self {
        Self::LinkButton(value)
    }
}

impl From<SelectElement> for ActionsChild {
    fn from(value: SelectElement) -> Self {
        Self::Select(value)
    }
}

impl From<RadioSelectElement> for ActionsChild {
    fn from(value: RadioSelectElement) -> Self {
        Self::RadioSelect(value)
    }
}

/// Container for action buttons and selects. 1:1 port of upstream
/// `interface ActionsElement`. Discriminator `"actions"`. Children must
/// be drawn from [`ActionsChild`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionsElement {
    pub children: Vec<ActionsChild>,
    #[serde(rename = "type")]
    pub kind: ActionsKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ActionsKind {
    #[default]
    #[serde(rename = "actions")]
    Actions,
}

// ============================================================================
// Builder functions — snake_case Rust analogues of upstream's PascalCase
// JSX-friendly constructors. Each returns the constructed struct with
// optional fields defaulted to None.
// ============================================================================

/// 1:1 port of upstream `Actions(children): ActionsElement`. Children
/// can be passed as concrete `ButtonElement`/`LinkButtonElement`/
/// `SelectElement`/`RadioSelectElement` values thanks to the
/// `From<...>` impls on [`ActionsChild`].
pub fn actions(children: Vec<ActionsChild>) -> ActionsElement {
    ActionsElement {
        children,
        kind: ActionsKind::Actions,
    }
}

/// 1:1 port of upstream `Section(children: CardChild[]): SectionElement`.
/// Children can be passed as concrete element values via
/// `From<...>` impls on [`CardChild`] (`Text`, `Image`, `Divider`,
/// `Actions`, `Section`, `Fields`, `Link`, `Table`).
pub fn section(children: Vec<CardChild>) -> SectionElement {
    SectionElement {
        children,
        kind: SectionKind::Section,
    }
}

/// Options for [`card`]. Mirrors upstream `interface CardOptions`. All
/// fields optional (the upstream default is `{ children: [] }`).
#[derive(Debug, Default, Clone)]
pub struct CardOptions {
    pub children: Option<Vec<CardChild>>,
    pub image_url: Option<String>,
    pub subtitle: Option<String>,
    pub title: Option<String>,
}

/// 1:1 port of upstream `Card(options?): CardElement`. Builds a root
/// card element with the given title/subtitle/image and children. The
/// upstream `children ?? []` default lands here as
/// `options.children.unwrap_or_default()`.
pub fn card(options: CardOptions) -> CardElement {
    CardElement {
        children: options.children.unwrap_or_default(),
        image_url: options.image_url,
        subtitle: options.subtitle,
        title: options.title,
        kind: CardKind::Card,
    }
}

/// Options for [`button`]. Mirrors upstream `interface ButtonOptions`.
#[derive(Debug, Default, Clone)]
pub struct ButtonOptions {
    pub action_type: Option<ButtonActionType>,
    pub callback_url: Option<String>,
    pub disabled: Option<bool>,
    pub id: String,
    pub label: String,
    pub style: Option<ButtonStyle>,
    pub value: Option<String>,
}

/// 1:1 port of upstream `Button(options): ButtonElement`.
pub fn button(options: ButtonOptions) -> ButtonElement {
    ButtonElement {
        action_type: options.action_type,
        callback_url: options.callback_url,
        disabled: options.disabled,
        id: options.id,
        label: options.label,
        style: options.style,
        kind: ButtonKind::Button,
        value: options.value,
    }
}

/// Options for [`link_button`]. Mirrors upstream `interface LinkButtonOptions`.
#[derive(Debug, Default, Clone)]
pub struct LinkButtonOptions {
    pub label: String,
    pub style: Option<ButtonStyle>,
    pub url: String,
}

/// 1:1 port of upstream `LinkButton(options): LinkButtonElement`.
pub fn link_button(options: LinkButtonOptions) -> LinkButtonElement {
    LinkButtonElement {
        label: options.label,
        style: options.style,
        kind: LinkButtonKind::LinkButton,
        url: options.url,
    }
}

/// 1:1 port of upstream `Text(content, options?): TextElement`.
/// The optional second argument is folded into a single
/// [`Option<TextStyle>`] here because Rust has no analogue of
/// JavaScript's "optional options object with default `{}`".
pub fn text(content: impl Into<String>, style: Option<TextStyle>) -> TextElement {
    TextElement {
        content: content.into(),
        style,
        kind: TextKind::Text,
    }
}

/// Alias of [`text`]. 1:1 port of upstream `export const CardText = Text`,
/// kept for callers that would otherwise collide with the DOM `Text`
/// global. The Rust port does not need this for namespacing but ships
/// the alias so consumers can swap `text` for `card_text` 1:1 with
/// upstream call sites.
pub fn card_text(content: impl Into<String>, style: Option<TextStyle>) -> TextElement {
    text(content, style)
}

/// 1:1 port of upstream `Image({ url, alt? }): ImageElement`.
pub fn image(url: impl Into<String>, alt: Option<String>) -> ImageElement {
    ImageElement {
        alt,
        kind: ImageKind::Image,
        url: url.into(),
    }
}

/// 1:1 port of upstream `Divider(): DividerElement`.
pub fn divider() -> DividerElement {
    DividerElement {
        kind: DividerKind::Divider,
    }
}

/// 1:1 port of upstream `Field({ label, value }): FieldElement`.
pub fn field(label: impl Into<String>, value: impl Into<String>) -> FieldElement {
    FieldElement {
        label: label.into(),
        kind: FieldKind::Field,
        value: value.into(),
    }
}

/// 1:1 port of upstream `Fields(children: FieldElement[]): FieldsElement`.
pub fn fields(children: Vec<FieldElement>) -> FieldsElement {
    FieldsElement {
        children,
        kind: FieldsKind::Fields,
    }
}

/// Options for [`table`]. Mirrors upstream `interface TableOptions`.
#[derive(Debug, Default, Clone)]
pub struct TableOptions {
    pub align: Option<Vec<TableAlignment>>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 1:1 port of upstream `Table(options): TableElement`.
pub fn table(options: TableOptions) -> TableElement {
    TableElement {
        align: options.align,
        headers: options.headers,
        rows: options.rows,
        kind: TableKind::Table,
    }
}

/// 1:1 port of upstream `CardLink({ url, label }): LinkElement`.
pub fn card_link(url: impl Into<String>, label: impl Into<String>) -> LinkElement {
    LinkElement {
        label: label.into(),
        kind: LinkKind::Link,
        url: url.into(),
    }
}

#[cfg(test)]
mod tests {
    //! Subset port of `packages/chat/src/cards.test.ts` (slice 32):
    //! the leaf-element builders and their wire-shape round-trips. The
    //! remaining upstream cases (Card / Section / Actions / CardChild
    //! union / isCardElement / Card.toAscii fallback) ship in follow-up
    //! slices alongside the corresponding API additions.

    use super::*;

    #[test]
    fn button_style_enum_uses_upstream_lowercase_strings() {
        for (style, wire) in [
            (ButtonStyle::Primary, "primary"),
            (ButtonStyle::Danger, "danger"),
            (ButtonStyle::Default, "default"),
        ] {
            assert_eq!(
                serde_json::to_string(&style).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn text_style_enum_uses_upstream_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&TextStyle::Plain).unwrap(),
            "\"plain\""
        );
        assert_eq!(serde_json::to_string(&TextStyle::Bold).unwrap(), "\"bold\"");
        assert_eq!(
            serde_json::to_string(&TextStyle::Muted).unwrap(),
            "\"muted\""
        );
    }

    #[test]
    fn table_alignment_enum_uses_upstream_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&TableAlignment::Left).unwrap(),
            "\"left\""
        );
        assert_eq!(
            serde_json::to_string(&TableAlignment::Center).unwrap(),
            "\"center\""
        );
        assert_eq!(
            serde_json::to_string(&TableAlignment::Right).unwrap(),
            "\"right\""
        );
    }

    #[test]
    fn button_builder_carries_all_options_through_to_the_struct() {
        let elem = button(ButtonOptions {
            action_type: Some(ButtonActionType::Modal),
            callback_url: Some("https://example.com/cb".to_string()),
            disabled: Some(true),
            id: "id_1".to_string(),
            label: "Approve".to_string(),
            style: Some(ButtonStyle::Primary),
            value: Some("v_1".to_string()),
        });
        assert_eq!(elem.id, "id_1");
        assert_eq!(elem.label, "Approve");
        assert_eq!(elem.style, Some(ButtonStyle::Primary));
        assert_eq!(elem.value.as_deref(), Some("v_1"));
        assert_eq!(elem.disabled, Some(true));
        assert_eq!(elem.action_type, Some(ButtonActionType::Modal));
        assert_eq!(elem.callback_url.as_deref(), Some("https://example.com/cb"));
        // Discriminator: serializes as upstream "button" literal.
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"button\""));
        assert!(json.contains("\"actionType\":\"modal\""));
        assert!(json.contains("\"callbackUrl\":\"https://example.com/cb\""));
    }

    #[test]
    fn button_builder_omits_unset_options_from_the_wire_shape() {
        let elem = button(ButtonOptions {
            id: "id_2".to_string(),
            label: "OK".to_string(),
            ..Default::default()
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"id\":\"id_2\""));
        assert!(json.contains("\"label\":\"OK\""));
        assert!(json.contains("\"type\":\"button\""));
        assert!(!json.contains("style"));
        assert!(!json.contains("value"));
        assert!(!json.contains("disabled"));
        assert!(!json.contains("actionType"));
        assert!(!json.contains("callbackUrl"));
    }

    #[test]
    fn link_button_builder_emits_kebab_case_discriminator() {
        let elem = link_button(LinkButtonOptions {
            label: "Docs".to_string(),
            style: Some(ButtonStyle::Default),
            url: "https://example.com".to_string(),
        });
        let json = serde_json::to_string(&elem).unwrap();
        // Critical: upstream uses "link-button" (kebab) for this
        // discriminator while every other element uses lowercase.
        assert!(json.contains("\"type\":\"link-button\""));
        assert!(json.contains("\"url\":\"https://example.com\""));
        assert!(json.contains("\"style\":\"default\""));
    }

    #[test]
    fn text_builder_omits_style_when_not_specified() {
        let elem = text("hi", None);
        let json = serde_json::to_string(&elem).unwrap();
        assert_eq!(json, "{\"content\":\"hi\",\"type\":\"text\"}");
        let back: TextElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn text_builder_serializes_style_when_provided() {
        let elem = text("Important", Some(TextStyle::Bold));
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"content\":\"Important\""));
        assert!(json.contains("\"style\":\"bold\""));
        assert!(json.contains("\"type\":\"text\""));
    }

    #[test]
    fn card_text_alias_returns_the_same_text_element_as_text() {
        let a = text("same", Some(TextStyle::Muted));
        let b = card_text("same", Some(TextStyle::Muted));
        assert_eq!(a, b);
    }

    #[test]
    fn image_builder_round_trips_with_optional_alt() {
        let with_alt = image("https://example.com/x.png", Some("A picture".to_string()));
        let without_alt = image("https://example.com/y.png", None);
        let with_alt_json = serde_json::to_string(&with_alt).unwrap();
        let without_alt_json = serde_json::to_string(&without_alt).unwrap();
        assert!(with_alt_json.contains("\"alt\":\"A picture\""));
        assert!(with_alt_json.contains("\"url\":\"https://example.com/x.png\""));
        assert!(with_alt_json.contains("\"type\":\"image\""));
        assert!(!without_alt_json.contains("alt"));
    }

    #[test]
    fn divider_builder_emits_only_the_discriminator() {
        let elem = divider();
        let json = serde_json::to_string(&elem).unwrap();
        assert_eq!(json, "{\"type\":\"divider\"}");
    }

    #[test]
    fn field_builder_emits_label_value_and_discriminator() {
        let elem = field("Name", "Alice");
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"label\":\"Name\""));
        assert!(json.contains("\"value\":\"Alice\""));
        assert!(json.contains("\"type\":\"field\""));
    }

    #[test]
    fn fields_builder_wraps_children_with_fields_discriminator() {
        let elem = fields(vec![field("k1", "v1"), field("k2", "v2")]);
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"fields\""));
        assert!(json.contains("\"children\":["));
        assert!(json.contains("\"label\":\"k1\""));
        assert!(json.contains("\"label\":\"k2\""));
    }

    #[test]
    fn table_builder_round_trips_headers_rows_and_optional_align() {
        let elem = table(TableOptions {
            align: Some(vec![
                TableAlignment::Left,
                TableAlignment::Right,
                TableAlignment::Center,
            ]),
            headers: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            rows: vec![
                vec!["1".to_string(), "2".to_string(), "3".to_string()],
                vec!["4".to_string(), "5".to_string(), "6".to_string()],
            ],
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"table\""));
        assert!(json.contains("\"align\":[\"left\",\"right\",\"center\"]"));
        assert!(json.contains("\"headers\":[\"A\",\"B\",\"C\"]"));
        assert!(json.contains("\"rows\":[[\"1\",\"2\",\"3\"],[\"4\",\"5\",\"6\"]]"));
        let back: TableElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn table_builder_omits_align_when_none() {
        let elem = table(TableOptions {
            align: None,
            headers: vec!["A".to_string()],
            rows: vec![],
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(!json.contains("align"));
    }

    #[test]
    fn actions_builder_wraps_button_and_link_button_children() {
        use crate::cards::{button, link_button};
        let elem = actions(vec![
            button(ButtonOptions {
                id: "approve".to_string(),
                label: "Approve".to_string(),
                style: Some(ButtonStyle::Primary),
                ..Default::default()
            })
            .into(),
            link_button(LinkButtonOptions {
                label: "Docs".to_string(),
                url: "https://example.com".to_string(),
                ..Default::default()
            })
            .into(),
        ]);
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"actions\""));
        assert!(json.contains("\"type\":\"button\""));
        assert!(json.contains("\"type\":\"link-button\""));
        assert!(json.contains("\"label\":\"Approve\""));
        assert!(json.contains("\"label\":\"Docs\""));
    }

    #[test]
    fn actions_children_round_trip_through_untagged_union() {
        // Build an ActionsElement, serialize, deserialize, verify
        // structural equivalence. The untagged ActionsChild enum should
        // pick the right variant on the wire shape `type` discriminator.
        use crate::cards::button;
        use crate::modals::{SelectOptions, select, select_option};
        let original = actions(vec![
            button(ButtonOptions {
                id: "ok".to_string(),
                label: "OK".to_string(),
                ..Default::default()
            })
            .into(),
            select(SelectOptions {
                id: "prio".to_string(),
                label: "Priority".to_string(),
                options: vec![select_option("High", "high", None)],
                ..Default::default()
            })
            .into(),
        ]);
        let json = serde_json::to_string(&original).unwrap();
        let back: ActionsElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
        // Spot-check that the deserialized children landed in the right
        // variants (not the leftmost-untagged-match catch-all).
        assert!(matches!(back.children[0], ActionsChild::Button(_)));
        assert!(matches!(back.children[1], ActionsChild::Select(_)));
    }

    #[test]
    fn actions_children_accept_radio_select_and_link_button_via_from() {
        use crate::cards::link_button;
        use crate::modals::{RadioSelectOptions, radio_select, select_option};
        let elem = actions(vec![
            link_button(LinkButtonOptions {
                label: "Open".to_string(),
                url: "https://example.com".to_string(),
                ..Default::default()
            })
            .into(),
            radio_select(RadioSelectOptions {
                id: "status".to_string(),
                label: "Status".to_string(),
                options: vec![select_option("Open", "open", None)],
                ..Default::default()
            })
            .into(),
        ]);
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"link-button\""));
        assert!(json.contains("\"type\":\"radio_select\""));
        let back: ActionsElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.children.len(), 2);
        assert!(matches!(back.children[0], ActionsChild::LinkButton(_)));
        assert!(matches!(back.children[1], ActionsChild::RadioSelect(_)));
    }

    #[test]
    fn section_builder_wraps_card_children() {
        let elem = section(vec![
            text("hi", None).into(),
            divider().into(),
            image("https://example.com/x.png", None).into(),
        ]);
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"section\""));
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"type\":\"divider\""));
        assert!(json.contains("\"type\":\"image\""));
        let back: SectionElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn card_builder_with_no_options_emits_empty_children_and_only_type() {
        let elem = card(CardOptions::default());
        let json = serde_json::to_string(&elem).unwrap();
        assert_eq!(json, "{\"children\":[],\"type\":\"card\"}");
        let back: CardElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn card_builder_full_shape_round_trips_camelcase_image_url() {
        let elem = card(CardOptions {
            children: Some(vec![text("Total: $50", None).into()]),
            image_url: Some("https://example.com/banner.png".to_string()),
            subtitle: Some("Order #1234".to_string()),
            title: Some("Welcome".to_string()),
        });
        let json = serde_json::to_string(&elem).unwrap();
        assert!(json.contains("\"type\":\"card\""));
        assert!(json.contains("\"title\":\"Welcome\""));
        assert!(json.contains("\"subtitle\":\"Order #1234\""));
        assert!(json.contains("\"imageUrl\":\"https://example.com/banner.png\""));
        assert!(json.contains("\"type\":\"text\""));
        let back: CardElement = serde_json::from_str(&json).unwrap();
        assert_eq!(back, elem);
    }

    #[test]
    fn is_card_element_distinguishes_cards_from_other_shapes() {
        let card_json = serde_json::to_value(card(CardOptions::default())).unwrap();
        assert!(is_card_element(&card_json));

        let text_json = serde_json::to_value(text("hello", None)).unwrap();
        assert!(!is_card_element(&text_json));

        let plain_object = serde_json::json!({"foo": "bar"});
        assert!(!is_card_element(&plain_object));

        let null_value = serde_json::Value::Null;
        assert!(!is_card_element(&null_value));
    }

    #[test]
    fn card_child_untagged_round_trip_handles_every_variant() {
        let payload: Vec<CardChild> = vec![
            text("hello", Some(TextStyle::Bold)).into(),
            image("https://example.com/x.png", Some("alt".to_string())).into(),
            divider().into(),
            actions(vec![]).into(),
            section(vec![text("nested", None).into()]).into(),
            fields(vec![field("k", "v")]).into(),
            card_link("https://example.com", "Open").into(),
            table(TableOptions {
                headers: vec!["A".to_string()],
                rows: vec![vec!["1".to_string()]],
                align: None,
            })
            .into(),
        ];
        let json = serde_json::to_string(&payload).unwrap();
        let back: Vec<CardChild> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), payload.len());
        // Spot-check that variants round-tripped to the correct enum
        // arms (not the leftmost-untagged-match catch-all).
        assert!(matches!(back[0], CardChild::Text(_)));
        assert!(matches!(back[1], CardChild::Image(_)));
        assert!(matches!(back[2], CardChild::Divider(_)));
        assert!(matches!(back[3], CardChild::Actions(_)));
        assert!(matches!(back[4], CardChild::Section(_)));
        assert!(matches!(back[5], CardChild::Fields(_)));
        assert!(matches!(back[6], CardChild::Link(_)));
        assert!(matches!(back[7], CardChild::Table(_)));
    }

    #[test]
    fn card_link_builder_emits_link_discriminator() {
        let elem = card_link("https://example.com", "Visit");
        let json = serde_json::to_string(&elem).unwrap();
        assert_eq!(
            json,
            "{\"label\":\"Visit\",\"type\":\"link\",\"url\":\"https://example.com\"}"
        );
    }
}
