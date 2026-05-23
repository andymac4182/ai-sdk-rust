//! Google Chat adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-gchat/src/index.ts`.
//!
//! Google Chat models conversations as **spaces** (Google's term for
//! a channel) with optional **threads** inside them. The thread id
//! encoding is `gchat:<space_id>:<thread_id>` — when posting a new
//! top-level message, `thread_id` is the empty string after the
//! colon. Space DM mode uses the same encoding with a 1:1 space.

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
}

impl GchatAdapterOptions {
    /// Construct options. API base URL defaults to
    /// [`DEFAULT_API_BASE`].
    pub fn new(service_account_json: impl Into<String>, subject_email: impl Into<String>) -> Self {
        Self {
            service_account_json: service_account_json.into(),
            subject_email: subject_email.into(),
            api_base: None,
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

    /// Build the Google Chat `messages.create` URL. 1:1 with
    /// upstream's inline `${apiBase}/spaces/${spaceId}/messages`
    /// template.
    fn messages_create_url(&self, space_id: &str) -> String {
        format!("{}/spaces/{space_id}/messages", self.api_base())
    }
}

#[async_trait]
impl Adapter for GchatAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
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
}
