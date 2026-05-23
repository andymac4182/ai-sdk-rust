//! Per-thread message-history cache helpers.
//!
//! 1:1 port (in progress) of `packages/chat/src/thread-history.ts`.
//!
//! **What this slice ships:** the pure surface:
//!
//! - [`DEFAULT_MAX_MESSAGES`] / [`DEFAULT_TTL_MS`] / [`KEY_PREFIX`]
//!   constants matching upstream values.
//! - [`ThreadHistoryConfig`] data struct (1:1 with upstream
//!   `interface ThreadHistoryConfig`).
//! - [`history_key`] helper that derives the state-store key for a
//!   given thread id (upstream inline `${KEY_PREFIX}${threadId}`).
//!
//! [`ThreadHistoryCache`] — the upstream class, ported in slice 119
//! after the Phase 1.5 StateAdapter trait extension.

use std::sync::Arc;

use crate::message::Message;
use crate::types::{StateAdapter, StateResult};

/// Default cap on stored messages per thread. 1:1 port of upstream
/// `const DEFAULT_MAX_MESSAGES = 100`.
pub const DEFAULT_MAX_MESSAGES: usize = 100;

/// Default TTL for cached thread history. 1:1 port of upstream
/// `const DEFAULT_TTL_MS = 7 * 24 * 60 * 60 * 1000` (7 days in ms).
pub const DEFAULT_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// State-store key prefix for thread-history entries. 1:1 port of
/// upstream `const KEY_PREFIX = "msg-history:"`.
///
/// **Storage-stability note:** upstream renamed the user-facing class
/// from `MessageHistoryCache` to `ThreadHistoryCache`, but kept this
/// prefix as `"msg-history:"` for backwards compatibility — renaming
/// would silently orphan every existing user's stored data. The Rust
/// port preserves the same wire-stable prefix.
pub const KEY_PREFIX: &str = "msg-history:";

/// Options for the (deferred) [`ThreadHistoryCache`] constructor. 1:1
/// port of upstream `interface ThreadHistoryConfig`. Both fields
/// optional with the upstream defaults applied when omitted.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadHistoryConfig {
    /// Maximum messages to keep per thread; defaults to
    /// [`DEFAULT_MAX_MESSAGES`] when `None`.
    pub max_messages: Option<usize>,
    /// TTL for cached history in ms; defaults to [`DEFAULT_TTL_MS`]
    /// when `None`.
    pub ttl_ms: Option<u64>,
}

/// Derive the state-store key for a given thread id. 1:1 port of
/// upstream's inline `const key = \`${KEY_PREFIX}${threadId}\`;`.
pub fn history_key(thread_id: &str) -> String {
    format!("{KEY_PREFIX}{thread_id}")
}

/// Predicate — does this state-store key carry the thread-history
/// [`KEY_PREFIX`]? Matches upstream's inline
/// `key.startsWith(KEY_PREFIX)` checks at iteration sites and the
/// implicit "is this our key namespace" filter inside
/// `ThreadHistoryCache.getMessages`.
pub fn is_history_key(key: &str) -> bool {
    key.starts_with(KEY_PREFIX)
}

/// Inverse of [`history_key`]: extract the thread id from a stored
/// thread-history key. Returns `None` for inputs that don't carry the
/// [`KEY_PREFIX`].
///
/// Mirrors the inline `key.slice(KEY_PREFIX.length)` upstream uses
/// when iterating cross-thread history keys.
pub fn thread_id_from_history_key(key: &str) -> Option<&str> {
    key.strip_prefix(KEY_PREFIX)
}

impl ThreadHistoryConfig {
    /// Effective max-messages cap with the upstream default applied.
    /// 1:1 with upstream's inline
    /// `this.maxMessages = config.maxMessages ?? DEFAULT_MAX_MESSAGES`.
    pub fn max_messages_or_default(&self) -> usize {
        self.max_messages.unwrap_or(DEFAULT_MAX_MESSAGES)
    }

