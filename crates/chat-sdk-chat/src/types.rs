//! Core types for `chat-sdk-chat`.
//!
//! Progressive 1:1 port of `packages/chat/src/types.ts` from upstream
//! `vercel/chat`. The upstream file is 2,549 lines and pulls in
//! `cards`, `channel`, `message`, `modals`, `postable-object`, `thread`,
//! `jsx-runtime`, and the `mdast` crate. Porting it in one slice is not
//! feasible.
//!
//! This module is built in layers. The current layer contains only the
//! upstream type aliases that have no inter-module dependencies — they are
//! safe to land before `cards`, `channel`, `message`, etc. land. Each new
//! module slice will extend this file with the next layer of types it
//! unblocks.
//!
//! Upstream `types.ts` has no matching `types.test.ts`, so the test floor
//! is satisfied by the per-module tests that exercise these types
//! indirectly. Rust-specific serde round-trip tests live in
//! `#[cfg(test)] mod tests` below as additive coverage.

use serde::{Deserialize, Serialize};

/// Visibility scope of a channel. 1:1 port of upstream
/// `export type ChannelVisibility = "private" | "workspace" | "external" | "unknown"`.
///
/// - `Private`: channel is only visible to invited members
///   (e.g. private Slack channels).
/// - `Workspace`: channel is visible to all workspace members
///   (e.g. public Slack channels).
/// - `External`: channel is shared with external organizations
///   (e.g. Slack Connect).
/// - `Unknown`: visibility cannot be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelVisibility {
    Private,
    Workspace,
    External,
    Unknown,
}

/// Adapter-supplied channel descriptor. 1:1 port of upstream
/// `interface ChannelInfo`. Every field except `id` and `metadata` is
/// optional and elided from the JSON wire format when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Visibility scope of the channel (private/workspace/external/unknown).
    #[serde(
        rename = "channelVisibility",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_visibility: Option<ChannelVisibility>,
    /// Platform-specific channel identifier.
    pub id: String,
    /// Whether the channel is a 1:1 direct message.
    #[serde(rename = "isDM", default, skip_serializing_if = "Option::is_none")]
    pub is_dm: Option<bool>,
    /// Member count when the platform exposes one.
    #[serde(
        rename = "memberCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub member_count: Option<u64>,
    /// Adapter-specific metadata. Mirrors upstream
    /// `metadata: Record<string, unknown>` exactly — any JSON object.
    pub metadata: serde_json::Map<String, serde_json::Value>,
    /// Display name of the channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Options for listing threads inside a channel. 1:1 port of upstream
/// `interface ListThreadsOptions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListThreadsOptions {
    /// Adapter-opaque cursor returned by a prior call. `None` starts a
    /// fresh listing from the most recent thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Upper bound on returned threads. `None` defers to the adapter's
    /// own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Scope at which a lock is acquired. 1:1 port of upstream
/// `export type LockScope = "thread" | "channel"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockScope {
    Thread,
    Channel,
}

/// Concurrency strategy for overlapping messages on the same thread. 1:1
/// port of upstream
/// `export type ConcurrencyStrategy = "drop" | "queue" | "debounce" | "burst" | "concurrent"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConcurrencyStrategy {
    Drop,
    Queue,
    Debounce,
    Burst,
    Concurrent,
}

/// Direction to fetch messages relative to a cursor. 1:1 port of upstream
/// `export type FetchDirection = "forward" | "backward"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FetchDirection {
    Forward,
    Backward,
}

/// Speaker role in a transcript entry. 1:1 port of upstream
/// `export type TranscriptRole = "user" | "assistant" | "system"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptRole {
    User,
    Assistant,
    System,
}

/// Default TTL for thread state, in milliseconds. 1:1 port of upstream
/// `export const THREAD_STATE_TTL_MS = 30 * 24 * 60 * 60 * 1000`.
pub const THREAD_STATE_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Well-known emoji shortcodes recognized across every chat adapter.
///
/// 1:1 port of upstream `export type WellKnownEmoji`. Each variant
/// serializes to the lowercase snake_case shortcode used on the wire
/// (e.g. `WellKnownEmoji::ThumbsUp` ↔ `"thumbs_up"`). Variants are
/// grouped to mirror the upstream comment headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WellKnownEmoji {
    // Reactions & Gestures
    ThumbsUp,
    ThumbsDown,
    Clap,
    Wave,
    Pray,
    Muscle,
    OkHand,
    PointUp,
    PointDown,
    PointLeft,
    PointRight,
    RaisedHands,
    Shrug,
    Facepalm,
    // Emotions & Faces
    Heart,
    Smile,
    Laugh,
    Thinking,
    Sad,
    Cry,
    Angry,
    LoveEyes,
    Cool,
    Wink,
    Surprised,
    Worried,
    Confused,
    Neutral,
    Sleeping,
    Sick,
    MindBlown,
    Relieved,
    Grimace,
    RollingEyes,
    Hug,
    Zany,
    // Status & Symbols
    Check,
    X,
    Question,
    Exclamation,
    Warning,
    Stop,
    Info,
    #[serde(rename = "100")]
    OneHundred,
    Fire,
    Star,
    Sparkles,
    Lightning,
    Boom,
    Eyes,
    // Status Indicators
    GreenCircle,
    YellowCircle,
    RedCircle,
    BlueCircle,
    WhiteCircle,
    BlackCircle,
    // Objects & Tools
    Rocket,
    Party,
    Confetti,
    Balloon,
    Gift,
    Trophy,
    Medal,
    Lightbulb,
    Gear,
    Wrench,
    Hammer,
    Bug,
    Link,
    Lock,
    Unlock,
    Key,
    Pin,
    Memo,
    Clipboard,
    Calendar,
    Clock,
    Hourglass,
    Bell,
    Megaphone,
    SpeechBubble,
    Email,
    Inbox,
    Outbox,
    Package,
    Folder,
    File,
    ChartUp,
    ChartDown,
    Coffee,
    Pizza,
    Beer,
    // Arrows & Directions
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Refresh,
    // Nature & Weather
    Sun,
    Cloud,
    Rain,
    Snow,
    Rainbow,
}

/// String value that may also appear on the wire as an array of strings.
/// Upstream uses `string | string[]` for [`EmojiFormats`] fields because
/// some emoji names map to multiple platform-specific shortcodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    /// Single shortcode (`"+1"`, `"thumbs_up"`, …).
    One(String),
    /// Multiple equivalent shortcodes the platform accepts for one emoji.
    Many(Vec<String>),
}

/// Platform-specific emoji shortcodes for a single emoji. 1:1 port of
/// upstream `interface EmojiFormats { gchat: string | string[]; slack: string | string[] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmojiFormats {
    /// Google Chat Unicode emoji (e.g. `"👍"`, `"❤️"`).
    pub gchat: StringOrList,
    /// Slack emoji shortcode without colons (e.g. `"+1"`, `"heart"`).
    pub slack: StringOrList,
}

/// Full emoji identifier covering both well-known shortcodes and the
/// user-extensible custom-emoji namespace. 1:1 port of upstream
/// `export type Emoji = WellKnownEmoji | keyof CustomEmojiMap`.
///
/// Upstream's `interface CustomEmojiMap {}` is a TypeScript module-
/// augmentation hook with no Rust equivalent; the [`Self::Custom`] variant
/// fills that role by accepting any string shortcode at runtime. On the
/// wire both variants are flat strings (untagged) so JSON shape matches
/// the upstream `string` union exactly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Emoji {
    WellKnown(WellKnownEmoji),
    Custom(String),
}

/// User-supplied mapping from [`Emoji`] to its platform-specific shortcodes.
/// 1:1 port of upstream
/// `export type EmojiMapConfig = Partial<Record<Emoji, EmojiFormats>>`.
pub type EmojiMapConfig = std::collections::HashMap<Emoji, EmojiFormats>;

/// What to do when the per-thread message queue is full. 1:1 port of
/// upstream `"drop-oldest" | "drop-newest"` literal union on
/// [`ConcurrencyConfig::on_queue_full`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueFullPolicy {
    DropOldest,
    DropNewest,
}

/// Fine-grained concurrency configuration. 1:1 port of upstream
/// `interface ConcurrencyConfig`.
///
/// Every field except `strategy` is optional (matches upstream `?:`
/// notation), so adapters can use either a strategy-only `ConcurrencyConfig`
/// or a fully-tuned one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    /// Debounce window in milliseconds (debounce/burst strategies).
    /// Default: 1500.
    #[serde(
        rename = "debounceMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub debounce_ms: Option<u32>,
    /// Max concurrent handlers per thread (concurrent strategy).
    /// Default: `Infinity` upstream → `None` here (no cap).
    #[serde(
        rename = "maxConcurrent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_concurrent: Option<u32>,
    /// Max queued messages per thread (queue/burst strategy). Default: 10.
    #[serde(
        rename = "maxQueueSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_queue_size: Option<u32>,
    /// What to do when the queue is full. Default: `DropOldest`.
    #[serde(
        rename = "onQueueFull",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub on_queue_full: Option<QueueFullPolicy>,
    /// TTL for queued entries in milliseconds. Default: 90000 (90s).
    #[serde(
        rename = "queueEntryTtlMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub queue_entry_ttl_ms: Option<u32>,
    /// The concurrency strategy to use.
    pub strategy: ConcurrencyStrategy,
}

/// Whether a chat author is a bot. 1:1 port of the upstream
/// `boolean | "unknown"` union on [`Author::is_bot`]. Rust has no implicit
/// "either bool or sentinel string" — the explicit enum keeps wire shape
/// honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BotStatus {
    /// Definite yes/no via a JSON boolean.
    Known(bool),
    /// Upstream sentinel string when the platform can't tell.
    Unknown(UnknownLiteral),
}

/// Tag type that serializes to/from the literal string `"unknown"` —
/// used inside [`BotStatus::Unknown`] so the untagged enum can
/// distinguish a JSON `true`/`false` from the string `"unknown"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnknownLiteral {
    #[serde(rename = "unknown")]
    Unknown,
}

impl BotStatus {
    /// `BotStatus::Known(true)` shortcut.
    pub const TRUE: Self = Self::Known(true);
    /// `BotStatus::Known(false)` shortcut.
    pub const FALSE: Self = Self::Known(false);
    /// `BotStatus::Unknown(UnknownLiteral::Unknown)` shortcut.
    pub const UNKNOWN: Self = Self::Unknown(UnknownLiteral::Unknown);
}

/// Message author. 1:1 port of upstream `interface Author`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    /// Display name.
    #[serde(rename = "fullName")]
    pub full_name: String,
    /// Whether the author is a bot. `Unknown` when the platform cannot tell.
    #[serde(rename = "isBot")]
    pub is_bot: BotStatus,
    /// Whether the author is this bot.
    #[serde(rename = "isMe")]
    pub is_me: bool,
    /// Unique user ID.
    #[serde(rename = "userId")]
    pub user_id: String,
    /// Username/handle for @-mentions.
    #[serde(rename = "userName")]
    pub user_name: String,
}

/// User information returned by `adapter.getUser()`. 1:1 port of upstream
/// `interface UserInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInfo {
    /// URL to the user's avatar/profile image.
    #[serde(rename = "avatarUrl", default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// User's email address (requires appropriate scopes on some platforms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// User's display name / full name.
    #[serde(rename = "fullName")]
    pub full_name: String,
    /// Whether the user is a bot.
    #[serde(rename = "isBot")]
    pub is_bot: bool,
    /// Platform-specific user ID.
    #[serde(rename = "userId")]
    pub user_id: String,
    /// Username/handle.
    #[serde(rename = "userName")]
    pub user_name: String,
}

/// Duration shorthand `e.g. "7d", "30m", "2h", "45s"`. 1:1 port of
/// upstream `export type DurationString = `${number}${"s" | "m" | "h" | "d"}``.
///
/// TypeScript expresses this as a template-literal type; Rust uses a
/// validated `String` newtype with [`FromStr`]/[`std::fmt::Display`] and a
/// `unit()` accessor. Construction goes through [`Self::parse`] so invalid
/// shapes can never reach the wire.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DurationString(String);

/// Unit suffix on a [`DurationString`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl DurationUnit {
    /// Single-character upstream suffix (`'s'`, `'m'`, `'h'`, `'d'`).
    pub const fn suffix(self) -> char {
        match self {
            Self::Seconds => 's',
            Self::Minutes => 'm',
            Self::Hours => 'h',
            Self::Days => 'd',
        }
    }

    fn from_char(c: char) -> Option<Self> {
        Some(match c {
            's' => Self::Seconds,
            'm' => Self::Minutes,
            'h' => Self::Hours,
            'd' => Self::Days,
            _ => return None,
        })
    }
}

/// Error from [`DurationString::parse`] when the input does not match
/// upstream `${number}${"s"|"m"|"h"|"d"}` shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDurationString(pub String);

impl std::fmt::Display for InvalidDurationString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid DurationString {:?}: expected `<number>(s|m|h|d)`",
            self.0
        )
    }
}

impl std::error::Error for InvalidDurationString {}

impl DurationString {
    /// Parse `"45s"`, `"30m"`, `"2h"`, `"7d"`, etc. into a validated
    /// [`DurationString`]. Returns [`InvalidDurationString`] on malformed
    /// input.
    pub fn parse(input: &str) -> Result<Self, InvalidDurationString> {
        let bytes = input.as_bytes();
        if bytes.len() < 2 {
            return Err(InvalidDurationString(input.to_string()));
        }
        let last = input.chars().last().expect("non-empty");
        if DurationUnit::from_char(last).is_none() {
            return Err(InvalidDurationString(input.to_string()));
        }
        let number = &input[..input.len() - 1];
        if number.is_empty() || number.parse::<u64>().is_err() {
            return Err(InvalidDurationString(input.to_string()));
        }
        Ok(Self(input.to_string()))
    }

    /// Build a [`DurationString`] from explicit components without going
    /// through string parsing.
    pub fn from_parts(value: u64, unit: DurationUnit) -> Self {
        Self(format!("{}{}", value, unit.suffix()))
    }

    /// Borrowed view of the raw `"<n>(s|m|h|d)"` shorthand.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Numeric component, e.g. `7` for `"7d"`. Always parses because
    /// construction validated it.
    pub fn value(&self) -> u64 {
        self.0[..self.0.len() - 1]
            .parse()
            .expect("validated at construction")
    }

