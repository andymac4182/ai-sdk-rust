//! Teams card rendering.
//!
//! Partial 1:1 port of `packages/adapter-teams/src/cards.ts`.
//! Covers `cardToFallbackText` plus the simple-element subset of
//! `cardToAdaptiveCard`: structure (`AdaptiveCard` envelope with
//! `$schema` + `version` + `body`), title (`TextBlock` bolder/large),
//! subtitle (`TextBlock` subtle), `imageUrl` (`Image` stretch), and
//! the `Text` / `Image` / `Divider` / `Link` child branches. The
//! interactive branches (`Actions` / `Section` / `Fields` / `Table` +
//! `Select` / `RadioSelect`) land in follow-up slices.

use chat_sdk_adapter_shared::card_utils::{
    BoldFormat, FallbackTextOptions, LineBreak, PlatformName, card_to_fallback_text,
};
use chat_sdk_adapter_shared::card_utils::{PlatformName as CardPlatform, map_button_style};
use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, ButtonActionType, ButtonElement, CardChild, CardElement,
    DividerElement, FieldsElement, ImageElement, LinkButtonElement, LinkElement, SectionElement,
    TableElement, TextElement, TextStyle,
};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::modals::{RadioSelectElement, SelectElement};
use serde_json::{Value, json};

/// Convert emoji placeholders for Teams output. 1:1 with upstream
/// `createEmojiConverter("teams")`.
fn convert_emoji(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Teams, None)
}

/// Stable action id Teams uses for the implicit submit button
/// attached to inputs without an explicit submit action. 1:1 with
/// upstream `export const AUTO_SUBMIT_ACTION_ID = "__auto_submit"`.
pub const AUTO_SUBMIT_ACTION_ID: &str = "__auto_submit";

/// Adaptive Card JSON schema URL emitted by `cardToAdaptiveCard`.
/// 1:1 with upstream's private
/// `ADAPTIVE_CARD_SCHEMA = "http://adaptivecards.io/schemas/adaptive-card.json"`.
/// Exposed at module scope (rather than private as upstream) so
/// the schema URL can be referenced + asserted without re-declaring.
pub const ADAPTIVE_CARD_SCHEMA: &str = "http://adaptivecards.io/schemas/adaptive-card.json";

/// Adaptive Card version emitted by `cardToAdaptiveCard`. 1:1
/// with upstream's private `ADAPTIVE_CARD_VERSION = "1.4"`.
pub const ADAPTIVE_CARD_VERSION: &str = "1.4";

/// 1:1 port of upstream `cardToAdaptiveCard(card)`. Handles
/// title / subtitle / imageUrl / Text / Image / Divider / Actions
/// / Section / Fields / Link / Table branches plus Select /
/// RadioSelect inside Actions (with auto-injected submit when
/// inputs are present without buttons). Returns the AdaptiveCard
/// JSON shape directly — the upstream class wrapper
/// (`@microsoft/teams.cards`) is omitted because the assertions all
/// run against the serialised JSON via `toMatchObject({...})`.
pub fn card_to_adaptive_card(card: &CardElement) -> Value {
    let mut body: Vec<Value> = Vec::new();
    let mut actions: Vec<Value> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        body.push(json!({
            "type": "TextBlock",
            "text": convert_emoji(title),
            "weight": "Bolder",
            "size": "Large",
            "wrap": true,
        }));
    }

    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        body.push(json!({
            "type": "TextBlock",
            "text": convert_emoji(subtitle),
            "isSubtle": true,
            "wrap": true,
        }));
    }

    if let Some(image_url) = card.image_url.as_deref().filter(|u| !u.is_empty()) {
        body.push(json!({
            "type": "Image",
            "url": image_url,
            "size": "Stretch",
        }));
    }

    for child in &card.children {
        let result = convert_child_to_adaptive(child);
        body.extend(result.elements);
        actions.extend(result.actions);
    }

    let mut adaptive = json!({
        "type": "AdaptiveCard",
        "$schema": ADAPTIVE_CARD_SCHEMA,
        "version": ADAPTIVE_CARD_VERSION,
        "body": body,
    });

    if !actions.is_empty() {
        adaptive["actions"] = Value::Array(actions);
    }

    adaptive
}

