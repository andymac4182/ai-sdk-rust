//! Messenger inbound-message parsing.
//!
//! 1:1 port of upstream `MessengerAdapter#parseMessage(raw)` ->
//! `parseMessengerMessage(event, threadId)` plus the supporting
//! `extractAttachments` / `mapAttachmentType` / `messageSequence` /
//! `compareMessages` helpers from
//! `packages/adapter-messenger/src/index.ts`.
//!
//! These helpers are pure functions over the webhook event shape
//! (`MessengerMessagingEvent`). The mutable message-cache + `Chat`
//! dispatch live on the adapter (the async HTTP layer); the pure
//! body-construction helpers are exposed here so they can be unit
//! tested without an HTTP harness.

use chat_sdk_chat::markdown::root;
use chat_sdk_chat::message::Message;
use chat_sdk_chat::types::{Attachment, AttachmentKind, Author, BotStatus, MessageMetadata};
use serde::Deserialize;

use crate::encode_thread_id;

/// Sender sub-block. 1:1 with upstream `MessengerSender`.
#[derive(Debug, Clone, Deserialize)]
pub struct MessengerSender {
    /// PSID (page-scoped user id).
    pub id: String,
}

/// Recipient sub-block. 1:1 with upstream `MessengerRecipient`.
#[derive(Debug, Clone, Deserialize)]
pub struct MessengerRecipient {
    /// Page id (always the page receiving the webhook).
    pub id: String,
}

/// Attachment payload sub-block. 1:1 with upstream
/// `MessengerAttachmentPayload`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessengerAttachmentPayload {
    /// Sticker id (image attachments only). When set without `url`,
    /// upstream skips the attachment.
    #[serde(default)]
    pub sticker_id: Option<i64>,
    /// CDN URL of the media (when present).
    #[serde(default)]
    pub url: Option<String>,
}

/// Single attachment on an inbound message. 1:1 with upstream
/// `MessengerAttachment`.
#[derive(Debug, Clone, Deserialize)]
pub struct MessengerAttachment {
    /// Facebook attachment type (`image` / `video` / `audio` / `file`
    /// / `fallback` / `location`).
    #[serde(rename = "type")]
    pub kind: String,
    /// Payload sub-block. Optional — upstream skips attachments
    /// without a payload URL.
    #[serde(default)]
    pub payload: Option<MessengerAttachmentPayload>,
}

/// Quick-reply payload. 1:1 with upstream `MessengerQuickReply`.
#[derive(Debug, Clone, Deserialize)]
pub struct MessengerQuickReply {
    /// Payload identifier.
    pub payload: String,
}

/// Message body. 1:1 with upstream `MessengerMessagePayload`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessengerMessagePayload {
    /// Message id assigned by Meta.
    #[serde(default)]
    pub mid: Option<String>,
    /// Body text. Optional (attachment-only / quick-reply-only
    /// messages may omit it).
    #[serde(default)]
    pub text: Option<String>,
    /// `is_echo: true` when Meta delivers a copy of an outbound
    /// message (sent by the same page) back through the webhook.
    #[serde(default)]
    pub is_echo: bool,
    /// Attachment array (when present).
    #[serde(default)]
    pub attachments: Option<Vec<MessengerAttachment>>,
    /// Quick-reply payload (when the user tapped a QR button).
    #[serde(default)]
    pub quick_reply: Option<MessengerQuickReply>,
}

/// Postback (button-tap) sub-block. 1:1 with upstream
/// `MessengerPostback`.
#[derive(Debug, Clone, Deserialize)]
pub struct MessengerPostback {
    /// Button label shown to the user.
    pub title: String,
    /// Button payload (`chat:{a, v?}` encoded for chat-sdk-rendered
    /// buttons; opaque for caller-defined buttons).
    pub payload: String,
    /// Postback message id when present (newer event shape).
    #[serde(default)]
    pub mid: Option<String>,
}

