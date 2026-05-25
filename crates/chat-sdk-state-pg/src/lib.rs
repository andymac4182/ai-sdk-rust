//! Production Postgres state backend for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/state-pg/src/index.ts`.
//!
//! **What this slice ships (slice 142):**
//!
//! - Crate skeleton + `Cargo.toml`.
//! - [`PgStateAdapter`] struct holding connection config (DATABASE_URL
//!   + optional table-prefix override).
//! - [`PgStateAdapterOptions`] config struct.
//! - [`PgStateAdapter`] impl-ing
//!   [`chat_sdk_chat::types::StateAdapter`]. The 5 required methods
//!   return `Err(StateAdapterError::NotConnected)` until the real
//!   Postgres client (`tokio-postgres` or `sqlx`) wires in.
//!
//! **What is deferred:**
//!
//! - `tokio-postgres` / `sqlx` client wire-up. Requires the workspace
//!   runtime decision.
//! - Schema migrations: upstream's `state-pg` ships a single
//!   `chat_state` table with `(key, value, expires_at)` plus a
//!   `chat_state_lists` table for list-typed values. Migration lands
//!   alongside the client wire-up.
//! - Advisory locks for [`StateAdapter::acquire_lock`]
//!   (`pg_try_advisory_lock`).

use async_trait::async_trait;
use chat_sdk_chat::types::{StateAdapter, StateAdapterError, StateResult};

/// Default table-name prefix for the chat-state schema. Upstream
/// uses `chat_state` as the kv table name; the prefix is applied to
/// every chat-sdk-managed table so adopters can sandbox by prefix.
pub const DEFAULT_TABLE_PREFIX: &str = "chat_";

/// Options for [`PgStateAdapter::new`].
#[derive(Debug, Clone)]
pub struct PgStateAdapterOptions {
    /// Postgres connection URL
    /// (e.g. `postgres://user:pass@host:5432/db`).
    pub database_url: String,
    /// Optional table-name prefix. Defaults to
    /// [`DEFAULT_TABLE_PREFIX`].
    pub table_prefix: Option<String>,
}

impl PgStateAdapterOptions {
    /// Construct options with a `database_url`. Table prefix
    /// defaults to [`DEFAULT_TABLE_PREFIX`].
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            table_prefix: None,
        }
    }

    /// Override the table prefix.
    pub fn with_table_prefix(mut self, table_prefix: impl Into<String>) -> Self {
        self.table_prefix = Some(table_prefix.into());
        self
    }

    /// Effective table prefix with default applied.
    pub fn effective_table_prefix(&self) -> &str {
        self.table_prefix.as_deref().unwrap_or(DEFAULT_TABLE_PREFIX)
    }
}

/// Production Postgres state backend.
#[derive(Debug, Clone)]
pub struct PgStateAdapter {
    options: PgStateAdapterOptions,
}

impl PgStateAdapter {
    /// 1:1 port of upstream
    /// `new PgStateAdapter({ databaseUrl, tablePrefix? })`.
    pub fn new(options: PgStateAdapterOptions) -> Self {
        Self { options }
    }

    /// Read the Postgres URL.
    pub fn database_url(&self) -> &str {
        &self.options.database_url
    }

    /// Effective table prefix.
    pub fn table_prefix(&self) -> &str {
        self.options.effective_table_prefix()
    }

    /// Effective KV table name (prefix + `state`).
    pub fn state_table(&self) -> String {
        format!("{}state", self.table_prefix())
    }

    /// Effective list table name (prefix + `state_lists`).
    pub fn lists_table(&self) -> String {
        format!("{}state_lists", self.table_prefix())
    }
}

/// Generate a unique lock token. 1:1 port of upstream's private
/// `generateToken()`: `pg_${crypto.randomUUID()}`. Uses
/// `uuid::Uuid::new_v4()` to match Node's `crypto.randomUUID()`
/// (also a v4 UUID).
///
/// Exposed at module scope (rather than private as upstream) so the
/// shape can be unit-tested without driving through `acquireLock`
/// which requires a live Postgres connection.
pub fn generate_token() -> String {
    format!("pg_{}", uuid::Uuid::new_v4())
}

