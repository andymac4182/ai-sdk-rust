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
use chat_sdk_chat::cards::{
    CardChild, CardElement, DividerElement, ImageElement, LinkElement, TextElement, TextStyle,
};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
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

/// 1:1 port of upstream `cardToAdaptiveCard(card)`. Partial:
/// handles title / subtitle / imageUrl / Text / Image / Divider /
/// Link branches. Action Row + Fields + Section + Table + Select /
/// RadioSelect branches are deferred to follow-up slices. Returns
/// the AdaptiveCard JSON shape directly — the upstream class wrapper
/// (`@microsoft/teams.cards`) is omitted because the assertions all
/// run against the serialised JSON via `toMatchObject({...})`.
pub fn card_to_adaptive_card(card: &CardElement) -> Value {
    let mut body: Vec<Value> = Vec::new();

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
        // Actions accumulator: collected but emitted at card-level,
        // not body-level. Currently no action-emitting branches are
        // implemented (deferred), so this is dropped.
    }

    json!({
        "type": "AdaptiveCard",
        "$schema": ADAPTIVE_CARD_SCHEMA,
        "version": ADAPTIVE_CARD_VERSION,
        "body": body,
    })
}

/// 1:1 with upstream's inline `interface ConvertResult`. Bundles
/// the child's body-elements with any card-level actions emitted by
/// converting that child (Actions / Section branches).
#[derive(Debug, Default)]
struct ConvertResult {
    elements: Vec<Value>,
    /// Card-level actions accumulator. Unused by the current
    /// branches (Action Row branch is deferred); kept on the type
    /// so future slices can plug `Actions` / `Section` results in
    /// without changing the function signature.
    #[allow(dead_code)]
    actions: Vec<Value>,
}

/// 1:1 port of upstream `convertChildToAdaptive(child)`. Returns
/// `ConvertResult { elements: [], actions: [] }` for any child whose
/// renderer branch is still deferred.
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
        CardChild::Link(l) => ConvertResult {
            elements: vec![convert_link_to_element(l)],
            actions: Vec::new(),
        },
        // Actions / Section / Fields / Table branches land in
        // follow-up slices.
        _ => ConvertResult::default(),
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

    use chat_sdk_chat::cards::{
        DividerElement, DividerKind, ImageElement, ImageKind, TextStyle,
    };

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
        let c = card(Some("Order Update"), Some("Your package is on its way"), vec![]);
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
}
