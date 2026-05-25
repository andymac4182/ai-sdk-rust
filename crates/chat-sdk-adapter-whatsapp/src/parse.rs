//! WhatsApp inbound-message parsing.
//!
//! 1:1 port of upstream `parseMessage(raw: WhatsAppRawMessage)`
//! plus the supporting `extractTextContent` / `buildAttachments`
//! helpers from `packages/adapter-whatsapp/src/index.ts`.
//!
//! Input shape is the upstream `WhatsAppRawMessage`:
//!
//! ```text
//! { message: WhatsAppInboundMessage, contact?: WhatsAppContact, phoneNumberId: string }
//! ```
//!
//! The `message` field is the per-message element from the Cloud API
//! webhook envelope (`entry[].changes[].value.messages[]`), and `contact`
//! is the matching entry from `value.contacts[]` (when present).
//!
//! The parser returns a cross-platform [`chat_sdk_chat::message::Message`].
//! Media attachments carry a `fetch_metadata["mediaId"]` hint that the
//! adapter uses to lazily download bytes (the Rust port has no
//! `fetch_data` callback on `Attachment` — adapters hydrate via the
//! stored mediaId on demand).

use chat_sdk_chat::markdown::{Node, root};
use chat_sdk_chat::message::Message;
use chat_sdk_chat::types::{Attachment, AttachmentKind, Author, BotStatus, MessageMetadata};
use serde::Deserialize;

use crate::encode_thread_id;
use crate::markdown::WhatsAppFormatConverter;

/// Sender contact info. 1:1 with upstream `interface WhatsAppContact`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppContact {
    /// `{ name }` block.
    pub profile: WhatsAppContactProfile,
    /// Sender WhatsApp ID (phone number).
    #[serde(default)]
    pub wa_id: Option<String>,
}

/// Profile sub-block of [`WhatsAppContact`].
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppContactProfile {
    /// Display name.
    pub name: String,
}

/// Text-message payload. 1:1 with upstream `message.text`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppTextBody {
    /// Raw text body.
    pub body: String,
}

/// Image-message payload. 1:1 with upstream `message.image`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppImage {
    /// WhatsApp media ID.
    pub id: String,
    /// MIME type (e.g. `image/jpeg`).
    pub mime_type: String,
    /// SHA-256 of the media (informational).
    #[serde(default)]
    pub sha256: Option<String>,
    /// Optional caption text supplied with the image.
    #[serde(default)]
    pub caption: Option<String>,
}

/// Document payload. 1:1 with upstream `message.document`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppDocument {
    /// WhatsApp media ID.
    pub id: String,
    /// MIME type.
    pub mime_type: String,
    /// SHA-256 of the media (informational).
    #[serde(default)]
    pub sha256: Option<String>,
    /// Original filename.
    #[serde(default)]
    pub filename: Option<String>,
    /// Optional caption text supplied with the document.
    #[serde(default)]
    pub caption: Option<String>,
}

