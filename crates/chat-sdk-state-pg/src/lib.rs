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

    #[test]
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
}
