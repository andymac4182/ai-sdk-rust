//! Slack card rendering.
//!
//! 1:1 port of the fallback-text subset of
//! `packages/adapter-slack/src/cards.ts`. The full Block Kit
//! renderer (`cardToBlockKit`) requires modelling Slack's
//! section / actions / image / divider / header block JSON
//! shapes and follows in later slices.

use chat_sdk_adapter_shared::card_utils::{
    BoldFormat, FallbackTextOptions, LineBreak, PlatformName, card_to_fallback_text,
};
use chat_sdk_chat::cards::CardElement;

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
}
