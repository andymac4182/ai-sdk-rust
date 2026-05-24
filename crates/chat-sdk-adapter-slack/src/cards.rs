//! Slack card rendering.
//!
//! Partial 1:1 port of `packages/adapter-slack/src/cards.ts`.
//! Covers `cardToFallbackText` plus the simple-block subset of
//! `cardToBlockKit`: title (`header`), subtitle (`context`),
//! `imageUrl` (`image`), and the `Text` / `Image` / `Divider` /
//! `Link` child-block converters. The interactive branches
//! (`Actions` / `Section` / `Fields` / `Table` + Select / RadioSelect)
//! land in follow-up slices.

use crate::format::markdown_bold_to_slack_mrkdwn;
use chat_sdk_adapter_shared::card_utils::{
    BoldFormat, FallbackTextOptions, LineBreak, PlatformName, card_to_fallback_text,
    map_button_style,
};
use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, ButtonElement, CardChild, CardElement, DividerElement,
    FieldsElement, ImageElement, LinkButtonElement, LinkElement, SectionElement, TextElement,
    TextStyle,
};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use serde_json::{Value, json};

/// Convert emoji placeholders for Slack mrkdwn (`{{emoji:wave}}` ->
/// `:wave:`). 1:1 with upstream's `createEmojiConverter("slack")`.
fn convert_emoji(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Slack, None)
}

/// 1:1 with upstream's inline `markdownToMrkdwn(text)` —
/// `**bold**` -> `*bold*` for Slack mrkdwn.
fn markdown_to_mrkdwn(text: &str) -> String {
    markdown_bold_to_slack_mrkdwn(text)
}

/// A Slack Block Kit block, modelled as the raw JSON Value the
/// upstream renderer emits. 1:1 with upstream `interface SlackBlock
/// { block_id?; type; [key: string]: unknown }`.
pub type SlackBlock = Value;

/// 1:1 port of upstream `cardToBlockKit(card)`. Partial: handles
/// title / subtitle / imageUrl / Text / Image / Divider / Link
/// children. Action Row + Fields + Section + Table + Select /
/// RadioSelect branches are deferred to follow-up slices.
pub fn card_to_block_kit(card: &CardElement) -> Vec<SlackBlock> {
    let mut blocks: Vec<SlackBlock> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        blocks.push(json!({
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": convert_emoji(title),
                "emoji": true,
            },
        }));
    }

    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        blocks.push(json!({
            "type": "context",
            "elements": [
                { "type": "mrkdwn", "text": convert_emoji(subtitle) }
            ],
        }));
    }

    if let Some(image_url) = card.image_url.as_deref().filter(|u| !u.is_empty()) {
        let alt = card
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or("Card image");
        blocks.push(json!({
            "type": "image",
            "image_url": image_url,
            "alt_text": alt,
        }));
    }

    for child in &card.children {
        blocks.extend(convert_child_to_blocks(child));
    }

    blocks
}

/// 1:1 port of upstream `convertChildToBlocks(child, state)` — the
/// supported-child subset. Returns an empty Vec for any child whose
/// renderer branch is still deferred.
fn convert_child_to_blocks(child: &CardChild) -> Vec<SlackBlock> {
    match child {
        CardChild::Text(t) => vec![convert_text_to_block(t)],
        CardChild::Image(i) => vec![convert_image_to_block(i)],
        CardChild::Divider(d) => vec![convert_divider_to_block(d)],
        CardChild::Actions(a) => vec![convert_actions_to_block(a)],
        CardChild::Section(s) => convert_section_to_blocks(s),
        CardChild::Fields(f) => vec![convert_fields_to_block(f)],
        CardChild::Link(l) => vec![convert_link_to_block(l)],
        // Table + Select / RadioSelect branches land in follow-up
        // slices.
        _ => Vec::new(),
    }
}

