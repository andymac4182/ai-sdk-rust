//! Linear adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-linear/src/index.ts`.
//!
//! Linear maps each issue's comment stream to one chat-sdk thread.
//! The thread id encoding is `linear:<team_key>:<issue_id>` — the
//! `team_key` (e.g. "ENG") plus the GraphQL issue UUID is sufficient
//! to address comments through Linear's API.

pub mod cards;
pub mod markdown;
pub mod thread_id;
pub mod utils;

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
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl LinearAdapter {
    /// 1:1 port of upstream
    /// `new LinearAdapter({ apiKey, graphqlUrl? })`.
    pub fn new(options: LinearAdapterOptions) -> Self {
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

    /// Read the API key.
    pub fn api_key(&self) -> &str {
        &self.options.api_key
    }

    /// Effective GraphQL URL.
    pub fn graphql_url(&self) -> &str {
        self.options.effective_graphql_url()
    }

    /// Derive channel id from a Linear thread id. 1:1 with upstream
    /// `adapter.channelIdFromThreadId(threadId)` which decodes via
    /// `decodeThreadId` and returns `linear:<issueId>`. Returns
    /// `None` when `thread_id` isn't a Linear-encoded value.
    ///
    /// Uses the upstream-shaped [`crate::thread_id::decode_thread_id`]
    /// (slice 216) so all 4 wire formats are handled:
    /// `linear:<issue>` / `linear:<issue>:c:<comment>` /
    /// `linear:<issue>:s:<session>` /
    /// `linear:<issue>:c:<comment>:s:<session>`.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        let decoded = crate::thread_id::decode_thread_id(thread_id).ok()?;
        Some(format!("linear:{}", decoded.issue_id))
    }

    /// All Linear conversations are issue comment threads, not DMs.
    /// 1:1 with upstream's hard-coded `isDM: false` on every parsed
    /// message.
    pub fn is_dm(&self, _thread_id: &str) -> bool {
        false
    }

    /// Render formatted content to Linear-flavored markdown. 1:1
    /// with upstream `adapter.renderFormatted(content)` which
    /// delegates to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::LinearFormatConverter::new().from_ast(ast)
    }
}

/// GraphQL mutation Linear uses to create a comment on an issue.
/// Lifted out as a `const` so tests can lock the wire shape.
pub const COMMENT_CREATE_MUTATION: &str = "mutation CreateComment($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success, comment { id } } }";

/// GraphQL mutation for `commentUpdate` (used by `edit_message`).
pub const COMMENT_UPDATE_MUTATION: &str = "mutation UpdateComment($id: String!, $body: String!) { commentUpdate(id: $id, input: { body: $body }) { success, comment { id } } }";

/// GraphQL mutation for `commentDelete` (used by `delete_message`).
pub const COMMENT_DELETE_MUTATION: &str =
    "mutation DeleteComment($id: String!) { commentDelete(id: $id) { success } }";

/// GraphQL mutation for `reactionCreate` (used by `add_reaction`).
pub const REACTION_CREATE_MUTATION: &str = "mutation CreateReaction($commentId: String!, $emoji: String!) { reactionCreate(input: { commentId: $commentId, emoji: $emoji }) { success } }";

