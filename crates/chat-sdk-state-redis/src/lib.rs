//! Production Redis state backend for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/state-redis/src/index.ts`.
//!
//! **What this slice ships (slice 140):**
//!
//! - Crate skeleton + `Cargo.toml` (deps: chat-sdk-chat,
//!   async-trait, serde + serde_json).
//! - [`RedisStateAdapter`] struct holding connection config
//!   (URL, optional namespace prefix, key-encoding strategy).
//! - [`RedisStateAdapterOptions`] config struct.
//! - [`RedisStateAdapter`] impl-ing the
//!   [`chat_sdk_chat::types::StateAdapter`] trait. Methods currently
//!   return `Err(StateAdapterError::NotConnected)` because the
//!   actual Redis client isn't wired in yet.
//!
//! **What is deferred:**
//!
//! - The actual Redis client (`redis-rs` + `bb8-redis` per the
//!   `scripts/codex-goal-chat/port-chat-sdk.md` "Phase 2 / Phase 3
//!   prep" recommendation). Requires the workspace runtime
//!   decision; pulls in `tokio`.
//! - Lock primitive: upstream uses Redis `SET NX PX` for
//!   acquireLock and Lua scripts for release/extend. Lands
//!   alongside the client wire-up.
//! - Pub/sub for subscriptions.

use async_trait::async_trait;
use chat_sdk_chat::types::{StateAdapter, StateAdapterError, StateResult};

/// Default Redis URL.
pub const DEFAULT_REDIS_URL: &str = "redis://localhost:6379";

/// Default key namespace prefix.
pub const DEFAULT_KEY_PREFIX: &str = "chat:";

/// Options for [`RedisStateAdapter::new`].
#[derive(Debug, Clone)]
pub struct RedisStateAdapterOptions {
    /// Redis connection URL (e.g. `redis://user:pass@host:6379/0`).
    pub url: String,
    /// Optional namespace prefix prepended to every key. Defaults
    /// to [`DEFAULT_KEY_PREFIX`].
    pub key_prefix: Option<String>,
}

impl RedisStateAdapterOptions {
    /// Construct options with default URL + prefix.
    pub fn new() -> Self {
        Self {
            url: DEFAULT_REDIS_URL.to_string(),
            key_prefix: None,
        }
    }

    /// Override the Redis URL.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Override the key prefix.
    pub fn with_key_prefix(mut self, key_prefix: impl Into<String>) -> Self {
        self.key_prefix = Some(key_prefix.into());
        self
    }

    /// Effective key prefix with default applied.
    pub fn effective_key_prefix(&self) -> &str {
        self.key_prefix.as_deref().unwrap_or(DEFAULT_KEY_PREFIX)
    }
}

impl Default for RedisStateAdapterOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Production Redis state backend.
#[derive(Debug, Clone)]
pub struct RedisStateAdapter {
    options: RedisStateAdapterOptions,
}

impl RedisStateAdapter {
    /// 1:1 port of upstream
    /// `new RedisStateAdapter({ url, keyPrefix? })`.
    pub fn new(options: RedisStateAdapterOptions) -> Self {
        Self { options }
    }

    /// Read the Redis URL.
    pub fn url(&self) -> &str {
        &self.options.url
    }

    /// Effective key prefix.
    pub fn key_prefix(&self) -> &str {
        self.options.effective_key_prefix()
    }

    /// Apply the namespace prefix to `key`. 1:1 with upstream's
    /// inline `\`${keyPrefix}${key}\`` template.
    pub fn prefixed_key(&self, key: &str) -> String {
        format!("{}{key}", self.key_prefix())
    }
}

/// Generate a unique lock/sub token. 1:1 port of upstream's
/// private `generateToken()`:
/// `redis_${Date.now()}_${Math.random().toString(36).substring(2, 15)}`.
/// Returns `redis_<unix-ms>_<13-char-base36-lowercased>`.
///
/// Exposed at module scope (rather than private as upstream) so the
/// suffix shape can be unit-tested without driving through
/// `acquireLock`/`subscribe` which require a live Redis connection.
pub fn generate_token() -> String {
    use rand::Rng;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    // Match upstream's `.toString(36).substring(2, 15)` which yields
    // 13 lowercase base36 characters (0-9 + a-z).
    const BASE36: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let suffix: String = (0..13)
        .map(|_| BASE36[rng.gen_range(0..BASE36.len())] as char)
        .collect();
    format!("redis_{now_ms}_{suffix}")
}

#[async_trait]
impl StateAdapter for RedisStateAdapter {
    async fn get(&self, _key: &str) -> StateResult<Option<serde_json::Value>> {
        Err(StateAdapterError::NotConnected)
    }

    async fn set(
        &self,
        _key: &str,
        _value: serde_json::Value,
        _ttl_ms: Option<u64>,
    ) -> StateResult<()> {
        Err(StateAdapterError::NotConnected)
    }

    async fn delete(&self, _key: &str) -> StateResult<()> {
        Err(StateAdapterError::NotConnected)
    }

    async fn append_to_list(
        &self,
        _key: &str,
        _value: serde_json::Value,
        _max_length: Option<usize>,
        _ttl_ms: Option<u64>,
    ) -> StateResult<()> {
        Err(StateAdapterError::NotConnected)
    }

