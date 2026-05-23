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
//! **What is deferred:** the [`ThreadHistoryCache`] class itself
//! (append / get_messages) requires `StateAdapter.append_to_list` /
//! `StateAdapter.get_list` on the [`crate::types::StateAdapter`]
//! trait, which is currently the empty placeholder. The class ships
//! once that trait is extended.

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
}
