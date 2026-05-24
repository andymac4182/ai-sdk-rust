//! Discord card rendering.
//!
//! 1:1 port of `packages/adapter-discord/src/cards.ts`. Covers both
//! `cardToFallbackText` (plain-markdown fallback) and the full
//! `cardToDiscordPayload` Embed + Action-Row renderer (incl. Table
//! via GFM, CardLink via the `Link` branch, and `ValidationError`
//! propagation when a button `id + value` overflows the 100-char
//! `custom_id` cap).

use chat_sdk_adapter_shared::card_utils::render_gfm_table;
use chat_sdk_adapter_shared::errors::AdapterError;
use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, ButtonElement, ButtonStyle, CardChild, CardElement,
    FieldsElement, LinkButtonElement, SectionElement, TableElement,
    card_child_to_fallback_text,
};
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

/// Discord button-style enum. 1:1 with `discord-api-types/v10`
/// `ButtonStyle`. Numeric values are stable per the Discord API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiscordButtonStyle {
    Primary = 1,
    Secondary = 2,
    Success = 3,
    Danger = 4,
    Link = 5,
}

/// Discord button component. 1:1 with upstream `DiscordButton` (type
/// `APIButtonComponent`). `type` is always `2` on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordButton {
    /// Discord component type — always `2` for buttons.
    pub component_type: u8,
    pub style: DiscordButtonStyle,
    pub label: String,
    /// Custom-id for interactive (non-link) buttons.
    pub custom_id: Option<String>,
    /// URL for link buttons (style = [`DiscordButtonStyle::Link`]).
    pub url: Option<String>,
    /// Whether the button is disabled. Omitted (None) when the
    /// upstream renderer didn't set it — matches upstream's `if
    /// (button.disabled) { discordButton.disabled = true; }`.
    pub disabled: Option<bool>,
}

/// Discord message-component action row. 1:1 with upstream
/// `DiscordActionRow` (type `APIActionRowComponent`). `type` is
/// always `1` on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordActionRow {
    /// Discord component type — always `1` for action rows.
    pub component_type: u8,
    pub components: Vec<DiscordButton>,
}

impl Default for DiscordActionRow {
    fn default() -> Self {
        Self {
            component_type: 1,
            components: Vec::new(),
        }
    }
}

/// Discord allows a maximum of 5 components per action row.
pub const DISCORD_ACTION_ROW_MAX_COMPONENTS: usize = 5;

/// Result of [`card_to_discord_payload`]. 1:1 with upstream
/// `{embeds, components}` return shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordPayload {
    pub embeds: Vec<DiscordEmbed>,
    pub components: Vec<DiscordActionRow>,
}

/// Convert a [`CardElement`] to a Discord message payload (embeds
/// + components). 1:1 port of upstream `cardToDiscordPayload(card)`.
/// Handles:
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
/// - `Actions` children -> [`DiscordActionRow`]s (chunked 5/row)
/// - `Section` children -> recursively flattened
/// - `Fields` children -> `embed.fields` (inline = true)
/// - `Link` children (including `CardLink`) -> markdown
///   `[label](url)` in description
/// - `Table` children -> GFM markdown table in description (via
///   [`render_gfm_table`])
///
/// Returns `Err(AdapterError::Validation)` when a button's
/// `id` + `value` exceeds Discord's 100-char `custom_id` cap,
/// matching upstream's `throw ValidationError` from
/// `encodeDiscordCustomId`.
///
/// Select / RadioSelect components inside `Actions` are silently
/// dropped — only `Button` and `LinkButton` are emitted, matching
/// upstream's `filter((child) => child.type === "button" ||
/// child.type === "link-button")`.
pub fn card_to_discord_payload(card: &CardElement) -> Result<DiscordPayload, AdapterError> {
    let mut embed = DiscordEmbed {
        color: Some(DISCORD_EMBED_DEFAULT_COLOR),
        ..Default::default()
    };
    let mut components: Vec<DiscordActionRow> = Vec::new();

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
        process_child(child, &mut text_parts, &mut embed.fields, &mut components)?;
    }

    if !text_parts.is_empty() {
        let joined = text_parts.join("\n\n");
        embed.description = Some(match embed.description.take() {
            Some(existing) => format!("{existing}\n\n{joined}"),
            None => joined,
        });
    }

    Ok(DiscordPayload {
        embeds: vec![embed],
        components,
    })
}