/// 1:1 with upstream `convertTextToBlock(element)`. Emits a
/// `section` mrkdwn block by default; `style: "muted"` emits a
/// `context` block (Slack has no muted style — upstream's
/// approximation); `style: "bold"` wraps the converted text in
/// Slack-mrkdwn single-asterisk bold.
pub fn convert_text_to_block(element: &TextElement) -> SlackBlock {
    let text = markdown_to_mrkdwn(&convert_emoji(&element.content));
    match element.style {
        Some(TextStyle::Muted) => json!({
            "type": "context",
            "elements": [
                { "type": "mrkdwn", "text": text }
            ],
        }),
        Some(TextStyle::Bold) => json!({
            "type": "section",
            "text": { "type": "mrkdwn", "text": format!("*{text}*") },
        }),
        _ => json!({
            "type": "section",
            "text": { "type": "mrkdwn", "text": text },
        }),
    }
}

/// 1:1 with upstream `convertImageToBlock(element)`. `alt` defaults
/// to the literal `"Image"` when missing.
fn convert_image_to_block(element: &ImageElement) -> SlackBlock {
    json!({
        "type": "image",
        "image_url": element.url,
        "alt_text": element.alt.as_deref().unwrap_or("Image"),
    })
}

/// 1:1 with upstream `convertDividerToBlock(_element)`.
fn convert_divider_to_block(_element: &DividerElement) -> SlackBlock {
    json!({ "type": "divider" })
}

/// 1:1 with upstream `convertLinkToBlock(element)`. Emits a
/// `section` mrkdwn block with Slack's `<url|label>` link syntax.
fn convert_link_to_block(element: &LinkElement) -> SlackBlock {
    json!({
        "type": "section",
        "text": {
            "type": "mrkdwn",
            "text": format!("<{}|{}>", element.url, convert_emoji(&element.label)),
        },
    })
}

/// 1:1 with upstream `convertActionsToBlock(element)`. Iterates the
/// children union (Button / LinkButton / Select / RadioSelect) and
/// dispatches per `child.type`. Select / RadioSelect are deferred to
/// follow-up slices and omitted here (any non-button child is
/// silently dropped, matching upstream's plain `if/else if/return
/// convertButton...` chain).
fn convert_actions_to_block(element: &ActionsElement) -> SlackBlock {
    let elements: Vec<Value> = element
        .children
        .iter()
        .filter_map(|child| match child {
            ActionsChild::Button(b) => Some(convert_button_to_element(b)),
            ActionsChild::LinkButton(b) => Some(convert_link_button_to_element(b)),
            // Select / RadioSelect land in a follow-up slice.
            _ => None,
        })
        .collect();

    json!({
        "type": "actions",
        "elements": elements,
    })
}

/// 1:1 with upstream `convertButtonToElement(button)`. Always emits
/// `type: "button"`, `text: { type: "plain_text", emoji: true }`,
/// and `action_id`. `value` and `style` are only present when set.
fn convert_button_to_element(button: &ButtonElement) -> Value {
    let mut element = json!({
        "type": "button",
        "text": {
            "type": "plain_text",
            "text": convert_emoji(&button.label),
            "emoji": true,
        },
        "action_id": button.id,
    });

    if let Some(value) = button.value.as_deref().filter(|v| !v.is_empty()) {
        element["value"] = Value::String(value.to_string());
    }

    if let Some(style) = map_button_style(button.style, PlatformName::Slack) {
        element["style"] = Value::String(style.to_string());
    }

    element
}

/// 1:1 with upstream `convertLinkButtonToElement(button)`. Synthesises
/// `action_id` as `link-<first 200 chars of url>` (Slack's `action_id`
/// is capped at 255 chars; upstream uses 200 to leave room for the
/// `link-` prefix and emoji escaping).
fn convert_link_button_to_element(button: &LinkButtonElement) -> Value {
    let url_slice: String = button.url.chars().take(200).collect();
    let mut element = json!({
        "type": "button",
        "text": {
            "type": "plain_text",
            "text": convert_emoji(&button.label),
            "emoji": true,
        },
        "action_id": format!("link-{url_slice}"),
        "url": button.url,
    });

    if let Some(style) = map_button_style(button.style, PlatformName::Slack) {
        element["style"] = Value::String(style.to_string());
    }

    element
}

