//! Telegram adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-telegram/src/index.ts`.
//!
//! **What this slice ships (slice 130):**
//!
//! - Crate skeleton + `Cargo.toml` (deps: chat-sdk-chat,
//!   async-trait, serde + serde_json).
//! - [`TelegramAdapter`] struct holding bot config (token + base
//!   URL) and impl-ing the chat-sdk [`chat_sdk_chat::types::Adapter`]
//!   trait with `name` overridden to `"telegram"`.
//! - [`TelegramAdapterOptions`] config struct (token + optional
//!   base URL override for testing).
//! - [`encode_thread_id`] / [`decode_thread_id`] / [`is_telegram_thread_id`] —
//!   pure helpers for the upstream `telegram:<chat_id>:<message_thread_id?>`
//!   wire format (1:1 with upstream's inline helpers).
//!
//! **What is deferred:**
//!
//! - HTTP I/O against `api.telegram.org` for the actual
//!   `post_message`/`post_object`/`fetch_subject`/... methods.
//!   Requires picking an HTTP client (`reqwest`/`ureq`) and an
//!   async runtime. Will land alongside the workspace-level
//!   runtime decision documented in
//!   `scripts/codex-goal-chat/port-chat-sdk.md`'s "Phase 2 /
//!   Phase 3 prep" section.
//! - Markdown / card rendering for Telegram's `MarkdownV2` /
//!   inline-keyboard layout.

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator. 1:1 with upstream
/// `export const ADAPTER_NAME = "telegram"`.
pub const ADAPTER_NAME: &str = "telegram";

/// Thread-id prefix Telegram-encoded thread ids carry. 1:1 with
/// upstream's inline `telegram:` namespace.
pub const THREAD_ID_PREFIX: &str = "telegram:";

/// Default Telegram Bot API base URL. 1:1 with upstream
/// `const DEFAULT_BASE_URL = "https://api.telegram.org"`.
pub const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

/// Options for [`TelegramAdapter::new`]. 1:1 with upstream
/// `interface TelegramAdapterOptions`.
#[derive(Debug, Clone)]
pub struct TelegramAdapterOptions {
    /// Telegram bot token (`<bot-id>:<secret>` from BotFather).
    pub token: String,
    /// Optional Bot API base URL override. Defaults to
    /// [`DEFAULT_BASE_URL`]. Used by tests + custom Telegram-API
    /// proxies.
    pub base_url: Option<String>,
}

impl TelegramAdapterOptions {
    /// Construct options with the token; base URL defaults to
    /// [`DEFAULT_BASE_URL`].
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            base_url: None,
        }
    }

    /// Override the base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Effective base URL with default applied.
    pub fn effective_base_url(&self) -> &str {
        self.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)
    }
}

/// Telegram adapter. 1:1 port (in progress) of upstream
/// `class TelegramAdapter implements Adapter`. The HTTP I/O methods
/// (`post_message`, `post_object`, `fetch_subject`, etc.) will land
/// once the workspace picks an async HTTP client; for now the
/// adapter only implements `name` and exposes the bot config so
/// downstream consumers can construct it.
#[derive(Debug, Clone)]
pub struct TelegramAdapter {
    options: TelegramAdapterOptions,
}

impl TelegramAdapter {
    /// 1:1 port of upstream `new TelegramAdapter({ token, baseUrl? })`.
    pub fn new(options: TelegramAdapterOptions) -> Self {
        Self { options }
    }

    /// Read the bot token (e.g. for constructing HTTPS request URIs).
    pub fn token(&self) -> &str {
        &self.options.token
    }

    /// Effective base URL.
    pub fn base_url(&self) -> &str {
        self.options.effective_base_url()
    }
}

#[async_trait]
impl Adapter for TelegramAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }
}

/// Encode a Telegram thread id. 1:1 with upstream's inline format:
/// `telegram:<chat_id>` or `telegram:<chat_id>:<thread_id>` when
/// the optional Telegram `message_thread_id` is present.
pub fn encode_thread_id(chat_id: i64, message_thread_id: Option<i64>) -> String {
    match message_thread_id {
        Some(tid) => format!("{THREAD_ID_PREFIX}{chat_id}:{tid}"),
        None => format!("{THREAD_ID_PREFIX}{chat_id}"),
    }
}

