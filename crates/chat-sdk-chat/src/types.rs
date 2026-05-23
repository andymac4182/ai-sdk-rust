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
/// `message`, `channel`, `thread`, and the JSX runtime. Those methods will
/// land on this trait as their dependency modules are ported. For now the
/// trait is intentionally empty — it exists so types referencing
/// `dyn Adapter` (the singleton holder, the future event payloads, …) can
/// compile against an opaque object. Each adapter slice MUST extend this
/// trait with the upstream method(s) it adds, never define a new trait.
///
/// Implementors must be `Send + Sync` since adapters are shared across
/// async tasks; the supertrait bounds enforce that statically.
pub trait Adapter: Send + Sync + std::fmt::Debug {}

/// State-backend trait — the storage abstraction (`state-memory`,
/// `state-redis`, `state-ioredis`, `state-pg`, …). 1:1 port of upstream
/// `interface StateAdapter`.
///
/// **Layered port.** Upstream `StateAdapter` has ~20 methods covering
/// connect/disconnect, locks, lists, queues, key/value cache, subscriptions.
/// Those land here as the matching `state-*` crates are ported. The trait is
/// intentionally empty today so the singleton and future state-consumer
/// types can hold a `dyn StateAdapter`. Each state-port slice MUST extend
/// this trait, never define a new trait.
pub trait StateAdapter: Send + Sync + std::fmt::Debug {}

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
    /// and as the `toString()` / `toJSON()` value.
    pub fn placeholder(&self) -> String {
        format!(":{}:", self.name)
    }
}

impl std::fmt::Display for EmojiValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.placeholder())
    }
}

impl Serialize for EmojiValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Upstream toJSON() returns the `:name:` placeholder, not the raw
        // name. Preserving that on the wire keeps message-formatting parity.
        serializer.serialize_str(&self.placeholder())
    }
}

impl<'de> Deserialize<'de> for EmojiValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let placeholder = String::deserialize(deserializer)?;
        let name = placeholder
            .strip_prefix(':')
            .and_then(|s| s.strip_suffix(':'))
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "expected EmojiValue placeholder `:name:`, got {placeholder:?}"
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
        assert_eq!(value.placeholder(), ":thumbs_up:");
        assert_eq!(value.to_string(), ":thumbs_up:");
    }

    #[test]
    fn emoji_value_serializes_as_placeholder_string() {
        let value = EmojiValue::new("heart");
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\":heart:\"");
        let back: EmojiValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn emoji_value_deserialization_rejects_malformed_placeholder() {
        let bad = serde_json::from_str::<EmojiValue>("\"thumbs_up\"");
        assert!(
            bad.is_err(),
            "expected placeholder without colons to fail to deserialize"
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
        // Mock adapter with no methods (Adapter is an empty placeholder
        // trait today — see types.rs module docs).
        #[derive(Debug)]
        struct MockAdapter;
        impl Adapter for MockAdapter {}

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
}
