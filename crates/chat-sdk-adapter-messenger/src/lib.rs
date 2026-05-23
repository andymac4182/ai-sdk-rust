//! Facebook Messenger adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-messenger/src/index.ts`.
//!
//! Messenger maps each user's DM to one chat-sdk thread. The thread
//! id encoding is `messenger:<recipient_id>` (single colon, matching
//! upstream's `encodeThreadId({recipientId})` -> `messenger:<id>`).
//! The page id is implicit in the page access token (Meta's
//! `/me/messages` endpoint).
//!
//! Slice 173 corrected the wire format: earlier slices used the
//! non-upstream `messenger:<page_id>:<user_id>` shape and `/v22.0/
//! <page_id>/messages`. The corrected port matches upstream's
//! `messenger:<recipient_id>` + `/v22.0/me/messages` and rejects
//! multi-colon thread ids in `decode_thread_id` (1:1 with upstream
//! `decodeThreadId` which throws `ValidationError` on multi-colon).

pub mod cards;

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
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl MessengerAdapter {
    /// 1:1 port of upstream
    /// `new MessengerAdapter({ pageAccessToken, verifyToken, graphBase? })`.
    pub fn new(options: MessengerAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
        }
    }

    /// Override the HTTP client (mostly useful for tests).
    pub fn with_http_client(
        mut self,
        client: chat_sdk_adapter_shared::runtime::reqwest::Client,
    ) -> Self {
        self.http = client;
        self
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

    /// Graph API version used in URLs. 1:1 with upstream's
    /// `apiVersion = "v22.0"` default.
    pub const GRAPH_API_VERSION: &'static str = "v22.0";

    /// Build the Send API URL. 1:1 with upstream's call to
    /// `graphApiFetch("me/messages", "POST", body)` which composes
    /// `<graph_base>/<api_version>/me/messages?access_token=<token>`.
    /// The page id is implicit in the access token (Meta routes by
    /// the token rather than a URL-path page id).
    fn send_url(&self) -> String {
        format!(
            "{}/{}/me/messages",
            self.graph_base(),
            Self::GRAPH_API_VERSION
        )
    }
}

#[async_trait]
impl Adapter for MessengerAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a text message via the Messenger Send API. 1:1 with
    /// upstream's `adapter.postMessage`:
    ///
    /// - Decodes `messenger:<recipient_id>`.
    /// - POSTs JSON `{recipient: {id: recipient_id}, message: {text}}` to
    ///   `<graph_base>/v22.0/me/messages?access_token=<page_token>`.
    /// - Returns the Send API's `message_id` as the chat-sdk
    ///   message id.
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!(
                "thread_id {thread_id:?} is not Messenger-encoded"
            ))
        })?;

        // Meta passes the access token as a URL query param rather
        // than an Authorization header.
        let url = format!(
            "{}?access_token={}",
            self.send_url(),
            self.page_access_token()
        );
        let body = serde_json::json!({
            "recipient": { "id": decoded.recipient_id },
            "message": { "text": text },
        });

        let response = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        if !status.is_success() {
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Messenger Send API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        json["message_id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(
                    "Messenger Send API response missing message_id".to_string(),
                )
            })
    }

    /// Messenger does not support message editing. 1:1 with
    /// upstream's `adapter.editMessage` which throws
    /// `ValidationError("messenger", "Messenger does not support
    /// editing messages")`.
    async fn edit_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "Messenger does not support editing messages".to_string(),
        ))
    }

    /// Messenger does not support message deletion. 1:1 with
    /// upstream's `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "Messenger does not support deleting messages".to_string(),
        ))
    }

    /// Messenger does not expose reactions via the API. 1:1 with
    /// upstream's `adapter.addReaction`.
    async fn add_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "Messenger does not support reactions via API".to_string(),
        ))
    }

    /// Send a Messenger typing indicator via the Send API
    /// `sender_action: typing_on`. 1:1 with upstream's
    /// `adapter.startTyping` (status arg ignored — upstream's
    /// signature omits it).
    async fn start_typing(
        &self,
        thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!(
                "thread_id {thread_id:?} is not Messenger-encoded"
            ))
        })?;

        let url = format!(
            "{}?access_token={}",
            self.send_url(),
            self.page_access_token()
        );
        let body = serde_json::json!({
            "recipient": { "id": decoded.recipient_id },
            "sender_action": "typing_on",
        });

        let response = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        if !status.is_success() {
            let json: serde_json::Value = response.json().await.unwrap_or_default();
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Messenger Send API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        Ok(())
    }
}

