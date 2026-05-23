//! `Message` data struct with workflow-friendly serialization.
//!
//! 1:1 port (in progress) of `packages/chat/src/message.ts`.
//!
//! **What this slice ships:**
//!
//! - [`Message`] in-memory data struct mirroring upstream
//!   `class Message<TRawMessage = unknown>`.
//! - [`SerializedMessage`] wire shape that 1:1-matches upstream
//!   `interface SerializedMessage` (including the `_type:
//!   "chat:Message"` discriminator and ISO-string dates).
//! - [`MessageKind`] discriminator-tag enum so the `_type` field
//!   serializes as the literal `"chat:Message"`.
//! - Round-trippable [`Message::to_serialized`] / [`Message::from_serialized`]
//!   helpers (the Rust equivalent of upstream `toJSON` / `fromJSON`).
//! - Module-header coverage notes for the upstream test cases that
//!   require adapter-bound behavior (the async `subject` getter) or
//!   are JS-symbol-specific (`WORKFLOW_SERIALIZE` /
//!   `WORKFLOW_DESERIALIZE`).
//!
//! **What is deferred:**
//!
//! - `subject` async getter + `setMessageAdapter` — depend on the
//!   `Adapter::fetch_subject` method, which lives on the
//!   placeholder [`crate::types::Adapter`] trait. Will land once the
//!   trait is extended.
//! - `WORKFLOW_SERIALIZE` / `WORKFLOW_DESERIALIZE` Symbol methods are
//!   JavaScript-runtime-specific. The equivalent Rust functionality
//!   is just `serde_json::to_value(msg.to_serialized())`. Documented
//!   in the test module as js-only-adjacent.
//! - Buffer-data stripping inside `to_serialized` — the Rust port
//!   already represents `Attachment.data` as `Option<FileBytes>` with
//!   `skip_serializing_if = "Option::is_none"`; consumers can set it
//!   to `None` before serializing. Documented in the module header
//!   so the upstream behavior is preserved.

use serde::{Deserialize, Serialize};

use crate::markdown::Root;
use crate::types::{Attachment, Author, LinkPreview, MessageMetadata};

/// Discriminator tag for [`SerializedMessage`]. Single-variant
/// unit-like enum so serde emits the upstream literal
/// `"chat:Message"` for the `_type` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MessageKind {
    /// The only legal value: `"chat:Message"`.
    #[default]
    #[serde(rename = "chat:Message")]
    Message,
}

/// In-memory representation of a chat message. 1:1 port of upstream
/// `class Message<TRawMessage = unknown>`.
///
/// `raw` carries the platform-specific raw payload (escape hatch).
/// Upstream is generic over the raw shape; the Rust port uses
/// `serde_json::Value` to preserve any JSON shape without forcing a
/// generic parameter through every callsite (an adapter-specific
/// typed alias can wrap this if desired).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Stable message ID.
    pub id: String,
    /// Thread the message belongs to.
    #[serde(rename = "threadId")]
    pub thread_id: String,
    /// Plain text content (all formatting stripped).
    pub text: String,
    /// Structured formatting as an mdast Root node.
    pub formatted: Root,
    /// Platform-specific raw payload.
    pub raw: serde_json::Value,
    /// Message author.
    pub author: Author,
    /// Message metadata (date sent, edited flag, …).
    pub metadata: MessageMetadata,
    /// File / image / video / audio attachments.
    pub attachments: Vec<Attachment>,
    /// Whether the bot is @-mentioned in this message.
    #[serde(rename = "isMention", default, skip_serializing_if = "Option::is_none")]
    pub is_mention: Option<bool>,
    /// Cross-platform user key for the author (resolved by chat::identity).
    #[serde(rename = "userKey", default, skip_serializing_if = "Option::is_none")]
    pub user_key: Option<String>,
    /// Links found in the message (default empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkPreview>,
}

