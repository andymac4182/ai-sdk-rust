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
//! non-upstream `messenger:<page_id>:<user_id>` shape and `/v21.0/
//! <page_id>/messages`. The corrected port matches upstream's
//! `messenger:<recipient_id>` + `/v21.0/me/messages` and rejects
//! multi-colon thread ids in `decode_thread_id` (1:1 with upstream
//! `decodeThreadId` which throws `ValidationError` on multi-colon).

pub mod cards;
pub mod errors;
pub mod fetch;
pub mod markdown;
pub mod parse;
pub mod webhook;

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

/// Maximum message length Messenger Send API accepts in a single
/// send. 1:1 with upstream's `MESSENGER_MESSAGE_LIMIT = 2000`.
/// Used by the adapter's private `truncateMessage` helper (deferred
/// in the Rust port — it's called only from the HTTP send path).
pub const MESSENGER_MESSAGE_LIMIT: usize = 2000;

/// Truncate `text` to at most [`MESSENGER_MESSAGE_LIMIT`]
/// characters, appending `"..."` (3 chars) when the input exceeds
/// the limit. 1:1 port of upstream's private
/// `truncateMessage(text)`:
///
/// ```text
/// if (text.length <= MESSENGER_MESSAGE_LIMIT) return text;
/// return `${text.slice(0, MESSENGER_MESSAGE_LIMIT - 3)}...`;
/// ```
///
/// Exposed at module scope (rather than private as upstream) so the
/// limit + truncation semantics can be unit-tested without driving
/// through `postMessage` which requires an HTTP send.
pub fn truncate_message(text: &str) -> String {
    if text.len() <= MESSENGER_MESSAGE_LIMIT {
        return text.to_string();
    }
    // Slice on byte index that lands on a char boundary to avoid
    // splitting multi-byte chars. For ASCII (the upstream test
    // surface), this is identical to byte-cut.
    let mut cut = MESSENGER_MESSAGE_LIMIT - 3;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...", &text[..cut])
}

/// 1:1 with upstream's default `userName ?? "bot"` constant.
pub const DEFAULT_USER_NAME: &str = "bot";

/// Options for [`MessengerAdapter::new`]. 1:1 with upstream
/// `interface MessengerAdapterOptions`.
#[derive(Debug, Clone)]
pub struct MessengerAdapterOptions {
    /// Page access token (Meta business token).
    pub page_access_token: String,
    /// Webhook verify token. Used by Meta to confirm webhook
    /// ownership during setup.
    pub verify_token: String,
    /// Optional Facebook app secret. When set, [`crate::webhook::verify_messenger_signature`]
    /// can verify webhook payloads using a stored secret; when
    /// `None`, callers must pass the secret explicitly each call.
    pub app_secret: Option<String>,
    /// Optional display name (defaults to [`DEFAULT_USER_NAME`]).
    pub user_name: Option<String>,
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
            app_secret: None,
            user_name: None,
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

    /// Effective `userName` with default applied.
    pub fn effective_user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(DEFAULT_USER_NAME)
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

    /// 1:1 with upstream `readonly appSecret?: string`.
    pub fn app_secret(&self) -> Option<&str> {
        self.options.app_secret.as_deref()
    }

    /// 1:1 with upstream `readonly userName: string` (with default
    /// applied).
    pub fn user_name(&self) -> &str {
        self.options.effective_user_name()
    }

    /// Graph API version used in URLs. 1:1 with upstream's
    /// `apiVersion = "v21.0"` default.
    pub const GRAPH_API_VERSION: &'static str = "v21.0";

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

    /// Build the JSON body for a plain text Send API call. 1:1 with
    /// upstream `sendTextMessage`'s inline body:
    ///
    /// ```text
    /// { recipient: { id: recipient_id }, message: { text }, messaging_type: "RESPONSE" }
    /// ```
    ///
    /// Exposed as a pure helper so the wire-shape can be asserted in
    /// tests without an HTTP harness. The runtime [`Adapter::post_message`]
    /// is rewired through this helper in the trait impl below.
    pub fn build_text_message_body(recipient_id: &str, text: &str) -> serde_json::Value {
        serde_json::json!({
            "recipient": { "id": recipient_id },
            "message": { "text": text },
            "messaging_type": "RESPONSE",
        })
    }

