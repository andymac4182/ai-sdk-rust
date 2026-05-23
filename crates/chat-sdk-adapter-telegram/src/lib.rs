//! Telegram adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-telegram/src/index.ts`.
//!
//! **Surface ported so far:**
//!
//! - [`TelegramAdapter`] + [`TelegramAdapterOptions`] (slice 130:
//!   skeleton + thread-id codec).
//! - `adapter.post_message(thread_id, text)` (slice 145): POST
//!   `<base_url>/bot<token>/sendMessage` -> parse
//!   `{ok, result: {message_id}}`.
//! - `adapter.fetch_subject(thread_id)` (slice 155): POST
//!   `<base_url>/bot<token>/getChat` -> parse
//!   `{ok, result: {title}}`. Reference impl for the per-adapter
//!   subject-fetch port.
//!
//! **What is still deferred:**
//!
//! - `post_object` / `edit_message` / `delete_message` /
//!   `add_reaction` / `start_typing` / other Adapter trait
//!   methods. Each follows the same recipe.
//! - Markdown / card rendering for Telegram's `MarkdownV2` /
//!   inline-keyboard layout.

pub mod cards;

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
/// `class TelegramAdapter implements Adapter`. Holds the bot
/// config + a shared [`reqwest::Client`] from
/// [`chat_sdk_adapter_shared::runtime::default_http_client`].
#[derive(Debug, Clone)]
pub struct TelegramAdapter {
    options: TelegramAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl TelegramAdapter {
    /// 1:1 port of upstream `new TelegramAdapter({ token, baseUrl? })`.
    pub fn new(options: TelegramAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
        }
    }

    /// Override the HTTP client (mostly useful for tests that point
    /// at a wiremock server).
    pub fn with_http_client(
        mut self,
        client: chat_sdk_adapter_shared::runtime::reqwest::Client,
    ) -> Self {
        self.http = client;
        self
    }

    /// Read the bot token (e.g. for constructing HTTPS request URIs).
    pub fn token(&self) -> &str {
        &self.options.token
    }

    /// Effective base URL.
    pub fn base_url(&self) -> &str {
        self.options.effective_base_url()
    }

    /// Build the absolute URL for a Telegram Bot API method. 1:1
    /// with upstream's inline `${baseUrl}/bot${token}/${method}`
    /// template.
    fn method_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.base_url(), self.token(), method)
    }
}

