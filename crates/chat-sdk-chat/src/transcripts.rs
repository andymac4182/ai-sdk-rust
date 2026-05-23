//! Cross-platform per-user transcript-store helpers.
//!
//! 1:1 port (in progress) of `packages/chat/src/transcripts.ts`.
//!
//! **What this slice ships:** the pure helpers used by the upstream
//! [`TranscriptsApiImpl`] class:
//!
//! - [`KEY_PREFIX`] / [`DEFAULT_MAX_PER_USER`] / [`DEFAULT_LIST_LIMIT`]
//!   /[`TOMBSTONE_MARKER`] constants matching upstream values.
//! - [`parse_duration`] — 1:1 port of upstream `parseDuration` that
//!   converts a [`DurationString`](crate::types::DurationString) or a
//!   raw millisecond count to milliseconds, panicking on malformed
//!   input (matching upstream's `throw new Error("Invalid duration: …")`).
//!
//! **What is deferred:** `TranscriptsApiImpl` itself — every `append` /
//! `list` / `delete` / `count` method calls `StateAdapter.appendToList`
//! or `StateAdapter.getList`. Those land once the placeholder
//! [`crate::types::StateAdapter`] trait is extended with concrete async
//! methods (see the state-memory module-header note for the design
//! decision and migration plan).

use crate::types::{DurationString, DurationUnit};

/// Either a raw millisecond count or a validated [`DurationString`].
/// 1:1 port of upstream `number | DurationString`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DurationInput {
    /// Raw millisecond count.
    Millis(u64),
    /// Validated `"<n>(s|m|h|d)"` shorthand.
    String(DurationString),
}

impl From<u64> for DurationInput {
    fn from(value: u64) -> Self {
        Self::Millis(value)
    }
}

impl From<DurationString> for DurationInput {
    fn from(value: DurationString) -> Self {
        Self::String(value)
    }
}

/// State-store key prefix for stored transcripts. 1:1 port of upstream
/// `const KEY_PREFIX = "transcripts:user:"`.
pub const KEY_PREFIX: &str = "transcripts:user:";

/// Default cap on stored transcripts per user. 1:1 port of upstream
/// `const DEFAULT_MAX_PER_USER = 200`.
pub const DEFAULT_MAX_PER_USER: usize = 200;

/// Default page size for [`list`]-style queries. 1:1 port of upstream
/// `const DEFAULT_LIST_LIMIT = 50`.
pub const DEFAULT_LIST_LIMIT: usize = 50;

/// Tombstone marker key. 1:1 port of upstream
/// `const TOMBSTONE_MARKER = "__chatSdkTombstone"`.
///
/// Written by `delete()` so the underlying list is functionally empty
/// without needing a `clearList` primitive on the state adapter
/// contract. The marker is filtered out by `list()` and `count()`.
pub const TOMBSTONE_MARKER: &str = "__chatSdkTombstone";

/// Shape guard. 1:1 port of upstream
/// `isTombstone(value: unknown): boolean`. Returns `true` for a JSON
/// object whose [`TOMBSTONE_MARKER`] field equals `true`.
pub fn is_tombstone(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|obj| obj.get(TOMBSTONE_MARKER))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// Build a tombstone payload that [`is_tombstone`] recognizes. 1:1
/// with upstream's inline `{ [TOMBSTONE_MARKER]: true }` literal used
/// by `TranscriptsApiImpl.delete()`.
pub fn tombstone() -> serde_json::Value {
    serde_json::json!({ TOMBSTONE_MARKER: true })
}

/// Build the state-store key for a user's transcript list. 1:1 with
/// upstream's inline `${KEY_PREFIX}${userKey}`.
pub fn user_transcript_key(user_key: &str) -> String {
    format!("{KEY_PREFIX}{user_key}")
}

/// Parse an upstream duration into milliseconds. 1:1 port of upstream
/// `parseDuration(value): number | undefined`.
///
/// Accepts a raw millisecond count or a validated [`DurationString`]
/// (`"30s"`, `"5m"`, `"2h"`, `"7d"`). The input type is already
/// validated by [`DurationString::parse`] at construction, so this
/// function is infallible — passing `None` returns `None`, matching
/// upstream's `if (value === undefined) return undefined`.
pub fn parse_duration(value: Option<&DurationInput>) -> Option<u64> {
    let value = value?;
    match value {
        DurationInput::Millis(ms) => Some(*ms),
        DurationInput::String(s) => Some(duration_string_to_ms(s)),
    }
}