#[async_trait]
impl Adapter for LinearAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a comment on a Linear issue via GraphQL. 1:1 with
    /// upstream's `adapter.postMessage`:
    ///
    /// - Decodes `linear:<team_key>:<issue_id>` (the `team_key` is
    ///   carried in the thread id for cross-team display but is
    ///   not required for the mutation — `issue_id` is the GraphQL
    ///   UUID Linear's API addresses by).
    /// - POSTs `{query, variables: {issueId, body}}` to the Linear
    ///   GraphQL endpoint with `Authorization: <api_key>` (Linear
    ///   personal API keys don't carry a scheme prefix; OAuth2
    ///   tokens use `Bearer ` — adopters using OAuth should pass
    ///   the full `Bearer <token>` string as the api_key).
    /// - Returns `data.commentCreate.comment.id` (Linear comment
    ///   UUID).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Linear-encoded"))
        })?;

        let body = serde_json::json!({
            "query": COMMENT_CREATE_MUTATION,
            "variables": {
                "issueId": decoded.issue_id,
                "body": text,
            }
        });

        let response = self
            .http
            .post(self.graphql_url())
            .header("Authorization", self.api_key())
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
                "{status}: Linear GraphQL request failed"
            )));
        }

        // GraphQL errors come back with status 200 + an `errors`
        // array; the comment data lives at
        // `data.commentCreate.comment.id`.
        if let Some(first_error) = json["errors"][0]["message"].as_str() {
            return Err(AdapterError::InvalidPayload(format!(
                "Linear GraphQL error: {first_error}"
            )));
        }

        if !json["data"]["commentCreate"]["success"]
            .as_bool()
            .unwrap_or(false)
        {
            return Err(AdapterError::InvalidPayload(
                "Linear commentCreate returned success=false".to_string(),
            ));
        }

        json["data"]["commentCreate"]["comment"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(
                    "Linear commentCreate response missing comment.id".to_string(),
                )
            })
    }

    /// Edit a Linear comment via the `commentUpdate` GraphQL
    /// mutation. 1:1 with the comment-path of upstream's
    /// `adapter.editMessage` (agent-session activities are
    /// append-only upstream — that branch is deferred).
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let _decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Linear-encoded"))
        })?;

        let payload = serde_json::json!({
            "query": COMMENT_UPDATE_MUTATION,
            "variables": {
                "id": message_id,
                "body": text,
            }
        });

        let json = self.linear_graphql_call(&payload).await?;

        if !json["data"]["commentUpdate"]["success"]
            .as_bool()
            .unwrap_or(false)
        {
            return Err(AdapterError::InvalidPayload(
                "Linear commentUpdate returned success=false".to_string(),
            ));
        }

        json["data"]["commentUpdate"]["comment"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(
                    "Linear commentUpdate response missing comment.id".to_string(),
                )
            })
    }

    /// Delete a Linear comment via the `commentDelete` GraphQL
    /// mutation. 1:1 with the comment-path of upstream's
    /// `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let _decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Linear-encoded"))
        })?;

        let payload = serde_json::json!({
            "query": COMMENT_DELETE_MUTATION,
            "variables": { "id": message_id }
        });

        let json = self.linear_graphql_call(&payload).await?;

        if !json["data"]["commentDelete"]["success"]
            .as_bool()
            .unwrap_or(false)
        {
            return Err(AdapterError::InvalidPayload(
                "Linear commentDelete returned success=false".to_string(),
            ));
        }
        Ok(())
    }

    /// Add an emoji reaction via `reactionCreate` GraphQL mutation.
    /// 1:1 with upstream's `adapter.addReaction`.
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let _decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Linear-encoded"))
        })?;

        let payload = serde_json::json!({
            "query": REACTION_CREATE_MUTATION,
            "variables": {
                "commentId": message_id,
                "emoji": emoji,
            }
        });

        let json = self.linear_graphql_call(&payload).await?;

        if !json["data"]["reactionCreate"]["success"]
            .as_bool()
            .unwrap_or(false)
        {
            return Err(AdapterError::InvalidPayload(
                "Linear reactionCreate returned success=false".to_string(),
            ));
        }
        Ok(())
    }

    /// Linear has no typing indicator for comments. The agent
    /// session path (createAgentActivity with Thought type) is
    /// deferred. 1:1 with upstream's no-op for the comment thread
    /// case.
    async fn start_typing(
        &self,
        _thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        Ok(())
    }
}

