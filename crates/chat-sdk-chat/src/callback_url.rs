//! Callback-URL token plumbing for button-action wiring.
//!
//! 1:1 port of `packages/chat/src/callback-url.ts`.
//!
//! **What this module ships (pure helpers + state path + HTTP path):**
//!
//! - [`CALLBACK_TOKEN_PREFIX`] / [`CALLBACK_CACHE_KEY_PREFIX`] /
//!   [`CALLBACK_TTL_MS`] constants matching upstream values.
//! - [`encode_callback_value`] / [`decode_callback_value`] — pure
//!   token (de)serialization (1:1 with upstream).
//! - [`is_callback_value`] — predicate matching upstream's inline
//!   `value.startsWith(CALLBACK_TOKEN_PREFIX)` checks.
//! - [`callback_cache_key`] — formatter matching upstream's inline
//!   `${CALLBACK_CACHE_KEY_PREFIX}${token}` template.
//! - [`CallbackUrlStore`] — the state-bound `resolveCallbackUrl` /
//!   token-issue surface (slice 120).
//! - [`post_to_callback_url`] — HTTP POST wired through an injected
//!   [`HttpPoster`] trait so callers can plug their preferred client
//!   without forcing a single runtime choice on the workspace. The
//!   upstream implementation simply calls `globalThis.fetch`; the
//!   Rust port retains that shape via `&dyn HttpPoster`.

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

    /// Legacy resolve. See module-level [`resolve_callback_url`] for
    /// the upstream-matching `{url, original_value}` shape.
    /// passthrough — kept for the simpler `state.set(token, url-string)`
    /// shape used by some callers.) Use [`resolve_callback_url`] for
    /// the upstream-matching `{url, original_value}` shape.
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

/// Stored callback metadata. 1:1 with upstream
/// `interface StoredCallback { url: string; originalValue?: string }`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoredCallback {
    pub url: String,
    #[serde(
        rename = "originalValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_value: Option<String>,
}

/// Process a [`CardElement`]: replace any button's `callback_url`
/// with a fresh `__cb:<token>` value and store the URL plus original
/// value in `state` under `chat:callback:<token>`. 1:1 port of
/// upstream `processCardCallbackUrls(card, stateAdapter)`. Returns
/// the card unchanged (by value-equality) when no buttons carry a
/// callback URL.
pub async fn process_card_callback_urls(
    card: &crate::cards::CardElement,
    state: &dyn StateAdapter,
) -> StateResult<crate::cards::CardElement> {
    if !has_callback_buttons(&card.children) {
        return Ok(card.clone());
    }
    let children = process_children(&card.children, state).await?;
    Ok(crate::cards::CardElement {
        title: card.title.clone(),
        subtitle: card.subtitle.clone(),
        image_url: card.image_url.clone(),
        kind: card.kind,
        children,
    })
}

/// Resolve a callback token back to its stored [`StoredCallback`].
/// 1:1 port of upstream `resolveCallbackUrl(token, stateAdapter)`.
/// Supports both the modern `{url, originalValue}` object shape and
/// the legacy plain-string format (`stored: string` -> `{url: stored}`).
pub async fn resolve_callback_url(
    token: &str,
    state: &dyn StateAdapter,
) -> StateResult<Option<StoredCallback>> {
    let key = callback_cache_key(token);
    let Some(stored) = state.get(&key).await? else {
        return Ok(None);
    };
    if let Some(url) = stored.as_str() {
        return Ok(Some(StoredCallback {
            url: url.to_string(),
            original_value: None,
        }));
    }
    Ok(serde_json::from_value::<StoredCallback>(stored).ok())
}

