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
pub mod crypto;
pub mod format;
pub mod webhook;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "slack";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "slack:";

/// Default Slack Web API base URL.
pub const DEFAULT_API_BASE: &str = "https://slack.com/api";

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
}

impl SlackAdapterOptions {
    /// Construct options.
    pub fn new(bot_token: impl Into<String>, signing_secret: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            signing_secret: signing_secret.into(),
            app_token: None,
            api_base: None,
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
}

/// Slack adapter.
#[derive(Debug, Clone)]
pub struct SlackAdapter {
    options: SlackAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl SlackAdapter {
    /// 1:1 port of upstream
    /// `new SlackAdapter({ botToken, signingSecret, appToken?, apiBase? })`.
    pub fn new(options: SlackAdapterOptions) -> Self {
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

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Build a URL for a Slack Web API method. 1:1 with upstream's
    /// inline `${apiBase}/${method}` template.
    fn method_url(&self, method: &str) -> String {
        format!("{}/{method}", self.api_base())
    }
}

#[async_trait]
impl Adapter for SlackAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
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

#[cfg(test)]
mod tests {
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
}
