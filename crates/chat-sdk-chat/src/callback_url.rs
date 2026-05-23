//! Callback-URL token plumbing for button-action wiring.
//!
//! 1:1 port (in progress) of `packages/chat/src/callback-url.ts`.
//!
//! **What this module ships (pure helpers; no state or network I/O):**
//!
//! - [`CALLBACK_TOKEN_PREFIX`] / [`CALLBACK_CACHE_KEY_PREFIX`] /
//!   [`CALLBACK_TTL_MS`] constants matching upstream values.
//! - [`encode_callback_value`] / [`decode_callback_value`] — pure
//!   token (de)serialization (1:1 with upstream).
//! - [`is_callback_value`] — predicate matching upstream's inline
//!   `value.startsWith(CALLBACK_TOKEN_PREFIX)` checks.
//! - [`callback_cache_key`] — formatter matching upstream's inline
//!   `${CALLBACK_CACHE_KEY_PREFIX}${token}` template.
//!
//! [`CallbackUrlStore`] — the state-bound `resolveCallbackUrl` /
//! token-issue surface, ported in slice 120 after the Phase 1.5
//! StateAdapter trait extension.
//!
//! **What is still deferred:**
//!
//! - `postToCallbackUrl` — requires an HTTP client. Lands once a
//!   default HTTP client wires into the workspace (likely `reqwest`
//!   or `ureq`).

/// Token prefix that marks a `button.value` as a callback-URL handle.
/// 1:1 port of upstream `const CALLBACK_TOKEN_PREFIX = "__cb:"`.
pub const CALLBACK_TOKEN_PREFIX: &str = "__cb:";

/// State-store key prefix for stored callback URLs. 1:1 port of
/// upstream `const CALLBACK_CACHE_KEY_PREFIX = "chat:callback:"`.
pub const CALLBACK_CACHE_KEY_PREFIX: &str = "chat:callback:";

/// TTL applied to stored callback URLs. 1:1 port of upstream
/// `const CALLBACK_TTL_MS = 30 * 24 * 60 * 60 * 1000` (30 days in
/// milliseconds).
pub const CALLBACK_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Encode a token into the `button.value` placeholder format. 1:1 port
/// of upstream `encodeCallbackValue(token): string`.
pub fn encode_callback_value(token: &str) -> String {
    format!("{CALLBACK_TOKEN_PREFIX}{token}")
}

/// Result of [`decode_callback_value`]. The single optional field
/// matches upstream's `{ callbackToken: string | undefined }` shape so
/// adapter callers can destructure the same way they do in TypeScript.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DecodedCallback {
    /// The decoded token when `value` carried the
    /// [`CALLBACK_TOKEN_PREFIX`]; `None` otherwise.
    pub callback_token: Option<String>,
}

/// Decode a callback-URL `button.value`. 1:1 port of upstream
/// `decodeCallbackValue(value): { callbackToken: string | undefined }`.
///
/// Passing `None` or a value without the prefix returns
/// `DecodedCallback::default()`, matching upstream's
/// `{ callbackToken: undefined }` branch.
pub fn decode_callback_value(value: Option<&str>) -> DecodedCallback {
    match value {
        Some(v) if v.starts_with(CALLBACK_TOKEN_PREFIX) => DecodedCallback {
            callback_token: Some(v[CALLBACK_TOKEN_PREFIX.len()..].to_string()),
        },
        _ => DecodedCallback::default(),
    }
}

/// Predicate — does this `button.value` carry the callback-URL prefix?
/// 1:1 with upstream's inline `value.startsWith(CALLBACK_TOKEN_PREFIX)`
/// checks used by `processCardCallbackUrls` and `resolveCallbackUrl`.
pub fn is_callback_value(value: &str) -> bool {
    value.starts_with(CALLBACK_TOKEN_PREFIX)
}

/// Build the state-store key for a callback token. 1:1 with upstream's
/// inline `${CALLBACK_CACHE_KEY_PREFIX}${token}` template used by every
/// callback-URL state path (`resolveCallbackUrl`, `processCardCallbackUrls`,
/// `postToCallbackUrl`).
pub fn callback_cache_key(token: &str) -> String {
    format!("{CALLBACK_CACHE_KEY_PREFIX}{token}")
}

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{StateAdapter, StateResult};