    async fn get_list(
        &self,
        _key: &str,
        _limit: Option<usize>,
    ) -> StateResult<Vec<serde_json::Value>> {
        Err(StateAdapterError::NotConnected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;

    // ---------- generate_token (additive) ----------
    // No standalone upstream tests; the helper is exercised through
    // `acquireLock` and friends. The Rust suite locks in the shape
    // (prefix, base36 suffix length, lowercase, uniqueness).

    #[test]
    fn generate_token_has_redis_prefix_and_two_underscores() {
        let t = generate_token();
        assert!(t.starts_with("redis_"), "got: {t}");
        // `redis_<ms>_<13chars>` -> exactly 2 underscores.
        assert_eq!(t.matches('_').count(), 2, "got: {t}");
    }

    #[test]
    fn generate_token_suffix_is_thirteen_lowercase_base36_chars() {
        let t = generate_token();
        let suffix = t.rsplit('_').next().expect("suffix");
        assert_eq!(suffix.len(), 13, "got: {t}");
        assert!(
            suffix.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()),
            "non-base36 char in suffix: {suffix}"
        );
    }

    #[test]
    fn generate_token_produces_unique_values_across_calls() {
        // Upstream relies on Date.now()+Math.random() for uniqueness;
        // the Rust port uses SystemTime+rand. 1000 consecutive calls
        // should all be unique (timestamp collisions are tolerated by
        // the random suffix).
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(generate_token()));
        }
    }

    #[test]
    fn options_new_uses_default_url_and_prefix() {
        let opts = RedisStateAdapterOptions::new();
        assert_eq!(opts.url, DEFAULT_REDIS_URL);
        assert_eq!(opts.effective_key_prefix(), DEFAULT_KEY_PREFIX);
    }

    #[test]
    fn options_with_url_overrides_the_default() {
        let opts = RedisStateAdapterOptions::new().with_url("redis://example.test:6380");
        assert_eq!(opts.url, "redis://example.test:6380");
    }

    #[test]
    fn options_with_key_prefix_overrides_the_default() {
        let opts = RedisStateAdapterOptions::new().with_key_prefix("custom:");
        assert_eq!(opts.effective_key_prefix(), "custom:");
    }

    #[test]
    fn adapter_url_and_key_prefix_accessors() {
        let adapter = RedisStateAdapter::new(
            RedisStateAdapterOptions::new()
                .with_url("redis://example.test")
                .with_key_prefix("ns:"),
        );
        assert_eq!(adapter.url(), "redis://example.test");
        assert_eq!(adapter.key_prefix(), "ns:");
    }

    #[test]
    fn adapter_prefixed_key_concatenates_the_prefix() {
        let adapter =
            RedisStateAdapter::new(RedisStateAdapterOptions::new().with_key_prefix("ns:"));
        assert_eq!(adapter.prefixed_key("transcripts:U1"), "ns:transcripts:U1");
        assert_eq!(adapter.prefixed_key(""), "ns:");
    }

    #[test]
    fn adapter_prefixed_key_uses_default_when_no_override() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        assert_eq!(adapter.prefixed_key("foo"), "chat:foo");
    }

    #[test]
    fn adapter_get_returns_not_connected_until_client_lands() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        match block_on(adapter.get("k")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_set_returns_not_connected_until_client_lands() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        match block_on(adapter.set("k", serde_json::json!(1), None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_returns_not_connected_until_client_lands() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        match block_on(adapter.delete("k")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_append_to_list_returns_not_connected_until_client_lands() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        match block_on(adapter.append_to_list("k", serde_json::json!(1), None, None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_get_list_returns_not_connected_until_client_lands() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        match block_on(adapter.get_list("k", None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    // ---------- upstream "should have X method" mappings (3 of 5) ----------
    // 1:1 with upstream `index.test.ts` cases:
    //
    // - `should have appendToList method` → mapped to
    //   `adapter_append_to_list_returns_not_connected_until_client_lands`
    //   above (calling the method proves it exists; the upstream test
    //   only asserts `typeof adapter.appendToList === "function"`).
    // - `should have getList method` → mapped to
    //   `adapter_get_list_returns_not_connected_until_client_lands`.
    // - `should have set_if_not_exists method` (implicit upstream
    //   coverage via the lock primitive) → mapped to the new
    //   `adapter_set_if_not_exists_returns_not_connected_until_client_lands`
    //   test below.
    //
    // The remaining 3 upstream method-existence cases (`enqueue` /
    // `dequeue` / `queueDepth`) are deferred until the chat-sdk-chat
    // `StateAdapter` trait gets extended with the queue-primitive
    // methods (currently only `state-memory` has them as inherent
    // methods, not trait surface). They're tracked as deferred in
    // `docs/chat/goal-refinements.md`.

    #[test]
    fn adapter_set_if_not_exists_returns_not_connected_until_client_lands() {
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        match block_on(adapter.set_if_not_exists("k", serde_json::json!(1), None)) {
            // The trait default impl falls through to `get(key)` first,
            // which surfaces the `NotConnected` placeholder from the
            // Rust port. Production Redis backends should override this
            // with an atomic `SETNX`.
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_connect_default_trait_impl_is_no_op() {
        // 1:1 with upstream's `connect` method-existence test (the
        // upstream test asserts the method exists; the Rust trait
        // default `connect` returns `Ok(())` until a real client is
        // wired).
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        assert!(block_on(adapter.connect()).is_ok());
    }

    #[test]
    fn adapter_disconnect_default_trait_impl_is_no_op() {
        // Same shape as the `connect` mapping — upstream tests
        // `typeof disconnect === "function"`; the Rust trait default
        // `disconnect` returns `Ok(())`. A real Redis backend should
        // override this to drop the connection pool.
        let adapter = RedisStateAdapter::new(RedisStateAdapterOptions::new());
        assert!(block_on(adapter.disconnect()).is_ok());
    }
}