    /// Unit component, e.g. [`DurationUnit::Days`] for `"7d"`.
    pub fn unit(&self) -> DurationUnit {
        DurationUnit::from_char(self.0.chars().last().expect("validated"))
            .expect("validated at construction")
    }
}

impl std::fmt::Display for DurationString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for DurationString {
    type Err = InvalidDurationString;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for DurationString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DurationString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Retention policy on stored transcripts. 1:1 port of the upstream
/// `number | DurationString` union on [`TranscriptsConfig::retention`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RetentionPolicy {
    /// Raw millisecond TTL (upstream `number`).
    Millis(u64),
    /// Duration shorthand (`"7d"`, `"30m"`, …).
    Duration(DurationString),
}

/// Transcript-storage configuration. 1:1 port of upstream
/// `interface TranscriptsConfig`. Every field is optional matching upstream
/// `?:` notation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TranscriptsConfig {
    /// Hard cap; older messages evicted on append. Default 200.
    #[serde(
        rename = "maxPerUser",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_per_user: Option<u32>,
    /// Default retention applied as the list TTL. Refreshed on every
    /// append. Omit for no expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<RetentionPolicy>,
    /// Persist `formatted` (mdast) on each transcript entry. Default false
    /// to keep storage small.
    #[serde(
        rename = "storeFormatted",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub store_formatted: Option<bool>,
}

/// Raw-text postable. 1:1 port of upstream `interface PostableRaw`.
/// Used when the caller wants the platform to render the body without
/// SDK-side markdown conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostableRaw {
    /// File/image attachments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    /// Files to upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileUpload>>,
    /// Raw text passed through as-is to the platform.
    pub raw: String,
}

/// Markdown postable. 1:1 port of upstream `interface PostableMarkdown`.
/// Markdown body is converted to platform format by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostableMarkdown {
    /// File/image attachments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    /// Files to upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileUpload>>,
    /// Markdown text, converted to platform format.
    pub markdown: String,
}

/// AST postable. 1:1 port of upstream `interface PostableAst`. The
/// `ast` field carries an mdast `Root` node which the adapter renders
/// into platform-specific markup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostableAst {
    /// mdast root node, converted to platform format.
    pub ast: crate::markdown::Root,
    /// File/image attachments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    /// Files to upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileUpload>>,
}

/// Card postable. 1:1 port of upstream `interface PostableCard`.
/// Wraps a [`CardElement`] with optional fallback text and files for
/// platforms that can't render rich cards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PostableCard {
    /// Rich card element.
    pub card: crate::cards::CardElement,
    /// Fallback text for platforms/clients that can't render cards.
    #[serde(
        rename = "fallbackText",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback_text: Option<String>,
    /// Files to upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileUpload>>,
}

/// Input type for adapter postMessage/editMessage methods. 1:1 port of
/// upstream `type AdapterPostableMessage = string | PostableRaw |
/// PostableMarkdown | PostableAst | PostableCard | CardElement`.
///
/// The variants are ordered so serde untagged matching picks the most
/// specific shape first: structs whose tag fields disambiguate
/// (`raw`/`markdown`/`ast`/`card`/`type:"card"`) come before the
/// catch-all `String`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AdapterPostableMessage {
    /// `{ raw: string, ... }` — raw text passed through to the platform.
    Raw(PostableRaw),
    /// `{ markdown: string, ... }` — markdown converted by the adapter.
    Markdown(PostableMarkdown),
    /// `{ ast: Root, ... }` — pre-built mdast AST.
    Ast(PostableAst),
    /// `{ card: CardElement, ... }` — card with optional fallback.
    Card(PostableCard),
    /// Direct [`CardElement`] (carries `type: "card"` itself).
    CardElement(crate::cards::CardElement),
    /// Plain string — passed through as raw text.
    Text(String),
}

impl From<PostableRaw> for AdapterPostableMessage {
    fn from(value: PostableRaw) -> Self {
        Self::Raw(value)
    }
}

impl From<PostableMarkdown> for AdapterPostableMessage {
    fn from(value: PostableMarkdown) -> Self {
        Self::Markdown(value)
    }
}

impl From<PostableAst> for AdapterPostableMessage {
    fn from(value: PostableAst) -> Self {
        Self::Ast(value)
    }
}

impl From<PostableCard> for AdapterPostableMessage {
    fn from(value: PostableCard) -> Self {
        Self::Card(value)
    }
}

impl From<crate::cards::CardElement> for AdapterPostableMessage {
    fn from(value: crate::cards::CardElement) -> Self {
        Self::CardElement(value)
    }
}

impl From<String> for AdapterPostableMessage {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for AdapterPostableMessage {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

/// Media type of an [`Attachment`]. 1:1 port of upstream
/// `"image" | "file" | "video" | "audio"` literal union on
/// [`Attachment::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    File,
    Video,
    Audio,
}

/// File/image/video/audio attachment on a chat message. 1:1 port of the
/// data shape of upstream `interface Attachment`.
///
/// **Callback elision.** Upstream also declares an optional
/// `fetchData?: () => Promise<Buffer>` method. That belongs to the
/// adapter behavior layer (each adapter knows how to authenticate
/// fetches to its platform's media endpoint) and will land as a Rust
/// trait method on the upstream `Adapter` placeholder trait once
/// adapter modules ship. The `fetch_metadata` field on this struct is
/// the serializable hint adapters store to reconstruct that callback
/// after rehydration — see the upstream comment for context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Binary data, when already fetched. Same `Buffer | Blob`-collapse
    /// as [`FileUpload::data`] — see [`FileBytes`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<FileBytes>,
    /// Platform-specific metadata an adapter needs to reconstruct
    /// `fetchData` after the attachment is serialized into the
    /// outgoing queue (e.g. WhatsApp `mediaId`, Telegram `fileId`).
    #[serde(
        rename = "fetchMetadata",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fetch_metadata: Option<std::collections::HashMap<String, String>>,
    /// Image/video height when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// MIME type.
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// File size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Type discriminator (`image`/`file`/`video`/`audio`).
    #[serde(rename = "type")]
    pub kind: AttachmentKind,
    /// URL to the file (for linking/downloading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Image/video width when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

impl Attachment {
    /// Return a copy with the inline binary payload removed. 1:1 with
    /// the behavior of upstream `Message.toJSON()`'s attachment-strip:
    /// preserves every other field (URL, mime type, fetch_metadata, …)
    /// so the receiver can rehydrate the data through the adapter's
    /// `fetchData` path. The Rust port has no `fetchData` callback (it
    /// will live on the platform adapter trait), so only `data` needs
    /// clearing.
    pub fn without_inline_data(&self) -> Self {
        Self {
            data: None,
            ..self.clone()
        }
    }
}

/// Link unfurl metadata. 1:1 port of the data shape of upstream
/// `interface LinkPreview`.
///
/// **Callback elision.** Upstream also declares an optional
/// `fetchMessage?: () => Promise<Message>` for links pointing to other
/// chat messages on the same platform. That lookup belongs to the
/// adapter behavior layer (per-platform message-by-id fetch) and will
/// land as a trait method once the `Message` module is ported. The
/// data fields stand cleanly on their own; only the URL is required.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LinkPreview {
    /// Description from unfurl metadata when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Preview image URL when available.
    #[serde(rename = "imageUrl", default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// Site name (e.g. `"Vercel"`).
    #[serde(rename = "siteName", default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    /// Title from unfurl metadata when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The URL itself — always present.
    pub url: String,
}

/// Formatted markdown content carried alongside chat messages and stored
/// transcripts. Upstream this is `Root` from the `mdast` package — the
/// canonical Markdown AST node — `export type FormattedContent = Root`.
///
/// **Placeholder representation.** The Rust port has not yet picked a
/// markdown crate (`markdown-rs` is the leading candidate, see slice 19
/// refinement-loop entry). Until that decision lands, `FormattedContent`
/// is exposed as `serde_json::Value` so the upstream JSON shape of an
/// mdast tree is preserved on the wire and consumers can introspect it.
/// The future `chat-sdk-chat::markdown` module will replace this alias
/// with a typed AST in a single coordinated slice; every downstream type
/// that holds a `FormattedContent` will pick up the new type
/// automatically.
pub type FormattedContent = serde_json::Value;

/// Input shape for appending a non-`Message` transcript entry (typically
/// an assistant reply already posted via `thread.post`). 1:1 port of
/// upstream `interface AppendInput`.
///
/// `formatted` is opaque mdast carried as [`FormattedContent`] (currently
/// `serde_json::Value` — see the placeholder note on that alias).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatted: Option<FormattedContent>,
    #[serde(
        rename = "platformMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub platform_message_id: Option<String>,
    pub role: TranscriptRole,
    pub text: String,
}

/// Single entry in a cross-platform transcript. 1:1 port of upstream
/// `interface TranscriptEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    /// mdast AST. Present only when `transcripts.storeFormatted` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatted: Option<FormattedContent>,
    /// UUID assigned by the SDK at append time. Opaque — not
    /// lexicographically sortable. Use [`Self::timestamp`] to reason about
    /// ordering across stores.
    pub id: String,
    /// Originating adapter name.
    pub platform: String,
    /// Platform-native message ID, when known.
    #[serde(
        rename = "platformMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub platform_message_id: Option<String>,
    pub role: TranscriptRole,
    /// Plain-text body — canonical field for prompt building.
    pub text: String,
    /// Originating thread ID.
    #[serde(rename = "threadId")]
    pub thread_id: String,
    /// Milliseconds since epoch, set at append time on the SDK side.
    pub timestamp: u64,
    /// Cross-platform user key from the `IdentityResolver`.
    #[serde(rename = "userKey")]
    pub user_key: String,
}

/// Options for `Postable::post_ephemeral`. 1:1 port of upstream
/// `interface PostEphemeralOptions`.
///
/// `fallback_to_dm` controls behavior on platforms where native ephemeral
/// messages aren't supported (e.g. Discord):
/// - `true` — fall back to sending a DM to the user.
/// - `false` — return `None` from `post_ephemeral`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PostEphemeralOptions {
    #[serde(rename = "fallbackToDM")]
    pub fallback_to_dm: bool,
}

/// Result of scheduling a message for future delivery. 1:1 port of
/// upstream `interface ScheduledMessage<TRawMessage>`. Currently only
/// supported by the Slack adapter via `chat.scheduleMessage`; other
/// adapters return [`AdapterError::Unsupported("schedule_message")`].
///
/// The upstream interface includes a `cancel(): Promise<void>` method
/// closing over adapter-specific cancellation state. The Rust port
/// captures cancellation via a separate inherent method on the
/// adapter (`cancel_scheduled_message`) — see slice 403; the
/// per-instance closure form is deferred behind a follow-up
/// `ScheduledMessageCancel` trait extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledMessage {
    /// Platform-specific scheduled message ID.
    #[serde(rename = "scheduledMessageId")]
    pub scheduled_message_id: String,
    /// Channel ID where the message will be posted.
    #[serde(rename = "channelId")]
    pub channel_id: String,
    /// When the message will be sent — unix milliseconds.
    /// (Upstream uses JavaScript `Date`; the Rust port stores the
    /// integer epoch millis to keep the type Serialize+Eq.)
    #[serde(rename = "postAtUnixMs")]
    pub post_at_unix_ms: u64,
    /// Platform-specific raw response.
    #[serde(default)]
    pub raw: serde_json::Value,
}

/// Result of opening a modal via [`Adapter::open_modal`]. 1:1 with
/// upstream `{ viewId: string }` return shape. Adapters that don't
/// expose a stable view id can return an empty string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenModalResult {
    /// Platform-specific view id (e.g. Slack `V01ABC`).
    #[serde(rename = "viewId")]
    pub view_id: String,
}

/// Result of posting an ephemeral message. 1:1 port of upstream
/// `interface EphemeralMessage`. Ephemeral messages are visible only to a
/// specific user and typically cannot be edited or deleted (platform-dependent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EphemeralMessage {
    /// Message ID (may be empty for some platforms).
    pub id: String,
    /// Thread ID where the message was sent (or DM thread if fallback was used).
    #[serde(rename = "threadId")]
    pub thread_id: String,
    /// Whether this used native ephemeral or fell back to DM.
    #[serde(rename = "usedFallback")]
    pub used_fallback: bool,
    /// Platform-specific raw response.
    #[serde(default)]
    pub raw: serde_json::Value,
}

/// Context passed to the upstream `lockScope` resolver function on
/// [`ChatConfig`]. 1:1 port of upstream `interface LockScopeContext`.
///
/// `adapter` is the active adapter; the placeholder trait shipped in slice
/// 14 lets this type compile before the per-adapter implementations land.
#[derive(Clone)]
pub struct LockScopeContext {
    /// The active adapter dispatching the message. Held as
    /// `Arc<dyn Adapter>` so the context can outlive a borrow.
    pub adapter: std::sync::Arc<dyn Adapter>,
    /// Platform-specific channel identifier.
    pub channel_id: String,
    /// Whether the originating channel is a direct message.
    pub is_dm: bool,
    /// Platform-specific thread identifier.
    pub thread_id: String,
}

impl std::fmt::Debug for LockScopeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockScopeContext")
            // The adapter trait already requires Debug, but render it as
            // a placeholder so the formatted output is stable across
            // adapter implementations.
            .field("adapter", &"<dyn Adapter>")
            .field("channel_id", &self.channel_id)
            .field("is_dm", &self.is_dm)
            .field("thread_id", &self.thread_id)
            .finish()
    }
}

/// Binary payload accompanying a [`FileUpload`]. 1:1 port adaptation of
/// upstream `data: Buffer | Blob | ArrayBuffer`.
///
/// JavaScript exposes three runtime types that all hold raw bytes; Rust
/// has exactly one canonical byte container, [`Vec<u8>`]. The port collapses
/// the three into a single newtype so the upstream API surface remains
/// honest about "this is the file body" without inventing a sham
/// Buffer/Blob/ArrayBuffer distinction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileBytes(pub Vec<u8>);