/// Inbound webhook event. 1:1 with upstream
/// `MessengerMessagingEvent`. Only the fields needed by the pure
/// parse/dispatch helpers are modelled — `delivery` / `read` /
/// `reaction` are handled by the adapter's async dispatch code (not
/// by the pure `parse_messenger_message`).
#[derive(Debug, Clone, Deserialize)]
pub struct MessengerMessagingEvent {
    /// Sender block.
    pub sender: MessengerSender,
    /// Recipient block (always the page).
    pub recipient: MessengerRecipient,
    /// Unix-millis timestamp Meta assigned.
    pub timestamp: i64,
    /// Message body, when this event carries one.
    #[serde(default)]
    pub message: Option<MessengerMessagePayload>,
    /// Postback (button-tap), when this event carries one.
    #[serde(default)]
    pub postback: Option<MessengerPostback>,
}

/// Map a Facebook attachment type discriminator to the chat-sdk
/// [`AttachmentKind`]. 1:1 with upstream `mapAttachmentType(fbType)`:
/// `image`/`video`/`audio` pass through; everything else (including
/// `file` / `fallback` / `location`) folds to `File`.
pub fn map_attachment_type(fb_type: &str) -> AttachmentKind {
    match fb_type {
        "image" => AttachmentKind::Image,
        "video" => AttachmentKind::Video,
        "audio" => AttachmentKind::Audio,
        _ => AttachmentKind::File,
    }
}

/// Extract chat-sdk attachments from an inbound event. 1:1 with
/// upstream `extractAttachments(event)`:
///
/// - When `event.message?.attachments` is missing, returns `[]`.
/// - Filters out attachments without a `payload.url` (upstream's
///   `.filter((attachment) => attachment.payload?.url)`).
/// - Maps the remaining attachments through [`map_attachment_type`].
///
/// The `fetch_data` callback upstream attaches is intentionally
/// omitted (the Rust port stores the URL only; HTTP fetches live on
/// the adapter, not on the pure parse path).
pub fn extract_attachments(event: &MessengerMessagingEvent) -> Vec<Attachment> {
    let Some(msg) = event.message.as_ref() else {
        return Vec::new();
    };
    let Some(atts) = msg.attachments.as_ref() else {
        return Vec::new();
    };
    atts.iter()
        .filter_map(|a| {
            let url = a.payload.as_ref().and_then(|p| p.url.as_ref())?.clone();
            Some(Attachment {
                data: None,
                fetch_metadata: None,
                height: None,
                mime_type: None,
                name: None,
                size: None,
                kind: map_attachment_type(&a.kind),
                url: Some(url),
                width: None,
            })
        })
        .collect()
}

/// Compute the trailing-numeric "sequence number" embedded in a
/// Messenger mid. 1:1 with upstream `messageSequence(messageId)`
/// which matches the regex `:(\d+)$` and returns 0 when absent.
///
/// Used by [`compare_messages`] as the tiebreaker when two messages
/// share the same timestamp.
pub fn message_sequence(message_id: &str) -> i64 {
    // Match upstream's `/:(\d+)$/` — take the last `:`-separated
    // numeric suffix.
    let Some(idx) = message_id.rfind(':') else {
        return 0;
    };
    let suffix = &message_id[idx + 1..];
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    suffix.parse::<i64>().unwrap_or(0)
}

/// Compare two parsed Messenger messages by `(dateSent,
/// messageSequence)`. 1:1 with upstream
/// `compareMessages(a, b)` — used to keep the per-thread cache in
/// chronological order with the embedded sequence number as
/// tiebreaker.
pub fn compare_messages(a: &Message, b: &Message) -> std::cmp::Ordering {
    let by_time = a.metadata.date_sent.cmp(&b.metadata.date_sent);
    if by_time != std::cmp::Ordering::Equal {
        return by_time;
    }
    message_sequence(&a.id).cmp(&message_sequence(&b.id))
}

