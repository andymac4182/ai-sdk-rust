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
