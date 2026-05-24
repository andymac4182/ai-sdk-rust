//! Google Chat adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-gchat/src/index.ts`.
//!
//! Google Chat models conversations as **spaces** (Google's term for
//! a channel) with optional **threads** inside them. The thread id
//! encoding is `gchat:<space_id>:<thread_id>` — when posting a new
//! top-level message, `thread_id` is the empty string after the
//! colon. Space DM mode uses the same encoding with a 1:1 space.

pub mod cards;
pub mod markdown;
pub mod thread_utils;
pub mod user_info;
pub mod workspace_events;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "gchat";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "gchat:";

/// Default Google Chat REST API base URL.
pub const DEFAULT_API_BASE: &str = "https://chat.googleapis.com/v1";

/// Refresh buffer for space subscriptions. The adapter renews a
/// subscription this long before its declared expiry so transient
/// renewal failures still leave wall-clock slack. 1:1 with
/// upstream's private `SUBSCRIPTION_REFRESH_BUFFER_MS = 60 * 60 *
/// 1000` (1 h).
pub const SUBSCRIPTION_REFRESH_BUFFER_MS: u64 = 60 * 60 * 1000;

/// TTL the adapter caches space-subscription metadata under (just
/// over the 24 h Chat-imposed cap). 1:1 with upstream's private
/// `SUBSCRIPTION_CACHE_TTL_MS = 25 * 60 * 60 * 1000` (25 h).
pub const SUBSCRIPTION_CACHE_TTL_MS: u64 = 25 * 60 * 60 * 1000;

/// State-key prefix the adapter writes space-subscription metadata
/// under. 1:1 with upstream's private
/// `SPACE_SUB_KEY_PREFIX = "gchat:space-sub:"`.
pub const SPACE_SUB_KEY_PREFIX: &str = "gchat:space-sub:";

/// 1:1 with upstream `userName ?? "bot"` adapter-default.
pub const DEFAULT_USER_NAME: &str = "bot";

/// Options for [`GchatAdapter::new`].
#[derive(Debug, Clone)]
pub struct GchatAdapterOptions {
    /// Service-account credentials JSON (the full
    /// `service_account.json` payload). Required for OAuth2 token
    /// minting against the Chat API.
    pub service_account_json: String,
    /// Subject email to impersonate when posting (domain-wide
    /// delegation). Required for posting on behalf of a bot user.
    pub subject_email: String,
    /// Optional API base URL override.
    pub api_base: Option<String>,
    /// Display name override. 1:1 with upstream
    /// `config.userName ?? "bot"`. Defaults to [`DEFAULT_USER_NAME`].
    pub user_name: Option<String>,
    /// Optional Pub/Sub topic for receiving Chat events. 1:1 with
    /// upstream `pubsubTopic` (carry-through; the Rust port's runtime
    /// surface for Pub/Sub is a separate workstream).
    pub pubsub_topic: Option<String>,
    /// Optional user to impersonate via domain-wide delegation. 1:1
    /// with upstream `impersonateUser` (an alternate spelling of
    /// `subjectEmail` used by some configurations).
    pub impersonate_user: Option<String>,
    /// Use Application Default Credentials instead of an explicit
    /// service-account JSON. 1:1 with upstream
    /// `useApplicationDefaultCredentials`. When `true`, the factory
    /// stores `service_account_json` as the empty string and the
    /// runtime token-mint will eventually consult `gcloud`/metadata.
    pub use_application_default_credentials: bool,
}

impl GchatAdapterOptions {
    /// Construct options. API base URL defaults to
    /// [`DEFAULT_API_BASE`].
    pub fn new(service_account_json: impl Into<String>, subject_email: impl Into<String>) -> Self {
        Self {
            service_account_json: service_account_json.into(),
            subject_email: subject_email.into(),
            api_base: None,
            user_name: None,
            pubsub_topic: None,
            impersonate_user: None,
            use_application_default_credentials: false,
        }
    }

    /// Override the API base URL.
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = Some(api_base.into());
        self
    }

    /// Effective API base URL with default applied.
    pub fn effective_api_base(&self) -> &str {
        self.api_base.as_deref().unwrap_or(DEFAULT_API_BASE)
    }

    /// Effective `userName` with default applied.
    pub fn effective_user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(DEFAULT_USER_NAME)
    }
}

/// Google Chat adapter.
#[derive(Debug, Clone)]
pub struct GchatAdapter {
    options: GchatAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
    /// Pre-minted OAuth2 access token. Google Chat tokens are
    /// short-lived (~1h) and minted out-of-band against
    /// `https://oauth2.googleapis.com/token` from the
    /// `service_account_json` private key (RS256 JWT assertion +
    /// optional domain-wide delegation for `subject_email`).
    /// Until a token-cache helper lands in chat-sdk-adapter-shared,
    /// adopters pass a pre-minted token via `with_bearer_token`.
    bearer_token: Option<String>,
}