impl Message {
    /// Construct a message. 1:1 port of upstream `new Message(data)`.
    /// Mirrors the upstream constructor: `links` defaults to an empty
    /// vector when omitted; `is_mention` flows through untouched.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        thread_id: impl Into<String>,
        text: impl Into<String>,
        formatted: Root,
        raw: serde_json::Value,
        author: Author,
        metadata: MessageMetadata,
        attachments: Vec<Attachment>,
    ) -> Self {
        Self {
            id: id.into(),
            thread_id: thread_id.into(),
            text: text.into(),
            formatted,
            raw,
            author,
            metadata,
            attachments,
            is_mention: None,
            user_key: None,
            links: Vec::new(),
        }
    }

    /// Convert to a wire-shape [`SerializedMessage`]. 1:1 port of
    /// upstream `Message.toJSON(): SerializedMessage`.
    ///
    /// **Buffer-stripping:** Upstream's `toJSON` strips `data` and
    /// `fetchData` from every attachment. The Rust [`Attachment`]
    /// already serializes `data` only when present (the
    /// `skip_serializing_if` attribute handles the absent case). When
    /// callers want to mirror upstream's "always strip binary data"
    /// behavior, they can map `attachments` through
    /// [`Attachment::without_inline_data`] before calling this method
    /// (added in a follow-up slice). The native Rust `Attachment`
    /// surface does not carry a `fetchData` callback, so the strip is
    /// already a no-op for that field.
    pub fn to_serialized(&self) -> SerializedMessage {
        SerializedMessage {
            kind: MessageKind::Message,
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            text: self.text.clone(),
            formatted: self.formatted.clone(),
            raw: self.raw.clone(),
            author: self.author.clone(),
            metadata: self.metadata.clone(),
            attachments: self.attachments.clone(),
            is_mention: self.is_mention,
            links: if self.links.is_empty() {
                None
            } else {
                Some(self.links.clone())
            },
        }
    }

    /// Convert to a wire-shape [`SerializedMessage`] with inline
    /// attachment payloads stripped. 1:1 with upstream
    /// `Message.toJSON()`'s `data` / `fetchData` strip behavior. Use
    /// this when emitting transcripts or workflow snapshots that must
    /// not embed raw binary bytes (the receiver rehydrates via the
    /// adapter's media-fetch path).
    pub fn to_serialized_stripped(&self) -> SerializedMessage {
        let mut serialized = self.to_serialized();
        serialized.attachments = serialized
            .attachments
            .iter()
            .map(Attachment::without_inline_data)
            .collect();
        serialized
    }

    /// Reconstruct a [`Message`] from a [`SerializedMessage`]. 1:1
    /// port of upstream `Message.fromJSON(json): Message`.
    pub fn from_serialized(serialized: SerializedMessage) -> Self {
        Self {
            id: serialized.id,
            thread_id: serialized.thread_id,
            text: serialized.text,
            formatted: serialized.formatted,
            raw: serialized.raw,
            author: serialized.author,
            metadata: serialized.metadata,
            attachments: serialized.attachments,
            is_mention: serialized.is_mention,
            user_key: None,
            links: serialized.links.unwrap_or_default(),
        }
    }
}

/// Wire shape for [`Message`]. 1:1 port of upstream
/// `interface SerializedMessage`. The `_type` discriminator is
/// represented via [`MessageKind`] so serde emits the upstream literal
/// `"chat:Message"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerializedMessage {
    /// Discriminator. Always `"chat:Message"` on the wire.
    #[serde(rename = "_type")]
    pub kind: MessageKind,
    /// Stable message ID.
    pub id: String,
    /// Thread the message belongs to.
    #[serde(rename = "threadId")]
    pub thread_id: String,
    /// Plain text content.
    pub text: String,
    /// mdast Root for the formatted body.
    pub formatted: Root,
    /// Platform-specific raw payload.
    pub raw: serde_json::Value,
    /// Author.
    pub author: Author,
    /// Metadata.
    pub metadata: MessageMetadata,
    /// Attachments. The wire representation omits `data` /
    /// `fetchData` automatically via `skip_serializing_if`.
    pub attachments: Vec<Attachment>,
    /// `@-mention` flag, when present.
    #[serde(rename = "isMention", default, skip_serializing_if = "Option::is_none")]
    pub is_mention: Option<bool>,
    /// Links, only emitted when non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<LinkPreview>>,
}

