//! `ioredis`-backed Redis state backend for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/state-ioredis/src/index.ts`.
//!
//! Upstream ships two Redis-flavored backends (`state-redis` and
//! `state-ioredis`) because the two upstream node clients
//! (`redis` and `ioredis`) have slightly different connection /
//! cluster / sentinel semantics. The Rust port collapses both into
//! the same family — `chat-sdk-state-redis` covers the standard
//! single-node case, and this `chat-sdk-state-ioredis` crate
//! covers cluster + sentinel + custom cluster-aware key hashing
//! scenarios.
//!
//! **What this slice ships (slice 141):**
//!
//! - Crate skeleton + `Cargo.toml`.
//! - [`IoredisStateAdapter`] struct holding cluster config (nodes
//!   + optional sentinel name + namespace prefix).
//! - [`IoredisStateAdapterOptions`] config struct.
//! - [`IoredisStateAdapter`] impl-ing
//!   [`chat_sdk_chat::types::StateAdapter`]. All required methods
//!   return `Err(StateAdapterError::NotConnected)` until the
//!   `redis-rs` cluster client lands.
//!
//! **What is deferred:**
//!
//! - `redis::cluster_async::ClusterClient` + `bb8-redis-cluster`
//!   pool wire-up. Requires the workspace runtime decision.

use async_trait::async_trait;
use chat_sdk_chat::types::{StateAdapter, StateAdapterError, StateResult};

/// Default key namespace prefix (matches state-redis).
pub const DEFAULT_KEY_PREFIX: &str = "chat:";

/// Options for [`IoredisStateAdapter::new`].
#[derive(Debug, Clone)]
pub struct IoredisStateAdapterOptions {
    /// Cluster nodes (`redis://host:port` URLs). At least one
    /// required; the client discovers the rest of the cluster
    /// topology from any single seed node.
    pub nodes: Vec<String>,
    /// Optional Sentinel master name. When set, [`Self::nodes`] is
    /// interpreted as Sentinel nodes rather than cluster nodes.
    pub sentinel_name: Option<String>,
    /// Optional namespace prefix prepended to every key.
    pub key_prefix: Option<String>,
}

impl IoredisStateAdapterOptions {
    /// Construct options from a single seed node URL.
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            nodes: vec![node.into()],
            sentinel_name: None,
            key_prefix: None,
        }
    }

    /// Add an additional cluster node.
    pub fn with_node(mut self, node: impl Into<String>) -> Self {
        self.nodes.push(node.into());
        self
    }

    /// Switch to Sentinel mode by naming the master.
    pub fn with_sentinel(mut self, sentinel_name: impl Into<String>) -> Self {
        self.sentinel_name = Some(sentinel_name.into());
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

    /// Whether this options struct describes a Sentinel deployment.
    pub fn is_sentinel(&self) -> bool {
        self.sentinel_name.is_some()
    }
}

/// `ioredis`-backed Redis state backend.
#[derive(Debug, Clone)]
pub struct IoredisStateAdapter {
    options: IoredisStateAdapterOptions,
}

impl IoredisStateAdapter {
    /// 1:1 port of upstream
    /// `new IoredisStateAdapter({ nodes, sentinelName?, keyPrefix? })`.
    pub fn new(options: IoredisStateAdapterOptions) -> Self {
        Self { options }
    }

    /// Read the cluster nodes.
    pub fn nodes(&self) -> &[String] {
        &self.options.nodes
    }

    /// Sentinel master name, if configured.
    pub fn sentinel_name(&self) -> Option<&str> {
        self.options.sentinel_name.as_deref()
    }

    /// Effective key prefix.
    pub fn key_prefix(&self) -> &str {
        self.options.effective_key_prefix()
    }

    /// Apply the namespace prefix to `key`.
    pub fn prefixed_key(&self, key: &str) -> String {
        format!("{}{key}", self.key_prefix())
    }
}

/// Generate a unique lock/sub token. 1:1 port of upstream's
/// private `generateToken()`:
/// `ioredis_${Date.now()}_${Math.random().toString(36).substring(2, 15)}`.
/// Returns `ioredis_<unix-ms>_<13-char-base36-lowercased>`.
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
    const BASE36: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let suffix: String = (0..13)
        .map(|_| BASE36[rng.gen_range(0..BASE36.len())] as char)
        .collect();
    format!("ioredis_{now_ms}_{suffix}")
}

#[async_trait]
impl StateAdapter for IoredisStateAdapter {
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
    // ---------- generate_token (additive) ----------
    // No standalone upstream tests; the helper is exercised through
    // `acquireLock` and friends. The Rust suite locks in the shape.

