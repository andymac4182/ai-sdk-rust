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
}

impl SlackAdapter {
    /// 1:1 port of upstream
    /// `new SlackAdapter({ botToken, signingSecret, appToken?, apiBase? })`.
    pub fn new(options: SlackAdapterOptions) -> Self {
        Self { options }
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
}

#[async_trait]
impl Adapter for SlackAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
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
    fn adapter_default_methods_return_unsupported() {
        let adapter = SlackAdapter::new(SlackAdapterOptions::new("xoxb", "s"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C0123:1234.5", "hi"));
        assert!(matches!(
            err,
            Err(AdapterError::Unsupported("post_message"))
        ));
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
