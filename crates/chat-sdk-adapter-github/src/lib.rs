//! GitHub adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-github/src/index.ts`.
//!
//! GitHub maps each PR / issue comment thread to one chat-sdk thread.
//! The thread id encoding is `github:<owner>/<repo>:<issue-or-pr-number>`.
//!
//! **What this slice ships (slice 131):**
//!
//! - Crate skeleton + `Cargo.toml`.
//! - [`GithubAdapter`] struct holding bot config (auth token +
//!   optional API base URL) impl-ing the chat-sdk
//!   [`chat_sdk_chat::types::Adapter`] trait with `name = "github"`.
//! - [`GithubAdapterOptions`] config struct (auth token + base
//!   URL override).
//! - [`encode_thread_id`] / [`decode_thread_id`] /
//!   [`is_github_thread_id`] — pure helpers for the upstream
//!   `github:<owner>/<repo>:<number>` wire format.
//!
//! **What is deferred:**
//!
//! - HTTP I/O against `api.github.com` for `post_message`/
//!   `post_object`/`fetch_subject`/... methods. Requires picking
//!   an HTTP client + async runtime; see `scripts/codex-goal-chat/
//!   port-chat-sdk.md`'s "Phase 2 / Phase 3 prep" section.
//! - GraphQL queries for richer scenarios (PR review comments,
//!   discussions).

pub mod cards;
pub mod markdown;
pub mod webhook;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator. 1:1 with upstream
/// `export const ADAPTER_NAME = "github"`.
pub const ADAPTER_NAME: &str = "github";

/// Thread-id prefix. 1:1 with upstream's inline `github:` namespace.
pub const THREAD_ID_PREFIX: &str = "github:";

/// Default GitHub REST API base URL. 1:1 with upstream
/// `const DEFAULT_API_BASE = "https://api.github.com"`.
pub const DEFAULT_API_BASE: &str = "https://api.github.com";

/// GitHub App credentials. 1:1 with upstream
/// `{ appId, privateKey, installationId? }` triple on
/// `GithubAdapterOptions`.
#[derive(Debug, Clone)]
pub struct GithubAppCredentials {
    pub app_id: String,
    pub private_key: String,
    /// `Some(id)` pins this app to a single tenant; `None` enables
    /// multi-tenant mode (the adapter resolves `installationId` per
    /// webhook via the app credentials).
    pub installation_id: Option<u64>,
}

/// GitHub authentication method. 1:1 with upstream's
/// `(token | app + installationId?)` discriminated union.
#[derive(Debug, Clone)]
pub enum GithubAuth {
    /// Personal access token or installation token.
    Token(String),
    /// GitHub App auth (single-tenant when `installation_id` is set,
    /// multi-tenant otherwise).
    App(GithubAppCredentials),
}

/// Options for [`GithubAdapter::new`]. 1:1 with upstream
/// `interface GithubAdapterOptions`.
#[derive(Debug, Clone)]
pub struct GithubAdapterOptions {
    /// Authentication credentials.
    pub auth: GithubAuth,
    /// Optional API base URL override (defaults to
    /// [`DEFAULT_API_BASE`]). Used by GitHub Enterprise installations
    /// and tests.
    pub api_base: Option<String>,
    /// Optional webhook signing secret (used by
    /// [`webhook::verify_github_signature`]).
    pub webhook_secret: Option<String>,
    /// Display name used as the bot identity.
    pub user_name: Option<String>,
    /// Numeric user id for self-mention detection. Stored as a
    /// string to match upstream's `botUserId: string` shape (it
    /// accepts a number config and stringifies on assignment).
    pub bot_user_id: Option<String>,
}

impl GithubAdapterOptions {
    /// Construct options with a personal access token; base URL
    /// defaults to [`DEFAULT_API_BASE`].
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            auth: GithubAuth::Token(token.into()),
            api_base: None,
            webhook_secret: None,
            user_name: None,
            bot_user_id: None,
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