    /// Effective TTL with the upstream default applied. 1:1 with
    /// upstream's inline `this.ttlMs = config.ttlMs ?? DEFAULT_TTL_MS`.
    pub fn ttl_ms_or_default(&self) -> u64 {
        self.ttl_ms.unwrap_or(DEFAULT_TTL_MS)
    }
}

/// Per-thread message-history cache. 1:1 port of upstream
/// `class ThreadHistoryCache`.
///
/// Reads/writes flow through the [`StateAdapter`] trait. `append`
/// calls [`StateAdapter::append_to_list`] with the configured
/// max-messages cap + TTL; `get_messages` calls
/// [`StateAdapter::get_list`] and rehydrates each stored
/// [`crate::message::SerializedMessage`] back into a [`Message`].
#[derive(Clone)]
pub struct ThreadHistoryCache {
    state: Arc<dyn StateAdapter>,
    config: ThreadHistoryConfig,
}

impl std::fmt::Debug for ThreadHistoryCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadHistoryCache")
            .field("state", &self.state)
            .field("config", &self.config)
            .finish()
    }
}

impl ThreadHistoryCache {
    /// 1:1 port of upstream
    /// `new ThreadHistoryCache({ state, config? })`.
    pub fn new(state: Arc<dyn StateAdapter>, config: ThreadHistoryConfig) -> Self {
        Self { state, config }
    }

    /// 1:1 port of upstream `async append(threadId, message)`. Writes
    /// the message's wire representation to the per-thread list,
    /// applying the configured `max_messages` cap (oldest entries
    /// drop when exceeded) and `ttl_ms` (refreshed on every append).
    pub async fn append(&self, thread_id: &str, message: &Message) -> StateResult<()> {
        let key = history_key(thread_id);
        // Null `raw` before storage to save space. 1:1 with upstream
        // `serialized.raw = null` in `ThreadHistoryCache.append`.
        let mut serialized = message.to_serialized();
        serialized.raw = serde_json::Value::Null;
        let value = serde_json::to_value(serialized).expect("Message serializes cleanly");
        self.state
            .append_to_list(
                &key,
                value,
                Some(self.config.max_messages_or_default()),
                Some(self.config.ttl_ms_or_default()),
            )
            .await
    }

    /// 1:1 port of upstream `async getMessages(threadId, limit?)`.
    /// Reads the per-thread list, skipping any value whose shape
    /// doesn't parse back as a [`Message`] (matching upstream's silent
    /// skip of malformed entries). When `limit` is `Some(n)` and the
    /// stored list is longer than `n`, returns the newest `n` messages
    /// (still in chronological order).
    pub async fn get_messages(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> StateResult<Vec<Message>> {
        let key = history_key(thread_id);
        let raw = self.state.get_list(&key, None).await?;
        let take_from = limit
            .filter(|n| raw.len() > *n)
            .map(|n| raw.len() - n)
            .unwrap_or(0);
        Ok(raw
            .into_iter()
            .skip(take_from)
            .filter_map(|v| serde_json::from_value(v).ok())
            .map(Message::from_serialized)
            .collect())
    }

    /// Total count of cached messages. Convenience helper matching
    /// upstream's inline `(await getMessages(id)).length` usage at
    /// adapter callsites.
    pub async fn count(&self, thread_id: &str) -> StateResult<usize> {
        Ok(self.get_messages(thread_id, None).await?.len())
    }
}

#[cfg(test)]
mod tests {
    //! Coverage notes for `packages/chat/src/thread-history.test.ts`:
    //! all 7 upstream cases exercise `ThreadHistoryCache.append` /
    //! `getMessages` which depend on the `StateAdapter.appendToList`
    //! / `getList` methods that the placeholder trait doesn't carry
    //! yet. Those cases ship in the follow-up `ThreadHistoryCache`
    //! slice once the trait is extended. The 4 tests below are
    //! additive Rust-side coverage for the pure helpers shipped now.
    use super::*;

    #[test]
    fn default_max_messages_matches_upstream_constant() {
        assert_eq!(DEFAULT_MAX_MESSAGES, 100);
    }

    #[test]
    fn default_ttl_ms_matches_upstream_seven_days() {
        assert_eq!(DEFAULT_TTL_MS, 7 * 24 * 60 * 60 * 1000);
    }

