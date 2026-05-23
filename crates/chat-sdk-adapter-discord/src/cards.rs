//! Discord card rendering.
//!
//! Partial 1:1 port of `packages/adapter-discord/src/cards.ts`.
//! This slice covers `cardToFallbackText` — the plain-markdown
//! fallback used when embeds aren't supported or for notifications.
//! The full `cardToDiscordPayload` Embed/Action-Row renderer is
//! deferred to a follow-up slice.

use chat_sdk_chat::cards::{CardChild, CardElement, card_child_to_fallback_text};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::markdown::table_element_to_ascii;

/// Convert emoji placeholders to Discord shortcode. 1:1 with upstream
/// private `convertEmoji(text) = convertEmojiPlaceholders(text, "discord")`.
fn convert_emoji(text: &str) -> String {
    convert_emoji_placeholders(text, PlaceholderPlatform::Discord, None)
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