    /// Borrow the auth token when configured with one. Returns
    /// `None` for App-based auth.
    pub fn token(&self) -> Option<&str> {
        match &self.auth {
            GithubAuth::Token(t) => Some(t.as_str()),
            GithubAuth::App(_) => None,
        }
    }

    /// Whether the adapter is in multi-tenant mode (App auth with
    /// no pinned `installation_id`). 1:1 with upstream's
    /// `readonly isMultiTenant: boolean`.
    pub fn is_multi_tenant(&self) -> bool {
        matches!(
            &self.auth,
            GithubAuth::App(GithubAppCredentials {
                installation_id: None,
                ..
            })
        )
    }
}

/// GitHub adapter. 1:1 port (in progress) of upstream
/// `class GithubAdapter implements Adapter`. Holds a shared
/// [`reqwest::Client`] from
/// [`chat_sdk_adapter_shared::runtime::default_http_client`].
#[derive(Debug, Clone)]
pub struct GithubAdapter {
    options: GithubAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl GithubAdapter {
    /// 1:1 port of upstream `new GithubAdapter({ token | (appId +
    /// privateKey + installationId?), webhookSecret?, userName?,
    /// botUserId?, apiBase? })`.
    pub fn new(options: GithubAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
        }
    }

    /// 1:1 with upstream `readonly isMultiTenant: boolean` —
    /// `true` when App-based auth has no pinned `installation_id`.
    pub fn is_multi_tenant(&self) -> bool {
        self.options.is_multi_tenant()
    }

    /// 1:1 with upstream `readonly userName?: string`.
    pub fn user_name(&self) -> Option<&str> {
        self.options.user_name.as_deref()
    }

    /// 1:1 with upstream `readonly botUserId?: string`.
    pub fn bot_user_id(&self) -> Option<&str> {
        self.options.bot_user_id.as_deref()
    }

    /// Override the HTTP client (mostly useful for tests).
    pub fn with_http_client(
        mut self,
        client: chat_sdk_adapter_shared::runtime::reqwest::Client,
    ) -> Self {
        self.http = client;
        self
    }

    /// Read the auth token (when configured with `GithubAuth::Token`).
    /// Returns the empty string for App-based auth — callers needing
    /// the credentials should match on `auth()` directly.
    pub fn token(&self) -> &str {
        self.options.token().unwrap_or("")
    }

    /// Borrow the configured authentication credentials.
    pub fn auth(&self) -> &GithubAuth {
        &self.options.auth
    }

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Build the absolute URL for `POST /repos/{owner}/{repo}/issues/{number}/comments`.
    /// 1:1 with upstream's inline comment-create URL template.
    fn comments_url(&self, owner: &str, repo: &str, number: u64) -> String {
        format!(
            "{}/repos/{owner}/{repo}/issues/{number}/comments",
            self.api_base()
        )
    }

    /// Build the absolute URL for `GET /repos/{owner}/{repo}/issues/{number}`.
    /// Used by `fetch_subject` to read the issue/PR title.
    fn issue_url(&self, owner: &str, repo: &str, number: u64) -> String {
        format!("{}/repos/{owner}/{repo}/issues/{number}", self.api_base())
    }

    /// Build the absolute URL for a specific issue comment:
    /// `<api_base>/repos/{owner}/{repo}/issues/comments/{comment_id}`.
    /// Used by `edit_message` (PATCH) and `delete_message` (DELETE).
    fn comment_url(&self, owner: &str, repo: &str, comment_id: u64) -> String {
        format!(
            "{}/repos/{owner}/{repo}/issues/comments/{comment_id}",
            self.api_base()
        )
    }

    /// Build the absolute URL for issue comment reactions:
    /// `<api_base>/repos/{owner}/{repo}/issues/comments/{comment_id}/reactions`.
    fn comment_reactions_url(&self, owner: &str, repo: &str, comment_id: u64) -> String {
        format!(
            "{}/repos/{owner}/{repo}/issues/comments/{comment_id}/reactions",
            self.api_base()
        )
    }