/// Audio payload. 1:1 with upstream `message.audio`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppAudio {
    /// WhatsApp media ID.
    pub id: String,
    /// MIME type.
    pub mime_type: String,
    /// SHA-256 (informational).
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Voice-message payload. 1:1 with upstream `message.voice`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppVoice {
    /// WhatsApp media ID.
    pub id: String,
    /// MIME type (e.g. `audio/ogg; codecs=opus`).
    pub mime_type: String,
    /// SHA-256 (informational).
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Video payload. 1:1 with upstream `message.video`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppVideo {
    /// WhatsApp media ID.
    pub id: String,
    /// MIME type.
    pub mime_type: String,
    /// SHA-256 (informational).
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Sticker payload. 1:1 with upstream `message.sticker`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppSticker {
    /// WhatsApp media ID.
    pub id: String,
    /// MIME type (e.g. `image/webp`).
    pub mime_type: String,
    /// SHA-256 (informational).
    #[serde(default)]
    pub sha256: Option<String>,
    /// Whether the sticker is animated.
    #[serde(default)]
    pub animated: Option<bool>,
}

/// Location payload. 1:1 with upstream `message.location`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppLocation {
    /// Latitude (`number` upstream).
    pub latitude: f64,
    /// Longitude.
    pub longitude: f64,
    /// Optional place name.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional street address.
    #[serde(default)]
    pub address: Option<String>,
}

/// Single inbound message from `entry[].changes[].value.messages[]`.
/// 1:1 with upstream `interface WhatsAppInboundMessage`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppInboundMessage {
    /// Stable message id (`wamid.*`).
    pub id: String,
    /// Sender's WhatsApp ID (E.164 phone number).
    pub from: String,
    /// Unix-seconds timestamp as a string.
    pub timestamp: String,
    /// Message type discriminator (`text`, `image`, `audio`, `video`,
    /// `voice`, `sticker`, `document`, `location`, …).
    #[serde(rename = "type")]
    pub kind: String,
    /// Text body (when `kind == "text"`).
    #[serde(default)]
    pub text: Option<WhatsAppTextBody>,
    /// Image payload (when `kind == "image"`).
    #[serde(default)]
    pub image: Option<WhatsAppImage>,
    /// Document payload (when `kind == "document"`).
    #[serde(default)]
    pub document: Option<WhatsAppDocument>,
    /// Audio payload (when `kind == "audio"`).
    #[serde(default)]
    pub audio: Option<WhatsAppAudio>,
    /// Voice payload (when `kind == "voice"`).
    #[serde(default)]
    pub voice: Option<WhatsAppVoice>,
    /// Video payload (when `kind == "video"`).
    #[serde(default)]
    pub video: Option<WhatsAppVideo>,
    /// Sticker payload (when `kind == "sticker"`).
    #[serde(default)]
    pub sticker: Option<WhatsAppSticker>,
    /// Location payload (when `kind == "location"`).
    #[serde(default)]
    pub location: Option<WhatsAppLocation>,
}

/// Adapter-internal "raw" envelope. 1:1 with upstream
/// `interface WhatsAppRawMessage { message; contact?; phoneNumberId }`.
#[derive(Debug, Clone, Deserialize)]
pub struct WhatsAppRawMessage {
    /// Inbound message body.
    pub message: WhatsAppInboundMessage,
    /// Optional sender contact info (matched from `value.contacts[]`).
    #[serde(default)]
    pub contact: Option<WhatsAppContact>,
    /// Business phone-number id (the bot's id).
    #[serde(rename = "phoneNumberId")]
    pub phone_number_id: String,
}

/// Extract a text-content string for `message`. 1:1 port of upstream
/// `WhatsAppAdapter#extractTextContent`:
///
/// - `text` -> `text.body` (defaulting to empty on missing field).
/// - `image` -> `image.caption ?? "[Image]"`.
/// - `document` -> `document.caption ?? "[Document: <filename ?? "file">]"`.
/// - `audio` -> `"[Audio message]"`.
/// - `voice` -> `"[Voice message]"`.
/// - `video` -> `"[Video]"`.
/// - `sticker` -> `"[Sticker]"`.
/// - `location` -> `"[Location: <name|lat,lng>[ - <address>]]"`.
/// - other -> `None` (unsupported type).
pub fn extract_text_content(message: &WhatsAppInboundMessage) -> Option<String> {
    match message.kind.as_str() {
        "text" => Some(
            message
                .text
                .as_ref()
                .map(|t| t.body.clone())
                .unwrap_or_default(),
        ),
        "image" => Some(
            message
                .image
                .as_ref()
                .and_then(|i| i.caption.clone())
                .unwrap_or_else(|| "[Image]".to_string()),
        ),
        "document" => Some(
            message
                .document
                .as_ref()
                .and_then(|d| d.caption.clone())
                .unwrap_or_else(|| {
                    let name = message
                        .document
                        .as_ref()
                        .and_then(|d| d.filename.clone())
                        .unwrap_or_else(|| "file".to_string());
                    format!("[Document: {name}]")
                }),
        ),
        "audio" => Some("[Audio message]".to_string()),
        "voice" => Some("[Voice message]".to_string()),
        "video" => Some("[Video]".to_string()),
        "sticker" => Some("[Sticker]".to_string()),
        "location" => {
            let loc = message.location.as_ref()?;
            let head = if let Some(name) = loc.name.as_ref().filter(|s| !s.is_empty()) {
                format!("[Location: {name}")
            } else {
                format!("[Location: {}, {}", loc.latitude, loc.longitude)
            };
            let body = if let Some(addr) = loc.address.as_ref().filter(|s| !s.is_empty()) {
                format!("{head} - {addr}]")
            } else {
                format!("{head}]")
            };
            Some(body)
        }
        _ => None,
    }
}

