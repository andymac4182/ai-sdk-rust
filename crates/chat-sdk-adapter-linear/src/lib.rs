//! Linear adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-linear/src/index.ts`.
//!
//! Linear maps each issue's comment stream to one chat-sdk thread.
//! The thread id encoding is `linear:<team_key>:<issue_id>` — the
//! `team_key` (e.g. "ENG") plus the GraphQL issue UUID is sufficient
//! to address comments through Linear's API.

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "linear";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "linear:";

/// Default Linear GraphQL API URL.
pub const DEFAULT_GRAPHQL_URL: &str = "https://api.linear.app/graphql";

/// Options for [`LinearAdapter::new`].
#[derive(Debug, Clone)]
pub struct LinearAdapterOptions {
    /// Linear API key (`lin_api_*`) or OAuth2 access token.
    pub api_key: String,
    /// Optional GraphQL endpoint URL override.
    pub graphql_url: Option<String>,
}

impl LinearAdapterOptions {
    /// Construct options. GraphQL URL defaults to
    /// [`DEFAULT_GRAPHQL_URL`].
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            graphql_url: None,
        }
    }

    /// Override the GraphQL URL.
    pub fn with_graphql_url(mut self, graphql_url: impl Into<String>) -> Self {
        self.graphql_url = Some(graphql_url.into());
        self
    }

    /// Effective GraphQL URL with default applied.
    pub fn effective_graphql_url(&self) -> &str {
        self.graphql_url.as_deref().unwrap_or(DEFAULT_GRAPHQL_URL)
    }
}

/// Linear adapter.
#[derive(Debug, Clone)]
pub struct LinearAdapter {
    options: LinearAdapterOptions,
}

impl LinearAdapter {
    /// 1:1 port of upstream
    /// `new LinearAdapter({ apiKey, graphqlUrl? })`.
    pub fn new(options: LinearAdapterOptions) -> Self {
        Self { options }
    }

    /// Read the API key.
    pub fn api_key(&self) -> &str {
        &self.options.api_key
    }

    /// Effective GraphQL URL.
    pub fn graphql_url(&self) -> &str {
        self.options.effective_graphql_url()
    }
}

#[async_trait]
impl Adapter for LinearAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }
}

/// Encode a Linear thread id. 1:1 with upstream's inline format:
/// `linear:<team_key>:<issue_id>`.
pub fn encode_thread_id(team_key: &str, issue_id: &str) -> String {
    format!("{THREAD_ID_PREFIX}{team_key}:{issue_id}")
}

/// Components of a decoded Linear thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedLinearThreadId {
    /// Linear team key (e.g. "ENG").
    pub team_key: String,
    /// Linear issue id (UUID).
    pub issue_id: String,
}

/// Decode a Linear thread id.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedLinearThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let team_key = parts.next()?;
    let issue_id = parts.next()?;
    if team_key.is_empty() || issue_id.is_empty() {
        return None;
    }
    Some(DecodedLinearThreadId {
        team_key: team_key.to_string(),
        issue_id: issue_id.to_string(),
    })
}

/// Predicate: does this thread id belong to the Linear adapter?
pub fn is_linear_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_linear() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("lin_api_xxx"));
        assert_eq!(adapter.name(), "linear");
    }

    #[test]
    fn options_new_stores_api_key_and_defaults_graphql_url() {
        let opts = LinearAdapterOptions::new("lin_api_xxx");
        assert_eq!(opts.api_key, "lin_api_xxx");
        assert_eq!(opts.effective_graphql_url(), DEFAULT_GRAPHQL_URL);
    }

    #[test]
    fn options_with_graphql_url_overrides_the_default() {
        let opts =
            LinearAdapterOptions::new("k").with_graphql_url("https://linear.example.test/graphql");
        assert_eq!(
            opts.effective_graphql_url(),
            "https://linear.example.test/graphql"
        );
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(encode_thread_id("ENG", "abc-uuid"), "linear:ENG:abc-uuid");
    }

    #[test]
    fn decode_thread_id_parses_team_and_issue() {
        let decoded = decode_thread_id("linear:ENG:abc-uuid").unwrap();
        assert_eq!(decoded.team_key, "ENG");
        assert_eq!(decoded.issue_id, "abc-uuid");
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("github:owner/repo:1").is_none());
        assert!(decode_thread_id("telegram:123").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_components() {
        assert!(decode_thread_id("linear:onlyone").is_none());
        assert!(decode_thread_id("linear::issue").is_none());
        assert!(decode_thread_id("linear:ENG:").is_none());
    }

    #[test]
    fn is_linear_thread_id_detects_the_prefix() {
        assert!(is_linear_thread_id("linear:ENG:abc"));
        assert!(!is_linear_thread_id("github:1"));
        assert!(!is_linear_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (t, i) in [("ENG", "uuid-1"), ("DESIGN", "another-uuid"), ("a", "b")] {
            let encoded = encode_thread_id(t, i);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.team_key, t);
            assert_eq!(decoded.issue_id, i);
        }
    }

    #[test]
    fn adapter_default_methods_return_unsupported() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("k"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("linear:ENG:abc", "hi"));
        assert!(matches!(
            err,
            Err(AdapterError::Unsupported("post_message"))
        ));
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = LinearAdapter::new(
            LinearAdapterOptions::new("lin_api_xxx")
                .with_graphql_url("https://example.test/graphql"),
        );
        assert_eq!(adapter.api_key(), "lin_api_xxx");
        assert_eq!(adapter.graphql_url(), "https://example.test/graphql");
    }
}