    /// Derive channel id from a GitHub thread id. 1:1 with upstream
    /// `adapter.channelIdFromThreadId(threadId)` which collapses any
    /// `github:<owner>/<repo>:<number>...` to `github:<owner>/<repo>`.
    /// Returns `None` when `thread_id` isn't GitHub-encoded.
    ///
    /// Handles both wire formats:
    /// - `github:<owner>/<repo>:<number>` (PR or issue thread)
    /// - `github:<owner>/<repo>:<number>:rc:<review-comment-id>` (review
    ///   comment thread)
    ///
    /// The collapsed channel id is always `github:<owner>/<repo>`,
    /// independent of which thread variant is supplied — upstream
    /// uses the same shape for both.
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
        // First colon-segment is `<owner>/<repo>`.
        let repo_path = suffix.split(':').next()?;
        let mut parts = repo_path.splitn(2, '/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        Some(format!("{THREAD_ID_PREFIX}{owner}/{repo}"))
    }

    /// All GitHub conversations are issue/PR threads, not DMs. 1:1
    /// with upstream's `isDM: false` on every parsed message.
    pub fn is_dm(&self, _thread_id: &str) -> bool {
        false
    }

    /// Render formatted content to GitHub-flavored markdown. 1:1
    /// with upstream `adapter.renderFormatted(content)` which
    /// delegates to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::GitHubFormatConverter::new().from_ast(ast)
    }
}

#[async_trait]
impl Adapter for GithubAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a comment on a GitHub issue or PR via the REST API.
    /// 1:1 with upstream's `adapter.postMessage`:
    ///
    /// - Decodes the chat-sdk thread id (`github:<owner>/<repo>:<number>`)
    ///   into the issue/PR coordinates.
    /// - POSTs JSON `{body: text}` to
    ///   `<api_base>/repos/<owner>/<repo>/issues/<number>/comments`
    ///   with the `Authorization: Bearer <token>` and
    ///   `Accept: application/vnd.github+json` headers.
    /// - Returns the comment id formatted as a decimal string.
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GitHub-encoded"))
        })?;

        let url = self.comments_url(&decoded.owner, &decoded.repo, decoded.number);
        let body = serde_json::json!({ "body": text });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.token())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
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
            let message = json["message"].as_str().unwrap_or("GitHub API call failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {message}")));
        }

        let id = json["id"].as_i64().ok_or_else(|| {
            AdapterError::InvalidPayload("GitHub comment-create response missing id".to_string())
        })?;
        Ok(id.to_string())
    }

    /// Fetch a GitHub issue/PR title via the REST API. 1:1 with
    /// upstream's `adapter.fetchSubject`:
    ///
    /// - Decodes `github:<owner>/<repo>:<number>`.
    /// - GETs `<api_base>/repos/<owner>/<repo>/issues/<number>`
    ///   with bearer auth + GitHub API headers.
    /// - Returns `Some(title)` from the response, matching
    ///   upstream's "subject = issue title" convention.
    async fn fetch_subject(
        &self,
        thread_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<Option<String>> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GitHub-encoded"))
        })?;

        let url = self.issue_url(&decoded.owner, &decoded.repo, decoded.number);

        let response = self
            .http
            .get(&url)
            .bearer_auth(self.token())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        if !status.is_success() {
            let message = json["message"]
                .as_str()
                .unwrap_or("GitHub issue-fetch failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {message}")));
        }

        Ok(json["title"].as_str().map(str::to_owned))
    }

    /// Edit an issue comment via the REST API. 1:1 with the
    /// issue-comment path of upstream `adapter.editMessage` (the
    /// review-comment branch is deferred). PATCH
    /// `repos/{owner}/{repo}/issues/comments/{comment_id}` with
    /// `{body: text}`. Returns the comment id.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GitHub-encoded"))
        })?;
        let comment_id: u64 = message_id.parse().map_err(|_| {
            AdapterError::InvalidPayload(format!("GitHub comment id {message_id:?} is not numeric"))
        })?;

        let url = self.comment_url(&decoded.owner, &decoded.repo, comment_id);
        let body = serde_json::json!({ "body": text });

        let response = self
            .http
            .patch(&url)
            .bearer_auth(self.token())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
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
            let message = json["message"].as_str().unwrap_or("GitHub API call failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {message}")));
        }

        let id = json["id"].as_i64().ok_or_else(|| {
            AdapterError::InvalidPayload("GitHub comment-update response missing id".to_string())
        })?;
        Ok(id.to_string())
    }

    /// Delete an issue comment. 1:1 with the issue-comment path of
    /// upstream `adapter.deleteMessage`. DELETE
    /// `repos/{owner}/{repo}/issues/comments/{comment_id}`.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GitHub-encoded"))
        })?;
        let comment_id: u64 = message_id.parse().map_err(|_| {
            AdapterError::InvalidPayload(format!("GitHub comment id {message_id:?} is not numeric"))
        })?;

        let url = self.comment_url(&decoded.owner, &decoded.repo, comment_id);
        let response = self
            .http
            .delete(&url)
            .bearer_auth(self.token())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
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

    /// Add a reaction to an issue comment. 1:1 with the
    /// issue-comment path of upstream `adapter.addReaction`. POSTs
    /// `{content}` to
    /// `repos/{owner}/{repo}/issues/comments/{comment_id}/reactions`.
    /// The `emoji` parameter is mapped to GitHub's allowed set via
    /// [`emoji_to_github_reaction`].
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not GitHub-encoded"))
        })?;
        let comment_id: u64 = message_id.parse().map_err(|_| {
            AdapterError::InvalidPayload(format!("GitHub comment id {message_id:?} is not numeric"))
        })?;

        let content = emoji_to_github_reaction(emoji);
        let url = self.comment_reactions_url(&decoded.owner, &decoded.repo, comment_id);
        let body = serde_json::json!({ "content": content });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.token())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
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

    /// GitHub has no typing indicator. 1:1 with upstream's
    /// `startTyping` which is a no-op.
    async fn start_typing(
        &self,
        _thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        Ok(())
    }
}