/// Build a single media attachment with a `fetch_metadata["mediaId"]`
/// hint. 1:1 with upstream `buildMediaAttachment(mediaId, type,
/// mimeType, name?)` minus the `fetchData` callback (Rust port stores
/// only the hint — adapters hydrate via the configured HTTP client on
/// demand).
fn build_media_attachment(
    media_id: &str,
    kind: AttachmentKind,
    mime_type: &str,
    name: Option<&str>,
) -> Attachment {
    let mut fetch_metadata = std::collections::HashMap::new();
    fetch_metadata.insert("mediaId".to_string(), media_id.to_string());
    Attachment {
        data: None,
        fetch_metadata: Some(fetch_metadata),
        height: None,
        mime_type: Some(mime_type.to_string()),
        name: name.map(str::to_string),
        size: None,
        kind,
        url: None,
        width: None,
    }
}

/// Build attachments for `message`. 1:1 port of upstream
/// `WhatsAppAdapter#buildAttachments`: emits one attachment per
/// present media field, mapping `voice` -> audio + name="voice" and
/// `sticker` -> image + name="sticker"; `location` becomes a synthetic
/// `file` attachment whose `url` points at Google Maps (matching
/// upstream's `https://www.google.com/maps?q=<lat>,<lng>` template).
pub fn build_attachments(message: &WhatsAppInboundMessage) -> Vec<Attachment> {
    let mut attachments = Vec::new();

    if let Some(image) = &message.image {
        attachments.push(build_media_attachment(
            &image.id,
            AttachmentKind::Image,
            &image.mime_type,
            None,
        ));
    }

    if let Some(document) = &message.document {
        attachments.push(build_media_attachment(
            &document.id,
            AttachmentKind::File,
            &document.mime_type,
            document.filename.as_deref(),
        ));
    }

    if let Some(audio) = &message.audio {
        attachments.push(build_media_attachment(
            &audio.id,
            AttachmentKind::Audio,
            &audio.mime_type,
            None,
        ));
    }

    if let Some(video) = &message.video {
        attachments.push(build_media_attachment(
            &video.id,
            AttachmentKind::Video,
            &video.mime_type,
            None,
        ));
    }

    if let Some(voice) = &message.voice {
        attachments.push(build_media_attachment(
            &voice.id,
            AttachmentKind::Audio,
            &voice.mime_type,
            Some("voice"),
        ));
    }

    if let Some(sticker) = &message.sticker {
        attachments.push(build_media_attachment(
            &sticker.id,
            AttachmentKind::Image,
            &sticker.mime_type,
            Some("sticker"),
        ));
    }

    if let Some(loc) = &message.location
        && loc.latitude.is_finite()
        && loc.longitude.is_finite()
    {
        let map_url = format!(
            "https://www.google.com/maps?q={},{}",
            loc.latitude, loc.longitude
        );
        let name = loc
            .name
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "Location".to_string());
        attachments.push(Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: Some("application/geo+json".to_string()),
            name: Some(name),
            size: None,
            kind: AttachmentKind::File,
            url: Some(map_url),
            width: None,
        });
    }

    attachments
}

