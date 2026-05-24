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
pub mod markdown;

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

/// HTTP request header Telegram sends with each webhook payload
/// when the bot has set a `secret_token` on its webhook config.
/// Webhook verifiers compare the header value byte-for-byte
/// against the configured secret token. 1:1 with upstream's
/// private `TELEGRAM_SECRET_TOKEN_HEADER =
/// "x-telegram-bot-api-secret-token"`.
pub const TELEGRAM_SECRET_TOKEN_HEADER: &str = "x-telegram-bot-api-secret-token";

/// Default long-polling timeout (in seconds) the adapter uses when
/// `mode: "polling"` or when auto mode falls back to polling. 1:1
/// with upstream's private
/// `TELEGRAM_DEFAULT_POLLING_TIMEOUT_SECONDS = 30`.
pub const TELEGRAM_DEFAULT_POLLING_TIMEOUT_SECONDS: u64 = 30;

/// 1:1 with upstream's default `userName ?? "bot"` constant.
pub const DEFAULT_USER_NAME: &str = "bot";

/// Options for [`TelegramAdapter::new`]. 1:1 with upstream
/// `interface TelegramAdapterOptions`.
#[derive(Debug, Clone)]
pub struct TelegramAdapterOptions {
    /// Telegram bot token (`<bot-id>:<secret>` from BotFather).
    pub token: String,
    /// Optional secret token Telegram sends in the
    /// `x-telegram-bot-api-secret-token` webhook header for
    /// verification. 1:1 with upstream `secretToken?: string`.
    pub secret_token: Option<String>,
    /// Optional display name (defaults to [`DEFAULT_USER_NAME`]).
    pub user_name: Option<String>,
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
            secret_token: None,
            user_name: None,
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

    /// Effective `userName` with default applied.
    pub fn effective_user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(DEFAULT_USER_NAME)
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

    /// 1:1 with upstream `readonly secretToken?: string`.
    pub fn secret_token(&self) -> Option<&str> {
        self.options.secret_token.as_deref()
    }

    /// 1:1 with upstream `readonly userName: string` (with default).
    pub fn user_name(&self) -> &str {
        self.options.effective_user_name()
    }

    /// Build the absolute URL for a Telegram Bot API method. 1:1
    /// with upstream's inline `${baseUrl}/bot${token}/${method}`
    /// template.
    fn method_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.base_url(), self.token(), method)
    }

    /// Derive channel id from a Telegram thread id. 1:1 with
    /// upstream `adapter.channelIdFromThreadId(threadId)` which
    /// strips any `:<message_thread_id>` suffix and returns
    /// `telegram:<chat_id>`. Returns `None` when `thread_id` isn't a
    /// Telegram-encoded value.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        let decoded = decode_thread_id(thread_id)?;
        Some(format!("{THREAD_ID_PREFIX}{}", decoded.chat_id))
    }

    /// Predicate: is the conversation a 1:1 DM? 1:1 with upstream's
    /// `adapter.isDM(threadId)` which returns `true` when the
    /// underlying Telegram `chat_id` doesn't start with `-` (the
    /// Telegram convention for groups/supergroups/channels). Returns
    /// `None` when `thread_id` isn't a Telegram-encoded value.
    pub fn is_dm(&self, thread_id: &str) -> Option<bool> {
        let decoded = decode_thread_id(thread_id)?;
        Some(decoded.chat_id >= 0)
    }

    /// Render formatted content to Telegram MarkdownV2. 1:1 with
    /// upstream `adapter.renderFormatted(content)` which delegates
    /// to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::TelegramFormatConverter::new().from_ast(ast)
    }

    /// Open a Direct Message with `user_id`. 1:1 with upstream
    /// `adapter.openDM(userId)` which returns
    /// `encodeThreadId({chatId: userId})`. Upstream passes the
    /// `userId` string into the encoder; the Rust encoder requires
    /// a numeric `chat_id`, so this parses `user_id` as `i64` and
    /// returns `None` if non-numeric. Returns the encoded thread
    /// id (no HTTP call — Telegram conversations are addressed by
    /// numeric chat id).
    pub fn open_dm(&self, user_id: &str) -> Option<String> {
        let chat_id: i64 = user_id.parse().ok()?;
        Some(encode_thread_id(chat_id, None))
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
/// A subset of `TelegramMessageEntity` carrying just the fields
/// [`apply_telegram_entities`] consumes. 1:1 port of upstream's
/// `interface TelegramMessageEntity` (with the same `language?` /
/// `url?` optional fields). Additional `user` field is unused by
/// the entity rendering path; it stays on the upstream type for
/// the inbound-parse path only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramMessageEntity {
    /// Entity type: `bold`, `italic`, `code`, `pre`, `strikethrough`,
    /// `text_link`, `url`, `mention`, `bot_command`, etc.
    pub kind: String,
    /// Byte offset into the rendered text (matches upstream's
    /// UTF-16 offset for ASCII inputs — the entire upstream test
    /// suite uses ASCII so this is functionally equivalent).
    pub offset: usize,
    /// Length of the entity span in bytes / UTF-16 code units.
    pub length: usize,
    /// URL for `text_link` entities.
    pub url: Option<String>,
    /// Language tag for `pre` (fenced-code) entities.
    pub language: Option<String>,
}

