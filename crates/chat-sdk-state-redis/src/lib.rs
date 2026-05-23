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
}
