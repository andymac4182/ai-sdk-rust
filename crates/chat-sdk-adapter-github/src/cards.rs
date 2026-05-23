//! Convert a chat-sdk [`CardElement`] to GitHub-flavored markdown.
//!
//! 1:1 port of `packages/adapter-github/src/cards.ts`. GitHub
//! comments don't support rich cards, so cards render as clean
//! markdown with bold title/subtitle, key:value field pairs,
//! `[label](url)` link buttons, and `**[label]**` for action
//! buttons (no interactivity in GitHub).

use chat_sdk_adapter_shared::card_utils::render_gfm_table;
use chat_sdk_chat::cards::{
    ActionsChild, ActionsElement, CardChild, CardElement, FieldsElement, TableElement, TextElement,
    TextStyle, card_child_to_fallback_text,
};

/// Convert a [`CardElement`] to GitHub-flavored markdown. 1:1 port
/// of upstream `cardToGitHubMarkdown(card)`.
pub fn card_to_github_markdown(card: &CardElement) -> String {
    let mut lines: Vec<String> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        lines.push(format!("**{}**", escape_markdown(title)));
    }

    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        lines.push(escape_markdown(subtitle));
    }

    let has_header = card.title.as_deref().filter(|t| !t.is_empty()).is_some()
        || card.subtitle.as_deref().filter(|s| !s.is_empty()).is_some();
    if has_header && !card.children.is_empty() {
        lines.push(String::new());
    }

    if let Some(image_url) = card.image_url.as_deref().filter(|u| !u.is_empty()) {
        lines.push(format!("![]({image_url})"));
        lines.push(String::new());
    }

    let last = card.children.len().saturating_sub(1);
    for (i, child) in card.children.iter().enumerate() {
        let child_lines = render_child(child);
        if !child_lines.is_empty() {
            lines.extend(child_lines);
            if i < last {
                lines.push(String::new());
            }
        }
    }

    lines.join("\n")
}

/// Generate plain-text fallback from a [`CardElement`]. 1:1 port of
/// upstream `cardToPlainText(card)`.
pub fn card_to_plain_text(card: &CardElement) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(title) = card.title.as_deref().filter(|t| !t.is_empty()) {
        parts.push(title.to_string());
    }
    if let Some(subtitle) = card.subtitle.as_deref().filter(|s| !s.is_empty()) {
        parts.push(subtitle.to_string());
    }

    for child in &card.children {
        if let Some(text) = child_to_plain_text(child) {
            parts.push(text);
        }
    }

    parts.join("\n")
}

fn render_child(child: &CardChild) -> Vec<String> {
    match child {
        CardChild::Text(t) => render_text(t),
        CardChild::Fields(f) => render_fields(f),
        CardChild::Actions(a) => render_actions(a),
        CardChild::Section(s) => s.children.iter().flat_map(render_child).collect(),
        CardChild::Image(img) => {
            if let Some(alt) = img.alt.as_deref().filter(|a| !a.is_empty()) {
                vec![format!("![{}]({})", escape_markdown(alt), img.url)]
            } else {
                vec![format!("![]({})", img.url)]
            }
        }
        CardChild::Link(l) => vec![format!("[{}]({})", escape_markdown(&l.label), l.url)],
        CardChild::Divider(_) => vec!["---".to_string()],
        CardChild::Table(t) => render_table(t),
    }
}

fn render_text(t: &TextElement) -> Vec<String> {
    match t.style {
        Some(TextStyle::Bold) => vec![format!("**{}**", t.content)],
        Some(TextStyle::Muted) => vec![format!("_{}_", t.content)],
        _ => vec![t.content.clone()],
    }
}

fn render_fields(fields: &FieldsElement) -> Vec<String> {
    fields
        .children
        .iter()
        .map(|f| {
            format!(
                "**{}:** {}",
                escape_markdown(&f.label),
                escape_markdown(&f.value)
            )
        })
        .collect()
}

fn render_table(t: &TableElement) -> Vec<String> {
    render_gfm_table(t)
}

fn render_actions(actions: &ActionsElement) -> Vec<String> {
    let pieces: Vec<String> = actions
        .children
        .iter()
        .map(|btn| match btn {
            ActionsChild::LinkButton(lb) => {
                format!("[{}]({})", escape_markdown(&lb.label), lb.url)
            }
            ActionsChild::Button(b) => format!("**[{}]**", escape_markdown(&b.label)),
            // Select/RadioSelect don't render as buttons; fall back to
            // bold label text.
            ActionsChild::Select(s) => format!("**[{}]**", escape_markdown(&s.label)),
            ActionsChild::RadioSelect(rs) => {
                format!("**[{}]**", escape_markdown(&rs.label))
            }
        })
        .collect();
    vec![pieces.join(" • ")]
}