/// 1:1 with upstream's inline `interface ConvertResult`. Bundles
/// the child's body-elements with any card-level actions emitted by
/// converting that child (Actions / Section branches).
#[derive(Debug, Default)]
struct ConvertResult {
    elements: Vec<Value>,
    actions: Vec<Value>,
}

/// 1:1 port of upstream `convertChildToAdaptive(child)`.
fn convert_child_to_adaptive(child: &CardChild) -> ConvertResult {
    match child {
        CardChild::Text(t) => ConvertResult {
            elements: vec![convert_text_to_element(t)],
            actions: Vec::new(),
        },
        CardChild::Image(i) => ConvertResult {
            elements: vec![convert_image_to_element(i)],
            actions: Vec::new(),
        },
        CardChild::Divider(d) => ConvertResult {
            elements: vec![convert_divider_to_element(d)],
            actions: Vec::new(),
        },
        CardChild::Actions(a) => convert_actions_to_elements(a),
        CardChild::Section(s) => convert_section_to_elements(s),
        CardChild::Fields(f) => ConvertResult {
            elements: vec![convert_fields_to_element(f)],
            actions: Vec::new(),
        },
        CardChild::Link(l) => ConvertResult {
            elements: vec![convert_link_to_element(l)],
            actions: Vec::new(),
        },
        CardChild::Table(t) => ConvertResult {
            elements: vec![convert_table_to_element(t)],
            actions: Vec::new(),
        },
    }
}

/// 1:1 with upstream `convertTextToElement(element)`. `bold` ->
/// `weight: "Bolder"`; `muted` -> `isSubtle: true`; default no
/// style modifier. All blocks set `wrap: true`.
fn convert_text_to_element(element: &TextElement) -> Value {
    let mut block = json!({
        "type": "TextBlock",
        "text": convert_emoji(&element.content),
        "wrap": true,
    });
    match element.style {
        Some(TextStyle::Bold) => {
            block["weight"] = Value::String("Bolder".to_string());
        }
        Some(TextStyle::Muted) => {
            block["isSubtle"] = Value::Bool(true);
        }
        _ => {}
    }
    block
}

/// 1:1 with upstream `convertImageToElement(element)`. `alt`
/// defaults to the literal `"Image"` when missing. `size` is always
/// `"Auto"` for body images (the card-level header image uses
/// `"Stretch"`).
fn convert_image_to_element(element: &ImageElement) -> Value {
    json!({
        "type": "Image",
        "url": element.url,
        "altText": element.alt.as_deref().unwrap_or("Image"),
        "size": "Auto",
    })
}

/// 1:1 with upstream `convertDividerToElement(_element)`. Adaptive
/// Cards has no dedicated divider — upstream uses an empty
/// `Container` with `separator: true`.
fn convert_divider_to_element(_element: &DividerElement) -> Value {
    json!({
        "type": "Container",
        "separator": true,
        "items": [],
    })
}

/// 1:1 with the upstream inline `link` branch of
/// `convertChildToAdaptive`. Emits a `TextBlock` containing a
/// standard markdown `[label](url)` link.
fn convert_link_to_element(element: &LinkElement) -> Value {
    json!({
        "type": "TextBlock",
        "text": format!("[{}]({})", convert_emoji(&element.label), element.url),
        "wrap": true,
    })
}

/// 1:1 with upstream `convertActionsToElements(element)`. Buttons /
/// LinkButtons become card-level `actions`. Select / RadioSelect
/// become body elements. When the Actions block contains inputs but
/// no buttons, auto-injects a `SubmitAction` with `actionId =
/// AUTO_SUBMIT_ACTION_ID` (Teams inputs don't auto-submit like
/// Slack does).
fn convert_actions_to_elements(element: &ActionsElement) -> ConvertResult {
    let mut actions: Vec<Value> = Vec::new();
    let mut elements: Vec<Value> = Vec::new();
    let mut has_buttons = false;
    let mut has_inputs = false;

    for child in &element.children {
        match child {
            ActionsChild::Button(b) => {
                has_buttons = true;
                actions.push(convert_button_to_action(b));
            }
            ActionsChild::LinkButton(b) => {
                actions.push(convert_link_button_to_action(b));
            }
            ActionsChild::Select(s) => {
                has_inputs = true;
                elements.push(convert_select_to_element(s));
            }
            ActionsChild::RadioSelect(r) => {
                has_inputs = true;
                elements.push(convert_radio_select_to_element(r));
            }
        }
    }

    if has_inputs && !has_buttons {
        actions.push(json!({
            "type": "Action.Submit",
            "title": "Submit",
            "data": { "actionId": AUTO_SUBMIT_ACTION_ID },
        }));
    }

    ConvertResult { elements, actions }
}