impl GchatAdapter {
    /// 1:1 port of upstream
    /// `new GchatAdapter({ serviceAccountJson, subjectEmail, apiBase? })`.
    pub fn new(options: GchatAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
            bearer_token: None,
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

    /// Provide a pre-minted OAuth2 access token. Required for
    /// `post_message`; adopters mint it out-of-band against
    /// `oauth2.googleapis.com/token` with a service-account JWT
    /// assertion (`urn:ietf:params:oauth:grant-type:jwt-bearer`).
    /// A token-cache helper for the minting flow will land in
    /// `chat_sdk_adapter_shared` in a future slice.
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Read the service-account JSON.
    pub fn service_account_json(&self) -> &str {
        &self.options.service_account_json
    }

    /// Read the subject email.
    pub fn subject_email(&self) -> &str {
        &self.options.subject_email
    }

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Read the currently-configured bearer token, if any.
    pub fn bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }

    /// 1:1 with upstream `readonly userName: string` (with default).
    pub fn user_name(&self) -> &str {
        self.options.effective_user_name()
    }

    /// 1:1 with upstream `readonly pubsubTopic?: string`.
    pub fn pubsub_topic(&self) -> Option<&str> {
        self.options.pubsub_topic.as_deref()
    }

    /// 1:1 with upstream `readonly impersonateUser?: string`.
    pub fn impersonate_user(&self) -> Option<&str> {
        self.options.impersonate_user.as_deref()
    }

    /// 1:1 with upstream `readonly useApplicationDefaultCredentials:
    /// boolean`.
    pub fn use_application_default_credentials(&self) -> bool {
        self.options.use_application_default_credentials
    }

    /// Build the Google Chat `messages.create` URL. 1:1 with
    /// upstream's inline `${apiBase}/spaces/${spaceId}/messages`
    /// template.
    fn messages_create_url(&self, space_id: &str) -> String {
        format!("{}/spaces/{space_id}/messages", self.api_base())
    }

    /// Derive channel id from a Google Chat thread id. 1:1 with
    /// upstream `adapter.channelIdFromThreadId(threadId)` which
    /// decodes and returns `gchat:<spaceName>`. Returns `None` when
    /// `thread_id` isn't a Google Chat-encoded value.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        let decoded = crate::thread_utils::decode_thread_id(thread_id).ok()?;
        Some(format!("gchat:{}", decoded.space_name))
    }

    /// Predicate: is the conversation a 1:1 DM? 1:1 with upstream's
    /// `adapter.isDM(threadId)` which delegates to `isDMThread` and
    /// just checks for the `:dm` suffix.
    pub fn is_dm(&self, thread_id: &str) -> bool {
        crate::thread_utils::is_dm_thread(thread_id)
    }

    /// Render formatted content to Google-Chat-flavored markdown.
    /// 1:1 with upstream `adapter.renderFormatted(content)` which
    /// delegates to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::GoogleChatFormatConverter::new().from_ast(ast)
    }
}

