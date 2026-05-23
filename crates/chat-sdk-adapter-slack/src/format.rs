//! Slack mrkdwn / text-object formatting helpers.
//!
//! 1:1 port (in progress) of `packages/adapter-slack/src/format/index.ts`.
//! This slice covers the simpler character-class + string-formatting
//! helpers. The regex-heavy `slackMrkdwnToMarkdown` /
//! `markdownBoldToSlackMrkdwn` / `linkBareSlackMentions` ports follow
//! in a future slice (they rely on lookbehind / negative-lookahead
//! patterns that need a small custom scanner in Rust without pulling
//! in the `regex` crate).

use serde::{Deserialize, Serialize};

/// `plain_text` text object. 1:1 with upstream
/// `interface SlackPlainTextObject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackPlainTextObject {
    pub text: String,
    #[serde(rename = "type")]
    pub kind: PlainTextKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlainTextKind {
    #[default]
    #[serde(rename = "plain_text")]
    PlainText,
}

/// `mrkdwn` text object. 1:1 with upstream
/// `interface SlackMrkdwnTextObject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackMrkdwnTextObject {
    pub text: String,
    #[serde(rename = "type")]
    pub kind: MrkdwnKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbatim: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MrkdwnKind {
    #[default]
    #[serde(rename = "mrkdwn")]
    Mrkdwn,
}

/// Options for [`create_slack_plain_text`] / [`create_slack_mrkdwn`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SlackTextOptions {
    pub emoji: Option<bool>,
    pub verbatim: Option<bool>,
}

/// Error returned by the format helpers on validation failure. 1:1
/// with upstream's `TypeError` throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackFormatError {
    pub message: String,
}

impl std::fmt::Display for SlackFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SlackFormatError {}

/// Max length of a Slack text object's `text` field. 1:1 with
/// upstream `TEXT_OBJECT_MAX_LENGTH = 3000`.
pub const TEXT_OBJECT_MAX_LENGTH: usize = 3000;

/// HTML-escape Slack-significant control characters
/// (`&`, `<`, `>`). 1:1 port of upstream `escapeSlackText(text)`.
pub fn escape_slack_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// Inverse of [`escape_slack_text`]: replace `&lt;`/`&gt;`/`&amp;`
/// with their literal characters. 1:1 with upstream
/// `unescapeSlackText(text)`. The replacement order matches
/// upstream's (`&lt;` -> `<`, `&gt;` -> `>`, `&amp;` -> `&`).
pub fn unescape_slack_text(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Build a `plain_text` text object. 1:1 port of upstream
/// `createSlackPlainText(text, options?)`.
pub fn create_slack_plain_text(
    text: &str,
    options: SlackTextOptions,
) -> Result<SlackPlainTextObject, SlackFormatError> {
    assert_slack_text_object_text(text)?;
    Ok(SlackPlainTextObject {
        text: text.to_string(),
        kind: PlainTextKind::PlainText,
        emoji: options.emoji,
    })
}

/// Build a `mrkdwn` text object. 1:1 port of upstream
/// `createSlackMrkdwn(text, options?)`.
pub fn create_slack_mrkdwn(
    text: &str,
    options: SlackTextOptions,
) -> Result<SlackMrkdwnTextObject, SlackFormatError> {
    assert_slack_text_object_text(text)?;
    Ok(SlackMrkdwnTextObject {
        text: text.to_string(),
        kind: MrkdwnKind::Mrkdwn,
        verbatim: options.verbatim,
    })
}

/// Format a user mention `<@U123>`. 1:1 with upstream
/// `formatSlackUser(userId)`.
pub fn format_slack_user(user_id: &str) -> Result<String, SlackFormatError> {
    assert_slack_id(user_id, "userId")?;
    Ok(format!("<@{user_id}>"))
}

/// Format a channel mention `<#C123>`. 1:1 with upstream
/// `formatSlackChannel(channelId)`.
pub fn format_slack_channel(channel_id: &str) -> Result<String, SlackFormatError> {
    assert_slack_id(channel_id, "channelId")?;
    Ok(format!("<#{channel_id}>"))
}

/// Format a user-group mention `<!subteam^S123>`. 1:1 with
/// upstream `formatSlackUserGroup(userGroupId)`.
pub fn format_slack_user_group(user_group_id: &str) -> Result<String, SlackFormatError> {
    assert_slack_id(user_group_id, "userGroupId")?;
    Ok(format!("<!subteam^{user_group_id}>"))
}

/// Slack special mention. 1:1 with upstream's
/// `"channel" | "everyone" | "here"` literal union.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlackSpecialMention {
    Channel,
    Everyone,
    Here,
}

