//! Slack adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-slack/src/index.ts`.
//!
//! Slack threads are addressed by `<channel_id>:<thread_ts>` — the
//! channel ID (e.g. `C0123ABCD`) plus the parent message's
//! timestamp (e.g. `1234567890.123456`). The Rust thread id encoding
//! is `slack:<channel_id>:<thread_ts>` (the same wire shape upstream
//! uses). For top-level messages, `thread_ts` is the message's own
//! timestamp, so the encoding is symmetric.

pub mod api;
pub mod cards;
pub mod crypto;
pub mod format;
pub mod markdown;
pub mod modals;
pub mod webhook;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "slack";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "slack:";

/// Default Slack Web API base URL.
pub const DEFAULT_API_BASE: &str = "https://slack.com/api";

/// Timeout the adapter waits before failing an
/// `options_load` external-select callback. Slack expects a
/// response within ~3 s; the adapter rounds down to 2500 ms. 1:1
/// with upstream's private `OPTIONS_LOAD_TIMEOUT_MS = 2500`.
pub const OPTIONS_LOAD_TIMEOUT_MS: u64 = 2500;

/// Wall-clock budget the adapter spends polling for link-unfurl
/// completion before giving up on the unfurl. 1:1 with upstream's
/// private `UNFURL_WAIT_MS = 2000` (2 s).
pub const UNFURL_WAIT_MS: u64 = 2000;

/// Poll interval the adapter uses while waiting for link-unfurl
/// completion. 1:1 with upstream's private
/// `UNFURL_POLL_MS = 150` (150 ms).
pub const UNFURL_POLL_MS: u64 = 150;

/// 1:1 with upstream's default `userName ?? "bot"` constant.
pub const DEFAULT_USER_NAME: &str = "bot";

/// Options for [`SlackAdapter::new`].
#[derive(Debug, Clone)]
pub struct SlackAdapterOptions {
    /// Bot user OAuth token (`xoxb-...`).
    pub bot_token: String,
    /// Signing secret used to verify webhook requests.
    pub signing_secret: String,
    /// Optional app-level token (`xapp-...`) for Socket Mode.
    pub app_token: Option<String>,
    /// Optional API base URL override.
    pub api_base: Option<String>,
    /// Optional display name (defaults to [`DEFAULT_USER_NAME`]).
    pub user_name: Option<String>,
    /// Slack user id of the bot (`U...`). Used for self-mention
    /// detection. Resolved automatically via `auth.test` upstream
    /// when not provided.
    pub bot_user_id: Option<String>,
}

impl SlackAdapterOptions {
    /// Construct options.
    pub fn new(bot_token: impl Into<String>, signing_secret: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            signing_secret: signing_secret.into(),
            app_token: None,
            api_base: None,
            user_name: None,
            bot_user_id: None,
        }
    }

    /// Attach an app-level token (Socket Mode).
    pub fn with_app_token(mut self, app_token: impl Into<String>) -> Self {
        self.app_token = Some(app_token.into());
        self
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

    /// Effective `userName` with default applied. 1:1 with upstream's
    /// `userName ?? "bot"`.
    pub fn effective_user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(DEFAULT_USER_NAME)
    }
}

/// Slack adapter.
#[derive(Debug, Clone)]
pub struct SlackAdapter {
    options: SlackAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
    /// Slack Connect (external) channel ids the adapter has
    /// observed. 1:1 with upstream's private
    /// `_externalChannels = new Set<string>()`. Populated by
    /// webhook/conversations.info handlers (deferred in the Rust
    /// port); consulted by [`get_channel_visibility`] /
    /// [`is_external_channel`].
    external_channels: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl SlackAdapter {
    /// 1:1 port of upstream
    /// `new SlackAdapter({ botToken, signingSecret, appToken?, apiBase? })`.
    pub fn new(options: SlackAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
            external_channels: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
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

    /// Read the bot OAuth token.
    pub fn bot_token(&self) -> &str {
        &self.options.bot_token
    }

    /// Read the signing secret.
    pub fn signing_secret(&self) -> &str {
        &self.options.signing_secret
    }

    /// Read the app-level token (Socket Mode), if configured.
    pub fn app_token(&self) -> Option<&str> {
        self.options.app_token.as_deref()
    }

    /// 1:1 with upstream `readonly userName: string`. Returns the
    /// configured value or [`DEFAULT_USER_NAME`].
    pub fn user_name(&self) -> &str {
        self.options.effective_user_name()
    }

    /// 1:1 with upstream `readonly botUserId?: string`. Returns
    /// `None` when not configured at construction (upstream resolves
    /// via `auth.test` on first webhook).
    pub fn bot_user_id(&self) -> Option<&str> {
        self.options.bot_user_id.as_deref()
    }

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Build a URL for a Slack Web API method. 1:1 with upstream's
    /// inline `${apiBase}/${method}` template.
    fn method_url(&self, method: &str) -> String {
        format!("{}/{method}", self.api_base())
    }

    /// Derive channel id from a Slack thread id. 1:1 with upstream
    /// `adapter.channelIdFromThreadId(threadId)` which decodes the
    /// thread and returns `slack:<channel>`. Returns `None` when
    /// `thread_id` isn't a Slack-encoded value.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        // 1:1 with upstream `channelIdFromThreadId(threadId)` which
        // returns `slack:<channel>` for any well-prefixed thread id,
        // including `slack:C456:` (empty threadTs) and bare
        // `slack:C456` (no threadTs). The stricter `decode_thread_id`
        // helper requires a non-empty threadTs (used by post_message
        // etc), so this path parses the suffix directly.
        let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
        let channel_id = suffix.split(':').next()?;
        if channel_id.is_empty() {
            return None;
        }
        Some(format!("{THREAD_ID_PREFIX}{channel_id}"))
    }

    /// Predicate: is the conversation a 1:1 DM? 1:1 with upstream's
    /// `adapter.isDM(threadId)` which decodes and tests
    /// `channel.startsWith("D")` (Slack convention: DM channel ids
    /// start with `D`). Returns `None` when `thread_id` isn't a
    /// Slack-encoded value.
    pub fn is_dm(&self, thread_id: &str) -> Option<bool> {
        let decoded = decode_thread_id(thread_id)?;
        Some(decoded.channel_id.starts_with('D'))
    }

    /// Render formatted content to Slack-flavored mrkdwn. 1:1 with
    /// upstream `adapter.renderFormatted(content)` which delegates
    /// to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::SlackFormatConverter::new().from_ast(ast)
    }

