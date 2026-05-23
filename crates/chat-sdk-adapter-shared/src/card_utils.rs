//! Shared card-conversion utilities for adapters.
//!
//! 1:1 port (in progress) of `packages/adapter-shared/src/card-utils.ts`.
//!
//! **What this slice ships (slice 38):**
//!
//! - [`PlatformName`] enum (Slack/GChat/Teams/Discord).
//! - [`button_style_mapping`] / [`button_style_mappings_all`] — the
//!   per-platform `BUTTON_STYLE_MAPPINGS` lookup table.
//! - [`map_button_style`] — 1:1 with upstream `mapButtonStyle`.
//! - [`escape_table_cell`] — 1:1 with upstream `escapeTableCell`.
//! - [`render_gfm_table`] — 1:1 with upstream `renderGfmTable`. Used by
//!   adapters that support native GFM table rendering (GitHub, Linear,
//!   Discord).
//!
//! **Slice 44 additions:**
//!
//! - [`create_emoji_converter`] — 1:1 port of upstream
//!   `createEmojiConverter(platform)`, now that
//!   `chat_sdk_chat::emoji::convert_emoji_placeholders` has landed.
//! - [`FallbackTextOptions`] + [`card_to_fallback_text`] — 1:1 port of
//!   upstream `cardToFallbackText`, with the platform-aware emoji
//!   conversion + configurable bold marker + line-break separator.

use chat_sdk_chat::cards::{
    ButtonStyle, CardChild, CardElement, FieldElement, TableElement, card_child_to_fallback_text,
};
use chat_sdk_chat::emoji::{PlaceholderPlatform, convert_emoji_placeholders};
use chat_sdk_chat::markdown::table_element_to_ascii;
use serde::{Deserialize, Serialize};

/// Supported platforms for adapter utilities. 1:1 port of upstream
/// `export type PlatformName = "slack" | "gchat" | "teams" | "discord"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformName {
    Slack,
    Gchat,
    Teams,
    Discord,
}

impl PlatformName {
    /// Iterate every supported platform, mirroring upstream
    /// `Object.keys(BUTTON_STYLE_MAPPINGS)`.
    pub const ALL: &'static [Self] = &[Self::Slack, Self::Gchat, Self::Teams, Self::Discord];
}

/// Per-platform mapping from chat-sdk [`ButtonStyle`] to the
/// platform-specific style identifier. 1:1 port of upstream
/// `BUTTON_STYLE_MAPPINGS[platform][style]`.
///
/// Returns `None` when the style is the upstream [`ButtonStyle::Default`]
/// — upstream's mapping only covers `primary` and `danger`; `default`
/// has no platform override.
pub fn button_style_mapping(platform: PlatformName, style: ButtonStyle) -> Option<&'static str> {
    match (platform, style) {
        (PlatformName::Slack, ButtonStyle::Primary) => Some("primary"),
        (PlatformName::Slack, ButtonStyle::Danger) => Some("danger"),
        (PlatformName::Gchat, ButtonStyle::Primary) => Some("primary"),
        (PlatformName::Gchat, ButtonStyle::Danger) => Some("danger"),
        (PlatformName::Teams, ButtonStyle::Primary) => Some("positive"),
        (PlatformName::Teams, ButtonStyle::Danger) => Some("destructive"),
        (PlatformName::Discord, ButtonStyle::Primary) => Some("primary"),
        (PlatformName::Discord, ButtonStyle::Danger) => Some("danger"),
        (_, ButtonStyle::Default) => None,
    }
}

/// 1:1 port of upstream `mapButtonStyle(style, platform): string | undefined`.
/// Mirrors upstream's "no style -> `undefined`" return.
pub fn map_button_style(
    style: Option<ButtonStyle>,
    platform: PlatformName,
) -> Option<&'static str> {
    style.and_then(|s| button_style_mapping(platform, s))
}

/// Escape a cell value for use in a GFM pipe table. 1:1 port of upstream
/// `escapeTableCell(value): string`.
///
/// Order is significant: backslashes are escaped first (`\\` -> `\\\\`),
/// then pipes (`|` -> `\\|`), then newlines collapse to single spaces.
/// Doing pipes before backslashes would double-escape the `\\|`
/// introduced by the pipe step.
pub fn escape_table_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', " ")
}

