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

/// Convert Slack mrkdwn to standard Markdown. 1:1 port of upstream
/// `slackMrkdwnToMarkdown(mrkdwn)` - rewrites Slack's
/// `<@U123|label>`, `<#C123|label>`, `<url|label>` link/mention
/// tokens into Markdown equivalents, then translates Slack's
/// single-asterisk bold (`*x*`) -> `**x**` and single-tilde
/// strikethrough (`~x~`) -> `~~x~~`, and finally
/// HTML-unescapes via [`unescape_slack_text`].
pub fn slack_mrkdwn_to_markdown(mrkdwn: &str) -> String {
    let s = replace_slack_user_with_label(mrkdwn);
    let s = replace_slack_user_no_label(&s);
    let s = replace_slack_channel_with_label(&s);
    let s = replace_slack_channel_no_label(&s);
    let s = replace_slack_link_with_label(&s);
    let s = replace_slack_link_no_label(&s);
    let s = replace_slack_bold_to_markdown(&s);
    let s = replace_slack_strike_to_markdown(&s);
    unescape_slack_text(&s)
}

/// Convert standard Markdown bold (`**x**`) to Slack mrkdwn
/// bold (`*x*`). 1:1 port of upstream
/// `markdownBoldToSlackMrkdwn(markdown)`.
pub fn markdown_bold_to_slack_mrkdwn(markdown: &str) -> String {
    let bytes: Vec<char> = markdown.chars().collect();
    let mut out = String::with_capacity(markdown.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == '*' && bytes[i + 1] == '*' {
            // Find matching `**` after at least 1 char.
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == '*' && bytes[j + 1] == '*' {
                    break;
                }
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == '*' && bytes[j + 1] == '*' && j > i + 2 {
                out.push('*');
                out.extend(&bytes[i + 2..j]);
                out.push('*');
                i = j + 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Replace bare `@U123` tokens with `<@U123>`. 1:1 port of
/// upstream `linkBareSlackMentions(text)` - matches Slack ID
/// patterns (`@[A-Z][A-Z0-9_]+`) NOT preceded by `<` or a word
/// character (so plain emails `user@example.com` and angle-
/// wrapped `<@U123>` are skipped).
pub fn link_bare_slack_mentions(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '@' {
            let prev_ok = i == 0 || (bytes[i - 1] != '<' && !is_word_char(bytes[i - 1]));
            // First char of an ID must be uppercase ASCII letter; the
            // rest must be [A-Z0-9_] and there must be at least 1
            // following char (upstream `[A-Z][A-Z0-9_]+`).
            if prev_ok && i + 1 < bytes.len() && bytes[i + 1].is_ascii_uppercase() {
                let mut j = i + 2;
                while j < bytes.len() && is_slack_id_char(bytes[j]) {
                    j += 1;
                }
                if j > i + 2 {
                    out.push_str("<@");
                    out.extend(&bytes[i + 1..j]);
                    out.push('>');
                    i = j;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn is_word_char(ch: char) -> bool {
    // 1:1 with JS `\w`: ASCII alphanumeric + underscore.
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_slack_id_char(ch: char) -> bool {
    ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_'
}

// ---------- private regex-equivalent scanners (slack_mrkdwn_to_markdown) ----------

/// Replace `<@USER|label>` -> `@label`.
fn replace_slack_user_with_label(text: &str) -> String {
    replace_id_link(text, "<@", false, |id, label| {
        format!("@{}", label.unwrap_or(id))
    })
}

/// Replace `<@USER>` -> `@USER`. Must run after the with-label
/// pass (the with-label pass strips `<@USER|...>`).
fn replace_slack_user_no_label(text: &str) -> String {
    replace_id_link(text, "<@", true, |id, _| format!("@{id}"))
}

/// Replace `<#CHANNEL|label>` -> `#label`.
fn replace_slack_channel_with_label(text: &str) -> String {
    replace_id_link(text, "<#", false, |id, label| {
        format!("#{}", label.unwrap_or(id))
    })
}

/// Replace `<#CHANNEL>` -> `#CHANNEL`.
fn replace_slack_channel_no_label(text: &str) -> String {
    replace_id_link(text, "<#", true, |id, _| format!("#{id}"))
}

/// Replace `<url|label>` -> `[label](url)`. Only matches http(s)
/// URLs to mirror upstream's `(https?:\/\/[^|<>]+)` anchor.
fn replace_slack_link_with_label(text: &str) -> String {
    replace_url_link(text, false, |url, label| match label {
        Some(label) => format!("[{label}]({url})"),
        None => format!("<{url}>"),
    })
}

/// Replace `<url>` (no label) -> `url`. Must run after the
/// with-label pass.
fn replace_slack_link_no_label(text: &str) -> String {
    replace_url_link(text, true, |url, _| url.to_string())
}

/// Generic `<prefix><id>[|label]>` scanner. When
/// `no_label_only` is true, only matches tokens without a `|`
/// (used as the second pass after with-label).
fn replace_id_link(
    text: &str,
    prefix: &str,
    no_label_only: bool,
    render: impl Fn(&str, Option<&str>) -> String,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find(prefix) {
        out.push_str(&rest[..idx]);
        let after_prefix = &rest[idx + prefix.len()..];
        // Scan an id: [A-Z0-9_]+
        let id_end = after_prefix
            .find(|c: char| !is_slack_id_char(c))
            .unwrap_or(after_prefix.len());
        if id_end == 0 {
            // Not a valid token; emit prefix as-is and continue.
            out.push_str(prefix);
            rest = after_prefix;
            continue;
        }
        let id = &after_prefix[..id_end];
        let after_id = &after_prefix[id_end..];
        if let Some(after_pipe) = after_id.strip_prefix('|') {
            if no_label_only {
                // Skip this match - we only want no-label tokens
                // in this pass.
                out.push_str(prefix);
                out.push_str(id);
                rest = after_id;
                continue;
            }
            let close = after_pipe.find('>');
            if let Some(end) = close {
                let label = &after_pipe[..end];
                // Forbid `<` or `>` inside the label (upstream regex
                // anchors `[^<>]+`).
                if !label.contains('<') && !label.contains('>') && !label.is_empty() {
                    out.push_str(&render(id, Some(label)));
                    rest = &after_pipe[end + 1..];
                    continue;
                }
            }
            // Bad shape; passthrough.
            out.push_str(prefix);
            out.push_str(id);
            rest = after_id;
            continue;
        }
        if after_id.starts_with('>') {
            if !no_label_only {
                // Skip - we only want with-label tokens in this pass.
                out.push_str(prefix);
                out.push_str(id);
                rest = after_id;
                continue;
            }
            out.push_str(&render(id, None));
            rest = &after_id[1..];
            continue;
        }
        // Bad shape; passthrough.
        out.push_str(prefix);
        out.push_str(id);
        rest = after_id;
    }
    out.push_str(rest);
    out
}

/// Generic `<url[|label]>` scanner for http(s) URLs. When
/// `no_label_only` is true, only matches tokens without a `|`.
fn replace_url_link(
    text: &str,
    no_label_only: bool,
    render: impl Fn(&str, Option<&str>) -> String,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find('<') {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + 1..];
        let is_http = after.starts_with("https://") || after.starts_with("http://");
        if !is_http {
            out.push('<');
            rest = after;
            continue;
        }
        // URL body: stop at `|` (only when with-label) / `<` / `>`.
        let url_end = after
            .find(|c: char| c == '|' || c == '<' || c == '>')
            .unwrap_or(after.len());
        let url = &after[..url_end];
        let rest_after_url = &after[url_end..];
        if let Some(after_pipe) = rest_after_url.strip_prefix('|') {
            if no_label_only {
                out.push('<');
                rest = after;
                continue;
            }
            let close = after_pipe.find('>');
            if let Some(end) = close {
                let label = &after_pipe[..end];
                if !label.contains('<') && !label.contains('>') && !label.is_empty() {
                    out.push_str(&render(url, Some(label)));
                    rest = &after_pipe[end + 1..];
                    continue;
                }
            }
            out.push('<');
            rest = after;
            continue;
        }
        if rest_after_url.starts_with('>') {
            if !no_label_only {
                out.push('<');
                rest = after;
                continue;
            }
            out.push_str(&render(url, None));
            rest = &rest_after_url[1..];
            continue;
        }
        out.push('<');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Slack `*bold*` -> Markdown `**bold**`. Upstream regex:
/// `/(?<![_*\\])\*([^*\n]+)\*(?![_*])/g`. Lookbehind is rejection
/// of `_`/`*`/`\` before the opener; lookahead is rejection of
/// `_`/`*` after the closer.
fn replace_slack_bold_to_markdown(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' {
            let prev_ok = i == 0 || !matches!(chars[i - 1], '_' | '*' | '\\');
            if prev_ok {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '*' && chars[j] != '\n' {
                    j += 1;
                }
                if j > i + 1 && j < chars.len() && chars[j] == '*' {
                    let next_ok = j + 1 == chars.len() || !matches!(chars[j + 1], '_' | '*');
                    if next_ok {
                        out.push_str("**");
                        out.extend(&chars[i + 1..j]);
                        out.push_str("**");
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Slack `~strike~` -> Markdown `~~strike~~`. Upstream regex:
/// `/(?<!~)~([^~\n]+)~(?!~)/g`.
fn replace_slack_strike_to_markdown(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '~' {
            let prev_ok = i == 0 || chars[i - 1] != '~';
            if prev_ok {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '~' && chars[j] != '\n' {
                    j += 1;
                }
                if j > i + 1 && j < chars.len() && chars[j] == '~' {
                    let next_ok = j + 1 == chars.len() || chars[j + 1] != '~';
                    if next_ok {
                        out.push_str("~~");
                        out.extend(&chars[i + 1..j]);
                        out.push_str("~~");
                        i = j + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// ---------- private validators ----------

fn assert_slack_text_object_text(text: &str) -> Result<(), SlackFormatError> {
    let len = text.chars().count();
    if !(1..=TEXT_OBJECT_MAX_LENGTH).contains(&len) {
        return Err(SlackFormatError {
            message: format!("text must be between 1 and {TEXT_OBJECT_MAX_LENGTH} characters"),
        });
    }
    Ok(())
}

fn assert_slack_id(value: &str, name: &str) -> Result<(), SlackFormatError> {
    // 1:1 with upstream's /^[A-Z0-9_]+$/
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
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

    #[test]
    fn normalizes_slack_mrkdwn_to_markdown() {
        let result = slack_mrkdwn_to_markdown(
            "Hey <@U123|jane> in <#C123|general>, see <https://example.com|this> and *bold* ~done~",
        );
        assert_eq!(
            result,
            "Hey @jane in #general, see [this](https://example.com) and **bold** ~~done~~"
        );
    }

    #[test]
    fn normalizes_bare_slack_links_to_markdown_urls() {
        assert_eq!(
            slack_mrkdwn_to_markdown("See <https://example.com>"),
            "See https://example.com"
        );
    }

    #[test]
    fn converts_basic_markdown_bold_to_slack_mrkdwn_bold() {
        assert_eq!(
            markdown_bold_to_slack_mrkdwn("The **domain** is example.com"),
            "The *domain* is example.com"
        );
    }

    #[test]
    fn links_bare_mention_like_tokens_without_touching_emails() {
        assert_eq!(
            link_bare_slack_mentions("(cc @U123, @U456)"),
            "(cc <@U123>, <@U456>)"
        );
        assert_eq!(link_bare_slack_mentions("@george"), "@george");
        assert_eq!(
            link_bare_slack_mentions("user@example.com"),
            "user@example.com"
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