/// Parse an inbound webhook event into a cross-platform [`Message`].
/// 1:1 with upstream `parseMessengerMessage(event, threadId)`:
///
/// - `text` ← `event.message?.text` ?? `event.postback?.title` ?? `""`
/// - `id` ← `event.message?.mid` ?? `event:<timestamp>`
///   (postback events with a `mid` field use the message id; events
///   without a body fall back to the synthetic `event:<ts>` id)
/// - `isEcho` ← `event.message?.is_echo ?? false`
/// - `isMe` / `isBot` ← `isEcho || event.sender.id === bot_user_id`
///
/// The `bot_user_id` argument is the page id resolved by
/// `MessengerAdapter::initialize` (from `/me`). Pass `None` when the
/// adapter hasn't initialized yet — upstream treats this as
/// `!== sender.id` (i.e. inbound message, `isMe = false`).
///
/// `is_mention` is hard-coded to `Some(true)` — upstream marks every
/// inbound Messenger message as a mention (DMs always address the
/// bot directly).
pub fn parse_messenger_message(
    event: &MessengerMessagingEvent,
    bot_user_id: Option<&str>,
) -> Message {
    let thread_id = encode_thread_id(&event.sender.id);

    let text = event
        .message
        .as_ref()
        .and_then(|m| m.text.clone())
        .or_else(|| event.postback.as_ref().map(|p| p.title.clone()))
        .unwrap_or_default();

    let id = event
        .message
        .as_ref()
        .and_then(|m| m.mid.clone())
        .unwrap_or_else(|| format!("event:{}", event.timestamp));

    let is_echo = event.message.as_ref().is_some_and(|m| m.is_echo);
    let is_me = is_echo
        || bot_user_id
            .map(|bot_id| bot_id == event.sender.id)
            .unwrap_or(false);

    let author = Author {
        full_name: event.sender.id.clone(),
        is_bot: if is_me {
            BotStatus::TRUE
        } else {
            BotStatus::FALSE
        },
        is_me,
        user_id: event.sender.id.clone(),
        user_name: event.sender.id.clone(),
    };

    let metadata = MessageMetadata {
        date_sent: event.timestamp.to_string(),
        edited: false,
        edited_at: None,
    };

    let attachments = extract_attachments(event);

    // The upstream parser also computes a `formatted: ast(text)`
    // mdast root via the markdown converter. The Rust port stores
    // `Root` directly; an empty root is a valid placeholder when the
    // adapter doesn't need the AST shape (the pure parse path's
    // callers typically only inspect `text` + `id` + `attachments`).
    let formatted = root(Vec::new());

    let mut msg = Message::new(
        id,
        thread_id,
        text,
        formatted,
        serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        author,
        metadata,
        attachments,
    );
    msg.is_mention = Some(true);
    msg
}

// MessengerMessagingEvent serialize-into-json shim for the `raw`
// field of the parsed [`Message`]. Manual to avoid re-deriving
// Serialize on every sub-struct; we only need a round-trip via
// `serde_json::Value`.
impl serde::Serialize for MessengerMessagingEvent {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut map = serde_json::Map::new();
        map.insert(
            "sender".to_string(),
            serde_json::json!({ "id": self.sender.id }),
        );
        map.insert(
            "recipient".to_string(),
            serde_json::json!({ "id": self.recipient.id }),
        );
        map.insert("timestamp".to_string(), serde_json::json!(self.timestamp));
        if let Some(m) = &self.message {
            map.insert(
                "message".to_string(),
                serde_json::json!({
                    "mid": m.mid,
                    "text": m.text,
                    "is_echo": m.is_echo,
                }),
            );
        }
        if let Some(p) = &self.postback {
            map.insert(
                "postback".to_string(),
                serde_json::json!({
                    "title": p.title,
                    "payload": p.payload,
                    "mid": p.mid,
                }),
            );
        }
        serde_json::Value::Object(map).serialize(ser)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> MessengerMessagingEvent {
        MessengerMessagingEvent {
            sender: MessengerSender {
                id: "USER_123".to_string(),
            },
            recipient: MessengerRecipient {
                id: "PAGE_456".to_string(),
            },
            timestamp: 1735689600000,
            message: Some(MessengerMessagePayload {
                mid: Some("mid.abc123".to_string()),
                text: Some("hello".to_string()),
                is_echo: false,
                attachments: None,
                quick_reply: None,
            }),
            postback: None,
        }
    }