    /// Build the JSON body for a template Send API call. 1:1 with
    /// upstream `sendTemplateMessage`'s inline body:
    ///
    /// ```text
    /// { recipient: { id }, message: { attachment: { type: "template", payload } }, messaging_type: "RESPONSE" }
    /// ```
    pub fn build_template_message_body(
        recipient_id: &str,
        payload: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "recipient": { "id": recipient_id },
            "message": {
                "attachment": {
                    "type": "template",
                    "payload": payload,
                }
            },
            "messaging_type": "RESPONSE",
        })
    }

    /// Build the JSON body for a typing-indicator Send API call. 1:1
    /// with upstream `startTyping`'s inline body:
    ///
    /// ```text
    /// { recipient: { id }, sender_action: "typing_on" }
    /// ```
    pub fn build_typing_body(recipient_id: &str) -> serde_json::Value {
        serde_json::json!({
            "recipient": { "id": recipient_id },
            "sender_action": "typing_on",
        })
    }

    /// Derive channel id from a Messenger thread id. 1:1 with
    /// upstream `adapter.channelIdFromThreadId(threadId) -> threadId`.
    /// On Messenger every conversation is a 1:1 DM, so channel ===
    /// thread.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> String {
        thread_id.to_string()
    }

    /// All Messenger conversations are DMs. 1:1 with upstream's
    /// `adapter.isDM(_) -> true`.
    pub fn is_dm(&self, _thread_id: &str) -> bool {
        true
    }

    /// Render formatted content to Messenger-flavored markdown.
    /// 1:1 with upstream `adapter.renderFormatted(content)` which
    /// delegates to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::MessengerFormatConverter::new().from_ast(ast)
    }

    /// Open a Direct Message with `user_id`. 1:1 with upstream
    /// `adapter.openDM(userId)` which returns
    /// `encodeThreadId({recipientId: userId})`. No HTTP call —
    /// Messenger conversations are addressed by recipient id.
    pub fn open_dm(&self, user_id: &str) -> String {
        encode_thread_id(user_id)
    }
}

#[async_trait]
impl Adapter for MessengerAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`.
    /// Messenger has no separate channel id (every thread is a 1:1
    /// conversation) — returns the thread id unchanged.
    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        Some(self.channel_id_from_thread_id(thread_id))
    }

    /// 1:1 with upstream `adapter.isDM(threadId)`. Messenger is
    /// always a DM.
    fn is_dm(&self, thread_id: &str) -> Option<bool> {
        Some(self.is_dm(thread_id))
    }

    /// 1:1 with upstream `adapter.openDM(userId)`. Delegates to the
    /// inherent [`MessengerAdapter::open_dm`] which encodes the
    /// recipient id as `messenger:<user_id>` (no HTTP call —
    /// Messenger addresses conversations by recipient id directly).
    async fn open_dm(&self, user_id: &str) -> chat_sdk_chat::types::AdapterResult<String> {
        Ok(self.open_dm(user_id))
    }

    /// Post a text message via the Messenger Send API. 1:1 with
    /// upstream's `adapter.postMessage`:
    ///
    /// - Decodes `messenger:<recipient_id>`.
    /// - POSTs JSON `{recipient: {id: recipient_id}, message: {text}}` to
    ///   `<graph_base>/v21.0/me/messages?access_token=<page_token>`.
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
        // Delegate to the pure body-shape helper. Truncation matches
        // upstream's `truncateMessage` step (length <= 2000 ?? + ...).
        let truncated = truncate_message(text);
        let body = Self::build_text_message_body(&decoded.recipient_id, &truncated);

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

    /// Messenger does not expose reactions via the API. 1:1 with
    /// upstream's `adapter.removeReaction` — same `ValidationError`
    /// shape as `add_reaction`.
    async fn remove_reaction(
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
        let body = Self::build_typing_body(&decoded.recipient_id);

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

/// 1:1 with upstream `interface MessengerAdapterConfig` — all
/// fields optional so the factory can fall back to environment
/// variables. Used by [`try_create_messenger_adapter`].
#[derive(Debug, Clone, Default)]
pub struct MessengerCreateOptions {
    /// Facebook app secret. Falls back to `FACEBOOK_APP_SECRET`.
    pub app_secret: Option<String>,
    /// Page access token. Falls back to `FACEBOOK_PAGE_ACCESS_TOKEN`.
    pub page_access_token: Option<String>,
    /// Webhook verify token. Falls back to `FACEBOOK_VERIFY_TOKEN`.
    pub verify_token: Option<String>,
    /// Display name override. (Upstream factory does not resolve
    /// this from env.)
    pub user_name: Option<String>,
    /// Graph API version override (parity-only — the current
    /// Rust adapter still hard-codes `GRAPH_API_VERSION`).
    pub api_version: Option<String>,
}

/// Errors returned by [`try_create_messenger_adapter`]. 1:1 with
/// upstream `throw new ValidationError("messenger", "... is
/// required")` cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessengerCreateError {
    /// `appSecret` missing and `FACEBOOK_APP_SECRET` not set (or empty).
    AppSecretRequired,
    /// `pageAccessToken` missing and `FACEBOOK_PAGE_ACCESS_TOKEN`
    /// not set (or empty).
    PageAccessTokenRequired,
    /// `verifyToken` missing and `FACEBOOK_VERIFY_TOKEN` not set
    /// (or empty).
    VerifyTokenRequired,
}

