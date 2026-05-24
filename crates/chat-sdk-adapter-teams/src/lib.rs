//! Microsoft Teams adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-teams/src/index.ts`.
//!
//! Teams uses the Bot Framework conversation model. The thread id
//! encoding is `teams:<conversation_id>:<message_id>` — when posting
//! a new top-level reply, `message_id` is the root activity id.

pub mod cards;
pub mod errors;
pub mod markdown;
pub mod thread_id;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "teams";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "teams:";

/// Default Bot Framework / Teams API base URL.
pub const DEFAULT_API_BASE: &str = "https://smba.trafficmanager.net";

/// TTL the adapter caches per-conversation metadata (serviceUrl,
/// tenantId, etc.) under. 1:1 with upstream's private
/// `CACHE_TTL_MS = 30 * 24 * 60 * 60 * 1000` (30 days).
pub const CACHE_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Maximum time the adapter waits for a handler to call
/// `chat.openModal()` after Teams sends an `invoke` activity
/// requesting a task module. 1:1 with upstream's private
/// `DEFAULT_DIALOG_OPEN_TIMEOUT_MS = 5000` (5 s).
pub const DEFAULT_DIALOG_OPEN_TIMEOUT_MS: u64 = 5000;

/// Options for [`TeamsAdapter::new`].
#[derive(Debug, Clone)]
pub struct TeamsAdapterOptions {
    /// Bot application id (Microsoft App ID).
    pub app_id: String,
    /// Bot application password / client secret.
    pub app_password: String,
    /// Tenant id (Azure AD tenant). Required for multi-tenant bots
    /// to mint the right access token.
    pub tenant_id: String,
    /// Optional API base URL override.
    pub api_base: Option<String>,
}