/// 1:1 with upstream `convertButtonToAction(button)`. Emits an
/// `Action.Submit` carrying `actionId` + `value` in `data`. For
/// `actionType: "modal"` buttons, also adds `msteams: { type:
/// "task/fetch" }` to the data hash so Teams opens a dialog.
fn convert_button_to_action(button: &ButtonElement) -> Value {
    let mut data = json!({
        "actionId": button.id,
        "value": button.value,
    });

    if button.action_type == Some(ButtonActionType::Modal) {
        data["msteams"] = json!({ "type": "task/fetch" });
    }

    let mut action = json!({
        "type": "Action.Submit",
        "title": convert_emoji(&button.label),
        "data": data,
    });

    if let Some(style) = map_button_style(button.style, CardPlatform::Teams) {
        action["style"] = Value::String(style.to_string());
    }

    action
}

/// 1:1 with upstream `convertLinkButtonToAction(button)`. Emits an
/// `Action.OpenUrl` action carrying the link URL.
fn convert_link_button_to_action(button: &LinkButtonElement) -> Value {
    let mut action = json!({
        "type": "Action.OpenUrl",
        "title": convert_emoji(&button.label),
        "url": button.url,
    });

    if let Some(style) = map_button_style(button.style, CardPlatform::Teams) {
        action["style"] = Value::String(style.to_string());
    }

    action
}

/// 1:1 with upstream `convertSelectToElement(select)`. Emits an
/// `Input.ChoiceSet` with `style: "compact"`. `isRequired` is the
/// inverse of `select.optional` (default required).
fn convert_select_to_element(select: &SelectElement) -> Value {
    let choices: Vec<Value> = select
        .options
        .iter()
        .map(|opt| {
            json!({
                "title": convert_emoji(&opt.label),
                "value": opt.value,
            })
        })
        .collect();

    let mut element = json!({
        "type": "Input.ChoiceSet",
        "id": select.id,
        "label": convert_emoji(&select.label),
        "style": "compact",
        "isRequired": !select.optional.unwrap_or(false),
        "choices": choices,
    });

    if let Some(placeholder) = select.placeholder.as_deref().filter(|p| !p.is_empty()) {
        element["placeholder"] = Value::String(placeholder.to_string());
    }
    if let Some(initial) = select.initial_option.as_deref().filter(|v| !v.is_empty()) {
        element["value"] = Value::String(initial.to_string());
    }

    element
}

/// 1:1 with upstream `convertRadioSelectToElement(radioSelect)`.
/// Emits an `Input.ChoiceSet` with `style: "expanded"` (no
/// placeholder for radio inputs).
fn convert_radio_select_to_element(radio: &RadioSelectElement) -> Value {
    let choices: Vec<Value> = radio
        .options
        .iter()
        .map(|opt| {
            json!({
                "title": convert_emoji(&opt.label),
                "value": opt.value,
            })
        })
        .collect();

    let mut element = json!({
        "type": "Input.ChoiceSet",
        "id": radio.id,
        "label": convert_emoji(&radio.label),
        "style": "expanded",
        "isRequired": !radio.optional.unwrap_or(false),
        "choices": choices,
    });

    if let Some(initial) = radio.initial_option.as_deref().filter(|v| !v.is_empty()) {
        element["value"] = Value::String(initial.to_string());
    }

    element
}

