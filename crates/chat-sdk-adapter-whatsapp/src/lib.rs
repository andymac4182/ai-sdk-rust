//! WhatsApp Business Cloud API adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-whatsapp/src/index.ts`.
//!
//! WhatsApp maps each (business phone number, customer phone number)
//! DM pair to one chat-sdk thread. The thread id encoding is
//! `whatsapp:<phone_number_id>:<customer_phone>`.

pub mod cards;
pub mod markdown;
pub mod webhook;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "whatsapp";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "whatsapp:";

/// Default WhatsApp Cloud API base URL (the Meta Graph endpoint).
pub const DEFAULT_GRAPH_BASE: &str = "https://graph.facebook.com";

/// 1:1 with upstream's default `userName ?? "bot"` constant
/// (adapter-constructor fallback).
pub const DEFAULT_USER_NAME: &str = "bot";

/// 1:1 with upstream's factory-level default
/// `userName ?? process.env.WHATSAPP_BOT_USERNAME ?? "whatsapp-bot"`.
/// Applied by [`try_create_whatsapp_adapter`] when neither config
/// nor env supplies a name; supersedes [`DEFAULT_USER_NAME`].
pub const DEFAULT_FACTORY_USER_NAME: &str = "whatsapp-bot";

/// Options for [`WhatsappAdapter::new`].
#[derive(Debug, Clone)]
pub struct WhatsappAdapterOptions {
    /// Business phone-number ID (Meta-issued identifier).
    pub phone_number_id: String,
    /// Permanent access token (Meta business token).
    pub access_token: String,
    /// Webhook verify token.
    pub verify_token: String,
    /// Optional Facebook app secret used by
    /// [`crate::webhook::verify_whatsapp_signature`].
    pub app_secret: Option<String>,
    /// Optional display name (defaults to [`DEFAULT_USER_NAME`]).
    pub user_name: Option<String>,
    /// Optional Graph API base URL override.
    pub graph_base: Option<String>,
    /// Optional Graph API version override. Defaults to
    /// [`DEFAULT_API_VERSION`].
    pub api_version: Option<String>,
}

impl WhatsappAdapterOptions {
    /// Construct options. Graph base URL defaults to
    /// [`DEFAULT_GRAPH_BASE`].
    pub fn new(
        phone_number_id: impl Into<String>,
        access_token: impl Into<String>,
        verify_token: impl Into<String>,
    ) -> Self {
        Self {
            phone_number_id: phone_number_id.into(),
            access_token: access_token.into(),
            verify_token: verify_token.into(),
            app_secret: None,
            user_name: None,
            graph_base: None,
            api_version: None,
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

    /// Effective Graph API version with default applied.
    pub fn effective_api_version(&self) -> &str {
        self.api_version.as_deref().unwrap_or(DEFAULT_API_VERSION)
    }

    /// Effective `userName` with default applied.
    pub fn effective_user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(DEFAULT_USER_NAME)
    }
}

/// WhatsApp Cloud API adapter.
#[derive(Debug, Clone)]
pub struct WhatsappAdapter {
    options: WhatsappAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl WhatsappAdapter {
    /// 1:1 port of upstream
    /// `new WhatsappAdapter({ phoneNumberId, accessToken, verifyToken, graphBase? })`.
    pub fn new(options: WhatsappAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
        }
    }

    /// Override the HTTP client.
    pub fn with_http_client(
        mut self,
        client: chat_sdk_adapter_shared::runtime::reqwest::Client,
    ) -> Self {
        self.http = client;
        self
    }

    /// Read the business phone-number ID.
    pub fn phone_number_id(&self) -> &str {
        &self.options.phone_number_id
    }

    /// Read the access token.
    pub fn access_token(&self) -> &str {
        &self.options.access_token
    }

    /// Read the webhook verify token.
    pub fn verify_token(&self) -> &str {
        &self.options.verify_token
    }

    /// Effective Graph API base URL.
    pub fn graph_base(&self) -> &str {
        self.options.effective_graph_base()
    }

    /// Effective Graph API version (e.g. `"v21.0"`).
    pub fn api_version(&self) -> &str {
        self.options.effective_api_version()
    }

    /// 1:1 with upstream `protected readonly graphApiUrl: string` —
    /// `<graph_base>/<api_version>`.
    pub fn graph_api_url(&self) -> String {
        format!("{}/{}", self.graph_base(), self.api_version())
    }

    /// 1:1 with upstream `readonly appSecret?: string`.
    pub fn app_secret(&self) -> Option<&str> {
        self.options.app_secret.as_deref()
    }

    /// 1:1 with upstream `readonly userName: string` (with default).
    pub fn user_name(&self) -> &str {
        self.options.effective_user_name()
    }

