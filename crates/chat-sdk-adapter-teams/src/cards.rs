//! Teams card rendering.
//!
//! Partial 1:1 port of `packages/adapter-teams/src/cards.ts`. This
//! slice covers `cardToFallbackText` (a thin wrapper over the shared
//! helper with Teams-shape options). The full Adaptive Card renderer
//! (`cardToAdaptiveCard`) is deferred to a follow-up slice.

use chat_sdk_adapter_shared::card_utils::{
    BoldFormat, FallbackTextOptions, LineBreak, PlatformName, card_to_fallback_text,
};
use chat_sdk_chat::cards::CardElement;

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
}