    #[test]
    fn key_prefix_stays_stable_msg_history_for_backwards_compat() {
        assert_eq!(KEY_PREFIX, "msg-history:");
    }

    #[test]
    fn history_key_concatenates_prefix_and_thread_id() {
        assert_eq!(
            history_key("slack:C123:1234.5678"),
            "msg-history:slack:C123:1234.5678"
        );
        assert_eq!(history_key(""), "msg-history:");
    }

    // ---------- slice 111: predicate + inverse + default-applied getters ----------

    #[test]
    fn is_history_key_detects_the_prefix() {
        assert!(is_history_key("msg-history:thread-1"));
        assert!(is_history_key("msg-history:"));
    }

    #[test]
    fn is_history_key_rejects_unrelated_keys() {
        assert!(!is_history_key("transcripts:user:U123"));
        assert!(!is_history_key("plain-key"));
        assert!(!is_history_key(""));
    }

    #[test]
    fn thread_id_from_history_key_strips_the_prefix() {
        assert_eq!(
            thread_id_from_history_key("msg-history:slack:C123:1234.5678"),
            Some("slack:C123:1234.5678")
        );
        assert_eq!(thread_id_from_history_key("msg-history:"), Some(""));
    }

    #[test]
    fn thread_id_from_history_key_returns_none_for_non_history_keys() {
        assert!(thread_id_from_history_key("transcripts:user:U123").is_none());
        assert!(thread_id_from_history_key("plain").is_none());
        assert!(thread_id_from_history_key("").is_none());
    }

    #[test]
    fn history_key_and_inverse_round_trip() {
        for tid in ["t1", "slack:C:1.2", "", "with:colons:and:more"] {
            let key = history_key(tid);
            assert_eq!(thread_id_from_history_key(&key), Some(tid));
        }
    }

    #[test]
    fn thread_history_config_max_messages_defaults_to_upstream_constant() {
        let cfg = ThreadHistoryConfig::default();
        assert_eq!(cfg.max_messages_or_default(), DEFAULT_MAX_MESSAGES);
    }

    #[test]
    fn thread_history_config_max_messages_returns_explicit_value_when_set() {
        let cfg = ThreadHistoryConfig {
            max_messages: Some(25),
            ttl_ms: None,
        };
        assert_eq!(cfg.max_messages_or_default(), 25);
    }

    #[test]
    fn thread_history_config_ttl_defaults_to_upstream_seven_days() {
        let cfg = ThreadHistoryConfig::default();
        assert_eq!(cfg.ttl_ms_or_default(), DEFAULT_TTL_MS);
    }

    #[test]
    fn thread_history_config_ttl_returns_explicit_value_when_set() {
        let cfg = ThreadHistoryConfig {
            max_messages: None,
            ttl_ms: Some(60_000),
        };
        assert_eq!(cfg.ttl_ms_or_default(), 60_000);
    }

    // ---------- slice 119: ThreadHistoryCache class ----------

    use crate::markdown::root;
    use crate::message::Message;
    use crate::types::{
        Author, BotStatus, MessageMetadata, StateAdapter, StateAdapterError, StateResult,
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
            _limit: Option<usize>,
        ) -> StateResult<Vec<serde_json::Value>> {
            let lists = self
                .lists
                .lock()
                .map_err(|_| StateAdapterError::NotConnected)?;
            Ok(lists.get(key).cloned().unwrap_or_default())
        }
    }

    fn sample_message(id: &str, text: &str) -> Message {
        Message::new(
            id,
            "slack:C123:1.0",
            text,
            root(vec![]),
            serde_json::json!({}),
            Author {
                user_id: "U1".to_string(),
                user_name: "alice".to_string(),
                full_name: "Alice".to_string(),
                is_bot: BotStatus::Known(false),
                is_me: false,
            },
            MessageMetadata {
                date_sent: "2024-01-15T10:30:00.000Z".to_string(),
                edited: false,
                edited_at: None,
            },
            vec![],
        )
    }