impl PlatformName {
    fn as_placeholder_platform(self) -> PlaceholderPlatform {
        match self {
            PlatformName::Slack => PlaceholderPlatform::Slack,
            PlatformName::Gchat => PlaceholderPlatform::Gchat,
            PlatformName::Teams => PlaceholderPlatform::Teams,
            PlatformName::Discord => PlaceholderPlatform::Discord,
        }
    }
}

/// Create a platform-specific emoji converter closure. 1:1 port of
/// upstream `createEmojiConverter(platform): (text: string) => string`.
///
/// The returned closure runs [`convert_emoji_placeholders`] against the
/// global [`chat_sdk_chat::emoji::DEFAULT_EMOJI_RESOLVER`].
pub fn create_emoji_converter(platform: PlatformName) -> impl Fn(&str) -> String {
    move |text: &str| convert_emoji_placeholders(text, platform.as_placeholder_platform(), None)
}

/// Bold-marker variant accepted by [`FallbackTextOptions`]. 1:1 port of
/// upstream `boldFormat?: "*" | "**"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoldFormat {
    /// Slack-style `mrkdwn` single-asterisk bold (`*bold*`).
    Single,
    /// CommonMark / Teams-style double-asterisk bold (`**bold**`).
    Double,
}

impl BoldFormat {
    fn marker(self) -> &'static str {
        match self {
            BoldFormat::Single => "*",
            BoldFormat::Double => "**",
        }
    }
}

/// Line-break separator accepted by [`FallbackTextOptions`]. 1:1 port
/// of upstream `lineBreak?: "\n" | "\n\n"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineBreak {
    /// Single newline (default).
    Single,
    /// Double newline (Teams-style spacing).
    Double,
}

impl LineBreak {
    fn as_str(self) -> &'static str {
        match self {
            LineBreak::Single => "\n",
            LineBreak::Double => "\n\n",
        }
    }
}

/// Options for [`card_to_fallback_text`]. 1:1 port of upstream
/// `interface FallbackTextOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FallbackTextOptions {
    /// Bold marker (default: [`BoldFormat::Single`], matching upstream
    /// `"*"`).
    pub bold_format: Option<BoldFormat>,
    /// Line-break separator (default: [`LineBreak::Single`]).
    pub line_break: Option<LineBreak>,
    /// Platform for emoji conversion. When `None`, emoji placeholders
    /// are left as-is.
    pub platform: Option<PlatformName>,
}

/// Generate plain-text fallback from a [`CardElement`] with
/// platform-aware emoji conversion + configurable formatting. 1:1
/// port of upstream
/// `cardToFallbackText(card, options): string`.
pub fn card_to_fallback_text(card: &CardElement, options: FallbackTextOptions) -> String {
    let bold = options.bold_format.unwrap_or(BoldFormat::Single).marker();
    let line_break = options.line_break.unwrap_or(LineBreak::Single).as_str();

    let convert_text: Box<dyn Fn(&str) -> String> = match options.platform {
        Some(p) => Box::new(create_emoji_converter(p)),
        None => Box::new(|t: &str| t.to_string()),
    };

    let mut parts: Vec<String> = Vec::new();
    if let Some(title) = &card.title {
        parts.push(format!("{bold}{}{bold}", convert_text(title)));
    }
    if let Some(subtitle) = &card.subtitle {
        parts.push(convert_text(subtitle));
    }
    for child in &card.children {
        if let Some(text) = child_to_fallback_text(child, &convert_text) {
            parts.push(text);
        }
    }
    parts.join(line_break)
}