    /// Mark a Slack channel id as a Slack Connect (external)
    /// channel. 1:1 with upstream's private `_externalChannels.add`
    /// callsites in webhook + conversations.info handlers. Called
    /// by adapter HTTP code; tests can call it directly to drive
    /// [`get_channel_visibility`].
    pub fn mark_external_channel(&self, channel_id: &str) {
        if let Ok(mut set) = self.external_channels.lock() {
            set.insert(channel_id.to_string());
        }
    }

    /// Whether the given channel id was marked as a Slack Connect
    /// channel via [`mark_external_channel`]. Used by
    /// [`get_channel_visibility`]; exposed for callers that want to
    /// branch on Connect status without rebuilding the visibility
    /// enum.
    pub fn is_external_channel(&self, channel_id: &str) -> bool {
        self.external_channels
            .lock()
            .map(|set| set.contains(channel_id))
            .unwrap_or(false)
    }

    /// Get the visibility scope of the channel that contains the
    /// given Slack thread id. 1:1 port of upstream
    /// `getChannelVisibility(threadId)`:
    ///
    /// - external if [`mark_external_channel`] was previously
    ///   called for this channel (Slack Connect membership).
    /// - private if the channel id starts with `G` (private
    ///   channel) or `D` (DM).
    /// - workspace if the channel id starts with `C` (public
    ///   channel).
    /// - unknown for anything else, or when `thread_id` doesn't
    ///   decode.
    pub fn get_channel_visibility(
        &self,
        thread_id: &str,
    ) -> chat_sdk_chat::types::ChannelVisibility {
        use chat_sdk_chat::types::ChannelVisibility;
        let Some(decoded) = decode_thread_id(thread_id) else {
            return ChannelVisibility::Unknown;
        };
        if self.is_external_channel(&decoded.channel_id) {
            return ChannelVisibility::External;
        }
        match decoded.channel_id.chars().next() {
            Some('G') | Some('D') => ChannelVisibility::Private,
            Some('C') => ChannelVisibility::Workspace,
            _ => ChannelVisibility::Unknown,
        }
    }
}