fn duration_string_to_ms(value: &DurationString) -> u64 {
    let multiplier: u64 = match value.unit() {
        DurationUnit::Seconds => 1_000,
        DurationUnit::Minutes => 60_000,
        DurationUnit::Hours => 3_600_000,
        DurationUnit::Days => 86_400_000,
    };
    value.value() * multiplier
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the `parseDuration` portion of
    //! `packages/chat/src/transcripts.test.ts`. The TranscriptsApiImpl
    //! class itself is deferred until StateAdapter trait extension.
    use super::*;

    #[test]
    fn parse_duration_returns_none_for_none_input() {
        assert_eq!(parse_duration(None), None);
    }

    #[test]
    fn parse_duration_passes_through_raw_milliseconds() {
        let ms = DurationInput::Millis(12_345);
        assert_eq!(parse_duration(Some(&ms)), Some(12_345));
    }

    #[test]
    fn parse_duration_resolves_seconds_suffix() {
        let v = DurationInput::String(DurationString::parse("30s").unwrap());
        assert_eq!(parse_duration(Some(&v)), Some(30 * 1_000));
    }

    #[test]
    fn parse_duration_resolves_minutes_suffix() {
        let v = DurationInput::String(DurationString::parse("5m").unwrap());
        assert_eq!(parse_duration(Some(&v)), Some(5 * 60_000));
    }

    #[test]
    fn parse_duration_resolves_hours_suffix() {
        let v = DurationInput::String(DurationString::parse("2h").unwrap());
        assert_eq!(parse_duration(Some(&v)), Some(2 * 3_600_000));
    }

    #[test]
    fn parse_duration_resolves_days_suffix() {
        let v = DurationInput::String(DurationString::parse("7d").unwrap());
        assert_eq!(parse_duration(Some(&v)), Some(7 * 86_400_000));
    }

    #[test]
    fn invalid_duration_strings_are_rejected_at_parse_time() {
        // The Rust port enforces validity at DurationString::parse,
        // matching the upstream behavior of `throw new Error("Invalid
        // duration: ...")`. Once a DurationString is constructed,
        // parse_duration is infallible.
        assert!(DurationString::parse("3y").is_err());
        assert!(DurationString::parse("abc").is_err());
        assert!(DurationString::parse("").is_err());
    }

    #[test]
    fn constants_match_upstream_values() {
        assert_eq!(KEY_PREFIX, "transcripts:user:");
        assert_eq!(DEFAULT_MAX_PER_USER, 200);
        assert_eq!(DEFAULT_LIST_LIMIT, 50);
        assert_eq!(TOMBSTONE_MARKER, "__chatSdkTombstone");
    }

    // ---------- slice 96: tombstone + user_transcript_key helpers ----------

    #[test]
    fn is_tombstone_accepts_a_well_formed_tombstone() {
        let value = tombstone();
        assert!(is_tombstone(&value));
    }

    #[test]
    fn is_tombstone_rejects_objects_without_the_marker() {
        let value = serde_json::json!({"foo": "bar"});
        assert!(!is_tombstone(&value));
    }

    #[test]
    fn is_tombstone_rejects_non_object_values() {
        assert!(!is_tombstone(&serde_json::json!(null)));
        assert!(!is_tombstone(&serde_json::json!("string")));
        assert!(!is_tombstone(&serde_json::json!(42)));
        assert!(!is_tombstone(&serde_json::json!([])));
    }

    #[test]
    fn is_tombstone_requires_marker_value_true() {
        // Marker present but value is false / not-bool.
        let value = serde_json::json!({"__chatSdkTombstone": false});
        assert!(!is_tombstone(&value));
        let value = serde_json::json!({"__chatSdkTombstone": "yes"});
        assert!(!is_tombstone(&value));
    }

    #[test]
    fn user_transcript_key_concatenates_prefix_and_user_key() {
        assert_eq!(user_transcript_key("U123"), "transcripts:user:U123");
        assert_eq!(user_transcript_key(""), "transcripts:user:");
    }
}
