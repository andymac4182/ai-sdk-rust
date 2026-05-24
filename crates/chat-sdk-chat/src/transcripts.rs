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
//! [`TranscriptsApiImpl`] — the upstream class, ported in slice 118
//! after the Phase 1.5 StateAdapter trait extension.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{
    DurationString, DurationUnit, StateAdapter, StateResult, TranscriptEntry, TranscriptsConfig,
};

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

/// Predicate — does this state-store key belong to a user transcript
/// list? 1:1 with upstream's inline
/// `key.startsWith(KEY_PREFIX)` checks used by transcripts
/// list/iteration paths.
pub fn is_user_transcript_key(key: &str) -> bool {
    key.starts_with(KEY_PREFIX)
}

/// Extract the user key from a stored transcript list key. Inverse of
/// [`user_transcript_key`]. Returns `None` for inputs that don't carry
/// the [`KEY_PREFIX`].
///
/// This mirrors the inline `key.slice(KEY_PREFIX.length)` upstream
/// uses when iterating cross-user transcript keys.
pub fn user_key_from_transcript_key(key: &str) -> Option<&str> {
    key.strip_prefix(KEY_PREFIX)
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn generate_id() -> String {
    // 1:1 with upstream `crypto.randomUUID()` semantically — the only
    // requirement is uniqueness within a single SDK instance's lifetime.
    // An atomic counter + timestamp gives that without pulling in `uuid`.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let t = now_ms();
    format!("ts-{t:016x}-{n:016x}")
}

/// Input to [`TranscriptsApiImpl::append`]. 1:1 with upstream
/// `interface AppendTranscriptInput` (every field optional except
/// `text` + `role` + `thread_id` + `platform`). The SDK fills in
/// `id` and `timestamp` at append time.
#[derive(Debug, Clone)]
pub struct AppendTranscriptInput {
    /// mdast AST. Stored only when `config.store_formatted == Some(true)`.
    pub formatted: Option<crate::types::FormattedContent>,
    /// Originating adapter name (e.g. `"slack"`, `"teams"`).
    pub platform: String,
    /// Platform-native message ID, when known.
    pub platform_message_id: Option<String>,
    /// `user` / `assistant` / `system`.
    pub role: crate::types::TranscriptRole,
    /// Plain-text body.
    pub text: String,
    /// Originating thread ID.
    pub thread_id: String,
    /// Cross-platform user key.
    pub user_key: String,
}

/// Per-user transcript store. 1:1 port of upstream
/// `class TranscriptsApiImpl`.
///
/// Reads/writes flow through the [`StateAdapter`] trait: `append`
/// calls [`StateAdapter::append_to_list`], `list` / `count` call
/// [`StateAdapter::get_list`], and `delete` writes a tombstone via
/// `append_to_list` (followed by a single-item list, matching
/// upstream's "delete-by-tombstone" trick).
#[derive(Clone)]
pub struct TranscriptsApiImpl {
    state: Arc<dyn StateAdapter>,
    config: TranscriptsConfig,
}

impl std::fmt::Debug for TranscriptsApiImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptsApiImpl")
            .field("state", &self.state)
            .field("config", &self.config)
            .finish()
    }
}

impl TranscriptsApiImpl {
    /// 1:1 port of upstream `new TranscriptsApiImpl({ state, config })`.
    pub fn new(state: Arc<dyn StateAdapter>, config: TranscriptsConfig) -> Self {
        Self { state, config }
    }