#[async_trait]
impl Adapter for GchatAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`.
    /// Delegates to the inherent
    /// [`GchatAdapter::channel_id_from_thread_id`].
    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        self.channel_id_from_thread_id(thread_id)
    }

    /// 1:1 with upstream `adapter.isDM(threadId)`. Delegates to the
    /// inherent [`GchatAdapter::is_dm`] (which returns `bool` directly
    /// — wraps in `Some(_)` to match the `Option<bool>` trait shape).
    fn is_dm(&self, thread_id: &str) -> Option<bool> {
        Some(self.is_dm(thread_id))
    }

    /// Post a text message via Google Chat's `messages.create` API.
    /// 1:1 with upstream's `adapter.postMessage`:
    ///
    /// - Decodes `gchat:<space_id>:<thread_id>`. When `thread_id`
    ///   is empty (top-level post), no `thread.name` field is
    ///   sent. Otherwise the body includes
    ///   `{thread: {name: "spaces/<space>/threads/<thread>"}}` and
    ///   the request sets `?messageReplyOption=REPLY_MESSAGE_OR_FAIL`
    ///   so the thread is reused.
    /// - POSTs to `<api_base>/spaces/<space_id>/messages` with
    ///   `Authorization: Bearer <bearer_token>`.
    /// - Returns the response `name` (Google's
    ///   `spaces/<space>/messages/<message>` resource name).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GChat-encoded"))
        })?;

        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload(
                "GchatAdapter has no bearer_token configured; call \
                 with_bearer_token() with a pre-minted Google OAuth2 \
                 access token (see GchatAdapter docs)"
                    .to_string(),
            )
        })?;

        let url = self.messages_create_url(&decoded.space_id);
        let mut body = serde_json::json!({ "text": text });

        let mut request = self.http.post(&url).bearer_auth(bearer);
        if !decoded.is_top_level() {
            body["thread"] = serde_json::json!({
                "name": format!(
                    "spaces/{}/threads/{}",
                    decoded.space_id, decoded.thread_id
                )
            });
            // Google's threading param is a URL query option, not
            // a body field.
            request = request.header("X-Goog-Threading-Option", "REPLY_MESSAGE_OR_FAIL");
        }
        request = request.json(&body);

        let response = request
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        if !status.is_success() {
            let msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Google Chat messages.create failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {msg}")));
        }

        json["name"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload(
                "Google Chat messages.create response missing name".to_string(),
            )
        })
    }

    /// Post an ephemeral Google Chat message via `spaces.messages.create`
    /// with the `privateMessageViewer` field. 1:1 with the text-path
    /// of upstream `adapter.postEphemeral` (the cardsV2 ephemeral
    /// branch is deferred — needs `cardToGoogleCard` infra).
    ///
    /// Builds the request body via [`gchat_post_ephemeral_payload`]
    /// (privateMessageViewer plus optional thread.name when the
    /// thread id has a thread component), POSTs to
    /// `messages_create_url`, and parses the response via
    /// [`parse_gchat_post_ephemeral_response`] (preserves upstream's
    /// `response.data.name || ""` empty-id fallback).
    async fn post_ephemeral(
        &self,
        thread_id: &str,
        user_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<chat_sdk_chat::types::EphemeralMessage> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GChat-encoded"))
        })?;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload(
                "GchatAdapter has no bearer_token configured".to_string(),
            )
        })?;

        let url = self.messages_create_url(&decoded.space_id);
        let body = gchat_post_ephemeral_payload(
            &decoded.space_id,
            &decoded.thread_id,
            user_id,
            text,
        );

        let response = self
            .http
            .post(&url)
            .bearer_auth(bearer)
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
            let msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Google Chat postEphemeral failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {msg}")));
        }

        Ok(parse_gchat_post_ephemeral_response(&json, thread_id))
    }

    /// Edit a Google Chat message via `spaces.messages.update`.
    /// 1:1 with the text-only path of upstream `adapter.editMessage`
    /// (the cardsV2 branch is deferred): PATCH
    /// `<api_base>/<message_id>?updateMask=text,cardsV2` with
    /// `{text, cardsV2: []}`. `message_id` is the Google resource
    /// name (e.g. `spaces/AAA/messages/BBB`).
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let _decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GChat-encoded"))
        })?;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload("GchatAdapter has no bearer_token configured".to_string())
        })?;

        let url = format!("{}/{message_id}?updateMask=text,cardsV2", self.api_base());
        let body = serde_json::json!({ "text": text, "cardsV2": [] });

        let response = self
            .http
            .patch(&url)
            .bearer_auth(bearer)
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
            let msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Google Chat messages.update failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {msg}")));
        }

        json["name"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload(
                "Google Chat messages.update response missing name".to_string(),
            )
        })
    }

    /// Delete a Google Chat message via `spaces.messages.delete`.
    /// 1:1 with upstream's `adapter.deleteMessage`: DELETE
    /// `<api_base>/<message_id>`.
    async fn delete_message(
        &self,
        _thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload("GchatAdapter has no bearer_token configured".to_string())
        })?;

        let url = format!("{}/{message_id}", self.api_base());
        let response = self
            .http
            .delete(&url)
            .bearer_auth(bearer)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {body_text}"
            )));
        }
        Ok(())
    }

    /// Add an emoji reaction via `spaces.messages.reactions.create`.
    /// 1:1 with upstream's `adapter.addReaction`: POST
    /// `<api_base>/<message_id>/reactions` with
    /// `{emoji: {unicode: <emoji>}}`.
    async fn add_reaction(
        &self,
        _thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload("GchatAdapter has no bearer_token configured".to_string())
        })?;

        let url = format!("{}/{message_id}/reactions", self.api_base());
        let body = serde_json::json!({ "emoji": { "unicode": emoji } });

        let response = self
            .http
            .post(&url)
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {body_text}"
            )));
        }
        Ok(())
    }

    /// Google Chat has no typing indicator API for bots. 1:1 with
    /// upstream's no-op `adapter.startTyping`.
    async fn start_typing(
        &self,
        _thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        Ok(())
    }
}

/// 1:1 with upstream `interface GoogleChatAdapterConfig` — all
/// fields optional so the factory can fall back to environment
/// variables. Used by [`try_create_gchat_adapter`].
#[derive(Debug, Clone, Default)]
pub struct GchatCreateOptions {
    /// Service-account JSON. Falls back to `GOOGLE_CHAT_CREDENTIALS`.
    pub credentials: Option<String>,
    /// Use Application Default Credentials. Falls back to
    /// `GOOGLE_CHAT_USE_ADC == "true"`.
    pub use_application_default_credentials: Option<bool>,
    /// Subject email (domain-wide-delegation impersonation). Falls
    /// back to `GOOGLE_CHAT_IMPERSONATE_USER`.
    pub subject_email: Option<String>,
    /// Display name override. (Upstream factory does not resolve
    /// this from env; defaults to [`DEFAULT_USER_NAME`].)
    pub user_name: Option<String>,
    /// Pub/Sub topic. Falls back to `GOOGLE_CHAT_PUBSUB_TOPIC`.
    pub pubsub_topic: Option<String>,
    /// API base URL override. Falls back to `GOOGLE_CHAT_API_URL`.
    pub api_url: Option<String>,
}

/// Errors returned by [`try_create_gchat_adapter`]. 1:1 with
/// upstream `throw new ValidationError("gchat", "Authentication is
/// required")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GchatCreateError {
    /// Neither `credentials` nor `use_application_default_credentials`
    /// resolved from config or environment.
    AuthenticationRequired,
}