#[cfg(test)]
mod tests {
    //! Subset port of `packages/chat/src/message.test.ts`.
    //!
    //! **Cases ported (12 of 19 upstream cases):**
    //!
    //! - Constructor: both `constructor` cases (basic + isMention).
    //! - `toJSON`: type-tag literal, ISO date round-trip, fetchMetadata
    //!   passthrough, isMention flag.
    //! - `toJSON` buffer strip: drops inline `data`, leaves
    //!   `fetch_metadata` / URL / mime type intact, no-op when `data`
    //!   already absent (the `to_serialized_stripped` helper).
    //! - `fromJSON`: present + absent `editedAt`.
    //! - Round-trip: preserves all fields.
    //!
    //! **Cases deferred (7 of 19 upstream cases):**
    //!
    //! - The 5 `subject` getter cases (`should return null when no
    //!   adapter is set`, `... when adapter has no fetchSubject`, `...
    //!   from adapter`, `... cache the result`, `... cache null
    //!   result`, `... concurrent access`) require the `Adapter` trait
    //!   to expose an async `fetch_subject` method and the
    //!   `setMessageAdapter` WeakMap; both land once the
    //!   `crate::types::Adapter` trait is extended.
    //! - The 2 `WORKFLOW_SERIALIZE` / `WORKFLOW_DESERIALIZE` cases use
    //!   a JS Symbol-key static method that has no Rust analogue
    //!   (Rust uses serde directly).
    use super::*;
    use crate::markdown::root;
    use crate::types::{Attachment, AttachmentKind, Author, FileBytes, LinkPreview};
    use serde_json::json;

    fn sample_author() -> Author {
        use crate::types::BotStatus;
        Author {
            user_id: "U123".to_string(),
            user_name: "testuser".to_string(),
            full_name: "Test User".to_string(),
            is_bot: BotStatus::Known(false),
            is_me: false,
        }
    }

    fn sample_metadata() -> MessageMetadata {
        MessageMetadata {
            date_sent: "2024-01-15T10:30:00.000Z".to_string(),
            edited: false,
            edited_at: None,
        }
    }

    fn sample_message() -> Message {
        Message::new(
            "msg-1",
            "slack:C123:1234.5678",
            "Hello world",
            root(vec![]),
            json!({"platform": "test"}),
            sample_author(),
            sample_metadata(),
            vec![],
        )
    }

    // ---------- constructor ----------

    #[test]
    fn constructor_assigns_all_properties() {
        let msg = sample_message();
        assert_eq!(msg.id, "msg-1");
        assert_eq!(msg.thread_id, "slack:C123:1234.5678");
        assert_eq!(msg.text, "Hello world");
        assert_eq!(msg.author.user_name, "testuser");
        assert_eq!(msg.metadata.date_sent, "2024-01-15T10:30:00.000Z");
        assert!(msg.attachments.is_empty());
        assert!(msg.is_mention.is_none());
    }

    #[test]
    fn constructor_assigns_is_mention_when_provided() {
        let mut msg = sample_message();
        msg.is_mention = Some(true);
        assert_eq!(msg.is_mention, Some(true));
    }

    // ---------- to_serialized ----------

    #[test]
    fn to_serialized_produces_correct_type_tag() {
        let serialized = sample_message().to_serialized();
        assert_eq!(
            serde_json::to_value(&serialized.kind).unwrap(),
            json!("chat:Message")
        );
    }

