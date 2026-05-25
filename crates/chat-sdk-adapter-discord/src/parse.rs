//! Discord inbound-message parsing.
//!
//! 1:1 port of `DiscordAdapter#parseMessage(raw)` and the
//! supporting `parseDiscordMessage` / `getAttachmentType` helpers
//! from `packages/adapter-discord/src/index.ts`.
//!
//! Upstream signature: `parseMessage(raw: unknown): Message<unknown>`.
//! The Rust port takes the raw JSON [`serde_json::Value`] (matching
//! upstream's `unknown`) and returns a cross-platform
//! [`chat_sdk_chat::message::Message`].
//!
//! Exposed as a module-scope `pub fn` (rather than only as an
//! adapter method) so the upstream
//! `describe("parseMessage" / "edge cases" / "date parsing" /
//! "formatted text extraction")` cases can be exercised without
//! going through HTTP-call dispatch — same pattern as
//! `chat-sdk-adapter-whatsapp/src/parse.rs`.

use chat_sdk_chat::markdown::{Node, root};
use chat_sdk_chat::message::Message;
use chat_sdk_chat::types::{Attachment, AttachmentKind, Author, BotStatus, MessageMetadata};
use serde_json::Value;

use crate::encode_thread_id;
use crate::markdown::DiscordFormatConverter;

/// Discord `MessageType.ThreadStarterMessage` value. 1:1 with
/// upstream `MessageType.ThreadStarterMessage` from
/// `discord-api-types/v10` which equals `21`.
pub const MESSAGE_TYPE_THREAD_STARTER: i64 = 21;

/// Determine attachment type from MIME type. 1:1 with upstream
/// `protected getAttachmentType(mimeType?: string | null)`:
/// `image/*` -> `image`, `video/*` -> `video`, `audio/*` -> `audio`,
/// otherwise (including undefined/null) -> `file`.
pub fn discord_attachment_type(mime_type: Option<&str>) -> AttachmentKind {
    match mime_type {
        None => AttachmentKind::File,
        Some(m) if m.starts_with("image/") => AttachmentKind::Image,
        Some(m) if m.starts_with("video/") => AttachmentKind::Video,
        Some(m) if m.starts_with("audio/") => AttachmentKind::Audio,
        Some(_) => AttachmentKind::File,
    }
}

