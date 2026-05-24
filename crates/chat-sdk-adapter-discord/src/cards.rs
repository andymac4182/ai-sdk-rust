//! Discord card rendering.
//!
//! Partial 1:1 port of `packages/adapter-discord/src/cards.ts`.
//! This slice covers `cardToFallbackText` — the plain-markdown
//! fallback used when embeds aren't supported or for notifications.
//! The full `cardToDiscordPayload` Embed/Action-Row renderer is
//! deferred to a follow-up slice.

use chat_sdk_adapter_shared::errors::AdapterError;
use chat_sdk_chat::cards::{CardChild, CardElement, card_child_to_fallback_text};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::markdown::table_element_to_ascii;

const DISCORD_CUSTOM_ID_DELIMITER: char = '\n';
const DISCORD_CUSTOM_ID_MAX_LENGTH: usize = 100;

fn validate_discord_custom_id(custom_id: &str) -> Result<(), AdapterError> {
    if custom_id.is_empty() || custom_id.len() > DISCORD_CUSTOM_ID_MAX_LENGTH {
        return Err(AdapterError::validation(
            "discord",
            format!(
                "Discord custom_id must be 1-{DISCORD_CUSTOM_ID_MAX_LENGTH} characters. \
                 Shorten the button id or value."
            ),
        ));
    }
    Ok(())
}

/// 1:1 port of upstream `encodeDiscordCustomId(actionId, value?)`.
///
/// Encodes `actionId` (alone if `value` is empty/None) into a Discord
/// component `custom_id`, joining with `\n` when a value is provided.
/// Returns `ValidationError` when the resulting string is empty or
/// exceeds 100 chars.
pub fn encode_discord_custom_id(action_id: &str, value: Option<&str>) -> Result<String, AdapterError> {
    match value {
        None | Some("") => {
            validate_discord_custom_id(action_id)?;
            Ok(action_id.to_string())
        }
        Some(v) => {
            let encoded = format!("{action_id}{DISCORD_CUSTOM_ID_DELIMITER}{v}");
            validate_discord_custom_id(&encoded)?;
            Ok(encoded)
        }
    }
}

/// Decoded Discord `custom_id` pair. Mirrors upstream
/// `{ actionId, value: string | undefined }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordCustomId {
    pub action_id: String,
    pub value: Option<String>,
}

/// 1:1 port of upstream `decodeDiscordCustomId(customId)`. Splits on
/// the first `\n`; everything before becomes `actionId`, the
/// remainder becomes `value`. Returns `value: None` when there is no
/// delimiter.
pub fn decode_discord_custom_id(custom_id: &str) -> DiscordCustomId {
    match custom_id.find(DISCORD_CUSTOM_ID_DELIMITER) {
        None => DiscordCustomId {
            action_id: custom_id.to_string(),
            value: None,
        },
        Some(idx) => DiscordCustomId {
            action_id: custom_id[..idx].to_string(),
            value: Some(custom_id[idx + 1..].to_string()),
        },
    }
}

/// Convert emoji placeholders to Discord shortcode. 1:1 with upstream
/// private `convertEmoji(text) = convertEmojiPlaceholders(text, "discord")`.
fn convert_emoji(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Discord, None)
}

/// Default Discord embed color (blurple). 1:1 with upstream's
/// inline `embed.color = 0x5865f2` literal.
pub const DISCORD_EMBED_DEFAULT_COLOR: u32 = 0x5865f2;

/// Discord embed image shape. 1:1 with upstream `APIEmbedImage`
/// (subset — only `url` is set by the renderer).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscordEmbedImage {
    pub url: String,
}

/// Discord embed shape. 1:1 with upstream `APIEmbed` (subset —
/// the renderer only sets title, description, image, color, and
/// fields).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscordEmbed {
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<DiscordEmbedImage>,
    pub color: Option<u32>,
    pub fields: Vec<DiscordEmbedField>,
}

/// Discord embed-field shape. 1:1 with upstream `APIEmbedField`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscordEmbedField {
    pub name: String,
    pub value: String,
    pub inline: Option<bool>,
}