/// Encode a Messenger thread id. 1:1 with upstream's
/// `encodeThreadId({recipientId}) -> "messenger:<recipientId>"`
/// (single colon).
pub fn encode_thread_id(recipient_id: &str) -> String {
    format!("{THREAD_ID_PREFIX}{recipient_id}")
}

/// Components of a decoded Messenger thread id. 1:1 with upstream's
/// returned object shape `{recipientId}` from
/// `decodeThreadId(threadId)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedMessengerThreadId {
    /// Recipient (PSID - page-scoped user id) the thread points
    /// at. Upstream calls this `recipientId`.
    pub recipient_id: String,
}

/// Decode a Messenger thread id. 1:1 port of upstream
/// `decodeThreadId(threadId)`. Returns `None` for any value that
/// doesn't carry the `messenger:` prefix, has an empty recipient,
/// or contains an extra colon. Upstream throws `ValidationError`
/// in all those cases; the Rust port returns `None` and lets the
/// caller (`post_message` etc.) surface it as
/// `AdapterError::InvalidPayload`.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedMessengerThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    if suffix.is_empty() {
        return None;
    }
    if suffix.contains(':') {
        // Upstream's decodeThreadId throws on extra colons:
        // `messenger:foo:bar` -> ValidationError.
        return None;
    }
    Some(DecodedMessengerThreadId {
        recipient_id: suffix.to_string(),
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

    // ---------- 4 cases ported from upstream
    // `packages/adapter-messenger/src/index.test.ts` "thread ID
    // encoding" describe block ----------

    #[test]
    fn encodes_and_decodes_thread_ids() {
        // Upstream:
        //   adapter.encodeThreadId({recipientId: "USER_123"})
        //     === "messenger:USER_123"
        //   adapter.decodeThreadId("messenger:USER_123")
        //     === { recipientId: "USER_123" }
        assert_eq!(encode_thread_id("USER_123"), "messenger:USER_123");
        let decoded = decode_thread_id("messenger:USER_123").unwrap();
        assert_eq!(decoded.recipient_id, "USER_123");
    }

    #[test]
    fn throws_on_invalid_thread_ids() {
        // Upstream `decodeThreadId` throws ValidationError for
        // "invalid", "messenger:", and "slack:C123:ts". Rust port
        // returns None (callers convert to InvalidPayload).
        assert!(decode_thread_id("invalid").is_none());
        assert!(decode_thread_id("messenger:").is_none());
        assert!(decode_thread_id("slack:C123:ts").is_none());
    }

    #[test]
    fn rejects_thread_id_with_extra_colons() {
        // Upstream: decodeThreadId("messenger:foo:bar") throws.
        assert!(decode_thread_id("messenger:foo:bar").is_none());
    }

    #[test]
    fn rejects_empty_thread_id() {
        // Upstream: decodeThreadId("") throws.
        assert!(decode_thread_id("").is_none());
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn is_messenger_thread_id_detects_the_prefix() {
        assert!(is_messenger_thread_id("messenger:USER"));
        assert!(!is_messenger_thread_id("telegram:1"));
        assert!(!is_messenger_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for r in ["USER", "1", "with-dashes", "psid_123_abc"] {
            let encoded = encode_thread_id(r);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.recipient_id, r);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_messenger_thread_ids() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Messenger-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_is_unsupported_with_validation_error() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("messenger:USER", "msg", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support editing"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_is_unsupported_with_validation_error() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("messenger:USER", "msg"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support deleting"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_is_unsupported_with_validation_error() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("messenger:USER", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support reactions"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_rejects_non_messenger_thread_ids() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.start_typing("slack:C1:1.0", None));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Messenger-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_send_url_builds_the_upstream_endpoint() {
        // 1:1 with upstream graphApiFetch("me/messages", ...):
        // <graph_base>/<api_version>/me/messages.
        let adapter = MessengerAdapter::new(
            MessengerAdapterOptions::new("p", "v").with_graph_base("https://graph.example.test"),
        );
        assert_eq!(
            adapter.send_url(),
            "https://graph.example.test/v22.0/me/messages"
        );
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