impl TeamsAdapterOptions {
    /// Construct options.
    pub fn new(
        app_id: impl Into<String>,
        app_password: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_password: app_password.into(),
            tenant_id: tenant_id.into(),
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

/// Microsoft Teams adapter.
#[derive(Debug, Clone)]
pub struct TeamsAdapter {
    options: TeamsAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
    /// Pre-minted bearer token. Bot Framework's OAuth2 flow mints
    /// short-lived tokens (`POST login.microsoftonline.com/.../oauth2/v2.0/token`
    /// with the `client_credentials` grant + `https://api.botframework.com/.default`
    /// scope) which adopters refresh out-of-band. Until a token-cache
    /// helper lands in chat-sdk-adapter-shared, the adapter accepts
    /// a pre-minted token via `with_bearer_token` and falls back to
    /// `AdapterError::InvalidPayload` when none is configured.
    bearer_token: Option<String>,
}

impl TeamsAdapter {
    /// 1:1 port of upstream
    /// `new TeamsAdapter({ appId, appPassword, tenantId, apiBase? })`.
    pub fn new(options: TeamsAdapterOptions) -> Self {
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

    /// Provide a pre-minted Bot Framework bearer token. Required
    /// for `post_message`; adopters mint it out-of-band against
    /// `login.microsoftonline.com/<tenant>/oauth2/v2.0/token` and
    /// refresh as needed (Bot Framework tokens are ~1 hour TTL).
    /// A token-cache helper for the minting flow will land in
    /// `chat_sdk_adapter_shared` in a future slice.
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Read the bot app id.
    pub fn app_id(&self) -> &str {
        &self.options.app_id
    }

    /// Read the bot app password.
    pub fn app_password(&self) -> &str {
        &self.options.app_password
    }

    /// Read the tenant id.
    pub fn tenant_id(&self) -> &str {
        &self.options.tenant_id
    }

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Read the currently-configured bearer token, if any.
    pub fn bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }

    /// Build the Bot Framework activity-create URL. 1:1 with
    /// upstream's inline `${apiBase}/v3/conversations/${conversationId}/activities`
    /// template.
    fn activities_url(&self, conversation_id: &str) -> String {
        format!(
            "{}/v3/conversations/{conversation_id}/activities",
            self.api_base()
        )
    }

    /// Build the URL for a specific activity:
    /// `<api_base>/v3/conversations/<conversation_id>/activities/<activity_id>`.
    fn activity_url(&self, conversation_id: &str, activity_id: &str) -> String {
        format!(
            "{}/v3/conversations/{conversation_id}/activities/{activity_id}",
            self.api_base()
        )
    }

    /// Render formatted content to Teams-flavored markdown. 1:1
    /// with upstream `adapter.renderFormatted(content)` which
    /// delegates to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::TeamsFormatConverter::new().from_ast(ast)
    }
}

#[async_trait]
impl Adapter for TeamsAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Derive the channel-level thread id from a Teams thread id.
    /// 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`.
    /// Delegates to [`crate::thread_id::channel_id_from_thread_id`]
    /// which strips any `;messageid=…` suffix from the decoded
    /// `conversation_id` and re-encodes.
    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        Some(crate::thread_id::channel_id_from_thread_id(thread_id))
    }

    /// Post a text message via the Bot Framework `activities` API.
    /// 1:1 with upstream's `adapter.postMessage`:
    ///
    /// - Decodes `teams:<conversation_id>:<message_id>` (we use
    ///   `conversation_id`; `message_id` becomes `replyToId` only
    ///   when the bot supports threading, which is a follow-up).
    /// - POSTs the activity `{type: "message", text}` to
    ///   `<api_base>/v3/conversations/<conversation_id>/activities`.
    /// - Auth via `Authorization: Bearer <bearer_token>` (the
    ///   pre-minted token from `with_bearer_token`; OAuth2
    ///   token-mint helper deferred).
    /// - Returns the Bot Framework response's `id` field (the
    ///   activity id).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Teams-encoded"))
        })?;

        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload(
                "TeamsAdapter has no bearer_token configured; call \
                 with_bearer_token() with a pre-minted Bot Framework \
                 OAuth2 access token (see TeamsAdapter docs)"
                    .to_string(),
            )
        })?;

        let url = self.activities_url(&decoded.conversation_id);
        let body = serde_json::json!({
            "type": "message",
            "text": text,
        });

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
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("Bot Framework activity-create failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        json["id"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload("Bot Framework activity response missing id".to_string())
        })
    }

    /// Edit a Teams activity via Bot Framework's update endpoint.
    /// 1:1 with the text-only path of upstream `adapter.editMessage`
    /// (Adaptive Cards branch deferred): PUT
    /// `<api_base>/v3/conversations/<conversation_id>/activities/<activity_id>`
    /// with `{type: "message", text}`. Returns the (unchanged)
    /// activity id.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Teams-encoded"))
        })?;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload("TeamsAdapter has no bearer_token configured".to_string())
        })?;

        let url = self.activity_url(&decoded.conversation_id, message_id);
        let body = serde_json::json!({
            "type": "message",
            "text": text,
        });

        let response = self
            .http
            .put(&url)
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
        Ok(message_id.to_string())
    }

    /// Delete a Teams activity via Bot Framework's delete endpoint.
    /// 1:1 with upstream's `adapter.deleteMessage`. DELETE
    /// `<api_base>/v3/conversations/<conversation_id>/activities/<activity_id>`.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Teams-encoded"))
        })?;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload("TeamsAdapter has no bearer_token configured".to_string())
        })?;

        let url = self.activity_url(&decoded.conversation_id, message_id);
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

    /// Teams Bot Framework does not yet expose reactions through
    /// the SDK. 1:1 with upstream's
    /// `throw NotImplementedError("addReaction is not yet supported
    /// by the Teams SDK", "addReaction")`.
    async fn add_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "addReaction is not yet supported by the Teams SDK".to_string(),
        ))
    }

    /// Not yet supported by the underlying Teams SDK. 1:1 with
    /// upstream's `throw NotImplementedError("removeReaction is
    /// not yet supported by the Teams SDK", "removeReaction")` —
    /// symmetric with the unsupported `add_reaction`.
    async fn remove_reaction(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "removeReaction is not yet supported by the Teams SDK".to_string(),
        ))
    }

    /// Send a Teams typing indicator. 1:1 with upstream's
    /// `adapter.startTyping`: POSTs `{type: "typing"}` as an
    /// activity to the conversation.
    async fn start_typing(
        &self,
        thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Teams-encoded"))
        })?;
        let bearer = self.bearer_token.as_deref().ok_or_else(|| {
            AdapterError::InvalidPayload("TeamsAdapter has no bearer_token configured".to_string())
        })?;

        let url = self.activities_url(&decoded.conversation_id);
        let body = serde_json::json!({ "type": "typing" });

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
}

/// Encode a Teams thread id. 1:1 with upstream's inline format:
/// `teams:<conversation_id>:<message_id>`. The `conversation_id`
/// is the Bot Framework conversation id (may itself contain
/// semicolons for channel/tenant; we treat it opaquely up to the
/// last colon).
pub fn encode_thread_id(conversation_id: &str, message_id: &str) -> String {
    format!("{THREAD_ID_PREFIX}{conversation_id}:{message_id}")
}