/// Components of a decoded Telegram thread id. 1:1 with upstream's
/// returned object shape from `decodeThreadId(threadId)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedTelegramThreadId {
    /// Telegram `chat_id` (always present).
    pub chat_id: i64,
    /// Optional Telegram `message_thread_id` (forum-style threads).
    pub message_thread_id: Option<i64>,
}

/// Decode a Telegram thread id. 1:1 port of upstream
/// `decodeThreadId(threadId)`. Returns `None` for any value that
/// doesn't carry the `telegram:` prefix or whose chat id can't be
/// parsed as an integer.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedTelegramThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let chat = parts.next()?.parse::<i64>().ok()?;
    let message_thread_id = match parts.next() {
        Some(t) => Some(t.parse::<i64>().ok()?),
        None => None,
    };
    Some(DecodedTelegramThreadId {
        chat_id: chat,
        message_thread_id,
    })
}

/// Predicate: does this thread id belong to the Telegram adapter?
/// 1:1 with upstream's inline `threadId.startsWith("telegram:")`.
pub fn is_telegram_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the [`TelegramAdapter`] surface. The
    //! HTTP-bound methods will land once the workspace commits to an
    //! async runtime; these tests lock in the pure config + thread-id
    //! helpers.
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_telegram() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("test-token"));
        assert_eq!(adapter.name(), "telegram");
        assert_eq!(ADAPTER_NAME, "telegram");
    }

    #[test]
    fn options_new_stores_the_token_and_defaults_base_url() {
        let opts = TelegramAdapterOptions::new("test-token");
        assert_eq!(opts.token, "test-token");
        assert_eq!(opts.effective_base_url(), DEFAULT_BASE_URL);
    }

    #[test]
    fn options_with_base_url_overrides_the_default() {
        let opts = TelegramAdapterOptions::new("t").with_base_url("https://test-proxy.example");
        assert_eq!(opts.effective_base_url(), "https://test-proxy.example");
    }

    #[test]
    fn encode_thread_id_with_no_message_thread_id() {
        assert_eq!(encode_thread_id(123456, None), "telegram:123456");
        // Negative chat ids (Telegram group chats) flow through.
        assert_eq!(encode_thread_id(-100123, None), "telegram:-100123");
    }

    #[test]
    fn encode_thread_id_with_message_thread_id() {
        assert_eq!(encode_thread_id(123, Some(45)), "telegram:123:45");
    }

    #[test]
    fn decode_thread_id_parses_chat_id_only() {
        let decoded = decode_thread_id("telegram:123456").unwrap();
        assert_eq!(decoded.chat_id, 123456);
        assert_eq!(decoded.message_thread_id, None);
    }

    #[test]
    fn decode_thread_id_parses_chat_id_and_message_thread_id() {
        let decoded = decode_thread_id("telegram:123:45").unwrap();
        assert_eq!(decoded.chat_id, 123);
        assert_eq!(decoded.message_thread_id, Some(45));
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("slack:C123:1.0").is_none());
        assert!(decode_thread_id("123:456").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_non_integer_chat_ids() {
        assert!(decode_thread_id("telegram:not-an-int").is_none());
        assert!(decode_thread_id("telegram:abc:45").is_none());
    }

    #[test]
    fn is_telegram_thread_id_detects_the_prefix() {
        assert!(is_telegram_thread_id("telegram:123"));
        assert!(is_telegram_thread_id("telegram:123:45"));
        assert!(!is_telegram_thread_id("slack:C1:1.0"));
        assert!(!is_telegram_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip_preserves_components() {
        for (chat, msg) in [(1i64, None), (-100123, None), (5, Some(10)), (-2, Some(0))] {
            let encoded = encode_thread_id(chat, msg);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.chat_id, chat);
            assert_eq!(decoded.message_thread_id, msg);
        }
    }

    #[test]
    fn adapter_default_methods_return_unsupported() {
        // HTTP-bound methods land in a follow-up slice; for now the
        // default impls on the Adapter trait propagate.
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("telegram:123", "hi"));
        assert!(matches!(
            err,
            Err(AdapterError::Unsupported("post_message"))
        ));
    }

    #[test]
    fn adapter_token_and_base_url_accessors() {
        let adapter = TelegramAdapter::new(
            TelegramAdapterOptions::new("test-token").with_base_url("https://example.test"),
        );
        assert_eq!(adapter.token(), "test-token");
        assert_eq!(adapter.base_url(), "https://example.test");
    }
}