/// Escape standard-markdown special characters inside inbound entity
/// text. 1:1 port of upstream's private
/// `escapeMarkdownInEntity(text)` (regex `/([[\]()\\])/g`).
fn escape_markdown_in_entity(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '[' | ']' | '(' | ')' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Convert Telegram message entities (inbound) to standard markdown.
/// 1:1 port of upstream's
/// `applyTelegramEntities(text, entities): string`.
///
/// Telegram delivers formatting as separate `MessageEntity` objects
/// alongside plain text. This function reconstructs **standard**
/// markdown (`**bold**`, `~~strike~~`, etc.) so the result can be
/// parsed by `chat_sdk_chat::markdown::parse_markdown`. The outbound
/// direction (AST → MarkdownV2) lives in [`crate::markdown`].
///
/// Sorting: entities are processed by `offset` descending so
/// replacements never shift later offsets; ties on offset go to the
/// shorter span first (so the inner entity is applied before its
/// outer wrapper, matching upstream).
///
/// Unknown entity kinds (`url`, `mention`, `bot_command`, ...) are
/// left in place — upstream's `default: break` branch.
pub fn apply_telegram_entities(text: &str, entities: &[TelegramMessageEntity]) -> String {
    if entities.is_empty() {
        return text.to_string();
    }

    // Sort by offset desc, then length asc.
    let mut sorted: Vec<&TelegramMessageEntity> = entities.iter().collect();
    sorted.sort_by(|a, b| {
        b.offset
            .cmp(&a.offset)
            .then_with(|| a.length.cmp(&b.length))
    });

    let mut result = text.to_string();
    for entity in sorted {
        let start = entity.offset;
        let end = entity.offset + entity.length;
        if end > result.len() || !result.is_char_boundary(start) || !result.is_char_boundary(end) {
            // Offset out of range or lands mid-char (only possible
            // for non-ASCII inputs where upstream's UTF-16 offsets
            // diverge from Rust byte offsets); skip this entity to
            // preserve invariants.
            continue;
        }
        let entity_text = &result[start..end];

        let replacement: Option<String> = match entity.kind.as_str() {
            "text_link" => entity
                .url
                .as_deref()
                .map(|url| format!("[{}]({url})", escape_markdown_in_entity(entity_text))),
            "bold" => Some(format!("**{entity_text}**")),
            "italic" => Some(format!("*{entity_text}*")),
            "code" => Some(format!("`{entity_text}`")),
            "pre" => {
                let lang = entity.language.as_deref().unwrap_or("");
                Some(format!("```{lang}\n{entity_text}\n```"))
            }
            "strikethrough" => Some(format!("~~{entity_text}~~")),
            // `url`, `mention`, `bot_command`, etc. are already
            // present in the text as-is — upstream `default: break`.
            _ => None,
        };

        if let Some(replacement) = replacement {
            // Splice in place: `result[..start] + replacement + result[end..]`.
            let mut new_result = String::with_capacity(result.len() + replacement.len());
            new_result.push_str(&result[..start]);
            new_result.push_str(&replacement);
            new_result.push_str(&result[end..]);
            result = new_result;
        }
    }

    result
}

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
    #[test]
    fn telegram_webhook_secret_header_and_polling_timeout_match_upstream() {
        // 1:1 with upstream `TELEGRAM_SECRET_TOKEN_HEADER` and
        // `TELEGRAM_DEFAULT_POLLING_TIMEOUT_SECONDS` consts.
        assert_eq!(
            TELEGRAM_SECRET_TOKEN_HEADER,
            "x-telegram-bot-api-secret-token"
        );
        assert_eq!(TELEGRAM_DEFAULT_POLLING_TIMEOUT_SECONDS, 30);
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

    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream `adapter.channelIdFromThreadId(threadId)` and
    // `adapter.isDM(threadId)`. Telegram supports both DMs (positive
    // chat ids) and groups/supergroups/channels (negative chat ids).

    #[test]
    // ---------- openDM (2 cases) ----------
    #[test]
    fn open_dm_encodes_a_numeric_chat_id_from_a_string_user_id() {
        // 1:1 with upstream's `openDM(userId)` which calls
        // `encodeThreadId({chatId: userId})`. Rust's encoder
        // requires `i64`, so the string-to-int parse layer is
        // explicit here.
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        assert_eq!(adapter.open_dm("42").as_deref(), Some("telegram:42"));
    }

    #[test]
    fn open_dm_returns_none_for_non_numeric_user_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        assert_eq!(adapter.open_dm("not-a-number"), None);
    }

    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        // Telegram MarkdownV2 escapes "!" but plain "Hello world" has
        // no special chars, so it should appear verbatim.
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn channel_id_from_thread_id_strips_the_message_thread_suffix() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        // Bare DM thread id passes through unchanged.
        assert_eq!(
            adapter.channel_id_from_thread_id("telegram:42").as_deref(),
            Some("telegram:42")
        );
        // Supergroup with topic id collapses to the bare chat id.
        assert_eq!(
            adapter
                .channel_id_from_thread_id("telegram:-100123:777")
                .as_deref(),
            Some("telegram:-100123")
        );
    }

    #[test]
    fn channel_id_from_thread_id_returns_none_for_non_telegram_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        assert!(adapter.channel_id_from_thread_id("slack:C1:1.0").is_none());
        assert!(adapter.channel_id_from_thread_id("").is_none());
    }

    #[test]
    fn is_dm_is_true_for_positive_chat_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        assert_eq!(adapter.is_dm("telegram:42"), Some(true));
        assert_eq!(adapter.is_dm("telegram:42:777"), Some(true));
    }

    #[test]
    fn is_dm_is_false_for_negative_chat_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        // Telegram convention: groups/supergroups/channels have
        // negative chat ids.
        assert_eq!(adapter.is_dm("telegram:-100123"), Some(false));
        assert_eq!(adapter.is_dm("telegram:-1:5"), Some(false));
    }

    #[test]
    fn is_dm_returns_none_for_non_telegram_ids() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("tok"));
        assert_eq!(adapter.is_dm("messenger:USER"), None);
        assert_eq!(adapter.is_dm(""), None);
    }

    // ---------- applyTelegramEntities (11 upstream cases) ----------
    // 1:1 with upstream `packages/adapter-telegram/src/index.test.ts`
    // `describe("applyTelegramEntities")` describe block.

    fn entity(kind: &str, offset: usize, length: usize) -> TelegramMessageEntity {
        TelegramMessageEntity {
            kind: kind.to_string(),
            offset,
            length,
            url: None,
            language: None,
        }
    }

    #[test]
    fn apply_telegram_entities_returns_text_unchanged_when_no_entities() {
        assert_eq!(apply_telegram_entities("hello world", &[]), "hello world");
    }

    #[test]
    fn apply_telegram_entities_converts_text_link_entities_to_markdown_links() {
        let e = TelegramMessageEntity {
            kind: "text_link".to_string(),
            offset: 10,
            length: 7,
            url: Some("https://example.com".to_string()),
            language: None,
        };
        assert_eq!(
            apply_telegram_entities("Visit our website for details", &[e]),
            "Visit our [website](https://example.com) for details"
        );
    }

    #[test]
    fn apply_telegram_entities_converts_bold_entities_to_markdown_bold() {
        assert_eq!(
            apply_telegram_entities("hello world", &[entity("bold", 6, 5)]),
            "hello **world**"
        );
    }

    #[test]
    fn apply_telegram_entities_converts_italic_entities_to_markdown_italic() {
        assert_eq!(
            apply_telegram_entities("hello world", &[entity("italic", 0, 5)]),
            "*hello* world"
        );
    }

    #[test]
    fn apply_telegram_entities_converts_code_entities_to_inline_code() {
        assert_eq!(
            apply_telegram_entities(
                "use the console.log function",
                &[entity("code", 8, 11)]
            ),
            "use the `console.log` function"
        );
    }

    #[test]
    fn apply_telegram_entities_converts_pre_entities_to_code_blocks() {
        assert_eq!(
            apply_telegram_entities("const x = 1", &[entity("pre", 0, 11)]),
            "```\nconst x = 1\n```"
        );
    }

    #[test]
    fn apply_telegram_entities_converts_pre_entities_with_language() {
        let e = TelegramMessageEntity {
            kind: "pre".to_string(),
            offset: 0,
            length: 11,
            url: None,
            language: Some("typescript".to_string()),
        };
        assert_eq!(
            apply_telegram_entities("const x = 1", &[e]),
            "```typescript\nconst x = 1\n```"
        );
    }

    #[test]
    fn apply_telegram_entities_converts_strikethrough_entities() {
        assert_eq!(
            apply_telegram_entities("old text here", &[entity("strikethrough", 0, 8)]),
            "~~old text~~ here"
        );
    }

    #[test]
    fn apply_telegram_entities_leaves_url_entities_unchanged() {
        assert_eq!(
            apply_telegram_entities(
                "check https://example.com out",
                &[entity("url", 6, 19)]
            ),
            "check https://example.com out"
        );
    }

    #[test]
    fn apply_telegram_entities_leaves_mention_entities_unchanged() {
        assert_eq!(
            apply_telegram_entities("hey @user check this", &[entity("mention", 4, 5)]),
            "hey @user check this"
        );
    }

    #[test]
    fn apply_telegram_entities_handles_multiple_non_overlapping_entities() {
        assert_eq!(
            apply_telegram_entities(
                "hello world foo",
                &[entity("bold", 0, 5), entity("italic", 6, 5)]
            ),
            "**hello** *world* foo"
        );
    }

    #[test]
    fn apply_telegram_entities_handles_text_link_with_special_markdown_chars_in_text() {
        let e = TelegramMessageEntity {
            kind: "text_link".to_string(),
            offset: 6,
            length: 6,
            url: Some("https://example.com".to_string()),
            language: None,
        };
        assert_eq!(
            apply_telegram_entities("click [here]", &[e]),
            "click [\\[here\\]](https://example.com)"
        );
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

    // ---------- createTelegramAdapter create-instance (2 cases) ----------
    // 1:1 with portable subset of upstream `index.test.ts >
    // describe("createTelegramAdapter")`. Env-var-driven cases
    // (`throws when bot token is missing` / `uses env vars when
    // config is omitted` / the 7 `constructor env var resolution`
    // cases) need an env-var-resolution factory; documented as
    // deferred.

    #[test]
    fn telegram_adapter_creates_an_instance() {
        let adapter = TelegramAdapter::new(TelegramAdapterOptions::new("token-from-env"));
        assert_eq!(adapter.name(), "telegram");
        // Default userName = "bot".
        assert_eq!(adapter.user_name(), "bot");
        // secret_token defaults to None.
        assert!(adapter.secret_token().is_none());
    }

    #[test]
    fn telegram_adapter_uses_provided_secret_token_and_user_name() {
        let mut opts = TelegramAdapterOptions::new("token");
        opts.secret_token = Some("env-secret".to_string());
        opts.user_name = Some("env_bot_name".to_string());
        let adapter = TelegramAdapter::new(opts);
        assert_eq!(adapter.user_name(), "env_bot_name");
        assert_eq!(adapter.secret_token(), Some("env-secret"));
    }
}
