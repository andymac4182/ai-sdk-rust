//! Microsoft Teams adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-teams/src/index.ts`.
//!
//! Teams uses the Bot Framework conversation model. The thread id
//! encoding is `teams:<conversation_id>:<message_id>` — when posting
//! a new top-level reply, `message_id` is the root activity id.

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "teams";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "teams:";

/// Default Bot Framework / Teams API base URL.
pub const DEFAULT_API_BASE: &str = "https://smba.trafficmanager.net";

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
}

impl TeamsAdapter {
    /// 1:1 port of upstream
    /// `new TeamsAdapter({ appId, appPassword, tenantId, apiBase? })`.
    pub fn new(options: TeamsAdapterOptions) -> Self {
        Self { options }
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
}

#[async_trait]
impl Adapter for TeamsAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
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
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_teams() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("app", "pwd", "tenant"));
        assert_eq!(adapter.name(), "teams");
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
    fn adapter_default_methods_return_unsupported() {
        let adapter = TeamsAdapter::new(TeamsAdapterOptions::new("a", "p", "t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("teams:CONV:MSG", "hi"));
        assert!(matches!(
            err,
            Err(AdapterError::Unsupported("post_message"))
        ));
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
}
