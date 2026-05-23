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
//! **What is deferred:**
//!
//! - `processCardCallbackUrls`, `resolveCallbackUrl`,
//!   `postToCallbackUrl` — these require the `StateAdapter` trait to
//!   carry concrete async `get`/`set` methods (currently the trait is
//!   the empty placeholder defined in [`crate::types::StateAdapter`])
//!   and an HTTP client. They land in a follow-up slice once
//!   `StateAdapter` is extended and a default HTTP client is wired in.

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
}