    #[test]
    fn thread_history_cache_append_then_get_returns_the_message() {
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state, ThreadHistoryConfig::default());
        let m = sample_message("m1", "hello");
        block_on(cache.append("slack:C123:1.0", &m)).unwrap();
        let history = block_on(cache.get_messages("slack:C123:1.0", None)).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "m1");
        assert_eq!(history[0].text, "hello");
    }

    #[test]
    fn thread_history_cache_count_reports_number_of_cached_messages() {
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state, ThreadHistoryConfig::default());
        assert_eq!(block_on(cache.count("t1")).unwrap(), 0);
        block_on(cache.append("t1", &sample_message("m1", "a"))).unwrap();
        block_on(cache.append("t1", &sample_message("m2", "b"))).unwrap();
        assert_eq!(block_on(cache.count("t1")).unwrap(), 2);
    }

    #[test]
    fn thread_history_cache_respects_max_messages_cap_via_state_layer() {
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(
            state,
            ThreadHistoryConfig {
                max_messages: Some(2),
                ttl_ms: None,
            },
        );
        for i in 0..5 {
            block_on(cache.append("t1", &sample_message(&format!("m{i}"), "x"))).unwrap();
        }
        let history = block_on(cache.get_messages("t1", None)).unwrap();
        assert_eq!(history.len(), 2);
        // Oldest entries drop — last two should remain (m3, m4).
        assert_eq!(history[0].id, "m3");
        assert_eq!(history[1].id, "m4");
    }

    #[test]
    fn thread_history_cache_isolates_threads_by_id() {
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state, ThreadHistoryConfig::default());
        block_on(cache.append("t1", &sample_message("m1", "x"))).unwrap();
        block_on(cache.append("t2", &sample_message("m2", "y"))).unwrap();
        block_on(cache.append("t2", &sample_message("m3", "z"))).unwrap();
        assert_eq!(block_on(cache.count("t1")).unwrap(), 1);
        assert_eq!(block_on(cache.count("t2")).unwrap(), 2);
    }

    #[test]
    fn thread_history_cache_get_messages_returns_empty_for_unknown_thread() {
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state, ThreadHistoryConfig::default());
        let history = block_on(cache.get_messages("nonexistent", None)).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn thread_history_cache_skips_malformed_stored_values() {
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state.clone(), ThreadHistoryConfig::default());
        // Plant a malformed value plus a good one.
        block_on(state.append_to_list(
            &history_key("t1"),
            serde_json::json!({"garbage": true}),
            None,
            None,
        ))
        .unwrap();
        block_on(cache.append("t1", &sample_message("m1", "ok"))).unwrap();
        let history = block_on(cache.get_messages("t1", None)).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "m1");
    }

    // ---------- 2 additional upstream cases (slice 225) ----------

    #[test]
    fn thread_history_cache_should_strip_raw_field_on_storage() {
        // 1:1 with upstream `should strip raw field on storage`.
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state.clone(), ThreadHistoryConfig::default());
        let mut msg = sample_message("m1", "Hello");
        msg.raw = serde_json::json!({"secret": "data", "nested": {"deep": true}});
        block_on(cache.append("t1", &msg)).unwrap();
        let stored = block_on(state.get_list(&history_key("t1"), None)).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(
            stored[0]["raw"].is_null(),
            "expected raw to be null, got: {}",
            stored[0]["raw"]
        );
    }

    #[test]
    fn thread_history_cache_should_support_limit_parameter_in_get_messages() {
        // 1:1 with upstream `should support limit parameter in
        // getMessages`. Append 5 messages, request limit=2, get newest
        // 2 in chronological order.
        let state: std::sync::Arc<dyn StateAdapter> = std::sync::Arc::new(MockState::default());
        let cache = ThreadHistoryCache::new(state, ThreadHistoryConfig::default());
        for i in 1..=5 {
            block_on(cache.append("t1", &sample_message(&format!("m{i}"), &format!("Msg {i}"))))
                .unwrap();
        }
        let limited = block_on(cache.get_messages("t1", Some(2))).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].id, "m4");
        assert_eq!(limited[1].id, "m5");
    }
}