    /// Build the Cloud API send URL. 1:1 with upstream's inline
    /// `<graph_base>/<DEFAULT_API_VERSION>/<phone_number_id>/messages`
    /// template.
    fn send_url(&self) -> String {
        format!(
            "{}/{}/messages",
            self.graph_api_url(),
            self.options.phone_number_id
        )
    }

    /// Derive channel id from a WhatsApp thread id. 1:1 with
    /// upstream `adapter.channelIdFromThreadId(threadId) -> threadId`.
    /// On WhatsApp every conversation is a 1:1 DM, so channel ===
    /// thread.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> String {
        thread_id.to_string()
    }

    /// All WhatsApp conversations are DMs. 1:1 with upstream's
    /// `adapter.isDM(_) -> true`.
    pub fn is_dm(&self, _thread_id: &str) -> bool {
        true
    }

    /// Render formatted content to WhatsApp markdown. 1:1 with
    /// upstream `adapter.renderFormatted(content)` which delegates
    /// to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::WhatsAppFormatConverter::new().from_ast(ast)
    }

    /// Open a Direct Message with `user_id` (E.164 customer phone).
    /// 1:1 with upstream `adapter.openDM(userId)` which returns
    /// `encodeThreadId({phoneNumberId: this.phoneNumberId, userWaId:
    /// userId})`. No HTTP call — WhatsApp Cloud API conversations
    /// are addressed by the business phone-number id + customer
    /// phone number.
    pub fn open_dm(&self, user_id: &str) -> String {
        encode_thread_id(&self.options.phone_number_id, user_id)
    }
}

#[async_trait]
impl Adapter for WhatsappAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`.
    /// WhatsApp has no separate channel id (every thread is a 1:1
    /// conversation) — returns the thread id unchanged.
    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        Some(self.channel_id_from_thread_id(thread_id))
    }

    /// 1:1 with upstream `adapter.isDM(threadId)`. WhatsApp is always
    /// a DM (no group chats in the Cloud API surface here).
    fn is_dm(&self, thread_id: &str) -> Option<bool> {
        Some(self.is_dm(thread_id))
    }

    /// 1:1 with upstream `adapter.openDM(userId)`. Delegates to the
    /// inherent [`WhatsappAdapter::open_dm`] which builds the thread
    /// id from the bound `phone_number_id` + `user_id` (WhatsApp
    /// Cloud API addresses conversations by `<business_phone>:<customer_phone>`
    /// — no HTTP call required).
    async fn open_dm(&self, user_id: &str) -> chat_sdk_chat::types::AdapterResult<String> {
        Ok(self.open_dm(user_id))
    }

    /// Post a text message via the WhatsApp Cloud API. 1:1 with
    /// upstream's `adapter.postMessage`:
    ///
    /// - Decodes `whatsapp:<phone_number_id>:<customer_phone>`.
    /// - POSTs JSON
    ///   `{messaging_product: "whatsapp", to: <customer_phone>,
    ///   type: "text", text: {body: <text>}}` to
    ///   `<graph_base>/<DEFAULT_API_VERSION>/<phone_number_id>/messages`.
    /// - Auth via `Authorization: Bearer <access_token>` header.
    /// - Returns the first element of `messages[*].id` (Cloud
    ///   API's envelope).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not WhatsApp-encoded"))
        })?;

        // The thread id's phone_number_id MUST match the adapter's
        // configured phone_number_id (the bot is keyed by phone
        // number on the Meta side).
        if decoded.phone_number_id != self.options.phone_number_id {
            return Err(AdapterError::InvalidPayload(format!(
                "thread_id phone_number_id {:?} does not match adapter's {:?}",
                decoded.phone_number_id, self.options.phone_number_id
            )));
        }

        let url = self.send_url();
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": decoded.customer_phone,
            "type": "text",
            "text": { "body": text },
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.access_token())
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
                .unwrap_or("WhatsApp Cloud API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        json["messages"][0]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(
                    "WhatsApp Cloud API response missing messages[0].id".to_string(),
                )
            })
    }

    /// WhatsApp does not support editing messages. 1:1 with
    /// upstream's `adapter.editMessage`.
    async fn edit_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "WhatsApp does not support editing messages. Use postMessage to send a new message instead.".to_string(),
        ))
    }

    /// WhatsApp does not support deleting messages. 1:1 with
    /// upstream's `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "WhatsApp does not support deleting messages.".to_string(),
        ))
    }

    /// Add an emoji reaction via WhatsApp Cloud API. 1:1 with
    /// upstream's `adapter.addReaction`: POST `{messaging_product:
    /// "whatsapp", recipient_type: "individual", to: <customer_phone>,
    /// type: "reaction", reaction: {message_id, emoji}}`.
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not WhatsApp-encoded"))
        })?;

        if decoded.phone_number_id != self.options.phone_number_id {
            return Err(AdapterError::InvalidPayload(format!(
                "thread_id phone_number_id {:?} does not match adapter's {:?}",
                decoded.phone_number_id, self.options.phone_number_id
            )));
        }

        let url = self.send_url();
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": decoded.customer_phone,
            "type": "reaction",
            "reaction": {
                "message_id": message_id,
                "emoji": emoji,
            },
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.access_token())
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        if !status.is_success() {
            let json: serde_json::Value = response.json().await.unwrap_or_default();
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("WhatsApp Cloud API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        Ok(())
    }

    /// Remove an emoji reaction by sending a `reaction` message with
    /// an **empty** emoji string. 1:1 with upstream's
    /// `adapter.removeReaction`: WhatsApp Cloud API removes the bot's
    /// reaction from `message_id` when the reaction payload's emoji
    /// is the empty string. Same POST endpoint and envelope as
    /// `add_reaction`; the `emoji` argument is intentionally ignored.
    async fn remove_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        _emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not WhatsApp-encoded"))
        })?;

        if decoded.phone_number_id != self.options.phone_number_id {
            return Err(AdapterError::InvalidPayload(format!(
                "thread_id phone_number_id {:?} does not match adapter's {:?}",
                decoded.phone_number_id, self.options.phone_number_id
            )));
        }

        let url = self.send_url();
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": decoded.customer_phone,
            "type": "reaction",
            "reaction": {
                "message_id": message_id,
                "emoji": "",
            },
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.access_token())
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        if !status.is_success() {
            let json: serde_json::Value = response.json().await.unwrap_or_default();
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("WhatsApp Cloud API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        Ok(())
    }

    /// WhatsApp Cloud API does not support typing indicators. 1:1
    /// with upstream's no-op `adapter.startTyping`.
    async fn start_typing(
        &self,
        _thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        Ok(())
    }
}