#[async_trait]
impl Adapter for SlackAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`.
    /// Delegates to the inherent
    /// [`SlackAdapter::channel_id_from_thread_id`].
    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        self.channel_id_from_thread_id(thread_id)
    }

    /// 1:1 with upstream `adapter.isDM(threadId)`. Delegates to the
    /// inherent [`SlackAdapter::is_dm`].
    fn is_dm(&self, thread_id: &str) -> Option<bool> {
        self.is_dm(thread_id)
    }

    /// Post a text message via Slack's `chat.postMessage` Web API.
    /// 1:1 with upstream's `adapter.postMessage`:
    ///
    /// - Decodes `slack:<channel_id>:<thread_ts>`.
    /// - POSTs JSON `{channel, text, thread_ts}` to
    ///   `<api_base>/chat.postMessage` with
    ///   `Authorization: Bearer <bot_token>` and
    ///   `Content-Type: application/json`.
    /// - Slack returns `{ok: bool, ts, channel, error?}`. We
    ///   surface `!ok` as `AdapterError::InvalidPayload` with the
    ///   `error` field (Slack uses a snake_case error code like
    ///   `channel_not_found`).
    /// - Returns the new message's `ts` (Slack's per-channel
    ///   timestamp serves as the message id).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("chat.postMessage");
        let body = serde_json::json!({
            "channel": decoded.channel_id,
            "text": text,
            "thread_ts": decoded.thread_ts,
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }

        // Slack returns 200 even for application-level failures;
        // the `ok` field discriminates.
        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "Slack chat.postMessage: {error_code}"
            )));
        }

        json["ts"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload("Slack chat.postMessage response missing ts".to_string())
        })
    }

    /// Post an ephemeral Slack message via `chat.postEphemeral`.
    /// 1:1 with upstream's text-path `adapter.postEphemeral` (the
    /// card-rendering branch via Block Kit is deferred — needs
    /// `cardToBlockKit` infra). Decodes the thread id, builds the
    /// payload via [`slack_post_ephemeral_payload`] (normalizes
    /// empty `thread_ts` to absent — upstream `threadTs ||
    /// undefined`), POSTs to `chat.postEphemeral`, parses via
    /// [`parse_slack_post_ephemeral_response`] (preserves
    /// upstream's `result.message_ts || ""` empty-id fallback).
    async fn post_ephemeral(
        &self,
        thread_id: &str,
        user_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<chat_sdk_chat::types::EphemeralMessage> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("chat.postEphemeral");
        let body =
            slack_post_ephemeral_payload(&decoded.channel_id, &decoded.thread_ts, user_id, text);

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }
        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "Slack chat.postEphemeral: {error_code}"
            )));
        }

        Ok(parse_slack_post_ephemeral_response(&json, thread_id))
    }

    /// Fetch a Slack channel's name via `conversations.info`. 1:1
    /// with upstream's `adapter.fetchSubject`:
    ///
    /// - Decodes `slack:<channel_id>:<thread_ts>` (we only need
    ///   the channel_id; the thread_ts is ignored here).
    /// - POSTs `{channel: channel_id}` to
    ///   `<api_base>/conversations.info` with bearer auth.
    /// - Slack returns `{ok: bool, channel: {name, ...}, error?}`.
    /// - Returns `Some(channel.name)` for public/private channels
    ///   and `None` for DMs (which have no `name` field —
    ///   Slack returns `{user: <user_id>}` instead).
    async fn fetch_subject(
        &self,
        thread_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<Option<String>> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("conversations.info");
        let body = serde_json::json!({ "channel": decoded.channel_id });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }

        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "Slack conversations.info: {error_code}"
            )));
        }

        // DM channels carry no `name`; everything else does.
        Ok(json["channel"]["name"].as_str().map(str::to_owned))
    }

    /// Edit an existing Slack message via `chat.update`. 1:1 with
    /// upstream's text-path `adapter.editMessage` (the card/ephemeral
    /// branches are deferred). POSTs `{channel, ts: message_id, text}`
    /// to `<api_base>/chat.update` with bearer auth, returns the
    /// updated message `ts`.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("chat.update");
        let body = serde_json::json!({
            "channel": decoded.channel_id,
            "ts": message_id,
            "text": text,
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }

        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "Slack chat.update: {error_code}"
            )));
        }

        json["ts"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload("Slack chat.update response missing ts".to_string())
        })
    }

    /// Delete an existing Slack message via `chat.delete`. 1:1 with
    /// upstream's `adapter.deleteMessage`. POSTs `{channel, ts:
    /// message_id}` to `<api_base>/chat.delete` with bearer auth.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("chat.delete");
        let body = serde_json::json!({
            "channel": decoded.channel_id,
            "ts": message_id,
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }

        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "Slack chat.delete: {error_code}"
            )));
        }

        Ok(())
    }

    /// Add an emoji reaction to a Slack message via `reactions.add`.
    /// 1:1 with upstream's `adapter.addReaction`. POSTs `{channel,
    /// timestamp: message_id, name: emoji}` with bearer auth.
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("reactions.add");
        let body = serde_json::json!({
            "channel": decoded.channel_id,
            "timestamp": message_id,
            "name": emoji,
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }

        // Slack treats `already_reacted` as a benign idempotent
        // outcome upstream — adapter.addReaction swallows it.
        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            if error_code == "already_reacted" {
                return Ok(());
            }
            return Err(AdapterError::InvalidPayload(format!(
                "Slack reactions.add: {error_code}"
            )));
        }

        Ok(())
    }

    /// Remove an emoji reaction from a Slack message via
    /// `reactions.remove`. 1:1 with upstream's
    /// `adapter.removeReaction`. POSTs `{channel, timestamp:
    /// message_id, name: emoji}` with bearer auth.
    async fn remove_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        let url = self.method_url("reactions.remove");
        let body = serde_json::json!({
            "channel": decoded.channel_id,
            "timestamp": message_id,
            "name": emoji,
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
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
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: Slack API request failed"
            )));
        }

        // Slack treats `no_reaction` as a benign idempotent outcome —
        // the reaction wasn't there to begin with. Match upstream's
        // swallow behavior.
        if !json["ok"].as_bool().unwrap_or(false) {
            let error_code = json["error"].as_str().unwrap_or("Slack API call failed");
            if error_code == "no_reaction" {
                return Ok(());
            }
            return Err(AdapterError::InvalidPayload(format!(
                "Slack reactions.remove: {error_code}"
            )));
        }

        Ok(())
    }

    /// Set a Slack AI Assistant "Typing…" status via
    /// `assistant.threads.setStatus`. 1:1 with upstream's
    /// `adapter.startTyping`:
    ///
    /// - Returns Ok(()) silently when the thread has no `thread_ts`
    ///   context (upstream logs "startTyping skipped").
    /// - POSTs `{channel_id, thread_ts, status, loading_messages}`.
    ///   `status` defaults to `"Typing..."`.
    /// - All API failures are swallowed silently (upstream warns
    ///   then proceeds — never throws). InvalidPayload only fires
    ///   for thread-id decode failure.
    async fn start_typing(
        &self,
        thread_id: &str,
        status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Slack-encoded"))
        })?;

        // Top-level (non-threaded) messages have thread_ts == ts
        // for the parent of the same message; the upstream check
        // is purely "no threadTs at all" which our encoding never
        // emits, so we always proceed.

        let url = self.method_url("assistant.threads.setStatus");
        let display_status = status.unwrap_or("Typing...");
        let body = serde_json::json!({
            "channel_id": decoded.channel_id,
            "thread_ts": decoded.thread_ts,
            "status": display_status,
            "loading_messages": [display_status],
        });

        // Swallow all HTTP / API errors — upstream warns and
        // returns void unconditionally.
        let _ = self
            .http
            .post(&url)
            .bearer_auth(self.bot_token())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        Ok(())
    }

    /// Post a structured object (plan, …) via Slack. 1:1 with the
    /// text-only path of upstream `adapter.postObject`:
    ///
    /// - For any `kind` other than `"plan"`, fall back to
    ///   `post_message(thread_id, &format!("[{kind}]"))` (the
    ///   upstream "unsupported kind — post as plain text fallback"
    ///   branch).
    /// - For `kind == "plan"`, parse `data` as a `PlanModel`,
    ///   render the upstream-shape fallback text
    ///   ([`render_plan_fallback_text`]), and post via
    ///   `chat.postMessage`. **Block Kit rendering of plans is
    ///   deferred** to a follow-up slice; the fallback text alone
    ///   matches what upstream sends in the `text` field.
    async fn post_object(
        &self,
        thread_id: &str,
        kind: &str,
        data: serde_json::Value,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        if kind != "plan" {
            return self.post_message(thread_id, &format!("[{kind}]")).await;
        }

        let plan: chat_sdk_chat::plan::PlanModel = serde_json::from_value(data).map_err(|err| {
            AdapterError::InvalidPayload(format!(
                "Slack post_object(plan): data is not a PlanModel: {err}"
            ))
        })?;
        let text = render_plan_fallback_text(&plan);
        self.post_message(thread_id, &text).await
    }
}