    fn max_per_user(&self) -> usize {
        self.config
            .max_per_user
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_PER_USER)
    }

    fn retention_ms(&self) -> Option<u64> {
        let policy = self.config.retention.as_ref()?;
        let input = match policy {
            crate::types::RetentionPolicy::Millis(ms) => DurationInput::Millis(*ms),
            crate::types::RetentionPolicy::Duration(d) => DurationInput::String(d.clone()),
        };
        parse_duration(Some(&input))
    }

    /// 1:1 port of upstream `async append(input): Promise<TranscriptEntry>`.
    /// Fills in `id` + `timestamp` and writes the entry to the user's
    /// list via [`StateAdapter::append_to_list`].
    pub async fn append(&self, input: AppendTranscriptInput) -> StateResult<TranscriptEntry> {
        let store_formatted = self.config.store_formatted.unwrap_or(false);
        let entry = TranscriptEntry {
            formatted: if store_formatted {
                input.formatted
            } else {
                None
            },
            id: generate_id(),
            platform: input.platform,
            platform_message_id: input.platform_message_id,
            role: input.role,
            text: input.text,
            thread_id: input.thread_id,
            timestamp: now_ms(),
            user_key: input.user_key.clone(),
        };
        let key = user_transcript_key(&input.user_key);
        let value = serde_json::to_value(&entry).expect("TranscriptEntry serializes cleanly");
        self.state
            .append_to_list(&key, value, Some(self.max_per_user()), self.retention_ms())
            .await?;
        Ok(entry)
    }

    /// 1:1 port of upstream `async list(userKey, options?): Promise<TranscriptEntry[]>`.
    /// Filters out tombstones (matching upstream's `list()` filter); the
    /// page-size limit defaults to [`DEFAULT_LIST_LIMIT`] when `None` (1:1
    /// with upstream's `options?.limit ?? DEFAULT_LIST_LIMIT`).
    pub async fn list(
        &self,
        user_key: &str,
        limit: Option<usize>,
    ) -> StateResult<Vec<TranscriptEntry>> {
        let key = user_transcript_key(user_key);
        let raw = self
            .state
            .get_list(&key, Some(limit.unwrap_or(DEFAULT_LIST_LIMIT)))
            .await?;
        Ok(raw
            .into_iter()
            .filter(|v| !is_tombstone(v))
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect())
    }

    /// 1:1 port of upstream `async list({ userKey, limit?, platforms?,
    /// roles?, threadId? }): Promise<TranscriptEntry[]>` — the full
    /// query shape with optional filters. Applies the upstream filter
    /// chain in order: tombstones filtered first, then platform /
    /// threadId / role filters, then truncated to `limit ??
    /// DEFAULT_LIST_LIMIT` keeping the newest entries (chronological
    /// order preserved).
    pub async fn list_query(
        &self,
        query: &crate::types::ListQuery,
    ) -> StateResult<Vec<TranscriptEntry>> {
        let key = user_transcript_key(&query.user_key);
        let raw = self.state.get_list(&key, None).await?;
        let mut entries: Vec<TranscriptEntry> = raw
            .into_iter()
            .filter(|v| !is_tombstone(v))
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        if let Some(platforms) = query.platforms.as_ref().filter(|p| !p.is_empty()) {
            entries.retain(|e| platforms.iter().any(|p| p == &e.platform));
        }
        if let Some(thread_id) = query.thread_id.as_deref() {
            entries.retain(|e| e.thread_id == thread_id);
        }
        if let Some(roles) = query.roles.as_ref().filter(|r| !r.is_empty()) {
            entries.retain(|e| roles.contains(&e.role));
        }

        let limit = query.limit.map(|n| n as usize).unwrap_or(DEFAULT_LIST_LIMIT);
        if entries.len() > limit {
            // Keep the newest `limit`, preserving chronological order.
            let drop_count = entries.len() - limit;
            entries.drain(0..drop_count);
        }
        Ok(entries)
    }

    /// 1:1 port of upstream `async delete(userKey): Promise<void>`.
    /// Writes a tombstone marker via [`StateAdapter::append_to_list`]
    /// with `max_length: Some(1)` so the list collapses to just the
    /// tombstone; subsequent [`Self::list`] / [`Self::count`] calls
    /// observe an empty result.
    pub async fn delete(&self, user_key: &str) -> StateResult<()> {
        let key = user_transcript_key(user_key);
        self.state
            .append_to_list(&key, tombstone(), Some(1), self.retention_ms())
            .await
    }

    /// 1:1 port of upstream `async count(userKey): Promise<number>`.
    /// Counts non-tombstone entries.
    pub async fn count(&self, user_key: &str) -> StateResult<usize> {
        let key = user_transcript_key(user_key);
        let raw = self.state.get_list(&key, None).await?;
        Ok(raw.iter().filter(|v| !is_tombstone(v)).count())
    }
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

    // ---------- slice 110: prefix predicate + inverse helper ----------

    #[test]
    fn is_user_transcript_key_detects_the_prefix() {
        assert!(is_user_transcript_key("transcripts:user:U123"));
        assert!(is_user_transcript_key("transcripts:user:"));
    }

    #[test]
    fn is_user_transcript_key_rejects_unrelated_keys() {
        assert!(!is_user_transcript_key("transcripts:other:U123"));
        assert!(!is_user_transcript_key("msg-history:U123"));
        assert!(!is_user_transcript_key(""));
    }

    #[test]
    fn user_key_from_transcript_key_strips_the_prefix() {
        assert_eq!(
            user_key_from_transcript_key("transcripts:user:U123"),
            Some("U123")
        );
        assert_eq!(user_key_from_transcript_key("transcripts:user:"), Some(""));
    }

    #[test]
    fn user_key_from_transcript_key_returns_none_for_non_transcript_keys() {
        assert!(user_key_from_transcript_key("msg-history:U123").is_none());
        assert!(user_key_from_transcript_key("U123").is_none());
        assert!(user_key_from_transcript_key("").is_none());
    }

    #[test]
    fn user_transcript_key_and_inverse_round_trip() {
        for user in ["U123", "slack:U999", "", "some:colon:user"] {
            let key = user_transcript_key(user);
            assert_eq!(user_key_from_transcript_key(&key), Some(user));
        }
    }

    // ---------- slice 118: TranscriptsApiImpl class ----------

    use crate::types::{
        StateAdapter, StateAdapterError, StateResult, TranscriptRole, TranscriptsConfig,
    };
    use futures_executor::block_on;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MockState {
        lists: Mutex<HashMap<String, Vec<serde_json::Value>>>,
    }

    #[async_trait::async_trait]
    impl StateAdapter for MockState {
        async fn get(&self, _key: &str) -> StateResult<Option<serde_json::Value>> {
            Ok(None)
        }
        async fn set(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn delete(&self, _key: &str) -> StateResult<()> {
            Ok(())
        }
        async fn append_to_list(
            &self,
            key: &str,
            value: serde_json::Value,
            max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            let mut lists = self
                .lists
                .lock()
                .map_err(|_| StateAdapterError::NotConnected)?;
            let list = lists.entry(key.to_string()).or_default();
            list.push(value);
            if let Some(max) = max_length {
                if list.len() > max {
                    let start = list.len() - max;
                    *list = list.split_off(start);
                }
            }
            Ok(())
        }
        async fn get_list(
            &self,
            key: &str,
            limit: Option<usize>,
        ) -> StateResult<Vec<serde_json::Value>> {
            let lists = self
                .lists
                .lock()
                .map_err(|_| StateAdapterError::NotConnected)?;
            let list = lists.get(key).cloned().unwrap_or_default();
            Ok(match limit {
                Some(n) if list.len() > n => list[list.len() - n..].to_vec(),
                _ => list,
            })
        }
    }

    fn sample_input(user: &str) -> AppendTranscriptInput {
        AppendTranscriptInput {
            formatted: None,
            platform: "slack".to_string(),
            platform_message_id: None,
            role: TranscriptRole::User,
            text: "hello".to_string(),
            thread_id: "slack:C1:1.0".to_string(),
            user_key: user.to_string(),
        }
    }

    #[test]
    fn transcripts_api_append_then_list_returns_the_entry() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        let entry = block_on(api.append(sample_input("U1"))).unwrap();
        assert_eq!(entry.user_key, "U1");
        assert_eq!(entry.text, "hello");
        assert!(!entry.id.is_empty());
        let list = block_on(api.list("U1", None)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, entry.id);
    }

    #[test]
    fn transcripts_api_count_returns_the_number_of_non_tombstone_entries() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        assert_eq!(block_on(api.count("U1")).unwrap(), 0);
        block_on(api.append(sample_input("U1"))).unwrap();
        block_on(api.append(sample_input("U1"))).unwrap();
        assert_eq!(block_on(api.count("U1")).unwrap(), 2);
    }

    #[test]
    fn transcripts_api_delete_writes_a_tombstone_and_empties_subsequent_lists() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        block_on(api.append(sample_input("U1"))).unwrap();
        block_on(api.append(sample_input("U1"))).unwrap();
        block_on(api.delete("U1")).unwrap();
        assert_eq!(block_on(api.list("U1", None)).unwrap().len(), 0);
        assert_eq!(block_on(api.count("U1")).unwrap(), 0);
    }

    #[test]
    fn transcripts_api_isolates_users_by_key() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        block_on(api.append(sample_input("U1"))).unwrap();
        block_on(api.append(sample_input("U2"))).unwrap();
        block_on(api.append(sample_input("U2"))).unwrap();
        assert_eq!(block_on(api.count("U1")).unwrap(), 1);
        assert_eq!(block_on(api.count("U2")).unwrap(), 2);
    }

    #[test]
    fn transcripts_api_respects_max_per_user_cap_via_state_layer() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                max_per_user: Some(2),
                ..Default::default()
            },
        );
        block_on(api.append(sample_input("U1"))).unwrap();
        block_on(api.append(sample_input("U1"))).unwrap();
        block_on(api.append(sample_input("U1"))).unwrap();
        assert_eq!(block_on(api.count("U1")).unwrap(), 2);
    }

    #[test]
    fn transcripts_api_store_formatted_false_drops_the_formatted_field() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                store_formatted: Some(false),
                ..Default::default()
            },
        );
        let mut input = sample_input("U1");
        input.formatted = Some(serde_json::json!({"type":"root","children":[]}));
        let entry = block_on(api.append(input)).unwrap();
        assert!(entry.formatted.is_none());
    }

    #[test]
    fn transcripts_api_store_formatted_true_keeps_the_formatted_field() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                store_formatted: Some(true),
                ..Default::default()
            },
        );
        let mut input = sample_input("U1");
        input.formatted = Some(serde_json::json!({"type":"root","children":[]}));
        let entry = block_on(api.append(input)).unwrap();
        assert!(entry.formatted.is_some());
    }

    #[test]
    fn transcripts_api_list_default_limit_caps_at_default_list_limit() {
        // The mock state honors the limit passed by the impl. With no
        // explicit limit, the impl asks for DEFAULT_LIST_LIMIT.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                max_per_user: Some(1000),
                ..Default::default()
            },
        );
        for _ in 0..60 {
            block_on(api.append(sample_input("U1"))).unwrap();
        }
        let list = block_on(api.list("U1", None)).unwrap();
        assert_eq!(list.len(), DEFAULT_LIST_LIMIT);
    }

    // ---------- list_query filters (3 upstream cases) ----------
    // 1:1 with upstream `list` describe block's filter cases.

    fn input_for_thread(user: &str, platform: &str, thread_id: &str, role: TranscriptRole, text: &str) -> AppendTranscriptInput {
        AppendTranscriptInput {
            formatted: None,
            platform: platform.to_string(),
            platform_message_id: None,
            role,
            text: text.to_string(),
            thread_id: thread_id.to_string(),
            user_key: user.to_string(),
        }
    }

    #[test]
    fn transcripts_api_list_query_filters_by_platform() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());

        block_on(api.append(input_for_thread("u1", "slack", "slack:C:T", TranscriptRole::User, "from slack")))
            .unwrap();
        block_on(api.append(input_for_thread(
            "u1",
            "discord",
            "discord:C:T",
            TranscriptRole::User,
            "from discord",
        )))
        .unwrap();

        let slack_only = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: Some(vec!["slack".to_string()]),
            roles: None,
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(slack_only.len(), 1);
        assert_eq!(slack_only[0].platform, "slack");
    }

    #[test]
    fn transcripts_api_list_query_filters_by_thread_id() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());

        block_on(api.append(input_for_thread("u1", "slack", "slack:C:A", TranscriptRole::User, "thread A")))
            .unwrap();
        block_on(api.append(input_for_thread("u1", "slack", "slack:C:B", TranscriptRole::User, "thread B")))
            .unwrap();

        let only_a = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: None,
            thread_id: Some("slack:C:A".to_string()),
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].text, "thread A");
    }

    #[test]
    fn transcripts_api_list_query_filters_by_role() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());

        block_on(api.append(input_for_thread(
            "u1",
            "slack",
            "slack:C:T",
            TranscriptRole::User,
            "user msg",
        )))
        .unwrap();
        block_on(api.append(input_for_thread(
            "u1",
            "slack",
            "slack:C:T",
            TranscriptRole::Assistant,
            "bot msg",
        )))
        .unwrap();

        let user_only = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: Some(vec![TranscriptRole::User]),
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(user_only.len(), 1);
        assert_eq!(user_only[0].role, TranscriptRole::User);

        let assistant_only = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: Some(vec![TranscriptRole::Assistant]),
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(assistant_only.len(), 1);
        assert_eq!(assistant_only[0].role, TranscriptRole::Assistant);
    }

    // ---------- list describe block base cases (3 upstream cases) ----------

    fn seed_five(api: &TranscriptsApiImpl, user: &str) {
        for i in 0..5 {
            block_on(api.append(input_for_thread(
                user,
                "slack",
                "slack:C:T",
                TranscriptRole::User,
                &format!("msg {i}"),
            )))
            .unwrap();
        }
    }

    #[test]
    fn transcripts_api_list_returns_all_messages_in_chronological_order_by_default() {
        // 1:1 with upstream `it("returns all messages in chronological
        // order by default")`. The Rust API exposes both `list(userKey,
        // limit)` and `list_query(ListQuery)` — both preserve insertion
        // (chronological) order from the underlying list adapter.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        seed_five(&api, "u1");
        let list = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: None,
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(list.len(), 5);
        let texts: Vec<String> = list.iter().map(|e| e.text.clone()).collect();
        assert_eq!(texts, vec!["msg 0", "msg 1", "msg 2", "msg 3", "msg 4"]);
    }

    #[test]
    fn transcripts_api_list_returns_empty_array_when_no_messages_exist() {
        // 1:1 with upstream `it("returns empty array when no messages
        // exist")`.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        let list = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: None,
            thread_id: None,
            user_key: "nobody".to_string(),
        }))
        .unwrap();
        assert!(list.is_empty());
    }

    // ---------- delete describe block additional cases (4 upstream) ----------

    #[test]
    fn transcripts_api_delete_hides_the_tombstone_from_list_and_count_after_deletion() {
        // 1:1 with upstream `it("hides the tombstone from list/count
        // after deletion")`. After delete, list/count both report
        // empty (tombstone is silently filtered, not surfaced as a
        // visible entry).
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.delete("u1")).unwrap();
        assert_eq!(block_on(api.count("u1")).unwrap(), 0);
        assert!(block_on(api.list("u1", None)).unwrap().is_empty());
    }

    #[test]
    fn transcripts_api_append_after_delete_behaves_as_if_list_were_freshly_empty() {
        // 1:1 with upstream `it("appends after delete behave as if the
        // list were freshly empty")`.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.delete("u1")).unwrap();
        // Fresh append after delete starts a new history.
        block_on(api.append(sample_input("u1"))).unwrap();
        assert_eq!(block_on(api.count("u1")).unwrap(), 1);
        let list = block_on(api.list("u1", None)).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn transcripts_api_delete_called_twice_does_not_double_count() {
        // 1:1 with upstream `it("does not double-count if delete is
        // called twice")`. The Rust delete API returns `()`, so the
        // "double-count" assertion lands on the post-delete state:
        // count stays 0, no extra tombstones leak into list.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.delete("u1")).unwrap();
        block_on(api.delete("u1")).unwrap();
        assert_eq!(block_on(api.count("u1")).unwrap(), 0);
        assert!(block_on(api.list("u1", None)).unwrap().is_empty());
    }

    // ---------- additional append/count describe block cases (3 upstream) ----------

    #[test]
    fn transcripts_api_append_assistant_message_with_explicit_user_key() {
        // 1:1 with upstream `it("appends an assistant message with
        // explicit userKey")`. The Rust API takes `user_key` as a
        // required field on `AppendTranscriptInput` rather than as
        // an `options` arg, so the test just shows the assistant
        // role round-trips through the entry.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        let entry = block_on(api.append(input_for_thread(
            "mike@acme.com",
            "slack",
            "slack:C:T",
            TranscriptRole::Assistant,
            "Hello, Mike",
        )))
        .unwrap();
        assert_eq!(entry.role, TranscriptRole::Assistant);
        assert_eq!(entry.user_key, "mike@acme.com");
        assert_eq!(entry.text, "Hello, Mike");
        assert!(entry.platform_message_id.is_none());
    }

    #[test]
    fn transcripts_api_append_passes_retention_duration_string_through_as_ttl_ms() {
        // 1:1 with upstream `it("passes retention duration string
        // through as ttlMs")`. The Rust impl resolves
        // `TranscriptsConfig.retention` to milliseconds via the same
        // `parse_duration` path; we assert the resolved retention
        // ms matches 7 * 24 * 60 * 60 * 1000.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                retention: Some(crate::types::RetentionPolicy::Duration("7d".parse().unwrap())),
                ..Default::default()
            },
        );
        assert_eq!(api.retention_ms(), Some(7 * 24 * 60 * 60 * 1000));
        block_on(api.append(sample_input("u1"))).unwrap();
        // Append succeeds with the retention configured.
        assert_eq!(block_on(api.count("u1")).unwrap(), 1);
    }

    #[test]
    fn transcripts_api_count_returns_zero_for_unknown_user_key() {
        // 1:1 with upstream `it("returns 0 for unknown userKey")`.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        assert_eq!(block_on(api.count("nobody")).unwrap(), 0);
    }

    // ---------- maxPerUser eviction describe block (1 upstream case) ----------

    #[test]
    fn transcripts_api_max_per_user_eviction_trims_via_append_to_list_semantics() {
        // 1:1 with upstream `describe("maxPerUser eviction") > it(
        // "trims to maxPerUser via appendToList semantics")`. With
        // maxPerUser=3, the oldest 2 of 5 entries get evicted; what
        // remains is the newest 3 ("msg 2", "msg 3", "msg 4") in
        // chronological order.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                max_per_user: Some(3),
                ..Default::default()
            },
        );
        for i in 0..5 {
            block_on(api.append(input_for_thread(
                "u1",
                "slack",
                "slack:C:T",
                TranscriptRole::User,
                &format!("msg {i}"),
            )))
            .unwrap();
        }
        let list = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: None,
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(list.len(), 3);
        let texts: Vec<String> = list.iter().map(|e| e.text.clone()).collect();
        assert_eq!(texts, vec!["msg 2", "msg 3", "msg 4"]);
    }

    // ---------- formatted round-trip describe block (1 upstream case) ----------

    #[test]
    fn transcripts_api_formatted_round_trip_preserves_mdast_root_through_state_serialization() {
        // 1:1 with upstream `describe("formatted round-trip") > it(
        // "preserves mdast Root through state serialization")`. With
        // storeFormatted=true, the AST passed in survives the
        // serialize-to-state -> deserialize-from-list round trip.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(
            state,
            TranscriptsConfig {
                store_formatted: Some(true),
                ..Default::default()
            },
        );
        let original = serde_json::json!({
            "type": "root",
            "children": [
                { "type": "heading", "depth": 1, "children": [{ "type": "text", "value": "Hello" }] },
                { "type": "paragraph", "children": [{ "type": "emphasis", "children": [{ "type": "text", "value": "world" }] }] },
            ],
        });
        let mut input = input_for_thread(
            "u1",
            "slack",
            "slack:C:T",
            TranscriptRole::User,
            "Hello world",
        );
        input.formatted = Some(original.clone());
        block_on(api.append(input)).unwrap();

        let list = block_on(api.list_query(&crate::types::ListQuery {
            limit: None,
            platforms: None,
            roles: None,
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].formatted.as_ref(), Some(&original));
    }

    #[test]
    fn transcripts_api_preserves_invariants_when_append_delete_append_are_interleaved() {
        // 1:1 with upstream `it("preserves invariants when
        // append/delete/append are interleaved without awaits")` —
        // adapted to Rust's blocking-on-each-step shape (futures are
        // not lazy; we still drive each in order).
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());

        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.delete("u1")).unwrap();
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.append(sample_input("u1"))).unwrap();
        block_on(api.delete("u1")).unwrap();
        block_on(api.append(sample_input("u1"))).unwrap();

        // After the final delete + 1 append, count = 1.
        assert_eq!(block_on(api.count("u1")).unwrap(), 1);
        assert_eq!(block_on(api.list("u1", None)).unwrap().len(), 1);
    }

    #[test]
    fn transcripts_api_list_returns_newest_n_when_limit_is_set_still_chronological() {
        // 1:1 with upstream `it("returns the newest N when limit is set,
        // still chronological")`.
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let api = TranscriptsApiImpl::new(state, TranscriptsConfig::default());
        seed_five(&api, "u1");
        let list = block_on(api.list_query(&crate::types::ListQuery {
            limit: Some(2),
            platforms: None,
            roles: None,
            thread_id: None,
            user_key: "u1".to_string(),
        }))
        .unwrap();
        assert_eq!(list.len(), 2);
        // Newest 2 (insertion order preserved): msg 3, msg 4.
        let texts: Vec<String> = list.iter().map(|e| e.text.clone()).collect();
        assert_eq!(texts, vec!["msg 3", "msg 4"]);
    }
}