/// Default Meta Graph API version the adapter targets. 1:1 with
/// upstream's private `DEFAULT_API_VERSION = "v21.0"`. Used by
/// `send_url()` to compose `<graph_base>/<version>/<phone-number>/
/// messages`. Exposed at module scope (rather than private as
/// upstream) so callers + tests can reference the canonical
/// version string without re-declaring it.
pub const DEFAULT_API_VERSION: &str = "v21.0";

/// Maximum message length the WhatsApp Cloud API accepts in a single
/// send. 1:1 with upstream's `WHATSAPP_MESSAGE_LIMIT = 4096`.
pub const WHATSAPP_MESSAGE_LIMIT: usize = 4096;

/// Split text into chunks that fit within WhatsApp's message limit.
/// 1:1 port of upstream's `splitMessage(text)`:
///
/// 1. Short-circuit when the input already fits in
///    [`WHATSAPP_MESSAGE_LIMIT`] bytes.
/// 2. Otherwise, in a loop slice off the first
///    [`WHATSAPP_MESSAGE_LIMIT`]-byte prefix and look for a paragraph
///    boundary (`\n\n`); fall back to a line boundary (`\n`); fall back
///    to a hard byte cut at the limit. Reject break points that land
///    before the halfway mark of the prefix (matches upstream's
///    `breakIndex < WHATSAPP_MESSAGE_LIMIT / 2` guard, which prevents
///    creating tiny "early" chunks).
/// 3. `trim_end` the emitted chunk and `trim_start` the remainder
///    around the break (so leading/trailing whitespace from the
///    boundary itself is collapsed).
///
/// All slicing operates on bytes; `\n` is a single ASCII byte so the
/// break-finder works for any UTF-8 input without splitting multi-byte
/// sequences as long as the hard-cut byte position lands on a char
/// boundary. (For the upstream test suite all inputs are ASCII.)
pub fn split_message(text: &str) -> Vec<String> {
    if text.len() <= WHATSAPP_MESSAGE_LIMIT {
        return vec![text.to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut remaining = text;

    while remaining.len() > WHATSAPP_MESSAGE_LIMIT {
        let slice = &remaining[..WHATSAPP_MESSAGE_LIMIT];
        let half = WHATSAPP_MESSAGE_LIMIT / 2;

        // Try paragraph boundary first.
        let mut break_index: Option<usize> = slice.rfind("\n\n").filter(|&idx| idx >= half);

        // Then line boundary.
        if break_index.is_none() {
            break_index = slice.rfind('\n').filter(|&idx| idx >= half);
        }

        // Hard break.
        let cut = break_index.unwrap_or(WHATSAPP_MESSAGE_LIMIT);
        chunks.push(remaining[..cut].trim_end().to_string());
        remaining = remaining[cut..].trim_start();
    }

    if !remaining.is_empty() {
        chunks.push(remaining.to_string());
    }

    chunks
}

/// 1:1 with upstream `interface WhatsAppAdapterConfig` — all
/// fields optional so the factory can fall back to environment
/// variables. Used by [`try_create_whatsapp_adapter`].
#[derive(Debug, Clone, Default)]
pub struct WhatsappCreateOptions {
    /// Permanent access token. Falls back to `WHATSAPP_ACCESS_TOKEN`.
    pub access_token: Option<String>,
    /// Facebook app secret. Falls back to `WHATSAPP_APP_SECRET`.
    pub app_secret: Option<String>,
    /// Business phone-number ID. Falls back to `WHATSAPP_PHONE_NUMBER_ID`.
    pub phone_number_id: Option<String>,
    /// Webhook verify token. Falls back to `WHATSAPP_VERIFY_TOKEN`.
    pub verify_token: Option<String>,
    /// Display name override. Falls back to `WHATSAPP_BOT_USERNAME`,
    /// then [`DEFAULT_FACTORY_USER_NAME`].
    pub user_name: Option<String>,
    /// Graph API base URL override. Falls back to `WHATSAPP_API_URL`.
    pub api_url: Option<String>,
    /// Graph API version override. Falls back to [`DEFAULT_API_VERSION`].
    pub api_version: Option<String>,
}

/// Errors returned by [`try_create_whatsapp_adapter`]. 1:1 with
/// upstream `throw new ValidationError("whatsapp", "... is
/// required")` cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatsappCreateError {
    /// `accessToken` missing and `WHATSAPP_ACCESS_TOKEN` not set.
    AccessTokenRequired,
    /// `appSecret` missing and `WHATSAPP_APP_SECRET` not set.
    AppSecretRequired,
    /// `phoneNumberId` missing and `WHATSAPP_PHONE_NUMBER_ID` not set.
    PhoneNumberIdRequired,
    /// `verifyToken` missing and `WHATSAPP_VERIFY_TOKEN` not set.
    VerifyTokenRequired,
}

impl std::fmt::Display for WhatsappCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessTokenRequired => write!(
                f,
                "accessToken is required. Set WHATSAPP_ACCESS_TOKEN or provide it in config."
            ),
            Self::AppSecretRequired => write!(
                f,
                "appSecret is required. Set WHATSAPP_APP_SECRET or provide it in config."
            ),
            Self::PhoneNumberIdRequired => write!(
                f,
                "phoneNumberId is required. Set WHATSAPP_PHONE_NUMBER_ID or provide it in config."
            ),
            Self::VerifyTokenRequired => write!(
                f,
                "verifyToken is required. Set WHATSAPP_VERIFY_TOKEN or provide it in config."
            ),
        }
    }
}