fn child_to_plain_text(child: &CardChild) -> Option<String> {
    match child {
        CardChild::Text(t) => Some(t.content.clone()),
        CardChild::Fields(f) => Some(
            f.children
                .iter()
                .map(|fld| format!("{}: {}", fld.label, fld.value))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        // Actions are interactive-only — exclude from fallback text.
        CardChild::Actions(_) => None,
        CardChild::Table(t) => Some(render_table(t).join("\n")),
        CardChild::Section(s) => {
            let pieces: Vec<String> = s.children.iter().filter_map(child_to_plain_text).collect();
            if pieces.is_empty() {
                None
            } else {
                Some(pieces.join("\n"))
            }
        }
        other => card_child_to_fallback_text(other),
    }
}

/// Escape markdown-significant characters. 1:1 with upstream
/// `escapeMarkdown(text)`: backslash, asterisk, underscore, square
/// brackets.
fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '*' => out.push_str("\\*"),
            '_' => out.push_str("\\_"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{
        ActionsKind, ButtonElement, ButtonKind, CardKind, FieldElement, FieldKind, FieldsKind,
        LinkButtonElement, LinkButtonKind, TextKind,
    };

    fn empty_card(title: &str) -> CardElement {
        CardElement {
            title: Some(title.to_string()),
            subtitle: None,
            image_url: None,
            children: vec![],
            kind: CardKind::Card,
        }
    }

    fn text(content: &str) -> CardChild {
        CardChild::Text(TextElement {
            content: content.to_string(),
            style: None,
            kind: TextKind::Text,
        })
    }

    fn field(label: &str, value: &str) -> FieldElement {
        FieldElement {
            label: label.to_string(),
            value: value.to_string(),
            kind: FieldKind::Field,
        }
    }

    fn link_button(label: &str, url: &str) -> ActionsChild {
        ActionsChild::LinkButton(LinkButtonElement {
            label: label.to_string(),
            url: url.to_string(),
            style: None,
            kind: LinkButtonKind::LinkButton,
        })
    }

    fn action_button(id: &str, label: &str) -> ActionsChild {
        ActionsChild::Button(ButtonElement {
            id: id.to_string(),
            label: label.to_string(),
            action_type: None,
            callback_url: None,
            disabled: None,
            style: None,
            value: None,
            kind: ButtonKind::Button,
        })
    }

    // ---------- cardToGitHubMarkdown: 6 ported upstream cases ----------

    #[test]
    fn should_render_a_simple_card_with_title() {
        let result = card_to_github_markdown(&empty_card("Hello World"));
        assert_eq!(result, "**Hello World**");
    }

    #[test]
    fn should_render_card_with_title_and_subtitle() {
        let mut card = empty_card("Order #1234");
        card.subtitle = Some("Status update".to_string());
        let result = card_to_github_markdown(&card);
        assert_eq!(result, "**Order #1234**\nStatus update");
    }

    #[test]
    fn should_render_card_with_text_content() {
        let mut card = empty_card("Notification");
        card.children = vec![text("Your order has been shipped!")];
        let result = card_to_github_markdown(&card);
        assert_eq!(result, "**Notification**\n\nYour order has been shipped!");
    }

    #[test]
    fn should_render_card_with_fields() {
        let mut card = empty_card("Order Details");
        card.children = vec![CardChild::Fields(FieldsElement {
            children: vec![field("Order ID", "12345"), field("Status", "Shipped")],
            kind: FieldsKind::Fields,
        })];
        let result = card_to_github_markdown(&card);
        assert!(result.contains("**Order ID:** 12345"));
        assert!(result.contains("**Status:** Shipped"));
    }

    #[test]
    fn should_render_card_with_link_buttons() {
        let mut card = empty_card("Actions");
        card.children = vec![CardChild::Actions(ActionsElement {
            children: vec![
                link_button("Track Order", "https://example.com/track"),
                link_button("Get Help", "https://example.com/help"),
            ],
            kind: ActionsKind::Actions,
        })];
        let result = card_to_github_markdown(&card);
        assert!(result.contains("[Track Order](https://example.com/track)"));
        assert!(result.contains("[Get Help](https://example.com/help)"));
    }

    #[test]
    fn should_render_card_with_action_buttons_as_bold_text() {
        let mut card = empty_card("Approve?");
        card.children = vec![CardChild::Actions(ActionsElement {
            children: vec![action_button("approve", "Approve")],
            kind: ActionsKind::Actions,
        })];
        let result = card_to_github_markdown(&card);
        assert!(result.contains("**[Approve]**"));
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn empty_card_renders_to_empty_string() {
        let card = CardElement {
            title: None,
            subtitle: None,
            image_url: None,
            children: vec![],
            kind: CardKind::Card,
        };
        assert_eq!(card_to_github_markdown(&card), "");
    }

    #[test]
    fn escape_markdown_escapes_asterisks_underscores_brackets_and_backslashes() {
        assert_eq!(
            escape_markdown(r"text with * _ [ ] \ chars"),
            r"text with \* \_ \[ \] \\ chars"
        );
    }

    #[test]
    fn divider_renders_as_horizontal_rule() {
        use chat_sdk_chat::cards::{DividerElement, DividerKind};
        let mut card = empty_card("Section");
        card.children = vec![CardChild::Divider(DividerElement {
            kind: DividerKind::Divider,
        })];
        let result = card_to_github_markdown(&card);
        assert!(result.ends_with("---"));
    }

    #[test]
    fn card_to_plain_text_strips_markdown() {
        let mut card = empty_card("Order");
        card.subtitle = Some("Shipped".to_string());
        card.children = vec![text("Tracking ABC123")];
        let result = card_to_plain_text(&card);
        assert_eq!(result, "Order\nShipped\nTracking ABC123");
    }

    #[test]
    fn card_to_plain_text_excludes_actions() {
        // Actions are interactive-only - upstream returns null and the
        // joiner skips it.
        let mut card = empty_card("Approve?");
        card.children = vec![CardChild::Actions(ActionsElement {
            children: vec![action_button("approve", "Approve")],
            kind: ActionsKind::Actions,
        })];
        let result = card_to_plain_text(&card);
        assert_eq!(result, "Approve?");
    }
}
