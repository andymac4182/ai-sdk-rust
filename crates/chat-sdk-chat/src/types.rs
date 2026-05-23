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
}