/// Format a unix-seconds-string timestamp as an ISO-8601 UTC string
/// (`YYYY-MM-DDTHH:MM:SS.sssZ`). 1:1 with upstream's
/// `new Date(Number.parseInt(raw.message.timestamp, 10) * 1000)` then
/// `.toISOString()` (which is what `MessageMetadata.date_sent` stores
/// in the Rust port — upstream stores the JS `Date` object).
fn iso_from_unix_seconds(timestamp: &str) -> String {
    // Mirror upstream's `Number.parseInt(timestamp, 10)`:
    // tolerant integer parse; on failure fall back to epoch.
    let secs: i64 = timestamp.parse::<i64>().unwrap_or(0);
    let secs_total = secs;
    let millis: i64 = secs_total.checked_mul(1000).unwrap_or(0);
    format_iso_8601_utc_millis(millis)
}

/// Bare-bones gregorian-calendar conversion of `unix_ms` to an
/// ISO-8601 UTC string. Avoids pulling in `chrono`/`time` for the
/// single use site here. Handles dates from year 1 to 9999 (well
/// beyond the WhatsApp Cloud API's epoch window).
fn format_iso_8601_utc_millis(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let ms = unix_ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z")
}

/// Convert a count of days since the Unix epoch (1970-01-01) into
/// a `(year, month, day)` Gregorian-calendar triple.
fn days_to_ymd(days_since_epoch: i64) -> (i64, u32, u32) {
    // Algorithm: Howard Hinnant's civil_from_days
    // (https://howardhinnant.github.io/date_algorithms.html#civil_from_days).
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// Parse a WhatsApp raw-message envelope into the cross-platform
/// [`Message`] shape. 1:1 port of upstream
/// `WhatsAppAdapter#parseMessage(raw)`.
///
/// `bot_phone_number_id` is used to set `author.is_me` (`true` when the
/// sender's WA id matches the bot's). Upstream reads `this._botUserId`
/// which is initialized to `this.phoneNumberId` in `adapter.initialize`.
pub fn parse_message(raw: &WhatsAppRawMessage, bot_phone_number_id: &str) -> Message {
    let text = extract_text_content(&raw.message).unwrap_or_default();
    let formatted_ast = WhatsAppFormatConverter::new()
        .to_ast(&text)
        .unwrap_or_else(|_| Node::Root(root(Vec::new())));
    // Message.formatted is a Root node specifically; coerce.
    let formatted_root = match formatted_ast {
        Node::Root(r) => r,
        other => root(vec![other]),
    };
    let attachments = build_attachments(&raw.message);
    let thread_id = encode_thread_id(&raw.phone_number_id, &raw.message.from);

    let display_name = raw
        .contact
        .as_ref()
        .map(|c| c.profile.name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| raw.message.from.clone());

    let author = Author {
        user_id: raw.message.from.clone(),
        user_name: display_name.clone(),
        full_name: display_name,
        is_bot: BotStatus::FALSE,
        is_me: raw.message.from == bot_phone_number_id,
    };

    let metadata = MessageMetadata {
        date_sent: iso_from_unix_seconds(&raw.message.timestamp),
        edited: false,
        edited_at: None,
    };

    // raw payload preserved on the Message.
    let raw_json = serde_json::to_value(serde_json::json!({
        "message": serde_json::to_value(&raw.message).ok(),
        "contact": raw.contact.as_ref().map(|c| serde_json::json!({
            "profile": { "name": c.profile.name },
            "wa_id": c.wa_id,
        })),
        "phoneNumberId": raw.phone_number_id,
    }))
    .unwrap_or(serde_json::Value::Null);

    Message::new(
        raw.message.id.clone(),
        thread_id,
        text,
        formatted_root,
        raw_json,
        author,
        metadata,
        attachments,
    )
}

// `WhatsAppInboundMessage` needs to be serializable for the `raw_json`
// pass-through above. Manual implementation kept narrow to avoid
// polluting the deserialize path.
impl serde::Serialize for WhatsAppInboundMessage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry("from", &self.from)?;
        map.serialize_entry("timestamp", &self.timestamp)?;
        map.serialize_entry("type", &self.kind)?;
        if let Some(t) = &self.text {
            map.serialize_entry("text", &serde_json::json!({ "body": t.body }))?;
        }
        if let Some(i) = &self.image {
            map.serialize_entry(
                "image",
                &serde_json::json!({
                    "id": i.id,
                    "mime_type": i.mime_type,
                    "sha256": i.sha256,
                    "caption": i.caption,
                }),
            )?;
        }
        if let Some(d) = &self.document {
            map.serialize_entry(
                "document",
                &serde_json::json!({
                    "id": d.id,
                    "mime_type": d.mime_type,
                    "sha256": d.sha256,
                    "filename": d.filename,
                    "caption": d.caption,
                }),
            )?;
        }
        if let Some(a) = &self.audio {
            map.serialize_entry(
                "audio",
                &serde_json::json!({
                    "id": a.id,
                    "mime_type": a.mime_type,
                    "sha256": a.sha256,
                }),
            )?;
        }
        if let Some(v) = &self.voice {
            map.serialize_entry(
                "voice",
                &serde_json::json!({
                    "id": v.id,
                    "mime_type": v.mime_type,
                    "sha256": v.sha256,
                }),
            )?;
        }
        if let Some(v) = &self.video {
            map.serialize_entry(
                "video",
                &serde_json::json!({
                    "id": v.id,
                    "mime_type": v.mime_type,
                    "sha256": v.sha256,
                }),
            )?;
        }
        if let Some(s) = &self.sticker {
            map.serialize_entry(
                "sticker",
                &serde_json::json!({
                    "id": s.id,
                    "mime_type": s.mime_type,
                    "sha256": s.sha256,
                    "animated": s.animated,
                }),
            )?;
        }
        if let Some(l) = &self.location {
            map.serialize_entry(
                "location",
                &serde_json::json!({
                    "latitude": l.latitude,
                    "longitude": l.longitude,
                    "name": l.name,
                    "address": l.address,
                }),
            )?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    //! 16 upstream `parseMessage` test cases from
    //! `packages/adapter-whatsapp/src/index.test.ts:161-503`.
    //!
    //! All tests use the default bot phone-number id `"123456789"` to
    //! match the upstream `createTestAdapter()` fixture.

    use super::*;
    use serde_json::json;

    /// Helper: parse via the inherent path so tests can hand a `serde_json`
    /// literal and get back a `Message`. Mirrors upstream's
    /// `adapter.parseMessage(raw)`.
    fn parse(raw: serde_json::Value) -> Message {
        let parsed: WhatsAppRawMessage = serde_json::from_value(raw).expect("raw deserializes");
        parse_message(&parsed, "123456789")
    }

    // ---------- describe("parseMessage") — 5 cases ----------

    #[test]
    fn should_parse_a_raw_whats_app_text_message() {
        // 1:1 with upstream index.test.ts:162 > "should parse a raw WhatsApp text message"
        let raw = json!({
            "message": {
                "id": "wamid.ABC123",
                "from": "15551234567",
                "timestamp": "1700000000",
                "type": "text",
                "text": { "body": "Hello from WhatsApp!" },
            },
            "phoneNumberId": "123456789",
            "contact": {
                "profile": { "name": "Alice" },
                "wa_id": "15551234567",
            },
        });
        let message = parse(raw);
        assert_eq!(message.id, "wamid.ABC123");
        assert_eq!(message.text, "Hello from WhatsApp!");
        assert_eq!(message.author.user_id, "15551234567");
        assert_eq!(message.author.user_name, "Alice");
    }

    #[test]
    fn should_parse_a_message_without_contact_info() {
        // 1:1 with upstream index.test.ts:186 > "should parse a message without contact info"
        let raw = json!({
            "message": {
                "id": "wamid.DEF456",
                "from": "15559876543",
                "timestamp": "1700000100",
                "type": "text",
                "text": { "body": "No contact info" },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.author.user_name, "15559876543");
    }

    #[test]
    fn should_parse_an_image_message_with_caption() {
        // 1:1 with upstream index.test.ts:202 > "should parse an image message with caption"
        let raw = json!({
            "message": {
                "id": "wamid.IMG001",
                "from": "15551234567",
                "timestamp": "1700000200",
                "type": "image",
                "image": {
                    "id": "media-123",
                    "mime_type": "image/jpeg",
                    "sha256": "abc",
                    "caption": "Check this out",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "Check this out");
    }

    #[test]
    fn should_parse_an_image_message_without_caption() {
        // 1:1 with upstream index.test.ts:222 > "should parse an image message without caption"
        let raw = json!({
            "message": {
                "id": "wamid.IMG002",
                "from": "15551234567",
                "timestamp": "1700000300",
                "type": "image",
                "image": {
                    "id": "media-456",
                    "mime_type": "image/png",
                    "sha256": "def",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Image]");
    }

    #[test]
    fn should_set_correct_date_sent_from_unix_timestamp() {
        // 1:1 with upstream index.test.ts:240 > "should set correct dateSent from unix timestamp"
        let raw = json!({
            "message": {
                "id": "wamid.TIME001",
                "from": "15551234567",
                "timestamp": "1700000000",
                "type": "text",
                "text": { "body": "test" },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        // Upstream asserts `message.metadata.dateSent.getTime() === 1700000000000`.
        // Rust port stores ISO-8601 UTC. 1700000000 -> 2023-11-14T22:13:20.000Z.
        assert_eq!(message.metadata.date_sent, "2023-11-14T22:13:20.000Z");
    }

    // ---------- describe("parseMessage - media attachments") — 9 cases ----------

    #[test]
    fn should_create_an_image_attachment_with_fetch_data() {
        // 1:1 with upstream index.test.ts:256 > "should create an image attachment with fetchData"
        let raw = json!({
            "message": {
                "id": "wamid.IMG001",
                "from": "15551234567",
                "timestamp": "1700000200",
                "type": "image",
                "image": {
                    "id": "media-img-123",
                    "mime_type": "image/jpeg",
                    "sha256": "abc",
                    "caption": "A photo",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "A photo");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("image/jpeg")
        );
        // Upstream asserts `typeof attachments[0].fetchData === "function"`.
        // The Rust port has no `fetchData` callback on Attachment; the
        // hydratable hint lives in `fetch_metadata["mediaId"]`.
        assert_eq!(
            message.attachments[0]
                .fetch_metadata
                .as_ref()
                .and_then(|m| m.get("mediaId"))
                .map(String::as_str),
            Some("media-img-123"),
        );
    }

    #[test]
    fn should_create_a_document_attachment_with_filename() {
        // 1:1 with upstream index.test.ts:280 > "should create a document attachment with filename"
        let raw = json!({
            "message": {
                "id": "wamid.DOC001",
                "from": "15551234567",
                "timestamp": "1700000300",
                "type": "document",
                "document": {
                    "id": "media-doc-456",
                    "mime_type": "application/pdf",
                    "sha256": "def",
                    "filename": "report.pdf",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Document: report.pdf]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::File);
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("application/pdf")
        );
        assert_eq!(message.attachments[0].name.as_deref(), Some("report.pdf"));
    }

    #[test]
    fn should_create_an_audio_attachment() {
        // 1:1 with upstream index.test.ts:303 > "should create an audio attachment"
        let raw = json!({
            "message": {
                "id": "wamid.AUD001",
                "from": "15551234567",
                "timestamp": "1700000400",
                "type": "audio",
                "audio": {
                    "id": "media-aud-789",
                    "mime_type": "audio/ogg",
                    "sha256": "ghi",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Audio message]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::Audio);
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("audio/ogg")
        );
    }

    #[test]
    fn should_create_a_video_attachment() {
        // 1:1 with upstream index.test.ts:324 > "should create a video attachment"
        let raw = json!({
            "message": {
                "id": "wamid.VID001",
                "from": "15551234567",
                "timestamp": "1700000500",
                "type": "video",
                "video": {
                    "id": "media-vid-101",
                    "mime_type": "video/mp4",
                    "sha256": "jkl",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Video]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::Video);
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("video/mp4")
        );
    }

    #[test]
    fn should_create_a_sticker_attachment_as_image_type() {
        // 1:1 with upstream index.test.ts:345 > "should create a sticker attachment as image type"
        let raw = json!({
            "message": {
                "id": "wamid.STK001",
                "from": "15551234567",
                "timestamp": "1700000600",
                "type": "sticker",
                "sticker": {
                    "id": "media-stk-202",
                    "mime_type": "image/webp",
                    "sha256": "mno",
                    "animated": false,
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Sticker]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::Image);
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("image/webp")
        );
        assert_eq!(message.attachments[0].name.as_deref(), Some("sticker"));
    }

    #[test]
    fn should_create_a_location_attachment_with_google_maps_url() {
        // 1:1 with upstream index.test.ts:368 > "should create a location attachment with Google Maps URL"
        let raw = json!({
            "message": {
                "id": "wamid.LOC001",
                "from": "15551234567",
                "timestamp": "1700000700",
                "type": "location",
                "location": {
                    "latitude": 37.7749,
                    "longitude": -122.4194,
                    "name": "San Francisco",
                    "address": "CA, USA",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Location: San Francisco - CA, USA]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::File);
        assert_eq!(
            message.attachments[0].name.as_deref(),
            Some("San Francisco")
        );
        assert_eq!(
            message.attachments[0].url.as_deref(),
            Some("https://www.google.com/maps?q=37.7749,-122.4194")
        );
    }

    #[test]
    fn should_format_location_text_with_coordinates_when_no_name() {
        // 1:1 with upstream index.test.ts:393 > "should format location text with coordinates when no name"
        let raw = json!({
            "message": {
                "id": "wamid.LOC002",
                "from": "15551234567",
                "timestamp": "1700000800",
                "type": "location",
                "location": {
                    "latitude": 48.8566,
                    "longitude": 2.3522,
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Location: 48.8566, 2.3522]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].name.as_deref(), Some("Location"));
    }

    #[test]
    fn should_create_a_voice_message_attachment_as_audio_type() {
        // 1:1 with upstream index.test.ts:413 > "should create a voice message attachment as audio type"
        let raw = json!({
            "message": {
                "id": "wamid.VOC001",
                "from": "15551234567",
                "timestamp": "1700000650",
                "type": "voice",
                "voice": {
                    "id": "media-voc-303",
                    "mime_type": "audio/ogg; codecs=opus",
                    "sha256": "pqr",
                },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.text, "[Voice message]");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].kind, AttachmentKind::Audio);
        assert_eq!(
            message.attachments[0].mime_type.as_deref(),
            Some("audio/ogg; codecs=opus")
        );
        assert_eq!(message.attachments[0].name.as_deref(), Some("voice"));
    }

    #[test]
    fn should_have_no_attachments_for_plain_text_messages() {
        // 1:1 with upstream index.test.ts:436 > "should have no attachments for plain text messages"
        let raw = json!({
            "message": {
                "id": "wamid.TXT001",
                "from": "15551234567",
                "timestamp": "1700000000",
                "type": "text",
                "text": { "body": "Hello" },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert_eq!(message.attachments.len(), 0);
    }

    // ---------- describe("parseMessage - isMention and threadId") — 2 cases ----------

    #[test]
    fn should_not_set_is_mention_for_dms_handled_by_chat_sdk() {
        // 1:1 with upstream index.test.ts:455 > "should not set isMention for DMs (handled by Chat SDK)"
        let raw = json!({
            "message": {
                "id": "wamid.MENTION001",
                "from": "15551234567",
                "timestamp": "1700000000",
                "type": "text",
                "text": { "body": "Hello" },
            },
            "phoneNumberId": "123456789",
        });
        let message = parse(raw);
        assert!(message.is_mention.is_none());
    }

    #[test]
    fn should_encode_thread_id_from_phone_number_id_and_sender() {
        // 1:1 with upstream index.test.ts:471 > "should encode threadId from phoneNumberId and sender"
        let raw = json!({
            "message": {
                "id": "wamid.THREAD001",
                "from": "15559876543",
                "timestamp": "1700000000",
                "type": "text",
                "text": { "body": "test" },
            },
            "phoneNumberId": "987654321",
        });
        let message = parse(raw);
        assert_eq!(message.thread_id, "whatsapp:987654321:15559876543");
    }
}