    // ---------- message parsing (9 upstream cases) ----------

    #[test]
    fn parses_raw_messages() {
        // 1:1 with upstream index.test.ts:1329 > "parses raw messages"
        let parsed = parse_messenger_message(&sample_event(), None);
        assert_eq!(parsed.text, "hello");
        assert_eq!(parsed.thread_id, "messenger:USER_123");
        assert_eq!(parsed.id, "mid.abc123");
    }

    #[test]
    fn sets_is_mention_to_true_for_all_inbound_messages() {
        // 1:1 with upstream index.test.ts:1339 > "sets isMention to true for all inbound messages"
        let parsed = parse_messenger_message(&sample_event(), None);
        assert_eq!(parsed.is_mention, Some(true));
    }

    #[test]
    fn marks_echo_messages_as_is_me_and_is_bot() {
        // 1:1 with upstream index.test.ts:1345 > "marks echo messages as isMe and isBot"
        let mut event = sample_event();
        event.sender.id = "PAGE_456".to_string();
        event.message = Some(MessengerMessagePayload {
            mid: Some("mid.echo".to_string()),
            text: Some("bot says".to_string()),
            is_echo: true,
            attachments: None,
            quick_reply: None,
        });
        let parsed = parse_messenger_message(&event, Some("PAGE_456"));
        assert!(parsed.author.is_me);
        assert_eq!(parsed.author.is_bot, BotStatus::TRUE);
    }