/// Map an SDK emoji name to a GitHub reaction `content` value.
/// 1:1 port of upstream's `emojiToGitHubReaction(emoji)` mapping
/// (with the same `+1` fallback for unknown emoji).
pub fn emoji_to_github_reaction(emoji: &str) -> &'static str {
    match emoji {
        "thumbs_up" | "+1" => "+1",
        "thumbs_down" | "-1" => "-1",
        "laugh" | "smile" => "laugh",
        "confused" | "thinking" => "confused",
        "heart" | "love_eyes" => "heart",
        "hooray" | "party" | "confetti" => "hooray",
        "rocket" => "rocket",
        "eyes" => "eyes",
        _ => "+1",
    }
}

/// Encode a GitHub thread id. 1:1 with upstream's inline format:
/// `github:<owner>/<repo>:<issue-or-pr-number>`.
pub fn encode_thread_id(owner: &str, repo: &str, number: u64) -> String {
    format!("{THREAD_ID_PREFIX}{owner}/{repo}:{number}")
}

/// Components of a decoded GitHub thread id. 1:1 with upstream's
/// returned object shape from `decodeThreadId(threadId)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedGithubThreadId {
    /// Repository owner (org or user).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Issue or PR number.
    pub number: u64,
}

/// Decode a GitHub thread id. 1:1 port of upstream
/// `decodeThreadId(threadId)`. Returns `None` for any value that
/// doesn't carry the `github:` prefix, doesn't include `owner/repo`,
/// or whose number can't be parsed as a positive integer.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedGithubThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let repo_path = parts.next()?;
    let number_str = parts.next()?;
    let mut repo_parts = repo_path.splitn(2, '/');
    let owner = repo_parts.next()?;
    let repo = repo_parts.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let number: u64 = number_str.parse().ok()?;
    Some(DecodedGithubThreadId {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

/// Predicate: does this thread id belong to the GitHub adapter?
/// 1:1 with upstream's inline `threadId.startsWith("github:")`.
pub fn is_github_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_github() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("test-token"));
        assert_eq!(adapter.name(), "github");
        assert_eq!(ADAPTER_NAME, "github");
    }

    #[test]
    fn options_new_stores_token_and_defaults_api_base() {
        let opts = GithubAdapterOptions::new("ghp_xxx");
        assert_eq!(opts.token(), Some("ghp_xxx"));
        assert_eq!(opts.effective_api_base(), DEFAULT_API_BASE);
    }

    #[test]
    fn options_with_api_base_overrides_the_default() {
        let opts =
            GithubAdapterOptions::new("t").with_api_base("https://github.example.com/api/v3");
        assert_eq!(
            opts.effective_api_base(),
            "https://github.example.com/api/v3"
        );
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(
            encode_thread_id("vercel", "chat", 42),
            "github:vercel/chat:42"
        );
        assert_eq!(
            encode_thread_id("andymac4182", "ai-sdk-rust", 1),
            "github:andymac4182/ai-sdk-rust:1"
        );
    }

    #[test]
    fn decode_thread_id_parses_owner_repo_and_number() {
        let decoded = decode_thread_id("github:vercel/chat:42").unwrap();
        assert_eq!(decoded.owner, "vercel");
        assert_eq!(decoded.repo, "chat");
        assert_eq!(decoded.number, 42);
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("slack:C123:1.0").is_none());
        assert!(decode_thread_id("telegram:123").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_owner_or_repo() {
        // No slash in the repo path.
        assert!(decode_thread_id("github:repo-only:1").is_none());
        // Empty owner.
        assert!(decode_thread_id("github:/repo:1").is_none());
        // Empty repo.
        assert!(decode_thread_id("github:owner/:1").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_non_integer_number() {
        assert!(decode_thread_id("github:vercel/chat:abc").is_none());
        // GitHub numbers are positive integers; negative parses as error.
        assert!(decode_thread_id("github:vercel/chat:-1").is_none());
    }

    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`
    // (collapses to `github:<owner>/<repo>`) and `adapter.isDM(_) ->
    // false` (GitHub conversations are always issue/PR threads).

    #[test]
    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    fn channel_id_from_thread_id_strips_the_number_suffix() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("test-token"));
        assert_eq!(
            adapter
                .channel_id_from_thread_id("github:vercel/chat:42")
                .as_deref(),
            Some("github:vercel/chat")
        );
        assert_eq!(
            adapter
                .channel_id_from_thread_id("github:a/b:1")
                .as_deref(),
            Some("github:a/b")
        );
    }

    #[test]
    fn channel_id_from_thread_id_derives_channel_for_pr_level_thread() {
        // 1:1 with upstream `channelIdFromThreadId > should derive
        // channel ID from PR-level thread`.
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        assert_eq!(
            adapter
                .channel_id_from_thread_id("github:acme/app:42")
                .as_deref(),
            Some("github:acme/app")
        );
    }

    #[test]
    fn channel_id_from_thread_id_derives_channel_for_review_comment_thread() {
        // 1:1 with upstream `channelIdFromThreadId > should derive
        // channel ID from review comment thread`. The Rust port now
        // walks the colon-segments instead of delegating to
        // `decode_thread_id` (which only handles 2-segment forms);
        // both 3- and 5-segment thread ids collapse to the same
        // channel id.
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        assert_eq!(
            adapter
                .channel_id_from_thread_id("github:acme/app:42:rc:200")
                .as_deref(),
            Some("github:acme/app")
        );
    }

    #[test]
    fn channel_id_from_thread_id_returns_none_for_non_github_ids() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        assert!(adapter.channel_id_from_thread_id("gitlab:foo/bar:1").is_none());
        assert!(adapter.channel_id_from_thread_id("").is_none());
    }

    #[test]
    fn is_dm_always_returns_false() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        assert!(!adapter.is_dm("github:vercel/chat:42"));
        assert!(!adapter.is_dm(""));
    }

    #[test]
    fn is_github_thread_id_detects_the_prefix() {
        assert!(is_github_thread_id("github:owner/repo:1"));
        assert!(!is_github_thread_id("gitlab:owner/repo:1"));
        assert!(!is_github_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip_preserves_components() {
        for (owner, repo, n) in [
            ("vercel", "chat", 1u64),
            ("andymac4182", "ai-sdk-rust", 9999),
            ("a", "b", 0),
        ] {
            let encoded = encode_thread_id(owner, repo, n);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.owner, owner);
            assert_eq!(decoded.repo, repo);
            assert_eq!(decoded.number, n);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_github_thread_ids() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GitHub-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_comments_url_builds_the_upstream_endpoint() {
        let adapter = GithubAdapter::new(
            GithubAdapterOptions::new("t").with_api_base("https://api.github.example"),
        );
        assert_eq!(
            adapter.comments_url("vercel", "chat", 42),
            "https://api.github.example/repos/vercel/chat/issues/42/comments"
        );
    }

    #[test]
    fn adapter_issue_url_builds_the_upstream_endpoint() {
        let adapter = GithubAdapter::new(
            GithubAdapterOptions::new("t").with_api_base("https://api.github.example"),
        );
        assert_eq!(
            adapter.issue_url("vercel", "chat", 42),
            "https://api.github.example/repos/vercel/chat/issues/42"
        );
    }

    #[test]
    fn adapter_fetch_subject_rejects_non_github_thread_ids() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.fetch_subject("slack:C1:1.0"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GitHub-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_github_thread_ids() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("slack:C1:1.0", "42", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GitHub-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_numeric_comment_id() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("github:vercel/chat:42", "abc", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not numeric"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_rejects_non_github_thread_ids() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("slack:C1:1.0", "42"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GitHub-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_github_thread_ids() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("slack:C1:1.0", "42", "thumbs_up"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not GitHub-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_is_a_noop() {
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        // GitHub doesn't support typing indicators - upstream returns
        // void unconditionally and never touches the network.
        assert!(block_on(adapter.start_typing("github:vercel/chat:1", None)).is_ok());
        assert!(block_on(adapter.start_typing("anything", Some("status"))).is_ok());
    }

    #[test]
    fn emoji_to_github_reaction_maps_well_known_names() {
        assert_eq!(emoji_to_github_reaction("thumbs_up"), "+1");
        assert_eq!(emoji_to_github_reaction("+1"), "+1");
        assert_eq!(emoji_to_github_reaction("thumbs_down"), "-1");
        assert_eq!(emoji_to_github_reaction("laugh"), "laugh");
        assert_eq!(emoji_to_github_reaction("smile"), "laugh");
        assert_eq!(emoji_to_github_reaction("confused"), "confused");
        assert_eq!(emoji_to_github_reaction("thinking"), "confused");
        assert_eq!(emoji_to_github_reaction("heart"), "heart");
        assert_eq!(emoji_to_github_reaction("love_eyes"), "heart");
        assert_eq!(emoji_to_github_reaction("hooray"), "hooray");
        assert_eq!(emoji_to_github_reaction("party"), "hooray");
        assert_eq!(emoji_to_github_reaction("confetti"), "hooray");
        assert_eq!(emoji_to_github_reaction("rocket"), "rocket");
        assert_eq!(emoji_to_github_reaction("eyes"), "eyes");
        // Unknown maps to +1 (upstream fallback).
        assert_eq!(emoji_to_github_reaction("anything_else"), "+1");
    }

    #[test]
    fn adapter_comment_url_template() {
        let adapter = GithubAdapter::new(
            GithubAdapterOptions::new("t").with_api_base("https://api.example.test"),
        );
        assert_eq!(
            adapter.comment_url("vercel", "chat", 99),
            "https://api.example.test/repos/vercel/chat/issues/comments/99"
        );
        assert_eq!(
            adapter.comment_reactions_url("vercel", "chat", 99),
            "https://api.example.test/repos/vercel/chat/issues/comments/99/reactions"
        );
    }

    #[test]
    fn adapter_token_and_api_base_accessors() {
        let adapter = GithubAdapter::new(
            GithubAdapterOptions::new("ghp_xxx").with_api_base("https://github.example.com"),
        );
        assert_eq!(adapter.token(), "ghp_xxx");
        assert_eq!(adapter.api_base(), "https://github.example.com");
    }

    #[test]
    fn decode_thread_id_handles_repo_names_with_dots_and_dashes() {
        let decoded = decode_thread_id("github:vercel/next.js:1").unwrap();
        assert_eq!(decoded.repo, "next.js");
        let decoded = decode_thread_id("github:vercel/chat-sdk-rust:1").unwrap();
        assert_eq!(decoded.repo, "chat-sdk-rust");
    }

    // ---------- constructor describe block (5 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("constructor")`.
    // Tests construct the adapter via the various auth shapes and
    // assert `name`/`userName`/`isMultiTenant`/`botUserId` getters.

    #[test]
    fn constructor_creates_adapter_with_pat_config() {
        let opts = GithubAdapterOptions {
            auth: GithubAuth::Token("ghp_abc".to_string()),
            api_base: None,
            webhook_secret: Some("secret".to_string()),
            user_name: Some("bot".to_string()),
            bot_user_id: None,
        };
        let a = GithubAdapter::new(opts);
        assert_eq!(a.name(), "github");
        assert_eq!(a.user_name(), Some("bot"));
        assert!(!a.is_multi_tenant());
    }

    #[test]
    fn constructor_creates_adapter_with_app_plus_installation_id_single_tenant() {
        let opts = GithubAdapterOptions {
            auth: GithubAuth::App(GithubAppCredentials {
                app_id: "12345".to_string(),
                private_key:
                    "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
                        .to_string(),
                installation_id: Some(99),
            }),
            api_base: None,
            webhook_secret: Some("secret".to_string()),
            user_name: Some("my-bot[bot]".to_string()),
            bot_user_id: None,
        };
        let a = GithubAdapter::new(opts);
        assert!(!a.is_multi_tenant());
    }

    #[test]
    fn constructor_creates_adapter_in_multi_tenant_mode_when_app_without_installation_id() {
        let opts = GithubAdapterOptions {
            auth: GithubAuth::App(GithubAppCredentials {
                app_id: "12345".to_string(),
                private_key:
                    "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
                        .to_string(),
                installation_id: None,
            }),
            api_base: None,
            webhook_secret: Some("secret".to_string()),
            user_name: Some("my-bot[bot]".to_string()),
            bot_user_id: None,
        };
        let a = GithubAdapter::new(opts);
        assert!(a.is_multi_tenant());
    }

    #[test]
    fn constructor_sets_bot_user_id_when_provided_in_config() {
        // Upstream stores numeric `botUserId` config as a string;
        // the Rust port models it as `Option<String>` directly.
        let opts = GithubAdapterOptions {
            auth: GithubAuth::Token("ghp_abc".to_string()),
            api_base: None,
            webhook_secret: Some("secret".to_string()),
            user_name: Some("bot".to_string()),
            bot_user_id: Some("42".to_string()),
        };
        let a = GithubAdapter::new(opts);
        assert_eq!(a.bot_user_id(), Some("42"));
    }

    #[test]
    fn constructor_returns_none_bot_user_id_when_not_provided() {
        let opts = GithubAdapterOptions::new("ghp_abc");
        let a = GithubAdapter::new(opts);
        assert!(a.bot_user_id().is_none());
    }
}
