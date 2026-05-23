//! Facebook Messenger adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-messenger/src/index.ts`.
//!
//! Messenger maps each (page, user) DM pair to one chat-sdk thread.
//! The thread id encoding is `messenger:<page_id>:<user_id>`.
//!
//! **What this slice ships (slice 132):**
//!
//! - Crate skeleton + `Cargo.toml`.
//! - [`MessengerAdapter`] struct holding page config (page access
//!   token + verify token + optional Graph API base URL) impl-ing
//!   the chat-sdk [`chat_sdk_chat::types::Adapter`] trait with
//!   `name = "messenger"`.
//! - [`MessengerAdapterOptions`] config struct.
//! - [`encode_thread_id`] / [`decode_thread_id`] /
//!   [`is_messenger_thread_id`] — pure helpers for the upstream
//!   `messenger:<page_id>:<user_id>` wire format.
//!
//! **What is deferred:**
//!
//! - HTTP I/O against `graph.facebook.com` for the Send API,
//!   webhook signature verification, persistent menu / quick
//!   reply rendering.

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator. 1:1 with upstream
/// `export const ADAPTER_NAME = "messenger"`.
pub const ADAPTER_NAME: &str = "messenger";

/// Thread-id prefix. 1:1 with upstream's inline `messenger:` namespace.
pub const THREAD_ID_PREFIX: &str = "messenger:";

/// Default Facebook Graph API base URL. 1:1 with upstream
/// `const DEFAULT_GRAPH_BASE = "https://graph.facebook.com"`.
pub const DEFAULT_GRAPH_BASE: &str = "https://graph.facebook.com";

/// Options for [`MessengerAdapter::new`]. 1:1 with upstream
/// `interface MessengerAdapterOptions`.
#[derive(Debug, Clone)]
pub struct MessengerAdapterOptions {
    /// Page access token (Meta business token).
    pub page_access_token: String,
    /// Webhook verify token. Used by Meta to confirm webhook
    /// ownership during setup.
    pub verify_token: String,
    /// Optional Graph API base URL override (defaults to
    /// [`DEFAULT_GRAPH_BASE`]).
    pub graph_base: Option<String>,
}

impl MessengerAdapterOptions {
    /// Construct options. Graph base URL defaults to
    /// [`DEFAULT_GRAPH_BASE`].
    pub fn new(page_access_token: impl Into<String>, verify_token: impl Into<String>) -> Self {
        Self {
            page_access_token: page_access_token.into(),
            verify_token: verify_token.into(),
            graph_base: None,
        }
    }

    /// Override the Graph API base URL.
    pub fn with_graph_base(mut self, graph_base: impl Into<String>) -> Self {
        self.graph_base = Some(graph_base.into());
        self
    }

    /// Effective Graph API base URL with default applied.
    pub fn effective_graph_base(&self) -> &str {
        self.graph_base.as_deref().unwrap_or(DEFAULT_GRAPH_BASE)
    }
}

/// Facebook Messenger adapter. 1:1 port (in progress) of upstream
/// `class MessengerAdapter implements Adapter`.
#[derive(Debug, Clone)]
pub struct MessengerAdapter {
    options: MessengerAdapterOptions,
}

impl MessengerAdapter {
    /// 1:1 port of upstream
    /// `new MessengerAdapter({ pageAccessToken, verifyToken, graphBase? })`.
    pub fn new(options: MessengerAdapterOptions) -> Self {
        Self { options }
    }

    /// Read the page access token.
    pub fn page_access_token(&self) -> &str {
        &self.options.page_access_token
    }

    /// Read the webhook verify token.
    pub fn verify_token(&self) -> &str {
        &self.options.verify_token
    }

    /// Effective Graph API base URL.
    pub fn graph_base(&self) -> &str {
        self.options.effective_graph_base()
    }
}

#[async_trait]
impl Adapter for MessengerAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }
}