/// 1:1 with upstream `DiscordAdapter.parseMessage(raw)`:
///
/// - `guildId = msg.guild_id ?? "@me"`
/// - `threadId = encodeThreadId({guildId, channelId: msg.channel_id})`
/// - Delegates to `parseDiscordMessage(msg, threadId)` which:
///   - Walks `referenced_message` instead of `msg` when
///     `msg.type === ThreadStarterMessage` and `referenced_message`
///     is present (upstream uses the referenced message because the
///     thread-starter placeholder has empty content).
///   - Falls back to the original `msg` when `referenced_message` is
///     missing/null (preserves the placeholder's id/text/author).
///   - Extracts plain-text + AST via [`DiscordFormatConverter`].
///   - Builds [`Attachment`]s from `msg.attachments[]` via
///     [`discord_attachment_type`].
///
/// `bot_user_id` is used to set `author.is_me` (`true` when the
/// author id matches the bot's). Upstream reads `this.botUserId`
/// which is initialized in `adapter.initialize` to the application
/// id; the Rust port accepts it as an explicit parameter so the
/// pure helper has no hidden dependency on adapter state.
///
/// Returns `None` when the JSON shape is missing required fields
/// (`id`, `channel_id`, `author.id`, `author.username`,
/// `timestamp`); upstream's `as APIMessage` cast would propagate
/// undefined in those cases and a downstream call would throw — the
/// Rust port surfaces the shape error as `None` rather than panic.
pub fn parse_discord_message(raw: &Value, bot_user_id: Option<&str>) -> Option<Message> {
    let msg_type = raw.get("type").and_then(Value::as_i64).unwrap_or(0);
    let referenced = raw.get("referenced_message");
    let referenced_is_value = matches!(referenced, Some(v) if !v.is_null());

    // Upstream: use referenced_message when type === ThreadStarter +
    // referenced_message is present; otherwise fall through to msg.
    let effective = if msg_type == MESSAGE_TYPE_THREAD_STARTER && referenced_is_value {
        referenced.unwrap()
    } else {
        raw
    };

    let id = effective.get("id").and_then(Value::as_str)?.to_string();
    let channel_id = effective
        .get("channel_id")
        .and_then(Value::as_str)?
        .to_string();
    let guild_id = effective
        .get("guild_id")
        .and_then(Value::as_str)
        .unwrap_or("@me")
        .to_string();

    let author_value = effective.get("author")?;
    let author_id = author_value.get("id").and_then(Value::as_str)?.to_string();
    let user_name = author_value
        .get("username")
        .and_then(Value::as_str)?
        .to_string();
    let global_name = author_value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let full_name = global_name
        .map(str::to_string)
        .unwrap_or_else(|| user_name.clone());
    let is_bot_known = author_value
        .get("bot")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let content = effective
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let timestamp = effective
        .get("timestamp")
        .and_then(Value::as_str)?
        .to_string();
    let edited_at = effective
        .get("edited_timestamp")
        .and_then(Value::as_str)
        .map(str::to_string);
    let edited = edited_at.is_some();

    let converter = DiscordFormatConverter::new();
    let text = converter.extract_plain_text(&content);
    let ast = converter.to_ast(&content);
    let formatted_root = match ast {
        Node::Root(r) => r,
        other => root(vec![other]),
    };

    let thread_id = encode_thread_id(&guild_id, &channel_id);

    let attachments_value = effective.get("attachments").cloned().unwrap_or(Value::Null);
    let attachments = if let Some(arr) = attachments_value.as_array() {
        arr.iter()
            .map(|att| {
                let content_type = att
                    .get("content_type")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let kind = discord_attachment_type(content_type.as_deref());
                let width = att.get("width").and_then(Value::as_u64).map(|v| v as u32);
                let height = att.get("height").and_then(Value::as_u64).map(|v| v as u32);
                let size = att.get("size").and_then(Value::as_u64);
                Attachment {
                    data: None,
                    fetch_metadata: None,
                    height,
                    mime_type: content_type,
                    name: att
                        .get("filename")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    size,
                    kind,
                    url: att.get("url").and_then(Value::as_str).map(str::to_string),
                    width,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let author = Author {
        user_id: author_id.clone(),
        user_name,
        full_name,
        is_bot: if is_bot_known {
            BotStatus::TRUE
        } else {
            BotStatus::FALSE
        },
        is_me: bot_user_id.map(|id| id == author_id).unwrap_or(false),
    };

    let metadata = MessageMetadata {
        date_sent: timestamp,
        edited,
        edited_at,
    };

    Some(Message::new(
        id,
        thread_id,
        text,
        formatted_root,
        raw.clone(),
        author,
        metadata,
        attachments,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::types::AttachmentKind;
    use serde_json::json;

    fn basic_message() -> Value {
        json!({
            "id": "message123",
            "channel_id": "channel456",
            "guild_id": "guild789",
            "author": {
                "id": "user123",
                "username": "testuser",
                "discriminator": "0001",
                "global_name": "Test User",
            },
            "content": "Hello world",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0,
        })
    }

    // 1:1 with upstream index.test.ts:769 > describe("parseMessage")
    // > it("parses a basic message")
    #[test]
    fn parse_message_parses_a_basic_message() {
        let raw = basic_message();
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.id, "message123");
        assert_eq!(m.text, "Hello world");
        assert_eq!(m.author.user_id, "user123");
        assert_eq!(m.author.user_name, "testuser");
        assert_eq!(m.author.full_name, "Test User");
        assert_eq!(m.author.is_bot, BotStatus::FALSE);
        assert_eq!(m.thread_id, "discord:guild789:channel456");
    }

    // 1:1 with upstream index.test.ts:804 > describe("parseMessage")
    // > it("parses a bot message")
    #[test]
    fn parse_message_parses_a_bot_message() {
        let raw = json!({
            "id": "message123",
            "channel_id": "channel456",
            "author": {
                "id": "bot123",
                "username": "somebot",
                "discriminator": "0000",
                "bot": true,
            },
            "content": "Bot message",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0,
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.author.user_id, "bot123");
        assert_eq!(m.author.is_bot, BotStatus::TRUE);
    }

    // 1:1 with upstream index.test.ts:833 > describe("parseMessage")
    // > it("parses a DM message (no guild_id)")
    #[test]
    fn parse_message_parses_a_dm_message() {
        let raw = json!({
            "id": "message123",
            "channel_id": "dm456",
            "author": {
                "id": "user123",
                "username": "testuser",
                "discriminator": "0001",
            },
            "content": "DM message",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0,
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.thread_id, "discord:@me:dm456");
    }

    // 1:1 with upstream index.test.ts:860 > describe("parseMessage")
    // > it("parses edited message")
    #[test]
    fn parse_message_parses_edited_message() {
        let mut raw = basic_message();
        raw["edited_timestamp"] = json!("2021-01-01T00:01:00.000Z");
        raw["content"] = json!("Edited message");
        let m = parse_discord_message(&raw, None).expect("message");
        assert!(m.metadata.edited);
        assert_eq!(
            m.metadata.edited_at.as_deref(),
            Some("2021-01-01T00:01:00.000Z")
        );
    }

    // 1:1 with upstream index.test.ts:891 > describe("parseMessage")
    // > it("uses referenced_message content for thread starter messages")
    #[test]
    fn parse_message_uses_referenced_message_for_thread_starter() {
        let raw = json!({
            "id": "starter123",
            "channel_id": "thread456",
            "guild_id": "guild789",
            "author": {
                "id": "system",
                "username": "system",
                "discriminator": "0000",
                "bot": true,
            },
            "content": "",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "type": MESSAGE_TYPE_THREAD_STARTER,
            "message_reference": {
                "message_id": "parent123",
                "channel_id": "channel456",
                "guild_id": "guild789",
            },
            "referenced_message": {
                "id": "parent123",
                "channel_id": "channel456",
                "guild_id": "guild789",
                "author": {
                    "id": "user123",
                    "username": "parent-author",
                    "discriminator": "0001",
                    "global_name": "Parent Author",
                },
                "content": "Parent message content",
                "timestamp": "2021-01-01T00:00:00.000Z",
                "edited_timestamp": null,
                "attachments": [],
                "embeds": [],
                "type": 0,
            },
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.id, "parent123");
        assert_eq!(m.text, "Parent message content");
        assert_eq!(m.author.user_id, "user123");
        // thread_id encodes the referenced message's guild + channel
        assert_eq!(m.thread_id, "discord:guild789:channel456");
    }

    // 1:1 with upstream index.test.ts:950 > describe("parseMessage")
    // > it("falls back gracefully when thread starter has no referenced_message")
    #[test]
    fn parse_message_thread_starter_without_referenced_falls_back() {
        let raw = json!({
            "id": "starter123",
            "channel_id": "thread456",
            "guild_id": "guild789",
            "author": {
                "id": "system",
                "username": "system",
                "discriminator": "0000",
                "bot": true,
            },
            "content": "",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "type": MESSAGE_TYPE_THREAD_STARTER,
            "message_reference": {
                "message_id": "parent123",
                "channel_id": "channel456",
                "guild_id": "guild789",
            },
            "referenced_message": null,
            "attachments": [],
            "embeds": [],
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.id, "starter123");
        assert_eq!(m.text, "");
        assert_eq!(m.author.user_id, "system");
    }

    // 1:1 with upstream index.test.ts:987 > describe("parseMessage")
    // > it("parses message with attachments")
    #[test]
    fn parse_message_parses_message_with_attachments() {
        let raw = json!({
            "id": "message123",
            "channel_id": "channel456",
            "guild_id": "guild789",
            "author": {"id": "user123", "username": "testuser", "discriminator": "0001"},
            "content": "Message with attachment",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "attachments": [
                {
                    "id": "att123",
                    "filename": "image.png",
                    "size": 12345,
                    "url": "https://cdn.discord.com/image.png",
                    "proxy_url": "https://media.discord.com/image.png",
                    "content_type": "image/png",
                    "width": 800,
                    "height": 600,
                }
            ],
            "embeds": [],
            "type": 0,
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.attachments.len(), 1);
        let att = &m.attachments[0];
        assert_eq!(att.kind, AttachmentKind::Image);
        assert_eq!(att.name.as_deref(), Some("image.png"));
        assert_eq!(att.mime_type.as_deref(), Some("image/png"));
        assert_eq!(att.width, Some(800));
        assert_eq!(att.height, Some(600));
    }

    // 1:1 with upstream index.test.ts:1031 > describe("parseMessage")
    // > it("handles different attachment types")
    #[test]
    fn parse_message_handles_different_attachment_types() {
        let make = |ct: &str| {
            json!({
                "id": "message123",
                "channel_id": "channel456",
                "author": {"id": "user123", "username": "testuser", "discriminator": "0001"},
                "content": "",
                "timestamp": "2021-01-01T00:00:00.000Z",
                "edited_timestamp": null,
                "attachments": [{
                    "id": "att123",
                    "filename": "file",
                    "size": 1000,
                    "url": "https://example.com",
                    "proxy_url": "https://example.com",
                    "content_type": ct,
                }],
                "embeds": [],
                "type": 0,
            })
        };
        assert_eq!(
            parse_discord_message(&make("image/jpeg"), None)
                .unwrap()
                .attachments[0]
                .kind,
            AttachmentKind::Image
        );
        assert_eq!(
            parse_discord_message(&make("video/mp4"), None)
                .unwrap()
                .attachments[0]
                .kind,
            AttachmentKind::Video
        );
        assert_eq!(
            parse_discord_message(&make("audio/mpeg"), None)
                .unwrap()
                .attachments[0]
                .kind,
            AttachmentKind::Audio
        );
        assert_eq!(
            parse_discord_message(&make("application/pdf"), None)
                .unwrap()
                .attachments[0]
                .kind,
            AttachmentKind::File
        );
    }

    // 1:1 with upstream index.test.ts:1075 > describe("parseMessage")
    // > it("uses username as fullName when global_name is missing")
    #[test]
    fn parse_message_uses_username_as_full_name_when_global_name_missing() {
        let raw = json!({
            "id": "message123",
            "channel_id": "channel456",
            "author": {"id": "user123", "username": "testuser", "discriminator": "0001"},
            "content": "Hello",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "type": 0,
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.author.full_name, "testuser");
    }

    // 1:1 with upstream index.test.ts:1162 > describe("edge cases")
    // > it("handles empty content in message")
    #[test]
    fn edge_cases_handles_empty_content_in_message() {
        let mut raw = basic_message();
        raw["content"] = json!("");
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.text, "");
    }

    // 1:1 with upstream index.test.ts:1188 > describe("edge cases")
    // > it("handles null width/height in attachments")
    #[test]
    fn edge_cases_handles_null_width_height_in_attachments() {
        let raw = json!({
            "id": "message123",
            "channel_id": "channel456",
            "author": {"id": "user123", "username": "testuser", "discriminator": "0001"},
            "content": "",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "attachments": [{
                "id": "att123",
                "filename": "doc.pdf",
                "size": 1000,
                "url": "https://example.com",
                "proxy_url": "https://example.com",
                "content_type": "application/pdf",
                "width": null,
                "height": null,
            }],
            "embeds": [],
            "type": 0,
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.attachments[0].width, None);
        assert_eq!(m.attachments[0].height, None);
    }

    // 1:1 with upstream index.test.ts:1226 > describe("edge cases")
    // > it("handles missing attachment content_type")
    #[test]
    fn edge_cases_handles_missing_attachment_content_type() {
        let raw = json!({
            "id": "message123",
            "channel_id": "channel456",
            "author": {"id": "user123", "username": "testuser", "discriminator": "0001"},
            "content": "",
            "timestamp": "2021-01-01T00:00:00.000Z",
            "edited_timestamp": null,
            "attachments": [{
                "id": "att123",
                "filename": "unknown",
                "size": 1000,
                "url": "https://example.com",
                "proxy_url": "https://example.com",
            }],
            "embeds": [],
            "type": 0,
        });
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.attachments[0].kind, AttachmentKind::File);
    }

    // 1:1 with upstream index.test.ts:1273 > describe("date parsing")
    // > it("parses ISO timestamp to Date")
    #[test]
    fn date_parsing_parses_iso_timestamp() {
        let mut raw = basic_message();
        raw["timestamp"] = json!("2021-01-01T12:30:00.000Z");
        let m = parse_discord_message(&raw, None).expect("message");
        // Rust port stores the ISO string verbatim (Message::metadata
        // .date_sent is a String per the shared types). Upstream
        // wraps it as `new Date(msg.timestamp)`; the string-form is
        // the parity invariant the Rust shared types use.
        assert_eq!(m.metadata.date_sent, "2021-01-01T12:30:00.000Z");
    }

    // 1:1 with upstream index.test.ts:1314 > describe("formatted text extraction")
    // > it("extracts plain text from Discord markdown")
    #[test]
    fn formatted_text_extraction_extracts_plain_text() {
        let mut raw = basic_message();
        raw["content"] = json!("**bold** and *italic*");
        let m = parse_discord_message(&raw, None).expect("message");
        assert_eq!(m.text, "bold and italic");
    }

    // 1:1 with upstream index.test.ts:1340 > describe("formatted text extraction")
    // > it("extracts text from user mentions")
    #[test]
    fn formatted_text_extraction_extracts_text_from_user_mentions() {
        let mut raw = basic_message();
        raw["content"] = json!("Hey <@456789>!");
        let m = parse_discord_message(&raw, None).expect("message");
        assert!(m.text.contains("@456789"), "text: {}", m.text);
    }

    // 1:1 with upstream index.test.ts:1366 > describe("formatted text extraction")
    // > it("extracts text from channel mentions")
    #[test]
    fn formatted_text_extraction_extracts_text_from_channel_mentions() {
        let mut raw = basic_message();
        raw["content"] = json!("Check <#987654>");
        let m = parse_discord_message(&raw, None).expect("message");
        assert!(m.text.contains("#987654"), "text: {}", m.text);
    }

    // Additive: bot_user_id parameter sets author.is_me when matching.
    #[test]
    fn parse_message_sets_is_me_when_bot_user_id_matches_author() {
        let raw = basic_message();
        let m = parse_discord_message(&raw, Some("user123")).expect("message");
        assert!(m.author.is_me);
        let m2 = parse_discord_message(&raw, Some("other-user")).expect("message");
        assert!(!m2.author.is_me);
    }

    // Additive: discord_attachment_type pure helper covers the
    // explicit none/image/video/audio/file branches.
    #[test]
    fn discord_attachment_type_classifies_by_mime_prefix() {
        assert_eq!(discord_attachment_type(None), AttachmentKind::File);
        assert_eq!(
            discord_attachment_type(Some("image/png")),
            AttachmentKind::Image
        );
        assert_eq!(
            discord_attachment_type(Some("video/mp4")),
            AttachmentKind::Video
        );
        assert_eq!(
            discord_attachment_type(Some("audio/mpeg")),
            AttachmentKind::Audio
        );
        assert_eq!(
            discord_attachment_type(Some("application/pdf")),
            AttachmentKind::File
        );
        assert_eq!(discord_attachment_type(Some("")), AttachmentKind::File);
    }
}
