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
};
use chat_sdk_chat::cards::{
    CardChild, CardElement, DividerElement, ImageElement, LinkElement, TextElement, TextStyle,
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
        CardChild::Link(l) => vec![convert_link_to_block(l)],
        // Actions / Section / Fields / Table branches land in
        // follow-up slices.
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