#[async_trait]
impl Adapter for TelegramAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a plain-text message via Telegram's `sendMessage` Bot
    /// API method. 1:1 with upstream's `adapter.postMessage`:
    ///
    /// - Decodes the chat-sdk thread id (`telegram:<chat>[:<thread>]`)
    ///   into `chat_id` + optional `message_thread_id`.
    /// - POSTs JSON `{chat_id, text, message_thread_id?}` to
    ///   `<base_url>/bot<token>/sendMessage`.
    /// - Parses the Telegram envelope `{ok, result: {message_id} }`.
    /// - Returns the integer message id formatted as a decimal string
    ///   (chat-sdk's `Adapter::post_message` -> `String`).
    ///
    /// Returns [`chat_sdk_chat::types::AdapterError::InvalidPayload`]
    /// when the `thread_id` isn't Telegram-encoded. Returns
    /// [`chat_sdk_chat::types::AdapterError::Io`] for network errors,
    /// non-200 HTTP responses, or unexpected JSON shapes.
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Telegram-encoded"))
        })?;

        let url = self.method_url("sendMessage");
        let mut body = serde_json::json!({
            "chat_id": decoded.chat_id,
            "text": text,
        });
        if let Some(message_thread_id) = decoded.message_thread_id {
            body["message_thread_id"] = serde_json::Value::from(message_thread_id);
        }

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

        if !status.is_success() || json["ok"] != serde_json::Value::Bool(true) {
            let description = json["description"]
                .as_str()
                .unwrap_or("Telegram API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {description}"
            )));
        }

        let message_id = json["result"]["message_id"].as_i64().ok_or_else(|| {
            AdapterError::InvalidPayload(
                "Telegram sendMessage response missing result.message_id".to_string(),
            )
        })?;
        Ok(message_id.to_string())
    }

    /// Fetch a Telegram chat's title via the `getChat` Bot API
    /// method. Returns `None` for private (DM) chats that have
    /// no title; returns the chat's `title` for groups +
    /// supergroups + channels.
    async fn fetch_subject(
        &self,
        thread_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<Option<String>> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Telegram-encoded"))
        })?;

        let url = self.method_url("getChat");
        let body = serde_json::json!({ "chat_id": decoded.chat_id });

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

        if !status.is_success() || json["ok"] != serde_json::Value::Bool(true) {
            let description = json["description"]
                .as_str()
                .unwrap_or("Telegram getChat call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {description}"
            )));
        }

        // Private chats have no title; groups/supergroups/channels
        // do. Both are valid outcomes.
        Ok(json["result"]["title"].as_str().map(str::to_owned))
    }

    /// Edit a Telegram message via `editMessageText`. 1:1 with the
    /// text-only path of upstream `adapter.editMessage` (card/inline
    /// keyboard branches deferred). Decodes message_id as either a
    /// composite `<chat_id>:<msg_id>` or a bare `<msg_id>`. Returns
    /// the (unchanged) telegram message id as the chat-sdk id.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Telegram-encoded"))
        })?;
        let telegram_message_id = decode_composite_message_id(message_id, decoded.chat_id)?;

        let url = self.method_url("editMessageText");
        let body = serde_json::json!({
            "chat_id": decoded.chat_id,
            "message_id": telegram_message_id,
            "text": text,
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

        if !status.is_success() || json["ok"] != serde_json::Value::Bool(true) {
            let description = json["description"]
                .as_str()
                .unwrap_or("Telegram editMessageText call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {description}"
            )));
        }

        Ok(format!("{}:{telegram_message_id}", decoded.chat_id))
    }

    /// Delete a Telegram message via `deleteMessage`. 1:1 with
    /// upstream `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Telegram-encoded"))
        })?;
        let telegram_message_id = decode_composite_message_id(message_id, decoded.chat_id)?;

        let url = self.method_url("deleteMessage");
        let body = serde_json::json!({
            "chat_id": decoded.chat_id,
            "message_id": telegram_message_id,
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

        if !status.is_success() || json["ok"] != serde_json::Value::Bool(true) {
            let description = json["description"]
                .as_str()
                .unwrap_or("Telegram deleteMessage call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {description}"
            )));
        }

        Ok(())
    }

    /// Add an emoji reaction via `setMessageReaction`. 1:1 with
    /// upstream `adapter.addReaction`. Wraps the emoji as
    /// `[{type: "emoji", emoji}]`.
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Telegram-encoded"))
        })?;
        let telegram_message_id = decode_composite_message_id(message_id, decoded.chat_id)?;

        let url = self.method_url("setMessageReaction");
        let body = serde_json::json!({
            "chat_id": decoded.chat_id,
            "message_id": telegram_message_id,
            "reaction": [{ "type": "emoji", "emoji": emoji }],
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

        if !status.is_success() || json["ok"] != serde_json::Value::Bool(true) {
            let description = json["description"]
                .as_str()
                .unwrap_or("Telegram setMessageReaction call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {description}"
            )));
        }

        Ok(())
    }

    /// Send a "typing…" chat action via `sendChatAction`. 1:1 with
    /// upstream `adapter.startTyping`. The optional `status`
    /// parameter is ignored (Telegram has no per-action status
    /// text; upstream's signature omits it too).
    async fn start_typing(
        &self,
        thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Telegram-encoded"))
        })?;

        let url = self.method_url("sendChatAction");
        let mut body = serde_json::json!({
            "chat_id": decoded.chat_id,
            "action": "typing",
        });
        if let Some(thread_id) = decoded.message_thread_id {
            body["message_thread_id"] = serde_json::Value::from(thread_id);
        }

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

        if !status.is_success() || json["ok"] != serde_json::Value::Bool(true) {
            let description = json["description"]
                .as_str()
                .unwrap_or("Telegram sendChatAction call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {description}"
            )));
        }

        Ok(())
    }
}