/// Components of a decoded Teams thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedTeamsThreadId {
    /// Bot Framework conversation id (channel + tenant suffix).
    pub conversation_id: String,
    /// Activity / message id.
    pub message_id: String,
}

/// Decode a Teams thread id. The conversation id can itself contain
/// colons (Teams encodes channel/tenant in it), so we split on the
/// LAST colon to keep that opaque structure intact.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedTeamsThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let split = suffix.rfind(':')?;
    let conversation_id = &suffix[..split];
    let message_id = &suffix[split + 1..];
    if conversation_id.is_empty() || message_id.is_empty() {
        return None;
    }
    Some(DecodedTeamsThreadId {
        conversation_id: conversation_id.to_string(),
        message_id: message_id.to_string(),
    })
}

/// Predicate: does this thread id belong to the Teams adapter?
pub fn is_teams_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    //! ---------- upstream js-only-documented cases (4) ----------
    //!
    //! Per the slice-380 type-system-impossible pattern, 4 upstream
    //! `index.test.ts` cases are enumerated as js-only-documented:
    //!
    //! - `describe("subclass extensibility") > should expose protected
    //!   members and methods to subclasses`: TypeScript-class-`protected`
    //!   access modifier check. Rust uses `pub(crate)` visibility +
    //!   trait composition rather than class inheritance — the
    //!   subclass-protected-leak test is unrepresentable by construction.
    //!
    //! - `describe("ESM compatibility") > all subpath imports resolve in
    //!   Node.js ESM (no bare directory imports)`: spawns a real
    //!   `node --input-type=module` subprocess and checks that every
    //!   non-relative `from "<pkg>"` import in `index.ts` resolves
    //!   under Node.js ESM rules (no bare directory imports without
    //!   an explicit `/index.js` suffix). The Rust port has no
    //!   equivalent constraint — the module system is statically
    //!   resolved at compile time via Cargo + `mod` declarations.
    //!   Adapter-teams is the only upstream adapter that has this
    //!   test (slice 414 audited cross-package).
    //!
    //! - `describe("constructor env var resolution") > should default
    //!   logger when not provided` — asserts the constructor falls
    //!   back to a default `Logger` instance when none is supplied.
    //!   Rust adapters do not take a `Logger` as a first-class
    //!   adapter dependency (logging is plumbed via the `log` crate's
    //!   static dispatch elsewhere); the constructor-default-logger
    //!   fallback shape is moot.
    //!
    //! - `describe("TeamsAdapter") > should export createTeamsAdapter
    //!   function` — asserts `typeof createTeamsAdapter === "function"`.
    //!   Rust's module system makes the `pub fn new` constructor
    //!   visible at compile time; missing exports become compilation
    //!   errors, not runtime assertion failures. (Slice 458 formalizes
    //!   what was previously an inline note at the
    //!   `teams_adapter_creates_an_instance_with_app_credentials`
    //!   test site.)
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_teams() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("app", "pwd", "tenant"));
        assert_eq!(adapter.name(), "teams");
    }

    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    #[test]
    fn teams_cache_and_dialog_timeout_consts_match_upstream() {
        // 1:1 with upstream's private `CACHE_TTL_MS = 30 * 24 * 60 *
        // 60 * 1000` and `DEFAULT_DIALOG_OPEN_TIMEOUT_MS = 5000`.
        assert_eq!(CACHE_TTL_MS, 30 * 24 * 60 * 60 * 1000);
        assert_eq!(DEFAULT_DIALOG_OPEN_TIMEOUT_MS, 5000);
    }

    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("app", "pwd", "tenant"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn options_new_stores_credentials_and_defaults_api_base() {
        let opts = TeamsAdapterOptions::new("app", "pwd", "tenant");
        assert_eq!(opts.app_id, "app");
        assert_eq!(opts.app_password, "pwd");
        assert_eq!(opts.tenant_id, "tenant");
        assert_eq!(opts.effective_api_base(), DEFAULT_API_BASE);
    }

    #[test]
    fn options_with_api_base_overrides_the_default() {
        let opts =
            TeamsAdapterOptions::new("a", "p", "t").with_api_base("https://teams.example.test/v3");
        assert_eq!(opts.effective_api_base(), "https://teams.example.test/v3");
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(encode_thread_id("CONV", "MSG"), "teams:CONV:MSG");
    }

    #[test]
    fn decode_thread_id_parses_conversation_and_message() {
        let decoded = decode_thread_id("teams:CONV:MSG").unwrap();
        assert_eq!(decoded.conversation_id, "CONV");
        assert_eq!(decoded.message_id, "MSG");
    }

    #[test]
    fn decode_thread_id_keeps_inner_colons_in_conversation_id() {
        // Bot Framework conversation ids encode channel + tenant
        // with their own colons. The last colon separates message id.
        let decoded = decode_thread_id("teams:19:abc;tenant=def:01HZZZ").unwrap();
        assert_eq!(decoded.conversation_id, "19:abc;tenant=def");
        assert_eq!(decoded.message_id, "01HZZZ");
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("slack:C1:1.0").is_none());
        assert!(decode_thread_id("gchat:AAA:BBB").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_components() {
        assert!(decode_thread_id("teams:onlyone").is_none());
        assert!(decode_thread_id("teams::MSG").is_none());
        assert!(decode_thread_id("teams:CONV:").is_none());
    }

    #[test]
    fn is_teams_thread_id_detects_the_prefix() {
        assert!(is_teams_thread_id("teams:CONV:MSG"));
        assert!(!is_teams_thread_id("gchat:AAA:BBB"));
        assert!(!is_teams_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (c, m) in [("CONV", "MSG"), ("19:abc;tenant=def", "01HZZZ"), ("a", "b")] {
            let encoded = encode_thread_id(c, m);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.conversation_id, c);
            assert_eq!(decoded.message_id, m);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_teams_thread_ids() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Teams-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_post_message_requires_a_pre_minted_bearer_token() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("teams:CONV:MSG", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("no bearer_token configured"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_teams_thread_ids() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("slack:C1:1.0", "msg", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Teams-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_requires_a_bearer() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("teams:CONV:MSG", "msg"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("no bearer_token configured"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_is_not_implemented() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("teams:CONV:MSG", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not yet supported by the Teams SDK"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_remove_reaction_is_not_implemented() {
        // 1:1 with upstream's `throw NotImplementedError(...)` in
        // `adapter.removeReaction`. Upstream has no removeReaction
        // describe block — the symmetric not-implemented surface
        // matches the unsupported add_reaction shape.
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.remove_reaction("teams:CONV:MSG", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not yet supported by the Teams SDK"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_rejects_non_teams_thread_ids() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.start_typing("slack:C1:1.0", None));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Teams-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_activity_url_builds_the_upstream_endpoint() {
        let adapter = TeamsAdapter::new(
            TeamsAdapterOptions::new("a", "p", "t").with_api_base("https://example.test"),
        );
        assert_eq!(
            adapter.activity_url("CONV", "ACT"),
            "https://example.test/v3/conversations/CONV/activities/ACT"
        );
    }

    #[test]
    fn adapter_activities_url_builds_the_upstream_endpoint() {
        let adapter = TeamsAdapter::new(
            TeamsAdapterOptions::new("a", "p", "t").with_api_base("https://example.test/v3"),
        );
        assert_eq!(
            adapter.activities_url("19:abc;tenant=def"),
            "https://example.test/v3/v3/conversations/19:abc;tenant=def/activities"
        );
    }

    #[test]
    fn adapter_bearer_token_accessor_round_trips_with_setter() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"))
            .with_bearer_token("ya29.tok");
        assert_eq!(adapter.bearer_token(), Some("ya29.tok"));
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = TeamsAdapter::new(
            TeamsAdapterOptions::new("app-id", "app-pwd", "tenant-id")
                .with_api_base("https://example.test/v3"),
        );
        assert_eq!(adapter.app_id(), "app-id");
        assert_eq!(adapter.app_password(), "app-pwd");
        assert_eq!(adapter.tenant_id(), "tenant-id");
        assert_eq!(adapter.api_base(), "https://example.test/v3");
    }

    // ---------- createTeamsAdapter describe block (1 upstream case) ----------
    // 1:1 with upstream `index.test.ts > describe("TeamsAdapter") >
    // it("should create an adapter instance")`. Upstream's
    // `it("should export createTeamsAdapter function")` is JS-only:
    // Rust's module system makes the `pub fn new` constructor
    // visible at compile time so a runtime function-exists check
    // doesn't apply.

    #[test]
    fn teams_adapter_creates_an_instance_with_app_credentials() {
        let opts = TeamsAdapterOptions::new("test-app-id", "test-password", "");
        let adapter = TeamsAdapter::new(opts);
        assert_eq!(adapter.name(), "teams");
    }
}