    #[test]
    fn to_serialized_preserves_iso_date_strings() {
        let mut msg = sample_message();
        msg.metadata = MessageMetadata {
            date_sent: "2024-06-01T12:00:00.000Z".to_string(),
            edited: true,
            edited_at: Some("2024-06-01T13:00:00.000Z".to_string()),
        };
        let serialized = msg.to_serialized();
        assert_eq!(serialized.metadata.date_sent, "2024-06-01T12:00:00.000Z");
        assert_eq!(
            serialized.metadata.edited_at.as_deref(),
            Some("2024-06-01T13:00:00.000Z")
        );
    }

    #[test]
    fn to_serialized_preserves_fetch_metadata_in_attachments() {
        let mut msg = sample_message();
        msg.attachments = vec![Attachment {
            data: Some(FileBytes::from(b"binary".to_vec())),
            fetch_metadata: Some(std::collections::HashMap::from([
                ("mediaId".to_string(), "123".to_string()),
                ("url".to_string(), "https://example.com/img.png".to_string()),
            ])),
            height: None,
            mime_type: None,
            name: Some("img.png".to_string()),
            size: None,
            kind: AttachmentKind::Image,
            url: Some("https://example.com/img.png".to_string()),
            width: None,
        }];
        let json = serde_json::to_value(msg.to_serialized()).unwrap();
        let fetch_metadata = json["attachments"][0]["fetchMetadata"].clone();
        assert_eq!(fetch_metadata["mediaId"], "123");
        let restored: SerializedMessage = serde_json::from_value(json).unwrap();
        let restored_msg = Message::from_serialized(restored);
        let fm = restored_msg.attachments[0].fetch_metadata.as_ref().unwrap();
        assert_eq!(fm["mediaId"], "123");
        assert_eq!(fm["url"], "https://example.com/img.png");
    }

    #[test]
    fn to_serialized_includes_is_mention_flag() {
        let mut msg = sample_message();
        msg.is_mention = Some(true);
        let serialized = msg.to_serialized();
        assert_eq!(serialized.is_mention, Some(true));
    }

    // ---------- from_serialized ----------

    #[test]
    fn from_serialized_preserves_metadata_dates() {
        let serialized = SerializedMessage {
            kind: MessageKind::Message,
            id: "msg-2".to_string(),
            thread_id: "teams:ch:th".to_string(),
            text: "hi".to_string(),
            formatted: root(vec![]),
            raw: json!({}),
            author: sample_author(),
            metadata: MessageMetadata {
                date_sent: "2024-03-01T00:00:00.000Z".to_string(),
                edited: true,
                edited_at: Some("2024-03-01T01:00:00.000Z".to_string()),
            },
            attachments: vec![],
            is_mention: None,
            links: None,
        };
        let msg = Message::from_serialized(serialized);
        assert_eq!(msg.metadata.date_sent, "2024-03-01T00:00:00.000Z");
        assert_eq!(
            msg.metadata.edited_at.as_deref(),
            Some("2024-03-01T01:00:00.000Z")
        );
    }

    #[test]
    fn from_serialized_handles_missing_edited_at() {
        let serialized = SerializedMessage {
            kind: MessageKind::Message,
            id: "msg-3".to_string(),
            thread_id: "t".to_string(),
            text: "t".to_string(),
            formatted: root(vec![]),
            raw: json!({}),
            author: sample_author(),
            metadata: MessageMetadata {
                date_sent: "2024-01-01T00:00:00.000Z".to_string(),
                edited: false,
                edited_at: None,
            },
            attachments: vec![],
            is_mention: None,
            links: None,
        };
        let msg = Message::from_serialized(serialized);
        assert!(msg.metadata.edited_at.is_none());
    }

    // ---------- round-trip ----------