impl SlackSpecialMention {
    fn as_str(self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Everyone => "everyone",
            Self::Here => "here",
        }
    }
}

/// Format a special mention `<!here>` / `<!channel>` /
/// `<!everyone>`. 1:1 with upstream
/// `formatSlackSpecialMention(mention)`.
pub fn format_slack_special_mention(mention: SlackSpecialMention) -> String {
    format!("<!{}>", mention.as_str())
}

/// Format a link `<url>` or `<url|label>`. 1:1 with upstream
/// `formatSlackLink(url, label?)`. The label is HTML-escaped via
/// [`escape_slack_text`].
pub fn format_slack_link(url: &str, label: Option<&str>) -> Result<String, SlackFormatError> {
    assert_no_slack_control(url, "url")?;
    Ok(match label {
        Some(label) => format!("<{url}|{}>", escape_slack_text(label)),
        None => format!("<{url}>"),
    })
}

/// Format a Slack date token `<!date^<seconds>^<token>[^<link>]|<fallback>>`.
/// 1:1 with upstream `formatSlackDate(timestamp, token, fallback, options?)`.
/// The timestamp is in seconds (use `seconds_from_date_ms` to
/// convert a JS-style ms timestamp).
pub fn format_slack_date(
    seconds: i64,
    token: &str,
    fallback: &str,
    link: Option<&str>,
) -> Result<String, SlackFormatError> {
    assert_no_slack_date_control(token, "token")?;
    let link_part = match link {
        Some(href) => {
            assert_no_slack_date_control(href, "link")?;
            format!("^{href}")
        }
        None => String::new(),
    };
    Ok(format!(
        "<!date^{seconds}^{token}{link_part}|{}>",
        escape_slack_text(fallback)
    ))
}

/// Convert a JS millisecond Unix timestamp to seconds (floored)
/// for [`format_slack_date`]. Helper mirroring upstream's inline
/// `Math.floor(timestamp.getTime() / 1000)`.
pub fn seconds_from_date_ms(ms: i64) -> i64 {
    ms.div_euclid(1000)
}

// ---------- private validators ----------

fn assert_slack_text_object_text(text: &str) -> Result<(), SlackFormatError> {
    let len = text.chars().count();
    if !(1..=TEXT_OBJECT_MAX_LENGTH).contains(&len) {
        return Err(SlackFormatError {
            message: format!(
                "text must be between 1 and {TEXT_OBJECT_MAX_LENGTH} characters"
            ),
        });
    }
    Ok(())
}

fn assert_slack_id(value: &str, name: &str) -> Result<(), SlackFormatError> {
    // 1:1 with upstream's /^[A-Z0-9_]+$/
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return Err(SlackFormatError {
            message: format!("{name} must be a Slack ID"),
        });
    }
    Ok(())
}

fn assert_no_slack_control(value: &str, name: &str) -> Result<(), SlackFormatError> {
    // 1:1 with upstream /[<>|]/
    if value.chars().any(|c| matches!(c, '<' | '>' | '|')) {
        return Err(SlackFormatError {
            message: format!("{name} cannot contain Slack control characters"),
        });
    }
    Ok(())
}

