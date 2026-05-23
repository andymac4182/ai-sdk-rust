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
}