/// Discord message-component action row (placeholder). The
/// `cardToDiscordPayload` Action-Row rendering branch is deferred;
/// this slice returns an empty Vec for the action-row component.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscordActionRow {}

/// Result of [`card_to_discord_payload`]. 1:1 with upstream
/// `{embeds, components}` return shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordPayload {
    pub embeds: Vec<DiscordEmbed>,
    pub components: Vec<DiscordActionRow>,
}

/// Convert a [`CardElement`] to a Discord message payload (embeds
/// + components). Partial 1:1 port of upstream
/// `cardToDiscordPayload(card)`. Currently handles:
///
/// - `title` -> `embed.title`
/// - `subtitle` -> `embed.description`
/// - `imageUrl` -> `embed.image.url`
/// - Default color = blurple [`DISCORD_EMBED_DEFAULT_COLOR`]
/// - `Text` children -> `embed.description` (with style markers
///   `**bold**` / `*muted*`)
/// - `Divider` children -> horizontal line marker `"───────────"`
/// - `Image` children -> no-op (Discord embeds support only one
///   image at the card level)
/// - `Fields` children -> `embed.fields`
/// - `Link` children -> markdown `[label](url)` in description
///
/// **Deferred:** `Actions`/`Section`/`Table`/`CardLink` rendering
/// and the full Action Row component output.
pub fn card_to_discord_payload(card: &CardElement) -> DiscordPayload {
    let mut embed = DiscordEmbed {
        color: Some(DISCORD_EMBED_DEFAULT_COLOR),
        ..Default::default()
    };

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        embed.title = Some(convert_emoji(title));
    }
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        embed.description = Some(convert_emoji(subtitle));
    }
    if let Some(image_url) = card.image_url.as_deref().filter(|u| !u.is_empty()) {
        embed.image = Some(DiscordEmbedImage {
            url: image_url.to_string(),
        });
    }

    let mut text_parts: Vec<String> = Vec::new();

    for child in &card.children {
        process_child(child, &mut text_parts, &mut embed.fields);
    }

    if !text_parts.is_empty() {
        let joined = text_parts.join("\n\n");
        embed.description = Some(match embed.description.take() {
            Some(existing) => format!("{existing}\n\n{joined}"),
            None => joined,
        });
    }

    DiscordPayload {
        embeds: vec![embed],
        components: Vec::new(),
    }
}

fn process_child(
    child: &CardChild,
    text_parts: &mut Vec<String>,
    fields: &mut Vec<DiscordEmbedField>,
) {
    match child {
        CardChild::Text(t) => {
            let converted = convert_emoji(&t.content);
            let styled = match t.style {
                Some(chat_sdk_chat::cards::TextStyle::Bold) => format!("**{converted}**"),
                Some(chat_sdk_chat::cards::TextStyle::Muted) => format!("*{converted}*"),
                _ => converted,
            };
            text_parts.push(styled);
        }
        CardChild::Image(_) => {
            // Discord embeds support only one image (set at the
            // card level via `imageUrl`); additional image children
            // are silently ignored — upstream comment notes "could
            // be added as separate embeds" but the current upstream
            // renderer does not.
        }
        CardChild::Divider(_) => {
            text_parts.push("───────────".to_string());
        }
        CardChild::Fields(f) => {
            for field in &f.children {
                fields.push(DiscordEmbedField {
                    name: convert_emoji(&field.label),
                    value: convert_emoji(&field.value),
                    inline: None,
                });
            }
        }
        CardChild::Link(l) => {
            text_parts.push(format!("[{}]({})", convert_emoji(&l.label), l.url));
        }
        // Actions / Section / Table / CardLink rendering deferred.
        _ => {}
    }
}

/// Render a [`CardElement`] as Discord markdown fallback text. 1:1
/// port of upstream `cardToFallbackText(card)`:
///
/// - Title -> `**<title>**`
/// - Subtitle -> plain
/// - Children rendered by [`child_to_fallback_text`]; parts joined
///   by `"\n\n"`.
pub fn card_to_fallback_text_discord(card: &CardElement) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        parts.push(format!("**{}**", convert_emoji(title)));
    }
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        parts.push(convert_emoji(subtitle));
    }

    for child in &card.children {
        if let Some(text) = child_to_fallback_text(child) {
            parts.push(text);
        }
    }

    parts.join("\n\n")
}

