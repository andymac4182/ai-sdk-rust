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
    CardChild, CardElement, DividerElement, ImageElement, LinkElement, TextElement, TextStyle,
};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
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

/// 1:1 port of upstream `cardToGoogleCard(card, options?)`. Partial:
/// handles the foundation child branches (Text / Image / Divider /
/// Link). Actions / Section / Fields / Table / Select / RadioSelect
/// branches are deferred to follow-up slices — children whose branch
/// is not yet implemented currently emit no widgets, and the section
/// they fall into is still emitted (with the placeholder fallback
/// if no widgets were produced for the whole card).
pub fn card_to_google_card(card: &CardElement, options: CardConversionOptions) -> Value {
    let mut sections: Vec<Value> = Vec::new();
    let mut current_widgets: Vec<Value> = Vec::new();

    let header = build_header(card);

    for child in &card.children {
        match child {
            CardChild::Section(_) => {
                // Section flushes pending widgets then emits its own
                // dedicated section. Renderer for section children
                // is implemented in a follow-up slice; emit an empty
                // section here so the section-count assertions stay
                // accurate.
                if !current_widgets.is_empty() {
                    sections.push(json!({ "widgets": std::mem::take(&mut current_widgets) }));
                }
                sections.push(json!({ "widgets": [] }));
            }
            other => {
                current_widgets.extend(convert_child_to_widgets(other));
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

fn convert_child_to_widgets(child: &CardChild) -> Vec<Value> {
    match child {
        CardChild::Text(t) => vec![convert_text_to_widget(t)],
        CardChild::Image(i) => vec![convert_image_to_widget(i)],
        CardChild::Divider(d) => vec![convert_divider_to_widget(d)],
        CardChild::Link(l) => vec![convert_link_to_widget(l)],
        // Actions / Section / Fields / Table / Select / RadioSelect
        // branches land in follow-up slices.
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