impl LinearAdapter {
    /// Internal helper for issuing GraphQL mutations against
    /// Linear's API. Centralises the auth + status + GraphQL-error
    /// envelope handling. Returns the parsed response JSON on
    /// success.
    async fn linear_graphql_call(
        &self,
        payload: &serde_json::Value,
    ) -> chat_sdk_chat::types::AdapterResult<serde_json::Value> {
        use chat_sdk_chat::types::AdapterError;

        let response = self
            .http
            .post(self.graphql_url())
            .header("Authorization", self.api_key())
            .header("Content-Type", "application/json")
            .json(payload)
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
                "{status}: Linear GraphQL request failed"
            )));
        }

        if let Some(first_error) = json["errors"][0]["message"].as_str() {
            return Err(AdapterError::InvalidPayload(format!(
                "Linear GraphQL error: {first_error}"
            )));
        }

        Ok(json)
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
    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream's helpers (channel = `linear:<issueId>`,
    // isDM always `false`). Uses the upstream-shape
    // `thread_id::decode_thread_id` so all 4 wire formats decode.

    #[test]
    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("api-key"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn channel_id_from_thread_id_returns_linear_prefix_plus_issue_id() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("api-key"));
        // issue-only thread id
        assert_eq!(
            adapter
                .channel_id_from_thread_id("linear:ISSUE-1")
                .as_deref(),
            Some("linear:ISSUE-1")
        );
        // issue + comment thread id
        assert_eq!(
            adapter
                .channel_id_from_thread_id("linear:ISSUE-1:c:COMMENT-A")
                .as_deref(),
            Some("linear:ISSUE-1")
        );
        // issue + agent session thread id
        assert_eq!(
            adapter
                .channel_id_from_thread_id("linear:ISSUE-1:s:SESSION-X")
                .as_deref(),
            Some("linear:ISSUE-1")
        );
        // issue + comment + agent session thread id
        assert_eq!(
            adapter
                .channel_id_from_thread_id("linear:ISSUE-1:c:COMMENT-A:s:SESSION-X")
                .as_deref(),
            Some("linear:ISSUE-1")
        );
    }

    #[test]
    fn channel_id_from_thread_id_returns_none_for_non_linear_ids() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("api-key"));
        assert!(adapter.channel_id_from_thread_id("github:vercel/chat:42").is_none());
        assert!(adapter.channel_id_from_thread_id("").is_none());
    }

    #[test]
    fn is_dm_always_returns_false() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("api-key"));
        assert!(!adapter.is_dm("linear:ISSUE-1"));
        assert!(!adapter.is_dm(""));
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
    fn adapter_post_message_rejects_non_linear_thread_ids() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("k"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Linear-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_linear_thread_ids() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("k"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("slack:C1:1.0", "msg", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Linear-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_rejects_non_linear_thread_ids() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("k"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("slack:C1:1.0", "msg"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Linear-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_linear_thread_ids() {
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("k"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("slack:C1:1.0", "msg", ":+1:"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Linear-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_is_a_noop() {
        // Linear comment threads have no typing indicator; the
        // agent-session path is deferred.
        let adapter = LinearAdapter::new(LinearAdapterOptions::new("k"));
        assert!(block_on(adapter.start_typing("anything", None)).is_ok());
        assert!(block_on(adapter.start_typing("anything", Some("Thinking..."))).is_ok());
    }

    #[test]
    fn comment_update_mutation_shape() {
        assert!(COMMENT_UPDATE_MUTATION.contains("commentUpdate"));
        assert!(COMMENT_UPDATE_MUTATION.contains("$id: String!"));
        assert!(COMMENT_UPDATE_MUTATION.contains("$body: String!"));
    }

    #[test]
    fn comment_delete_mutation_shape() {
        assert!(COMMENT_DELETE_MUTATION.contains("commentDelete"));
        assert!(COMMENT_DELETE_MUTATION.contains("$id: String!"));
    }

    #[test]
    fn reaction_create_mutation_shape() {
        assert!(REACTION_CREATE_MUTATION.contains("reactionCreate"));
        assert!(REACTION_CREATE_MUTATION.contains("$commentId: String!"));
        assert!(REACTION_CREATE_MUTATION.contains("$emoji: String!"));
    }

    #[test]
    fn comment_create_mutation_includes_required_variables() {
        // Lock the GraphQL mutation shape so renames of the
        // upstream `commentCreate` payload break this test
        // rather than silently regressing the wire format.
        assert!(COMMENT_CREATE_MUTATION.contains("commentCreate"));
        assert!(COMMENT_CREATE_MUTATION.contains("$issueId: String!"));
        assert!(COMMENT_CREATE_MUTATION.contains("$body: String!"));
        assert!(COMMENT_CREATE_MUTATION.contains("comment { id }"));
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