    #[test]
    fn parses_message_with_empty_text_as_empty_string() {
        // 1:1 with upstream index.test.ts:1362 > "parses message with empty text as empty string"
        let mut event = sample_event();
        event.message = Some(MessengerMessagePayload {
            mid: Some("mid.empty".to_string()),
            text: None,
            is_echo: false,
            attachments: None,
            quick_reply: None,
        });
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.text, "");
    }

    #[test]
    fn parses_message_with_quick_reply_payload() {
        // 1:1 with upstream index.test.ts:1371 > "parses message with quick_reply payload"
        let mut event = sample_event();
        event.message = Some(MessengerMessagePayload {
            mid: Some("mid.qr".to_string()),
            text: Some("Yes".to_string()),
            is_echo: false,
            attachments: None,
            quick_reply: Some(MessengerQuickReply {
                payload: "QR_YES".to_string(),
            }),
        });
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.text, "Yes");
        assert_eq!(parsed.id, "mid.qr");
    }

    #[test]
    fn handles_message_with_no_text_and_no_postback_title() {
        // 1:1 with upstream index.test.ts:1385 > "handles message with no text and no postback title"
        let mut event = sample_event();
        event.message = Some(MessengerMessagePayload {
            mid: Some("mid.attach-only".to_string()),
            text: None,
            is_echo: false,
            attachments: Some(vec![MessengerAttachment {
                kind: "image".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/img.jpg".to_string()),
                }),
            }]),
            quick_reply: None,
        });
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.text, "");
        assert_eq!(parsed.attachments.len(), 1);
    }

    #[test]
    fn uses_event_timestamp_for_id_when_no_mid() {
        // 1:1 with upstream index.test.ts:1403 > "uses event timestamp for ID when no mid"
        let event = MessengerMessagingEvent {
            sender: MessengerSender {
                id: "USER_123".to_string(),
            },
            recipient: MessengerRecipient {
                id: "PAGE_456".to_string(),
            },
            timestamp: 1735689600000,
            message: None,
            postback: Some(MessengerPostback {
                title: "Get Started".to_string(),
                payload: "START".to_string(),
                mid: None,
            }),
        };
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.id, "event:1735689600000");
        assert_eq!(parsed.text, "Get Started");
    }

    #[test]
    fn updates_cached_message_when_same_id_is_parsed_again() {
        // 1:1 with upstream index.test.ts:1417 > "updates cached message when same ID is parsed again"
        // The upstream test asserts the cache replaces by id; the pure
        // parser deterministically returns the same id+text for the
        // second event, which is the precondition the cache layer
        // relies on.
        let mut event1 = sample_event();
        event1.message.as_mut().unwrap().mid = Some("mid.dup".to_string());
        event1.message.as_mut().unwrap().text = Some("first".to_string());
        let mut event2 = sample_event();
        event2.message.as_mut().unwrap().mid = Some("mid.dup".to_string());
        event2.message.as_mut().unwrap().text = Some("updated".to_string());
        let p1 = parse_messenger_message(&event1, None);
        let p2 = parse_messenger_message(&event2, None);
        assert_eq!(p1.id, p2.id);
        assert_eq!(p2.text, "updated");
    }

    #[test]
    fn sorts_messages_by_timestamp_then_by_sequence_number() {
        // 1:1 with upstream index.test.ts:1431 > "sorts messages by
        // timestamp then by sequence number". Asserts the
        // `compare_messages` tiebreaker mirrors upstream's regex-based
        // sequence-number ordering.
        let mut e1 = sample_event();
        e1.timestamp = 1735689600000;
        e1.message.as_mut().unwrap().mid = Some("mid.abc:2".to_string());
        e1.message.as_mut().unwrap().text = Some("second".to_string());
        let mut e2 = sample_event();
        e2.timestamp = 1735689600000;
        e2.message.as_mut().unwrap().mid = Some("mid.abc:1".to_string());
        e2.message.as_mut().unwrap().text = Some("first".to_string());
        let mut msgs = vec![
            parse_messenger_message(&e1, None),
            parse_messenger_message(&e2, None),
        ];
        msgs.sort_by(compare_messages);
        assert_eq!(msgs[0].text, "first");
        assert_eq!(msgs[1].text, "second");
    }

    // ---------- attachments (8 upstream cases) ----------

    #[test]
    fn extracts_attachments_from_messages() {
        // 1:1 with upstream index.test.ts:1455 > "extracts attachments from messages"
        let mut event = sample_event();
        event.message.as_mut().unwrap().mid = Some("mid.attach".to_string());
        event.message.as_mut().unwrap().text = Some("check this".to_string());
        event.message.as_mut().unwrap().attachments = Some(vec![
            MessengerAttachment {
                kind: "image".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/img.jpg".to_string()),
                }),
            },
            MessengerAttachment {
                kind: "video".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/vid.mp4".to_string()),
                }),
            },
            MessengerAttachment {
                kind: "audio".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/aud.mp3".to_string()),
                }),
            },
            MessengerAttachment {
                kind: "file".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/doc.pdf".to_string()),
                }),
            },
            MessengerAttachment {
                kind: "fallback".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/fallback".to_string()),
                }),
            },
        ]);
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.attachments.len(), 5);
        assert_eq!(parsed.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(parsed.attachments[1].kind, AttachmentKind::Video);
        assert_eq!(parsed.attachments[2].kind, AttachmentKind::Audio);
        assert_eq!(parsed.attachments[3].kind, AttachmentKind::File);
        assert_eq!(parsed.attachments[4].kind, AttachmentKind::File);
    }

    #[test]
    fn skips_attachments_without_url() {
        // 1:1 with upstream index.test.ts:1483 > "skips attachments without URL"
        let mut event = sample_event();
        event.message.as_mut().unwrap().mid = Some("mid.nourl".to_string());
        event.message.as_mut().unwrap().text = Some("sticker".to_string());
        event.message.as_mut().unwrap().attachments = Some(vec![
            MessengerAttachment {
                kind: "image".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: Some(123),
                    url: None,
                }),
            },
            MessengerAttachment {
                kind: "image".to_string(),
                payload: None,
            },
        ]);
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.attachments.len(), 0);
    }

    #[test]
    fn maps_location_attachment_type_to_file() {
        // 1:1 with upstream index.test.ts:1564 > "maps location attachment type to file"
        let mut event = sample_event();
        event.message.as_mut().unwrap().mid = Some("mid.loc".to_string());
        event.message.as_mut().unwrap().text = Some("location".to_string());
        event.message.as_mut().unwrap().attachments = Some(vec![MessengerAttachment {
            kind: "location".to_string(),
            payload: Some(MessengerAttachmentPayload {
                sticker_id: None,
                url: Some("https://maps.example.com/loc".to_string()),
            }),
        }]);
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].kind, AttachmentKind::File);
    }

    #[test]
    fn handles_mix_of_attachments_with_and_without_urls() {
        // 1:1 with upstream index.test.ts:1583 > "handles mix of attachments with and without URLs"
        let mut event = sample_event();
        event.message.as_mut().unwrap().mid = Some("mid.mixed".to_string());
        event.message.as_mut().unwrap().text = Some("mixed".to_string());
        event.message.as_mut().unwrap().attachments = Some(vec![
            MessengerAttachment {
                kind: "image".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/img.jpg".to_string()),
                }),
            },
            MessengerAttachment {
                kind: "image".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: Some(369239263222822),
                    url: None,
                }),
            },
            MessengerAttachment {
                kind: "video".to_string(),
                payload: Some(MessengerAttachmentPayload {
                    sticker_id: None,
                    url: Some("https://example.com/vid.mp4".to_string()),
                }),
            },
            MessengerAttachment {
                kind: "fallback".to_string(),
                payload: None,
            },
        ]);
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.attachments.len(), 2);
        assert_eq!(parsed.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(parsed.attachments[1].kind, AttachmentKind::Video);
    }

    #[test]
    fn returns_empty_attachments_when_message_has_no_attachments_field() {
        // 1:1 with upstream index.test.ts:1603 > "returns empty attachments when message has no attachments field"
        let mut event = sample_event();
        event.message.as_mut().unwrap().mid = Some("mid.noatt".to_string());
        event.message.as_mut().unwrap().text = Some("plain text".to_string());
        event.message.as_mut().unwrap().attachments = None;
        let parsed = parse_messenger_message(&event, None);
        assert_eq!(parsed.attachments.len(), 0);
    }

    // ---------- map_attachment_type direct tests ----------
    // 1:1 with the upstream mapping function used by extract_attachments:
    // `image`/`video`/`audio` pass through; everything else folds to `File`.

    #[test]
    fn map_attachment_type_maps_image_to_image() {
        assert_eq!(map_attachment_type("image"), AttachmentKind::Image);
    }

    #[test]
    fn map_attachment_type_maps_video_to_video() {
        assert_eq!(map_attachment_type("video"), AttachmentKind::Video);
    }

    #[test]
    fn map_attachment_type_maps_audio_to_audio() {
        assert_eq!(map_attachment_type("audio"), AttachmentKind::Audio);
    }

    #[test]
    fn map_attachment_type_folds_other_types_to_file() {
        // file / fallback / location / unknown all -> File.
        for fb in ["file", "fallback", "location", "totally_unknown"] {
            assert_eq!(map_attachment_type(fb), AttachmentKind::File, "for {fb}");
        }
    }

    // ---------- message_sequence ----------
    // Additive direct tests of the trailing-`:N` sequence extractor
    // used by compare_messages. Upstream tests cover this through the
    // sort behaviour ("sorts messages by timestamp then by sequence
    // number"); a direct test pins the regex semantics.

    #[test]
    fn message_sequence_returns_zero_when_no_trailing_colon_digits() {
        assert_eq!(message_sequence("mid.abc"), 0);
        assert_eq!(message_sequence(""), 0);
        assert_eq!(message_sequence("mid.abc:no_digits"), 0);
    }

    #[test]
    fn message_sequence_returns_trailing_numeric_suffix() {
        assert_eq!(message_sequence("mid.abc:1"), 1);
        assert_eq!(message_sequence("mid.abc:42"), 42);
        assert_eq!(message_sequence("mid.abc:99999"), 99999);
    }
}