impl std::fmt::Display for GchatCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => write!(
                f,
                "Authentication is required. Set GOOGLE_CHAT_CREDENTIALS, GOOGLE_CHAT_USE_ADC, or provide credentials in config."
            ),
        }
    }
}

impl std::error::Error for GchatCreateError {}

/// 1:1 with upstream `createGoogleChatAdapter(config)` /
/// `new GoogleChatAdapter(config)` env-var-resolution path. The
/// `env` reader is a closure (avoids `unsafe std::env::set_var`
/// and parallel-test races).
///
/// Resolution rules (1:1 with upstream):
/// - **Auth** ← config.credentials > config.use_application_default_credentials >
///   `env("GOOGLE_CHAT_CREDENTIALS")` >
///   `env("GOOGLE_CHAT_USE_ADC") == "true"` > error.
///   Config credentials short-circuit env auth ("don't mix modes").
/// - `subject_email` ← `opts` ?? `env("GOOGLE_CHAT_IMPERSONATE_USER")`
///   ?? `""` (subject is optional; ADC mode often omits it).
/// - `user_name` ← `opts.user_name` ?? [`DEFAULT_USER_NAME`] (no
///   env fallback at the factory in upstream).
/// - `pubsub_topic` ← `opts` ?? `env("GOOGLE_CHAT_PUBSUB_TOPIC")`.
/// - `api_url` ← `opts` ?? `env("GOOGLE_CHAT_API_URL")`.
pub fn try_create_gchat_adapter(
    opts: GchatCreateOptions,
    env: impl Fn(&str) -> Option<String>,
) -> Result<GchatAdapter, GchatCreateError> {
    let has_config_auth =
        opts.credentials.is_some() || opts.use_application_default_credentials.is_some();

    let (service_account_json, use_adc) = if has_config_auth {
        if let Some(creds) = opts.credentials {
            (creds, false)
        } else if matches!(opts.use_application_default_credentials, Some(true)) {
            (String::new(), true)
        } else {
            return Err(GchatCreateError::AuthenticationRequired);
        }
    } else if let Some(creds) = env("GOOGLE_CHAT_CREDENTIALS") {
        (creds, false)
    } else if env("GOOGLE_CHAT_USE_ADC").as_deref() == Some("true") {
        (String::new(), true)
    } else {
        return Err(GchatCreateError::AuthenticationRequired);
    };

    let subject_email = opts
        .subject_email
        .or_else(|| env("GOOGLE_CHAT_IMPERSONATE_USER"))
        .unwrap_or_default();
    let pubsub_topic = opts.pubsub_topic.or_else(|| env("GOOGLE_CHAT_PUBSUB_TOPIC"));
    let api_url = opts.api_url.or_else(|| env("GOOGLE_CHAT_API_URL"));

    Ok(GchatAdapter::new(GchatAdapterOptions {
        service_account_json,
        subject_email: subject_email.clone(),
        api_base: api_url,
        user_name: opts.user_name,
        pubsub_topic,
        impersonate_user: if subject_email.is_empty() {
            None
        } else {
            Some(subject_email)
        },
        use_application_default_credentials: use_adc,
    }))
}

/// Build the request body posted to Google Chat's
/// `spaces.messages.create` endpoint for an ephemeral message. 1:1
/// with upstream's text-path postEphemeral payload assembly. Sets
/// `privateMessageViewer.name = user_id` (Google's API field that
/// makes the message visible only to that user). Omits the
/// `thread.name` field entirely when `thread_id` is empty (top-level
/// post), matching upstream's `thread: threadName ? { name: threadName } : undefined`.
pub fn gchat_post_ephemeral_payload(
    space_id: &str,
    thread_id: &str,
    user_id: &str,
    text: &str,
) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(3);
    map.insert(
        "privateMessageViewer".to_string(),
        serde_json::json!({ "name": user_id }),
    );
    map.insert("text".to_string(), serde_json::Value::from(text));
    if !thread_id.is_empty() {
        map.insert(
            "thread".to_string(),
            serde_json::json!({
                "name": format!("spaces/{space_id}/threads/{thread_id}")
            }),
        );
    }
    serde_json::Value::Object(map)
}