    #[test]
    fn round_trip_preserves_all_fields() {
        let mut msg = sample_message();
        msg.is_mention = Some(true);
        msg.metadata = MessageMetadata {
            date_sent: "2024-01-15T10:30:00.000Z".to_string(),
            edited: true,
            edited_at: Some("2024-01-15T11:00:00.000Z".to_string()),
        };
        msg.attachments = vec![Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: None,
            name: Some("f.pdf".to_string()),
            size: None,
            kind: AttachmentKind::File,
            url: Some("https://example.com/f.pdf".to_string()),
            width: None,
        }];
        msg.links = vec![LinkPreview {
            url: "https://example.com".to_string(),
            description: None,
            image_url: None,
            site_name: None,
            title: None,
        }];

        let serialized = msg.to_serialized();
        let restored = Message::from_serialized(serialized);
        assert_eq!(restored.id, msg.id);
        assert_eq!(restored.text, msg.text);
        assert_eq!(restored.is_mention, msg.is_mention);
        assert_eq!(restored.metadata.date_sent, msg.metadata.date_sent);
        assert_eq!(restored.attachments[0].url, msg.attachments[0].url);
        assert_eq!(restored.links[0].url, "https://example.com");
    }

    // ---------- buffer-strip (upstream toJSON data/fetchData strip) ----------

    #[test]
    fn to_serialized_stripped_drops_inline_attachment_data() {
        let mut msg = sample_message();
        msg.attachments = vec![Attachment {
            data: Some(FileBytes::from(b"binary-bytes".to_vec())),
            fetch_metadata: Some(std::collections::HashMap::from([(
                "mediaId".to_string(),
                "123".to_string(),
            )])),
            height: None,
            mime_type: Some("image/png".to_string()),
            name: Some("img.png".to_string()),
            size: Some(12),
            kind: AttachmentKind::Image,
            url: Some("https://example.com/img.png".to_string()),
            width: None,
        }];
        let stripped = msg.to_serialized_stripped();
        assert!(stripped.attachments[0].data.is_none());
        // Every other field still flows through so the receiver can
        // rehydrate via the adapter's media-fetch path.
        assert_eq!(
            stripped.attachments[0].url.as_deref(),
            Some("https://example.com/img.png")
        );
        assert_eq!(
            stripped.attachments[0].mime_type.as_deref(),
            Some("image/png")
        );
        assert_eq!(stripped.attachments[0].size, Some(12));
        let fm = stripped.attachments[0].fetch_metadata.as_ref().unwrap();
        assert_eq!(fm.get("mediaId").map(String::as_str), Some("123"));
    }

    #[test]
    fn to_serialized_stripped_leaves_attachments_without_data_unchanged() {
        let mut msg = sample_message();
        msg.attachments = vec![Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: None,
            name: Some("f.pdf".to_string()),
            size: None,
            kind: AttachmentKind::File,
            url: Some("https://example.com/f.pdf".to_string()),
            width: None,
        }];
        let stripped = msg.to_serialized_stripped();
        assert!(stripped.attachments[0].data.is_none());
        assert_eq!(stripped.attachments[0].name.as_deref(), Some("f.pdf"));
        assert_eq!(
            stripped.attachments[0].url.as_deref(),
            Some("https://example.com/f.pdf")
        );
    }

    #[test]
    fn full_json_round_trip_preserves_fetch_metadata() {
        let mut msg = sample_message();
        msg.attachments = vec![Attachment {
            data: None,
            fetch_metadata: Some(std::collections::HashMap::from([(
                "mediaId".to_string(),
                "123".to_string(),
            )])),
            height: None,
            mime_type: None,
            name: None,
            size: None,
            kind: AttachmentKind::Image,
            url: Some("https://example.com/img.png".to_string()),
            width: None,
        }];
        let json_value = serde_json::to_value(msg.to_serialized()).unwrap();
        let json_string = serde_json::to_string(&json_value).unwrap();
        let parsed: SerializedMessage = serde_json::from_str(&json_string).unwrap();
        let restored = Message::from_serialized(parsed);
        assert_eq!(
            restored.attachments[0]
                .fetch_metadata
                .as_ref()
                .unwrap()
                .get("mediaId")
                .map(String::as_str),
            Some("123")
        );
    }
}