impl From<Vec<u8>> for FileBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for FileBytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl std::ops::Deref for FileBytes {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// File attachment to upload with a message. 1:1 port of upstream
/// `interface FileUpload`. The `data: Buffer | Blob | ArrayBuffer` union is
/// collapsed into [`FileBytes`] — see the adaptation note on that type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileUpload {
    /// Binary data. Upstream accepts Buffer/Blob/ArrayBuffer; all three
    /// are functionally identical byte containers and collapse to one
    /// [`FileBytes`] / [`Vec<u8>`] in Rust.
    pub data: FileBytes,
    /// Filename.
    pub filename: String,
    /// MIME type. Optional upstream — adapters infer from the filename
    /// when omitted.
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Options for fetching messages from a thread. 1:1 port of upstream
/// `interface FetchOptions`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct FetchOptions {
    /// Pagination cursor returned by a prior `FetchResult` (`nextCursor`
    /// on the upstream side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Direction. Upstream default: [`FetchDirection::Backward`]. Messages
    /// within each returned page are always in chronological order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<FetchDirection>,
    /// Maximum number of messages to fetch. Upstream default varies by
    /// adapter (50–100); leaving `None` defers to the adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Minimal raw-message payload used by adapters that can hand off the
/// platform-native body opaquely. 1:1 port of upstream
/// `interface RawMessage<TRawMessage = unknown>`.
///
/// The default `TRaw = serde_json::Value` mirrors upstream's
/// `TRawMessage = unknown`. Concrete adapter ports may substitute a
/// platform-specific type (`SlackEvent`, `TelegramUpdate`, …) once their
/// schemas land.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RawMessage<TRaw = serde_json::Value> {
    pub id: String,
    pub raw: TRaw,
    #[serde(rename = "threadId")]
    pub thread_id: String,
}

/// Options accepted by `Transcripts.append`. 1:1 port of upstream
/// `interface AppendOptions`. `user_key` is mandatory only when appending
/// an `AppendInput` (i.e. assistant/system role) — the upstream behavior
/// is documented on the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppendOptions {
    /// Required when appending an `AppendInput` (assistant/system role) —
    /// the SDK has no `Message` instance from which to read the resolved
    /// key. Ignored when appending a `Message`; the message's own
    /// `userKey` is used.
    #[serde(rename = "userKey", default, skip_serializing_if = "Option::is_none")]
    pub user_key: Option<String>,
}

/// Query shape for `Transcripts.list`. 1:1 port of upstream `interface ListQuery`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListQuery {
    /// Newest N kept (still returned in chronological order). Default 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Filter to a subset of adapter names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<String>>,
    /// Filter to specific roles. Upstream default: all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<TranscriptRole>>,
    /// Filter to a single thread.
    #[serde(rename = "threadId", default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(rename = "userKey")]
    pub user_key: String,
}

/// Target for `Transcripts.delete`. Wipes every stored message under the
/// given user key. 1:1 port of upstream `interface DeleteTarget`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeleteTarget {
    #[serde(rename = "userKey")]
    pub user_key: String,
}

/// Query shape for `Transcripts.count`. 1:1 port of upstream
/// `interface CountQuery`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CountQuery {
    #[serde(rename = "userKey")]
    pub user_key: String,
}

/// Display name + identifier pair used for `assignee` / `author` on
/// [`MessageSubject`]. Matches the inline anonymous TypeScript object
/// `{ id: string; name: string }` upstream.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageSubjectActor {
    pub id: String,
    pub name: String,
}

/// Platform-native subject (e.g. GitHub issue, Linear ticket) the upstream
/// `Message` is attached to. 1:1 port of upstream `interface MessageSubject`.
///
/// `raw` mirrors upstream `unknown` exactly with `serde_json::Value`. All
/// other optionals elide from the wire when None.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSubject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<MessageSubjectActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<MessageSubjectActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    pub raw: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Platform-specific subject type tag (e.g. `"issue"`, `"pr"`,
    /// `"ticket"`). Upstream typed as `string`, intentionally not an enum
    /// — the set of values is open-ended across adapters.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Adapter-supplied thread descriptor. 1:1 port of upstream
/// `interface ThreadInfo`. `metadata` mirrors `Record<string, unknown>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadInfo {
    #[serde(rename = "channelId")]
    pub channel_id: String,
    #[serde(
        rename = "channelName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_name: Option<String>,
    #[serde(
        rename = "channelVisibility",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_visibility: Option<ChannelVisibility>,
    pub id: String,
    #[serde(rename = "isDM", default, skip_serializing_if = "Option::is_none")]
    pub is_dm: Option<bool>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// Status of a streamed task. 1:1 port of upstream
/// `"pending" | "in_progress" | "complete" | "error"` literal union on
/// [`TaskUpdateChunk::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Complete,
    Error,
}

/// Slack-specific display mode for `task_update` chunks. 1:1 port of upstream
/// `"timeline" | "plan"` literal union on [`StreamOptions::task_display_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskDisplayMode {
    Timeline,
    Plan,
}

/// Streamed payload emitted by the chat protocol. 1:1 port of upstream
/// `export type StreamChunk = MarkdownTextChunk | TaskUpdateChunk | PlanUpdateChunk`.
///
/// Discriminated by the `type` field on the wire (`"markdown_text"`,
/// `"task_update"`, `"plan_update"`). Each variant carries its upstream
/// payload struct unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamChunk {
    MarkdownText(MarkdownTextChunkData),
    TaskUpdate(TaskUpdateChunkData),
    PlanUpdate(PlanUpdateChunkData),
}

/// Body of a [`StreamChunk::MarkdownText`]. 1:1 port of the non-`type`
/// fields on upstream `interface MarkdownTextChunk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownTextChunkData {
    pub text: String,
}

/// Body of a [`StreamChunk::TaskUpdate`]. 1:1 port of the non-`type`
/// fields on upstream `interface TaskUpdateChunk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskUpdateChunkData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub status: TaskStatus,
    pub title: String,
}

/// Body of a [`StreamChunk::PlanUpdate`]. 1:1 port of the non-`type`
/// fields on upstream `interface PlanUpdateChunk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanUpdateChunkData {
    pub title: String,
}

/// Options for streaming messages. 1:1 port of upstream
/// `interface StreamOptions`. Platform-specific fields are passed through to
/// the adapter; every field is optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StreamOptions {
    /// Slack: the team/workspace ID.
    #[serde(
        rename = "recipientTeamId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub recipient_team_id: Option<String>,
    /// Slack: the user ID to stream to (for AI assistant context).
    #[serde(
        rename = "recipientUserId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub recipient_user_id: Option<String>,
    /// Slack-only Block Kit elements to attach when stopping the stream
    /// (via `chat.stopStream`). Upstream typed as `unknown[]`; preserved
    /// here as `Vec<serde_json::Value>` to mirror that opacity.
    #[serde(
        rename = "stopBlocks",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub stop_blocks: Option<Vec<serde_json::Value>>,
    /// Slack: controls how `task_update` chunks render —
    /// [`TaskDisplayMode::Timeline`] (default upstream) or
    /// [`TaskDisplayMode::Plan`].
    #[serde(
        rename = "taskDisplayMode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub task_display_mode: Option<TaskDisplayMode>,
    /// Minimum interval between updates in ms (default upstream: 1000).
    /// Used for fallback mode (GChat / Teams).
    #[serde(
        rename = "updateIntervalMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub update_interval_ms: Option<u32>,
}

/// Adapter trait — the abstraction every chat platform (`slack`, `teams`,
/// `discord`, …) implements. 1:1 port of upstream `interface Adapter`.
///
/// **Layered port.** Upstream `Adapter` has ~20 methods (`getUser`,
/// `getChannelInfo`, `openModal`, etc.) that depend on `cards`, `modals`,
/// `message`, `channel`, `thread`, and the JSX runtime. Phase 1.5 (slice
/// 122) added the 4-method subset that the chat-sdk consumer modules
/// (`message`, `postable_object`, `reviver`) need; the remaining ~16
/// methods follow as each adapter package ships.
///
/// **Async surface.** Methods are `async` because every adapter method
/// performs platform I/O (Slack Web API, Teams Bot Framework, etc.).
/// Default implementations return [`AdapterError::Unsupported`] so
/// adapters can opt into only the methods their platform supports.
///
/// Implementors must be `Send + Sync` since adapters are shared across
/// async tasks; the supertrait bounds enforce that statically.
#[async_trait::async_trait]
pub trait Adapter: Send + Sync + std::fmt::Debug {
    /// Adapter platform name (e.g. `"slack"`, `"teams"`). 1:1 with
    /// upstream's required `readonly name: string` field. Adapters MUST
    /// implement this; there is no sensible default.
    fn name(&self) -> &str;

    /// Bot user name used by [`crate::chat::Chat::handle_incoming_message`]
    /// to detect `@bot` mentions in incoming messages. 1:1 with
    /// upstream optional `userName?: string` accessor. Default
    /// returns `None`; the dispatcher then falls back to
    /// [`crate::chat::ChatOptions::user_name`] (also optional).
    /// Adapters that fetch the bot identity at `initialize` time
    /// override this to return the resolved name.
    fn user_name(&self) -> Option<&str> {
        None
    }

    /// Bot user id used by [`crate::chat::Chat::handle_incoming_message`]
    /// as a fallback signal for `@<bot-id>` and `<@!?<bot-id>>`
    /// (Discord-style) mentions. 1:1 with upstream optional
    /// `botUserId?: string` accessor. Default returns `None`.
    fn bot_user_id(&self) -> Option<&str> {
        None
    }

    /// Optional adapter-level lock scope. 1:1 with upstream
    /// optional `lockScope?: "thread" | "channel"`. When `Some`,
    /// overrides the per-Chat [`crate::chat::ChatOptions::lock_scope`]
    /// for messages dispatched through this adapter. Default
    /// returns `None` so adapters without an explicit scope fall
    /// through to the chat-level config (which itself defaults to
    /// thread scope). String value (not an enum) to keep the
    /// trait method serializable; the dispatcher parses it.
    fn lock_scope(&self) -> Option<&str> {
        None
    }

    /// Whether the adapter wants per-thread message history cached
    /// on every incoming-message dispatch. 1:1 with upstream
    /// optional `persistThreadHistory?: boolean` (defaults to
    /// `false`). When `true`, the dispatcher appends each
    /// incoming message to `msg-history:<thread_id>` via
    /// [`StateAdapter::append_to_list`] using the per-Chat
    /// [`crate::thread_history::ThreadHistoryConfig`] caps.
    fn persist_thread_history(&self) -> bool {
        false
    }

    /// Deprecated alias for [`Self::persist_thread_history`]. 1:1
    /// with upstream optional `persistMessageHistory?: boolean`.
    /// Either flag (or both) opts the adapter in to history caching.
    fn persist_message_history(&self) -> bool {
        false
    }

    /// Tear down any adapter-side resources (sockets, intervals, in-
    /// flight retries). 1:1 with upstream `disconnect?: () =>
    /// Promise<void>`. The default no-op makes this opt-in for
    /// adapters that don't hold long-lived resources.
    async fn disconnect(&self) -> AdapterResult<()> {
        Ok(())
    }

    /// Called when the parent `Chat` instance binds this adapter.
    /// 1:1 with upstream `initialize(chat: ChatInstance): Promise<void>`
    /// minus the `ChatInstance` argument — upstream uses it to capture
    /// a reference for later self-singleton lookup; in the Rust port
    /// adapters reach `Chat` via [`crate::chat_singleton::get_chat_singleton`].
    /// Default no-op so existing adapters compile unchanged.
    async fn initialize(&self) -> AdapterResult<()> {
        Ok(())
    }

    /// Fetch the subject of a thread (the channel topic or DM partner
    /// label). 1:1 with upstream `fetchSubject(threadId): Promise<string
    /// | null>`. The default returns `Ok(None)` so adapters that don't
    /// expose a subject (1-on-1 DMs on some platforms) can leave it
    /// unimplemented.
    async fn fetch_subject(&self, _thread_id: &str) -> AdapterResult<Option<String>> {
        Ok(None)
    }

    /// Post a plain-text message to a thread. 1:1 with upstream
    /// `postMessage(threadId, text, options?): Promise<{ id: string }>`.
    /// Returns the platform-assigned message id. Adapters that don't
    /// implement this return [`AdapterError::Unsupported`].
    async fn post_message(&self, _thread_id: &str, _text: &str) -> AdapterResult<String> {
        Err(AdapterError::Unsupported("post_message"))
    }

    /// Post a typed object (card, modal, plan, …) to a thread. 1:1
    /// with upstream `postObject(threadId, kind, data): Promise<{ id:
    /// string }>`. The receiving adapter routes by `kind` to the
    /// platform-specific renderer.
    async fn post_object(
        &self,
        _thread_id: &str,
        _kind: &str,
        _data: serde_json::Value,
    ) -> AdapterResult<String> {
        Err(AdapterError::Unsupported("post_object"))
    }

    /// Parse a platform-native message payload into the cross-platform
    /// [`crate::message::Message`] shape. 1:1 with upstream
    /// `parseMessage(raw): Message`. The default returns
    /// [`AdapterError::Unsupported`] because parsing is inherently
    /// platform-specific.
    async fn parse_message(
        &self,
        _raw: serde_json::Value,
    ) -> AdapterResult<crate::message::Message> {
        Err(AdapterError::Unsupported("parse_message"))
    }