impl std::error::Error for WhatsappCreateError {}

/// 1:1 with upstream `createWhatsAppAdapter(config)` env-var
/// resolution path. The `env` reader is a closure (avoids `unsafe
/// std::env::set_var` and parallel-test races).
///
/// Resolution rules (1:1 with upstream):
/// - `access_token` ← `opts` ?? `env("WHATSAPP_ACCESS_TOKEN")`
/// - `app_secret` ← `opts` ?? `env("WHATSAPP_APP_SECRET")`
/// - `phone_number_id` ← `opts` ?? `env("WHATSAPP_PHONE_NUMBER_ID")`
/// - `verify_token` ← `opts` ?? `env("WHATSAPP_VERIFY_TOKEN")`
/// - `user_name` ← `opts` ?? `env("WHATSAPP_BOT_USERNAME")` ??
///   [`DEFAULT_FACTORY_USER_NAME`]
/// - `api_url` ← `opts` ?? `env("WHATSAPP_API_URL")`
/// - `api_version` ← `opts` ?? [`DEFAULT_API_VERSION`]
pub fn try_create_whatsapp_adapter(
    opts: WhatsappCreateOptions,
    env: impl Fn(&str) -> Option<String>,
) -> Result<WhatsappAdapter, WhatsappCreateError> {
    let access_token = opts
        .access_token
        .or_else(|| env("WHATSAPP_ACCESS_TOKEN"))
        .ok_or(WhatsappCreateError::AccessTokenRequired)?;
    let app_secret = opts
        .app_secret
        .or_else(|| env("WHATSAPP_APP_SECRET"))
        .ok_or(WhatsappCreateError::AppSecretRequired)?;
    let phone_number_id = opts
        .phone_number_id
        .or_else(|| env("WHATSAPP_PHONE_NUMBER_ID"))
        .ok_or(WhatsappCreateError::PhoneNumberIdRequired)?;
    let verify_token = opts
        .verify_token
        .or_else(|| env("WHATSAPP_VERIFY_TOKEN"))
        .ok_or(WhatsappCreateError::VerifyTokenRequired)?;

    let user_name = opts
        .user_name
        .or_else(|| env("WHATSAPP_BOT_USERNAME"))
        .or_else(|| Some(DEFAULT_FACTORY_USER_NAME.to_string()));
    let graph_base = opts.api_url.or_else(|| env("WHATSAPP_API_URL"));

    Ok(WhatsappAdapter::new(WhatsappAdapterOptions {
        phone_number_id,
        access_token,
        verify_token,
        app_secret: Some(app_secret),
        user_name,
        graph_base,
        api_version: opts.api_version,
    }))
}