/// Decode a Telegram message id (composite `<chat_id>:<msg_id>` or
/// bare `<msg_id>`). 1:1 port of upstream
/// `decodeCompositeMessageId(messageId, expectedChatId)`. Returns
/// an `AdapterError::InvalidPayload` for malformed input or a
/// chat-id mismatch against the thread.
fn decode_composite_message_id(
    message_id: &str,
    expected_chat_id: i64,
) -> chat_sdk_chat::types::AdapterResult<i64> {
    use chat_sdk_chat::types::AdapterError;
    if let Some((chat_part, msg_part)) = message_id.split_once(':') {
        let chat = chat_part.parse::<i64>().map_err(|_| {
            AdapterError::InvalidPayload(format!(
                "Telegram composite message id {message_id:?}: chat id is not numeric"
            ))
        })?;
        if chat != expected_chat_id {
            return Err(AdapterError::InvalidPayload(format!(
                "Telegram composite message id {message_id:?}: chat id {chat} does not match thread chat id {expected_chat_id}"
            )));
        }
        msg_part.parse::<i64>().map_err(|_| {
            AdapterError::InvalidPayload(format!(
                "Telegram composite message id {message_id:?}: message id is not numeric"
            ))
        })
    } else {
        message_id.parse::<i64>().map_err(|_| {
            AdapterError::InvalidPayload(format!(
                "Telegram message id {message_id:?} must be numeric or <chat_id>:<msg_id>"
            ))
        })
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
    fn adapter_post_message_rejects_non_telegram_thread_ids() {
        // Slice 145 wired post_message to the HTTP layer; the
        // pre-HTTP validation rejects mismatched thread ids before
        // any network call. This test exercises that path without
        // needing a tokio runtime.
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Telegram-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_fetch_subject_rejects_non_telegram_thread_ids() {
        // Slice 155 wired fetch_subject to the HTTP layer; the
        // pre-HTTP validation rejects non-Telegram ids before
        // any network call.
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.fetch_subject("slack:C1:1.0"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Telegram-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_telegram_thread_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("slack:C1:1.0", "42", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Telegram-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_rejects_non_telegram_thread_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("slack:C1:1.0", "42"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Telegram-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_telegram_thread_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("slack:C1:1.0", "42", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Telegram-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_rejects_non_telegram_thread_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.start_typing("slack:C1:1.0", None));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Telegram-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn decode_composite_message_id_parses_composite_and_bare() {
        // Composite form
        assert_eq!(decode_composite_message_id("123:42", 123).unwrap(), 42);
        // Bare numeric form (chat from thread)
        assert_eq!(decode_composite_message_id("42", 123).unwrap(), 42);
    }

    #[test]
    fn decode_composite_message_id_rejects_chat_id_mismatch() {
        use chat_sdk_chat::types::AdapterError;
        match decode_composite_message_id("999:42", 123) {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not match thread chat id"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn decode_composite_message_id_rejects_non_numeric() {
        use chat_sdk_chat::types::AdapterError;
        assert!(matches!(
            decode_composite_message_id("abc", 1),
            Err(AdapterError::InvalidPayload(_))
        ));
        assert!(matches!(
            decode_composite_message_id("1:abc", 1),
            Err(AdapterError::InvalidPayload(_))
        ));
    }

    #[test]
    fn adapter_method_url_combines_base_token_and_method() {
        let adapter = TelegramAdapter::new(
            TelegramAdapterOptions::new("BOTTOK").with_base_url("https://example.test"),
        );
        assert_eq!(
            adapter.method_url("sendMessage"),
            "https://example.test/botBOTTOK/sendMessage"
        );
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
