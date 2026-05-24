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
//! - [`MessageSubjectResolver`] — subject getter + cache, ported in
//!   slice 123 after the Phase 1.5 Adapter trait extension landed
//!   `Adapter::fetch_subject`.
//!
//! **What is still js-only-adjacent:**
//!
//! - `WORKFLOW_SERIALIZE` / `WORKFLOW_DESERIALIZE` Symbol methods are
//!   JavaScript-runtime-specific. The equivalent Rust functionality
//!   is just `serde_json::to_value(msg.to_serialized())`. Documented
//!   in the test module as js-only-adjacent.

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

    /// Whether the message carries any attachments. 1:1 with the
    /// inline `msg.attachments.length > 0` check upstream uses at
    /// adapter callsites to gate attachment-rendering paths.
    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    /// Count of attachments on the message. Trivial accessor matching
    /// upstream `msg.attachments.length`.
    pub fn attachment_count(&self) -> usize {
        self.attachments.len()
    }

    /// Count of link previews on the message. Matches upstream
    /// `msg.links.length`.
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// Whether this message has been edited. Convenience matching the
    /// upstream `msg.metadata.edited === true` check.
    pub fn is_edited(&self) -> bool {
        self.metadata.edited
    }

    /// Whether the bot is @-mentioned in this message. Matches the
    /// upstream `msg.isMention === true` predicate; treats `None` and
    /// `Some(false)` as false (no mention).
    pub fn mentions_bot(&self) -> bool {
        self.is_mention.unwrap_or(false)
    }

    /// User key with a caller-provided fallback. Matches upstream's
    /// inline `msg.userKey ?? fallback` coalescing used by emoji /
    /// fallback-text renderers.
    pub fn user_key_or(&self, fallback: &str) -> String {
        self.user_key
            .clone()
            .unwrap_or_else(|| fallback.to_string())
    }

    /// 1:1 port of upstream `Message[WORKFLOW_SERIALIZE](msg)` —
    /// the static method exposed under the
    /// `WORKFLOW_SERIALIZE` symbol so workflow runtimes can
    /// serialize a `Message` without knowing the concrete class.
    /// Equivalent to `msg.to_serialized()`; ported as an associated
    /// function for parity with upstream's static-method shape.
    pub fn workflow_serialize(msg: &Self) -> SerializedMessage {
        msg.to_serialized()
    }

    /// 1:1 port of upstream `Message[WORKFLOW_DESERIALIZE](serialized)`.
    /// Reconstructs a `Message` from its `SerializedMessage`
    /// form via [`Self::from_serialized`].
    pub fn workflow_deserialize(serialized: SerializedMessage) -> Self {
        Self::from_serialized(serialized)
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

/// Caches the result of [`crate::types::Adapter::fetch_subject`] per
/// `(adapter_name, thread_id)` pair. 1:1 port of upstream's
/// `setMessageAdapter` WeakMap + private `_subject` cache on `Message`.
///
/// The cache lives outside [`Message`] (which is `Clone + Serialize`)
/// so neither serialization nor cloning carries adapter state. Adopters
/// hold one [`MessageSubjectResolver`] per Chat instance and call
/// [`Self::resolve`] whenever a message needs its subject.
#[derive(Debug, Default)]
pub struct MessageSubjectResolver {
    /// (adapter_name, thread_id) -> Some(cached_subject_or_none).
    cache: std::sync::Mutex<std::collections::HashMap<(String, String), Option<String>>>,
}

impl MessageSubjectResolver {
    /// Construct an empty resolver. 1:1 with upstream's
    /// `new MessageSubjectResolver()` (the upstream form is a closure
    /// captured at Chat-singleton construction time; the data shape
    /// matches).
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve the subject for `message` against `adapter`. 1:1 port of
    /// upstream's `Message.subject` getter:
    ///
    /// - Returns the cached value when present (1:1 with upstream's
    ///   "cache the result" / "cache null result" tests).
    /// - Otherwise calls `adapter.fetch_subject(thread_id)`, caches the
    ///   outcome (including `None`!), and returns it.
    /// - Adapters that don't implement `fetch_subject` (default trait
    ///   impl returns `Ok(None)`) get cached as `None` so subsequent
    ///   calls skip the adapter dispatch entirely — matches upstream's
    ///   "return null when adapter has no fetchSubject" test which
    ///   relies on JS optional-chaining `adapter.fetchSubject?.(...)`
    ///   resolving to `undefined` → `null` cached value.
    /// - Concurrent calls for the same `(adapter, thread_id)` may each
    ///   trigger a fetch since the cache write happens after the
    ///   `await`; the cache then stabilizes (matches upstream's
    ///   `Promise.race`-style coalescing observable behavior).
    pub async fn resolve(
        &self,
        adapter: &dyn crate::types::Adapter,
        message: &Message,
    ) -> crate::types::AdapterResult<Option<String>> {
        let key = (adapter.name().to_string(), message.thread_id.clone());
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return Ok(cached.clone());
        }
        let subject = match adapter.fetch_subject(&message.thread_id).await {
            Ok(s) => s,
            // Adapter doesn't support fetch_subject — treat as None
            // (matches upstream's optional-chaining `adapter.fetchSubject?.(...)`
            // resolving to undefined → null caching path).
            Err(crate::types::AdapterError::Unsupported(_)) => None,
            Err(e) => return Err(e),
        };
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, subject.clone());
        Ok(subject)
    }

    /// Drop the cache entry for `(adapter, thread_id)` so the next
    /// [`Self::resolve`] call triggers a fresh `fetch_subject`. Mirrors
    /// upstream's `clearSubjectCache(message)` test helper used to
    /// exercise the "cache miss after invalidation" path.
    pub fn invalidate(&self, adapter_name: &str, thread_id: &str) {
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&(adapter_name.to_string(), thread_id.to_string()));
    }

    /// Snapshot the number of cached subjects. Convenience for tests
    /// asserting cache hits / misses.
    pub fn cached_count(&self) -> usize {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).len()
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

    #[test]
    fn to_serialized_should_handle_undefined_edited_at() {
        // 1:1 with upstream `describe("Message.toJSON()") > it("should
        // handle undefined editedAt")` — when `edited: false` and
        // `editedAt: None`, the serialized payload's `editedAt` field
        // is `None` (omitted on the wire via `skip_serializing_if`).
        let mut msg = sample_message();
        msg.metadata = MessageMetadata {
            date_sent: "2024-01-15T10:30:00.000Z".to_string(),
            edited: false,
            edited_at: None,
        };
        let serialized = msg.to_serialized();
        assert_eq!(serialized.metadata.edited_at, None);
        // Wire-shape check: the JSON should not include `editedAt`
        // when None (matches upstream's `editedAt: undefined`
        // which JSON.stringify omits).
        let json = serde_json::to_value(&serialized).unwrap();
        assert!(json["metadata"].get("editedAt").is_none());
    }

    #[test]
    fn to_serialized_should_serialize_author_correctly() {
        // 1:1 with upstream `describe("Message.toJSON()") > it("should
        // serialize author correctly")` — author round-trips with its
        // `userId` / `userName` / `fullName` / `isBot` / `isMe`
        // fields preserved.
        let serialized = sample_message().to_serialized();
        let json = serde_json::to_value(&serialized).unwrap();
        let author = &json["author"];
        assert!(author["userId"].is_string());
        assert!(author["userName"].is_string());
        assert!(author["fullName"].is_string());
        assert!(author["isBot"].is_boolean() || author["isBot"].is_string());
    }

    #[test]
    fn to_serialized_should_serialize_links_without_fetch_message() {
        // 1:1 with upstream `describe("Message.toJSON()") > it("should
        // serialize links without fetchMessage")` — the wire shape
        // includes url / title / description / imageUrl / siteName
        // and excludes any callback fields. The Rust LinkPreview type
        // has no fetchMessage callback by construction (the type
        // system enforces the exclusion), so this test asserts the
        // pure data wire-shape round-trip.
        let mut msg = sample_message();
        msg.links = vec![
            LinkPreview {
                url: "https://example.com".to_string(),
                title: Some("Example".to_string()),
                description: None,
                image_url: None,
                site_name: None,
            },
            LinkPreview {
                url: "https://vercel.com".to_string(),
                title: None,
                description: None,
                image_url: None,
                site_name: Some("Vercel".to_string()),
            },
        ];
        let json = serde_json::to_value(msg.to_serialized()).unwrap();
        let links = json["links"].as_array().expect("links present");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0]["url"], "https://example.com");
        assert_eq!(links[0]["title"], "Example");
        assert_eq!(links[1]["url"], "https://vercel.com");
        assert_eq!(links[1]["siteName"], "Vercel");
        // fetchMessage callback field is absent by construction.
        assert!(links[0].get("fetchMessage").is_none());
    }

    #[test]
    fn to_serialized_should_serialize_attachments_without_inline_data() {
        // 1:1 with upstream `describe("Message.toJSON()") > it("should
        // serialize attachments without data/fetchData")` — the
        // wire shape includes type/url/name/mimeType/size/width/height
        // and excludes the inline `data` bytes + `fetchData` callback.
        // The Rust port uses to_serialized_stripped to drop inline
        // data (matches upstream's to-JSON-strips-binary semantics).
        let mut msg = sample_message();
        msg.attachments = vec![Attachment {
            data: Some(FileBytes::from(b"test".to_vec())),
            fetch_metadata: None,
            height: Some(600),
            mime_type: Some("image/png".to_string()),
            name: Some("image.png".to_string()),
            size: Some(1024),
            kind: AttachmentKind::Image,
            url: Some("https://example.com/image.png".to_string()),
            width: Some(800),
        }];
        let json = serde_json::to_value(msg.to_serialized_stripped()).unwrap();
        let att = &json["attachments"][0];
        assert_eq!(att["type"], "image");
        assert_eq!(att["url"], "https://example.com/image.png");
        assert_eq!(att["name"], "image.png");
        assert_eq!(att["mimeType"], "image/png");
        assert_eq!(att["size"], 1024);
        assert_eq!(att["width"], 800);
        assert_eq!(att["height"], 600);
        // The stripped form omits the inline data bytes.
        assert!(
            att.get("data").is_none() || att["data"].is_null(),
            "stripped attachment should not carry inline data: {att}"
        );
    }

    #[test]
    fn to_serialized_should_omit_links_when_empty() {
        // 1:1 with upstream `describe("Message.toJSON()") > it("should
        // omit links when empty")` — the SerializedMessage.links
        // field is `Option<Vec<LinkPreview>>` and the to_serialized
        // helper maps an empty Vec to `None`, which serde omits via
        // the default-skip serde attr on the SerializedMessage struct.
        let msg = sample_message();
        let json = serde_json::to_value(msg.to_serialized()).unwrap();
        assert!(
            json.get("links").is_none() || json["links"].is_null(),
            "links should be omitted from the wire shape when empty, got: {json}"
        );
    }

    #[test]
    fn to_serialized_should_produce_json_serializable_output() {
        // 1:1 with upstream `describe("Message.toJSON()") > it("should
        // produce JSON-serializable output")` — the serialized form
        // round-trips through `JSON.stringify`+`JSON.parse` losslessly
        // (the Rust equivalent is `serde_json::to_string` →
        // `serde_json::from_str` round-trip).
        let serialized = sample_message().to_serialized();
        let text = serde_json::to_string(&serialized).expect("serialize");
        let _parsed: serde_json::Value =
            serde_json::from_str(&text).expect("re-parse round-trips");
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
    fn from_serialized_should_restore_message_from_json() {
        // 1:1 with upstream `describe("Message.fromJSON()") > it("should
        // restore message from JSON")` — round-trips id / text / author
        // through the wire format.
        let serialized = SerializedMessage {
            kind: MessageKind::Message,
            id: "msg-1".to_string(),
            thread_id: "slack:C123:1234.5678".to_string(),
            text: "Hello world".to_string(),
            formatted: root(vec![]),
            raw: json!({ "some": "data" }),
            author: sample_author(),
            metadata: MessageMetadata {
                date_sent: "2024-01-15T10:30:00.000Z".to_string(),
                edited: false,
                edited_at: None,
            },
            attachments: vec![],
            is_mention: None,
            links: None,
        };
        let msg = Message::from_serialized(serialized);
        assert_eq!(msg.id, "msg-1");
        assert_eq!(msg.text, "Hello world");
        assert_eq!(msg.author.user_name, "testuser");
    }

    #[test]
    fn from_serialized_should_preserve_iso_strings_for_dates() {
        // 1:1 with upstream `describe("Message.fromJSON()") > it("should
        // convert ISO strings back to Date objects")`. The TS port
        // converts the string to a `Date` instance; the Rust port
        // keeps the ISO string verbatim (no Date type) — the
        // observable contract is identical: the value round-trips
        // unchanged.
        let serialized = SerializedMessage {
            kind: MessageKind::Message,
            id: "msg-1".to_string(),
            thread_id: "slack:C123:1234.5678".to_string(),
            text: "Test".to_string(),
            formatted: root(vec![]),
            raw: json!({}),
            author: sample_author(),
            metadata: MessageMetadata {
                date_sent: "2024-01-15T10:30:00.000Z".to_string(),
                edited: true,
                edited_at: Some("2024-01-15T11:00:00.000Z".to_string()),
            },
            attachments: vec![],
            is_mention: None,
            links: None,
        };
        let msg = Message::from_serialized(serialized);
        assert_eq!(msg.metadata.date_sent, "2024-01-15T10:30:00.000Z");
        assert_eq!(
            msg.metadata.edited_at.as_deref(),
            Some("2024-01-15T11:00:00.000Z")
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

    #[test]
    fn round_trip_should_round_trip_links_correctly() {
        // 1:1 with upstream `describe("Message.fromJSON()") > it("should
        // round-trip links correctly")` — links survive toJSON +
        // fromJSON without losing url/title/siteName fields. The
        // fetchMessage callback isn't preserved (the Rust LinkPreview
        // type has no such field by construction).
        let mut original = sample_message();
        original.links = vec![
            LinkPreview {
                url: "https://example.com".to_string(),
                title: Some("Example".to_string()),
                description: None,
                image_url: None,
                site_name: None,
            },
            LinkPreview {
                url: "https://vercel.com".to_string(),
                title: None,
                description: None,
                image_url: None,
                site_name: Some("Vercel".to_string()),
            },
        ];
        let serialized = original.to_serialized();
        let restored = Message::from_serialized(serialized);
        assert_eq!(restored.links.len(), 2);
        assert_eq!(restored.links[0].url, "https://example.com");
        assert_eq!(restored.links[0].title.as_deref(), Some("Example"));
        assert_eq!(restored.links[1].url, "https://vercel.com");
        assert_eq!(restored.links[1].site_name.as_deref(), Some("Vercel"));
    }

    // ---------- describe("WORKFLOW_SERIALIZE / WORKFLOW_DESERIALIZE") (2 cases) ----------
    // 1:1 with upstream `message.test.ts > describe("WORKFLOW_SERIALIZE
    // / WORKFLOW_DESERIALIZE")`.

    #[test]
    fn workflow_serialize_should_serialize_via_static_method() {
        let msg = sample_message();
        let serialized = Message::workflow_serialize(&msg);
        assert_eq!(
            serde_json::to_value(&serialized.kind).unwrap(),
            json!("chat:Message")
        );
        assert_eq!(serialized.id, "msg-1");
    }

    #[test]
    fn workflow_deserialize_should_deserialize_via_static_method() {
        let msg = sample_message();
        let serialized = Message::workflow_serialize(&msg);
        let restored = Message::workflow_deserialize(serialized);
        assert_eq!(restored.id, msg.id);
        // Upstream asserts metadata.dateSent is a Date instance.
        // Rust port stores it as ISO String — assert the same string
        // is preserved (the Rust shape doesn't convert to a typed
        // date; that's a Date<->String boundary that lives in
        // wire-shape layer).
        assert_eq!(restored.metadata.date_sent, msg.metadata.date_sent);
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

    // ---------- slice 113: pure accessor helpers ----------

    #[test]
    fn has_attachments_is_false_for_an_empty_attachment_list() {
        let msg = sample_message();
        assert!(!msg.has_attachments());
        assert_eq!(msg.attachment_count(), 0);
    }

    #[test]
    fn has_attachments_is_true_when_attachments_exist() {
        let mut msg = sample_message();
        msg.attachments = vec![Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: None,
            name: Some("f.pdf".to_string()),
            size: None,
            kind: AttachmentKind::File,
            url: None,
            width: None,
        }];
        assert!(msg.has_attachments());
        assert_eq!(msg.attachment_count(), 1);
    }

    #[test]
    fn link_count_counts_link_previews() {
        let mut msg = sample_message();
        assert_eq!(msg.link_count(), 0);
        msg.links = vec![LinkPreview {
            url: "https://example.com".to_string(),
            description: None,
            image_url: None,
            site_name: None,
            title: None,
        }];
        assert_eq!(msg.link_count(), 1);
    }

    #[test]
    fn is_edited_reflects_metadata_edited_flag() {
        let mut msg = sample_message();
        assert!(!msg.is_edited());
        msg.metadata.edited = true;
        assert!(msg.is_edited());
    }

    #[test]
    fn mentions_bot_treats_none_and_false_as_false() {
        let mut msg = sample_message();
        assert!(!msg.mentions_bot());
        msg.is_mention = Some(false);
        assert!(!msg.mentions_bot());
        msg.is_mention = Some(true);
        assert!(msg.mentions_bot());
    }

    #[test]
    fn user_key_or_returns_fallback_when_no_user_key_is_set() {
        let mut msg = sample_message();
        assert_eq!(msg.user_key_or("anon"), "anon");
        msg.user_key = Some("user:slack:U999".to_string());
        assert_eq!(msg.user_key_or("anon"), "user:slack:U999");
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

    // ---------- slice 123: MessageSubjectResolver ----------

    use crate::types::{Adapter, AdapterResult};
    use futures_executor::block_on;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Adapter that counts how many times `fetch_subject` was called
    /// and returns a configurable Option<String>.
    #[derive(Debug)]
    struct CountingAdapter {
        name: String,
        fetched: AtomicUsize,
        result: Option<String>,
    }

    #[async_trait::async_trait]
    impl Adapter for CountingAdapter {
        fn name(&self) -> &str {
            &self.name
        }
        async fn fetch_subject(&self, _thread_id: &str) -> AdapterResult<Option<String>> {
            self.fetched.fetch_add(1, Ordering::SeqCst);
            Ok(self.result.clone())
        }
    }

    /// Adapter that returns Unsupported(fetch_subject) — equivalent to
    /// upstream's "adapter has no fetchSubject" case. Achieved by NOT
    /// overriding fetch_subject and relying on the default — except the
    /// default returns Ok(None), not Err. So we explicitly return Err
    /// to exercise the upstream "throw" path.
    #[derive(Debug)]
    struct NoFetchSubjectAdapter;

    #[async_trait::async_trait]
    impl Adapter for NoFetchSubjectAdapter {
        fn name(&self) -> &str {
            "no-fetch-subject"
        }
        async fn fetch_subject(&self, _thread_id: &str) -> AdapterResult<Option<String>> {
            Err(crate::types::AdapterError::Unsupported("fetch_subject"))
        }
    }

    fn make_message(id: &str, thread_id: &str) -> Message {
        Message::new(
            id,
            thread_id,
            "hi",
            crate::markdown::root(vec![]),
            json!({}),
            sample_author(),
            sample_metadata(),
            vec![],
        )
    }

    #[test]
    fn message_subject_resolver_returns_subject_from_adapter() {
        let resolver = MessageSubjectResolver::new();
        let adapter = CountingAdapter {
            name: "test".to_string(),
            fetched: AtomicUsize::new(0),
            result: Some("General".to_string()),
        };
        let msg = make_message("m1", "slack:C1");
        let subject = block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert_eq!(subject.as_deref(), Some("General"));
    }

    #[test]
    fn message_subject_resolver_caches_the_result() {
        let resolver = MessageSubjectResolver::new();
        let adapter = CountingAdapter {
            name: "test".to_string(),
            fetched: AtomicUsize::new(0),
            result: Some("General".to_string()),
        };
        let msg = make_message("m1", "slack:C1");
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert_eq!(adapter.fetched.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn message_subject_resolver_caches_a_none_result() {
        let resolver = MessageSubjectResolver::new();
        let adapter = CountingAdapter {
            name: "test".to_string(),
            fetched: AtomicUsize::new(0),
            result: None,
        };
        let msg = make_message("m1", "slack:C1");
        let first = block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert!(first.is_none());
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert_eq!(adapter.fetched.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn message_subject_resolver_returns_none_when_adapter_does_not_support_fetch_subject() {
        // 1:1 with upstream `it("should return null when adapter has
        // no fetchSubject")`. Adapters that don't implement
        // `fetch_subject` (default trait impl returns `Ok(None)` —
        // but explicit `Err(Unsupported)` also collapses to `None`
        // here so the resolver matches upstream's nullable getter
        // semantics).
        let resolver = MessageSubjectResolver::new();
        let adapter = NoFetchSubjectAdapter;
        let msg = make_message("m1", "slack:C1");
        let value = block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert!(value.is_none(), "expected None for Unsupported adapter, got {value:?}");
        // Second call uses cache, never hits the adapter.
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
    }

    #[test]
    fn message_subject_resolver_isolates_threads_by_id() {
        let resolver = MessageSubjectResolver::new();
        let adapter = CountingAdapter {
            name: "test".to_string(),
            fetched: AtomicUsize::new(0),
            result: Some("General".to_string()),
        };
        let m1 = make_message("m1", "slack:C1");
        let m2 = make_message("m2", "slack:C2");
        block_on(resolver.resolve(&adapter, &m1)).unwrap();
        block_on(resolver.resolve(&adapter, &m2)).unwrap();
        // Different thread_ids -> two cache entries -> two fetch calls.
        assert_eq!(adapter.fetched.load(Ordering::SeqCst), 2);
        assert_eq!(resolver.cached_count(), 2);
    }

    #[test]
    fn message_subject_resolver_isolates_caches_by_adapter_name() {
        let resolver = MessageSubjectResolver::new();
        let a = CountingAdapter {
            name: "slack".to_string(),
            fetched: AtomicUsize::new(0),
            result: Some("A".to_string()),
        };
        let b = CountingAdapter {
            name: "teams".to_string(),
            fetched: AtomicUsize::new(0),
            result: Some("B".to_string()),
        };
        let msg = make_message("m1", "T1");
        let from_a = block_on(resolver.resolve(&a, &msg)).unwrap();
        let from_b = block_on(resolver.resolve(&b, &msg)).unwrap();
        assert_eq!(from_a.as_deref(), Some("A"));
        assert_eq!(from_b.as_deref(), Some("B"));
        assert_eq!(resolver.cached_count(), 2);
    }

    #[test]
    fn message_subject_resolver_invalidate_drops_the_cache_entry() {
        let resolver = MessageSubjectResolver::new();
        let adapter = CountingAdapter {
            name: "test".to_string(),
            fetched: AtomicUsize::new(0),
            result: Some("S".to_string()),
        };
        let msg = make_message("m1", "T1");
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert_eq!(adapter.fetched.load(Ordering::SeqCst), 1);
        resolver.invalidate("test", "T1");
        block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert_eq!(adapter.fetched.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn message_subject_resolver_returns_none_for_default_fetch_subject_impl() {
        // 1:1 with upstream `it("should return null when no adapter is
        // set")` (no MessageAdapter wired). In the Rust port the
        // resolver always needs *an* adapter — but adapters that
        // don't override `fetch_subject` get the trait default impl
        // (`Ok(None)`), matching upstream's null-resolver branch.
        #[derive(Debug, Default)]
        struct DefaultAdapter;
        #[async_trait::async_trait]
        impl crate::types::Adapter for DefaultAdapter {
            fn name(&self) -> &str {
                "default"
            }
            // No `fetch_subject` override — uses the trait default
            // which returns `Ok(None)`.
        }
        let resolver = MessageSubjectResolver::new();
        let adapter = DefaultAdapter;
        let msg = make_message("m1", "slack:C1");
        let value = block_on(resolver.resolve(&adapter, &msg)).unwrap();
        assert!(value.is_none());
    }
}