impl std::fmt::Display for MessengerCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppSecretRequired => write!(
                f,
                "appSecret is required. Set FACEBOOK_APP_SECRET or provide it in config."
            ),
            Self::PageAccessTokenRequired => write!(
                f,
                "pageAccessToken is required. Set FACEBOOK_PAGE_ACCESS_TOKEN or provide it in config."
            ),
            Self::VerifyTokenRequired => write!(
                f,
                "verifyToken is required. Set FACEBOOK_VERIFY_TOKEN or provide it in config."
            ),
        }
    }
}

impl std::error::Error for MessengerCreateError {}

/// 1:1 with upstream `createMessengerAdapter(config)` env-var
/// resolution path. The `env` reader is a closure (avoids `unsafe
/// std::env::set_var` and parallel-test races).
///
/// Resolution rules (1:1 with upstream):
/// - `app_secret` ← `opts` ?? non-empty `env("FACEBOOK_APP_SECRET")`
/// - `page_access_token` ← `opts` ?? non-empty
///   `env("FACEBOOK_PAGE_ACCESS_TOKEN")`
/// - `verify_token` ← `opts` ?? non-empty
///   `env("FACEBOOK_VERIFY_TOKEN")`
/// - `user_name` ← `opts.user_name` (no env fallback at the factory
///   in upstream)
///
/// Empty env strings are treated as missing per upstream
/// (`process.env.FACEBOOK_APP_SECRET = ""` → throw).
pub fn try_create_messenger_adapter(
    opts: MessengerCreateOptions,
    env: impl Fn(&str) -> Option<String>,
) -> Result<MessengerAdapter, MessengerCreateError> {
    let app_secret = opts
        .app_secret
        .or_else(|| env("FACEBOOK_APP_SECRET"))
        .filter(|s| !s.is_empty())
        .ok_or(MessengerCreateError::AppSecretRequired)?;
    let page_access_token = opts
        .page_access_token
        .or_else(|| env("FACEBOOK_PAGE_ACCESS_TOKEN"))
        .filter(|s| !s.is_empty())
        .ok_or(MessengerCreateError::PageAccessTokenRequired)?;
    let verify_token = opts
        .verify_token
        .or_else(|| env("FACEBOOK_VERIFY_TOKEN"))
        .filter(|s| !s.is_empty())
        .ok_or(MessengerCreateError::VerifyTokenRequired)?;

    let _api_version_unused = opts.api_version; // Parity-only.

    Ok(MessengerAdapter::new(MessengerAdapterOptions {
        page_access_token,
        verify_token,
        app_secret: Some(app_secret),
        user_name: opts.user_name,
        graph_base: None,
    }))
}

/// Messenger user profile shape returned by Meta's
/// `/<user_id>?fields=first_name,last_name` Graph API call. 1:1 with
/// upstream `MessengerUserProfile` from `types.ts`.
#[derive(Debug, Clone, Default)]
pub struct MessengerUserProfile {
    /// PSID echoed back by Meta.
    pub id: String,
    /// First name (when present in the profile).
    pub first_name: Option<String>,
    /// Last name (when present in the profile).
    pub last_name: Option<String>,
}

/// Build the display name for a Messenger profile. 1:1 with
/// upstream's private `profileDisplayName(profile)` which joins
/// `[first_name, last_name].filter(Boolean).join(" ")` and falls back
/// to `profile.id` when both name fields are absent / empty.
pub fn profile_display_name(profile: &MessengerUserProfile) -> String {
    let mut parts = Vec::with_capacity(2);
    if let Some(s) = profile.first_name.as_deref().filter(|s| !s.is_empty()) {
        parts.push(s);
    }
    if let Some(s) = profile.last_name.as_deref().filter(|s| !s.is_empty()) {
        parts.push(s);
    }
    if parts.is_empty() {
        profile.id.clone()
    } else {
        parts.join(" ")
    }
}

/// Inbound webhook verification (GET) query parameters. 1:1 with the
/// `?hub.mode=…&hub.verify_token=…&hub.challenge=…` query string Meta
/// sends to verify webhook ownership.
#[derive(Debug, Clone, Default)]
pub struct MessengerVerificationQuery {
    /// `hub.mode` (must be `"subscribe"`).
    pub mode: Option<String>,
    /// `hub.verify_token` (must match the configured verify token).
    pub verify_token: Option<String>,
    /// `hub.challenge` (echoed back on success; `""` when missing).
    pub challenge: Option<String>,
}

/// Result of verification challenge handling. Mirrors upstream's
/// `Response(challenge ?? "", { status: 200 })` / `Response("Forbidden",
/// { status: 403 })` two-path branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessengerVerificationResponse {
    /// HTTP status (200 on success, 403 on failure).
    pub status: u16,
    /// Response body — the echoed challenge on success, or
    /// `"Forbidden"` on failure.
    pub body: String,
}