/// State-bound callback-URL token store. 1:1 port of upstream's
/// `processCardCallbackUrls` / `resolveCallbackUrl` state path (the
/// `postToCallbackUrl` HTTP path is deferred until a default HTTP
/// client lands).
///
/// Wraps an [`Arc<dyn StateAdapter>`] and offers:
/// - [`Self::issue`] — generate a fresh token, store the URL under it,
///   and return the encoded `button.value`.
/// - [`Self::resolve`] — look up the URL behind a previously-issued
///   `button.value`.
#[derive(Clone)]
pub struct CallbackUrlStore {
    state: Arc<dyn StateAdapter>,
    ttl_ms: u64,
}

impl std::fmt::Debug for CallbackUrlStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackUrlStore")
            .field("state", &self.state)
            .field("ttl_ms", &self.ttl_ms)
            .finish()
    }
}

fn generate_callback_token() -> String {
    // Uniqueness within a single SDK instance is sufficient; an
    // atomic counter + timestamp gives that without a uuid dep
    // (same pattern as transcripts::generate_id).
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("cb-{t:016x}-{n:016x}")
}

impl CallbackUrlStore {
    /// 1:1 port of upstream's implicit constructor at
    /// `processCardCallbackUrls` callsites. TTL defaults to
    /// [`CALLBACK_TTL_MS`].
    pub fn new(state: Arc<dyn StateAdapter>) -> Self {
        Self {
            state,
            ttl_ms: CALLBACK_TTL_MS,
        }
    }

    /// Override the TTL. Mirrors upstream's `options.ttlMs` override
    /// (used by tests + adapters that want shorter retention).
    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    /// Effective TTL applied to stored callback URLs.
    pub fn ttl_ms(&self) -> u64 {
        self.ttl_ms
    }

    /// Issue a fresh callback token for `url`, store the mapping in
    /// the state backend (TTL = [`Self::ttl_ms`]), and return the
    /// encoded `button.value` that should ship on the card. 1:1 with
    /// the token-issue portion of upstream `processCardCallbackUrls`.
    pub async fn issue(&self, url: &str) -> StateResult<String> {
        let token = generate_callback_token();
        let key = callback_cache_key(&token);
        self.state
            .set(
                &key,
                serde_json::Value::String(url.to_string()),
                Some(self.ttl_ms),
            )
            .await?;
        Ok(encode_callback_value(&token))
    }

    /// Look up the URL behind a callback `button.value`. 1:1 port of
    /// upstream `resolveCallbackUrl(value)`. Returns `None` when:
    /// - `value` is `None`,
    /// - `value` doesn't carry the [`CALLBACK_TOKEN_PREFIX`],
    /// - the token is unknown / expired in the state backend, or
    /// - the stored value isn't a JSON string (shouldn't happen for
    ///   tokens we issued, but we silently drop other shapes the way
    ///   upstream does).
    pub async fn resolve(&self, value: Option<&str>) -> StateResult<Option<String>> {
        let Some(token) = decode_callback_value(value).callback_token else {
            return Ok(None);
        };
        let key = callback_cache_key(&token);
        Ok(self
            .state
            .get(&key)
            .await?
            .and_then(|v| v.as_str().map(str::to_owned)))
    }
}

#[cfg(test)]
mod tests {
    //! 1:1 port of the `encodeCallbackValue` / `decodeCallbackValue`
    //! suite in `packages/chat/src/callback-url.test.ts` (5 of 17 upstream
    //! cases; the remaining 12 cases exercise stateful and network paths
    //! deferred to the follow-up slice).
    use super::*;

    #[test]
    fn encode_callback_value_prefixes_the_token() {
        assert_eq!(encode_callback_value("abc123"), "__cb:abc123");
    }

    #[test]
    fn decode_callback_value_returns_the_token_for_an_encoded_value() {
        let decoded = decode_callback_value(Some("__cb:xyz"));
        assert_eq!(decoded.callback_token.as_deref(), Some("xyz"));
    }

    #[test]
    fn decode_callback_value_returns_none_for_regular_values() {
        let decoded = decode_callback_value(Some("just-a-value"));
        assert!(decoded.callback_token.is_none());
    }

    #[test]
    fn decode_callback_value_returns_none_for_absent_input() {
        let decoded = decode_callback_value(None);
        assert!(decoded.callback_token.is_none());
    }

    #[test]
    fn encode_and_decode_round_trip() {
        let encoded = encode_callback_value("round-trip-token");
        let decoded = decode_callback_value(Some(&encoded));
        assert_eq!(decoded.callback_token.as_deref(), Some("round-trip-token"));
    }