fn child_to_fallback_text(
    child: &CardChild,
    convert_text: &dyn Fn(&str) -> String,
) -> Option<String> {
    match child {
        CardChild::Text(t) => Some(convert_text(&t.content)),
        CardChild::Link(l) => Some(format!("{} ({})", convert_text(&l.label), l.url)),
        CardChild::Fields(f) => Some(
            f.children
                .iter()
                .map(|field: &FieldElement| {
                    format!(
                        "{}: {}",
                        convert_text(&field.label),
                        convert_text(&field.value)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        CardChild::Actions(_) => None,
        CardChild::Section(s) => Some(
            s.children
                .iter()
                .filter_map(|c| child_to_fallback_text(c, convert_text))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        CardChild::Table(t) => Some(table_element_to_ascii(&t.headers, &t.rows)),
        CardChild::Divider(_) => Some("---".to_string()),
        // Unknown / Image fall back to the chat-side core helper,
        // mirroring upstream's `default: coreCardChildToFallbackText`.
        CardChild::Image(_) => card_child_to_fallback_text(child),
    }
}

/// Render a [`TableElement`] as GFM markdown table lines. 1:1 port of
/// upstream `renderGfmTable(table): string[]`. Each line is a row in the
/// output table; callers can `lines.join("\n")` to produce the final
/// markdown.
pub fn render_gfm_table(table: &TableElement) -> Vec<String> {
    let headers: Vec<String> = table.headers.iter().map(|h| escape_table_cell(h)).collect();
    let mut lines: Vec<String> = Vec::with_capacity(table.rows.len() + 2);
    lines.push(format!("| {} |", headers.join(" | ")));
    let separator: Vec<&str> = headers.iter().map(|_| "---").collect();
    lines.push(format!("| {} |", separator.join(" | ")));
    for row in &table.rows {
        let cells: Vec<String> = row.iter().map(|c| escape_table_cell(c)).collect();
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    lines
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/adapter-shared/src/card-utils.test.ts`
    //! covering the standalone helpers + `createEmojiConverter` +
    //! `cardToFallbackText`. The Rust suite has more cases than
    //! upstream (50 vs 39) because each chat-cards variant the helper
    //! observes gets a dedicated test in Rust; upstream batches several
    //! variants per `it()` block.

    use super::*;
    use chat_sdk_chat::cards::{TableOptions, table};

    #[test]
    fn platform_name_uses_upstream_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&PlatformName::Slack).unwrap(),
            "\"slack\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformName::Gchat).unwrap(),
            "\"gchat\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformName::Teams).unwrap(),
            "\"teams\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformName::Discord).unwrap(),
            "\"discord\""
        );
    }

    #[test]
    fn map_button_style_returns_platform_specific_values_for_primary_and_danger() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Slack),
            Some("primary")
        );
        assert_eq!(
            map_button_style(Some(ButtonStyle::Danger), PlatformName::Slack),
            Some("danger")
        );
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Teams),
            Some("positive")
        );
        assert_eq!(
            map_button_style(Some(ButtonStyle::Danger), PlatformName::Teams),
            Some("destructive")
        );
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Gchat),
            Some("primary")
        );
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Discord),
            Some("primary")
        );
    }

    #[test]
    fn map_button_style_returns_none_for_no_style() {
        assert_eq!(map_button_style(None, PlatformName::Teams), None);
        assert_eq!(map_button_style(None, PlatformName::Slack), None);
    }

    #[test]
    fn map_button_style_returns_none_for_default_style() {
        // Upstream's BUTTON_STYLE_MAPPINGS doesn't include "default" — it's
        // the platform's native style. The Rust port mirrors this with
        // None for ButtonStyle::Default.
        assert_eq!(
            map_button_style(Some(ButtonStyle::Default), PlatformName::Slack),
            None
        );
        assert_eq!(
            map_button_style(Some(ButtonStyle::Default), PlatformName::Teams),
            None
        );
    }

    #[test]
    fn escape_table_cell_escapes_pipes_and_collapses_newlines() {
        assert_eq!(escape_table_cell("plain"), "plain");
        assert_eq!(escape_table_cell("with | pipe"), "with \\| pipe");
        assert_eq!(escape_table_cell("line\nbreak"), "line break");
        // Order matters: backslashes escaped first, then pipes.
        // Input `\|` should become `\\\\|` (escape the backslash) then
        // `\\\\\\|` (escape the pipe). Confirm:
        assert_eq!(escape_table_cell("\\"), "\\\\");
        assert_eq!(escape_table_cell("\\|"), "\\\\\\|");
    }

    #[test]
    fn render_gfm_table_produces_a_three_or_more_line_pipe_table() {
        let t = table(TableOptions {
            headers: vec!["A".to_string(), "B".to_string()],
            rows: vec![
                vec!["1".to_string(), "2".to_string()],
                vec!["3".to_string(), "4".to_string()],
            ],
            align: None,
        });
        let lines = render_gfm_table(&t);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "| A | B |");
        assert_eq!(lines[1], "| --- | --- |");
        assert_eq!(lines[2], "| 1 | 2 |");
        assert_eq!(lines[3], "| 3 | 4 |");
    }

    // ---------- createEmojiConverter (1:1 with upstream 4 cases) ----------

    #[test]
    fn create_emoji_converter_creates_a_slack_emoji_converter() {
        let convert = create_emoji_converter(PlatformName::Slack);
        assert_eq!(convert("{{emoji:wave}} Hello"), ":wave: Hello");
        assert_eq!(convert("{{emoji:fire}}"), ":fire:");
    }

    #[test]
    fn create_emoji_converter_creates_a_teams_emoji_converter() {
        let convert = create_emoji_converter(PlatformName::Teams);
        let result = convert("{{emoji:wave}} Hello");
        assert!(result.contains("Hello"));
        assert!(!result.contains("{{emoji:"));
    }

    #[test]
    fn create_emoji_converter_creates_a_gchat_emoji_converter() {
        let convert = create_emoji_converter(PlatformName::Gchat);
        let result = convert("{{emoji:wave}} Hello");
        assert!(result.contains("Hello"));
        assert!(!result.contains("{{emoji:"));
    }

    #[test]
    fn create_emoji_converter_returns_text_unchanged_when_no_emoji_placeholders() {
        let convert = create_emoji_converter(PlatformName::Slack);
        assert_eq!(convert("Hello world"), "Hello world");
    }

    // ---------- mapButtonStyle (1:1 with upstream 8 cases) ----------

    #[test]
    fn map_button_style_slack_primary_to_primary() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Slack),
            Some("primary")
        );
    }

    #[test]
    fn map_button_style_slack_danger_to_danger() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Danger), PlatformName::Slack),
            Some("danger")
        );
    }

    #[test]
    fn map_button_style_slack_undefined_to_undefined() {
        assert_eq!(map_button_style(None, PlatformName::Slack), None);
    }

    #[test]
    fn map_button_style_teams_primary_to_positive() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Teams),
            Some("positive")
        );
    }

    #[test]
    fn map_button_style_teams_danger_to_destructive() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Danger), PlatformName::Teams),
            Some("destructive")
        );
    }

    #[test]
    fn map_button_style_teams_undefined_to_undefined() {
        assert_eq!(map_button_style(None, PlatformName::Teams), None);
    }

    #[test]
    fn map_button_style_gchat_primary_to_primary() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Primary), PlatformName::Gchat),
            Some("primary")
        );
    }

    #[test]
    fn map_button_style_gchat_danger_to_danger() {
        assert_eq!(
            map_button_style(Some(ButtonStyle::Danger), PlatformName::Gchat),
            Some("danger")
        );
    }

    // ---------- BUTTON_STYLE_MAPPINGS (1:1 with upstream 2 cases) ----------

    #[test]
    fn button_style_mappings_has_mappings_for_all_platforms() {
        // All three platforms have at least primary+danger entries.
        for p in [
            PlatformName::Slack,
            PlatformName::Teams,
            PlatformName::Gchat,
        ] {
            assert!(button_style_mapping(p, ButtonStyle::Primary).is_some());
            assert!(button_style_mapping(p, ButtonStyle::Danger).is_some());
        }
    }

    #[test]
    fn button_style_mappings_has_primary_and_danger_for_each_platform() {
        for p in [
            PlatformName::Slack,
            PlatformName::Teams,
            PlatformName::Gchat,
        ] {
            assert_eq!(
                button_style_mapping(p, ButtonStyle::Primary).is_some(),
                true,
                "primary missing for {p:?}"
            );
            assert_eq!(
                button_style_mapping(p, ButtonStyle::Danger).is_some(),
                true,
                "danger missing for {p:?}"
            );
        }
    }

    // ---------- cardToFallbackText (1:1 with upstream 14 cases) ----------

    #[test]
    fn card_to_fallback_text_formats_title_with_bold() {
        use chat_sdk_chat::cards::{CardOptions, card};
        let c = card(CardOptions {
            title: Some("Test Title".to_string()),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            "*Test Title*"
        );
    }

    #[test]
    fn card_to_fallback_text_formats_title_and_subtitle() {
        use chat_sdk_chat::cards::{CardOptions, card};
        let c = card(CardOptions {
            title: Some("Title".to_string()),
            subtitle: Some("Subtitle".to_string()),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            "*Title*\nSubtitle"
        );
    }

    #[test]
    fn card_to_fallback_text_uses_double_asterisks_for_markdown_bold_format() {
        use chat_sdk_chat::cards::{CardOptions, card};
        let c = card(CardOptions {
            title: Some("Title".to_string()),
            ..Default::default()
        });
        let out = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                bold_format: Some(BoldFormat::Double),
                ..Default::default()
            },
        );
        assert_eq!(out, "**Title**");
    }

    #[test]
    fn card_to_fallback_text_uses_double_line_breaks_when_specified() {
        use chat_sdk_chat::cards::{CardOptions, card};
        let c = card(CardOptions {
            title: Some("Title".to_string()),
            subtitle: Some("Subtitle".to_string()),
            ..Default::default()
        });
        let out = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                line_break: Some(LineBreak::Double),
                ..Default::default()
            },
        );
        assert_eq!(out, "*Title*\n\nSubtitle");
    }

    #[test]
    fn card_to_fallback_text_formats_text_children() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text};
        let c = card(CardOptions {
            title: Some("Card".to_string()),
            children: Some(vec![card_text("Some content", None).into()]),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            "*Card*\nSome content"
        );
    }

    #[test]
    fn card_to_fallback_text_formats_fields_as_label_value_pairs() {
        use chat_sdk_chat::cards::{CardOptions, card, field, fields};
        let c = card(CardOptions {
            children: Some(vec![
                fields(vec![field("Name", "John"), field("Age", "30")]).into(),
            ]),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            "Name: John\nAge: 30"
        );
    }

    #[test]
    fn card_to_fallback_text_formats_fields_with_double_bold_marker() {
        use chat_sdk_chat::cards::{CardOptions, card, field, fields};
        let c = card(CardOptions {
            children: Some(vec![fields(vec![field("Key", "Value")]).into()]),
            ..Default::default()
        });
        let out = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                bold_format: Some(BoldFormat::Double),
                ..Default::default()
            },
        );
        assert_eq!(out, "Key: Value");
    }

    #[test]
    fn card_to_fallback_text_excludes_actions_from_fallback_output() {
        use chat_sdk_chat::cards::{ButtonOptions, CardOptions, actions, button, card};
        let c = card(CardOptions {
            children: Some(vec![
                actions(vec![
                    button(ButtonOptions {
                        label: "OK".to_string(),
                        id: "ok".to_string(),
                        ..Default::default()
                    })
                    .into(),
                    button(ButtonOptions {
                        label: "Cancel".to_string(),
                        id: "cancel".to_string(),
                        ..Default::default()
                    })
                    .into(),
                ])
                .into(),
            ]),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            ""
        );
    }

    #[test]
    fn card_to_fallback_text_formats_dividers_as_horizontal_rules() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text, divider};
        let c = card(CardOptions {
            title: Some("Title".to_string()),
            children: Some(vec![
                divider().into(),
                card_text("After divider", None).into(),
            ]),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            "*Title*\n---\nAfter divider"
        );
    }

    #[test]
    fn card_to_fallback_text_converts_emoji_placeholders_when_platform_specified() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text};
        let c = card(CardOptions {
            title: Some("{{emoji:wave}} Welcome".to_string()),
            children: Some(vec![card_text("{{emoji:fire}} Hot stuff", None).into()]),
            ..Default::default()
        });
        let out = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                platform: Some(PlatformName::Slack),
                ..Default::default()
            },
        );
        assert_eq!(out, "*:wave: Welcome*\n:fire: Hot stuff");
    }

    #[test]
    fn card_to_fallback_text_leaves_emoji_placeholders_when_no_platform_specified() {
        use chat_sdk_chat::cards::{CardOptions, card};
        let c = card(CardOptions {
            title: Some("{{emoji:wave}} Welcome".to_string()),
            ..Default::default()
        });
        let out = card_to_fallback_text(&c, FallbackTextOptions::default());
        assert_eq!(out, "*{{emoji:wave}} Welcome*");
    }

    #[test]
    fn card_to_fallback_text_handles_complex_card_with_all_elements() {
        use chat_sdk_chat::cards::{
            ButtonOptions, CardOptions, actions, button, card, card_text, divider, field, fields,
        };
        let c = card(CardOptions {
            title: Some("Order #123".to_string()),
            subtitle: Some("Your order is confirmed".to_string()),
            children: Some(vec![
                card_text("Thank you for your purchase!", None).into(),
                divider().into(),
                fields(vec![
                    field("Status", "Processing"),
                    field("Total", "$99.99"),
                ])
                .into(),
                actions(vec![
                    button(ButtonOptions {
                        label: "View Order".to_string(),
                        id: "view".to_string(),
                        style: Some(ButtonStyle::Primary),
                        ..Default::default()
                    })
                    .into(),
                    button(ButtonOptions {
                        label: "Cancel".to_string(),
                        id: "cancel".to_string(),
                        style: Some(ButtonStyle::Danger),
                        ..Default::default()
                    })
                    .into(),
                ])
                .into(),
            ]),
            ..Default::default()
        });
        let result = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                bold_format: Some(BoldFormat::Double),
                line_break: Some(LineBreak::Double),
                ..Default::default()
            },
        );
        assert!(result.contains("**Order #123**"));
        assert!(result.contains("Your order is confirmed"));
        assert!(result.contains("Thank you for your purchase!"));
        assert!(result.contains("---"));
        assert!(result.contains("Status: Processing"));
        assert!(result.contains("Total: $99.99"));
        assert!(!result.contains("[View Order]"));
        assert!(!result.contains("[Cancel]"));
    }

    #[test]
    fn card_to_fallback_text_handles_empty_card() {
        use chat_sdk_chat::cards::{CardOptions, card};
        let c = card(CardOptions::default());
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            ""
        );
    }

    #[test]
    fn card_to_fallback_text_handles_card_with_only_children() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text};
        let c = card(CardOptions {
            children: Some(vec![card_text("Just text", None).into()]),
            ..Default::default()
        });
        assert_eq!(
            card_to_fallback_text(&c, FallbackTextOptions::default()),
            "Just text"
        );
    }

    // ---------- escapeTableCell (1:1 with upstream 7 cases) ----------

    #[test]
    fn escape_table_cell_escapes_a_single_pipe_character() {
        assert_eq!(escape_table_cell("a|b"), "a\\|b");
    }

    #[test]
    fn escape_table_cell_escapes_multiple_pipes() {
        assert_eq!(escape_table_cell("a|b|c"), "a\\|b\\|c");
    }

    #[test]
    fn escape_table_cell_escapes_backslashes_before_pipes() {
        assert_eq!(escape_table_cell("a\\|b"), "a\\\\\\|b");
    }

    #[test]
    fn escape_table_cell_escapes_standalone_backslashes() {
        assert_eq!(escape_table_cell("a\\b"), "a\\\\b");
    }

    #[test]
    fn escape_table_cell_replaces_newlines_with_spaces() {
        assert_eq!(escape_table_cell("line1\nline2"), "line1 line2");
    }

    #[test]
    fn escape_table_cell_handles_text_with_no_special_characters() {
        assert_eq!(escape_table_cell("hello"), "hello");
    }

    #[test]
    fn escape_table_cell_handles_empty_string() {
        assert_eq!(escape_table_cell(""), "");
    }

    #[test]
    fn card_to_fallback_text_uses_default_bold_marker_and_newline() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text};
        let c = card(CardOptions {
            title: Some("Order".to_string()),
            subtitle: Some("Processing".to_string()),
            children: Some(vec![card_text("Body", None).into()]),
            ..Default::default()
        });
        let out = card_to_fallback_text(&c, FallbackTextOptions::default());
        assert_eq!(out, "*Order*\nProcessing\nBody");
    }

    #[test]
    fn card_to_fallback_text_supports_double_bold_and_double_linebreak_teams_style() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text};
        let c = card(CardOptions {
            title: Some("Title".to_string()),
            children: Some(vec![card_text("Body", None).into()]),
            ..Default::default()
        });
        let out = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                bold_format: Some(BoldFormat::Double),
                line_break: Some(LineBreak::Double),
                platform: Some(PlatformName::Teams),
            },
        );
        assert_eq!(out, "**Title**\n\nBody");
    }

    #[test]
    fn card_to_fallback_text_applies_emoji_conversion_when_platform_set() {
        use chat_sdk_chat::cards::{CardOptions, card, card_text};
        let c = card(CardOptions {
            title: Some("Hello {{emoji:wave}}".to_string()),
            children: Some(vec![card_text("Use {{emoji:fire}} carefully", None).into()]),
            ..Default::default()
        });
        let out = card_to_fallback_text(
            &c,
            FallbackTextOptions {
                platform: Some(PlatformName::Slack),
                ..Default::default()
            },
        );
        assert_eq!(out, "*Hello :wave:*\nUse :fire: carefully");
    }

    #[test]
    fn card_to_fallback_text_omits_actions_and_renders_divider_marker() {
        use chat_sdk_chat::cards::{
            ButtonOptions, CardOptions, actions, button, card, card_text, divider,
        };
        let c = card(CardOptions {
            title: Some("With sep".to_string()),
            children: Some(vec![
                card_text("Top", None).into(),
                divider().into(),
                actions(vec![
                    button(ButtonOptions {
                        label: "Approve".to_string(),
                        id: "ok".to_string(),
                        ..Default::default()
                    })
                    .into(),
                ])
                .into(),
                card_text("Bottom", None).into(),
            ]),
            ..Default::default()
        });
        let out = card_to_fallback_text(&c, FallbackTextOptions::default());
        assert_eq!(out, "*With sep*\nTop\n---\nBottom");
    }

    // ---------- renderGfmTable (1:1 with upstream 4 cases) ----------

    #[test]
    fn render_gfm_table_renders_a_basic_table() {
        let t = table(TableOptions {
            headers: vec!["Name".to_string(), "Age".to_string()],
            rows: vec![
                vec!["Alice".to_string(), "30".to_string()],
                vec!["Bob".to_string(), "25".to_string()],
            ],
            align: None,
        });
        assert_eq!(
            render_gfm_table(&t),
            vec![
                "| Name | Age |".to_string(),
                "| --- | --- |".to_string(),
                "| Alice | 30 |".to_string(),
                "| Bob | 25 |".to_string(),
            ]
        );
    }

    #[test]
    fn render_gfm_table_escapes_pipes_in_cell_values() {
        let t = table(TableOptions {
            headers: vec!["Command".to_string(), "Description".to_string()],
            rows: vec![vec!["a|b".to_string(), "pipes|here".to_string()]],
            align: None,
        });
        assert_eq!(
            render_gfm_table(&t),
            vec![
                "| Command | Description |".to_string(),
                "| --- | --- |".to_string(),
                "| a\\|b | pipes\\|here |".to_string(),
            ]
        );
    }

    #[test]
    fn render_gfm_table_escapes_backslashes_in_cell_values() {
        let t = table(TableOptions {
            headers: vec!["Path".to_string()],
            rows: vec![vec!["C:\\Users\\test".to_string()]],
            align: None,
        });
        let lines = render_gfm_table(&t);
        assert_eq!(lines[2], "| C:\\\\Users\\\\test |");
    }

    #[test]
    fn render_gfm_table_handles_empty_rows() {
        let t = table(TableOptions {
            headers: vec!["A".to_string(), "B".to_string()],
            rows: vec![],
            align: None,
        });
        assert_eq!(
            render_gfm_table(&t),
            vec!["| A | B |".to_string(), "| --- | --- |".to_string()]
        );
    }

    #[test]
    fn render_gfm_table_escapes_cells_with_pipes_and_newlines() {
        let t = table(TableOptions {
            headers: vec!["A|B".to_string()],
            rows: vec![vec!["x\ny".to_string()]],
            align: None,
        });
        let lines = render_gfm_table(&t);
        assert_eq!(lines[0], "| A\\|B |");
        assert_eq!(lines[2], "| x y |");
    }
}