/// 1:1 with upstream `convertSectionToBlocks(element, state)`.
/// Recursively flattens section children into the parent block list.
fn convert_section_to_blocks(element: &SectionElement) -> Vec<SlackBlock> {
    let mut blocks: Vec<SlackBlock> = Vec::new();
    for child in &element.children {
        blocks.extend(convert_child_to_blocks(child));
    }
    blocks
}

/// 1:1 with upstream `convertFieldsToBlock(element)`. Emits a single
/// `section` block with a `fields` array; each field is
/// `*label*\nvalue` (label/value both run through
/// `markdownToMrkdwn(convertEmoji(...))`).
pub fn convert_fields_to_block(element: &FieldsElement) -> SlackBlock {
    let fields: Vec<Value> = element
        .children
        .iter()
        .map(|field| {
            let label = markdown_to_mrkdwn(&convert_emoji(&field.label));
            let value = markdown_to_mrkdwn(&convert_emoji(&field.value));
            json!({
                "type": "mrkdwn",
                "text": format!("*{label}*\n{value}"),
            })
        })
        .collect();

    json!({
        "type": "section",
        "fields": fields,
    })
}

/// Render a [`CardElement`] as Slack mrkdwn fallback text. 1:1 port
/// of upstream `cardToFallbackText(card)`: delegates to the shared
/// helper with `boldFormat: "*"`, `lineBreak: "\n"`, and the
/// `"slack"` emoji platform (so `{{emoji:wave}}` is normalised to
/// Slack's `:wave:` shortcode).
pub fn card_to_fallback_text_slack(card: &CardElement) -> String {
    card_to_fallback_text(
        card,
        FallbackTextOptions {
            bold_format: Some(BoldFormat::Single),
            line_break: Some(LineBreak::Single),
            platform: Some(PlatformName::Slack),
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

        let result = card_to_fallback_text_slack(&c);

        assert!(result.contains("*Order Update*"), "got: {result}");
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
        assert_eq!(card_to_fallback_text_slack(&c), "*Simple Card*");
    }

    // ---------- cardToBlockKit (7 of ~30 portable cases) ----------
    // 1:1 with upstream `cards.test.ts > describe("cardToBlockKit")`.
    // Covers the title/subtitle/imageUrl/Text/Image/Divider/Link
    // subset. Action Row + Section + Fields + Table + Select /
    // RadioSelect cases land in follow-up slices.

    use chat_sdk_chat::cards::{DividerKind, ImageElement, ImageKind, LinkElement, LinkKind};
    use serde_json::json;

    #[test]
    fn block_kit_converts_a_simple_card_with_title() {
        let c = card(Some("Welcome"), None, vec![]);
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            json!({
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": "Welcome",
                    "emoji": true,
                },
            })
        );
    }

    #[test]
    fn block_kit_converts_a_card_with_title_and_subtitle() {
        let c = card(Some("Order Update"), Some("Your order is on its way"), vec![]);
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(
            blocks[1],
            json!({
                "type": "context",
                "elements": [
                    { "type": "mrkdwn", "text": "Your order is on its way" }
                ],
            })
        );
    }

    #[test]
    fn block_kit_converts_a_card_with_header_image() {
        let c = CardElement {
            title: Some("Product".to_string()),
            subtitle: None,
            image_url: Some("https://example.com/product.png".to_string()),
            kind: CardKind::Card,
            children: vec![],
        };
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[1],
            json!({
                "type": "image",
                "image_url": "https://example.com/product.png",
                "alt_text": "Product",
            })
        );
    }

    #[test]
    fn block_kit_converts_text_elements() {
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
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[0],
            json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": "Regular text" },
            })
        );
        assert_eq!(
            blocks[1],
            json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": "*Bold text*" },
            })
        );
        assert_eq!(
            blocks[2],
            json!({
                "type": "context",
                "elements": [
                    { "type": "mrkdwn", "text": "Muted text" }
                ],
            })
        );
    }

    #[test]
    fn block_kit_converts_image_elements() {
        let c = card(
            None,
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/img.png".to_string(),
                alt: Some("My image".to_string()),
                kind: ImageKind::Image,
            })],
        );
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            json!({
                "type": "image",
                "image_url": "https://example.com/img.png",
                "alt_text": "My image",
            })
        );
    }

    #[test]
    fn block_kit_converts_divider_elements() {
        use chat_sdk_chat::cards::DividerElement;
        let c = card(
            None,
            None,
            vec![CardChild::Divider(DividerElement {
                kind: DividerKind::Divider,
            })],
        );
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], json!({ "type": "divider" }));
    }

    // ---------- cardToBlockKit Actions/Buttons + Fields + Section + complete-card (6 cases) ----------

    use chat_sdk_chat::cards::{ButtonStyle, LinkButtonKind, SectionKind};

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

    #[test]
    fn block_kit_converts_actions_with_buttons() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button("approve", "Approve", Some(ButtonStyle::Primary), None)),
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
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "actions");
        let elements = blocks[0]["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 3);
        assert_eq!(
            elements[0],
            json!({
                "type": "button",
                "text": { "type": "plain_text", "text": "Approve", "emoji": true },
                "action_id": "approve",
                "style": "primary",
            })
        );
        assert_eq!(
            elements[1],
            json!({
                "type": "button",
                "text": { "type": "plain_text", "text": "Reject", "emoji": true },
                "action_id": "reject",
                "value": "data-123",
                "style": "danger",
            })
        );
        assert_eq!(
            elements[2],
            json!({
                "type": "button",
                "text": { "type": "plain_text", "text": "Skip", "emoji": true },
                "action_id": "skip",
            })
        );
    }

    #[test]
    fn block_kit_converts_link_buttons_with_url_property() {
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
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "actions");
        let elements = blocks[0]["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["type"], "button");
        assert_eq!(
            elements[0]["text"],
            json!({ "type": "plain_text", "text": "View Docs", "emoji": true })
        );
        assert_eq!(elements[0]["url"], "https://example.com/docs");
        assert_eq!(elements[0]["style"], "primary");
        assert_eq!(elements[0]["action_id"], "link-https://example.com/docs");
    }

    #[test]
    fn block_kit_converts_fields() {
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
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "section");
        assert_eq!(
            blocks[0]["fields"],
            json!([
                { "type": "mrkdwn", "text": "*Status*\nActive" },
                { "type": "mrkdwn", "text": "*Priority*\nHigh" },
            ])
        );
    }

    #[test]
    fn block_kit_flattens_section_children() {
        use chat_sdk_chat::cards::SectionElement;
        let c = card(
            None,
            None,
            vec![CardChild::Section(SectionElement {
                children: vec![
                    text("Inside section"),
                    CardChild::Divider(DividerElement {
                        kind: DividerKind::Divider,
                    }),
                ],
                kind: SectionKind::Section,
            })],
        );
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "section");
        assert_eq!(blocks[1]["type"], "divider");
    }

    #[test]
    fn block_kit_converts_a_complete_card() {
        let c = CardElement {
            title: Some("Order #1234".to_string()),
            subtitle: Some("Status update".to_string()),
            image_url: None,
            kind: CardKind::Card,
            children: vec![
                text("Your order has been shipped!"),
                CardChild::Divider(DividerElement {
                    kind: DividerKind::Divider,
                }),
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
        let blocks = card_to_block_kit(&c);
        // Header + context + section(text) + divider + section(fields) + actions = 6.
        assert_eq!(blocks.len(), 6);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[1]["type"], "context");
        assert_eq!(blocks[2]["type"], "section");
        assert_eq!(blocks[3]["type"], "divider");
        assert_eq!(blocks[4]["type"], "section");
        assert_eq!(blocks[5]["type"], "actions");
    }

    #[test]
    fn block_kit_converts_link_children_to_section_with_slack_link_syntax() {
        // Additive coverage for the Link branch — not in
        // describe("cardToBlockKit") proper, but exercised
        // indirectly via the CardLink describe block.
        let c = card(
            None,
            None,
            vec![CardChild::Link(LinkElement {
                label: "Click here".to_string(),
                kind: LinkKind::Link,
                url: "https://example.com".to_string(),
            })],
        );
        let blocks = card_to_block_kit(&c);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            json!({
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": "<https://example.com|Click here>",
                },
            })
        );
    }
}