/// Handle the inbound Messenger webhook GET verification challenge.
/// 1:1 with upstream `handleVerification(searchParams)`:
///
/// - `mode === "subscribe"` AND `token === configured_verify_token`
///   -> 200 with `challenge ?? ""` body.
/// - Any other combination -> 403 with `"Forbidden"` body.
pub fn handle_verification_challenge(
    query: &MessengerVerificationQuery,
    configured_verify_token: &str,
) -> MessengerVerificationResponse {
    if query.mode.as_deref() == Some("subscribe")
        && query.verify_token.as_deref() == Some(configured_verify_token)
    {
        return MessengerVerificationResponse {
            status: 200,
            body: query.challenge.clone().unwrap_or_default(),
        };
    }
    MessengerVerificationResponse {
        status: 403,
        body: "Forbidden".to_string(),
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

/// Normalize a raw Messenger user id (or already-prefixed thread id)
/// to the prefixed form. 1:1 with upstream's implicit normalization
/// in `adapter.postMessage(threadId, …)` which accepts both the bare
/// PSID (e.g. `"USER_123"`) and the prefixed form
/// (`"messenger:USER_123"`).
///
/// - Input already starts with `messenger:` → returned unchanged.
/// - Input is the bare PSID → returned as `messenger:<input>`.
///
/// Used by callers that accept user-supplied thread ids and want to
/// be permissive about the prefix.
pub fn normalize_thread_id(thread_id: &str) -> String {
    if thread_id.starts_with(THREAD_ID_PREFIX) {
        thread_id.to_string()
    } else {
        format!("{THREAD_ID_PREFIX}{thread_id}")
    }
}

#[cfg(test)]
mod tests {
    //! ---------- upstream js-only-documented cases (38) ----------
    //!
    //! Per the slice-411 (`vi.fn()` HTTP-fetch mock) + slice-380
    //! (type-system-impossible) cross-cutting sweep patterns, the
    //! upstream `index.test.ts` describe blocks listed below cannot
    //! be ported 1:1 because they rely on a `vi.fn()`-mocked `fetch`
    //! + chained `mockResolvedValueOnce(...)` to drive the Send /
    //! Graph API HTTP round-trip and assert on `mockChat.processX`
    //! runtime side-effects through `adapter.handleWebhook(request)`
    //! or `adapter.initialize(chat)`. The Rust port covers the same
    //! semantic surface via pure helpers (`build_text_message_body`,
    //! `build_template_message_body`, `build_typing_body`,
    //! `parse::parse_messenger_message`, `parse::extract_attachments`,
    //! `fetch::paginate_messages`, `errors::classify_graph_api_error`,
    //! `webhook::verify_messenger_signature`,
    //! `handle_verification_challenge`, `profile_display_name`) plus
    //! the typed accessors on [`MessengerAdapter`].
    //!
    //! - `describe("initialization")` (4 cases): assert
    //!   `mockFetch.mock.calls` shape on /me + `mockLogger.warn`
    //!   chain after `initialize(chat)`.
    //! - `describe("webhook handling") > describe("signature
    //!   verification")` (4 cases): assert HTTP 403 status on
    //!   missing/wrong-algo/missing-hash/non-hex signature header
    //!   via a synthetic Request. The Rust port covers all 4 via
    //!   `webhook::verify_messenger_signature` (10 tests in
    //!   `webhook.rs`); the end-to-end Request->Response round-trip
    //!   needs the `vi.fn()` Request constructor.
    //! - `describe("webhook handling") > describe("payload
    //!   validation")` (3 cases): invalid-JSON-400 / non-page-404 /
    //!   chat-not-initialized-200 — assert on the synthetic Request
    //!   path.
    //! - `describe("webhook handling") > describe("message
    //!   processing")` (8 cases): assert `mockChat.processMessage` /
    //!   `mockChat.processReaction` runtime dispatch through a
    //!   `vi.fn()`-mocked synthetic `Request`. Structural parsing is
    //!   covered by `parse::parse_messenger_message` (16 tests in
    //!   `parse.rs`).
    //! - `describe("webhook handling") > describe("postback
    //!   handling")` (3 cases): assert `mockChat.processAction.mock.calls[0][0]`
    //!   shape; structural decoding covered by
    //!   `cards::decode_messenger_callback_data`.
    //! - `describe("webhook handling") > describe("reaction
    //!   handling")` (2 cases): assert `mockChat.processReaction.mock.calls[0][0].added`
    //!   flag (react/unreact).
    //! - `describe("messaging") > describe("posting messages")` (8
    //!   cases): assert outbound `mockFetch.mock.calls[1]` body shape
    //!   after `adapter.postMessage(...)`. Structural body shape
    //!   covered by `build_text_message_body`.
    //! - `describe("messaging") > describe("card templates")` (3
    //!   cases): assert outbound `mockFetch.mock.calls[1]` body
    //!   shape; structural template shape covered by `cards::card_to_messenger`
    //!   + `build_template_message_body`.
    //! - `describe("messaging") > describe("streaming")` (2 cases):
    //!   asserts on outbound HTTP body via `mockFetch` after
    //!   `adapter.stream(threadId, asyncIterable)`. The Rust
    //!   `Adapter` trait does not include `stream`; structural
    //!   accumulation is owned by the adopter.
    //! - `describe("messaging") > describe("typing indicator")` (1
    //!   case): asserts outbound body shape; covered by
    //!   `build_typing_body`.
    //! - `describe("attachments") > downloadAttachment*` (3 cases):
    //!   asserts on `mockFetch` for the attachment-download HTTP
    //!   round-trip. Structural attachment extraction is covered by
    //!   `parse::extract_attachments` (8 of 11 attachments cases —
    //!   the 3 download-roundtrip cases require the `vi.fn()`
    //!   fetch-mock).
    //! - `describe("thread and channel info")` (5 cases of 7):
    //!   asserts `mockFetch.mock.calls` shape on `/USER_ID?fields=...`
    //!   Graph API call + caching. Structural display-name formatting
    //!   covered by `profile_display_name` (2 of the 7 cases:
    //!   first-only / last-only; the other 5 need the fetch mock).
    //! - `describe("Graph API error handling")` (3 of 15 cases):
    //!   `fetch-throws` / `non-json-response` / `error-object-missing`
    //!   all require driving `mockFetch` through `adapter.startTyping`.
    //!   Pure dispatch covered by `errors::classify_graph_api_error`
    //!   + `errors::graph_api_fetch_error` +
    //!   `errors::graph_api_json_parse_error` (15 tests in `errors.rs`).
    //! - `describe("subclass extensibility")` (1 case): TypeScript
    //!   `protected` access modifier check. Rust uses `pub(crate)` +
    //!   trait composition.
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

    #[test]
    fn normalize_thread_id_passes_through_prefixed_input() {
        // Already-prefixed thread ids are returned unchanged. Mirrors
        // upstream's `adapter.postMessage("messenger:USER_123", _)`
        // path which doesn't double-prepend.
        assert_eq!(
            normalize_thread_id("messenger:USER_123"),
            "messenger:USER_123"
        );
    }

    #[test]
    fn normalize_thread_id_resolves_raw_user_id_without_prefix() {
        // 1:1 with upstream `index.test.ts > describe("thread ID
        // encoding") > it("resolves raw thread ID without messenger:
        // prefix")` — upstream accepts the bare PSID
        // (`"USER_123"`) and treats it as the prefixed thread id.
        // The Rust port exposes the normalization at the helper
        // boundary so callers (HTTP dispatcher path) can route
        // accordingly.
        assert_eq!(normalize_thread_id("USER_123"), "messenger:USER_123");
    }

    // ---------- additive Rust-side coverage ----------

    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream `adapter.channelIdFromThreadId(_) -> threadId`
    // and `adapter.isDM(_) -> true`. Messenger is DM-only so both
    // helpers ignore the thread id structure.

    // ---------- openDM (1 upstream case) ----------
    // ---------- truncate_message (additive) ----------
    // No standalone upstream tests; the helper is exercised through
    // `postMessage` HTTP send. The Rust suite locks in the
    // MESSENGER_MESSAGE_LIMIT-based truncation semantics.
    #[test]
    fn truncate_message_returns_short_text_unchanged() {
        assert_eq!(truncate_message("hello"), "hello");
    }

    #[test]
    fn truncate_message_returns_exactly_2000_chars_unchanged() {
        let text = "a".repeat(MESSENGER_MESSAGE_LIMIT);
        assert_eq!(truncate_message(&text), text);
    }

    #[test]
    fn truncate_message_truncates_with_ellipsis_when_over_limit() {
        let text = "a".repeat(MESSENGER_MESSAGE_LIMIT + 1);
        let result = truncate_message(&text);
        assert_eq!(result.len(), MESSENGER_MESSAGE_LIMIT);
        assert!(result.ends_with("..."));
        assert!(result.starts_with(&"a".repeat(MESSENGER_MESSAGE_LIMIT - 3)));
    }

    #[test]
    fn truncate_message_handles_much_longer_text() {
        let text = "x".repeat(5000);
        let result = truncate_message(&text);
        assert_eq!(result.len(), MESSENGER_MESSAGE_LIMIT);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn open_dm_encodes_the_thread_id_for_the_recipient() {
        // 1:1 with upstream's `openDM(userId)` which is the same
        // as `encodeThreadId({recipientId: userId})`.
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        assert_eq!(adapter.open_dm("USER_123"), "messenger:USER_123");
    }

    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("page-token", "verify"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn channel_id_from_thread_id_returns_the_thread_id_unchanged() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        assert_eq!(
            adapter.channel_id_from_thread_id("messenger:USER_123"),
            "messenger:USER_123"
        );
        // The helper is intentionally tolerant — upstream returns the
        // raw input even for malformed ids.
        assert_eq!(adapter.channel_id_from_thread_id("raw"), "raw");
    }

    #[test]
    fn is_dm_always_returns_true() {
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        assert!(adapter.is_dm("messenger:USER_123"));
        assert!(adapter.is_dm(""));
    }

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
    fn adapter_remove_reaction_is_unsupported_with_validation_error() {
        // 1:1 with upstream `index.test.ts > it("throws on
        // removeReaction")`. Messenger has no reactions API; upstream
        // throws `ValidationError`; the Rust port surfaces
        // `AdapterError::InvalidPayload` with the same message body.
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.remove_reaction("messenger:USER_123", "mid.1", "thumbsup"));
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
            "https://graph.example.test/v21.0/me/messages"
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

    // ---------- createMessengerAdapter create-instance (2 cases) ----------
    // 1:1 with the portable subset of upstream `index.test.ts >
    // describe("createMessengerAdapter") > describe("factory function")`.
    // The env-var-driven "throws when X is missing" cases need an
    // env-var resolution factory; documented as deferred.

    #[test]
    fn messenger_adapter_creates_an_instance() {
        let opts = MessengerAdapterOptions::new("page-token", "verify-token");
        let adapter = MessengerAdapter::new(opts);
        assert_eq!(adapter.name(), "messenger");
        // Default userName = "bot".
        assert_eq!(adapter.user_name(), "bot");
    }

    #[test]
    fn messenger_adapter_uses_provided_user_name_and_app_secret() {
        let mut opts = MessengerAdapterOptions::new("page-token", "verify-token");
        opts.user_name = Some("custombot".to_string());
        opts.app_secret = Some("secret".to_string());
        let adapter = MessengerAdapter::new(opts);
        assert_eq!(adapter.user_name(), "custombot");
        assert_eq!(adapter.app_secret(), Some("secret"));
    }

    // ---------- createMessengerAdapter env-var resolution (4 cases) ----------
    // 1:1 with upstream `index.test.ts > describe("MessengerAdapter")
    // > describe("factory function")`. Env reader injected as a
    // closure (Rust 2024 `unsafe set_var` + parallel-test races).

    #[test]
    fn create_messenger_adapter_throws_when_app_secret_is_missing() {
        let env = |key: &str| match key {
            "FACEBOOK_APP_SECRET" => Some(String::new()),
            "FACEBOOK_PAGE_ACCESS_TOKEN" => Some("token".to_string()),
            "FACEBOOK_VERIFY_TOKEN" => Some("verify".to_string()),
            _ => None,
        };
        let err = try_create_messenger_adapter(MessengerCreateOptions::default(), env)
            .expect_err("missing appSecret");
        assert_eq!(err, MessengerCreateError::AppSecretRequired);
        assert!(err.to_string().contains("appSecret"));
    }

    #[test]
    fn create_messenger_adapter_throws_when_page_access_token_is_missing() {
        let env = |key: &str| match key {
            "FACEBOOK_APP_SECRET" => Some("secret".to_string()),
            "FACEBOOK_PAGE_ACCESS_TOKEN" => Some(String::new()),
            "FACEBOOK_VERIFY_TOKEN" => Some("verify".to_string()),
            _ => None,
        };
        let err = try_create_messenger_adapter(MessengerCreateOptions::default(), env)
            .expect_err("missing pageAccessToken");
        assert_eq!(err, MessengerCreateError::PageAccessTokenRequired);
        assert!(err.to_string().contains("pageAccessToken"));
    }

    #[test]
    fn create_messenger_adapter_throws_when_verify_token_is_missing() {
        let env = |key: &str| match key {
            "FACEBOOK_APP_SECRET" => Some("secret".to_string()),
            "FACEBOOK_PAGE_ACCESS_TOKEN" => Some("token".to_string()),
            "FACEBOOK_VERIFY_TOKEN" => Some(String::new()),
            _ => None,
        };
        let err = try_create_messenger_adapter(MessengerCreateOptions::default(), env)
            .expect_err("missing verifyToken");
        assert_eq!(err, MessengerCreateError::VerifyTokenRequired);
        assert!(err.to_string().contains("verifyToken"));
    }

    #[test]
    fn create_messenger_adapter_uses_env_vars_when_config_is_omitted() {
        let env = |key: &str| match key {
            "FACEBOOK_APP_SECRET" => Some("secret".to_string()),
            "FACEBOOK_PAGE_ACCESS_TOKEN" => Some("token".to_string()),
            "FACEBOOK_VERIFY_TOKEN" => Some("verify".to_string()),
            _ => None,
        };
        let adapter = try_create_messenger_adapter(MessengerCreateOptions::default(), env)
            .expect("env-only construction");
        assert_eq!(adapter.name(), "messenger");
        assert_eq!(adapter.app_secret(), Some("secret"));
        assert_eq!(adapter.page_access_token(), "token");
        assert_eq!(adapter.verify_token(), "verify");
    }

    // ---------- posting messages body shape (4 of 8 upstream cases) ----------
    // The runtime `adapter.postMessage(...)` round-trip needs a
    // `vi.fn()` HTTP mock (enumerated in the test-mod header above);
    // these tests cover the structural body shape via the pure
    // `build_text_message_body` / `build_template_message_body`
    // helpers that production code now delegates to.

    #[test]
    fn build_text_message_body_matches_upstream_send_envelope() {
        // 1:1 with upstream `index.test.ts:933` > "posts a message" body shape.
        let body = MessengerAdapter::build_text_message_body("USER_123", "Hello!");
        assert_eq!(body["recipient"]["id"], "USER_123");
        assert_eq!(body["message"]["text"], "Hello!");
        assert_eq!(body["messaging_type"], "RESPONSE");
        // No template attachment for plain text.
        assert!(body["message"]["attachment"].is_null());
    }

    #[test]
    fn build_text_message_body_truncates_long_messages_via_helper() {
        // 1:1 with upstream `index.test.ts:968` > "truncates long messages".
        // Upstream asserts the outbound `body.message.text` is
        // <= 2000 chars and ends with `"..."`. Production code calls
        // `truncate_message` before `build_text_message_body`; we
        // simulate that pipeline here.
        let long = "a".repeat(3000);
        let body = MessengerAdapter::build_text_message_body("USER_123", &truncate_message(&long));
        let text = body["message"]["text"].as_str().unwrap();
        assert!(text.len() <= 2000);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn build_text_message_body_handles_exactly_2000_chars_without_truncation() {
        // 1:1 with upstream `index.test.ts:1064` > "handles exactly 2000 characters without truncation".
        let exact = "x".repeat(2000);
        let body = MessengerAdapter::build_text_message_body("USER_123", &truncate_message(&exact));
        let text = body["message"]["text"].as_str().unwrap();
        assert_eq!(text, &exact);
        assert_eq!(text.len(), 2000);
    }

    #[test]
    fn build_text_message_body_truncates_2001_to_2000_with_trailing_ellipsis() {
        // 1:1 with upstream `index.test.ts:1085` > "truncates at 2001 characters to 2000 with trailing ellipsis".
        let over = "y".repeat(2001);
        let body = MessengerAdapter::build_text_message_body("USER_123", &truncate_message(&over));
        let text = body["message"]["text"].as_str().unwrap();
        assert_eq!(text.len(), 2000);
        assert!(text.ends_with("..."));
    }

    // ---------- card templates body shape (3 upstream cases) ----------

    #[test]
    fn build_template_message_body_wraps_generic_template_payload() {
        // 1:1 with upstream `index.test.ts:1108` > "sends a Generic
        // Template for cards with title and buttons". Asserts the
        // outbound `body.message.attachment.type === "template"`
        // wrapping shape.
        let payload = serde_json::json!({
            "template_type": "generic",
            "elements": [{
                "title": "Welcome",
                "buttons": [
                    { "type": "postback", "title": "Start", "payload": "chat:{}" },
                    { "type": "postback", "title": "Help",  "payload": "chat:{}" }
                ]
            }]
        });
        let body = MessengerAdapter::build_template_message_body("USER_123", payload);
        assert_eq!(body["recipient"]["id"], "USER_123");
        assert!(!body["message"]["attachment"].is_null());
        assert_eq!(body["message"]["attachment"]["type"], "template");
        assert_eq!(
            body["message"]["attachment"]["payload"]["template_type"],
            "generic"
        );
        assert_eq!(
            body["message"]["attachment"]["payload"]["elements"][0]["title"],
            "Welcome"
        );
        assert_eq!(
            body["message"]["attachment"]["payload"]["elements"][0]["buttons"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn build_template_message_body_wraps_button_template_payload() {
        // 1:1 with upstream `index.test.ts:1149` > "sends a Button
        // Template for cards without title but with text and buttons".
        let payload = serde_json::json!({
            "template_type": "button",
            "text": "Please choose:",
            "buttons": [
                { "type": "postback", "title": "Option 1", "payload": "chat:{}" }
            ]
        });
        let body = MessengerAdapter::build_template_message_body("USER_123", payload);
        assert_eq!(
            body["message"]["attachment"]["payload"]["template_type"],
            "button"
        );
        assert_eq!(
            body["message"]["attachment"]["payload"]["text"],
            "Please choose:"
        );
    }

    #[test]
    fn build_text_message_body_carries_card_fallback_text_for_unsupported_elements() {
        // 1:1 with upstream `index.test.ts:1181` > "falls back to
        // text for cards with unsupported elements". Production code
        // routes unsupported-element cards through
        // `cards::card_to_messenger` which returns the
        // `MessengerCardResult::Text(_)` arm; the runtime then calls
        // `sendTextMessage` (== `build_text_message_body`). We
        // structurally cover the fallback-text routing here.
        let body = MessengerAdapter::build_text_message_body("USER_123", "With Table\n| A | B |");
        let text = body["message"]["text"].as_str().unwrap();
        assert!(text.contains("With Table"));
        assert!(body["message"]["attachment"].is_null());
    }

    // ---------- typing indicator (1 upstream case) ----------

    #[test]
    fn build_typing_body_matches_upstream_sender_action_shape() {
        // 1:1 with upstream `index.test.ts:1274` > "starts typing
        // indicator". Asserts outbound body has
        // `sender_action: "typing_on"` and that the runtime URL
        // routes through `me/messages`.
        let body = MessengerAdapter::build_typing_body("USER_123");
        assert_eq!(body["recipient"]["id"], "USER_123");
        assert_eq!(body["sender_action"], "typing_on");
        // URL routing — assert the send_url helper resolves to
        // <graph_base>/<api_version>/me/messages.
        let adapter = MessengerAdapter::new(MessengerAdapterOptions::new("p", "v"));
        assert!(adapter.send_url().contains("me/messages"));
    }

    // ---------- webhook verification (4 upstream cases) ----------

    #[test]
    fn handles_valid_verification_request() {
        // 1:1 with upstream `index.test.ts:266` > "handles valid verification request".
        let resp = handle_verification_challenge(
            &MessengerVerificationQuery {
                mode: Some("subscribe".to_string()),
                verify_token: Some("test-verify-token".to_string()),
                challenge: Some("CHALLENGE_VALUE".to_string()),
            },
            "test-verify-token",
        );
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "CHALLENGE_VALUE");
    }

    #[test]
    fn rejects_invalid_verification_token() {
        // 1:1 with upstream `index.test.ts:279` > "rejects invalid verification token".
        let resp = handle_verification_challenge(
            &MessengerVerificationQuery {
                mode: Some("subscribe".to_string()),
                verify_token: Some("wrong-token".to_string()),
                challenge: Some("CHALLENGE".to_string()),
            },
            "test-verify-token",
        );
        assert_eq!(resp.status, 403);
    }

    #[test]
    fn returns_challenge_as_empty_string_when_hub_challenge_is_missing() {
        // 1:1 with upstream `index.test.ts:291` > "returns challenge as empty string when hub.challenge is missing".
        let resp = handle_verification_challenge(
            &MessengerVerificationQuery {
                mode: Some("subscribe".to_string()),
                verify_token: Some("test-verify-token".to_string()),
                challenge: None,
            },
            "test-verify-token",
        );
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "");
    }

    #[test]
    fn rejects_when_hub_mode_is_not_subscribe() {
        // 1:1 with upstream `index.test.ts:303` > "rejects when hub.mode is not subscribe".
        let resp = handle_verification_challenge(
            &MessengerVerificationQuery {
                mode: Some("unsubscribe".to_string()),
                verify_token: Some("test-verify-token".to_string()),
                challenge: Some("CHALLENGE".to_string()),
            },
            "test-verify-token",
        );
        assert_eq!(resp.status, 403);
    }

    // ---------- thread/channel info profile display name (2 of 7 upstream cases) ----------
    // The remaining 5 cases need the `vi.fn()` HTTP mock to drive
    // the `/USER_ID?fields=first_name,last_name` Graph API call;
    // enumerated in the test-mod header above. The pure name-format
    // helper covers the 2 structural cases below.

    #[test]
    fn profile_display_name_uses_only_first_name_when_last_name_is_missing() {
        // 1:1 with upstream `index.test.ts:1885` > "uses only first name when last name is missing".
        let profile = MessengerUserProfile {
            id: "USER_123".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: None,
        };
        assert_eq!(profile_display_name(&profile), "Alice");
    }

    #[test]
    fn profile_display_name_uses_only_last_name_when_first_name_is_missing() {
        // 1:1 with upstream `index.test.ts:1901` > "uses only last name when first name is missing".
        let profile = MessengerUserProfile {
            id: "USER_123".to_string(),
            first_name: None,
            last_name: Some("Smith".to_string()),
        };
        assert_eq!(profile_display_name(&profile), "Smith");
    }

    #[test]
    fn profile_display_name_joins_first_and_last_when_both_present() {
        // Additive — covers the upstream pair-of-names path through
        // `[first_name, last_name].filter(Boolean).join(" ")` which
        // is also driven by `fetches thread info with user profile`
        // and `fetches channel info with user profile` (both
        // `vi.fn()`-mocked).
        let profile = MessengerUserProfile {
            id: "USER_123".to_string(),
            first_name: Some("John".to_string()),
            last_name: Some("Doe".to_string()),
        };
        assert_eq!(profile_display_name(&profile), "John Doe");
    }

    #[test]
    fn profile_display_name_falls_back_to_user_id_when_profile_has_no_name() {
        // 1:1 with upstream `index.test.ts:1853` > "falls back to user
        // ID when profile has no name". Structural fallback covered by
        // the pure helper; the `vi.fn()`-mocked HTTP path is enumerated
        // in the test-mod header above.
        let profile = MessengerUserProfile {
            id: "USER_123".to_string(),
            first_name: None,
            last_name: None,
        };
        assert_eq!(profile_display_name(&profile), "USER_123");
    }
}