    /// Edit an existing message. 1:1 with upstream
    /// `editMessage(threadId, messageId, message): Promise<RawMessage>`.
    /// Returns the platform-assigned id of the edited message (most
    /// platforms reuse the original id; Slack returns the same `ts`,
    /// Discord/Telegram return the same message_id, etc.).
    async fn edit_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _text: &str,
    ) -> AdapterResult<String> {
        Err(AdapterError::Unsupported("edit_message"))
    }

    /// Delete an existing message. 1:1 with upstream
    /// `deleteMessage(threadId, messageId): Promise<void>`. Returns
    /// `()` on success; the platform's "already deleted" / "not found"
    /// responses surface as `AdapterError::InvalidPayload`.
    async fn delete_message(&self, _thread_id: &str, _message_id: &str) -> AdapterResult<()> {
        Err(AdapterError::Unsupported("delete_message"))
    }

    /// Add an emoji reaction to a message. 1:1 with upstream
    /// `addReaction(threadId, messageId, emoji): Promise<void>`. The
    /// `emoji` parameter is the platform-native short-name (e.g.
    /// `"thumbsup"` for Slack, the literal `"👍"` glyph for Discord).
    async fn add_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> AdapterResult<()> {
        Err(AdapterError::Unsupported("add_reaction"))
    }

    /// Remove a reaction from `message_id` in `thread_id`. 1:1 with
    /// upstream `removeReaction(threadId, messageId, emoji):
    /// Promise<void>`. Symmetric with [`Self::add_reaction`] — the
    /// `emoji` is the same platform-native short-name. Default
    /// returns `Err(Unsupported("remove_reaction"))`.
    async fn remove_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> AdapterResult<()> {
        Err(AdapterError::Unsupported("remove_reaction"))
    }

    /// Signal the human user that the bot is composing a reply. 1:1
    /// with upstream `startTyping(threadId, status?): Promise<void>`.
    /// `status` is an optional platform-specific status line (e.g.
    /// Slack's "is typing…" footer). Adapters that don't expose a
    /// typing indicator return Ok(()) silently.
    async fn start_typing(&self, _thread_id: &str, _status: Option<&str>) -> AdapterResult<()> {
        Err(AdapterError::Unsupported("start_typing"))
    }

    /// Optional hook called when a thread is subscribed via
    /// [`crate::thread::Thread::subscribe`]. 1:1 with upstream
    /// `onThreadSubscribe?(threadId): Promise<void>`. Default no-op
    /// — adapters that need to react to subscription (e.g. join a
    /// platform-side channel) override this method.
    async fn on_thread_subscribe(&self, _thread_id: &str) -> AdapterResult<()> {
        Ok(())
    }

    /// Derive the channel id this thread lives in. 1:1 with upstream
    /// optional `channelIdFromThreadId?(threadId): string`. Default
    /// returns `None` — [`crate::channel::derive_channel_id`] falls
    /// back to `thread_id` when this is `None`. Adapters that have
    /// a meaningful channel/thread separation (every adapter except
    /// Messenger/WhatsApp) override this.
    fn channel_id_from_thread_id(&self, _thread_id: &str) -> Option<String> {
        None
    }

    /// Optional DM-thread detection. 1:1 with upstream optional
    /// `isDM?(threadId): boolean`. Returns `None` when the adapter
    /// doesn't model channel/DM separation (e.g. Messenger/WhatsApp
    /// where every thread is a DM, or platforms that always return
    /// the same answer — those override and return
    /// `Some(<constant>)`).
    fn is_dm(&self, _thread_id: &str) -> Option<bool> {
        None
    }

    /// Optional adapter-supplied channel descriptor lookup. 1:1 with
    /// upstream optional `fetchChannelInfo?(channelId): Promise<ChannelInfo>`.
    /// Default returns `Err(Unsupported("fetch_channel_info"))` —
    /// [`crate::channel::Channel::fetch_metadata`] collapses that to
    /// a synthesized basic `ChannelInfo` so callers always get
    /// *some* descriptor back.
    async fn fetch_channel_info(&self, _channel_id: &str) -> AdapterResult<ChannelInfo> {
        Err(AdapterError::Unsupported("fetch_channel_info"))
    }

    /// Optional Open-DM convenience. 1:1 with upstream optional
    /// `openDM?(userId): Promise<string>` — returns the
    /// platform-encoded thread id for the DM with `user_id`. Default
    /// returns `Err(Unsupported("open_dm"))` so [`crate::chat::Chat::open_dm`]
    /// can surface the upstream "Adapter does not support openDM" error.
    async fn open_dm(&self, _user_id: &str) -> AdapterResult<String> {
        Err(AdapterError::Unsupported("open_dm"))
    }

    /// Optional user-info lookup. 1:1 with upstream optional
    /// `getUser?(userId): Promise<UserInfo | null>` — resolves a
    /// platform user id to a `UserInfo` descriptor, or `Ok(None)`
    /// when the user isn't found. Default returns
    /// `Err(Unsupported("get_user"))` so [`crate::chat::Chat::get_user`]
    /// can surface the upstream "Adapter does not support getUser"
    /// error.
    async fn get_user(&self, _user_id: &str) -> AdapterResult<Option<UserInfo>> {
        Err(AdapterError::Unsupported("get_user"))
    }

    /// Optional native ephemeral post. 1:1 with upstream optional
    /// `postEphemeral?(threadId, userId, message): Promise<EphemeralMessage>`.
    /// Posts a message visible only to `user_id` in `thread_id`.
    /// Default returns `Err(Unsupported("post_ephemeral"))` so
    /// [`crate::thread::Thread::post_ephemeral`] can detect the
    /// missing-method case and apply the DM-fallback policy
    /// (mirrors upstream `if (this.adapter.postEphemeral)`).
    async fn post_ephemeral(
        &self,
        _thread_id: &str,
        _user_id: &str,
        _text: &str,
    ) -> AdapterResult<EphemeralMessage> {
        Err(AdapterError::Unsupported("post_ephemeral"))
    }

    /// Optional channel-scoped post. 1:1 with upstream optional
    /// `postChannelMessage?(channelId, text): Promise<{id: string}>`.
    /// Some platforms distinguish channel-level posts (no parent
    /// thread) from in-thread replies — upstream uses
    /// `postChannelMessage` when present, otherwise falls back to
    /// `post_message`. Default returns
    /// `Err(Unsupported("post_channel_message"))` so
    /// [`crate::channel::Channel::post`] can fall through to
    /// `post_message`.
    async fn post_channel_message(&self, _channel_id: &str, _text: &str) -> AdapterResult<String> {
        Err(AdapterError::Unsupported("post_channel_message"))
    }

    /// Optional native scheduled-message dispatch. 1:1 with upstream
    /// optional `scheduleMessage?(threadId, text, options):
    /// Promise<ScheduledMessage>`. Schedules `text` for future
    /// delivery in `thread_id` at `post_at_unix_ms`. Default returns
    /// `Err(Unsupported("schedule_message"))` so
    /// [`crate::thread::Thread::schedule`] can detect the
    /// missing-method case and surface
    /// [`crate::errors::ChatError::NotImplemented`] (matching
    /// upstream `NotImplementedError("scheduling")`).
    async fn schedule_message(
        &self,
        _thread_id: &str,
        _text: &str,
        _post_at_unix_ms: u64,
    ) -> AdapterResult<ScheduledMessage> {
        Err(AdapterError::Unsupported("schedule_message"))
    }

    /// Schedule a typed PostableMessage (raw / markdown / AST shape)
    /// for future delivery. 1:1 with the upstream
    /// `scheduleMessage(threadId, message: PostableMessage, options)`
    /// overload that accepts non-string message shapes. The Rust
    /// port keeps the string-only [`Self::schedule_message`] for
    /// the common case (slices 403..405) and adds this sibling for
    /// the typed-message variants (slice 484): the message value is
    /// the upstream JSON shape `{raw} | {markdown} | {ast}` passed
    /// through opaquely. Adapters that want to handle typed inputs
    /// override this; the default returns `Err(Unsupported)` and
    /// [`crate::thread::Thread::schedule_postable`] surfaces
    /// `ChatError::NotImplemented("scheduling")` to match upstream's
    /// `NotImplementedError`.
    async fn schedule_message_postable(
        &self,
        _thread_id: &str,
        _message: &serde_json::Value,
        _post_at_unix_ms: u64,
    ) -> AdapterResult<ScheduledMessage> {
        Err(AdapterError::Unsupported("schedule_message_postable"))
    }

    /// Optional native modal-open dispatch. 1:1 with upstream
    /// optional `openModal?(triggerId, modal, contextId):
    /// Promise<{ viewId: string } | undefined>`. Opens a modal
    /// view in response to an interaction-trigger (button click,
    /// slash command, etc.). The `context_id` is a dispatcher-
    /// generated UUID that the adapter persists on the modal so
    /// the eventual `viewSubmission` event can look up the
    /// originating thread/channel context.
    ///
    /// Default returns `Err(Unsupported("open_modal"))` so the
    /// [`crate::chat::Chat`] orchestration layer can detect the
    /// missing-method case and surface upstream's
    /// `"<adapter> does not support modals"` warning + return
    /// `None` to the caller.
    async fn open_modal(
        &self,
        _trigger_id: &str,
        _modal: &crate::modals::ModalElement,
        _context_id: &str,
    ) -> AdapterResult<OpenModalResult> {
        Err(AdapterError::Unsupported("open_modal"))
    }

    /// Optional cancellation hook for previously-scheduled messages.
    /// 1:1 with upstream's per-`ScheduledMessage` `cancel():
    /// Promise<void>` closure — the Rust port routes cancellation
    /// through this Adapter method so [`ScheduledMessage`] can stay
    /// `Serialize + Eq` (closures aren't representable in serde-
    /// derivable structs). Default returns
    /// `Err(Unsupported("cancel_scheduled_message"))`.
    async fn cancel_scheduled_message(
        &self,
        _channel_id: &str,
        _scheduled_message_id: &str,
    ) -> AdapterResult<()> {
        Err(AdapterError::Unsupported("cancel_scheduled_message"))
    }
}