fn process_child(
    child: &CardChild,
    text_parts: &mut Vec<String>,
    fields: &mut Vec<DiscordEmbedField>,
    components: &mut Vec<DiscordActionRow>,
) -> Result<(), AdapterError> {
    match child {
        CardChild::Text(t) => {
            text_parts.push(convert_text_element(t));
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
        CardChild::Actions(a) => {
            components.extend(convert_actions_to_rows(a)?);
        }
        CardChild::Section(s) => {
            process_section_element(s, text_parts, fields, components)?;
        }
        CardChild::Fields(f) => {
            convert_fields_element(f, fields);
        }
        CardChild::Link(l) => {
            text_parts.push(format!("[{}]({})", convert_emoji(&l.label), l.url));
        }
        CardChild::Table(t) => {
            text_parts.push(render_table_element(t));
        }
    }
    Ok(())
}

/// 1:1 with upstream `renderGfmTable(child).join("\n")` — the inline
/// table-handling branch of `processChild`.
fn render_table_element(element: &TableElement) -> String {
    render_gfm_table(element).join("\n")
}

/// 1:1 with upstream `convertTextElement(element)`. Applies style:
/// `bold` -> `**...**`, `muted` -> `*...*` (italic approximation,
/// since Discord has no muted).
fn convert_text_element(element: &chat_sdk_chat::cards::TextElement) -> String {
    let text = convert_emoji(&element.content);
    match element.style {
        Some(chat_sdk_chat::cards::TextStyle::Bold) => format!("**{text}**"),
        Some(chat_sdk_chat::cards::TextStyle::Muted) => format!("*{text}*"),
        _ => text,
    }
}

/// 1:1 with upstream `convertActionsToRows(element)`. Filters out
/// non-button children, then chunks the remaining buttons into
/// action rows of at most 5. Returns `Err` if any
/// [`convert_button_element`] call rejects an id/value pair (matches
/// upstream's bubbling throw from `encodeDiscordCustomId`).
fn convert_actions_to_rows(element: &ActionsElement) -> Result<Vec<DiscordActionRow>, AdapterError> {
    let mut buttons: Vec<DiscordButton> = Vec::new();
    for child in &element.children {
        match child {
            ActionsChild::Button(b) => buttons.push(convert_button_element(b)?),
            ActionsChild::LinkButton(b) => buttons.push(convert_link_button_element(b)),
            _ => {}
        }
    }

    Ok(buttons
        .chunks(DISCORD_ACTION_ROW_MAX_COMPONENTS)
        .map(|chunk| DiscordActionRow {
            component_type: 1,
            components: chunk.to_vec(),
        })
        .collect())
}

/// 1:1 with upstream `convertButtonElement(button)`. Returns a
/// `ValidationError` when `encode_discord_custom_id` rejects the
/// id/value pair — upstream surfaces this as a thrown exception.
fn convert_button_element(button: &ButtonElement) -> Result<DiscordButton, AdapterError> {
    let custom_id = encode_discord_custom_id(&button.id, button.value.as_deref())?;
    Ok(DiscordButton {
        component_type: 2,
        style: get_button_style(button.style),
        label: button.label.clone(),
        custom_id: Some(custom_id),
        url: None,
        disabled: button.disabled.and_then(|d| if d { Some(true) } else { None }),
    })
}

/// 1:1 with upstream `convertLinkButtonElement(button)`. Always
/// emits style = [`DiscordButtonStyle::Link`].
fn convert_link_button_element(button: &LinkButtonElement) -> DiscordButton {
    DiscordButton {
        component_type: 2,
        style: DiscordButtonStyle::Link,
        label: button.label.clone(),
        custom_id: None,
        url: Some(button.url.clone()),
        disabled: None,
    }
}

/// 1:1 with upstream `getButtonStyle(style)`. Maps card-level
/// `ButtonStyle` to Discord-API `ButtonStyle`; `Default` and all
/// other variants collapse to `Secondary`.
fn get_button_style(style: Option<ButtonStyle>) -> DiscordButtonStyle {
    match style {
        Some(ButtonStyle::Primary) => DiscordButtonStyle::Primary,
        Some(ButtonStyle::Danger) => DiscordButtonStyle::Danger,
        _ => DiscordButtonStyle::Secondary,
    }
}

/// 1:1 with upstream `processSectionElement(element, ...)`.
/// Recursively flattens section children into the parent embed's
/// text/fields/components.
fn process_section_element(
    element: &SectionElement,
    text_parts: &mut Vec<String>,
    fields: &mut Vec<DiscordEmbedField>,
    components: &mut Vec<DiscordActionRow>,
) -> Result<(), AdapterError> {
    for child in &element.children {
        process_child(child, text_parts, fields, components)?;
    }
    Ok(())
}

/// 1:1 with upstream `convertFieldsElement(element, fields)`. All
/// fields are emitted `inline: true`.
fn convert_fields_element(element: &FieldsElement, fields: &mut Vec<DiscordEmbedField>) {
    for field in &element.children {
        fields.push(DiscordEmbedField {
            name: convert_emoji(&field.label),
            value: convert_emoji(&field.value),
            inline: Some(true),
        });
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
        ActionsChild, ActionsElement, ActionsKind, ButtonElement, ButtonKind, ButtonStyle, CardKind,
        DividerElement, DividerKind, FieldElement, FieldKind, FieldsElement, FieldsKind, LinkKind,
        LinkButtonElement, LinkButtonKind, SectionElement, SectionKind, TextElement, TextKind,
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
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.embeds.len(), 1);
        assert_eq!(payload.embeds[0].title.as_deref(), Some("Welcome"));
        assert_eq!(payload.components.len(), 0);
    }

    #[test]
    fn converts_a_card_with_title_and_subtitle() {
        let c = card(Some("Order Update"), Some("Your order is on its way"), vec![]);
        let payload = card_to_discord_payload(&c).unwrap();
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
        let payload = card_to_discord_payload(&c).unwrap();
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
        let payload = card_to_discord_payload(&c).unwrap();
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
        let payload = card_to_discord_payload(&c).unwrap();
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
        let payload = card_to_discord_payload(&c).unwrap();
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
        let payload = card_to_discord_payload(&c).unwrap();
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("Before"), "got: {description}");
        assert!(description.contains("───────────"), "got: {description}");
        assert!(description.contains("After"), "got: {description}");
    }

    // ---------- cardToDiscordPayload Action Row + Section + Fields (8 cases) ----------

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
    fn converts_actions_with_buttons() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button("approve", "Approve", Some(ButtonStyle::Primary), None)),
                    ActionsChild::Button(button("reject", "Reject", Some(ButtonStyle::Danger), Some("data-123"))),
                    ActionsChild::Button(button("skip", "Skip", None, None)),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.components.len(), 1);
        assert_eq!(payload.components[0].component_type, 1);
        let buttons = &payload.components[0].components;
        assert_eq!(buttons.len(), 3);
        assert_eq!(
            buttons[0],
            DiscordButton {
                component_type: 2,
                style: DiscordButtonStyle::Primary,
                label: "Approve".to_string(),
                custom_id: Some("approve".to_string()),
                url: None,
                disabled: None,
            }
        );
        assert_eq!(
            buttons[1],
            DiscordButton {
                component_type: 2,
                style: DiscordButtonStyle::Danger,
                label: "Reject".to_string(),
                custom_id: Some("reject\ndata-123".to_string()),
                url: None,
                disabled: None,
            }
        );
        assert_eq!(
            buttons[2],
            DiscordButton {
                component_type: 2,
                style: DiscordButtonStyle::Secondary,
                label: "Skip".to_string(),
                custom_id: Some("skip".to_string()),
                url: None,
                disabled: None,
            }
        );
    }

    #[test]
    fn sets_disabled_on_button_when_specified() {
        let mut cancel = button("cancel", "Cancelled", Some(ButtonStyle::Danger), None);
        cancel.disabled = Some(true);
        let retry = button("retry", "Retry", None, None);
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Button(cancel), ActionsChild::Button(retry)],
                kind: ActionsKind::Actions,
            })],
        );
        let payload = card_to_discord_payload(&c).unwrap();
        let buttons = &payload.components[0].components;
        assert_eq!(buttons.len(), 2);
        assert_eq!(
            buttons[0],
            DiscordButton {
                component_type: 2,
                style: DiscordButtonStyle::Danger,
                label: "Cancelled".to_string(),
                custom_id: Some("cancel".to_string()),
                url: None,
                disabled: Some(true),
            }
        );
        assert_eq!(
            buttons[1],
            DiscordButton {
                component_type: 2,
                style: DiscordButtonStyle::Secondary,
                label: "Retry".to_string(),
                custom_id: Some("retry".to_string()),
                url: None,
                disabled: None,
            }
        );
    }

    #[test]
    fn converts_link_buttons_using_link_style() {
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::LinkButton(LinkButtonElement {
                    label: "View Docs".to_string(),
                    style: None,
                    kind: LinkButtonKind::LinkButton,
                    url: "https://example.com/docs".to_string(),
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.components.len(), 1);
        assert_eq!(payload.components[0].component_type, 1);
        let buttons = &payload.components[0].components;
        assert_eq!(buttons.len(), 1);
        assert_eq!(
            buttons[0],
            DiscordButton {
                component_type: 2,
                style: DiscordButtonStyle::Link,
                label: "View Docs".to_string(),
                custom_id: None,
                url: Some("https://example.com/docs".to_string()),
                disabled: None,
            }
        );
    }

    #[test]
    fn converts_fields_to_embed_fields() {
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
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.embeds[0].fields.len(), 2);
        assert_eq!(
            payload.embeds[0].fields[0],
            DiscordEmbedField {
                name: "Status".to_string(),
                value: "Active".to_string(),
                inline: Some(true),
            }
        );
        assert_eq!(
            payload.embeds[0].fields[1],
            DiscordEmbedField {
                name: "Priority".to_string(),
                value: "High".to_string(),
                inline: Some(true),
            }
        );
    }

    #[test]
    fn flattens_section_children() {
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
        let payload = card_to_discord_payload(&c).unwrap();
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("Inside section"), "got: {description}");
        assert!(description.contains("───────────"), "got: {description}");
    }

    #[test]
    fn converts_a_complete_card() {
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
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.embeds.len(), 1);
        assert_eq!(payload.embeds[0].title.as_deref(), Some("Order #1234"));
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("Status update"), "got: {description}");
        assert!(
            description.contains("Your order has been shipped!"),
            "got: {description}"
        );
        assert!(description.contains("───────────"), "got: {description}");
        assert_eq!(payload.embeds[0].fields.len(), 2);
        assert_eq!(payload.components.len(), 1);
        assert_eq!(payload.components[0].components.len(), 1);
    }

    #[test]
    fn handles_card_with_no_title_or_subtitle() {
        let c = card(None, None, vec![text("Just content")]);
        let payload = card_to_discord_payload(&c).unwrap();
        assert!(payload.embeds[0].title.is_none());
        assert_eq!(payload.embeds[0].description.as_deref(), Some("Just content"));
    }

    #[test]
    fn combines_title_subtitle_and_content() {
        let c = card(Some("Title"), Some("Subtitle"), vec![text("Content")]);
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.embeds[0].title.as_deref(), Some("Title"));
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("Subtitle"), "got: {description}");
        assert!(description.contains("Content"), "got: {description}");
    }

    // ---------- cardToDiscordPayload with CardLink + 2 codec cases ----------

    #[test]
    fn appends_markdown_link_to_embed_description() {
        // 1:1 with upstream `cardToDiscordPayload with CardLink >
        // appends markdown link to embed description`. CardLink is
        // a LinkElement (type "link"), so it routes through the
        // generic `CardChild::Link` branch.
        use chat_sdk_chat::cards::LinkElement;
        let c = card(
            None,
            None,
            vec![CardChild::Link(LinkElement {
                label: "Click here".to_string(),
                kind: LinkKind::Link,
                url: "https://example.com".to_string(),
            })],
        );
        let payload = card_to_discord_payload(&c).unwrap();
        assert_eq!(payload.embeds.len(), 1);
        assert_eq!(
            payload.embeds[0].description.as_deref(),
            Some("[Click here](https://example.com)")
        );
    }

    #[test]
    fn throws_when_a_button_value_makes_custom_id_too_long() {
        // 1:1 with upstream `encodeDiscordCustomId /
        // decodeDiscordCustomId > throws when a button value makes
        // custom_id too long`. id is 90 chars + value 20 chars +
        // delimiter = 111 chars, exceeds the 100-char cap.
        let mut b = button(&"x".repeat(90), "Approve", None, Some("__cb:1234567890abcdef"));
        b.value = Some("__cb:1234567890abcdef".to_string());
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Button(b)],
                kind: ActionsKind::Actions,
            })],
        );
        let err = card_to_discord_payload(&c).unwrap_err();
        match err {
            AdapterError::Validation { .. } => {}
            other => panic!("expected Validation error, got: {other:?}"),
        }
    }

    #[test]
    fn renders_cards_with_values_into_discord_button_payloads() {
        // 1:1 with upstream `encodeDiscordCustomId /
        // decodeDiscordCustomId > renders cards with values into
        // Discord button payloads`. Buttons with values get
        // `id\nvalue` custom_id; buttons without get just `id`.
        let c = card(
            None,
            None,
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    ActionsChild::Button(button("approve", "Approve", None, Some("order-99"))),
                    ActionsChild::Button(button("deny", "Deny", None, None)),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let payload = card_to_discord_payload(&c).unwrap();
        let buttons = &payload.components[0].components;
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].custom_id.as_deref(), Some("approve\norder-99"));
        assert_eq!(buttons[1].custom_id.as_deref(), Some("deny"));
    }

    // ---------- additive Table coverage (no upstream Table case in
    // cards.test.ts but the Table branch needs renderer-level coverage)

    #[test]
    fn renders_table_children_as_gfm_table_in_description() {
        use chat_sdk_chat::cards::{TableElement as Tbl, TableKind};
        let c = card(
            None,
            None,
            vec![CardChild::Table(Tbl {
                align: None,
                headers: vec!["Name".to_string(), "Status".to_string()],
                rows: vec![
                    vec!["Alpha".to_string(), "Active".to_string()],
                    vec!["Beta | Pipe".to_string(), "Idle\nWrap".to_string()],
                ],
                kind: TableKind::Table,
            })],
        );
        let payload = card_to_discord_payload(&c).unwrap();
        let description = payload.embeds[0].description.as_deref().unwrap_or("");
        assert!(description.contains("| Name | Status |"), "got: {description}");
        assert!(description.contains("| --- | --- |"), "got: {description}");
        assert!(description.contains("| Alpha | Active |"), "got: {description}");
        // Pipes inside cells must be escaped; newlines collapsed to space.
        assert!(
            description.contains("| Beta \\| Pipe | Idle Wrap |"),
            "got: {description}"
        );
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