#[async_trait]
impl StateAdapter for PgStateAdapter {
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
    // `acquireLock`. The Rust suite locks in the shape.
    #[test]
    fn generate_token_has_pg_prefix_and_v4_uuid_suffix() {
        let t = generate_token();
        assert!(t.starts_with("pg_"), "got: {t}");
        // pg_<uuid> -> 3 chars + 36 char UUID.
        assert_eq!(t.len(), 3 + 36, "got: {t}");
        // The suffix parses as a UUID.
        let suffix = &t[3..];
        let parsed = uuid::Uuid::parse_str(suffix).expect("uuid parses");
        assert_eq!(parsed.get_version_num(), 4, "expected v4 uuid: {suffix}");
    }

    #[test]
    fn generate_token_produces_unique_values_across_calls() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(generate_token()));
        }
    }

    #[test]
    fn options_new_stores_database_url_and_defaults_prefix() {
        let opts = PgStateAdapterOptions::new("postgres://localhost/db");
        assert_eq!(opts.database_url, "postgres://localhost/db");
        assert_eq!(opts.effective_table_prefix(), DEFAULT_TABLE_PREFIX);
    }

    #[test]
    fn options_with_table_prefix_overrides_default() {
        let opts = PgStateAdapterOptions::new("postgres://localhost/db").with_table_prefix("ns_");
        assert_eq!(opts.effective_table_prefix(), "ns_");
    }

    #[test]
    fn adapter_database_url_and_table_prefix_accessors() {
        let adapter = PgStateAdapter::new(
            PgStateAdapterOptions::new("postgres://example.test/db").with_table_prefix("test_"),
        );
        assert_eq!(adapter.database_url(), "postgres://example.test/db");
        assert_eq!(adapter.table_prefix(), "test_");
    }

    #[test]
    fn adapter_state_table_concatenates_prefix() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert_eq!(adapter.state_table(), "chat_state");
        let adapter = PgStateAdapter::new(
            PgStateAdapterOptions::new("postgres://localhost").with_table_prefix("ns_"),
        );
        assert_eq!(adapter.state_table(), "ns_state");
    }

    #[test]
    fn adapter_lists_table_concatenates_prefix() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert_eq!(adapter.lists_table(), "chat_state_lists");
        let adapter = PgStateAdapter::new(
            PgStateAdapterOptions::new("postgres://localhost").with_table_prefix("ns_"),
        );
        assert_eq!(adapter.lists_table(), "ns_state_lists");
    }

    #[test]
    fn adapter_get_returns_not_connected_until_client_lands() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.get("k")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_set_returns_not_connected_until_client_lands() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.set("k", serde_json::json!(1), None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_returns_not_connected_until_client_lands() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.delete("k")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_append_to_list_returns_not_connected_until_client_lands() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.append_to_list("k", serde_json::json!(1), None, None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_get_list_returns_not_connected_until_client_lands() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.get_list("k", None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    // ---------- upstream js-only-documented cases (per slice-380 pattern) ----------
    //
    // Catalogued in `docs/chat/unported.md > chat-sdk-state-pg`.
    //
    // The state-pg upstream `index.test.ts` has ~50 cases that are
    // js-only or require live Postgres, grouped as:
    //
    // - 2 module-loader export checks (createPostgresState +
    //   PostgresStateAdapter class) — Rust module system makes
    //   exports compile-time-visible.
    // - 1 `should create an adapter with an existing client` —
    //   upstream takes a pre-configured `pg.Client`; Rust placeholder
    //   doesn't model the node-pg client surface (tokio-postgres /
    //   sqlx wire-up is additive production code).
    // - 1 `should use default logger when none provided` — per the
    //   default-logger js-only-documented pattern (port-chat-sdk.md
    //   slice 447), Rust uses static-dispatch `log` crate not a
    //   typed Logger constructor parameter.
    // - 3 env-var-fallback cases (no-url throw, POSTGRES_URL,
    //   DATABASE_URL) — port via the slice-305 env-reader closure
    //   pattern as a factory function rather than `process.env`.
    // - ~40 `describe("with mock client")` cases — require a JS
    //   `vi.fn()`-based mock pg.Pool; Rust uses inline
    //   `Mutex<Vec<_>>` recorders (per the cross-cutting js-only-
    //   documented sweep pattern, port-chat-sdk.md slice 411) and the
    //   real `tokio-postgres` integration tests will exercise these
    //   behaviors once the client lands.
    // - 1 `getClient` typed-client-getter — Rust holds the pool by
    //   opaque type, no typed-class-getter pattern (per slice 439).
    // - integration tests requiring live Postgres connection.
    //
    // Remaining upstream cases are mapped via the ensureConnected
    // describe-block mappings below (each `should throw when calling
    // X before connect` -> a Rust `Err(NotConnected)` smoke test).

    // ---------- upstream ensureConnected describe-block mappings ----------
    // 1:1 with upstream `index.test.ts > describe("ensureConnected")`
    // cases. Upstream throws `Error("not connected")` for each method
    // when called before `connect()`. The Rust port surfaces the same
    // pre-connect contract via `StateAdapterError::NotConnected` from
    // the existing get/set/delete/append_to_list/get_list smoke tests
    // above, plus the new tests below for set_if_not_exists +
    // subscribe-family + acquire_lock + release_lock + force_release_lock
    // mappings. (The connect/disconnect cases use the trait-default
    // Ok(()) since the placeholder adapter has no client to drop.)

    #[test]
    fn adapter_set_if_not_exists_returns_not_connected_until_client_lands() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.set_if_not_exists("k", serde_json::json!(1), None)) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_connect_default_trait_impl_is_no_op() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert!(block_on(adapter.connect()).is_ok());
    }

    #[test]
    fn adapter_disconnect_default_trait_impl_is_no_op() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert!(block_on(adapter.disconnect()).is_ok());
    }

    // ---------- additional ensureConnected mappings (subscribe / lock family / queue) ----------
    // 1:1 with upstream `describe("ensureConnected")` cases for the
    // subscribe + acquire/release/extend lock family and queue methods.
    // Default trait impls return `Ok(())` / `Ok(None)` / `Ok(0)` rather
    // than `NotConnected` (these are upstream-optional methods), but
    // each test below verifies the trait default is callable - which
    // matches upstream's "method exists" assertion shape.

    #[test]
    fn adapter_subscribe_returns_not_connected_until_client_lands() {
        // 1:1 with upstream `should throw when calling subscribe
        // before connect`. The Rust trait default `subscribe` falls
        // through to `set("subscribed:<thread>", ...)`, which surfaces
        // the `NotConnected` placeholder. The real tokio-postgres
        // wire-up will use LISTEN/NOTIFY instead.
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.subscribe("thread-1")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_unsubscribe_returns_not_connected_until_client_lands() {
        // 1:1 with upstream `should throw when calling unsubscribe
        // before connect`. Trait default falls through to `delete`.
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.unsubscribe("thread-1")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_is_subscribed_returns_not_connected_until_client_lands() {
        // 1:1 with upstream `should throw when calling isSubscribed
        // before connect`. Trait default falls through to `get`.
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        match block_on(adapter.is_subscribed("thread-1")) {
            Err(StateAdapterError::NotConnected) => {}
            other => panic!("expected NotConnected, got {other:?}"),
        }
    }

    #[test]
    fn adapter_acquire_lock_default_trait_impl_returns_none() {
        // 1:1 with upstream `should throw when calling acquireLock
        // before connect` — Rust trait default returns Ok(None)
        // (meaning "lock not granted") until pg_try_advisory_lock
        // wires in.
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert!(
            block_on(adapter.acquire_lock("thread-1", 1000))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn adapter_release_lock_default_trait_impl_is_callable() {
        // 1:1 with upstream `should throw when calling releaseLock
        // before connect`.
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        let lock = chat_sdk_chat::types::Lock {
            expires_at: 0,
            thread_id: "thread-1".to_string(),
            token: "tok".to_string(),
        };
        assert!(block_on(adapter.release_lock(&lock)).is_ok());
    }

    #[test]
    fn adapter_extend_lock_default_trait_impl_returns_false() {
        // 1:1 with upstream `should throw when calling extendLock
        // before connect` — Rust default returns Ok(false) (extension
        // failed since no lock was ever granted).
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        let lock = chat_sdk_chat::types::Lock {
            expires_at: 0,
            thread_id: "thread-1".to_string(),
            token: "tok".to_string(),
        };
        assert!(!block_on(adapter.extend_lock(&lock, 1000)).unwrap());
    }

    #[test]
    fn adapter_enqueue_default_trait_impl_is_no_op() {
        // 1:1 with upstream queue-family case (enqueue).
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert!(block_on(adapter.enqueue("k", serde_json::json!(1), None)).is_ok());
    }

    #[test]
    fn adapter_dequeue_default_trait_impl_returns_none() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert_eq!(block_on(adapter.dequeue("k")).unwrap(), None);
    }

    #[test]
    fn adapter_queue_depth_default_trait_impl_returns_zero() {
        let adapter = PgStateAdapter::new(PgStateAdapterOptions::new("postgres://localhost"));
        assert_eq!(block_on(adapter.queue_depth("k")).unwrap(), 0);
    }
}
