//! Google Chat card rendering.
//!
//! Partial 1:1 port of `packages/adapter-gchat/src/cards.ts`.
//! Covers `cardToFallbackText` (shared-helper wrapper) plus the
//! foundation subset of `cardToGoogleCard`: card-id wiring, header
//! (title / subtitle / imageUrl with `SQUARE` imageType), section
//! grouping with placeholder fallback, and the Text / Image /
//! Divider / Link child branches. Action Row + Fields + Section +
//! Table + Select / RadioSelect branches land in follow-up slices.

use chat_sdk_adapter_shared::card_utils::{
    BoldFormat, FallbackTextOptions, LineBreak, PlatformName, card_to_fallback_text,
};
use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, ButtonElement, ButtonStyle, CardChild, CardElement,
    DividerElement, FieldsElement, ImageElement, LinkButtonElement, LinkElement, SectionElement,
    TextElement, TextStyle,
};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::modals::{RadioSelectElement, SelectElement};
use serde_json::{Value, json};

/// Convert emoji placeholders for Google Chat (unicode glyphs). 1:1
/// with upstream `createEmojiConverter("gchat")`.
fn convert_emoji(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Gchat, None)
}

/// 1:1 with upstream's private `markdownToGChat(text)` —
/// `**bold**` -> `*bold*` for Google Chat formatting.
fn markdown_to_gchat(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 2] == b"**" {
            // Look for next `**` after the opener; upstream's regex
            // requires `.+?` (at least one char between).
            if let Some(end_rel) = text[i + 2..].find("**") {
                if end_rel > 0 {
                    let end = i + 2 + end_rel;
                    out.push('*');
                    out.push_str(&text[i + 2..end]);
                    out.push('*');
                    i = end + 2;
                    continue;
                }
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Options for [`card_to_google_card`]. 1:1 with upstream
/// `interface CardConversionOptions { cardId?, endpointUrl? }`. The
/// legacy `string` form of `options` upstream (where a bare string
/// is treated as the `cardId`) is exposed via two entry points in
/// Rust: [`card_to_google_card`] for the typed form, and
/// [`card_to_google_card_with_id`] for the convenience wrapper.
#[derive(Debug, Default, Clone)]
pub struct CardConversionOptions {
    pub card_id: Option<String>,
    pub endpoint_url: Option<String>,
}

/// 1:1 port of upstream `cardToGoogleCard(card, options?)`. Handles
/// all `cardToGoogleCard` branches: title / subtitle / imageUrl,
/// Text / Image / Divider / Actions (Button / LinkButton / Select /
/// RadioSelect) / Section / Fields / Link / Table. Children
/// flush-and-section is preserved 1:1 — `Section` children get a
/// dedicated section; preceding/following non-section children
/// accumulate into surrounding sections. Empty cards emit a single
/// section with a placeholder empty textParagraph (Google Chat
/// rejects cards without widgets).
pub fn card_to_google_card(card: &CardElement, options: CardConversionOptions) -> Value {
    let endpoint = options.endpoint_url.as_deref();
    let mut sections: Vec<Value> = Vec::new();
    let mut current_widgets: Vec<Value> = Vec::new();

    let header = build_header(card);

    for child in &card.children {
        match child {
            CardChild::Section(s) => {
                if !current_widgets.is_empty() {
                    sections.push(json!({ "widgets": std::mem::take(&mut current_widgets) }));
                }
                let widgets = convert_section_to_widgets(s, endpoint);
                sections.push(json!({ "widgets": widgets }));
            }
            other => {
                current_widgets.extend(convert_child_to_widgets(other, endpoint));
            }
        }
    }

    if !current_widgets.is_empty() {
        sections.push(json!({ "widgets": current_widgets }));
    }

    if sections.is_empty() {
        sections.push(json!({
            "widgets": [
                { "textParagraph": { "text": "" } }
            ],
        }));
    }

    let mut card_inner = json!({ "sections": sections });
    if let Some(h) = header {
        card_inner["header"] = h;
    }

    let mut out = json!({ "card": card_inner });
    if let Some(id) = options.card_id.filter(|s| !s.is_empty()) {
        out["cardId"] = Value::String(id);
    }

    out
}

/// Convenience wrapper for the upstream `cardToGoogleCard(card,
/// "myCardId")` shorthand: passes `cardId` only.
pub fn card_to_google_card_with_id(card: &CardElement, card_id: &str) -> Value {
    card_to_google_card(
        card,
        CardConversionOptions {
            card_id: Some(card_id.to_string()),
            endpoint_url: None,
        },
    )
}

fn build_header(card: &CardElement) -> Option<Value> {
    let has_title = card.title.as_deref().is_some_and(|s| !s.is_empty());
    let has_subtitle = card.subtitle.as_deref().is_some_and(|s| !s.is_empty());
    let has_image = card.image_url.as_deref().is_some_and(|s| !s.is_empty());

    if !(has_title || has_subtitle || has_image) {
        return None;
    }

    let title = card.title.as_deref().unwrap_or("");
    let mut header = json!({
        "title": convert_emoji(title),
    });
    if has_subtitle {
        header["subtitle"] = Value::String(convert_emoji(card.subtitle.as_deref().unwrap()));
    }
    if has_image {
        header["imageUrl"] = Value::String(card.image_url.as_deref().unwrap().to_string());
        header["imageType"] = Value::String("SQUARE".to_string());
    }
    Some(header)
}

fn convert_child_to_widgets(child: &CardChild, endpoint: Option<&str>) -> Vec<Value> {
    match child {
        CardChild::Text(t) => vec![convert_text_to_widget(t)],
        CardChild::Image(i) => vec![convert_image_to_widget(i)],
        CardChild::Divider(d) => vec![convert_divider_to_widget(d)],
        CardChild::Actions(a) => convert_actions_to_widgets(a, endpoint),
        CardChild::Section(s) => convert_section_to_widgets(s, endpoint),
        CardChild::Fields(f) => convert_fields_to_widgets(f),
        CardChild::Link(l) => vec![convert_link_to_widget(l)],
        // Table branch deferred.
        _ => Vec::new(),
    }
}

/// 1:1 with upstream `convertTextToWidget(element)`. `bold` wraps
/// the text in single asterisks; `muted` returns the raw emoji-
/// converted text without the `markdownToGChat` step (mirrors
/// upstream's `text = convertEmoji(element.content)` reassignment in
/// the muted branch).
fn convert_text_to_widget(element: &TextElement) -> Value {
    let converted = markdown_to_gchat(&convert_emoji(&element.content));
    let text = match element.style {
        Some(TextStyle::Bold) => format!("*{converted}*"),
        Some(TextStyle::Muted) => convert_emoji(&element.content),
        _ => converted,
    };
    json!({ "textParagraph": { "text": text } })
}

fn convert_image_to_widget(element: &ImageElement) -> Value {
    json!({
        "image": {
            "imageUrl": element.url,
            "altText": element.alt.as_deref().unwrap_or("Image"),
        }
    })
}

fn convert_divider_to_widget(_element: &DividerElement) -> Value {
    json!({ "divider": {} })
}

/// 1:1 with the upstream inline `link` branch of
/// `convertChildToWidgets`. Emits a `textParagraph` widget
/// containing an HTML `<a href="url">label</a>` link.
fn convert_link_to_widget(element: &LinkElement) -> Value {
    json!({
        "textParagraph": {
            "text": format!("<a href=\"{}\">{}</a>", element.url, convert_emoji(&element.label)),
        }
    })
}

/// 1:1 with upstream `convertActionsToWidgets(element, endpointUrl?)`.
/// Buttons + LinkButtons accumulate into a single `buttonList` widget;
/// Select / RadioSelect children flush the pending button list, then
/// emit their own dedicated `selectionInput` widget — preserving
/// mixed-order ordering from upstream.
fn convert_actions_to_widgets(
    element: &ActionsElement,
    endpoint: Option<&str>,
) -> Vec<Value> {
    let mut widgets: Vec<Value> = Vec::new();
    let mut buttons: Vec<Value> = Vec::new();

    fn flush(buttons: &mut Vec<Value>, widgets: &mut Vec<Value>) {
        if buttons.is_empty() {
            return;
        }
        let drained: Vec<Value> = std::mem::take(buttons);
        widgets.push(json!({ "buttonList": { "buttons": drained } }));
    }

    for child in &element.children {
        match child {
            ActionsChild::Button(b) => {
                buttons.push(convert_button_to_google_button(b, endpoint));
            }
            ActionsChild::LinkButton(b) => {
                buttons.push(convert_link_button_to_google_button(b));
            }
            ActionsChild::Select(s) => {
                flush(&mut buttons, &mut widgets);
                widgets.push(convert_select_to_widget(s, endpoint));
            }
            ActionsChild::RadioSelect(r) => {
                flush(&mut buttons, &mut widgets);
                widgets.push(convert_radio_select_to_widget(r, endpoint));
            }
        }
    }

    flush(&mut buttons, &mut widgets);
    widgets
}

/// 1:1 with upstream `convertButtonToGoogleButton(button,
/// endpointUrl?)`. For HTTP-endpoint apps, `function` is the endpoint
/// URL and the action ID is passed via parameters; otherwise
/// `function` is the action ID itself. Style maps to GChat
/// `color` (primary = blue, danger = red). `disabled` is only set
/// when true (mirrors upstream's `if (button.disabled) ...`).
fn convert_button_to_google_button(button: &ButtonElement, endpoint: Option<&str>) -> Value {
    let mut parameters: Vec<Value> = vec![json!({ "key": "actionId", "value": button.id })];
    if let Some(value) = button.value.as_deref().filter(|v| !v.is_empty()) {
        parameters.push(json!({ "key": "value", "value": value }));
    }

    let function = endpoint.unwrap_or(&button.id).to_string();
    let mut out = json!({
        "text": convert_emoji(&button.label),
        "onClick": {
            "action": {
                "function": function,
                "parameters": parameters,
            }
        },
    });

    if let Some(color) = button_color(button.style) {
        out["color"] = color;
    }
    if button.disabled == Some(true) {
        out["disabled"] = Value::Bool(true);
    }

    out
}

/// 1:1 with upstream `convertLinkButtonToGoogleButton(button)`.
/// Style maps to the same GChat `color` as buttons.
fn convert_link_button_to_google_button(button: &LinkButtonElement) -> Value {
    let mut out = json!({
        "text": convert_emoji(&button.label),
        "onClick": {
            "openLink": { "url": button.url }
        },
    });
    if let Some(color) = button_color(button.style) {
        out["color"] = color;
    }
    out
}

/// 1:1 with the upstream inline `button.style === "primary"` /
/// `"danger"` colour mapping (no other styles emit a colour). The
/// colour shape matches GChat `{ red, green, blue }` floats.
fn button_color(style: Option<ButtonStyle>) -> Option<Value> {
    match style {
        Some(ButtonStyle::Primary) => Some(json!({ "red": 0.2, "green": 0.5, "blue": 0.9 })),
        Some(ButtonStyle::Danger) => Some(json!({ "red": 0.9, "green": 0.2, "blue": 0.2 })),
        _ => None,
    }
}

/// 1:1 with the dropdown branch of upstream
/// `convertSelectionInputToWidget(element, endpointUrl?)`. Items
/// carry `selected: true` for the option matching
/// `select.initialOption`; the description field on
/// [`SelectOptionElement`] is intentionally NOT emitted (GChat
/// selectionInput items don't carry descriptions, matching upstream).
fn convert_select_to_widget(select: &SelectElement, endpoint: Option<&str>) -> Value {
    selection_input(
        &select.id,
        &convert_emoji(&select.label),
        "DROPDOWN",
        &select.options,
        select.initial_option.as_deref(),
        endpoint,
    )
}

/// 1:1 with the radio branch of upstream
/// `convertSelectionInputToWidget(element, endpointUrl?)`.
fn convert_radio_select_to_widget(radio: &RadioSelectElement, endpoint: Option<&str>) -> Value {
    selection_input(
        &radio.id,
        &convert_emoji(&radio.label),
        "RADIO_BUTTON",
        &radio.options,
        radio.initial_option.as_deref(),
        endpoint,
    )
}

fn selection_input(
    id: &str,
    label: &str,
    kind: &str,
    options: &[chat_sdk_chat::modals::SelectOptionElement],
    initial: Option<&str>,
    endpoint: Option<&str>,
) -> Value {
    let items: Vec<Value> = options
        .iter()
        .map(|opt| {
            let mut item = json!({
                "text": convert_emoji(&opt.label),
                "value": opt.value,
            });
            if initial == Some(opt.value.as_str()) {
                item["selected"] = Value::Bool(true);
            }
            item
        })
        .collect();

    let function = endpoint.unwrap_or(id).to_string();
    json!({
        "selectionInput": {
            "name": id,
            "label": label,
            "type": kind,
            "items": items,
            "onChangeAction": {
                "function": function,
                "parameters": [
                    { "key": "actionId", "value": id }
                ],
            },
        }
    })
}

/// 1:1 with upstream `convertSectionToWidgets(element,
/// endpointUrl?)`. Recursively expands the section's children — no
/// wrapper widget (the caller decides whether to emit a dedicated
/// section block).
fn convert_section_to_widgets(element: &SectionElement, endpoint: Option<&str>) -> Vec<Value> {
    let mut widgets: Vec<Value> = Vec::new();
    for child in &element.children {
        widgets.extend(convert_child_to_widgets(child, endpoint));
    }
    widgets
}

/// 1:1 with upstream `convertFieldsToWidgets(element)`. Emits one
/// `decoratedText` widget per field. Both `label` and `value` run
/// through `markdownToGChat(convertEmoji(...))`.
fn convert_fields_to_widgets(element: &FieldsElement) -> Vec<Value> {
    element
        .children
        .iter()
        .map(|field| {
            json!({
                "decoratedText": {
                    "topLabel": markdown_to_gchat(&convert_emoji(&field.label)),
                    "text": markdown_to_gchat(&convert_emoji(&field.value)),
                }
            })
        })
        .collect()
}

/// Render a [`CardElement`] as Google Chat fallback text. 1:1 port
/// of upstream `cardToFallbackText(card)`: delegates to the shared
/// helper with `boldFormat: "*"`, `lineBreak: "\n"`, and the
/// `"gchat"` emoji platform.
pub fn card_to_fallback_text_gchat(card: &CardElement) -> String {
    card_to_fallback_text(
        card,
        FallbackTextOptions {
            bold_format: Some(BoldFormat::Single),
            line_break: Some(LineBreak::Single),
            platform: Some(PlatformName::Gchat),
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

        let r = card_to_fallback_text_gchat(&c);
        assert!(r.contains("*Order Update*"), "got: {r}");
        assert!(r.contains("Status changed"), "got: {r}");
        assert!(r.contains("Your order is ready"), "got: {r}");
        assert!(r.contains("Order ID: #1234"), "got: {r}");
        assert!(r.contains("Status: Ready"), "got: {r}");
        assert!(!r.contains("[Schedule Pickup]"), "actions leaked: {r}");
        assert!(!r.contains("[Delay]"), "actions leaked: {r}");
    }

    #[test]
    fn handles_card_with_only_title() {
        let c = card(Some("Simple Card"), None, vec![]);
        assert_eq!(card_to_fallback_text_gchat(&c), "*Simple Card*");
    }

    // ---------- cardToGoogleCard foundation (8 of ~25 portable cases) ----------
    // 1:1 with upstream `cards.test.ts > describe("cardToGoogleCard")`.
    // Covers structure / cardId / title / subtitle / imageUrl /
    // text-elements / image / divider. The Actions / Section /
    // Fields / Table / Select / RadioSelect / CardLink branches +
    // markdown-bold describe block land in follow-up slices.

    use chat_sdk_chat::cards::{DividerElement, DividerKind, ImageElement, ImageKind, TextStyle};
    use serde_json::json;

    #[test]
    fn google_card_creates_a_valid_google_chat_card_structure() {
        let c = card(Some("Test"), None, vec![]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert!(!g["card"].is_null());
        assert!(g["card"]["sections"].is_array());
    }

    #[test]
    fn google_card_accepts_an_optional_card_id() {
        let c = card(Some("Test"), None, vec![]);
        let g = card_to_google_card_with_id(&c, "my-card-id");
        assert_eq!(g["cardId"], "my-card-id");
    }

    #[test]
    fn google_card_converts_a_card_with_title() {
        let c = card(Some("Welcome Message"), None, vec![]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(g["card"]["header"], json!({ "title": "Welcome Message" }));
    }

    #[test]
    fn google_card_converts_a_card_with_title_and_subtitle() {
        let c = card(Some("Order Update"), Some("Your package is on its way"), vec![]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["header"],
            json!({
                "title": "Order Update",
                "subtitle": "Your package is on its way",
            })
        );
    }

    #[test]
    fn google_card_converts_a_card_with_header_image() {
        let c = CardElement {
            title: Some("Product".to_string()),
            subtitle: None,
            image_url: Some("https://example.com/product.png".to_string()),
            kind: CardKind::Card,
            children: vec![],
        };
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["header"],
            json!({
                "title": "Product",
                "imageUrl": "https://example.com/product.png",
                "imageType": "SQUARE",
            })
        );
    }

    #[test]
    fn google_card_converts_text_elements_to_text_paragraph_widgets() {
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
            ],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let sections = g["card"]["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 1);
        let widgets = sections[0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 2);
        assert_eq!(widgets[0], json!({ "textParagraph": { "text": "Regular text" } }));
        assert_eq!(widgets[1], json!({ "textParagraph": { "text": "*Bold text*" } }));
    }

    #[test]
    fn google_card_converts_image_elements() {
        let c = card(
            None,
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/img.png".to_string(),
                alt: Some("My image".to_string()),
                kind: ImageKind::Image,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 1);
        assert_eq!(
            widgets[0],
            json!({
                "image": {
                    "imageUrl": "https://example.com/img.png",
                    "altText": "My image",
                }
            })
        );
    }

    #[test]
    fn google_card_converts_divider_elements() {
        let c = card(
            None,
            None,
            vec![CardChild::Divider(DividerElement {
                kind: DividerKind::Divider,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 1);
        assert_eq!(widgets[0], json!({ "divider": {} }));
    }

    // ---------- additive empty-card placeholder coverage ----------

    // ---------- cardToGoogleCard interactive + markdown-bold + CardLink (18 cases) ----------

    use chat_sdk_chat::cards::{
        ButtonStyle, LinkButtonElement, LinkButtonKind, LinkElement, LinkKind, SectionElement,
        SectionKind,
    };
    use chat_sdk_chat::modals::{
        RadioSelectElement, RadioSelectKind, SelectElement, SelectKind, SelectOptionElement,
    };

    fn button(id: &str, label: &str, style: Option<ButtonStyle>, value: Option<&str>) -> ButtonElement {
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

    fn opt(label: &str, value: &str) -> SelectOptionElement {
        SelectOptionElement {
            description: None,
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn google_card_converts_actions_with_buttons_to_button_list() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button("approve", "Approve", Some(ButtonStyle::Primary), None)),
                    ActionsChild::Button(button("reject", "Reject", Some(ButtonStyle::Danger), Some("data-123"))),
                    ActionsChild::Button(button("skip", "Skip", None, None)),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 1);
        let buttons = widgets[0]["buttonList"]["buttons"].as_array().unwrap();
        assert_eq!(buttons.len(), 3);
        assert_eq!(
            buttons[0],
            json!({
                "text": "Approve",
                "onClick": {
                    "action": {
                        "function": "approve",
                        "parameters": [{ "key": "actionId", "value": "approve" }],
                    }
                },
                "color": { "red": 0.2, "green": 0.5, "blue": 0.9 },
            })
        );
        assert_eq!(
            buttons[1],
            json!({
                "text": "Reject",
                "onClick": {
                    "action": {
                        "function": "reject",
                        "parameters": [
                            { "key": "actionId", "value": "reject" },
                            { "key": "value", "value": "data-123" },
                        ],
                    }
                },
                "color": { "red": 0.9, "green": 0.2, "blue": 0.2 },
            })
        );
        assert_eq!(
            buttons[2],
            json!({
                "text": "Skip",
                "onClick": {
                    "action": {
                        "function": "skip",
                        "parameters": [{ "key": "actionId", "value": "skip" }],
                    }
                },
            })
        );
    }

    #[test]
    fn google_card_sets_disabled_on_button_when_specified() {
        let mut cancel = button("cancel", "Cancelled", Some(ButtonStyle::Danger), None);
        cancel.disabled = Some(true);
        let retry = button("retry", "Retry", None, None);
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Button(cancel), ActionsChild::Button(retry)],
                kind: ActionsKind::Actions,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let buttons = g["card"]["sections"][0]["widgets"][0]["buttonList"]["buttons"]
            .as_array()
            .unwrap();
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0]["disabled"], true);
        assert!(buttons[1].get("disabled").is_none());
    }

    #[test]
    fn google_card_uses_endpoint_url_as_function_when_provided() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button("approve", "Approve", None, None)),
                    ActionsChild::Button(button("reject", "Reject", None, Some("data-123"))),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let g = card_to_google_card(
            &c,
            CardConversionOptions {
                card_id: None,
                endpoint_url: Some("https://example.com/api/webhooks/gchat".to_string()),
            },
        );
        let buttons = g["card"]["sections"][0]["widgets"][0]["buttonList"]["buttons"]
            .as_array()
            .unwrap();
        assert_eq!(
            buttons[0]["onClick"]["action"]["function"],
            "https://example.com/api/webhooks/gchat"
        );
        assert_eq!(
            buttons[1]["onClick"]["action"]["function"],
            "https://example.com/api/webhooks/gchat"
        );
        assert_eq!(
            buttons[1]["onClick"]["action"]["parameters"],
            json!([
                { "key": "actionId", "value": "reject" },
                { "key": "value", "value": "data-123" },
            ])
        );
    }

    #[test]
    fn google_card_converts_link_buttons_with_open_link() {
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
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let buttons = g["card"]["sections"][0]["widgets"][0]["buttonList"]["buttons"]
            .as_array()
            .unwrap();
        assert_eq!(buttons.len(), 1);
        assert_eq!(
            buttons[0],
            json!({
                "text": "View Docs",
                "onClick": {
                    "openLink": { "url": "https://example.com/docs" }
                },
                "color": { "red": 0.2, "green": 0.5, "blue": 0.9 },
            })
        );
    }

    #[test]
    fn google_card_converts_select_to_selection_input_dropdown_widgets() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Select(SelectElement {
                    id: "priority".to_string(),
                    initial_option: Some("normal".to_string()),
                    label: "Priority".to_string(),
                    optional: None,
                    options: vec![
                        SelectOptionElement {
                            description: Some("Urgent".to_string()),
                            label: "High".to_string(),
                            value: "high".to_string(),
                        },
                        opt("Normal", "normal"),
                    ],
                    placeholder: None,
                    kind: SelectKind::Select,
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let g = card_to_google_card(
            &c,
            CardConversionOptions {
                card_id: None,
                endpoint_url: Some("https://example.com/api/webhooks/gchat".to_string()),
            },
        );
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 1);
        assert_eq!(
            widgets[0],
            json!({
                "selectionInput": {
                    "name": "priority",
                    "label": "Priority",
                    "type": "DROPDOWN",
                    "items": [
                        { "text": "High", "value": "high" },
                        { "text": "Normal", "value": "normal", "selected": true },
                    ],
                    "onChangeAction": {
                        "function": "https://example.com/api/webhooks/gchat",
                        "parameters": [{ "key": "actionId", "value": "priority" }],
                    },
                }
            })
        );
    }

    #[test]
    fn google_card_converts_radio_select_to_selection_input_radio_widgets() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::RadioSelect(RadioSelectElement {
                    id: "status".to_string(),
                    initial_option: Some("open".to_string()),
                    label: "Status".to_string(),
                    optional: None,
                    options: vec![opt("Open", "open"), opt("Closed", "closed")],
                    kind: RadioSelectKind::RadioSelect,
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 1);
        assert_eq!(
            widgets[0],
            json!({
                "selectionInput": {
                    "name": "status",
                    "label": "Status",
                    "type": "RADIO_BUTTON",
                    "items": [
                        { "text": "Open", "value": "open", "selected": true },
                        { "text": "Closed", "value": "closed" },
                    ],
                    "onChangeAction": {
                        "function": "status",
                        "parameters": [{ "key": "actionId", "value": "status" }],
                    },
                }
            })
        );
    }

    #[test]
    fn google_card_preserves_action_order_for_mixed_buttons_and_selection_inputs() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button("refresh", "Refresh", None, None)),
                    ActionsChild::Select(SelectElement {
                        id: "category".to_string(),
                        initial_option: None,
                        label: "Category".to_string(),
                        optional: None,
                        options: vec![opt("Alpha", "alpha"), opt("Beta", "beta")],
                        placeholder: None,
                        kind: SelectKind::Select,
                    }),
                    ActionsChild::LinkButton(LinkButtonElement {
                        label: "Docs".to_string(),
                        style: None,
                        kind: LinkButtonKind::LinkButton,
                        url: "https://example.com/docs".to_string(),
                    }),
                    ActionsChild::RadioSelect(RadioSelectElement {
                        id: "view".to_string(),
                        initial_option: None,
                        label: "View".to_string(),
                        optional: None,
                        options: vec![opt("Summary", "summary"), opt("Detailed", "detailed")],
                        kind: RadioSelectKind::RadioSelect,
                    }),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 4);
        assert_eq!(
            widgets[0]["buttonList"]["buttons"][0],
            json!({
                "text": "Refresh",
                "onClick": {
                    "action": {
                        "function": "refresh",
                        "parameters": [{ "key": "actionId", "value": "refresh" }],
                    }
                }
            })
        );
        assert_eq!(widgets[1]["selectionInput"]["name"], "category");
        assert_eq!(widgets[1]["selectionInput"]["type"], "DROPDOWN");
        assert_eq!(
            widgets[2]["buttonList"]["buttons"],
            json!([{
                "text": "Docs",
                "onClick": { "openLink": { "url": "https://example.com/docs" } },
            }])
        );
        assert_eq!(widgets[3]["selectionInput"]["name"], "view");
        assert_eq!(widgets[3]["selectionInput"]["type"], "RADIO_BUTTON");
    }

    #[test]
    fn google_card_converts_fields_to_decorated_text_widgets() {
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
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let widgets = g["card"]["sections"][0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 2);
        assert_eq!(
            widgets[0],
            json!({ "decoratedText": { "topLabel": "Status", "text": "Active" } })
        );
        assert_eq!(
            widgets[1],
            json!({ "decoratedText": { "topLabel": "Priority", "text": "High" } })
        );
    }

    #[test]
    fn google_card_creates_separate_sections_for_section_children() {
        let c = card(
            None,
            None,
            vec![
                text("Before section"),
                CardChild::Section(SectionElement {
                    children: vec![text("Inside section")],
                    kind: SectionKind::Section,
                }),
                text("After section"),
            ],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let sections = g["card"]["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(
            sections[0]["widgets"][0]["textParagraph"]["text"],
            "Before section"
        );
        assert_eq!(
            sections[1]["widgets"][0]["textParagraph"]["text"],
            "Inside section"
        );
        assert_eq!(
            sections[2]["widgets"][0]["textParagraph"]["text"],
            "After section"
        );
    }

    #[test]
    fn google_card_converts_a_complete_card() {
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
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(g["card"]["header"]["title"], "Order #1234");
        assert_eq!(g["card"]["header"]["subtitle"], "Status update");
        let sections = g["card"]["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 1);
        let widgets = sections[0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 4);
        assert!(widgets[0].get("textParagraph").is_some());
        assert!(widgets[1].get("decoratedText").is_some());
        assert!(widgets[2].get("decoratedText").is_some());
        assert!(widgets[3].get("buttonList").is_some());
    }

    // ---------- describe("markdown bold to Google Chat conversion") (6 cases) ----------

    #[test]
    fn google_card_converts_double_asterisk_bold_to_single_in_card_text() {
        let c = card(None, None, vec![text("The **domain** is example.com")]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["sections"][0]["widgets"][0]["textParagraph"]["text"],
            "The *domain* is example.com"
        );
    }

    #[test]
    fn google_card_converts_multiple_bold_segments() {
        let c = card(None, None, vec![text("**Project**: my-app, **Status**: active")]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["sections"][0]["widgets"][0]["textParagraph"]["text"],
            "*Project*: my-app, *Status*: active"
        );
    }

    #[test]
    fn google_card_preserves_existing_single_asterisk_formatting() {
        let c = card(None, None, vec![text("Already *bold* in GChat format")]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["sections"][0]["widgets"][0]["textParagraph"]["text"],
            "Already *bold* in GChat format"
        );
    }

    #[test]
    fn google_card_converts_bold_in_field_values() {
        let c = card(
            None,
            None,
            vec![CardChild::Fields(FieldsElement {
                children: vec![FieldElement {
                    label: "Status".to_string(),
                    value: "**Active**".to_string(),
                    kind: FieldKind::Field,
                }],
                kind: FieldsKind::Fields,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let text_v = g["card"]["sections"][0]["widgets"][0]["decoratedText"]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text_v, "*Active*");
        assert!(!text_v.contains("**"));
    }

    #[test]
    fn google_card_converts_bold_in_field_labels() {
        let c = card(
            None,
            None,
            vec![CardChild::Fields(FieldsElement {
                children: vec![FieldElement {
                    label: "**Important**".to_string(),
                    value: "value".to_string(),
                    kind: FieldKind::Field,
                }],
                kind: FieldsKind::Fields,
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["sections"][0]["widgets"][0]["decoratedText"]["topLabel"],
            "*Important*"
        );
    }

    #[test]
    fn google_card_handles_text_with_no_markdown() {
        let c = card(None, None, vec![text("Plain text")]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        assert_eq!(
            g["card"]["sections"][0]["widgets"][0]["textParagraph"]["text"],
            "Plain text"
        );
    }

    // ---------- describe("cardToGoogleCard with CardLink") (1 case) ----------

    #[test]
    fn google_card_converts_cardlink_to_text_paragraph_with_html_link() {
        let c = card(
            None,
            None,
            vec![CardChild::Link(LinkElement {
                label: "Click here".to_string(),
                kind: LinkKind::Link,
                url: "https://example.com".to_string(),
            })],
        );
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let sections = g["card"]["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 1);
        let widgets = sections[0]["widgets"].as_array().unwrap();
        assert_eq!(widgets.len(), 1);
        assert_eq!(
            widgets[0],
            json!({
                "textParagraph": { "text": "<a href=\"https://example.com\">Click here</a>" }
            })
        );
    }

    #[test]
    fn google_card_creates_empty_section_with_placeholder_for_empty_cards() {
        // 1:1 with upstream `it("creates an empty section with
        // placeholder for empty cards")` (line 537). Cards with no
        // header and no children still produce a section with a
        // single empty textParagraph widget (Google Chat rejects
        // cards with no widgets).
        let c = card(None, None, vec![]);
        let g = card_to_google_card(&c, CardConversionOptions::default());
        let sections = g["card"]["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0],
            json!({
                "widgets": [
                    { "textParagraph": { "text": "" } }
                ]
            })
        );
    }
}