    #[test]
    fn generate_token_has_ioredis_prefix_and_two_underscores() {
        let t = generate_token();
        assert!(t.starts_with("ioredis_"), "got: {t}");
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
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(generate_token()));
        }
    }

    #[test]
    fn options_new_stores_a_seed_node() {
        let opts = IoredisStateAdapterOptions::new("redis://node1:6379");
        assert_eq!(opts.nodes, vec!["redis://node1:6379".to_string()]);
        assert!(opts.sentinel_name.is_none());
        assert_eq!(opts.effective_key_prefix(), DEFAULT_KEY_PREFIX);
    }

    #[test]
    fn options_with_node_appends_to_cluster() {
        let opts =
            IoredisStateAdapterOptions::new("redis://node1:6379").with_node("redis://node2:6379");
        assert_eq!(opts.nodes.len(), 2);
        assert_eq!(opts.nodes[1], "redis://node2:6379");
    }

    #[test]
    fn options_with_sentinel_sets_sentinel_mode() {
        let opts =
            IoredisStateAdapterOptions::new("redis://sentinel1:26379").with_sentinel("mymaster");
        assert!(opts.is_sentinel());
        assert_eq!(opts.sentinel_name.as_deref(), Some("mymaster"));
    }

    #[test]
    fn options_with_key_prefix_overrides_the_default() {
        let opts = IoredisStateAdapterOptions::new("redis://node1:6379").with_key_prefix("ns:");
        assert_eq!(opts.effective_key_prefix(), "ns:");
    }

    #[test]
    fn adapter_accessors_expose_options() {
        let adapter = IoredisStateAdapter::new(
            IoredisStateAdapterOptions::new("redis://n1")
                .with_node("redis://n2")
                .with_sentinel("mymaster")
                .with_key_prefix("ns:"),
        );
        assert_eq!(adapter.nodes().len(), 2);
        assert_eq!(adapter.sentinel_name(), Some("mymaster"));
        assert_eq!(adapter.key_prefix(), "ns:");
    }

    #[test]
    fn adapter_prefixed_key_concatenates() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        assert_eq!(adapter.prefixed_key("foo"), "chat:foo");
    }

    #[test]
    fn adapter_get_returns_not_connected_until_client_lands() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        match block_on(adapter.get("k")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_set_returns_not_connected_until_client_lands() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        match block_on(adapter.set("k", serde_json::json!(1), None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_returns_not_connected_until_client_lands() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        match block_on(adapter.delete("k")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_append_to_list_returns_not_connected_until_client_lands() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        match block_on(adapter.append_to_list("k", serde_json::json!(1), None, None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_get_list_returns_not_connected_until_client_lands() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        match block_on(adapter.get_list("k", None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    // ---------- upstream js-only-documented cases (per slice-380 pattern) ----------
    //
    // The following 4 upstream `index.test.ts` cases are js-only or
    // require a live Redis cluster and have no matching Rust test:
    //
    // - `should export createIoRedisState function`: JS module-loader
    //   check (`typeof createIoRedisState === "function"`). Rust's
    //   module system makes the export visible at compile time.
    // - 3 `describe.skip("integration tests")` cases — explicitly
    //   skipped upstream too; would need a live Redis cluster
    //   (cluster mode + Sentinel).
    //
    // Remaining upstream cases are mapped (5 method-existence mapped
    // to NotConnected smoke tests below + 8 NotConnected smoke
    // tests + 3 generate_token additive tests).

    // ---------- upstream "should have X method" mappings (2 of 5) ----------
    // 1:1 with upstream `index.test.ts` cases:
    //
    // - `should have appendToList method` → mapped to
    //   `adapter_append_to_list_returns_not_connected_until_client_lands`
    //   above (calling the method proves it exists; the upstream
    //   test only asserts `typeof adapter.appendToList === "function"`).
    // - `should have getList method` → mapped to
    //   `adapter_get_list_returns_not_connected_until_client_lands`.
    //
    // The remaining 3 upstream method-existence cases (`enqueue` /
    // `dequeue` / `queueDepth`) are deferred until the chat-sdk-chat
    // `StateAdapter` trait gets extended with the queue-primitive
    // methods (currently only `state-memory` has them as inherent
    // methods, not trait surface). They're tracked as deferred in
    // `docs/chat/goal-refinements.md`.

    #[test]
    fn adapter_set_if_not_exists_returns_not_connected_until_client_lands() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        match block_on(adapter.set_if_not_exists("k", serde_json::json!(1), None)) {
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
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        assert!(block_on(adapter.connect()).is_ok());
    }

    #[test]
    fn adapter_disconnect_default_trait_impl_is_no_op() {
        let adapter = IoredisStateAdapter::new(IoredisStateAdapterOptions::new("redis://n1"));
        assert!(block_on(adapter.disconnect()).is_ok());
    }
}