    // ---------- slice 106: additional pure helpers ----------

    #[test]
    fn is_callback_value_detects_the_prefix() {
        assert!(is_callback_value("__cb:token"));
        assert!(is_callback_value("__cb:")); // empty token still has prefix
    }

    #[test]
    fn is_callback_value_rejects_values_without_the_prefix() {
        assert!(!is_callback_value("plain-value"));
        assert!(!is_callback_value(""));
        // The prefix is case-sensitive (matching upstream's `startsWith`).
        assert!(!is_callback_value("__CB:upper"));
    }

    #[test]
    fn callback_cache_key_concatenates_prefix_and_token() {
        assert_eq!(callback_cache_key("abc"), "chat:callback:abc");
    }

    #[test]
    fn callback_cache_key_accepts_empty_token() {
        assert_eq!(callback_cache_key(""), "chat:callback:");
    }

    #[test]
    fn encode_callback_value_accepts_empty_token() {
        // The encoder is a pure prefix concat, so an empty token round
        // -trips to a value whose decoded token is the empty string.
        let encoded = encode_callback_value("");
        assert_eq!(encoded, "__cb:");
        let decoded = decode_callback_value(Some(&encoded));
        assert_eq!(decoded.callback_token.as_deref(), Some(""));
    }

    // ---------- slice 120: CallbackUrlStore class ----------

    use crate::types::{StateAdapter, StateAdapterError, StateResult};
    use futures_executor::block_on;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MockState {
        kv: Mutex<HashMap<String, serde_json::Value>>,
    }

    #[async_trait::async_trait]
    impl StateAdapter for MockState {
        async fn get(&self, key: &str) -> StateResult<Option<serde_json::Value>> {
            let kv = self
                .kv
                .lock()
                .map_err(|_| StateAdapterError::NotConnected)?;
            Ok(kv.get(key).cloned())
        }
        async fn set(
            &self,
            key: &str,
            value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            let mut kv = self
                .kv
                .lock()
                .map_err(|_| StateAdapterError::NotConnected)?;
            kv.insert(key.to_string(), value);
            Ok(())
        }
        async fn delete(&self, key: &str) -> StateResult<()> {
            let mut kv = self
                .kv
                .lock()
                .map_err(|_| StateAdapterError::NotConnected)?;
            kv.remove(key);
            Ok(())
        }
        async fn append_to_list(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn get_list(
            &self,
            _key: &str,
            _limit: Option<usize>,
        ) -> StateResult<Vec<serde_json::Value>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn callback_url_store_issue_returns_an_encoded_value() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        let value = block_on(store.issue("https://example.com/cb")).unwrap();
        assert!(is_callback_value(&value));
        assert!(value.starts_with(CALLBACK_TOKEN_PREFIX));
    }

    #[test]
    fn callback_url_store_resolve_returns_the_stored_url() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        let value = block_on(store.issue("https://example.com/cb")).unwrap();
        let resolved = block_on(store.resolve(Some(&value))).unwrap();
        assert_eq!(resolved.as_deref(), Some("https://example.com/cb"));
    }

    #[test]
    fn callback_url_store_resolve_returns_none_for_none_input() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        let resolved = block_on(store.resolve(None)).unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn callback_url_store_resolve_returns_none_for_plain_values() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        let resolved = block_on(store.resolve(Some("not-a-callback-value"))).unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn callback_url_store_resolve_returns_none_for_unknown_tokens() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        let fake = encode_callback_value("never-issued");
        let resolved = block_on(store.resolve(Some(&fake))).unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn callback_url_store_default_ttl_matches_upstream_constant() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        assert_eq!(store.ttl_ms(), CALLBACK_TTL_MS);
    }

    #[test]
    fn callback_url_store_with_ttl_ms_overrides_the_default() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state).with_ttl_ms(60_000);
        assert_eq!(store.ttl_ms(), 60_000);
    }

    #[test]
    fn callback_url_store_issue_produces_unique_tokens_for_separate_urls() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let store = CallbackUrlStore::new(state);
        let a = block_on(store.issue("https://example.com/a")).unwrap();
        let b = block_on(store.issue("https://example.com/b")).unwrap();
        assert_ne!(a, b);
        assert_eq!(
            block_on(store.resolve(Some(&a))).unwrap().as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(
            block_on(store.resolve(Some(&b))).unwrap().as_deref(),
            Some("https://example.com/b")
        );
    }
}