/// Render a [`PlanModel`] as plain text matching upstream Slack
/// adapter's `protected renderPlanFallbackText(plan)`:
///
/// ```text
/// {plan.title or "Plan"}
/// - ({task.status}) {task.title}
/// - ...
/// ```
pub fn render_plan_fallback_text(plan: &chat_sdk_chat::plan::PlanModel) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(1 + plan.tasks.len());
    let title = if plan.title.is_empty() {
        "Plan".to_string()
    } else {
        plan.title.clone()
    };
    lines.push(title);
    for task in &plan.tasks {
        lines.push(format!(
            "- ({}) {}",
            plan_task_status_str(task.status),
            task.title
        ));
    }
    lines.join("\n")
}

fn plan_task_status_str(status: chat_sdk_chat::plan::PlanTaskStatus) -> &'static str {
    use chat_sdk_chat::plan::PlanTaskStatus;
    match status {
        PlanTaskStatus::Pending => "pending",
        PlanTaskStatus::InProgress => "in_progress",
        PlanTaskStatus::Complete => "complete",
        PlanTaskStatus::Error => "error",
    }
}

/// Encode a Slack thread id. 1:1 with upstream's inline format:
/// `slack:<channel_id>:<thread_ts>`.
pub fn encode_thread_id(channel_id: &str, thread_ts: &str) -> String {
    format!("{THREAD_ID_PREFIX}{channel_id}:{thread_ts}")
}

/// Components of a decoded Slack thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedSlackThreadId {
    /// Channel id (`C...` / `G...` / `D...`).
    pub channel_id: String,
    /// Parent message timestamp (e.g. `1234567890.123456`).
    pub thread_ts: String,
}

impl DecodedSlackThreadId {
    /// Whether this channel id is a DM. Slack DM channel ids start
    /// with `D`. 1:1 with upstream's inline
    /// `channelId.startsWith("D")` check.
    pub fn is_dm(&self) -> bool {
        self.channel_id.starts_with('D')
    }

    /// Whether this channel id is a private channel (group DM /
    /// multi-party IM / private channel). Slack uses `G` for those.
    pub fn is_group(&self) -> bool {
        self.channel_id.starts_with('G')
    }
}

/// Decode a Slack thread id.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedSlackThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let channel_id = parts.next()?;
    let thread_ts = parts.next()?;
    if channel_id.is_empty() || thread_ts.is_empty() {
        return None;
    }
    Some(DecodedSlackThreadId {
        channel_id: channel_id.to_string(),
        thread_ts: thread_ts.to_string(),
    })
}

/// Predicate: does this thread id belong to the Slack adapter?
pub fn is_slack_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

/// Build the request payload posted to Slack's `chat.postEphemeral`
/// endpoint. 1:1 with upstream's text-path payload assembly. Mirrors
/// upstream's `const threadTs = rawThreadTs || undefined` normalization
/// — when `thread_ts` is empty, the `thread_ts` field is omitted from
/// the payload (matching upstream's `JSON.stringify` of `undefined`,
/// which removes the key entirely).
pub fn slack_post_ephemeral_payload(
    channel: &str,
    thread_ts: &str,
    user_id: &str,
    text: &str,
) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(4);
    map.insert("channel".to_string(), serde_json::Value::from(channel));
    map.insert("user".to_string(), serde_json::Value::from(user_id));
    map.insert("text".to_string(), serde_json::Value::from(text));
    if !thread_ts.is_empty() {
        map.insert("thread_ts".to_string(), serde_json::Value::from(thread_ts));
    }
    serde_json::Value::Object(map)
}

/// Parse a Slack `chat.postEphemeral` response JSON into an
/// [`chat_sdk_chat::types::EphemeralMessage`]. 1:1 with upstream's
/// response-mapping branch. Preserves the `result.message_ts || ""`
/// fallback — Slack occasionally returns a successful payload
/// without `message_ts`, and the upstream contract surfaces that as
/// an empty string rather than a typed error.
pub fn parse_slack_post_ephemeral_response(
    json: &serde_json::Value,
    thread_id: &str,
) -> chat_sdk_chat::types::EphemeralMessage {
    chat_sdk_chat::types::EphemeralMessage {
        id: json["message_ts"].as_str().unwrap_or("").to_string(),
        thread_id: thread_id.to_string(),
        used_fallback: false,
        raw: json.clone(),
    }
}

