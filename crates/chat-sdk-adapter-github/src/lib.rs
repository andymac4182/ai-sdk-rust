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

/// Options for [`GithubAdapter::new`]. 1:1 with upstream
/// `interface GithubAdapterOptions`.
#[derive(Debug, Clone)]
pub struct GithubAdapterOptions {
    /// GitHub personal access token (PAT) or installation token.
    pub token: String,
    /// Optional API base URL override (defaults to
    /// [`DEFAULT_API_BASE`]). Used by GitHub Enterprise installations
    /// and tests.
    pub api_base: Option<String>,
}

impl GithubAdapterOptions {
    /// Construct options with a token; base URL defaults to
    /// [`DEFAULT_API_BASE`].
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
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
    /// 1:1 port of upstream `new GithubAdapter({ token, apiBase? })`.
    pub fn new(options: GithubAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
        }
    }

    /// Override the HTTP client (mostly useful for tests).
    pub fn with_http_client(
        mut self,
        client: chat_sdk_adapter_shared::runtime::reqwest::Client,
    ) -> Self {
        self.http = client;
        self
    }

    /// Read the auth token.
    pub fn token(&self) -> &str {
        &self.options.token
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
        assert_eq!(opts.token, "ghp_xxx");
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
}