/// Errors returned by [`Adapter`] methods. Mirrors upstream's
/// `throw new Error(...)` posture across the platform-specific
/// adapter implementations.
#[derive(Debug)]
pub enum AdapterError {
    /// The adapter does not implement this method. Matches upstream's
    /// `throw new Error("not implemented")` pattern.
    Unsupported(&'static str),
    /// Platform-specific I/O or API error wrapped from the adapter's
    /// HTTP/SDK layer.
    Io(Box<dyn std::error::Error + Send + Sync>),
    /// The supplied raw payload didn't match the platform's expected
    /// shape (used by `parse_message` when a Slack event-payload field
    /// is missing, a Teams `messageId` is malformed, …).
    InvalidPayload(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(method) => {
                write!(f, "Adapter does not implement `{method}`")
            }
            Self::Io(err) => write!(f, "Adapter I/O error: {err}"),
            Self::InvalidPayload(msg) => {
                write!(f, "Adapter parsed an invalid payload: {msg}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

/// Convenience alias for the `Result` shape every [`Adapter`] method
/// returns.
pub type AdapterResult<T> = Result<T, AdapterError>;

/// State-backend trait — the storage abstraction (`state-memory`,
/// `state-redis`, `state-ioredis`, `state-pg`, …). 1:1 port of upstream
/// `interface StateAdapter`.
///
/// **Layered port.** Upstream `StateAdapter` has ~20 methods covering
/// connect/disconnect, locks, lists, queues, key/value cache, subscriptions.
/// Phase 1.5 (slice 117) added the 5-method key/value + list subset that
/// the chat-sdk consumer modules (`callback_url`, `transcripts`,
/// `thread_history`, `postable_object`) need; locks / queues /
/// subscriptions follow when their consumer code lands.
///
/// **Async surface.** Methods are `async` because the production state
/// backends (Redis, ioredis, Postgres) perform real I/O. The in-memory
/// backend (`crates/chat-sdk-state-memory`) wraps its sync internals in
/// trivial `async fn` shims so it can satisfy the trait without pulling
/// in a runtime dependency at the call site. Callers that drive the
/// trait need an async executor (tokio, smol, async-std, …) the way
/// upstream consumers need a JS event loop.
///
/// Each state-port slice MUST extend this trait, never define a new
/// trait.
#[async_trait::async_trait]
pub trait StateAdapter: Send + Sync + std::fmt::Debug {
    /// Tear down state-backend resources (connection pool, scheduled
    /// expirations). 1:1 with upstream `disconnect?: () =>
    /// Promise<void>`. Default no-op for in-memory backends.
    async fn disconnect(&self) -> StateResult<()> {
        Ok(())
    }

    /// Open the state-backend connection. 1:1 with upstream
    /// `connect(): Promise<void>` — called once by `Chat` during
    /// initialization. Default no-op for in-memory backends that
    /// hold no real connections.
    async fn connect(&self) -> StateResult<()> {
        Ok(())
    }

    /// Read a value out of the key/value cache. 1:1 with upstream
    /// `get<T>(key: string): Promise<T | null>`. Returns `Ok(None)`
    /// when the key is unset or expired (matching upstream's
    /// `Promise<T | null>` resolved with `null`).
    async fn get(&self, key: &str) -> StateResult<Option<serde_json::Value>>;

    /// Write a value to the key/value cache. 1:1 with upstream
    /// `set<T>(key, value, options?): Promise<void>`. `ttl_ms` mirrors
    /// upstream's `options.ttlMs`; `None` means "no expiry" (matching
    /// upstream's `ttlMs ?? null` behavior).
    async fn set(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
    ) -> StateResult<()>;

    /// Delete a key from the cache. 1:1 with upstream
    /// `delete(key): Promise<void>`. A no-op when the key is absent
    /// (matches upstream's silent absence handling).
    async fn delete(&self, key: &str) -> StateResult<()>;

    /// Append a value to a list. 1:1 with upstream
    /// `appendToList<T>(key, value, options?): Promise<void>`.
    /// `max_length` caps the list (oldest entries drop when exceeded);
    /// `ttl_ms` matches upstream's optional `options.ttlMs`.
    async fn append_to_list(
        &self,
        key: &str,
        value: serde_json::Value,
        max_length: Option<usize>,
        ttl_ms: Option<u64>,
    ) -> StateResult<()>;

    /// Read a list. 1:1 with upstream
    /// `getList<T>(key, options?): Promise<T[]>`. Returns an empty
    /// vector when the list is unset or expired. `limit` matches
    /// upstream's optional `options.limit`.
    async fn get_list(
        &self,
        key: &str,
        limit: Option<usize>,
    ) -> StateResult<Vec<serde_json::Value>>;

    /// Conditional write. 1:1 with upstream
    /// `setIfNotExists<T>(key, value, options?): Promise<boolean>`.
    /// Returns `true` when the value was written, `false` when a
    /// non-expired value already lived at `key`. Default impl falls
    /// back to `get`+`set` — production backends should override with
    /// an atomic implementation (`SETNX` / `INSERT … ON CONFLICT`).
    async fn set_if_not_exists(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
    ) -> StateResult<bool> {
        if self.get(key).await?.is_some() {
            return Ok(false);
        }
        self.set(key, value, ttl_ms).await?;
        Ok(true)
    }

    /// Acquire a per-thread lock. 1:1 with upstream
    /// `acquireLock(threadId, ttlMs): Promise<Lock | null>`. Returns
    /// `Ok(None)` when a non-expired lock is already held for the
    /// thread; otherwise returns a fresh [`Lock`] (token + expiry).
    /// Default impl returns `Ok(None)` so backends that don't ship a
    /// lock primitive can opt out (consumers must not assume a lock
    /// was granted just because no error came back).
    async fn acquire_lock(&self, _thread_id: &str, _ttl_ms: u64) -> StateResult<Option<Lock>> {
        Ok(None)
    }

    /// Release a per-thread lock. 1:1 with upstream
    /// `releaseLock(lock): Promise<void>`. Default is a no-op so
    /// backends that don't implement locks can leave it unimplemented.
    async fn release_lock(&self, _lock: &Lock) -> StateResult<()> {
        Ok(())
    }

    /// Force-release a thread's lock without checking ownership.
    /// 1:1 with upstream `forceReleaseLock(threadId): Promise<void>`.
    /// Default is a no-op.
    async fn force_release_lock(&self, _thread_id: &str) -> StateResult<()> {
        Ok(())
    }

    /// Extend a lock's TTL. 1:1 with upstream
    /// `extendLock(lock, ttlMs): Promise<boolean>`. Returns `true` if
    /// the lock was extended, `false` if it had already expired or was
    /// stolen. Default returns `false`.
    /// Subscribe to a thread. 1:1 with upstream
    /// `subscribe(threadId): Promise<void>`. Default impl writes a
    /// truthy marker under `subscribed:<thread_id>` via the existing
    /// `set` call. Backends with a native set/registry (e.g.
    /// state-memory's `HashSet`) override for better lookup cost.
    async fn subscribe(&self, thread_id: &str) -> StateResult<()> {
        self.set(
            &format!("subscribed:{thread_id}"),
            serde_json::Value::Bool(true),
            None,
        )
        .await
    }

    /// Unsubscribe from a thread. 1:1 with upstream
    /// `unsubscribe(threadId): Promise<void>`. Default impl deletes
    /// the `subscribed:<thread_id>` key.
    async fn unsubscribe(&self, thread_id: &str) -> StateResult<()> {
        self.delete(&format!("subscribed:{thread_id}")).await
    }

    /// Is this thread currently subscribed? 1:1 with upstream
    /// `isSubscribed(threadId): Promise<boolean>`. Default impl
    /// inspects the `subscribed:<thread_id>` key for a truthy value.
    async fn is_subscribed(&self, thread_id: &str) -> StateResult<bool> {
        Ok(matches!(
            self.get(&format!("subscribed:{thread_id}")).await?,
            Some(serde_json::Value::Bool(true))
        ))
    }

    async fn extend_lock(&self, _lock: &Lock, _ttl_ms: u64) -> StateResult<bool> {
        Ok(false)
    }
}

/// Errors returned by [`StateAdapter`] method calls. Mirrors upstream's
/// `throw new Error(...)` posture: every method can fail if the backend
/// is disconnected or hits an I/O error. The Rust port surfaces these
/// through `Result` rather than panicking.
///
/// Production state backends (Redis, Postgres) will return
/// [`StateAdapterError::Io`] with the underlying error wrapped.
#[derive(Debug)]
pub enum StateAdapterError {
    /// The backend is not in a usable state (e.g. `connect()` was
    /// never called, or a previous `disconnect()` torched the
    /// connection pool). 1:1 with upstream's `"not connected"` throw.
    NotConnected,
    /// Underlying I/O or serialization error from a production state
    /// backend.
    Io(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for StateAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConnected => f.write_str("StateAdapter is not connected"),
            Self::Io(err) => write!(f, "StateAdapter I/O error: {err}"),
        }
    }
}

impl std::error::Error for StateAdapterError {}

/// Convenience alias for the `Result` shape every [`StateAdapter`]
/// method returns.
pub type StateResult<T> = Result<T, StateAdapterError>;

/// Metadata fields carried alongside the body of a [`crate::message::Message`].
/// 1:1 port of upstream `interface MessageMetadata`.
///
/// **Date representation.** Upstream `dateSent: Date` is a JavaScript
/// `Date` instance. On the Rust port the serialized wire form keeps the
/// upstream ISO-8601 string, and the in-memory struct also stores the
/// ISO string directly. Adopters that need a typed clock can parse the
/// string with `chrono::DateTime` or `time::OffsetDateTime`; the Rust
/// port refuses to take on a heavy datetime dependency just for this
/// pass-through. The same applies to `edited_at`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// ISO-8601 UTC timestamp of when the message was sent.
    #[serde(rename = "dateSent")]
    pub date_sent: String,
    /// Whether the message has been edited at least once.
    pub edited: bool,
    /// ISO-8601 UTC timestamp of the last edit, when present.
    #[serde(rename = "editedAt", default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<String>,
}

/// Queue entry used by the per-thread message queues in the
/// `queue`/`debounce`/`burst` concurrency strategies. 1:1 port of
/// upstream `interface QueueEntry { enqueuedAt; expiresAt; message }`.
///
/// **Layered port.** Upstream's `message: Message` field references
/// the not-yet-ported `interface Message` from `chat::types`. The Rust
/// port uses `serde_json::Value` as the placeholder until [the
/// `Message` layer lands](../../docs/chat/upstream-parity.md). Adapter
/// + state-backend tests treat the field as opaque, so the placeholder
/// preserves wire-shape parity without forcing a Message port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    /// Unix-ms timestamp when this entry was enqueued.
    #[serde(rename = "enqueuedAt")]
    pub enqueued_at: u64,
    /// Unix-ms expiry; stale entries are discarded on dequeue.
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
    /// Opaque payload (upstream `Message`).
    pub message: serde_json::Value,
}

/// State-backend lock token. 1:1 port of upstream
/// `interface Lock { expiresAt: number; threadId: string; token: string }`.
///
/// Locks are issued by [`StateAdapter::acquireLock`-equivalent] implementations
/// (e.g. the future `chat-sdk-state-memory` crate). The `token` is the
/// ownership credential — a release call only succeeds when the supplied
/// token matches the stored one. `expiresAt` is a Unix timestamp in
/// milliseconds (matching upstream `Date.now() + ttlMs`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Lock {
    /// Unix-ms timestamp at which the lock auto-expires.
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
    /// Thread the lock guards.
    #[serde(rename = "threadId")]
    pub thread_id: String,
    /// Ownership credential — only the holder of this token can release.
    pub token: String,
}

/// Immutable emoji value with object identity (upstream singletons).
///
/// 1:1 port of upstream `interface EmojiValue` — see
/// `packages/chat/src/types.ts` and `packages/chat/src/emoji.ts`. The
/// upstream object identity guarantee (`emoji.thumbs_up === emoji.thumbs_up`)
/// will be enforced by the future `emoji.rs` module via an interning
/// registry; this type definition only captures the data and JSON shape.
///
/// Serializes as a plain string equal to the upstream
/// `toJSON()` / `toString()` placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmojiValue {
    /// Normalized emoji shortcode (e.g. `"thumbs_up"`).
    pub name: String,
}

impl EmojiValue {
    /// Construct an [`EmojiValue`] from its normalized shortcode.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Render the upstream placeholder string used in formatted messages
    /// and as the `toString()` / `toJSON()` value. Upstream format:
    /// `{{emoji:name}}`.
    pub fn placeholder(&self) -> String {
        format!("{{{{emoji:{}}}}}", self.name)
    }
}

impl std::fmt::Display for EmojiValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.placeholder())
    }
}

impl Serialize for EmojiValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Upstream toJSON() returns the `{{emoji:name}}` placeholder, not
        // the raw name. Preserving that on the wire keeps message-formatting
        // parity.
        serializer.serialize_str(&self.placeholder())
    }
}