/// Encode a WhatsApp thread id. 1:1 with upstream's inline format:
/// `whatsapp:<phone_number_id>:<customer_phone>`.
pub fn encode_thread_id(phone_number_id: &str, customer_phone: &str) -> String {
    format!("{THREAD_ID_PREFIX}{phone_number_id}:{customer_phone}")
}

/// Components of a decoded WhatsApp thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedWhatsappThreadId {
    /// Business phone-number ID.
    pub phone_number_id: String,
    /// Customer phone number (E.164 form).
    pub customer_phone: String,
}

/// Decode a WhatsApp thread id. 1:1 with upstream
/// `decodeThreadId(threadId)`: requires exactly the
/// `whatsapp:<phone_number_id>:<customer_phone>` shape (rejects extra
/// segments and empty components). Returns `None` for any malformed
/// input; upstream throws `ValidationError` in the same cases.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedWhatsappThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let parts: Vec<&str> = suffix.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let phone_number_id = parts[0];
    let customer_phone = parts[1];
    if phone_number_id.is_empty() || customer_phone.is_empty() {
        return None;
    }
    Some(DecodedWhatsappThreadId {
        phone_number_id: phone_number_id.to_string(),
        customer_phone: customer_phone.to_string(),
    })
}

/// Predicate: does this thread id belong to the WhatsApp adapter?
pub fn is_whatsapp_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    //! ---------- upstream js-only-documented cases (1) ----------
    //!
    //! Per the slice-380 type-system-impossible pattern, the 1
    //! upstream `index.test.ts > describe("subclass extensibility")`
    //! case is enumerated as js-only-documented here:
    //!
    //! - `should expose protected members and methods to subclasses`:
    //!   TypeScript-class-`protected` access modifier check. Rust
    //!   uses `pub(crate)` visibility + trait composition rather
    //!   than class inheritance — the subclass-protected-leak test
    //!   is unrepresentable by construction.
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_whatsapp() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID", "access", "verify"));
        assert_eq!(adapter.name(), "whatsapp");
        assert_eq!(ADAPTER_NAME, "whatsapp");
    }

    #[test]
    fn options_new_stores_credentials_and_defaults_graph_base() {
        let opts = WhatsappAdapterOptions::new("PNID", "access", "verify");
        assert_eq!(opts.phone_number_id, "PNID");
        assert_eq!(opts.access_token, "access");
        assert_eq!(opts.verify_token, "verify");
        assert_eq!(opts.effective_graph_base(), DEFAULT_GRAPH_BASE);
    }

    #[test]
    fn options_with_graph_base_overrides_the_default() {
        let opts = WhatsappAdapterOptions::new("p", "a", "v")
            .with_graph_base("https://graph.example.test/v20.0");
        assert_eq!(
            opts.effective_graph_base(),
            "https://graph.example.test/v20.0"
        );
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(
            encode_thread_id("PNID123", "15551234567"),
            "whatsapp:PNID123:15551234567"
        );
    }

    #[test]
    fn decode_thread_id_parses_phone_number_id_and_customer_phone() {
        let decoded = decode_thread_id("whatsapp:PNID123:15551234567").unwrap();
        assert_eq!(decoded.phone_number_id, "PNID123");
        assert_eq!(decoded.customer_phone, "15551234567");
    }

    #[test]
    fn decode_thread_id_returns_none_for_invalid_prefix() {
        // 1:1 with upstream `decodeThreadId > should throw on invalid
        // prefix` (e.g. `slack:C123:ts123`). The Rust port maps the
        // throw to None per the Option<DecodedWhatsappThreadId> shape.
        assert!(decode_thread_id("slack:C123:ts123").is_none());
        assert!(decode_thread_id("messenger:PAGE:USER").is_none());
        assert!(decode_thread_id("telegram:123").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_empty_after_prefix() {
        // 1:1 with upstream `decodeThreadId > should throw on empty
        // after prefix` — `whatsapp:` (bare prefix with no segments).
        assert!(decode_thread_id("whatsapp:").is_none());
        // Also: `whatsapp` (no colon at all).
        assert!(decode_thread_id("whatsapp").is_none());
        // Also: `whatsapp:onlyone` (only 1 of 2 required segments).
        assert!(decode_thread_id("whatsapp:onlyone").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_user_wa_id() {
        // 1:1 with upstream `decodeThreadId > should throw on missing
        // userWaId` — `whatsapp:123456789:` (trailing colon, empty
        // 2nd segment).
        assert!(decode_thread_id("whatsapp:123456789:").is_none());
        assert!(decode_thread_id("whatsapp:PNID:").is_none());
        // Symmetric additive coverage: empty 1st segment also rejected.
        assert!(decode_thread_id("whatsapp::15551234567").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_completely_wrong_format() {
        // 1:1 with upstream `decodeThreadId("nonsense") throws`.
        assert!(decode_thread_id("nonsense").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_extra_segments() {
        // 1:1 with upstream `decodeThreadId("whatsapp:123:456:extra")
        // throws`. The Rust port now uses `split(':')` + exact-length
        // check (was `splitn(2, ':')` which silently accepted extras).
        assert!(decode_thread_id("whatsapp:123:456:extra").is_none());
    }

    #[test]
    fn encode_decode_round_trip_with_international_numbers() {
        // 1:1 with upstream `encodeThreadId / decodeThreadId roundtrip
        // > should round-trip with international numbers`.
        let encoded = encode_thread_id("999888777", "919876543210");
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded.phone_number_id, "999888777");
        assert_eq!(decoded.customer_phone, "919876543210");
    }

    #[test]
    fn encode_thread_id_works_with_different_phone_numbers() {
        // 1:1 with upstream `encodeThreadId > should encode with
        // different phone numbers`.
        assert_eq!(
            encode_thread_id("987654321", "44771234567"),
            "whatsapp:987654321:44771234567"
        );
    }

    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream `adapter.channelIdFromThreadId(_) -> threadId`
    // and `adapter.isDM(_) -> true`. WhatsApp is DM-only.

    #[test]
    // ---------- splitMessage (8 upstream cases) ----------
    // 1:1 with upstream `packages/adapter-whatsapp/src/index.test.ts`
    // `describe("splitMessage")` describe block.
    #[test]
    fn split_message_returns_a_single_chunk_for_short_messages() {
        assert_eq!(split_message("Hello world"), vec!["Hello world"]);
    }

    #[test]
    fn split_message_returns_a_single_chunk_for_exactly_4096_chars() {
        let text = "a".repeat(WHATSAPP_MESSAGE_LIMIT);
        assert_eq!(split_message(&text), vec![text.clone()]);
    }

    #[test]
    fn split_message_splits_on_paragraph_boundaries_when_possible() {
        let p1 = "a".repeat(3000);
        let p2 = "b".repeat(3000);
        let text = format!("{p1}\n\n{p2}");
        let result = split_message(&text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], p1);
        assert_eq!(result[1], p2);
    }

    #[test]
    fn split_message_splits_on_line_boundaries_when_no_paragraph_break() {
        let l1 = "a".repeat(3000);
        let l2 = "b".repeat(3000);
        let text = format!("{l1}\n{l2}");
        let result = split_message(&text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], l1);
        assert_eq!(result[1], l2);
    }

    #[test]
    fn split_message_hard_breaks_when_no_line_boundaries_exist() {
        let text = "a".repeat(5000);
        let result = split_message(&text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "a".repeat(4096));
        assert_eq!(result[1], "a".repeat(904));
    }

    #[test]
    fn split_message_handles_three_chunks() {
        let p1 = "a".repeat(4000);
        let p2 = "b".repeat(4000);
        let p3 = "c".repeat(4000);
        let text = format!("{p1}\n\n{p2}\n\n{p3}");
        let result = split_message(&text);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], p1);
        assert_eq!(result[1], p2);
        assert_eq!(result[2], p3);
    }

    #[test]
    fn split_message_skips_break_that_is_too_early_in_the_chunk() {
        // A paragraph break at position 1000 (< 2048 = limit/2) should
        // be skipped per upstream's `< WHATSAPP_MESSAGE_LIMIT / 2`
        // guard, falling through to a hard break at the limit.
        let early = "a".repeat(1000);
        let rest = "b".repeat(4500);
        let text = format!("{early}\n\n{rest}");
        let result = split_message(&text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 4096);
        assert_eq!(result[1].len(), text.len() - 4096);
    }

    #[test]
    fn split_message_preserves_all_content_across_chunks() {
        let text = "x".repeat(10000);
        let result = split_message(&text);
        assert_eq!(result.join(""), text);
    }

    // ---------- openDM (1 upstream case) ----------
    #[test]
    fn open_dm_builds_the_thread_id_from_phone_number_id_and_user_wa_id() {
        // 1:1 with upstream's `openDM(userId)` which calls
        // `encodeThreadId({phoneNumberId, userWaId: userId})`.
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID", "a", "v"));
        assert_eq!(adapter.open_dm("15551234567"), "whatsapp:PNID:15551234567");
    }

    // ---------- renderFormatted (1 upstream case) ----------
    // 1:1 with upstream `WhatsAppAdapter > renderFormatted >
    // should render markdown from AST`.

    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID", "a", "v"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn channel_id_from_thread_id_returns_the_thread_id_unchanged() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID", "a", "v"));
        assert_eq!(
            adapter.channel_id_from_thread_id("whatsapp:PNID:15551234567"),
            "whatsapp:PNID:15551234567"
        );
        assert_eq!(adapter.channel_id_from_thread_id("raw"), "raw");
    }

    #[test]
    fn is_dm_always_returns_true() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID", "a", "v"));
        assert!(adapter.is_dm("whatsapp:PNID:15551234567"));
        assert!(adapter.is_dm(""));
    }

    #[test]
    fn is_whatsapp_thread_id_detects_the_prefix() {
        assert!(is_whatsapp_thread_id("whatsapp:PNID:CUST"));
        assert!(!is_whatsapp_thread_id("messenger:1:2"));
        assert!(!is_whatsapp_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (p, c) in [
            ("PNID", "15551234567"),
            ("a", "b"),
            ("with-dash", "with.dot"),
        ] {
            let encoded = encode_thread_id(p, c);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.phone_number_id, p);
            assert_eq!(decoded.customer_phone, c);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_whatsapp_thread_ids() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("p", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not WhatsApp-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_post_message_rejects_thread_id_with_mismatched_phone_number_id() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID1", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("whatsapp:PNID2:15551234567", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not match"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_is_unsupported() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("whatsapp:P:1234567890", "msg", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support editing"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_is_unsupported() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("whatsapp:P:1234567890", "msg"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support deleting"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_whatsapp_thread_ids() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("slack:C1:1.0", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not WhatsApp-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_remove_reaction_rejects_non_whatsapp_thread_ids() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.remove_reaction("slack:C1:1.0", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not WhatsApp-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_remove_reaction_rejects_thread_id_with_mismatched_phone_number_id() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("MY_PNID", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.remove_reaction(
            "whatsapp:OTHER_PNID:15551234567",
            "wamid.msg1",
            "👍",
        ));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not match adapter's"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_is_a_noop() {
        // WhatsApp Cloud API doesn't expose typing indicators -
        // upstream's body is empty.
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        assert!(block_on(adapter.start_typing("anything", None)).is_ok());
        assert!(block_on(adapter.start_typing("anything", Some("s"))).is_ok());
    }

    #[test]
    #[test]
    fn default_api_version_matches_upstream() {
        // 1:1 with upstream's private `DEFAULT_API_VERSION = "v21.0"`.
        // The Rust port previously hardcoded `v22.0` in send_url;
        // slice 257 aligned the version with upstream.
        assert_eq!(DEFAULT_API_VERSION, "v21.0");
    }

    #[test]
    fn adapter_send_url_builds_the_upstream_endpoint() {
        let adapter = WhatsappAdapter::new(
            WhatsappAdapterOptions::new("PNID123", "a", "v")
                .with_graph_base("https://graph.example.test"),
        );
        assert_eq!(
            adapter.send_url(),
            "https://graph.example.test/v21.0/PNID123/messages"
        );
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = WhatsappAdapter::new(
            WhatsappAdapterOptions::new("PNID", "access-tok", "verify-tok")
                .with_graph_base("https://example.test"),
        );
        assert_eq!(adapter.phone_number_id(), "PNID");
        assert_eq!(adapter.access_token(), "access-tok");
        assert_eq!(adapter.verify_token(), "verify-tok");
        assert_eq!(adapter.graph_base(), "https://example.test");
    }

    // ---------- createWhatsAppAdapter create-instance (2 cases) ----------
    // 1:1 with portable subset of upstream `index.test.ts >
    // describe("createWhatsAppAdapter")`. Env-var-driven "throws
    // when X is missing" cases need an env-var-resolution factory;
    // documented as deferred.

    #[test]
    fn whatsapp_adapter_creates_an_instance() {
        let opts = WhatsappAdapterOptions::new("123456789", "test-token", "test-verify-token");
        let adapter = WhatsappAdapter::new(opts);
        assert_eq!(adapter.name(), "whatsapp");
        // Default userName = "bot".
        assert_eq!(adapter.user_name(), "bot");
        // app_secret defaults to None.
        assert!(adapter.app_secret().is_none());
    }

    #[test]
    fn whatsapp_adapter_uses_provided_user_name_and_app_secret() {
        let mut opts = WhatsappAdapterOptions::new("123456789", "test-token", "test-verify-token");
        opts.user_name = Some("test-bot".to_string());
        opts.app_secret = Some("test-secret".to_string());
        let adapter = WhatsappAdapter::new(opts);
        assert_eq!(adapter.user_name(), "test-bot");
        assert_eq!(adapter.app_secret(), Some("test-secret"));
    }

    // ---------- createWhatsAppAdapter env-var resolution (6 cases) ----------
    // 1:1 with upstream `index.test.ts > describe("createWhatsAppAdapter")`.
    // Env reader is an injected closure (Rust 2024 `unsafe set_var`
    // is racy across parallel tests).

    fn required_whatsapp_env(key: &str) -> Option<String> {
        match key {
            "WHATSAPP_ACCESS_TOKEN" => Some("env-token".to_string()),
            "WHATSAPP_APP_SECRET" => Some("env-secret".to_string()),
            "WHATSAPP_PHONE_NUMBER_ID" => Some("env-phone-id".to_string()),
            "WHATSAPP_VERIFY_TOKEN" => Some("env-verify".to_string()),
            _ => None,
        }
    }

    fn empty_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn create_whatsapp_adapter_throws_when_access_token_is_missing() {
        let err = try_create_whatsapp_adapter(
            WhatsappCreateOptions {
                app_secret: Some("secret".to_string()),
                phone_number_id: Some("123".to_string()),
                verify_token: Some("verify".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect_err("missing accessToken");
        assert_eq!(err, WhatsappCreateError::AccessTokenRequired);
        let msg = err.to_string();
        // Upstream matches /accessToken/i — Rust mirrors the
        // case-sensitive substring.
        assert!(msg.contains("accessToken"));
    }

    #[test]
    fn create_whatsapp_adapter_throws_when_app_secret_is_missing() {
        let err = try_create_whatsapp_adapter(
            WhatsappCreateOptions {
                access_token: Some("token".to_string()),
                phone_number_id: Some("123".to_string()),
                verify_token: Some("verify".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect_err("missing appSecret");
        assert_eq!(err, WhatsappCreateError::AppSecretRequired);
        assert!(err.to_string().contains("appSecret"));
    }

    #[test]
    fn create_whatsapp_adapter_uses_environment_variables_as_fallback() {
        let adapter =
            try_create_whatsapp_adapter(WhatsappCreateOptions::default(), required_whatsapp_env)
                .expect("env-only construction");
        assert_eq!(adapter.name(), "whatsapp");
        assert_eq!(adapter.phone_number_id(), "env-phone-id");
        assert_eq!(adapter.access_token(), "env-token");
        assert_eq!(adapter.verify_token(), "env-verify");
        assert_eq!(adapter.app_secret(), Some("env-secret"));
        // Factory default userName.
        assert_eq!(adapter.user_name(), "whatsapp-bot");
    }

    #[test]
    fn create_whatsapp_adapter_uses_api_url_config_to_override_base_url() {
        let adapter = try_create_whatsapp_adapter(
            WhatsappCreateOptions {
                access_token: Some("test-token".to_string()),
                app_secret: Some("test-secret".to_string()),
                phone_number_id: Some("123456789".to_string()),
                verify_token: Some("test-verify-token".to_string()),
                user_name: Some("test-bot".to_string()),
                api_url: Some("https://custom-graph.example.com".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("apiUrl config override");
        assert_eq!(
            adapter.graph_api_url(),
            "https://custom-graph.example.com/v21.0"
        );
    }

    #[test]
    fn create_whatsapp_adapter_uses_whatsapp_api_url_env_var_via_factory() {
        let env = |key: &str| match key {
            "WHATSAPP_API_URL" => Some("https://custom-graph.example.com".to_string()),
            other => required_whatsapp_env(other),
        };
        let adapter = try_create_whatsapp_adapter(WhatsappCreateOptions::default(), env)
            .expect("WHATSAPP_API_URL env applied");
        assert_eq!(
            adapter.graph_api_url(),
            "https://custom-graph.example.com/v21.0"
        );
    }

    #[test]
    fn create_whatsapp_adapter_uses_api_url_with_custom_api_version() {
        let adapter = try_create_whatsapp_adapter(
            WhatsappCreateOptions {
                access_token: Some("test-token".to_string()),
                app_secret: Some("test-secret".to_string()),
                phone_number_id: Some("123456789".to_string()),
                verify_token: Some("test-verify-token".to_string()),
                user_name: Some("test-bot".to_string()),
                api_url: Some("https://custom-graph.example.com".to_string()),
                api_version: Some("v19.0".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("custom apiVersion");
        assert_eq!(
            adapter.graph_api_url(),
            "https://custom-graph.example.com/v19.0"
        );
    }
}