/// 1:1 with upstream `convertSectionToElements(element)`. Wraps
/// the section's body-elements in a `Container`; card-level
/// actions accumulated by child Actions blocks bubble up
/// untouched.
fn convert_section_to_elements(element: &SectionElement) -> ConvertResult {
    let mut container_items: Vec<Value> = Vec::new();
    let mut actions: Vec<Value> = Vec::new();

    for child in &element.children {
        let result = convert_child_to_adaptive(child);
        container_items.extend(result.elements);
        actions.extend(result.actions);
    }

    let mut elements: Vec<Value> = Vec::new();
    if !container_items.is_empty() {
        elements.push(json!({
            "type": "Container",
            "items": container_items,
        }));
    }

    ConvertResult { elements, actions }
}

/// 1:1 with upstream `convertFieldsToElement(element)`. Emits a
/// `FactSet` of `{ title, value }` facts.
fn convert_fields_to_element(element: &FieldsElement) -> Value {
    let facts: Vec<Value> = element
        .children
        .iter()
        .map(|field| {
            json!({
                "title": convert_emoji(&field.label),
                "value": convert_emoji(&field.value),
            })
        })
        .collect();

    json!({
        "type": "FactSet",
        "facts": facts,
    })
}

/// 1:1 with upstream `convertTableToElement(element)`. Emits a
/// `Container` of `ColumnSet`s — first the header row with bolder
/// text, then a `ColumnSet` per data row. Each cell is a
/// stretch-width `Column` containing a wrapped `TextBlock`.
fn convert_table_to_element(element: &TableElement) -> Value {
    let header_columns: Vec<Value> = element
        .headers
        .iter()
        .map(|h| {
            json!({
                "type": "Column",
                "width": "stretch",
                "items": [
                    {
                        "type": "TextBlock",
                        "text": convert_emoji(h),
                        "weight": "Bolder",
                        "wrap": true,
                    }
                ],
            })
        })
        .collect();

    let mut items: Vec<Value> = Vec::new();
    items.push(json!({ "type": "ColumnSet", "columns": header_columns }));

    for row in &element.rows {
        let cols: Vec<Value> = row
            .iter()
            .map(|cell| {
                json!({
                    "type": "Column",
                    "width": "stretch",
                    "items": [
                        {
                            "type": "TextBlock",
                            "text": convert_emoji(cell),
                            "wrap": true,
                        }
                    ],
                })
            })
            .collect();
        items.push(json!({ "type": "ColumnSet", "columns": cols }));
    }

    json!({
        "type": "Container",
        "items": items,
    })
}

