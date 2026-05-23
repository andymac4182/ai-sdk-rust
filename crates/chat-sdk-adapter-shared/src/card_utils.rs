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
//! **What remains for follow-up slices:**
//!
//! - `createEmojiConverter` (depends on `chat::emoji::convertEmojiPlaceholders`
//!   which has not been ported yet).
//! - `cardToFallbackText` + `childToFallbackText` (depend on the
//!   upstream JSX-side `cardChildToFallbackText` helper, which is in
//!   the `js-only-documented` JSX runtime layer of `chat::cards`).

use chat_sdk_chat::cards::{ButtonStyle, TableElement};
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
    //! 1:1 port (subset) of
    //! `packages/adapter-shared/src/card-utils.test.ts` covering the
    //! standalone helpers shipped in slice 38. Tests for the deferred
    //! `createEmojiConverter` and `cardToFallbackText` follow when their
    //! `chat::emoji`/`chat::cards` JSX-layer dependencies land.

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