/// Encode a Messenger thread id. 1:1 with upstream's inline format:
/// `messenger:<page_id>:<user_id>`.
pub fn encode_thread_id(page_id: &str, user_id: &str) -> String {
    format!("{THREAD_ID_PREFIX}{page_id}:{user_id}")
}

/// Components of a decoded Messenger thread id. 1:1 with upstream's
/// returned object shape from `decodeThreadId(threadId)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedMessengerThreadId {
    /// Page identifier.
    pub page_id: String,
    /// User identifier (PSID — page-scoped user id).
    pub user_id: String,
}

/// Decode a Messenger thread id. 1:1 port of upstream
/// `decodeThreadId(threadId)`. Returns `None` for any value that
/// doesn't carry the `messenger:` prefix or lacks both
/// `page_id` and `user_id` separated by a colon.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedMessengerThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let page_id = parts.next()?;
    let user_id = parts.next()?;
    if page_id.is_empty() || user_id.is_empty() {
        return None;
    }
    Some(DecodedMessengerThreadId {
        page_id: page_id.to_string(),
        user_id: user_id.to_string(),
    })
}

/// Predicate: does this thread id belong to the Messenger adapter?
/// 1:1 with upstream's inline `threadId.startsWith("messenger:")`.
pub fn is_messenger_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_messenger() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("page-token", "verify"));
        assert_eq!(adapter.name(), "messenger");
        assert_eq!(ADAPTER_NAME, "messenger");
    }

    #[test]
    fn options_new_stores_tokens_and_defaults_graph_base() {
        let opts = MessengerAdapterOptions::new("page-token", "verify-token");
        assert_eq!(opts.page_access_token, "page-token");
        assert_eq!(opts.verify_token, "verify-token");
        assert_eq!(opts.effective_graph_base(), DEFAULT_GRAPH_BASE);
    }

    #[test]
    fn options_with_graph_base_overrides_the_default() {
        let opts = MessengerAdapterOptions::new("p", "v")
            .with_graph_base("https://graph.example.test/v20.0");
        assert_eq!(
            opts.effective_graph_base(),
            "https://graph.example.test/v20.0"
        );
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(
            encode_thread_id("PAGE123", "USER456"),
            "messenger:PAGE123:USER456"
        );
    }

    #[test]
    fn decode_thread_id_parses_page_and_user() {
        let decoded = decode_thread_id("messenger:PAGE123:USER456").unwrap();
        assert_eq!(decoded.page_id, "PAGE123");
        assert_eq!(decoded.user_id, "USER456");
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("slack:C1:1.0").is_none());
        assert!(decode_thread_id("whatsapp:1:2").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_page_or_user() {
        assert!(decode_thread_id("messenger:onlyone").is_none());
        assert!(decode_thread_id("messenger::USER").is_none());
        assert!(decode_thread_id("messenger:PAGE:").is_none());
    }

    #[test]
    fn is_messenger_thread_id_detects_the_prefix() {
        assert!(is_messenger_thread_id("messenger:PAGE:USER"));
        assert!(!is_messenger_thread_id("telegram:1"));
        assert!(!is_messenger_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (p, u) in [("PAGE", "USER"), ("1", "2"), ("with-dashes", "with.dots")] {
            let encoded = encode_thread_id(p, u);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.page_id, p);
            assert_eq!(decoded.user_id, u);
        }
    }

    #[test]
    fn adapter_default_methods_return_unsupported() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("messenger:PAGE:USER", "hi"));
        assert!(matches!(
            err,
            Err(AdapterError::Unsupported("post_message"))
        ));
    }

    #[test]
    fn adapter_token_accessors() {
        let adapter = MessengerAdapter::new(
            MessengerAdapterOptions::new("page-tok", "verify-tok")
                .with_graph_base("https://example.test"),
        );
        assert_eq!(adapter.page_access_token(), "page-tok");
        assert_eq!(adapter.verify_token(), "verify-tok");
        assert_eq!(adapter.graph_base(), "https://example.test");
    }
}