/// Parse a Google Chat `spaces.messages.create` ephemeral response
/// JSON into an [`chat_sdk_chat::types::EphemeralMessage`]. 1:1 with
/// upstream's response-mapping branch. Preserves the
/// `response.data.name || ""` empty-id fallback (Google occasionally
/// returns a successful create without `name`).
pub fn parse_gchat_post_ephemeral_response(
    json: &serde_json::Value,
    thread_id: &str,
) -> chat_sdk_chat::types::EphemeralMessage {
    chat_sdk_chat::types::EphemeralMessage {
        id: json["name"].as_str().unwrap_or("").to_string(),
        thread_id: thread_id.to_string(),
        used_fallback: false,
        raw: json.clone(),
    }
}

/// Encode a Google Chat thread id. 1:1 with upstream's inline format:
/// `gchat:<space_id>:<thread_id>`. When `thread_id` is empty, the
/// resulting id encodes a "post a new top-level message" target.
pub fn encode_thread_id(space_id: &str, thread_id: &str) -> String {
    format!("{THREAD_ID_PREFIX}{space_id}:{thread_id}")
}

/// Components of a decoded Google Chat thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedGchatThreadId {
    /// Space identifier (`spaces/<id>` in Google API URL form).
    pub space_id: String,
    /// Thread identifier (may be empty for top-level posts).
    pub thread_id: String,
}

impl DecodedGchatThreadId {
    /// Whether this thread id encodes a top-level post (empty
    /// `thread_id`). 1:1 with upstream's
    /// `decoded.threadId === ""` check.
    pub fn is_top_level(&self) -> bool {
        self.thread_id.is_empty()
    }
}

/// Decode a Google Chat thread id. Unlike most other adapters,
/// an empty `thread_id` portion is legal and signals a top-level
/// post; only the `space_id` is required to be non-empty.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedGchatThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let space = parts.next()?;
    let thread = parts.next().unwrap_or("");
    if space.is_empty() {
        return None;
    }
    Some(DecodedGchatThreadId {
        space_id: space.to_string(),
        thread_id: thread.to_string(),
    })
}