fn assert_no_slack_date_control(value: &str, name: &str) -> Result<(), SlackFormatError> {
    // 1:1 with upstream /[\^|>]/
    if value.chars().any(|c| matches!(c, '^' | '|' | '>')) {
        return Err(SlackFormatError {
            message: format!("{name} cannot contain Slack date control characters"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- ported upstream cases ----------

    #[test]
    fn escapes_slack_mrkdwn_control_characters() {
        assert_eq!(escape_slack_text("a & <b>"), "a &amp; &lt;b&gt;");
    }

    #[test]
    fn unescapes_slack_mrkdwn_control_characters() {
        assert_eq!(unescape_slack_text("a &amp; &lt;b&gt;"), "a & <b>");
    }

    #[test]
    fn creates_plain_text_objects() {
        let obj = create_slack_plain_text(
            "hello",
            SlackTextOptions {
                emoji: Some(true),
                verbatim: None,
            },
        )
        .unwrap();
        assert_eq!(obj.text, "hello");
        assert_eq!(obj.kind, PlainTextKind::PlainText);
        assert_eq!(obj.emoji, Some(true));
    }

    #[test]
    fn rejects_invalid_text_object_lengths() {
        assert!(create_slack_plain_text("", SlackTextOptions::default()).is_err());
        let too_long = "x".repeat(3001);
        assert!(create_slack_mrkdwn(&too_long, SlackTextOptions::default()).is_err());
    }

    #[test]
    fn creates_mrkdwn_objects() {
        let obj = create_slack_mrkdwn(
            "*hello*",
            SlackTextOptions {
                emoji: None,
                verbatim: Some(true),
            },
        )
        .unwrap();
        assert_eq!(obj.text, "*hello*");
        assert_eq!(obj.kind, MrkdwnKind::Mrkdwn);
        assert_eq!(obj.verbatim, Some(true));
    }

    #[test]
    fn formats_slack_user_mentions() {
        assert_eq!(format_slack_user("U123").unwrap(), "<@U123>");
    }

    #[test]
    fn formats_slack_channel_mentions() {
        assert_eq!(format_slack_channel("C123").unwrap(), "<#C123>");
    }

    #[test]
    fn formats_slack_user_group_mentions() {
        assert_eq!(format_slack_user_group("S123").unwrap(), "<!subteam^S123>");
    }

    #[test]
    fn formats_slack_special_mentions() {
        assert_eq!(
            format_slack_special_mention(SlackSpecialMention::Here),
            "<!here>"
        );
    }

    #[test]
    fn formats_slack_links() {
        assert_eq!(
            format_slack_link("https://example.com?a=1&b=2", None).unwrap(),
            "<https://example.com?a=1&b=2>"
        );
        assert_eq!(
            format_slack_link("https://example.com", Some("read <this>")).unwrap(),
            "<https://example.com|read &lt;this&gt;>"
        );
    }

    #[test]
    fn rejects_unsafe_slack_link_control_characters() {
        assert!(format_slack_link("https://example.com|bad", None).is_err());
    }

    #[test]
    fn formats_slack_dates() {
        assert_eq!(
            format_slack_date(1_710_000_000, "{date_short}", "Mar 9", None).unwrap(),
            "<!date^1710000000^{date_short}|Mar 9>"
        );
        // 2024-03-09T16:00:00.000Z -> 1710000000 seconds
        assert_eq!(
            format_slack_date(
                seconds_from_date_ms(1_710_000_000_000),
                "{time}",
                "4pm",
                Some("https://example.com")
            )
            .unwrap(),
            "<!date^1710000000^{time}^https://example.com|4pm>"
        );
    }

    // ---------- additive Rust-side ----------

    #[test]
    fn slack_id_rejects_lowercase_and_special_chars() {
        assert!(format_slack_user("u123").is_err());
        assert!(format_slack_user("U-123").is_err());
        assert!(format_slack_user("").is_err());
    }

    #[test]
    fn unescape_is_the_inverse_of_escape_for_safe_inputs() {
        for input in ["hello", "a&b", "a < b > c", "a&amp;"] {
            let escaped = escape_slack_text(input);
            // Round-tripping arbitrary input doesn't always recover the
            // original (e.g. "a&amp;" double-encodes), but the simple
            // cases hold.
            if !input.contains("&amp;") && !input.contains("&lt;") && !input.contains("&gt;") {
                assert_eq!(unescape_slack_text(&escaped), input);
            }
        }
    }

    #[test]
    fn date_helpers_floor_negative_millis() {
        // -1ms should floor to -1s (1:1 with Math.floor semantics).
        assert_eq!(seconds_from_date_ms(-1), -1);
        assert_eq!(seconds_from_date_ms(1500), 1);
        assert_eq!(seconds_from_date_ms(-1500), -2);
    }
}