/// Render a single [`CardChild`] as Discord fallback text. 1:1 with
/// upstream private `childToFallbackText`.
fn child_to_fallback_text(child: &CardChild) -> Option<String> {
    match child {
        CardChild::Text(t) => Some(convert_emoji(&t.content)),
        CardChild::Fields(f) => {
            let lines: Vec<String> = f
                .children
                .iter()
                .map(|fld| {
                    format!(
                        "**{}**: {}",
                        convert_emoji(&fld.label),
                        convert_emoji(&fld.value)
                    )
                })
                .collect();
            Some(lines.join("\n"))
        }
        // Actions are intentionally excluded from fallback text.
        CardChild::Actions(_) => None,
        CardChild::Section(s) => {
            let pieces: Vec<String> = s
                .children
                .iter()
                .filter_map(child_to_fallback_text)
                .collect();
            if pieces.is_empty() {
                None
            } else {
                Some(pieces.join("\n"))
            }
        }
        CardChild::Table(t) => Some(format!(
            "```\n{}\n```",
            table_element_to_ascii(&t.headers, &t.rows)
        )),
        CardChild::Divider(_) => Some("---".to_string()),
        // Upstream `default` branch falls through to
        // `cardChildToFallbackText`.
        other => card_child_to_fallback_text(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{
        ActionsChild, ActionsElement, ActionsKind, ButtonElement, ButtonKind, CardKind,
        DividerElement, DividerKind, FieldElement, FieldKind, FieldsElement, FieldsKind,
        SectionElement, SectionKind, TextElement, TextKind,
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

    // ---------- cardToFallbackText (7 upstream cases) ----------

    #[test]
    // ---------- cardToDiscordPayload (7 of 31 portable cases) ----------
    // 1:1 with upstream `cards.test.ts > describe("cardToDiscordPayload")`.
    // Covers the 7 cases that don't require the deferred Action-Row
    // (buttons / sections / link-buttons / table / CardLink) renderer.

    #[test]
    fn converts_a_simple_card_with_title() {
        let c = card(Some("Welcome"), None, vec![]);
        let payload = card_to_discord_payload(&c);
        assert_eq!(payload.embeds.len(), 1);
        assert_eq!(payload.embeds[0].title.as_deref(), Some("Welcome"));
        assert_eq!(payload.components.len(), 0);
    }

    #[test]
    fn converts_a_card_with_title_and_subtitle() {
        let c = card(Some("Order Update"), Some("Your order is on its way"), vec![]);
        let payload = card_to_discord_payload(&c);
        assert_eq!(payload.embeds.len(), 1);
        assert_eq!(payload.embeds[0].title.as_deref(), Some("Order Update"));
        assert!(payload.embeds[0]
            .description
            .as_deref()
            .unwrap_or("")
            .contains("Your order is on its way"));
    }

    #[test]
    fn converts_a_card_with_header_image() {
        let c = CardElement {
            title: Some("Product".to_string()),
            subtitle: None,
            image_url: Some("https://example.com/product.png".to_string()),
            kind: CardKind::Card,
            children: vec![],
        };
        let payload = card_to_discord_payload(&c);
        assert_eq!(payload.embeds.len(), 1);
        assert_eq!(
            payload.embeds[0].image,
            Some(DiscordEmbedImage {
                url: "https://example.com/product.png".to_string()
            })
        );
    }

    #[test]
    fn sets_default_color_to_discord_blurple() {
        let c = card(Some("Test"), None, vec![]);
        let payload = card_to_discord_payload(&c);
        assert_eq!(payload.embeds[0].color, Some(0x5865f2));
    }

    #[test]
    fn converts_text_elements() {
        use chat_sdk_chat::cards::TextStyle;
        let regular = CardChild::Text(TextElement {
            content: "Regular text".to_string(),
            style: None,
            kind: TextKind::Text,
        });
        let bold = CardChild::Text(TextElement {
            content: "Bold text".to_string(),
            style: Some(TextStyle::Bold),
            kind: TextKind::Text,
        });
        let muted = CardChild::Text(TextElement {
            content: "Muted text".to_string(),
            style: Some(TextStyle::Muted),
            kind: TextKind::Text,
        });
        let c = card(None, None, vec![regular, bold, muted]);
        let payload = card_to_discord_payload(&c);
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("Regular text"), "got: {description}");
        assert!(description.contains("**Bold text**"), "got: {description}");
        assert!(description.contains("*Muted text*"), "got: {description}");
    }

    #[test]
    fn converts_image_elements_in_children_no_op() {
        use chat_sdk_chat::cards::{ImageElement, ImageKind};
        let c = card(
            None,
            None,
            vec![CardChild::Image(ImageElement {
                url: "https://example.com/img.png".to_string(),
                alt: Some("My image".to_string()),
                kind: ImageKind::Image,
            })],
        );
        let payload = card_to_discord_payload(&c);
        // Image children are silently dropped (Discord embeds only
        // support one image, at the card-level via `imageUrl`).
        assert_eq!(payload.embeds.len(), 1);
    }

    #[test]
    fn converts_divider_elements_to_horizontal_line_markers() {
        let c = card(
            None,
            None,
            vec![
                text("Before"),
                CardChild::Divider(DividerElement {
                    kind: DividerKind::Divider,
                }),
                text("After"),
            ],
        );
        let payload = card_to_discord_payload(&c);
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("Before"), "got: {description}");
        assert!(description.contains("───────────"), "got: {description}");
        assert!(description.contains("After"), "got: {description}");
    }

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
        let r = card_to_fallback_text_discord(&c);
        assert!(r.contains("**Order Update**"), "got: {r}");
        assert!(r.contains("Status changed"), "got: {r}");
        assert!(r.contains("Your order is ready"), "got: {r}");
        assert!(r.contains("**Order ID**: #1234"), "got: {r}");
        assert!(r.contains("**Status**: Ready"), "got: {r}");
        assert!(!r.contains("[Schedule Pickup]"), "actions leaked: {r}");
        assert!(!r.contains("[Delay]"), "actions leaked: {r}");
    }

    #[test]
    fn handles_card_with_only_title() {
        let c = card(Some("Simple Card"), None, vec![]);
        assert_eq!(card_to_fallback_text_discord(&c), "**Simple Card**");
    }

    #[test]
    fn handles_card_with_subtitle_only() {
        let c = card(None, Some("Just a subtitle"), vec![]);
        assert_eq!(card_to_fallback_text_discord(&c), "Just a subtitle");
    }

    #[test]
    fn handles_divider_elements() {
        let c = card(
            None,
            None,
            vec![
                text("Before"),
                CardChild::Divider(DividerElement {
                    kind: DividerKind::Divider,
                }),
                text("After"),
            ],
        );
        let r = card_to_fallback_text_discord(&c);
        assert!(r.contains("Before"));
        assert!(r.contains("---"));
        assert!(r.contains("After"));
    }

    #[test]
    fn handles_section_elements() {
        let c = card(
            None,
            None,
            vec![CardChild::Section(SectionElement {
                children: vec![text("Section content")],
                kind: SectionKind::Section,
            })],
        );
        assert!(
            card_to_fallback_text_discord(&c).contains("Section content"),
            "got: {}",
            card_to_fallback_text_discord(&c)
        );
    }

    #[test]
    fn handles_empty_card() {
        let c = card(None, None, vec![]);
        assert_eq!(card_to_fallback_text_discord(&c), "");
    }

    // ---------- encodeDiscordCustomId / decodeDiscordCustomId ----------
    // 11 portable upstream cases (2 cardToDiscordPayload-dependent cases
    // deferred until the full embed/action-row renderer lands).

    #[test]
    fn encodes_action_id_only_when_no_value() {
        assert_eq!(
            encode_discord_custom_id("approve", None).unwrap(),
            "approve"
        );
    }

    #[test]
    fn encodes_action_id_with_value() {
        assert_eq!(
            encode_discord_custom_id("approve", Some("order-123")).unwrap(),
            "approve\norder-123"
        );
    }

    #[test]
    fn skips_encoding_when_empty_value() {
        assert_eq!(
            encode_discord_custom_id("approve", Some("")).unwrap(),
            "approve"
        );
    }

    #[test]
    fn throws_when_action_id_is_empty() {
        let err = encode_discord_custom_id("", None).unwrap_err();
        assert!(err.is_validation(), "expected ValidationError, got {err}");
    }

    #[test]
    fn throws_when_action_id_exceeds_100_chars() {
        let long = "x".repeat(101);
        let err = encode_discord_custom_id(&long, None).unwrap_err();
        assert!(err.is_validation());
    }

    #[test]
    fn throws_when_encoded_custom_id_exceeds_100_chars() {
        let long_value = "x".repeat(100);
        let err = encode_discord_custom_id("btn", Some(&long_value)).unwrap_err();
        assert!(err.is_validation());
    }

    #[test]
    fn decodes_action_id_only() {
        assert_eq!(
            decode_discord_custom_id("approve"),
            DiscordCustomId {
                action_id: "approve".to_string(),
                value: None,
            }
        );
    }

    #[test]
    fn decodes_action_id_with_value() {
        assert_eq!(
            decode_discord_custom_id("approve\norder-123"),
            DiscordCustomId {
                action_id: "approve".to_string(),
                value: Some("order-123".to_string()),
            }
        );
    }

    #[test]
    fn round_trips_encode_decode() {
        let encoded = encode_discord_custom_id("btn", Some("__cb:a1b2c3d4e5f6g7h8")).unwrap();
        let decoded = decode_discord_custom_id(&encoded);
        assert_eq!(decoded.action_id, "btn");
        assert_eq!(decoded.value.as_deref(), Some("__cb:a1b2c3d4e5f6g7h8"));
    }

    #[test]
    fn preserves_embedded_delimiter_chars_in_the_value() {
        let decoded = decode_discord_custom_id("btn\nfirst\nsecond");
        assert_eq!(decoded.action_id, "btn");
        assert_eq!(decoded.value.as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn treats_explicitly_none_value_as_no_value() {
        assert_eq!(
            encode_discord_custom_id("approve", None).unwrap(),
            "approve"
        );
    }

    #[test]
    fn encodes_a_custom_id_at_the_100_char_boundary() {
        let action_id = "a".repeat(50);
        let value = "b".repeat(49);
        let encoded = encode_discord_custom_id(&action_id, Some(&value)).unwrap();
        assert_eq!(encoded.len(), 100);
        let decoded = decode_discord_custom_id(&encoded);
        assert_eq!(decoded.action_id, action_id);
        assert_eq!(decoded.value.as_deref(), Some(value.as_str()));
    }

    #[test]
    fn rejects_a_custom_id_one_char_past_the_boundary() {
        let action_id = "a".repeat(50);
        let value = "b".repeat(50);
        let err = encode_discord_custom_id(&action_id, Some(&value)).unwrap_err();
        assert!(err.is_validation());
    }

    #[test]
    fn handles_card_with_multiple_fields() {
        let c = card(
            None,
            None,
            vec![CardChild::Fields(FieldsElement {
                children: vec![
                    FieldElement {
                        label: "A".to_string(),
                        value: "1".to_string(),
                        kind: FieldKind::Field,
                    },
                    FieldElement {
                        label: "B".to_string(),
                        value: "2".to_string(),
                        kind: FieldKind::Field,
                    },
                    FieldElement {
                        label: "C".to_string(),
                        value: "3".to_string(),
                        kind: FieldKind::Field,
                    },
                ],
                kind: FieldsKind::Fields,
            })],
        );
        let r = card_to_fallback_text_discord(&c);
        assert!(r.contains("**A**: 1"), "got: {r}");
        assert!(r.contains("**B**: 2"), "got: {r}");
        assert!(r.contains("**C**: 3"), "got: {r}");
    }
}