/// Render a [`CardElement`] as Teams markdown fallback text. 1:1
/// port of upstream `cardToFallbackText(card)`: delegates to the
/// shared helper with `boldFormat: "**"`, `lineBreak: "\n\n"`, and
/// the `"teams"` emoji platform.
pub fn card_to_fallback_text_teams(card: &CardElement) -> String {
    card_to_fallback_text(
        card,
        FallbackTextOptions {
            bold_format: Some(BoldFormat::Double),
            line_break: Some(LineBreak::Double),
            platform: Some(PlatformName::Teams),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{
        ActionsChild, ActionsElement, ActionsKind, ButtonElement, ButtonKind, CardChild, CardKind,
        FieldElement, FieldKind, FieldsElement, FieldsKind, TextElement, TextKind,
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

    fn text(content: &str) -> CardChild {
        CardChild::Text(TextElement {
            content: content.to_string(),
            style: None,
            kind: TextKind::Text,
        })
    }

    // ---------- cardToFallbackText (2 upstream cases) ----------

    #[test]
    fn generates_fallback_text_for_a_card() {
        let c = card(
            Some("Order Update"),
            Some("Status changed"),
            vec![
                text("Your order is ready"),
                CardChild::Fields(FieldsElement {
                    children: vec![
                        FieldElement {
                            label: "Order ID".to_string(),
                            value: "#1234".to_string(),
                            kind: FieldKind::Field,
                        },
                        FieldElement {
                            label: "Status".to_string(),
                            value: "Ready".to_string(),
                            kind: FieldKind::Field,
                        },
                    ],
                    kind: FieldsKind::Fields,
                }),
                CardChild::Actions(ActionsElement {
                    children: vec![
                        ActionsChild::Button(ButtonElement {
                            action_type: None,
                            callback_url: None,
                            disabled: None,
                            id: "pickup".to_string(),
                            label: "Schedule Pickup".to_string(),
                            style: None,
                            kind: ButtonKind::Button,
                            value: None,
                        }),
                        ActionsChild::Button(ButtonElement {
                            action_type: None,
                            callback_url: None,
                            disabled: None,
                            id: "delay".to_string(),
                            label: "Delay".to_string(),
                            style: None,
                            kind: ButtonKind::Button,
                            value: None,
                        }),
                    ],
                    kind: ActionsKind::Actions,
                }),
            ],
        );

        let result = card_to_fallback_text_teams(&c);

        assert!(result.contains("**Order Update**"), "got: {result}");
        assert!(result.contains("Status changed"), "got: {result}");
        assert!(result.contains("Your order is ready"), "got: {result}");
        assert!(result.contains("Order ID: #1234"), "got: {result}");
        assert!(result.contains("Status: Ready"), "got: {result}");
        // Actions are intentionally excluded from fallback text — interactive
        // elements aren't meaningful in notifications.
        assert!(
            !result.contains("[Schedule Pickup]"),
            "actions leaked: {result}"
        );
        assert!(!result.contains("[Delay]"), "actions leaked: {result}");
    }

    #[test]
    fn handles_card_with_only_title() {
        let c = card(Some("Simple Card"), None, vec![]);
        assert_eq!(card_to_fallback_text_teams(&c), "**Simple Card**");
    }

    #[test]
    fn adaptive_card_schema_and_version_match_upstream() {
        // 1:1 with upstream `ADAPTIVE_CARD_SCHEMA = "http://adapt..."`
        // and `ADAPTIVE_CARD_VERSION = "1.4"`. The Adaptive Card JSON
        // renderer is deferred; these constants are exposed for any
        // caller / future renderer that needs them.
        assert_eq!(
            ADAPTIVE_CARD_SCHEMA,
            "http://adaptivecards.io/schemas/adaptive-card.json"
        );
        assert_eq!(ADAPTIVE_CARD_VERSION, "1.4");
    }

    #[test]
    fn auto_submit_action_id_matches_upstream() {
        // 1:1 with upstream `export const AUTO_SUBMIT_ACTION_ID =
        // "__auto_submit"`. Stable identifier used by
        // `cardToAdaptiveCard` when wiring up inputs without an
        // explicit submit action.
        assert_eq!(AUTO_SUBMIT_ACTION_ID, "__auto_submit");
    }

    // ---------- cardToAdaptiveCard (7 of ~20 portable cases) ----------
    // 1:1 with upstream `cards.test.ts > describe("cardToAdaptiveCard")`.
    // Covers structure / title / subtitle / imageUrl / Text / Image /
    // Divider. Actions / Section / Fields / Table / Select /
    // RadioSelect / CardLink branches land in follow-up slices.

    use chat_sdk_chat::cards::{DividerElement, DividerKind, ImageElement, ImageKind, TextStyle};

    #[test]
    fn adaptive_card_creates_a_valid_adaptive_card_structure() {
        let c = card(Some("Test"), None, vec![]);
        let adaptive = card_to_adaptive_card(&c);
        assert_eq!(adaptive["type"], "AdaptiveCard");
        assert_eq!(
            adaptive["$schema"],
            "http://adaptivecards.io/schemas/adaptive-card.json"
        );
        assert_eq!(adaptive["version"], "1.4");
        assert!(adaptive["body"].is_array(), "got: {adaptive}");
    }

    #[test]
    fn adaptive_card_converts_a_card_with_title() {
        let c = card(Some("Welcome Message"), None, vec![]);
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "TextBlock");
        assert_eq!(body[0]["text"], "Welcome Message");
        assert_eq!(body[0]["weight"], "Bolder");
        assert_eq!(body[0]["size"], "Large");
        assert_eq!(body[0]["wrap"], true);
    }

    #[test]
    fn adaptive_card_converts_a_card_with_title_and_subtitle() {
        let c = card(
            Some("Order Update"),
            Some("Your package is on its way"),
            vec![],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 2);
        assert_eq!(body[1]["type"], "TextBlock");
        assert_eq!(body[1]["text"], "Your package is on its way");
        assert_eq!(body[1]["isSubtle"], true);
        assert_eq!(body[1]["wrap"], true);
    }

    #[test]
    fn adaptive_card_converts_a_card_with_header_image() {
        let c = CardElement {
            title: Some("Product".to_string()),
            subtitle: None,
            image_url: Some("https://example.com/product.png".to_string()),
            kind: CardKind::Card,
            children: vec![],
        };
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 2);
        assert_eq!(body[1]["type"], "Image");
        assert_eq!(body[1]["url"], "https://example.com/product.png");
        assert_eq!(body[1]["size"], "Stretch");
    }

    #[test]
    fn adaptive_card_converts_text_elements() {
        let c = card(
            None,
            None,
            vec![
                text("Regular text"),
                CardChild::Text(TextElement {
                    content: "Bold text".to_string(),
                    style: Some(TextStyle::Bold),
                    kind: TextKind::Text,
                }),
                CardChild::Text(TextElement {
                    content: "Muted text".to_string(),
                    style: Some(TextStyle::Muted),
                    kind: TextKind::Text,
                }),
            ],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 3);

        assert_eq!(body[0]["type"], "TextBlock");
        assert_eq!(body[0]["text"], "Regular text");
        assert_eq!(body[0]["wrap"], true);

        assert_eq!(body[1]["type"], "TextBlock");
        assert_eq!(body[1]["text"], "Bold text");
        assert_eq!(body[1]["wrap"], true);
        assert_eq!(body[1]["weight"], "Bolder");

        assert_eq!(body[2]["type"], "TextBlock");
        assert_eq!(body[2]["text"], "Muted text");
        assert_eq!(body[2]["wrap"], true);
        assert_eq!(body[2]["isSubtle"], true);
    }

    #[test]
    fn adaptive_card_converts_image_elements() {
        let c = card(
            None,
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/img.png".to_string(),
                alt: Some("My image".to_string()),
                kind: ImageKind::Image,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Image");
        assert_eq!(body[0]["url"], "https://example.com/img.png");
        assert_eq!(body[0]["altText"], "My image");
        assert_eq!(body[0]["size"], "Auto");
    }

    #[test]
    fn adaptive_card_converts_divider_elements() {
        let c = card(
            None,
            None,
            vec![CardChild::Divider(DividerElement {
                kind: DividerKind::Divider,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Container");
        assert_eq!(body[0]["separator"], true);
        assert_eq!(body[0]["items"], serde_json::json!([]));
    }

    // ---------- cardToAdaptiveCard interactive branches (10 upstream cases + 1 additive Table) ----------

    use chat_sdk_chat::cards::{
        ButtonActionType, ButtonStyle, LinkButtonElement, LinkButtonKind, LinkKind,
    };
    use chat_sdk_chat::modals::{
        RadioSelectElement, RadioSelectKind, SelectElement, SelectKind, SelectOptionElement,
    };

    fn button(
        id: &str,
        label: &str,
        style: Option<ButtonStyle>,
        value: Option<&str>,
    ) -> ButtonElement {
        ButtonElement {
            action_type: None,
            callback_url: None,
            disabled: None,
            id: id.to_string(),
            label: label.to_string(),
            style,
            kind: ButtonKind::Button,
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn adaptive_card_converts_actions_with_buttons_to_card_level_actions() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button(
                        "approve",
                        "Approve",
                        Some(ButtonStyle::Primary),
                        None,
                    )),
                    ActionsChild::Button(button(
                        "reject",
                        "Reject",
                        Some(ButtonStyle::Danger),
                        Some("data-123"),
                    )),
                    ActionsChild::Button(button("skip", "Skip", None, None)),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        assert_eq!(adaptive["body"].as_array().unwrap().len(), 0);
        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);

        assert_eq!(actions[0]["type"], "Action.Submit");
        assert_eq!(actions[0]["title"], "Approve");
        assert_eq!(actions[0]["data"]["actionId"], "approve");
        assert!(actions[0]["data"]["value"].is_null());
        assert_eq!(actions[0]["style"], "positive");

        assert_eq!(actions[1]["type"], "Action.Submit");
        assert_eq!(actions[1]["title"], "Reject");
        assert_eq!(actions[1]["data"]["actionId"], "reject");
        assert_eq!(actions[1]["data"]["value"], "data-123");
        assert_eq!(actions[1]["style"], "destructive");

        assert_eq!(actions[2]["type"], "Action.Submit");
        assert_eq!(actions[2]["title"], "Skip");
        assert_eq!(actions[2]["data"]["actionId"], "skip");
        assert!(actions[2]["data"]["value"].is_null());
    }

    #[test]
    fn adaptive_card_converts_link_buttons_to_action_open_url() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::LinkButton(LinkButtonElement {
                    label: "View Docs".to_string(),
                    style: Some(ButtonStyle::Primary),
                    kind: LinkButtonKind::LinkButton,
                    url: "https://example.com/docs".to_string(),
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["type"], "Action.OpenUrl");
        assert_eq!(actions[0]["title"], "View Docs");
        assert_eq!(actions[0]["url"], "https://example.com/docs");
        assert_eq!(actions[0]["style"], "positive");
    }

    #[test]
    fn adaptive_card_converts_fields_to_fact_set() {
        let c = card(
            None,
            None,
            vec![CardChild::Fields(FieldsElement {
                children: vec![
                    FieldElement {
                        label: "Status".to_string(),
                        value: "Active".to_string(),
                        kind: FieldKind::Field,
                    },
                    FieldElement {
                        label: "Priority".to_string(),
                        value: "High".to_string(),
                        kind: FieldKind::Field,
                    },
                ],
                kind: FieldsKind::Fields,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "FactSet");
        assert_eq!(
            body[0]["facts"],
            serde_json::json!([
                { "title": "Status", "value": "Active" },
                { "title": "Priority", "value": "High" },
            ])
        );
    }

    #[test]
    fn adaptive_card_wraps_section_children_in_a_container() {
        use chat_sdk_chat::cards::{SectionElement, SectionKind};
        let c = card(
            None,
            None,
            vec![CardChild::Section(SectionElement {
                children: vec![text("Inside section")],
                kind: SectionKind::Section,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Container");
        assert_eq!(body[0]["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn adaptive_card_converts_a_complete_card() {
        let c = CardElement {
            title: Some("Order #1234".to_string()),
            subtitle: Some("Status update".to_string()),
            image_url: None,
            kind: CardKind::Card,
            children: vec![
                text("Your order has been shipped!"),
                CardChild::Fields(FieldsElement {
                    children: vec![
                        FieldElement {
                            label: "Tracking".to_string(),
                            value: "ABC123".to_string(),
                            kind: FieldKind::Field,
                        },
                        FieldElement {
                            label: "ETA".to_string(),
                            value: "Dec 25".to_string(),
                            kind: FieldKind::Field,
                        },
                    ],
                    kind: FieldsKind::Fields,
                }),
                CardChild::Actions(ActionsElement {
                    children: vec![ActionsChild::Button(button(
                        "track",
                        "Track Package",
                        Some(ButtonStyle::Primary),
                        None,
                    ))],
                    kind: ActionsKind::Actions,
                }),
            ],
        };
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 4);
        assert_eq!(body[0]["type"], "TextBlock"); // title
        assert_eq!(body[1]["type"], "TextBlock"); // subtitle
        assert_eq!(body[2]["type"], "TextBlock"); // text
        assert_eq!(body[3]["type"], "FactSet"); // fields
        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["title"], "Track Package");
    }

    #[test]
    fn adaptive_card_adds_msteams_task_fetch_hint_for_action_type_modal() {
        let mut b = button("open-dialog", "Open", None, None);
        b.action_type = Some(ButtonActionType::Modal);
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Button(b)],
                kind: ActionsKind::Actions,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["type"], "Action.Submit");
        assert_eq!(actions[0]["title"], "Open");
        assert_eq!(actions[0]["data"]["actionId"], "open-dialog");
        assert_eq!(
            actions[0]["data"]["msteams"],
            serde_json::json!({ "type": "task/fetch" })
        );
    }

    fn opt(label: &str, value: &str) -> SelectOptionElement {
        SelectOptionElement {
            description: None,
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn adaptive_card_converts_select_to_compact_choice_set_input_in_body() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Select(SelectElement {
                    id: "color".to_string(),
                    initial_option: None,
                    label: "Pick a color".to_string(),
                    optional: None,
                    options: vec![opt("Red", "red"), opt("Blue", "blue")],
                    placeholder: Some("Choose...".to_string()),
                    kind: SelectKind::Select,
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Input.ChoiceSet");
        assert_eq!(body[0]["id"], "color");
        assert_eq!(body[0]["label"], "Pick a color");
        assert_eq!(body[0]["style"], "compact");
        assert_eq!(body[0]["isRequired"], true);
        assert_eq!(body[0]["placeholder"], "Choose...");
        let choices = body[0]["choices"].as_array().unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0]["title"], "Red");
        assert_eq!(choices[0]["value"], "red");

        // Auto-injects submit
        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["type"], "Action.Submit");
        assert_eq!(actions[0]["title"], "Submit");
        assert_eq!(actions[0]["data"]["actionId"], "__auto_submit");
    }

    #[test]
    fn adaptive_card_converts_radio_select_to_expanded_choice_set_input_in_body() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::RadioSelect(RadioSelectElement {
                    id: "plan".to_string(),
                    initial_option: None,
                    label: "Choose Plan".to_string(),
                    optional: None,
                    options: vec![opt("Free", "free"), opt("Pro", "pro")],
                    kind: RadioSelectKind::RadioSelect,
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Input.ChoiceSet");
        assert_eq!(body[0]["id"], "plan");
        assert_eq!(body[0]["label"], "Choose Plan");
        assert_eq!(body[0]["style"], "expanded");
        assert_eq!(body[0]["isRequired"], true);

        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["type"], "Action.Submit");
        assert_eq!(actions[0]["data"]["actionId"], "__auto_submit");
    }

    #[test]
    fn adaptive_card_does_not_auto_inject_submit_when_buttons_are_present() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Select(SelectElement {
                        id: "color".to_string(),
                        initial_option: None,
                        label: "Color".to_string(),
                        optional: None,
                        options: vec![opt("Red", "red")],
                        placeholder: None,
                        kind: SelectKind::Select,
                    }),
                    ActionsChild::Button(button(
                        "submit",
                        "Submit",
                        Some(ButtonStyle::Primary),
                        None,
                    )),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Input.ChoiceSet");
        assert_eq!(body[0]["id"], "color");
        let actions = adaptive["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["type"], "Action.Submit");
        assert_eq!(actions[0]["title"], "Submit");
    }

    #[test]
    fn adaptive_card_converts_cardlink_to_text_block_with_markdown_link() {
        use chat_sdk_chat::cards::LinkElement;
        let c = card(
            None,
            None,
            vec![CardChild::Link(LinkElement {
                label: "Click here".to_string(),
                kind: LinkKind::Link,
                url: "https://example.com".to_string(),
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "TextBlock");
        assert_eq!(body[0]["text"], "[Click here](https://example.com)");
        assert_eq!(body[0]["wrap"], true);
    }

    // ---------- additive Table coverage ----------

    #[test]
    fn adaptive_card_converts_table_to_container_of_column_sets() {
        use chat_sdk_chat::cards::{TableElement, TableKind};
        let c = card(
            None,
            None,
            vec![CardChild::Table(TableElement {
                align: None,
                headers: vec!["Name".to_string(), "Status".to_string()],
                rows: vec![
                    vec!["Alpha".to_string(), "OK".to_string()],
                    vec!["Beta".to_string(), "FAIL".to_string()],
                ],
                kind: TableKind::Table,
            })],
        );
        let adaptive = card_to_adaptive_card(&c);
        let body = adaptive["body"].as_array().unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["type"], "Container");
        let items = body[0]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3); // header + 2 rows
        assert_eq!(items[0]["type"], "ColumnSet");
        let header_cols = items[0]["columns"].as_array().unwrap();
        assert_eq!(header_cols.len(), 2);
        assert_eq!(header_cols[0]["items"][0]["text"], "Name");
        assert_eq!(header_cols[0]["items"][0]["weight"], "Bolder");
        assert_eq!(items[1]["columns"][0]["items"][0]["text"], "Alpha");
        assert_eq!(items[2]["columns"][1]["items"][0]["text"], "FAIL");
    }
}