impl<'de> Deserialize<'de> for EmojiValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let placeholder = String::deserialize(deserializer)?;
        let name = placeholder
            .strip_prefix("{{emoji:")
            .and_then(|s| s.strip_suffix("}}"))
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "expected EmojiValue placeholder `{{{{emoji:name}}}}`, got {placeholder:?}"
                ))
            })?;
        Ok(Self {
            name: name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    //! Rust-specific serde round-trip coverage for the standalone type
    //! aliases. `types.ts` has no upstream test file, so these are purely
    //! additive Rust safety nets — they keep the wire format honest while
    //! the rest of `types.ts` waits on its module dependencies to land.

    use super::*;

    #[test]
    fn channel_visibility_serializes_to_upstream_strings() {
        assert_eq!(
            serde_json::to_string(&ChannelVisibility::Private).unwrap(),
            "\"private\""
        );
        assert_eq!(
            serde_json::to_string(&ChannelVisibility::Workspace).unwrap(),
            "\"workspace\""
        );
        assert_eq!(
            serde_json::to_string(&ChannelVisibility::External).unwrap(),
            "\"external\""
        );
        assert_eq!(
            serde_json::to_string(&ChannelVisibility::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    #[test]
    fn channel_visibility_round_trips() {
        for value in [
            ChannelVisibility::Private,
            ChannelVisibility::Workspace,
            ChannelVisibility::External,
            ChannelVisibility::Unknown,
        ] {
            let json = serde_json::to_string(&value).unwrap();
            let back: ChannelVisibility = serde_json::from_str(&json).unwrap();
            assert_eq!(value, back);
        }
    }

    #[test]
    fn lock_scope_serializes_to_upstream_strings() {
        assert_eq!(
            serde_json::to_string(&LockScope::Thread).unwrap(),
            "\"thread\""
        );
        assert_eq!(
            serde_json::to_string(&LockScope::Channel).unwrap(),
            "\"channel\""
        );
    }

    #[test]
    fn concurrency_strategy_serializes_to_upstream_strings() {
        for (value, wire) in [
            (ConcurrencyStrategy::Drop, "drop"),
            (ConcurrencyStrategy::Queue, "queue"),
            (ConcurrencyStrategy::Debounce, "debounce"),
            (ConcurrencyStrategy::Burst, "burst"),
            (ConcurrencyStrategy::Concurrent, "concurrent"),
        ] {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn fetch_direction_serializes_to_upstream_strings() {
        assert_eq!(
            serde_json::to_string(&FetchDirection::Forward).unwrap(),
            "\"forward\""
        );
        assert_eq!(
            serde_json::to_string(&FetchDirection::Backward).unwrap(),
            "\"backward\""
        );
    }

    #[test]
    fn transcript_role_serializes_to_upstream_strings() {
        for (value, wire) in [
            (TranscriptRole::User, "user"),
            (TranscriptRole::Assistant, "assistant"),
            (TranscriptRole::System, "system"),
        ] {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn well_known_emoji_serializes_to_snake_case_shortcodes() {
        assert_eq!(
            serde_json::to_string(&WellKnownEmoji::ThumbsUp).unwrap(),
            "\"thumbs_up\""
        );
        assert_eq!(
            serde_json::to_string(&WellKnownEmoji::LoveEyes).unwrap(),
            "\"love_eyes\""
        );
        assert_eq!(
            serde_json::to_string(&WellKnownEmoji::SpeechBubble).unwrap(),
            "\"speech_bubble\""
        );
        assert_eq!(
            serde_json::to_string(&WellKnownEmoji::MindBlown).unwrap(),
            "\"mind_blown\""
        );
    }

    #[test]
    fn well_known_emoji_100_shortcode_uses_numeric_literal() {
        // Upstream variant `"100"` is a numeric-literal string. Rust
        // identifiers can't start with a digit, so the variant is named
        // `OneHundred` with a #[serde(rename = "100")] attribute.
        assert_eq!(
            serde_json::to_string(&WellKnownEmoji::OneHundred).unwrap(),
            "\"100\""
        );
        let parsed: WellKnownEmoji = serde_json::from_str("\"100\"").unwrap();
        assert_eq!(parsed, WellKnownEmoji::OneHundred);
    }

    #[test]
    fn thread_state_ttl_ms_matches_upstream_constant() {
        // Upstream: 30 * 24 * 60 * 60 * 1000 = 2_592_000_000.
        assert_eq!(THREAD_STATE_TTL_MS, 2_592_000_000);
    }

    #[test]
    fn string_or_list_round_trips_single_and_array() {
        let one = StringOrList::One("thumbs_up".to_string());
        assert_eq!(serde_json::to_string(&one).unwrap(), "\"thumbs_up\"");
        let back: StringOrList = serde_json::from_str("\"thumbs_up\"").unwrap();
        assert_eq!(back, one);

        let many = StringOrList::Many(vec!["+1".to_string(), "thumbs_up".to_string()]);
        assert_eq!(
            serde_json::to_string(&many).unwrap(),
            "[\"+1\",\"thumbs_up\"]"
        );
        let back: StringOrList = serde_json::from_str("[\"+1\",\"thumbs_up\"]").unwrap();
        assert_eq!(back, many);
    }

    #[test]
    fn emoji_formats_serializes_with_string_union() {
        let formats = EmojiFormats {
            gchat: StringOrList::One("👍".to_string()),
            slack: StringOrList::Many(vec!["+1".to_string(), "thumbs_up".to_string()]),
        };
        let json = serde_json::to_string(&formats).unwrap();
        assert_eq!(json, "{\"gchat\":\"👍\",\"slack\":[\"+1\",\"thumbs_up\"]}");
        let back: EmojiFormats = serde_json::from_str(&json).unwrap();
        assert_eq!(back, formats);
    }

    #[test]
    fn emoji_well_known_and_custom_are_untagged_strings() {
        assert_eq!(
            serde_json::to_string(&Emoji::WellKnown(WellKnownEmoji::ThumbsUp)).unwrap(),
            "\"thumbs_up\""
        );
        assert_eq!(
            serde_json::to_string(&Emoji::Custom("my_logo".to_string())).unwrap(),
            "\"my_logo\""
        );

        // Untagged round-trip: known shortcodes prefer the WellKnown variant,
        // unknown strings fall through to Custom.
        let known: Emoji = serde_json::from_str("\"thumbs_up\"").unwrap();
        assert_eq!(known, Emoji::WellKnown(WellKnownEmoji::ThumbsUp));
        let custom: Emoji = serde_json::from_str("\"my_logo\"").unwrap();
        assert_eq!(custom, Emoji::Custom("my_logo".to_string()));
    }

    #[test]
    fn emoji_map_config_serializes_as_object() {
        let mut config: EmojiMapConfig = std::collections::HashMap::new();
        config.insert(
            Emoji::Custom("custom_emoji".to_string()),
            EmojiFormats {
                gchat: StringOrList::One("🎯".to_string()),
                slack: StringOrList::One("custom".to_string()),
            },
        );
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"custom_emoji\""));
        let back: EmojiMapConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn emoji_value_to_string_and_placeholder_match_upstream() {
        let value = EmojiValue::new("thumbs_up");
        assert_eq!(value.name, "thumbs_up");
        assert_eq!(value.placeholder(), "{{emoji:thumbs_up}}");
        assert_eq!(value.to_string(), "{{emoji:thumbs_up}}");
    }

    #[test]
    fn emoji_value_serializes_as_placeholder_string() {
        let value = EmojiValue::new("heart");
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"{{emoji:heart}}\"");
        let back: EmojiValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn emoji_value_deserialization_rejects_malformed_placeholder() {
        let bad = serde_json::from_str::<EmojiValue>("\"thumbs_up\"");
        assert!(
            bad.is_err(),
            "expected placeholder without the {{{{emoji:...}}}} wrapper to fail"
        );
    }

    #[test]
    fn lock_round_trips_with_camelcase_fields() {
        // Upstream JSON shape uses camelCase keys.
        let lock = Lock {
            expires_at: 1_700_000_000_000,
            thread_id: "T123".to_string(),
            token: "tok_abc".to_string(),
        };
        let json = serde_json::to_string(&lock).unwrap();
        assert_eq!(
            json,
            "{\"expiresAt\":1700000000000,\"threadId\":\"T123\",\"token\":\"tok_abc\"}"
        );
        let back: Lock = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lock);
    }

    #[test]
    fn queue_full_policy_uses_upstream_kebab_strings() {
        assert_eq!(
            serde_json::to_string(&QueueFullPolicy::DropOldest).unwrap(),
            "\"drop-oldest\""
        );
        assert_eq!(
            serde_json::to_string(&QueueFullPolicy::DropNewest).unwrap(),
            "\"drop-newest\""
        );
    }

    #[test]
    fn concurrency_config_round_trips_minimum_shape() {
        // Strategy-only — every other field omitted.
        let config = ConcurrencyConfig {
            debounce_ms: None,
            max_concurrent: None,
            max_queue_size: None,
            on_queue_full: None,
            queue_entry_ttl_ms: None,
            strategy: ConcurrencyStrategy::Queue,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(json, "{\"strategy\":\"queue\"}");
        let back: ConcurrencyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn concurrency_config_round_trips_full_shape() {
        let config = ConcurrencyConfig {
            debounce_ms: Some(1500),
            max_concurrent: Some(4),
            max_queue_size: Some(10),
            on_queue_full: Some(QueueFullPolicy::DropOldest),
            queue_entry_ttl_ms: Some(90_000),
            strategy: ConcurrencyStrategy::Burst,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"debounceMs\":1500"));
        assert!(json.contains("\"maxConcurrent\":4"));
        assert!(json.contains("\"maxQueueSize\":10"));
        assert!(json.contains("\"onQueueFull\":\"drop-oldest\""));
        assert!(json.contains("\"queueEntryTtlMs\":90000"));
        assert!(json.contains("\"strategy\":\"burst\""));
        let back: ConcurrencyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn bot_status_serializes_as_bool_or_unknown_string() {
        assert_eq!(serde_json::to_string(&BotStatus::TRUE).unwrap(), "true");
        assert_eq!(serde_json::to_string(&BotStatus::FALSE).unwrap(), "false");
        assert_eq!(
            serde_json::to_string(&BotStatus::UNKNOWN).unwrap(),
            "\"unknown\""
        );

        let true_back: BotStatus = serde_json::from_str("true").unwrap();
        assert_eq!(true_back, BotStatus::TRUE);
        let false_back: BotStatus = serde_json::from_str("false").unwrap();
        assert_eq!(false_back, BotStatus::FALSE);
        let unknown_back: BotStatus = serde_json::from_str("\"unknown\"").unwrap();
        assert_eq!(unknown_back, BotStatus::UNKNOWN);
    }

    #[test]
    fn author_round_trips_camelcase_with_bot_status_union() {
        let author = Author {
            full_name: "Ada Lovelace".to_string(),
            is_bot: BotStatus::UNKNOWN,
            is_me: false,
            user_id: "U_ADA".to_string(),
            user_name: "ada".to_string(),
        };
        let json = serde_json::to_string(&author).unwrap();
        assert!(json.contains("\"isBot\":\"unknown\""));
        assert!(json.contains("\"fullName\":\"Ada Lovelace\""));
        let back: Author = serde_json::from_str(&json).unwrap();
        assert_eq!(back, author);
    }

    #[test]
    fn user_info_omits_absent_optional_fields() {
        let user = UserInfo {
            avatar_url: None,
            email: None,
            full_name: "Grace".to_string(),
            is_bot: false,
            user_id: "U_GRACE".to_string(),
            user_name: "grace".to_string(),
        };
        let json = serde_json::to_string(&user).unwrap();
        assert!(!json.contains("avatarUrl"));
        assert!(!json.contains("email"));
        assert!(json.contains("\"fullName\":\"Grace\""));
        let back: UserInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, user);
    }

    #[test]
    fn duration_string_parses_valid_shapes() {
        let cases = [
            ("45s", 45, DurationUnit::Seconds),
            ("30m", 30, DurationUnit::Minutes),
            ("2h", 2, DurationUnit::Hours),
            ("7d", 7, DurationUnit::Days),
            ("0s", 0, DurationUnit::Seconds),
            ("123456d", 123456, DurationUnit::Days),
        ];
        for (input, value, unit) in cases {
            let d = DurationString::parse(input).unwrap();
            assert_eq!(d.as_str(), input);
            assert_eq!(d.value(), value);
            assert_eq!(d.unit(), unit);
            assert_eq!(d.to_string(), input);
        }
    }

    #[test]
    fn duration_string_rejects_invalid_shapes() {
        for bad in [
            "",     // empty
            "s",    // missing number
            "5",    // missing unit
            "5x",   // invalid unit
            "ms",   // non-numeric prefix
            "5dd",  // trailing chars
            "-1d",  // negative
            "1.5d", // decimal
        ] {
            assert!(
                DurationString::parse(bad).is_err(),
                "expected {bad:?} to fail to parse"
            );
        }
    }

    #[test]
    fn duration_string_from_parts_round_trips_through_parse() {
        let built = DurationString::from_parts(15, DurationUnit::Minutes);
        assert_eq!(built.as_str(), "15m");
        let reparsed = DurationString::parse(built.as_str()).unwrap();
        assert_eq!(built, reparsed);
    }

    #[test]
    fn duration_string_serializes_as_plain_string() {
        let d = DurationString::parse("7d").unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"7d\"");
        let back: DurationString = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        assert!(serde_json::from_str::<DurationString>("\"5x\"").is_err());
    }

    #[test]
    fn retention_policy_discriminates_number_and_duration_string() {
        let millis = RetentionPolicy::Millis(60_000);
        assert_eq!(serde_json::to_string(&millis).unwrap(), "60000");
        let parsed: RetentionPolicy = serde_json::from_str("60000").unwrap();
        assert_eq!(parsed, millis);

        let duration = RetentionPolicy::Duration(DurationString::parse("7d").unwrap());
        assert_eq!(serde_json::to_string(&duration).unwrap(), "\"7d\"");
        let parsed: RetentionPolicy = serde_json::from_str("\"7d\"").unwrap();
        assert_eq!(parsed, duration);
    }

    #[test]
    fn channel_info_minimum_round_trips() {
        let info = ChannelInfo {
            channel_visibility: None,
            id: "C123".to_string(),
            is_dm: None,
            member_count: None,
            metadata: serde_json::Map::new(),
            name: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(json, "{\"id\":\"C123\",\"metadata\":{}}");
        let back: ChannelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn channel_info_full_shape_round_trips_with_camelcase() {
        let mut metadata = serde_json::Map::new();
        metadata.insert("platform".to_string(), serde_json::json!("slack"));
        let info = ChannelInfo {
            channel_visibility: Some(ChannelVisibility::Workspace),
            id: "C456".to_string(),
            is_dm: Some(false),
            member_count: Some(42),
            metadata,
            name: Some("general".to_string()),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"channelVisibility\":\"workspace\""));
        assert!(json.contains("\"isDM\":false"));
        assert!(json.contains("\"memberCount\":42"));
        assert!(json.contains("\"metadata\":{\"platform\":\"slack\"}"));
        assert!(json.contains("\"name\":\"general\""));
        let back: ChannelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn postable_raw_minimum_shape_round_trips_with_only_raw_text() {
        let postable = PostableRaw {
            attachments: None,
            files: None,
            raw: "hello world".to_string(),
        };
        let json = serde_json::to_string(&postable).unwrap();
        assert_eq!(json, "{\"raw\":\"hello world\"}");
        let back: PostableRaw = serde_json::from_str(&json).unwrap();
        assert_eq!(back, postable);
    }

    #[test]
    fn postable_raw_full_shape_with_attachments_and_files() {
        let postable = PostableRaw {
            attachments: Some(vec![Attachment {
                data: None,
                fetch_metadata: None,
                height: None,
                mime_type: None,
                name: None,
                size: None,
                kind: AttachmentKind::Image,
                url: Some("https://example.com/img.png".to_string()),
                width: None,
            }]),
            files: Some(vec![FileUpload {
                data: FileBytes::from(b"hello"[..].to_vec()),
                filename: "hello.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
            }]),
            raw: "raw".to_string(),
        };
        let json = serde_json::to_string(&postable).unwrap();
        assert!(json.contains("\"attachments\":["));
        assert!(json.contains("\"files\":["));
        assert!(json.contains("\"raw\":\"raw\""));
        let back: PostableRaw = serde_json::from_str(&json).unwrap();
        assert_eq!(back, postable);
    }

    #[test]
    fn postable_markdown_distinct_from_postable_raw_on_the_wire() {
        // Critical for the platform dispatcher: PostableMarkdown emits a
        // `markdown` key, PostableRaw emits a `raw` key. The two are NOT
        // interchangeable — adapters branch on which key is present.
        let md = PostableMarkdown {
            attachments: None,
            files: None,
            markdown: "**hi**".to_string(),
        };
        let raw = PostableRaw {
            attachments: None,
            files: None,
            raw: "**hi**".to_string(),
        };
        let md_json = serde_json::to_string(&md).unwrap();
        let raw_json = serde_json::to_string(&raw).unwrap();
        assert_eq!(md_json, "{\"markdown\":\"**hi**\"}");
        assert_eq!(raw_json, "{\"raw\":\"**hi**\"}");
        assert_ne!(md_json, raw_json);
    }

    #[test]
    fn attachment_kind_uses_upstream_lowercase_strings() {
        for (kind, wire) in [
            (AttachmentKind::Image, "image"),
            (AttachmentKind::File, "file"),
            (AttachmentKind::Video, "video"),
            (AttachmentKind::Audio, "audio"),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{wire}\""));
        }
    }

    #[test]
    fn attachment_minimum_shape_round_trips_with_only_kind() {
        let att = Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: None,
            name: None,
            size: None,
            kind: AttachmentKind::File,
            url: None,
            width: None,
        };
        let json = serde_json::to_string(&att).unwrap();
        assert_eq!(json, "{\"type\":\"file\"}");
        let back: Attachment = serde_json::from_str(&json).unwrap();
        assert_eq!(back, att);
    }

    #[test]
    fn attachment_full_shape_round_trips_with_all_fields() {
        let mut fetch_meta = std::collections::HashMap::new();
        fetch_meta.insert("mediaId".to_string(), "media_123".to_string());
        let att = Attachment {
            data: Some(FileBytes::from(vec![1, 2, 3])),
            fetch_metadata: Some(fetch_meta),
            height: Some(720),
            mime_type: Some("image/png".to_string()),
            name: Some("screenshot.png".to_string()),
            size: Some(4096),
            kind: AttachmentKind::Image,
            url: Some("https://example.com/x.png".to_string()),
            width: Some(1280),
        };
        let json = serde_json::to_string(&att).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        assert!(json.contains("\"data\":[1,2,3]"));
        assert!(json.contains("\"fetchMetadata\":{\"mediaId\":\"media_123\"}"));
        assert!(json.contains("\"mimeType\":\"image/png\""));
        assert!(json.contains("\"height\":720"));
        assert!(json.contains("\"width\":1280"));
        let back: Attachment = serde_json::from_str(&json).unwrap();
        assert_eq!(back, att);
    }

    #[test]
    fn link_preview_minimum_shape_round_trips_with_only_url() {
        let preview = LinkPreview {
            description: None,
            image_url: None,
            site_name: None,
            title: None,
            url: "https://example.com".to_string(),
        };
        let json = serde_json::to_string(&preview).unwrap();
        assert_eq!(json, "{\"url\":\"https://example.com\"}");
        let back: LinkPreview = serde_json::from_str(&json).unwrap();
        assert_eq!(back, preview);
    }

    #[test]
    fn link_preview_full_shape_uses_upstream_camelcase() {
        let preview = LinkPreview {
            description: Some("A description".to_string()),
            image_url: Some("https://example.com/img.png".to_string()),
            site_name: Some("Vercel".to_string()),
            title: Some("Some title".to_string()),
            url: "https://example.com/post".to_string(),
        };
        let json = serde_json::to_string(&preview).unwrap();
        assert!(json.contains("\"imageUrl\":\"https://example.com/img.png\""));
        assert!(json.contains("\"siteName\":\"Vercel\""));
        let back: LinkPreview = serde_json::from_str(&json).unwrap();
        assert_eq!(back, preview);
    }

    #[test]
    fn append_input_round_trips_with_formatted_opaque_value() {
        // FormattedContent today is serde_json::Value (see placeholder
        // note). The wire shape must accept *any* JSON value there —
        // future markdown-crate decision swaps the type but preserves
        // this shape.
        let input = AppendInput {
            formatted: Some(serde_json::json!({"type": "root", "children": []})),
            platform_message_id: Some("M1".to_string()),
            role: TranscriptRole::Assistant,
            text: "hi".to_string(),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("\"role\":\"assistant\""));
        assert!(json.contains("\"platformMessageId\":\"M1\""));
        assert!(json.contains("\"text\":\"hi\""));
        assert!(json.contains("\"formatted\":{"));
        let back: AppendInput = serde_json::from_str(&json).unwrap();
        assert_eq!(back, input);
    }

    #[test]
    fn append_input_omits_optional_fields_when_absent() {
        let input = AppendInput {
            formatted: None,
            platform_message_id: None,
            role: TranscriptRole::User,
            text: "msg".to_string(),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(!json.contains("formatted"));
        assert!(!json.contains("platformMessageId"));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"text\":\"msg\""));
    }

    #[test]
    fn transcript_entry_round_trips_camelcase_keys_with_formatted_value() {
        let entry = TranscriptEntry {
            formatted: Some(serde_json::json!({"type": "root"})),
            id: "uuid_1".to_string(),
            platform: "slack".to_string(),
            platform_message_id: Some("M2".to_string()),
            role: TranscriptRole::User,
            text: "hello".to_string(),
            thread_id: "T9".to_string(),
            timestamp: 1_700_000_000_000,
            user_key: "user_x".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"platformMessageId\":\"M2\""));
        assert!(json.contains("\"threadId\":\"T9\""));
        assert!(json.contains("\"userKey\":\"user_x\""));
        assert!(json.contains("\"timestamp\":1700000000000"));
        assert!(json.contains("\"formatted\":{"));
        let back: TranscriptEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn transcript_entry_omits_formatted_when_storage_disabled() {
        let entry = TranscriptEntry {
            formatted: None,
            id: "uuid_2".to_string(),
            platform: "teams".to_string(),
            platform_message_id: None,
            role: TranscriptRole::Assistant,
            text: "reply".to_string(),
            thread_id: "T10".to_string(),
            timestamp: 1,
            user_key: "user_y".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("formatted"));
        assert!(!json.contains("platformMessageId"));
    }

    #[test]
    fn post_ephemeral_options_round_trips_camelcase_field() {
        let yes = PostEphemeralOptions {
            fallback_to_dm: true,
        };
        assert_eq!(
            serde_json::to_string(&yes).unwrap(),
            "{\"fallbackToDM\":true}"
        );
        let no = PostEphemeralOptions {
            fallback_to_dm: false,
        };
        assert_eq!(
            serde_json::to_string(&no).unwrap(),
            "{\"fallbackToDM\":false}"
        );
        let back: PostEphemeralOptions = serde_json::from_str("{\"fallbackToDM\":true}").unwrap();
        assert_eq!(back, yes);
    }

    #[test]
    fn lock_scope_context_holds_arc_dyn_adapter_and_renders_stable_debug() {
        // Mock adapter using the default impls of the Phase 1.5
        // Adapter trait methods (slice 122) — only `name` needs to
        // be supplied; the four async methods take the trait defaults.
        #[derive(Debug)]
        struct MockAdapter;
        #[async_trait::async_trait]
        impl Adapter for MockAdapter {
            fn name(&self) -> &str {
                "mock"
            }
        }

        let ctx = LockScopeContext {
            adapter: std::sync::Arc::new(MockAdapter) as std::sync::Arc<dyn Adapter>,
            channel_id: "C1".to_string(),
            is_dm: false,
            thread_id: "T1".to_string(),
        };
        let dbg = format!("{ctx:?}");
        // Debug rendering stays stable regardless of which adapter is
        // plugged in — slice 14's placeholder trait does not commit us
        // to a specific Debug impl.
        assert!(dbg.contains("LockScopeContext"));
        assert!(dbg.contains("adapter: \"<dyn Adapter>\""));
        assert!(dbg.contains("channel_id: \"C1\""));
        assert!(dbg.contains("is_dm: false"));
        assert!(dbg.contains("thread_id: \"T1\""));
    }

    #[test]
    fn file_bytes_serializes_transparently_as_byte_array() {
        // serde encodes Vec<u8> as a JSON number array by default; serde's
        // `transparent` representation preserves that so FileBytes is
        // wire-compatible with a raw byte array.
        let bytes = FileBytes::from(vec![0x68u8, 0x69]); // "hi"
        let json = serde_json::to_string(&bytes).unwrap();
        assert_eq!(json, "[104,105]");
        let back: FileBytes = serde_json::from_str(&json).unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn file_bytes_derefs_to_byte_slice_and_constructs_from_slice() {
        let bytes = FileBytes::from(&b"hello"[..]);
        assert_eq!(&*bytes, b"hello");
        assert_eq!(bytes.0.len(), 5);
    }

    #[test]
    fn file_upload_round_trips_with_optional_mime_type() {
        let upload = FileUpload {
            data: FileBytes::from(vec![1, 2, 3]),
            filename: "report.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
        };
        let json = serde_json::to_string(&upload).unwrap();
        assert!(json.contains("\"filename\":\"report.pdf\""));
        assert!(json.contains("\"mimeType\":\"application/pdf\""));
        assert!(json.contains("\"data\":[1,2,3]"));
        let back: FileUpload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, upload);
    }

    #[test]
    fn file_upload_omits_mime_type_when_absent() {
        let upload = FileUpload {
            data: FileBytes::from(vec![]),
            filename: "blank.bin".to_string(),
            mime_type: None,
        };
        let json = serde_json::to_string(&upload).unwrap();
        assert!(!json.contains("mimeType"));
    }

    #[test]
    fn fetch_options_default_serializes_empty() {
        let opts = FetchOptions::default();
        assert_eq!(serde_json::to_string(&opts).unwrap(), "{}");
    }

    #[test]
    fn fetch_options_full_shape_round_trips() {
        let opts = FetchOptions {
            cursor: Some("opaque_cursor".to_string()),
            direction: Some(FetchDirection::Forward),
            limit: Some(75),
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("\"cursor\":\"opaque_cursor\""));
        assert!(json.contains("\"direction\":\"forward\""));
        assert!(json.contains("\"limit\":75"));
        let back: FetchOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, opts);
    }

    #[test]
    fn raw_message_round_trips_with_default_value_payload() {
        let msg: RawMessage<serde_json::Value> = RawMessage {
            id: "M1".to_string(),
            raw: serde_json::json!({ "platform": "slack", "ts": "1.0" }),
            thread_id: "T1".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"id\":\"M1\""));
        assert!(json.contains("\"threadId\":\"T1\""));
        assert!(json.contains("\"raw\":{"));
        let back: RawMessage<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn raw_message_works_with_a_typed_payload() {
        // Smoke-test that the generic accepts a concrete typed payload —
        // future adapter slices will substitute Slack/Teams/etc. types
        // here in place of `serde_json::Value`.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        struct SlackBody {
            channel: String,
            ts: String,
        }
        let msg: RawMessage<SlackBody> = RawMessage {
            id: "M2".to_string(),
            raw: SlackBody {
                channel: "C123".to_string(),
                ts: "1.0".to_string(),
            },
            thread_id: "T2".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: RawMessage<SlackBody> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn append_options_default_serializes_empty() {
        let opts = AppendOptions::default();
        assert_eq!(serde_json::to_string(&opts).unwrap(), "{}");
    }

    #[test]
    fn append_options_with_user_key_round_trips_camelcase() {
        let opts = AppendOptions {
            user_key: Some("user_abc".to_string()),
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert_eq!(json, "{\"userKey\":\"user_abc\"}");
        let back: AppendOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, opts);
    }

    #[test]
    fn list_query_required_user_key_with_full_filters() {
        let query = ListQuery {
            limit: Some(50),
            platforms: Some(vec!["slack".to_string(), "teams".to_string()]),
            roles: Some(vec![TranscriptRole::User, TranscriptRole::Assistant]),
            thread_id: Some("T_999".to_string()),
            user_key: "user_x".to_string(),
        };
        let json = serde_json::to_string(&query).unwrap();
        assert!(json.contains("\"limit\":50"));
        assert!(json.contains("\"platforms\":[\"slack\",\"teams\"]"));
        assert!(json.contains("\"roles\":[\"user\",\"assistant\"]"));
        assert!(json.contains("\"threadId\":\"T_999\""));
        assert!(json.contains("\"userKey\":\"user_x\""));
        let back: ListQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(back, query);
    }

    #[test]
    fn list_query_minimum_shape_only_emits_user_key() {
        let query = ListQuery {
            limit: None,
            platforms: None,
            roles: None,
            thread_id: None,
            user_key: "user_y".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&query).unwrap(),
            "{\"userKey\":\"user_y\"}"
        );
    }

    #[test]
    fn delete_target_and_count_query_round_trip_with_camelcase_user_key() {
        let dt = DeleteTarget {
            user_key: "user_a".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&dt).unwrap(),
            "{\"userKey\":\"user_a\"}"
        );
        let cq = CountQuery {
            user_key: "user_b".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&cq).unwrap(),
            "{\"userKey\":\"user_b\"}"
        );
        let back: DeleteTarget = serde_json::from_str("{\"userKey\":\"user_a\"}").unwrap();
        assert_eq!(back, dt);
        let back: CountQuery = serde_json::from_str("{\"userKey\":\"user_b\"}").unwrap();
        assert_eq!(back, cq);
    }

    #[test]
    fn message_subject_minimum_shape_round_trips() {
        let subj = MessageSubject {
            assignee: None,
            author: None,
            description: None,
            id: "I123".to_string(),
            labels: None,
            raw: serde_json::Value::Null,
            status: None,
            title: None,
            kind: "issue".to_string(),
            url: None,
        };
        let json = serde_json::to_string(&subj).unwrap();
        // `raw: null` and required `id`/`type` must appear; everything
        // optional must be elided.
        assert_eq!(json, "{\"id\":\"I123\",\"raw\":null,\"type\":\"issue\"}");
        let back: MessageSubject = serde_json::from_str(&json).unwrap();
        assert_eq!(back, subj);
    }

    #[test]
    fn message_subject_full_shape_uses_upstream_inline_actor_shape() {
        let mut raw = serde_json::Map::new();
        raw.insert("source".to_string(), serde_json::json!("github"));
        let subj = MessageSubject {
            assignee: Some(MessageSubjectActor {
                id: "U1".to_string(),
                name: "ada".to_string(),
            }),
            author: Some(MessageSubjectActor {
                id: "U2".to_string(),
                name: "grace".to_string(),
            }),
            description: Some("desc".to_string()),
            id: "I9".to_string(),
            labels: Some(vec!["bug".to_string(), "p0".to_string()]),
            raw: serde_json::Value::Object(raw),
            status: Some("open".to_string()),
            title: Some("a title".to_string()),
            kind: "pr".to_string(),
            url: Some("https://example.com/pr/9".to_string()),
        };
        let json = serde_json::to_string(&subj).unwrap();
        assert!(json.contains("\"assignee\":{\"id\":\"U1\",\"name\":\"ada\"}"));
        assert!(json.contains("\"author\":{\"id\":\"U2\",\"name\":\"grace\"}"));
        assert!(json.contains("\"labels\":[\"bug\",\"p0\"]"));
        assert!(json.contains("\"type\":\"pr\""));
        assert!(json.contains("\"raw\":{\"source\":\"github\"}"));
        let back: MessageSubject = serde_json::from_str(&json).unwrap();
        assert_eq!(back, subj);
    }

    #[test]
    fn thread_info_round_trips_camelcase_keys() {
        let mut metadata = serde_json::Map::new();
        metadata.insert("platform".to_string(), serde_json::json!("slack"));
        let info = ThreadInfo {
            channel_id: "C123".to_string(),
            channel_name: Some("general".to_string()),
            channel_visibility: Some(ChannelVisibility::Workspace),
            id: "T999".to_string(),
            is_dm: Some(false),
            metadata,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"channelId\":\"C123\""));
        assert!(json.contains("\"channelName\":\"general\""));
        assert!(json.contains("\"channelVisibility\":\"workspace\""));
        assert!(json.contains("\"isDM\":false"));
        assert!(json.contains("\"metadata\":{\"platform\":\"slack\"}"));
        let back: ThreadInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn task_status_uses_upstream_snake_case_strings() {
        for (status, wire) in [
            (TaskStatus::Pending, "pending"),
            (TaskStatus::InProgress, "in_progress"),
            (TaskStatus::Complete, "complete"),
            (TaskStatus::Error, "error"),
        ] {
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                format!("\"{wire}\"")
            );
        }
    }

    #[test]
    fn task_display_mode_uses_upstream_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&TaskDisplayMode::Timeline).unwrap(),
            "\"timeline\""
        );
        assert_eq!(
            serde_json::to_string(&TaskDisplayMode::Plan).unwrap(),
            "\"plan\""
        );
    }

    #[test]
    fn stream_chunk_markdown_text_serializes_with_tagged_type() {
        let chunk = StreamChunk::MarkdownText(MarkdownTextChunkData {
            text: "hello".to_string(),
        });
        let json = serde_json::to_string(&chunk).unwrap();
        assert_eq!(json, "{\"type\":\"markdown_text\",\"text\":\"hello\"}");
        let back: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back, chunk);
    }

    #[test]
    fn stream_chunk_task_update_round_trips_full_shape() {
        let chunk = StreamChunk::TaskUpdate(TaskUpdateChunkData {
            details: Some("running".to_string()),
            id: "T1".to_string(),
            output: Some("done".to_string()),
            status: TaskStatus::Complete,
            title: "deploy".to_string(),
        });
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"type\":\"task_update\""));
        assert!(json.contains("\"status\":\"complete\""));
        assert!(json.contains("\"id\":\"T1\""));
        assert!(json.contains("\"title\":\"deploy\""));
        let back: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(back, chunk);
    }

    #[test]
    fn stream_chunk_task_update_omits_absent_optionals() {
        let chunk = StreamChunk::TaskUpdate(TaskUpdateChunkData {
            details: None,
            id: "T2".to_string(),
            output: None,
            status: TaskStatus::Pending,
            title: "warm-up".to_string(),
        });
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(!json.contains("details"));
        assert!(!json.contains("output"));
    }

    #[test]
    fn stream_chunk_plan_update_serializes_with_only_title_and_tag() {
        let chunk = StreamChunk::PlanUpdate(PlanUpdateChunkData {
            title: "rollout".to_string(),
        });
        let json = serde_json::to_string(&chunk).unwrap();
        assert_eq!(json, "{\"type\":\"plan_update\",\"title\":\"rollout\"}");
    }

    #[test]
    fn stream_chunk_untagged_object_fails_to_deserialize() {
        // Missing the `type` discriminator is a hard error, just like
        // upstream's tagged TS union would reject it at compile time.
        let bad = serde_json::from_str::<StreamChunk>("{\"text\":\"hi\"}");
        assert!(bad.is_err());
    }

    #[test]
    fn stream_options_default_serializes_empty_and_round_trips() {
        let opts = StreamOptions::default();
        assert_eq!(serde_json::to_string(&opts).unwrap(), "{}");
        let back: StreamOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(back, opts);
    }

    #[test]
    fn stream_options_full_shape_uses_upstream_camelcase_and_strings() {
        let opts = StreamOptions {
            recipient_team_id: Some("T_TEAM".to_string()),
            recipient_user_id: Some("U_USER".to_string()),
            stop_blocks: Some(vec![serde_json::json!({"type": "divider"})]),
            task_display_mode: Some(TaskDisplayMode::Plan),
            update_interval_ms: Some(750),
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("\"recipientTeamId\":\"T_TEAM\""));
        assert!(json.contains("\"recipientUserId\":\"U_USER\""));
        assert!(json.contains("\"stopBlocks\":[{\"type\":\"divider\"}]"));
        assert!(json.contains("\"taskDisplayMode\":\"plan\""));
        assert!(json.contains("\"updateIntervalMs\":750"));
        let back: StreamOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, opts);
    }

    #[test]
    fn list_threads_options_default_serializes_empty() {
        let opts = ListThreadsOptions::default();
        assert_eq!(serde_json::to_string(&opts).unwrap(), "{}");
    }

    #[test]
    fn list_threads_options_full_shape_round_trips() {
        let opts = ListThreadsOptions {
            cursor: Some("opaque_cursor".to_string()),
            limit: Some(25),
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("\"cursor\":\"opaque_cursor\""));
        assert!(json.contains("\"limit\":25"));
        let back: ListThreadsOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, opts);
    }

    #[test]
    fn transcripts_config_minimum_and_full_shapes_round_trip() {
        let default_cfg = TranscriptsConfig::default();
        assert_eq!(serde_json::to_string(&default_cfg).unwrap(), "{}");

        let full = TranscriptsConfig {
            max_per_user: Some(100),
            retention: Some(RetentionPolicy::Duration(
                DurationString::parse("30d").unwrap(),
            )),
            store_formatted: Some(true),
        };
        let json = serde_json::to_string(&full).unwrap();
        assert!(json.contains("\"maxPerUser\":100"));
        assert!(json.contains("\"retention\":\"30d\""));
        assert!(json.contains("\"storeFormatted\":true"));
        let back: TranscriptsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, full);
    }

    #[test]
    fn user_info_preserves_present_optional_fields() {
        let user = UserInfo {
            avatar_url: Some("https://example.com/a.png".to_string()),
            email: Some("ada@example.com".to_string()),
            full_name: "Ada".to_string(),
            is_bot: false,
            user_id: "U_ADA".to_string(),
            user_name: "ada".to_string(),
        };
        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains("\"avatarUrl\":\"https://example.com/a.png\""));
        assert!(json.contains("\"email\":\"ada@example.com\""));
        let back: UserInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, user);
    }

    #[test]
    fn postable_ast_round_trips_through_serde() {
        let ast = PostableAst {
            ast: crate::markdown::root(vec![crate::markdown::Node::Paragraph(
                crate::markdown::paragraph(vec![crate::markdown::Node::Text(
                    crate::markdown::text("hello"),
                )]),
            )]),
            attachments: None,
            files: None,
        };
        let json = serde_json::to_string(&ast).unwrap();
        let back: PostableAst = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ast);
    }

    #[test]
    fn postable_card_round_trips_through_serde() {
        let card = crate::cards::card(crate::cards::CardOptions {
            title: Some("Test".to_string()),
            ..Default::default()
        });
        let pc = PostableCard {
            card,
            fallback_text: Some("plain".to_string()),
            files: None,
        };
        let json = serde_json::to_string(&pc).unwrap();
        assert!(json.contains("\"fallbackText\":\"plain\""));
        let back: PostableCard = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pc);
    }

    #[test]
    fn adapter_postable_message_dispatches_each_variant_through_untagged_serde() {
        // Raw
        let raw_json = r#"{"raw":"hi"}"#;
        let raw: AdapterPostableMessage = serde_json::from_str(raw_json).unwrap();
        assert!(matches!(raw, AdapterPostableMessage::Raw(_)));

        // Markdown
        let md_json = r#"{"markdown":"**hi**"}"#;
        let md: AdapterPostableMessage = serde_json::from_str(md_json).unwrap();
        assert!(matches!(md, AdapterPostableMessage::Markdown(_)));

        // Card (PostableCard wrapper)
        let card = crate::cards::card(crate::cards::CardOptions {
            title: Some("t".to_string()),
            ..Default::default()
        });
        let pc = PostableCard {
            card,
            fallback_text: None,
            files: None,
        };
        let wrapper_json = serde_json::to_string(&pc).unwrap();
        let wrap: AdapterPostableMessage = serde_json::from_str(&wrapper_json).unwrap();
        assert!(matches!(wrap, AdapterPostableMessage::Card(_)));

        // Plain string text
        let s: AdapterPostableMessage = AdapterPostableMessage::from("plain");
        assert!(matches!(s, AdapterPostableMessage::Text(_)));
    }

    // ---------- slice 122: Adapter trait extension ----------

    use futures_executor::block_on;

    /// Bare-minimum adapter that overrides only the required `name`.
    /// Used to exercise the default impls of the four async methods.
    #[derive(Debug)]
    struct UnconfiguredAdapter;

    #[async_trait::async_trait]
    impl Adapter for UnconfiguredAdapter {
        fn name(&self) -> &str {
            "unconfigured"
        }
    }

    /// Adapter that overrides every method with a recognizable
    /// behavior, used to verify the trait surface dispatches.
    #[derive(Debug)]
    struct EchoAdapter;

    #[async_trait::async_trait]
    impl Adapter for EchoAdapter {
        fn name(&self) -> &str {
            "echo"
        }
        async fn fetch_subject(&self, thread_id: &str) -> AdapterResult<Option<String>> {
            Ok(Some(format!("subject:{thread_id}")))
        }
        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            Ok(format!("msg:{thread_id}:{text}"))
        }
        async fn post_object(
            &self,
            thread_id: &str,
            kind: &str,
            _data: serde_json::Value,
        ) -> AdapterResult<String> {
            Ok(format!("obj:{thread_id}:{kind}"))
        }
    }

    #[test]
    fn adapter_default_fetch_subject_returns_none() {
        let a = UnconfiguredAdapter;
        let subject = block_on(a.fetch_subject("t1")).unwrap();
        assert!(subject.is_none());
    }

    #[test]
    fn adapter_default_post_message_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.post_message("t1", "hi")) {
            Err(AdapterError::Unsupported("post_message")) => {}
            other => panic!("expected Unsupported(post_message), got {other:?}"),
        }
    }

    #[test]
    fn adapter_default_post_object_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.post_object("t1", "plan", serde_json::json!({}))) {
            Err(AdapterError::Unsupported("post_object")) => {}
            other => panic!("expected Unsupported(post_object), got {other:?}"),
        }
    }

    #[test]
    fn adapter_default_parse_message_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.parse_message(serde_json::json!({}))) {
            Err(AdapterError::Unsupported("parse_message")) => {}
            other => panic!("expected Unsupported(parse_message), got {other:?}"),
        }
    }

    #[test]
    fn adapter_default_edit_message_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.edit_message("t1", "m1", "hi")) {
            Err(AdapterError::Unsupported("edit_message")) => {}
            other => panic!("expected Unsupported(edit_message), got {other:?}"),
        }
    }

    #[test]
    fn adapter_default_delete_message_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.delete_message("t1", "m1")) {
            Err(AdapterError::Unsupported("delete_message")) => {}
            other => panic!("expected Unsupported(delete_message), got {other:?}"),
        }
    }

    #[test]
    fn adapter_default_add_reaction_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.add_reaction("t1", "m1", "thumbsup")) {
            Err(AdapterError::Unsupported("add_reaction")) => {}
            other => panic!("expected Unsupported(add_reaction), got {other:?}"),
        }
    }

    #[test]
    fn adapter_default_start_typing_returns_unsupported() {
        let a = UnconfiguredAdapter;
        match block_on(a.start_typing("t1", None)) {
            Err(AdapterError::Unsupported("start_typing")) => {}
            other => panic!("expected Unsupported(start_typing), got {other:?}"),
        }
        match block_on(a.start_typing("t1", Some("thinking…"))) {
            Err(AdapterError::Unsupported("start_typing")) => {}
            other => panic!("expected Unsupported(start_typing), got {other:?}"),
        }
    }

    #[test]
    fn adapter_overridden_methods_dispatch_to_the_impl() {
        let a = EchoAdapter;
        assert_eq!(a.name(), "echo");
        assert_eq!(
            block_on(a.fetch_subject("t1")).unwrap().as_deref(),
            Some("subject:t1")
        );
        assert_eq!(block_on(a.post_message("t1", "hi")).unwrap(), "msg:t1:hi");
        assert_eq!(
            block_on(a.post_object("t1", "card", serde_json::json!({}))).unwrap(),
            "obj:t1:card"
        );
    }

    #[test]
    fn adapter_error_display_includes_method_name_for_unsupported() {
        let err = AdapterError::Unsupported("post_message");
        assert_eq!(err.to_string(), "Adapter does not implement `post_message`");
    }

    #[test]
    fn adapter_error_display_includes_payload_message_for_invalid_payload() {
        let err = AdapterError::InvalidPayload("missing field `id`".into());
        assert_eq!(
            err.to_string(),
            "Adapter parsed an invalid payload: missing field `id`"
        );
    }

    // ---------- slice 125: StateAdapter trait extension (locks + set_if_not_exists) ----------

    /// State that exercises the minimal `get`/`set`/`append_to_list`/
    /// `get_list`/`delete` methods. Used to verify the default impls
    /// of the new locks + `set_if_not_exists` trait methods.
    #[derive(Debug, Default)]
    struct MinimalState {
        kv: std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
    }

    #[async_trait::async_trait]
    impl StateAdapter for MinimalState {
        async fn get(&self, key: &str) -> StateResult<Option<serde_json::Value>> {
            Ok(self
                .kv
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(key)
                .cloned())
        }
        async fn set(
            &self,
            key: &str,
            value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            self.kv
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(key.to_string(), value);
            Ok(())
        }
        async fn delete(&self, key: &str) -> StateResult<()> {
            self.kv
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(key);
            Ok(())
        }
        async fn append_to_list(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn get_list(
            &self,
            _key: &str,
            _limit: Option<usize>,
        ) -> StateResult<Vec<serde_json::Value>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn state_adapter_default_set_if_not_exists_writes_when_missing() {
        let state = MinimalState::default();
        let wrote = block_on(state.set_if_not_exists("k", serde_json::json!(1), None)).unwrap();
        assert!(wrote);
        let got = block_on(state.get("k")).unwrap();
        assert_eq!(got, Some(serde_json::json!(1)));
    }

    #[test]
    fn state_adapter_default_set_if_not_exists_no_op_when_present() {
        let state = MinimalState::default();
        block_on(state.set("k", serde_json::json!("first"), None)).unwrap();
        let wrote =
            block_on(state.set_if_not_exists("k", serde_json::json!("second"), None)).unwrap();
        assert!(!wrote);
        let got = block_on(state.get("k")).unwrap();
        assert_eq!(got, Some(serde_json::json!("first")));
    }

    #[test]
    fn state_adapter_default_acquire_lock_returns_none() {
        let state = MinimalState::default();
        let lock = block_on(state.acquire_lock("t1", 1000)).unwrap();
        assert!(lock.is_none());
    }

    #[test]
    fn state_adapter_default_release_lock_is_no_op() {
        let state = MinimalState::default();
        let dummy = Lock {
            expires_at: 0,
            thread_id: "t1".to_string(),
            token: "tok".to_string(),
        };
        block_on(state.release_lock(&dummy)).unwrap();
    }

    #[test]
    fn state_adapter_default_force_release_lock_is_no_op() {
        let state = MinimalState::default();
        block_on(state.force_release_lock("t1")).unwrap();
    }

    #[test]
    fn state_adapter_default_extend_lock_returns_false() {
        let state = MinimalState::default();
        let dummy = Lock {
            expires_at: 0,
            thread_id: "t1".to_string(),
            token: "tok".to_string(),
        };
        let extended = block_on(state.extend_lock(&dummy, 1000)).unwrap();
        assert!(!extended);
    }

    // ---------- Adapter::open_modal — slice 428 ----------
    //
    // Phase A of the deferred-adapter-method 3-slice cadence for
    // the upstream openModal flow. Verifies the trait method
    // default returns Err(Unsupported("open_modal")) and a custom
    // override returns the typed OpenModalResult.

    #[derive(Debug, Default)]
    struct BareAdapterForModal;

    #[async_trait::async_trait]
    impl Adapter for BareAdapterForModal {
        fn name(&self) -> &str {
            "bare"
        }
    }

    #[derive(Debug, Default)]
    struct ModalAdapter {
        calls: std::sync::Mutex<Vec<(String, String, String)>>,
    }

    #[async_trait::async_trait]
    impl Adapter for ModalAdapter {
        fn name(&self) -> &str {
            "modal"
        }
        async fn open_modal(
            &self,
            trigger_id: &str,
            modal: &crate::modals::ModalElement,
            context_id: &str,
        ) -> AdapterResult<OpenModalResult> {
            self.calls.lock().unwrap().push((
                trigger_id.to_string(),
                modal.callback_id.clone(),
                context_id.to_string(),
            ));
            Ok(OpenModalResult {
                view_id: "V_FROM_ADAPTER".to_string(),
            })
        }
    }

    #[test]
    fn adapter_default_open_modal_returns_unsupported_error() {
        let adapter = BareAdapterForModal;
        let modal = crate::modals::modal(crate::modals::ModalOptions {
            callback_id: "test_modal".to_string(),
            title: "Test".to_string(),
            children: Some(Vec::new()),
            ..Default::default()
        });
        let err = block_on(adapter.open_modal("trigger-1", &modal, "ctx-1"))
            .expect_err("expected Unsupported");
        assert!(matches!(err, AdapterError::Unsupported("open_modal")));
    }

    #[test]
    fn adapter_open_modal_override_returns_typed_result() {
        let adapter = ModalAdapter::default();
        let modal = crate::modals::modal(crate::modals::ModalOptions {
            callback_id: "my_modal".to_string(),
            title: "Title".to_string(),
            children: Some(Vec::new()),
            ..Default::default()
        });
        let result = block_on(adapter.open_modal("trigger-1", &modal, "ctx-uuid")).unwrap();
        assert_eq!(result.view_id, "V_FROM_ADAPTER");
        let calls = adapter.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "trigger-1");
        assert_eq!(calls[0].1, "my_modal");
        assert_eq!(calls[0].2, "ctx-uuid");
    }

    #[test]
    fn open_modal_result_round_trips_through_serde_with_camelcase_view_id() {
        let value = OpenModalResult {
            view_id: "V01ABC".to_string(),
        };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, r#"{"viewId":"V01ABC"}"#);
        let back: OpenModalResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.view_id, "V01ABC");
    }
}