#[cfg(test)]
mod tests {
    //! ---------- upstream js-only-documented cases (7) ----------
    //!
    //! Per the slice-380 type-system-impossible pattern, the
    //! following upstream `index.test.ts` cases are enumerated as
    //! js-only-documented here because they exercise behavior that
    //! is unrepresentable in the Rust port by construction:
    //!
    //! 1. `describe("subclass extensibility") > should expose
    //!    protected members and methods to subclasses` —
    //!    TypeScript `protected` access modifier check (verifies
    //!    subclasses can reach `logger` / `formatConverter` / etc on
    //!    the base class). Rust uses `pub(crate)` visibility +
    //!    trait composition rather than class inheritance.
    //!
    //! 2. `describe("webClient getter") > returns the underlying
    //!    WebClient bound to the static botToken` — asserts the
    //!    getter returns a `WebClient` typed-class instance with
    //!    `.token` exposed. Rust has no `WebClient` equivalent —
    //!    HTTP is held as an opaque `reqwest::Client`; the typed
    //!    "instanceof" + `.token` accessor have no Rust analogue.
    //!
    //! 3. `describe("webClient getter") > returns the same
    //!    instance across calls in single-workspace mode` —
    //!    WebClient referential equality. The Rust port's `Client`
    //!    is held by value (Clone-shared underlying pool); per-call
    //!    referential equality is moot.
    //!
    //! 4. `describe("webClient getter") > exposes the same
    //!    instance via the deprecated "client" alias` — alias-name
    //!    backwards-compat. The Rust port never shipped the
    //!    deprecated alias, so there is nothing to assert.
    //!
    //! 5. `describe("webClient getter") > throws on both
    //!    "webClient" and the "client" alias in multi-workspace
    //!    mode without context` — runtime "no workspace context"
    //!    throw via AsyncLocalStorage. The Rust port surfaces the
    //!    equivalent via typed errors at the per-workspace call
    //!    sites (webhook handler), not via a property getter, so
    //!    the property-throw shape is unrepresentable.
    //!
    //! 6. `describe("webClient getter") > uses the request
    //!    context token under withBotToken via "webClient"` —
    //!    AsyncLocalStorage-based per-request token resolution
    //!    through a getter. The Rust port plumbs per-request
    //!    token state through function parameters rather than
    //!    thread-local context, so the ALS-based getter shape is
    //!    moot.
    //!
    //! Additionally `describe("direct WebClient access via
    //!    adapter.client")` re-asserts the same alias semantics
    //!    as case 4 (deprecated property alias check); accounted
    //!    for under case 4.
    //!
    //! 7. `describe("constructor env var resolution") > should
    //!    default logger when not provided` — asserts the
    //!    constructor falls back to a default `Logger` instance
    //!    when none is supplied. Rust adapters do not take a
    //!    `Logger` as a first-class adapter dependency (logging
    //!    is plumbed via the `log` crate's static dispatch
    //!    elsewhere); the constructor-default-logger fallback
    //!    shape is moot.
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_slack() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb-test", "secret"));
        assert_eq!(adapter.name(), "slack");
    }

    #[test]
    fn options_new_stores_credentials_and_defaults_api_base() {
        let opts = SlackAdapterOptions::new("xoxb-test", "secret");
        assert_eq!(opts.bot_token, "xoxb-test");
        assert_eq!(opts.signing_secret, "secret");
        assert!(opts.app_token.is_none());
        assert_eq!(opts.effective_api_base(), DEFAULT_API_BASE);
    }

    #[test]
    fn options_with_app_token_attaches_the_token() {
        let opts = SlackAdapterOptions::new("xoxb", "s").with_app_token("xapp-1-XXX");
        assert_eq!(opts.app_token.as_deref(), Some("xapp-1-XXX"));
    }

    #[test]
    fn options_with_api_base_overrides_the_default() {
        let opts =
            SlackAdapterOptions::new("xoxb", "s").with_api_base("https://slack.example.test/api");
        assert_eq!(opts.effective_api_base(), "https://slack.example.test/api");
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(
            encode_thread_id("C0123ABCD", "1234567890.123456"),
            "slack:C0123ABCD:1234567890.123456"
        );
    }

    #[test]
    fn decode_thread_id_parses_channel_and_thread_ts() {
        let decoded = decode_thread_id("slack:C0123ABCD:1234567890.123456").unwrap();
        assert_eq!(decoded.channel_id, "C0123ABCD");
        assert_eq!(decoded.thread_ts, "1234567890.123456");
        assert!(!decoded.is_dm());
        assert!(!decoded.is_group());
    }

    #[test]
    fn decode_thread_id_detects_dm_channels() {
        let decoded = decode_thread_id("slack:D012ABC:1234567890.0").unwrap();
        assert!(decoded.is_dm());
        assert!(!decoded.is_group());
    }

    #[test]
    fn decode_thread_id_detects_group_channels() {
        let decoded = decode_thread_id("slack:G123XYZ:1234567890.0").unwrap();
        assert!(decoded.is_group());
        assert!(!decoded.is_dm());
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("teams:CONV:MSG").is_none());
        assert!(decode_thread_id("gchat:A:B").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_components() {
        assert!(decode_thread_id("slack:onlyone").is_none());
        assert!(decode_thread_id("slack::1234.5").is_none());
        assert!(decode_thread_id("slack:C123:").is_none());
    }

    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream's `channelIdFromThreadId(threadId)` (returns
    // `slack:<channel>`) and `isDM(threadId)` (true iff the underlying
    // Slack channel id starts with `D`).

    #[test]
    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    // ---------- getChannelVisibility (4 upstream cases) ----------
    // 1:1 with upstream's `getChannelVisibility(threadId)` behavior:
    // external if `_externalChannels.has(channel)`, else private for
    // G/D prefixes, workspace for C, unknown otherwise.
    #[test]
    fn get_channel_visibility_returns_workspace_for_public_channels() {
        use chat_sdk_chat::types::ChannelVisibility;
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "s"));
        assert_eq!(
            adapter.get_channel_visibility("slack:C12345:1700000000.000200"),
            ChannelVisibility::Workspace
        );
    }

    #[test]
    fn get_channel_visibility_returns_private_for_g_and_d_prefixed_channels() {
        use chat_sdk_chat::types::ChannelVisibility;
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "s"));
        assert_eq!(
            adapter.get_channel_visibility("slack:G99999:1700000000.000200"),
            ChannelVisibility::Private
        );
        assert_eq!(
            adapter.get_channel_visibility("slack:DABCDE:1700000000.000200"),
            ChannelVisibility::Private
        );
    }

    #[test]
    fn get_channel_visibility_returns_external_after_mark_external_channel() {
        use chat_sdk_chat::types::ChannelVisibility;
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "s"));
        adapter.mark_external_channel("C12345");
        assert!(adapter.is_external_channel("C12345"));
        assert_eq!(
            adapter.get_channel_visibility("slack:C12345:1700000000.000200"),
            ChannelVisibility::External
        );
    }

    #[test]
    fn get_channel_visibility_returns_unknown_for_unrecognized_prefixes_and_non_slack_ids() {
        use chat_sdk_chat::types::ChannelVisibility;
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "s"));
        // Channel ids that don't start with C/G/D fall through to
        // Unknown — matches upstream's default branch.
        assert_eq!(
            adapter.get_channel_visibility("slack:X1234:1700000000.000200"),
            ChannelVisibility::Unknown
        );
        // Non-Slack thread ids fail to decode -> Unknown.
        assert_eq!(
            adapter.get_channel_visibility("discord:G:C:T"),
            ChannelVisibility::Unknown
        );
        assert_eq!(
            adapter.get_channel_visibility(""),
            ChannelVisibility::Unknown
        );
    }

    #[test]
    #[test]
    fn slack_timing_constants_match_upstream() {
        // 1:1 with upstream's private `OPTIONS_LOAD_TIMEOUT_MS`,
        // `UNFURL_WAIT_MS`, `UNFURL_POLL_MS`.
        assert_eq!(OPTIONS_LOAD_TIMEOUT_MS, 2500);
        assert_eq!(UNFURL_WAIT_MS, 2000);
        assert_eq!(UNFURL_POLL_MS, 150);
    }

    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("bot-token", "signing"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    // ---------- describe("channelIdFromThreadId") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("channelIdFromThreadId")`.
    // Previously a single `channel_id_from_thread_id_strips_the_thread_ts_suffix`
    // test bundled upstream's case 1 ("extracts channel ID from thread
    // ID"); upstream's case 2 ("works with empty threadTs") wasn't
    // covered. Per the slice-451 split-and-rename pattern, the bundle
    // is now split into one Rust test per upstream case, and the
    // `channel_id_from_thread_id` helper was made more permissive to
    // match upstream's behavior on `slack:C456:` (empty threadTs).

    #[test]
    fn channel_id_from_thread_id_extracts_channel_id_from_thread_id() {
        // 1:1 with upstream `channelIdFromThreadId > extracts channel
        // ID from thread ID` — `slack:C123:1234567890.000000` ->
        // `slack:C123`.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("bot-token", "signing"));
        assert_eq!(
            adapter
                .channel_id_from_thread_id("slack:C123:1234567890.000000")
                .as_deref(),
            Some("slack:C123")
        );
    }

    #[test]
    fn channel_id_from_thread_id_works_with_empty_thread_ts() {
        // 1:1 with upstream `channelIdFromThreadId > works with empty
        // threadTs` — `slack:C456:` (trailing colon, empty threadTs)
        // -> `slack:C456`. Previously the strict `decode_thread_id`
        // helper rejected this and `channel_id_from_thread_id`
        // returned None, diverging from upstream.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("bot-token", "signing"));
        assert_eq!(
            adapter.channel_id_from_thread_id("slack:C456:").as_deref(),
            Some("slack:C456")
        );
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn channel_id_from_thread_id_handles_dm_channels() {
        // Additive (not in upstream's `channelIdFromThreadId` describe):
        // exercises the DM-channel-prefix path the additional encoder
        // tests already cover, so the helper's behavior is locked in
        // for adapter trait callers.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("bot-token", "signing"));
        assert_eq!(
            adapter
                .channel_id_from_thread_id("slack:DABC:1700000000.000200")
                .as_deref(),
            Some("slack:DABC")
        );
    }

    #[test]
    fn channel_id_from_thread_id_returns_none_for_non_slack_ids() {
        // Additive: upstream throws ValidationError on wrong prefix;
        // the Rust port maps that to None per the Adapter trait shape.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "signing"));
        assert!(adapter.channel_id_from_thread_id("discord:G:C:1").is_none());
        assert!(adapter.channel_id_from_thread_id("").is_none());
    }

    // ---------- describe("isDM") (3 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("isDM")`. Previously
    // the C-prefix and G-prefix cases were bundled into a single
    // `is_dm_false_for_c_and_g_prefixed_channels` test; per the
    // slice-451 split-and-rename pattern they're now split into one
    // Rust test per upstream case.

    #[test]
    fn is_dm_returns_true_for_dm_channels_d_prefix() {
        // 1:1 with upstream `isDM > returns true for DM channels (D prefix)`.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "signing"));
        assert_eq!(adapter.is_dm("slack:DABC:1.0"), Some(true));
        assert_eq!(adapter.is_dm("slack:D1:1.0"), Some(true));
    }

    #[test]
    fn is_dm_returns_false_for_public_channels_c_prefix() {
        // 1:1 with upstream `isDM > returns false for public channels (C prefix)`.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "signing"));
        assert_eq!(adapter.is_dm("slack:C123:1.0"), Some(false));
    }

    #[test]
    fn is_dm_returns_false_for_private_channels_g_prefix() {
        // 1:1 with upstream `isDM > returns false for private channels (G prefix)`.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "signing"));
        assert_eq!(adapter.is_dm("slack:G123:1.0"), Some(false));
    }

    #[test]
    fn is_dm_returns_none_for_non_slack_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("t", "signing"));
        assert_eq!(adapter.is_dm("discord:G:C"), None);
        assert_eq!(adapter.is_dm(""), None);
    }

    #[test]
    fn is_slack_thread_id_detects_the_prefix() {
        assert!(is_slack_thread_id("slack:C0123:1234.5"));
        assert!(!is_slack_thread_id("teams:1:2"));
        assert!(!is_slack_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (c, t) in [
            ("C0123ABCD", "1234567890.123456"),
            ("D012ABC", "1234567890.0"),
            ("G123XYZ", "1.0"),
        ] {
            let encoded = encode_thread_id(c, t);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.channel_id, c);
            assert_eq!(decoded.thread_ts, t);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("teams:CONV:MSG", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("teams:CONV:MSG", "1234.5", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("teams:CONV:MSG", "1234.5"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("teams:CONV:MSG", "1234.5", "thumbsup"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_remove_reaction_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.remove_reaction("teams:CONV:MSG", "1234.5", "thumbsup"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_remove_reaction_method_url_targets_reactions_remove() {
        // Verifies the URL helper produces the right Slack API
        // method for remove_reaction (the path tested upstream
        // implicitly via the `reactions.remove` mock spy).
        let adapter = SlackAdapter::new(
            SlackAdapterOptions::new("xoxb", "s").with_api_base("https://slack.example.test/api"),
        );
        assert_eq!(
            adapter.method_url("reactions.remove"),
            "https://slack.example.test/api/reactions.remove"
        );
    }

    // ---------- Slack Web API method coverage (parametric) ----------
    // Parametric coverage of all Slack Web API methods the adapter
    // posts to. Mirrors the upstream `index.test.ts` URL-shape
    // assertions (each per-method `it("calls slack <method>")`
    // describes asserts via the mockClient spy on the URL path).
    // Bundles them into one Rust test since they all flow through
    // the same `method_url` helper.

    #[test]
    fn adapter_method_url_produces_slack_endpoints_for_all_runtime_methods() {
        let adapter = SlackAdapter::new(
            SlackAdapterOptions::new("xoxb", "s").with_api_base("https://slack.example.test/api"),
        );
        for method in [
            "chat.postMessage",
            "chat.postEphemeral",
            "chat.update",
            "chat.delete",
            "conversations.info",
            "reactions.add",
            "reactions.remove",
            "assistant.threads.setStatus",
            "views.open",
            "views.update",
        ] {
            let url = adapter.method_url(method);
            assert_eq!(
                url,
                format!("https://slack.example.test/api/{method}"),
                "method_url({method}) mismatch"
            );
        }
    }

    #[test]
    fn adapter_start_typing_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.start_typing("teams:CONV:MSG", None));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_fetch_subject_rejects_non_slack_thread_ids() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.fetch_subject("teams:CONV:MSG"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_method_url_combines_api_base_and_method() {
        let adapter = SlackAdapter::new(
            SlackAdapterOptions::new("xoxb", "s").with_api_base("https://slack.example.test/api"),
        );
        assert_eq!(
            adapter.method_url("chat.postMessage"),
            "https://slack.example.test/api/chat.postMessage"
        );
    }

    #[test]
    fn adapter_post_object_rejects_non_slack_thread_ids_via_fallback_path() {
        // Unknown kind -> falls back to post_message which decodes the
        // thread id first.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_object("teams:CONV:MSG", "card", serde_json::json!({})));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_post_object_plan_rejects_non_plan_payloads() {
        // Slack post_object with kind="plan" requires PlanModel-shaped
        // data; non-conforming JSON surfaces as InvalidPayload.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_object(
            "slack:C0123:1.0",
            "plan",
            serde_json::json!({ "not-a-plan": true }),
        ));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("is not a PlanModel"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn render_plan_fallback_text_matches_upstream_layout() {
        // Mirrors upstream renderPlanFallbackText:
        //   title
        //   - (status) task title
        //   ...
        use chat_sdk_chat::plan::{PlanModel, PlanModelTask, PlanTaskStatus};
        let plan = PlanModel {
            title: "Onboarding".to_string(),
            tasks: vec![
                PlanModelTask {
                    id: "t1".to_string(),
                    title: "Read docs".to_string(),
                    status: PlanTaskStatus::Complete,
                    details: None,
                    output: None,
                },
                PlanModelTask {
                    id: "t2".to_string(),
                    title: "Run setup".to_string(),
                    status: PlanTaskStatus::InProgress,
                    details: None,
                    output: None,
                },
                PlanModelTask {
                    id: "t3".to_string(),
                    title: "Verify".to_string(),
                    status: PlanTaskStatus::Pending,
                    details: None,
                    output: None,
                },
                PlanModelTask {
                    id: "t4".to_string(),
                    title: "Cleanup".to_string(),
                    status: PlanTaskStatus::Error,
                    details: None,
                    output: None,
                },
            ],
        };
        let text = render_plan_fallback_text(&plan);
        assert_eq!(
            text,
            "Onboarding\n\
             - (complete) Read docs\n\
             - (in_progress) Run setup\n\
             - (pending) Verify\n\
             - (error) Cleanup"
        );
    }

    #[test]
    fn render_plan_fallback_text_uses_default_title_when_empty() {
        use chat_sdk_chat::plan::PlanModel;
        let plan = PlanModel {
            title: String::new(),
            tasks: vec![],
        };
        assert_eq!(render_plan_fallback_text(&plan), "Plan");
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = SlackAdapter::new(
            SlackAdapterOptions::new("xoxb-tok", "sig-sec")
                .with_app_token("xapp-tok")
                .with_api_base("https://example.test/api"),
        );
        assert_eq!(adapter.bot_token(), "xoxb-tok");
        assert_eq!(adapter.signing_secret(), "sig-sec");
        assert_eq!(adapter.app_token(), Some("xapp-tok"));
        assert_eq!(adapter.api_base(), "https://example.test/api");
    }

    // ---------- createSlackAdapter describe block (4 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("createSlackAdapter")`.

    #[test]
    fn create_slack_adapter_creates_an_instance() {
        let opts = SlackAdapterOptions::new("xoxb-test-token", "test-secret");
        let adapter = SlackAdapter::new(opts);
        assert_eq!(adapter.name(), "slack");
    }

    #[test]
    fn create_slack_adapter_sets_default_user_name_to_bot() {
        let opts = SlackAdapterOptions::new("xoxb-test-token", "test-secret");
        let adapter = SlackAdapter::new(opts);
        assert_eq!(adapter.user_name(), "bot");
    }

    #[test]
    fn create_slack_adapter_uses_provided_user_name() {
        let mut opts = SlackAdapterOptions::new("xoxb-test-token", "test-secret");
        opts.user_name = Some("custombot".to_string());
        let adapter = SlackAdapter::new(opts);
        assert_eq!(adapter.user_name(), "custombot");
    }

    #[test]
    fn create_slack_adapter_stores_bot_user_id_when_provided() {
        let mut opts = SlackAdapterOptions::new("xoxb-test-token", "test-secret");
        opts.bot_user_id = Some("U12345".to_string());
        let adapter = SlackAdapter::new(opts);
        assert_eq!(adapter.bot_user_id(), Some("U12345"));
    }

    // ---------- describe("postEphemeral") (3 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("postEphemeral")`.
    // Upstream relies on `mockClient.chat.postEphemeral` to intercept
    // the Slack Web API call; the Rust port has no HTTP mock layer
    // here, so we cover the same observable behavior via the pure
    // [`slack_post_ephemeral_payload`] + [`parse_slack_post_ephemeral_response`]
    // helpers that the runtime path also flows through. Per-method
    // URL coverage stays in the parametric `method_url` test below.

    #[test]
    fn slack_post_ephemeral_method_url_targets_chat_post_ephemeral() {
        let adapter = SlackAdapter::new(
            SlackAdapterOptions::new("xoxb", "s").with_api_base("https://slack.example.test/api"),
        );
        assert_eq!(
            adapter.method_url("chat.postEphemeral"),
            "https://slack.example.test/api/chat.postEphemeral"
        );
    }

    #[test]
    fn slack_post_ephemeral_posts_ephemeral_message_to_a_user() {
        // 1:1 with upstream "posts an ephemeral message to a user".
        // Validates the payload includes channel/user/text/thread_ts
        // (channel decoded from the slack: thread id, thread_ts
        // preserved when non-empty), and the parsed response surfaces
        // id/threadId/usedFallback=false.
        let body =
            slack_post_ephemeral_payload("C123", "1234567890.000000", "U_USER_1", "Ephemeral text");
        assert_eq!(body["channel"], "C123");
        assert_eq!(body["user"], "U_USER_1");
        assert_eq!(body["text"], "Ephemeral text");
        assert_eq!(body["thread_ts"], "1234567890.000000");

        let response = serde_json::json!({ "ok": true, "message_ts": "1234567890.888888" });
        let parsed = parse_slack_post_ephemeral_response(&response, "slack:C123:1234567890.000000");
        assert_eq!(parsed.id, "1234567890.888888");
        assert_eq!(parsed.thread_id, "slack:C123:1234567890.000000");
        assert!(!parsed.used_fallback);
    }

    #[test]
    fn slack_post_ephemeral_normalizes_empty_thread_ts_to_undefined() {
        // 1:1 with upstream "normalizes empty threadTs to undefined".
        // Decoded `slack:C123:` doesn't parse via `decode_thread_id`
        // (empty thread_ts is rejected as an invalid thread id), so
        // upstream's empty-ts shape can't reach the dispatcher in the
        // Rust port. Validate the helper at the payload-shape layer:
        // an empty thread_ts is omitted entirely from the JSON body
        // (matching upstream's `JSON.stringify(undefined)` semantics
        // which removes the key).
        let body = slack_post_ephemeral_payload("C123", "", "U_USER_1", "Ephemeral text");
        assert_eq!(body["channel"], "C123");
        assert_eq!(body["user"], "U_USER_1");
        assert!(body.get("thread_ts").is_none());
    }

    #[test]
    fn slack_post_ephemeral_handles_empty_message_ts_in_response() {
        // 1:1 with upstream "handles empty message_ts in response".
        // Upstream contract: `result.message_ts || ""` — when Slack
        // omits message_ts on success, the EphemeralMessage.id is the
        // empty string rather than a typed error.
        let response = serde_json::json!({ "ok": true });
        let parsed = parse_slack_post_ephemeral_response(&response, "slack:C123:1234567890.000000");
        assert_eq!(parsed.id, "");
        assert_eq!(parsed.thread_id, "slack:C123:1234567890.000000");
        assert!(!parsed.used_fallback);
    }

    #[test]
    fn slack_post_ephemeral_rejects_non_slack_thread_ids() {
        // Rust-only: the dispatcher decodes the thread id first; a
        // non-slack-prefixed id surfaces as InvalidPayload.
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_ephemeral("teams:CONV:MSG", "U_USER_1", "x"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Slack-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }
}