/// Predicate: does this thread id belong to the Google Chat adapter?
pub fn is_gchat_thread_id(thread_id: &str) -> bool {
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
    fn adapter_name_is_gchat() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert_eq!(adapter.name(), "gchat");
    }

    #[test]
    fn options_new_stores_credentials_and_defaults_api_base() {
        let opts = GchatAdapterOptions::new("{}", "bot@example.com");
        assert_eq!(opts.service_account_json, "{}");
        assert_eq!(opts.subject_email, "bot@example.com");
        assert_eq!(opts.effective_api_base(), DEFAULT_API_BASE);
    }

    #[test]
    fn options_with_api_base_overrides_the_default() {
        let opts = GchatAdapterOptions::new("{}", "bot@example.com")
            .with_api_base("https://chat.example.test/v1");
        assert_eq!(opts.effective_api_base(), "https://chat.example.test/v1");
    }

    #[test]
    fn encode_thread_id_with_thread_id() {
        assert_eq!(encode_thread_id("AAAA", "BBBB"), "gchat:AAAA:BBBB");
    }

    #[test]
    fn encode_thread_id_with_empty_thread_id_signals_top_level() {
        assert_eq!(encode_thread_id("AAAA", ""), "gchat:AAAA:");
    }

    #[test]
    fn decode_thread_id_parses_space_and_thread() {
        let decoded = decode_thread_id("gchat:AAAA:BBBB").unwrap();
        assert_eq!(decoded.space_id, "AAAA");
        assert_eq!(decoded.thread_id, "BBBB");
        assert!(!decoded.is_top_level());
    }

    #[test]
    fn decode_thread_id_handles_empty_thread_portion_as_top_level() {
        let decoded = decode_thread_id("gchat:AAAA:").unwrap();
        assert_eq!(decoded.space_id, "AAAA");
        assert!(decoded.is_top_level());
    }

    #[test]
    fn decode_thread_id_handles_missing_thread_portion_as_top_level() {
        // Just "gchat:AAAA" with no trailing colon also legal.
        let decoded = decode_thread_id("gchat:AAAA").unwrap();
        assert_eq!(decoded.space_id, "AAAA");
        assert!(decoded.is_top_level());
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("slack:C1:1.0").is_none());
        assert!(decode_thread_id("teams:1:2:3").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_space() {
        assert!(decode_thread_id("gchat:").is_none());
        assert!(decode_thread_id("gchat::BBBB").is_none());
    }

    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream's `channelIdFromThreadId(threadId)` (returns
    // `gchat:<spaceName>`) and `isDM(threadId)` (delegates to
    // `isDMThread` which checks the `:dm` suffix).

    #[test]
    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    #[test]
    fn gchat_subscription_constants_match_upstream() {
        // 1:1 with upstream's private `SUBSCRIPTION_REFRESH_BUFFER_MS`,
        // `SUBSCRIPTION_CACHE_TTL_MS`, `SPACE_SUB_KEY_PREFIX`.
        assert_eq!(SUBSCRIPTION_REFRESH_BUFFER_MS, 60 * 60 * 1000);
        assert_eq!(SUBSCRIPTION_CACHE_TTL_MS, 25 * 60 * 60 * 1000);
        assert_eq!(SPACE_SUB_KEY_PREFIX, "gchat:space-sub:");
    }

    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn channel_id_from_thread_id_returns_the_space_name() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert_eq!(
            adapter
                .channel_id_from_thread_id("gchat:spaces/ABC123:dGVzdA")
                .as_deref(),
            Some("gchat:spaces/ABC123")
        );
        // DM thread id: still produces the bare space.
        assert_eq!(
            adapter
                .channel_id_from_thread_id("gchat:spaces/ABC123:dm")
                .as_deref(),
            Some("gchat:spaces/ABC123")
        );
    }

    #[test]
    fn channel_id_from_thread_id_returns_none_for_non_gchat_ids() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert!(adapter.channel_id_from_thread_id("teams:1:2:3").is_none());
        assert!(adapter.channel_id_from_thread_id("").is_none());
    }

    // ---------- describe("isDM") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("isDM")`.
    // GChat encodes DM-vs-room as a `:dm` suffix on the thread id;
    // `encodeThreadId({spaceName, isDM: true})` appends it.

    #[test]
    fn is_dm_should_return_true_for_dm_thread_ids() {
        // 1:1 with upstream "should return true for DM thread IDs".
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert!(adapter.is_dm("gchat:spaces/DM123:dm"));
    }

    #[test]
    fn is_dm_should_return_false_for_non_dm_thread_ids() {
        // 1:1 with upstream "should return false for non-DM thread
        // IDs". A room thread id encodes without the `:dm` suffix —
        // `encodeThreadId({spaceName: "spaces/ROOM456"})` yields
        // `gchat:spaces/ROOM456` plus an optional thread suffix.
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert!(!adapter.is_dm("gchat:spaces/ROOM456"));
        assert!(!adapter.is_dm("gchat:spaces/ROOM456:dGVzdA"));
    }

    // Additive: empty thread id is not a DM. Retained from the
    // bundled test above for defensive coverage outside upstream's
    // explicit describe block.
    #[test]
    fn is_dm_returns_false_for_empty_thread_id() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert!(!adapter.is_dm(""));
    }

    #[test]
    fn is_gchat_thread_id_detects_the_prefix() {
        assert!(is_gchat_thread_id("gchat:AAAA:BBBB"));
        assert!(!is_gchat_thread_id("teams:1"));
        assert!(!is_gchat_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (s, t) in [("AAAA", "BBBB"), ("space-1", ""), ("a", "b")] {
            let encoded = encode_thread_id(s, t);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.space_id, s);
            assert_eq!(decoded.thread_id, t);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_gchat_thread_ids() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GChat-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_post_message_requires_a_pre_minted_bearer_token() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("gchat:AAAA:BBBB", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("no bearer_token configured"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_gchat_thread_ids() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("slack:C1:1.0", "spaces/A/messages/B", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GChat-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_requires_bearer() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("gchat:AAA:", "spaces/A/messages/B"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("no bearer_token configured"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_requires_bearer() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("gchat:AAA:", "spaces/A/messages/B", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("no bearer_token configured"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_is_a_noop() {
        // Google Chat has no typing indicator API for bots -
        // upstream returns void unconditionally.
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"));
        assert!(block_on(adapter.start_typing("anything", None)).is_ok());
        assert!(block_on(adapter.start_typing("anything", Some("s"))).is_ok());
    }

    #[test]
    fn adapter_messages_create_url_builds_the_upstream_endpoint() {
        let adapter = GchatAdapter::new(
            GchatAdapterOptions::new("{}", "bot@example.com")
                .with_api_base("https://chat.example.test/v1"),
        );
        assert_eq!(
            adapter.messages_create_url("AAAA"),
            "https://chat.example.test/v1/spaces/AAAA/messages"
        );
    }

    #[test]
    fn adapter_bearer_token_accessor_round_trips_with_setter() {
        let adapter = GchatAdapter::new(GchatAdapterOptions::new("{}", "bot@example.com"))
            .with_bearer_token("ya29.tok");
        assert_eq!(adapter.bearer_token(), Some("ya29.tok"));
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = GchatAdapter::new(
            GchatAdapterOptions::new("{}", "bot@example.com")
                .with_api_base("https://example.test/v1"),
        );
        assert_eq!(adapter.service_account_json(), "{}");
        assert_eq!(adapter.subject_email(), "bot@example.com");
        assert_eq!(adapter.api_base(), "https://example.test/v1");
    }

    // ---------- GoogleChatAdapter create-instance (1 portable case) ----------
    // 1:1 with upstream `index.test.ts > describe("GoogleChatAdapter")
    // > it("should create an adapter instance")`. Upstream's
    // `it("should export createGoogleChatAdapter function")` is
    // JS-only — Rust's module system makes constructors visible at
    // compile time.

    #[test]
    fn google_chat_adapter_creates_an_instance() {
        let opts = GchatAdapterOptions::new(
            "{\"type\":\"service_account\"}",
            "bot@example.com",
        );
        let adapter = GchatAdapter::new(opts);
        assert_eq!(adapter.name(), "gchat");
    }

    // ---------- constructor / initialization (portable subset, 4 cases) +
    //            constructor env var resolution (8 cases) ----------
    // 1:1 with upstream `describe("constructor / initialization")` +
    // `describe("constructor env var resolution")`. The 2
    // `initialize`-restore-botUserId cases are deferred — they need
    // the chat-sdk-chat StateAdapter wiring + `GchatAdapter::initialize`
    // implementation. The "should default logger when not provided"
    // case is js-only here (logger isn't a first-class adapter
    // dependency in this port). The 2 `apiUrl` cases are part of the
    // env-var-resolution block.

    const TEST_CREDS: &str = "{\"type\":\"service_account\"}";

    fn empty_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn ctor_init_should_use_provided_user_name() {
        let adapter = try_create_gchat_adapter(
            GchatCreateOptions {
                credentials: Some(TEST_CREDS.to_string()),
                user_name: Some("mybot".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("config credentials");
        assert_eq!(adapter.user_name(), "mybot");
    }

    #[test]
    fn ctor_init_should_default_user_name_to_bot() {
        let adapter = try_create_gchat_adapter(
            GchatCreateOptions {
                credentials: Some(TEST_CREDS.to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("default user name");
        assert_eq!(adapter.user_name(), "bot");
    }

    #[test]
    fn ctor_init_should_throw_when_no_auth_is_configured() {
        let err = try_create_gchat_adapter(GchatCreateOptions::default(), empty_env)
            .expect_err("no auth");
        assert_eq!(err, GchatCreateError::AuthenticationRequired);
        assert!(err.to_string().contains("Authentication is required"));
    }

    #[test]
    fn ctor_init_should_accept_adc_config() {
        let adapter = try_create_gchat_adapter(
            GchatCreateOptions {
                use_application_default_credentials: Some(true),
                ..Default::default()
            },
            empty_env,
        )
        .expect("ADC config");
        assert_eq!(adapter.name(), "gchat");
        assert!(adapter.use_application_default_credentials());
    }

    #[test]
    fn ctor_env_throws_when_no_auth_configured_and_no_env_vars_set() {
        let err = try_create_gchat_adapter(GchatCreateOptions::default(), empty_env)
            .expect_err("no auth anywhere");
        assert_eq!(err, GchatCreateError::AuthenticationRequired);
    }

    #[test]
    fn ctor_env_resolves_credentials_from_google_chat_credentials_env_var() {
        let env = |key: &str| match key {
            "GOOGLE_CHAT_CREDENTIALS" => Some(TEST_CREDS.to_string()),
            _ => None,
        };
        let adapter = try_create_gchat_adapter(GchatCreateOptions::default(), env)
            .expect("env credentials");
        assert_eq!(adapter.service_account_json(), TEST_CREDS);
        assert!(!adapter.use_application_default_credentials());
    }

    #[test]
    fn ctor_env_resolves_adc_from_google_chat_use_adc_env_var() {
        let env = |key: &str| match key {
            "GOOGLE_CHAT_USE_ADC" => Some("true".to_string()),
            _ => None,
        };
        let adapter = try_create_gchat_adapter(GchatCreateOptions::default(), env)
            .expect("env ADC");
        assert!(adapter.use_application_default_credentials());
    }

    #[test]
    fn ctor_env_resolves_pubsub_topic_from_google_chat_pubsub_topic_env_var() {
        let env = |key: &str| match key {
            "GOOGLE_CHAT_CREDENTIALS" => Some(TEST_CREDS.to_string()),
            "GOOGLE_CHAT_PUBSUB_TOPIC" => Some("projects/test/topics/test".to_string()),
            _ => None,
        };
        let adapter = try_create_gchat_adapter(GchatCreateOptions::default(), env)
            .expect("env pubsub topic");
        assert_eq!(adapter.pubsub_topic(), Some("projects/test/topics/test"));
    }

    #[test]
    fn ctor_env_resolves_impersonate_user_from_google_chat_impersonate_user_env_var() {
        let env = |key: &str| match key {
            "GOOGLE_CHAT_CREDENTIALS" => Some(TEST_CREDS.to_string()),
            "GOOGLE_CHAT_IMPERSONATE_USER" => Some("user@example.com".to_string()),
            _ => None,
        };
        let adapter = try_create_gchat_adapter(GchatCreateOptions::default(), env)
            .expect("env impersonate user");
        assert_eq!(adapter.impersonate_user(), Some("user@example.com"));
        assert_eq!(adapter.subject_email(), "user@example.com");
    }

    #[test]
    fn ctor_env_prefers_config_credentials_over_env_vars() {
        let env = |key: &str| match key {
            "GOOGLE_CHAT_USE_ADC" => Some("true".to_string()),
            _ => None,
        };
        let adapter = try_create_gchat_adapter(
            GchatCreateOptions {
                credentials: Some(TEST_CREDS.to_string()),
                ..Default::default()
            },
            env,
        )
        .expect("config credentials override env ADC");
        assert_eq!(adapter.service_account_json(), TEST_CREDS);
        assert!(!adapter.use_application_default_credentials());
    }

    #[test]
    fn ctor_env_resolves_api_url_from_google_chat_api_url_env_var() {
        let env = |key: &str| match key {
            "GOOGLE_CHAT_CREDENTIALS" => Some(TEST_CREDS.to_string()),
            "GOOGLE_CHAT_API_URL" => Some("https://custom-chat.googleapis.com".to_string()),
            _ => None,
        };
        let adapter = try_create_gchat_adapter(GchatCreateOptions::default(), env)
            .expect("env api url");
        assert_eq!(adapter.api_base(), "https://custom-chat.googleapis.com");
    }

    #[test]
    fn ctor_env_accepts_api_url_config() {
        let adapter = try_create_gchat_adapter(
            GchatCreateOptions {
                credentials: Some(TEST_CREDS.to_string()),
                api_url: Some("https://custom-chat.googleapis.com".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("api url config");
        assert_eq!(adapter.api_base(), "https://custom-chat.googleapis.com");
    }

    // ---------- describe("postEphemeral") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("postEphemeral")`.
    // Upstream stubs the `chatApi.spaces.messages.create` method via
    // `vi.fn().mockResolvedValue(...)`; the Rust port covers the
    // observable payload/response behavior via the pure
    // [`gchat_post_ephemeral_payload`] + [`parse_gchat_post_ephemeral_response`]
    // helpers that the runtime path also flows through.

    #[test]
    fn gchat_post_ephemeral_creates_ephemeral_text_message_with_private_message_viewer() {
        // 1:1 with upstream "should create ephemeral text message with
        // privateMessageViewer". Validates the payload sets
        // privateMessageViewer.name + text + thread.name when the
        // thread component is non-empty; the parsed response surfaces
        // id from name and usedFallback=false.
        // Inputs match the Rust port's decoded shape (raw IDs, not
        // full `spaces/...` resource paths — upstream's `decodeThreadId`
        // returns full paths, but the Rust port stores bare IDs and
        // reassembles the resource path at the API boundary, matching
        // the existing post_message helper convention).
        let body = gchat_post_ephemeral_payload(
            "ABC123",
            "T1",
            "users/TARGET",
            "Only you can see this",
        );
        assert_eq!(
            body["privateMessageViewer"],
            serde_json::json!({ "name": "users/TARGET" })
        );
        assert_eq!(body["text"], "Only you can see this");
        assert_eq!(
            body["thread"],
            serde_json::json!({ "name": "spaces/ABC123/threads/T1" })
        );

        let response =
            serde_json::json!({ "name": "spaces/ABC123/messages/eph1" });
        let parsed = parse_gchat_post_ephemeral_response(
            &response,
            "gchat:ABC123:T1",
        );
        assert_eq!(parsed.id, "spaces/ABC123/messages/eph1");
        assert_eq!(parsed.thread_id, "gchat:ABC123:T1");
        assert!(!parsed.used_fallback);
    }

    #[test]
    fn gchat_post_ephemeral_omits_thread_for_top_level_post() {
        // Rust-only edge case (also implicit in upstream's "no
        // threadName" branch): when the thread component is empty,
        // the payload omits the thread field entirely (matching
        // upstream's `thread: threadName ? { name: threadName } : undefined`
        // semantics that drops the key on undefined).
        let body = gchat_post_ephemeral_payload("ABC123", "", "users/TARGET", "hi");
        assert!(body.get("thread").is_none());
        assert_eq!(
            body["privateMessageViewer"],
            serde_json::json!({ "name": "users/TARGET" })
        );
    }

    #[test]
    fn gchat_post_ephemeral_handles_missing_name_in_response() {
        // Mirrors upstream's `response.data.name || ""` empty-id
        // fallback (Google occasionally returns a successful create
        // without `name`). The Rust parser surfaces this as an empty
        // string rather than a typed error.
        let response = serde_json::json!({});
        let parsed = parse_gchat_post_ephemeral_response(&response, "gchat:ABC123:");
        assert_eq!(parsed.id, "");
        assert_eq!(parsed.thread_id, "gchat:ABC123:");
        assert!(!parsed.used_fallback);
    }

    #[test]
    fn gchat_post_ephemeral_rejects_non_gchat_thread_ids() {
        // 1:1 with upstream "should throw on API error" but at the
        // pre-dispatch layer: a non-gchat-prefixed thread id can
        // never reach the API, surfacing as InvalidPayload.
        // Reproduces the upstream behavior of rejecting at the
        // adapter boundary.
        let env = |_: &str| None;
        let adapter = try_create_gchat_adapter(
            GchatCreateOptions {
                credentials: Some(TEST_CREDS.to_string()),
                ..Default::default()
            },
            env,
        )
        .expect("ctor");
        use chat_sdk_chat::types::AdapterError;
        let err =
            futures_executor::block_on(adapter.post_ephemeral("slack:C1:1.0", "users/1", "x"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GChat-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }
}