fn has_callback_buttons(children: &[crate::cards::CardChild]) -> bool {
    for child in children {
        match child {
            crate::cards::CardChild::Actions(a) => {
                for el in &a.children {
                    if let crate::cards::ActionsChild::Button(b) = el
                        && b.callback_url
                            .as_deref()
                            .filter(|u| !u.is_empty())
                            .is_some()
                    {
                        return true;
                    }
                }
            }
            crate::cards::CardChild::Section(s) => {
                if has_callback_buttons(&s.children) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

async fn process_children(
    children: &[crate::cards::CardChild],
    state: &dyn StateAdapter,
) -> StateResult<Vec<crate::cards::CardChild>> {
    let mut result = Vec::with_capacity(children.len());
    for child in children {
        match child {
            crate::cards::CardChild::Actions(a) => {
                let processed = process_actions_element(a, state).await?;
                result.push(crate::cards::CardChild::Actions(processed));
            }
            crate::cards::CardChild::Section(s) => {
                let processed_children = Box::pin(process_children(&s.children, state)).await?;
                result.push(crate::cards::CardChild::Section(
                    crate::cards::SectionElement {
                        children: processed_children,
                        kind: s.kind,
                    },
                ));
            }
            other => result.push(other.clone()),
        }
    }
    Ok(result)
}

async fn process_actions_element(
    actions: &crate::cards::ActionsElement,
    state: &dyn StateAdapter,
) -> StateResult<crate::cards::ActionsElement> {
    let mut processed_children = Vec::with_capacity(actions.children.len());
    for child in &actions.children {
        match child {
            crate::cards::ActionsChild::Button(b)
                if b.callback_url
                    .as_deref()
                    .filter(|u| !u.is_empty())
                    .is_some() =>
            {
                let token = generate_callback_token();
                let stored = StoredCallback {
                    url: b.callback_url.clone().unwrap_or_default(),
                    original_value: b.value.clone(),
                };
                let value = serde_json::to_value(&stored)
                    .map_err(|e| crate::types::StateAdapterError::Io(Box::new(e)))?;
                state
                    .set(&callback_cache_key(&token), value, Some(CALLBACK_TTL_MS))
                    .await?;
                processed_children.push(crate::cards::ActionsChild::Button(
                    crate::cards::ButtonElement {
                        action_type: b.action_type,
                        callback_url: None,
                        disabled: b.disabled,
                        id: b.id.clone(),
                        label: b.label.clone(),
                        style: b.style,
                        kind: b.kind,
                        value: Some(encode_callback_value(&token)),
                    },
                ));
            }
            other => processed_children.push(other.clone()),
        }
    }
    Ok(crate::cards::ActionsElement {
        children: processed_children,
        kind: actions.kind,
    })
}

/// HTTP-client abstraction used by [`post_to_callback_url`]. 1:1
/// with upstream's `globalThis.fetch` dependency: callers inject
/// any client (reqwest, ureq, custom mock) that can POST a JSON
/// body to a URL and return either the HTTP status code on success
/// or a transport-level error on failure.
///
/// Mirrors the upstream test pattern of `vi.spyOn(globalThis,
/// "fetch")` — the trait sits at the same seam.
#[async_trait::async_trait]
pub trait HttpPoster: Send + Sync {
    /// POST `body` (JSON) to `url` with `Content-Type:
    /// application/json`. Return the HTTP status code on a completed
    /// request (even if non-2xx), or an `Err` for transport-level
    /// failures (DNS, TCP, TLS).
    async fn post_json(
        &self,
        url: &str,
        body: &str,
    ) -> Result<u16, Box<dyn std::error::Error + Send + Sync>>;
}

/// Result of [`post_to_callback_url`]. 1:1 with upstream's `{
/// error?, status? }` return shape: `error` is `None` on success,
/// `status` is set whenever the HTTP request completed (regardless
/// of status code).
#[derive(Debug, Default)]
pub struct CallbackPostResult {
    pub status: Option<u16>,
    pub error: Option<Box<dyn std::error::Error + Send + Sync>>,
}

/// 1:1 port of upstream `postToCallbackUrl(url, payload)`. Serialises
/// `payload` as JSON, POSTs it via [`HttpPoster::post_json`] with
/// `Content-Type: application/json`, and bundles the response into
/// [`CallbackPostResult`]:
///
/// - **2xx response** → `{ status: Some(code), error: None }`.
/// - **non-2xx response** → `{ status: Some(code), error: Some(...) }`.
/// - **transport error** → `{ status: None, error: Some(...) }`.
pub async fn post_to_callback_url(
    poster: &dyn HttpPoster,
    url: &str,
    payload: &serde_json::Value,
) -> CallbackPostResult {
    let body = payload.to_string();
    match poster.post_json(url, &body).await {
        Ok(status) if (200..300).contains(&status) => CallbackPostResult {
            status: Some(status),
            error: None,
        },
        Ok(status) => CallbackPostResult {
            status: Some(status),
            error: Some(format!("HTTP {status}").into()),
        },
        Err(error) => CallbackPostResult {
            status: None,
            error: Some(error),
        },
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

    // ---------- processCardCallbackUrls + resolveCallbackUrl (9 upstream cases) ----------

    use crate::cards::{
        ActionsChild, ActionsElement, ActionsKind, ButtonElement, ButtonKind, CardChild,
        CardElement, CardKind, SectionElement, SectionKind, TextElement, TextKind,
    };

    fn card(title: Option<&str>, children: Vec<CardChild>) -> CardElement {
        CardElement {
            title: title.map(str::to_owned),
            subtitle: None,
            image_url: None,
            kind: CardKind::Card,
            children,
        }
    }

    fn button_with_callback(id: &str, label: &str, callback_url: &str) -> ActionsChild {
        ActionsChild::Button(ButtonElement {
            action_type: None,
            callback_url: Some(callback_url.to_string()),
            disabled: None,
            id: id.to_string(),
            label: label.to_string(),
            style: None,
            kind: ButtonKind::Button,
            value: None,
        })
    }

    fn button_plain(id: &str, label: &str, value: Option<&str>) -> ActionsChild {
        ActionsChild::Button(ButtonElement {
            action_type: None,
            callback_url: None,
            disabled: None,
            id: id.to_string(),
            label: label.to_string(),
            style: None,
            kind: ButtonKind::Button,
            value: value.map(str::to_owned),
        })
    }

    fn extract_first_button_value(card: &CardElement) -> Option<String> {
        for child in &card.children {
            if let CardChild::Actions(a) = child {
                if let Some(ActionsChild::Button(b)) = a.children.first() {
                    return b.value.clone();
                }
            }
            if let CardChild::Section(s) = child {
                let nested = CardElement {
                    title: None,
                    subtitle: None,
                    image_url: None,
                    kind: CardKind::Card,
                    children: s.children.clone(),
                };
                if let Some(v) = extract_first_button_value(&nested) {
                    return Some(v);
                }
            }
        }
        None
    }

    #[test]
    fn process_returns_card_unchanged_when_no_buttons_have_callback_url() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let c = card(
            Some("Test"),
            vec![
                CardChild::Text(TextElement {
                    content: "Hello".to_string(),
                    style: None,
                    kind: TextKind::Text,
                }),
                CardChild::Actions(ActionsElement {
                    children: vec![button_plain("btn", "Click", None)],
                    kind: ActionsKind::Actions,
                }),
            ],
        );
        let result = block_on(process_card_callback_urls(&c, state.as_ref())).unwrap();
        assert_eq!(result, c);
    }

    #[test]
    fn process_encodes_callback_url_into_button_value_and_stores_in_state() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let c = card(
            Some("Test"),
            vec![CardChild::Actions(ActionsElement {
                children: vec![button_with_callback(
                    "approve",
                    "Approve",
                    "https://example.com/webhook/123",
                )],
                kind: ActionsKind::Actions,
            })],
        );
        let result = block_on(process_card_callback_urls(&c, state.as_ref())).unwrap();
        let v = extract_first_button_value(&result).expect("button value");
        assert!(is_callback_value(&v), "got: {v}");
        let token = decode_callback_value(Some(&v)).callback_token.unwrap();
        let resolved = block_on(resolve_callback_url(&token, state.as_ref()))
            .unwrap()
            .expect("resolved");
        assert_eq!(resolved.url, "https://example.com/webhook/123");
    }

    #[test]
    fn process_stores_original_value_in_state_alongside_callback_url() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let c = card(
            Some("Test"),
            vec![CardChild::Actions(ActionsElement {
                children: vec![ActionsChild::Button(ButtonElement {
                    action_type: None,
                    callback_url: Some("https://hook.example.com".to_string()),
                    disabled: None,
                    id: "btn".to_string(),
                    label: "Go".to_string(),
                    style: None,
                    kind: ButtonKind::Button,
                    value: Some("item-99".to_string()),
                })],
                kind: ActionsKind::Actions,
            })],
        );
        let result = block_on(process_card_callback_urls(&c, state.as_ref())).unwrap();
        let v = extract_first_button_value(&result).expect("button value");
        let token = decode_callback_value(Some(&v)).callback_token.unwrap();
        let resolved = block_on(resolve_callback_url(&token, state.as_ref()))
            .unwrap()
            .expect("resolved");
        assert_eq!(resolved.url, "https://hook.example.com");
        assert_eq!(resolved.original_value.as_deref(), Some("item-99"));
    }

    #[test]
    fn process_only_processes_buttons_with_callback_url_leaves_others_untouched() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let c = card(
            Some("Test"),
            vec![CardChild::Actions(ActionsElement {
                children: vec![
                    button_plain("normal", "Normal", Some("keep")),
                    button_with_callback("callback", "Callback", "https://example.com"),
                ],
                kind: ActionsKind::Actions,
            })],
        );
        let result = block_on(process_card_callback_urls(&c, state.as_ref())).unwrap();
        let actions = result
            .children
            .iter()
            .find_map(|c| match c {
                CardChild::Actions(a) => Some(a),
                _ => None,
            })
            .unwrap();
        let normal = match &actions.children[0] {
            ActionsChild::Button(b) => b,
            _ => panic!(),
        };
        let cb = match &actions.children[1] {
            ActionsChild::Button(b) => b,
            _ => panic!(),
        };
        assert_eq!(normal.value.as_deref(), Some("keep"));
        assert!(
            cb.value
                .as_deref()
                .is_some_and(|v| v.starts_with(CALLBACK_TOKEN_PREFIX)),
            "got: {:?}",
            cb.value
        );
    }

    #[test]
    fn process_processes_buttons_nested_inside_sections() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let c = card(
            Some("Test"),
            vec![CardChild::Section(SectionElement {
                children: vec![
                    CardChild::Text(TextElement {
                        content: "Nested".to_string(),
                        style: None,
                        kind: TextKind::Text,
                    }),
                    CardChild::Actions(ActionsElement {
                        children: vec![button_with_callback(
                            "nested-btn",
                            "Go",
                            "https://example.com/nested",
                        )],
                        kind: ActionsKind::Actions,
                    }),
                ],
                kind: SectionKind::Section,
            })],
        );
        let result = block_on(process_card_callback_urls(&c, state.as_ref())).unwrap();
        let v = extract_first_button_value(&result).expect("nested button value");
        let token = decode_callback_value(Some(&v)).callback_token.unwrap();
        let resolved = block_on(resolve_callback_url(&token, state.as_ref()))
            .unwrap()
            .expect("resolved");
        assert_eq!(resolved.url, "https://example.com/nested");
    }

    #[test]
    fn process_does_not_mutate_the_original_card() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let c = card(
            Some("Test"),
            vec![CardChild::Actions(ActionsElement {
                children: vec![button_with_callback("btn", "Go", "https://example.com")],
                kind: ActionsKind::Actions,
            })],
        );
        let snapshot = c.clone();
        let _ = block_on(process_card_callback_urls(&c, state.as_ref())).unwrap();
        assert_eq!(c, snapshot);
    }

    #[test]
    fn resolve_returns_none_for_unknown_token() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let r = block_on(resolve_callback_url("nope", state.as_ref())).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn resolve_returns_stored_callback_with_url_and_original_value() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        let value = serde_json::json!({
            "url": "https://example.com",
            "originalValue": "v1",
        });
        block_on(state.set(&callback_cache_key("tok1"), value, None)).unwrap();
        let r = block_on(resolve_callback_url("tok1", state.as_ref()))
            .unwrap()
            .unwrap();
        assert_eq!(r.url, "https://example.com");
        assert_eq!(r.original_value.as_deref(), Some("v1"));
    }

    #[test]
    fn resolve_handles_legacy_string_format() {
        let state: Arc<dyn StateAdapter> = Arc::new(MockState::default());
        block_on(state.set(
            &callback_cache_key("tok2"),
            serde_json::Value::String("https://legacy.example.com".to_string()),
            None,
        ))
        .unwrap();
        let r = block_on(resolve_callback_url("tok2", state.as_ref()))
            .unwrap()
            .unwrap();
        assert_eq!(r.url, "https://legacy.example.com");
        assert!(r.original_value.is_none());
    }

    // ---------- postToCallbackUrl (3 upstream cases) ----------

    /// Mock HTTP poster — the Rust equivalent of `vi.spyOn(globalThis,
    /// "fetch")`. Records each call (url + body) and returns the
    /// configured response. `result` is `Ok(status)` for successful
    /// responses or `Err(message)` for transport errors.
    struct MockPoster {
        result: Mutex<Option<Result<u16, String>>>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl MockPoster {
        fn new(result: Result<u16, String>) -> Self {
            Self {
                result: Mutex::new(Some(result)),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl HttpPoster for MockPoster {
        async fn post_json(
            &self,
            url: &str,
            body: &str,
        ) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
            self.calls
                .lock()
                .unwrap()
                .push((url.to_string(), body.to_string()));
            match self
                .result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err("called twice".to_string()))
            {
                Ok(status) => Ok(status),
                Err(msg) => Err(msg.into()),
            }
        }
    }

    #[test]
    fn post_to_callback_url_posts_json_payload_to_the_url() {
        // 1:1 with upstream `it("POSTs JSON payload to the URL")`.
        let poster = MockPoster::new(Ok(200));
        let payload = serde_json::json!({
            "type": "action",
            "actionId": "approve",
        });
        let result = futures_executor::block_on(post_to_callback_url(
            &poster,
            "https://example.com/hook",
            &payload,
        ));
        assert!(result.error.is_none(), "got error: {:?}", result.error);
        let calls = poster.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "https://example.com/hook");
        // Body shape matches upstream's `JSON.stringify({...})` content,
        // but serde_json's `json!({...})` ordering for object Values is
        // alphabetical via BTreeMap (whereas JS preserves insertion
        // order). The byte-for-byte ordering diverges; assert on both
        // keys + values being present instead of a literal match.
        let body = &calls[0].1;
        assert!(body.contains(r#""type":"action""#), "got: {body}");
        assert!(body.contains(r#""actionId":"approve""#), "got: {body}");
    }

    #[test]
    fn post_to_callback_url_returns_error_for_non_2xx_responses() {
        // 1:1 with upstream `it("returns error for non-2xx responses")`.
        let poster = MockPoster::new(Ok(404));
        let result = futures_executor::block_on(post_to_callback_url(
            &poster,
            "https://example.com/hook",
            &serde_json::json!({}),
        ));
        assert!(result.error.is_some(), "expected error for 404 response");
        assert_eq!(result.status, Some(404));
    }

    #[test]
    fn post_to_callback_url_catches_fetch_errors_and_returns_them() {
        // 1:1 with upstream `it("catches fetch errors and returns them")`.
        let poster = MockPoster::new(Err("Network error".to_string()));
        let result = futures_executor::block_on(post_to_callback_url(
            &poster,
            "https://example.com/hook",
            &serde_json::json!({}),
        ));
        assert!(
            result.error.is_some(),
            "expected error for transport failure"
        );
        assert!(
            result.status.is_none(),
            "status should be unset on transport error"
        );
    }
}
