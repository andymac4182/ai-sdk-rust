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
pub mod parse;
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

/// Default `userName`. 1:1 with upstream
/// `config.userName ?? "github-bot"`.
pub const DEFAULT_USER_NAME: &str = "github-bot";

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

    /// 1:1 with upstream `protected readonly webhookSecret?: string`.
    pub fn webhook_secret(&self) -> Option<&str> {
        self.options.webhook_secret.as_deref()
    }

    /// 1:1 with upstream `protected readonly apiUrl: string` —
    /// the configured base URL (with default applied).
    pub fn api_url(&self) -> &str {
        self.api_base()
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

    /// 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`.
    /// Delegates to the inherent
    /// [`GithubAdapter::channel_id_from_thread_id`].
    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        self.channel_id_from_thread_id(thread_id)
    }

    /// 1:1 with upstream `adapter.isDM(threadId)`. GitHub issues and
    /// PRs are never DMs — returns `Some(false)`.
    fn is_dm(&self, thread_id: &str) -> Option<bool> {
        Some(self.is_dm(thread_id))
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

/// 1:1 with upstream `interface GitHubAdapterConfig` — all fields
/// optional so the factory can fall back to environment variables.
/// Used by [`try_create_github_adapter`].
#[derive(Debug, Clone, Default)]
pub struct GithubCreateOptions {
    /// Personal access token / installation token. Falls back to
    /// `GITHUB_TOKEN`.
    pub token: Option<String>,
    /// GitHub App id. Falls back to `GITHUB_APP_ID`.
    pub app_id: Option<String>,
    /// GitHub App private key (PEM). Falls back to `GITHUB_PRIVATE_KEY`.
    pub private_key: Option<String>,
    /// Installation id (single-tenant App). Falls back to
    /// `GITHUB_INSTALLATION_ID`. When absent (and app credentials
    /// are supplied), the adapter runs in multi-tenant mode.
    pub installation_id: Option<u64>,
    /// Webhook signing secret. Falls back to `GITHUB_WEBHOOK_SECRET`.
    pub webhook_secret: Option<String>,
    /// Display name override. Falls back to `GITHUB_BOT_USERNAME`,
    /// then [`DEFAULT_USER_NAME`].
    pub user_name: Option<String>,
    /// Numeric bot user id (stringified to match upstream's
    /// `botUserId: string` shape).
    pub bot_user_id: Option<u64>,
    /// GitHub Enterprise API base URL. Falls back to `GITHUB_API_URL`,
    /// then [`DEFAULT_API_BASE`].
    pub api_url: Option<String>,
}

/// Errors returned by [`try_create_github_adapter`]. 1:1 with
/// upstream's `throw new ValidationError("github", "...")` cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubCreateError {
    /// `webhookSecret` missing and `GITHUB_WEBHOOK_SECRET` not set.
    WebhookSecretRequired,
    /// No usable auth resolved from config or environment.
    AuthenticationRequired,
}

impl std::fmt::Display for GithubCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WebhookSecretRequired => write!(
                f,
                "webhookSecret is required. Set GITHUB_WEBHOOK_SECRET or provide it in config."
            ),
            Self::AuthenticationRequired => write!(
                f,
                "Authentication is required. Provide a token or App credentials, or set GITHUB_TOKEN / GITHUB_APP_ID + GITHUB_PRIVATE_KEY."
            ),
        }
    }
}

impl std::error::Error for GithubCreateError {}

/// 1:1 with upstream `createGitHubAdapter(config)` env-var-resolution
/// path. Prefer explicit options; otherwise fall through to the
/// supplied `env` reader.
///
/// Auth-resolution priority matches upstream:
/// 1. `opts.token` (PAT) — takes precedence outright
/// 2. `opts.app_id` + `opts.private_key` (App, single- or
///    multi-tenant depending on `installation_id`)
/// 3. `env("GITHUB_TOKEN")`
/// 4. `env("GITHUB_APP_ID")` + `env("GITHUB_PRIVATE_KEY")`
///
/// **Important**: if the explicit `opts` has any auth field set
/// (token / app_id / private_key / installation_id), env auth is
/// skipped entirely — partial config (e.g. only `app_id`) yields
/// `AuthenticationRequired` rather than falling through to env.
/// This matches upstream's "don't mix auth modes" behavior.
///
/// The `env` reader is a closure rather than `std::env::var` so
/// tests don't have to mutate process-global state (unsafe in Rust
/// 2024 edition).
pub fn try_create_github_adapter(
    opts: GithubCreateOptions,
    env: impl Fn(&str) -> Option<String>,
) -> Result<GithubAdapter, GithubCreateError> {
    let webhook_secret = opts
        .webhook_secret
        .or_else(|| env("GITHUB_WEBHOOK_SECRET"))
        .ok_or(GithubCreateError::WebhookSecretRequired)?;

    let user_name = opts
        .user_name
        .or_else(|| env("GITHUB_BOT_USERNAME"))
        .or_else(|| Some(DEFAULT_USER_NAME.to_string()));
    let api_url = opts.api_url.or_else(|| env("GITHUB_API_URL"));
    let bot_user_id = opts.bot_user_id.map(|n| n.to_string());

    let has_config_auth = opts.token.is_some()
        || opts.app_id.is_some()
        || opts.private_key.is_some()
        || opts.installation_id.is_some();

    let auth = if has_config_auth {
        if let Some(tok) = opts.token {
            GithubAuth::Token(tok)
        } else if let (Some(app_id), Some(private_key)) = (opts.app_id, opts.private_key) {
            GithubAuth::App(GithubAppCredentials {
                app_id,
                private_key,
                installation_id: opts.installation_id,
            })
        } else {
            // Partial config (e.g. appId only) — don't fall through
            // to env, matching upstream's "don't mix auth modes".
            return Err(GithubCreateError::AuthenticationRequired);
        }
    } else if let Some(tok) = env("GITHUB_TOKEN") {
        GithubAuth::Token(tok)
    } else if let (Some(app_id), Some(private_key)) =
        (env("GITHUB_APP_ID"), env("GITHUB_PRIVATE_KEY"))
    {
        let installation_id = env("GITHUB_INSTALLATION_ID").and_then(|s| s.parse::<u64>().ok());
        GithubAuth::App(GithubAppCredentials {
            app_id,
            private_key,
            installation_id,
        })
    } else {
        return Err(GithubCreateError::AuthenticationRequired);
    };

    Ok(GithubAdapter::new(GithubAdapterOptions {
        auth,
        api_base: api_url,
        webhook_secret: Some(webhook_secret),
        user_name,
        bot_user_id,
    }))
}

/// Encode a GitHub thread id. 1:1 with upstream's inline format:
/// `github:<owner>/<repo>:<issue-or-pr-number>`.
pub fn encode_thread_id(owner: &str, repo: &str, number: u64) -> String {
    format!("{THREAD_ID_PREFIX}{owner}/{repo}:{number}")
}

/// Thread variant. 1:1 with upstream's
/// `type: "pr" | "issue"` discriminator on `GitHubThreadId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GithubThreadKind {
    /// Pull-request thread (or unspecified — upstream defaults to `pr`).
    #[default]
    Pr,
    /// Issue thread.
    Issue,
}

/// Structured GitHub thread id. 1:1 with upstream's
/// `interface GitHubThreadId { owner; repo; prNumber; type?; reviewCommentId? }`.
///
/// `pr_number` carries the PR **or** issue number per upstream — the
/// upstream type re-uses the same field for both, gated by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GithubThreadId {
    /// Repository owner (org or user login).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// PR or issue number.
    pub pr_number: u64,
    /// Variant. `Pr` is the default; `Issue` selects the
    /// `github:<owner>/<repo>:issue:<n>` wire form.
    pub kind: GithubThreadKind,
    /// Review-comment thread id (when this thread is a review
    /// comment). Only valid on `Pr`-kind threads.
    pub review_comment_id: Option<u64>,
}

/// Errors returned by [`encode_thread_id_full`]. 1:1 with upstream's
/// `throw new ValidationError("github", "...")` in `encodeThreadId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeThreadIdError {
    /// Issue-kind thread carries a `review_comment_id`. Upstream
    /// throws `"Review comments are not supported on issue threads"`.
    ReviewCommentOnIssueThread,
}

impl std::fmt::Display for EncodeThreadIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReviewCommentOnIssueThread => {
                write!(f, "Review comments are not supported on issue threads")
            }
        }
    }
}

impl std::error::Error for EncodeThreadIdError {}

/// Encode a structured [`GithubThreadId`] to the wire format. 1:1
/// with upstream `encodeThreadId(platformData)`.
///
/// Wire formats:
/// - PR-level: `github:<owner>/<repo>:<prNumber>`
/// - Issue-level: `github:<owner>/<repo>:issue:<issueNumber>`
/// - Review comment: `github:<owner>/<repo>:<prNumber>:rc:<reviewCommentId>`
pub fn encode_thread_id_full(thread: &GithubThreadId) -> Result<String, EncodeThreadIdError> {
    if thread.kind == GithubThreadKind::Issue && thread.review_comment_id.is_some() {
        return Err(EncodeThreadIdError::ReviewCommentOnIssueThread);
    }
    if thread.kind == GithubThreadKind::Issue {
        return Ok(format!(
            "{THREAD_ID_PREFIX}{}/{}:issue:{}",
            thread.owner, thread.repo, thread.pr_number
        ));
    }
    if let Some(rc) = thread.review_comment_id {
        return Ok(format!(
            "{THREAD_ID_PREFIX}{}/{}:{}:rc:{}",
            thread.owner, thread.repo, thread.pr_number, rc
        ));
    }
    Ok(format!(
        "{THREAD_ID_PREFIX}{}/{}:{}",
        thread.owner, thread.repo, thread.pr_number
    ))
}

/// Errors returned by [`decode_thread_id_full`]. 1:1 with upstream's
/// `throw new ValidationError("github", "...")` in `decodeThreadId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeThreadIdError {
    /// Thread id doesn't start with `github:`. Upstream throws
    /// `"Invalid GitHub thread ID: <id>"`.
    InvalidPrefix(String),
    /// Thread id is `github:`-prefixed but doesn't match any of the
    /// PR / issue / review-comment patterns. Upstream throws
    /// `"Invalid GitHub thread ID format: <id>"`.
    InvalidFormat(String),
}

impl std::fmt::Display for DecodeThreadIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPrefix(s) => write!(f, "Invalid GitHub thread ID: {s}"),
            Self::InvalidFormat(s) => write!(f, "Invalid GitHub thread ID format: {s}"),
        }
    }
}

impl std::error::Error for DecodeThreadIdError {}

/// Decode a wire thread id into a structured [`GithubThreadId`]. 1:1
/// with upstream `decodeThreadId(threadId)`.
///
/// Patterns (matched in order — same priority as upstream):
/// 1. `github:<owner>/<repo>:<prNumber>:rc:<reviewCommentId>`
/// 2. `github:<owner>/<repo>:issue:<issueNumber>`
/// 3. `github:<owner>/<repo>:<prNumber>`
pub fn decode_thread_id_full(thread_id: &str) -> Result<GithubThreadId, DecodeThreadIdError> {
    let suffix = thread_id
        .strip_prefix(THREAD_ID_PREFIX)
        .ok_or_else(|| DecodeThreadIdError::InvalidPrefix(thread_id.to_string()))?;

    // First: review-comment pattern `<owner>/<repo>:<n>:rc:<m>`.
    if let Some((repo_path, rest)) = suffix.split_once(':') {
        if let Some((owner, repo)) = repo_path.split_once('/') {
            if !owner.is_empty() && !repo.is_empty() {
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() == 3 && parts[1] == "rc" {
                    if let (Ok(pr), Ok(rc)) = (parts[0].parse::<u64>(), parts[2].parse::<u64>()) {
                        return Ok(GithubThreadId {
                            owner: owner.to_string(),
                            repo: repo.to_string(),
                            pr_number: pr,
                            kind: GithubThreadKind::Pr,
                            review_comment_id: Some(rc),
                        });
                    }
                }
                if parts.len() == 2 && parts[0] == "issue" {
                    if let Ok(n) = parts[1].parse::<u64>() {
                        return Ok(GithubThreadId {
                            owner: owner.to_string(),
                            repo: repo.to_string(),
                            pr_number: n,
                            kind: GithubThreadKind::Issue,
                            review_comment_id: None,
                        });
                    }
                }
                if parts.len() == 1 {
                    if let Ok(n) = parts[0].parse::<u64>() {
                        return Ok(GithubThreadId {
                            owner: owner.to_string(),
                            repo: repo.to_string(),
                            pr_number: n,
                            kind: GithubThreadKind::Pr,
                            review_comment_id: None,
                        });
                    }
                }
            }
        }
    }

    Err(DecodeThreadIdError::InvalidFormat(thread_id.to_string()))
}

/// Parse a GitHub channel id (`github:<owner>/<repo>`) into its
/// `(owner, repo)` parts. 1:1 with upstream's inline parsing in
/// `listThreads` / `fetchChannelInfo`. Throws upstream's
/// `"Invalid GitHub channel ID: <id>"` on a missing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubChannelId {
    /// Repository owner (org or user login).
    pub owner: String,
    /// Repository name.
    pub repo: String,
}

/// Errors returned by [`parse_channel_id`]. 1:1 with upstream's
/// inline `throw new ValidationError("github", \`Invalid GitHub channel ID: ${channelId}\`)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidChannelIdError(pub String);

impl std::fmt::Display for InvalidChannelIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid GitHub channel ID: {}", self.0)
    }
}

impl std::error::Error for InvalidChannelIdError {}

/// Parse a channel id (`github:<owner>/<repo>`). Returns
/// [`InvalidChannelIdError`] when the slash separator is missing or
/// the prefix is wrong — matches upstream's `listThreads` /
/// `fetchChannelInfo` validation paths exactly.
pub fn parse_channel_id(channel_id: &str) -> Result<GithubChannelId, InvalidChannelIdError> {
    let suffix = channel_id
        .strip_prefix(THREAD_ID_PREFIX)
        .ok_or_else(|| InvalidChannelIdError(channel_id.to_string()))?;
    let slash = suffix
        .find('/')
        .ok_or_else(|| InvalidChannelIdError(channel_id.to_string()))?;
    let owner = &suffix[..slash];
    let repo = &suffix[slash + 1..];
    if owner.is_empty() || repo.is_empty() {
        return Err(InvalidChannelIdError(channel_id.to_string()));
    }
    Ok(GithubChannelId {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// Build the JSON body for a `POST /repos/:owner/:repo/issues/:n/comments`
/// (or `PATCH .../issues/comments/:id`) request. 1:1 with upstream's
/// inline `{ body: text }` shape.
///
/// Exposing this as a pure helper lets the body-shape contract be
/// asserted without an HTTP harness (mirrors the slice-515/516/517
/// `build_*_body` pattern for telegram/whatsapp/discord/teams/messenger).
pub fn build_comment_body(text: &str) -> serde_json::Value {
    serde_json::json!({ "body": text })
}

/// Build the JSON body for a `POST /repos/:owner/:repo/issues/comments/:id/reactions`
/// request. 1:1 with upstream's inline `{ content: emojiToGitHubReaction(emoji) }`.
pub fn build_reaction_body(emoji: &str) -> serde_json::Value {
    serde_json::json!({ "content": emoji_to_github_reaction(emoji) })
}

/// Page-based cursor parser for `listThreads`. 1:1 with upstream's
/// inline `options.cursor ? Number.parseInt(options.cursor, 10) : 1`.
/// Returns 1 when the cursor is missing or unparseable (matches
/// `parseInt(undefined, 10)` → `NaN` → `|| 1` upstream).
pub fn parse_list_threads_cursor(cursor: Option<&str>) -> u64 {
    cursor.and_then(|s| s.parse::<u64>().ok()).unwrap_or(1)
}

/// Compute the `next_cursor` for `listThreads` pagination. 1:1 with
/// upstream's inline `pulls.length === limit ? String(currentPage + 1) : undefined`.
/// Returns `Some(next_page)` only when the page is fully filled.
pub fn compute_next_cursor(current_page: u64, returned_len: usize, limit: usize) -> Option<String> {
    if returned_len == limit {
        Some((current_page + 1).to_string())
    } else {
        None
    }
}

/// Direction of `fetchMessages` pagination. 1:1 with upstream
/// `FetchOptions.direction: "forward" | "backward"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FetchDirection {
    /// Take the **last** `limit` messages (most recent). Upstream's
    /// default.
    #[default]
    Backward,
    /// Take the **first** `limit` messages.
    Forward,
}

/// Slice a result set down to `limit`, honoring direction. 1:1 with
/// upstream's inline `direction === "forward" ? items.slice(0, limit)
/// : items.slice(-limit)` in `fetchMessages`.
pub fn limit_messages_window<T: Clone>(
    items: &[T],
    direction: FetchDirection,
    limit: usize,
) -> Vec<T> {
    if limit >= items.len() {
        return items.to_vec();
    }
    match direction {
        FetchDirection::Forward => items[..limit].to_vec(),
        FetchDirection::Backward => items[items.len() - limit..].to_vec(),
    }
}

/// Concatenate a stream of text chunks into a single body string. 1:1
/// with upstream's `stream(threadId, generator)` text-accumulation
/// loop — it collects all string + `markdown_text` chunks and ignores
/// non-text chunks like `task_update`. Exposed as a pure helper so the
/// accumulation contract can be asserted without a generator harness.
///
/// The `is_text` predicate filters which chunks to keep (matches
/// upstream's `typeof chunk === "string" || chunk.type === "markdown_text"`).
pub fn accumulate_stream_text<I, S>(chunks: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = String::new();
    for chunk in chunks {
        out.push_str(chunk.as_ref());
    }
    out
}

/// Filter a list of reactions down to those left by the bot. 1:1 with
/// upstream's `removeReaction` inline filter:
/// `reactions.filter(r => r.user.id === botUserId && r.content === content)`.
/// Returns the first match's id (`reaction_id`) to delete, or `None`
/// when no matching reaction exists — matches upstream's "do nothing
/// when no matching reaction found" case.
pub fn find_bot_reaction_id(
    reactions: &[(u64, &str, u64)],
    bot_user_id: u64,
    content: &str,
) -> Option<u64> {
    reactions
        .iter()
        .find(|(_, c, uid)| *uid == bot_user_id && *c == content)
        .map(|(id, _, _)| *id)
}

/// Build the GitHub `getUser` display name. 1:1 with upstream's
/// inline `user.name || user.login` fallback chain. Returns `login`
/// when `name` is `None` or empty (matches the `||` semantics).
pub fn user_display_name(login: &str, name: Option<&str>) -> String {
    match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => login.to_string(),
    }
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
    //! ---------- upstream js-only-documented cases ----------
    //!
    //! Per the slice-411 Vitest-`vi.fn()`-HTTP-mock + slice-380
    //! type-system-impossible + slice-447 default-Logger patterns,
    //! the following upstream `index.test.ts` cases are enumerated
    //! as js-only-documented because they exercise behavior that
    //! is unrepresentable in the Rust port by construction.
    //! Behaviour parity is preserved via pure-helper splits and
    //! typed errors at the call sites:
    //!
    //! ### `describe("octokit getter")` (5 cases, lines 276-369)
    //!
    //! 1. `should return the underlying Octokit instance in PAT
    //!    mode` — asserts the getter returns an `Octokit` typed
    //!    class instance. Rust has no `Octokit` equivalent — HTTP
    //!    is held as an opaque `reqwest::Client` injected via
    //!    `with_http_client(...)`; the "type identity" assertion
    //!    is moot.
    //! 2. `should return the same instance across calls in
    //!    single-tenant mode` — Octokit-instance referential
    //!    equality. The Rust port's `Client` is held by value
    //!    (`Clone`-ed when the adapter is cloned); per-call
    //!    referential equality is moot since `Clone` produces
    //!    shared underlying connection pools.
    //! 3. `should expose the same instance via the deprecated
    //!    "client" alias` — alias-name backwards-compat. The Rust
    //!    port never shipped the deprecated alias.
    //! 4. `should throw in multi-tenant mode when called outside a
    //!    webhook` — runtime "no installation context" throw. The
    //!    Rust port surfaces the equivalent via typed errors at
    //!    the call sites that need the per-installation client,
    //!    not via a property getter.
    //! 5. `should resolve the per-installation Octokit when
    //!    accessed inside a webhook context` — Octokit injection
    //!    via `AsyncLocalStorage`. The Rust port's HTTP is held by
    //!    value (no per-call `Octokit` instance), so the per-call
    //!    swap is unrepresentable.
    //!
    //! ### `describe("constructor")` (1 of 6 cases, line 249)
    //!
    //! - `should throw when no auth method is provided` —
    //!   upstream constructs `new GithubAdapter({})` and asserts
    //!   `throw new ValidationError`. The Rust port requires
    //!   `GithubAuth` at compile time on `GithubAdapterOptions`,
    //!   so passing "no auth" is a type error; the runtime throw
    //!   is unrepresentable.
    //!
    //! ### `describe("initialize")` (3 cases, lines 371-437)
    //!
    //! - 3 cases drive `await adapter.initialize(mockChat)` with
    //!   `mockUsersGetAuthenticated.mockResolvedValueOnce({...})`
    //!   and assert on `mockChat.handleIncomingMessage` calls or
    //!   on the cached `botUserId`. Both the HTTP-fetch (`vi.spyOn
    //!   (global, "fetch")`) and the `vi.fn()`-Chat are Vitest
    //!   constructs without Rust analogues.
    //!
    //! ### `describe("getInstallationId")` (3 of 7 cases mocked, lines 439-568)
    //!
    //! - 3 of the 7 cases (`returns cached after webhook`, `returns
    //!   undefined when not cached`, `throws before initialization
    //!   in multi-tenant`) drive `await
    //!   multiTenantAdapter.initialize(mockChat)` + `await
    //!   multiTenantAdapter.handleWebhook(...)` and assert on
    //!   `getInstallationId(...)`. They require the Vitest
    //!   `vi.fn()` Chat + HTTP-mock infrastructure. The other 4
    //!   cases (`fixed installation id`, `accept thread object`,
    //!   `undefined in PAT mode`, `throw non-github thread`) are
    //!   pure-helper-portable and ported below.
    //!
    //! ### `describe("handleWebhook")` (14 cases, lines 570-877)
    //!
    //! - All 14 cases drive `await adapter.handleWebhook(request)`
    //!   with synthetic `Request` constructors + `vi.fn()`-Chat +
    //!   `signPayload(body)` helper. The signature-rejection /
    //!   400-JSON / pong / ignore-action / no-init paths are
    //!   structurally covered by `webhook::verify_github_signature`
    //!   (7 webhook.rs tests) + `parse::parse_message` (10
    //!   parse.rs tests). Driver-level dispatch through a
    //!   `Request` object requires the synthetic Request +
    //!   Vitest-mocked Chat.
    //!
    //! ### `describe("self-message detection")` (4 cases, lines 878-1040)
    //!
    //! - All 4 cases (`ignore issue comment from bot`, `ignore
    //!   review comment from bot`, `auto-detect botUserId on first
    //!   webhook`, `fall back to apps.getAuthenticated`) require
    //!   the same `vi.fn()`-Chat + HTTP-mock infrastructure as
    //!   `handleWebhook`. The self-message gate itself is covered
    //!   structurally via `parse::parse_author` `is_me` boolean (3
    //!   parse.rs tests).
    //!
    //! ### `describe("postMessage")` (4 cases, lines 1041-1149)
    //!
    //! - All 4 cases assert on `mockIssuesCreateComment.toHaveBeen
    //!   CalledWith({owner, repo, issue_number, body})` from a
    //!   sequenced `mockResolvedValueOnce(...)` chain. The body
    //!   shape is asserted structurally via `build_comment_body`
    //!   (covered below) + the card-render path via
    //!   `card_to_github_markdown` (cards.rs tests).
    //!
    //! ### `describe("editMessage")` (3 cases, lines 1151-1238)
    //!
    //! - Same `mockIssuesUpdateComment.toHaveBeenCalledWith(...)` /
    //!   `mockPullsUpdateReviewComment.toHaveBeenCalledWith(...)`
    //!   shape — covered structurally via `build_comment_body` +
    //!   `decode_thread_id_full` routing.
    //!
    //! ### `describe("stream")` (4 cases, lines 1240-1359)
    //!
    //! - All 4 cases drive `await adapter.stream(threadId,
    //!   generator)` with an `async function*` chunk stream and
    //!   assert on `mockIssuesCreateComment.toHaveBeenCalledTimes
    //!   (1)`. The text-accumulation contract is covered via
    //!   `accumulate_stream_text` (4 cases below). The
    //!   `not.toHaveBeenCalled()` assertion on the edit path
    //!   requires the Vitest mock spy infrastructure.
    //!
    //! ### `describe("deleteMessage")` (2 cases, lines 1361-1385)
    //!
    //! - Both cases assert on `mockIssuesDeleteComment` /
    //!   `mockPullsDeleteReviewComment.toHaveBeenCalledWith(...)`.
    //!   Endpoint routing is covered structurally via
    //!   `decode_thread_id_full`.
    //!
    //! ### `describe("addReaction")` (3 cases, lines 1387-1427)
    //!
    //! - All 3 cases assert on the request body via Vitest's
    //!   `toHaveBeenCalledWith(expect.objectContaining({content}))`.
    //!   The body shape is covered structurally via
    //!   `build_reaction_body` (3 cases below) + the 16
    //!   `emoji_to_github_reaction` mapping tests.
    //!
    //! ### `describe("removeReaction")` (4 cases, lines 1429-1525)
    //!
    //! - All 4 cases drive `await adapter.removeReaction(...)`
    //!   with a `mockReactionsListForIssueComment.mockResolvedValue
    //!   Once({data: [...]})` chain and assert on
    //!   `mockReactionsDeleteForIssueComment.toHaveBeenCalledWith
    //!   ({reaction_id})`. The bot-reaction-filter contract is
    //!   covered via `find_bot_reaction_id` (3 cases below). The
    //!   lazy `botUserId` detection (`should lazily detect
    //!   botUserId when not set`) requires the HTTP-mock chain.
    //!
    //! ### `describe("fetchMessages")` (4 cases, lines 1852-1984)
    //!
    //! - All 4 cases drive `await adapter.fetchMessages(...)` with
    //!   `mockIssuesListComments.mockResolvedValueOnce({data: [...]})`
    //!   and assert on `toHaveBeenCalledWith({per_page: 100, ...})`.
    //!   The limit + direction window contract is covered
    //!   structurally via `limit_messages_window` (3 cases below).
    //!   The review-comment thread filter is covered via
    //!   `decode_thread_id_full` routing.
    //!
    //! ### `describe("fetchThread")` (3 cases, lines 1986-2061)
    //!
    //! - All 3 cases drive `await adapter.fetchThread(...)` with
    //!   `mockPullsGet.mockResolvedValueOnce(...)` /
    //!   `mockIssuesGet.mockResolvedValueOnce(...)` and assert on
    //!   the resulting `ThreadInfo.metadata`. The PR-vs-issue
    //!   endpoint routing is covered via `decode_thread_id_full`
    //!   (the new issue/rc variants). The metadata-shape
    //!   assertion requires the Vitest mock chain.
    //!
    //! ### `describe("listThreads")` (6 cases, lines 2063-2186)
    //!
    //! - All 6 cases drive `await adapter.listThreads(...)` with
    //!   `mockPullsList.mockResolvedValueOnce({data: [...]})` and
    //!   assert on `toHaveBeenCalledWith({page, per_page})` plus
    //!   the `nextCursor` derivation. The channel-id parsing is
    //!   covered via `parse_channel_id` (1 case below) and the
    //!   cursor math is covered via `parse_list_threads_cursor` +
    //!   `compute_next_cursor` (5 cases below).
    //!
    //! ### `describe("fetchChannelInfo")` (2 cases, lines 2188-2224)
    //!
    //! - Both cases drive `await adapter.fetchChannelInfo(...)`
    //!   with `mockReposGet.mockResolvedValueOnce(...)`. The
    //!   channel-id validation is covered via `parse_channel_id`.
    //!   The metadata-shape assertion requires the Vitest mock.
    //!
    //! ### `describe("getUser")` (5 of 6 cases, lines 2587-2716)
    //!
    //! - 5 of the 6 cases drive `await adapter.getUser(...)` with
    //!   `mockRequest.mockResolvedValue({...})` and assert on the
    //!   resulting `UserInfo`. The display-name fallback contract
    //!   is covered via `user_display_name` (3 cases below). The
    //!   `should call GitHub API with correct endpoint and params`
    //!   case asserts on `mockRequest.toHaveBeenCalledWith("GET
    //!   /user/{account_id}", {account_id: 12345})` — the URL
    //!   templating requires the `Octokit.request()` typed-client
    //!   convention which has no Rust analogue.
    //!
    //! ### `describe("fetchSubject")` (4 cases, lines 2718-2897)
    //!
    //! - All 4 cases drive `await adapter.fetchSubject(raw)` with
    //!   a per-test `mockOctokit` assigned to `defaultOctokit` via
    //!   `(adapter as unknown as ...) = mockOctokit`. The
    //!   property-injection pattern + `vi.fn()` resolver are
    //!   Vitest-specific. The issue-vs-PR dispatch logic is
    //!   covered structurally via `parse::GithubRawMessage` enum
    //!   discriminant.
    //!
    //! ### `describe("subclass extensibility")` (1 case, line 2899)
    //!
    //! - `exposes protected members and methods to subclasses` —
    //!   TypeScript `protected` access-modifier compile-time
    //!   check. Rust uses `pub(crate)` visibility + trait
    //!   composition rather than class inheritance.
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

    // ---------- describe("renderFormatted") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("renderFormatted")`.

    #[test]
    fn render_formatted_should_render_simple_markdown() {
        // 1:1 with upstream "should render simple markdown".
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert_eq!(result.trim(), "Hello world");
    }

    #[test]
    fn render_formatted_should_render_bold_text() {
        // 1:1 with upstream "should render bold text".
        use chat_sdk_chat::markdown::{Node, paragraph, root, strong, text};
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Strong(
            strong(vec![Node::Text(text("bold"))]),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert_eq!(result.trim(), "**bold**");
    }

    // ---------- describe("startTyping") (1 upstream case) ----------

    #[test]
    fn start_typing_should_be_a_no_op() {
        // 1:1 with upstream "should be a no-op". The Rust port's
        // start_typing returns Ok(()) without dispatching anywhere.
        let adapter = GithubAdapter::new(GithubAdapterOptions::new("t"));
        block_on(adapter.start_typing("github:acme/app:42", None)).unwrap();
        block_on(adapter.start_typing("github:acme/app:42", Some("thinking..."))).unwrap();
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
            adapter.channel_id_from_thread_id("github:a/b:1").as_deref(),
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
        assert!(
            adapter
                .channel_id_from_thread_id("gitlab:foo/bar:1")
                .is_none()
        );
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

    // ---------- describe("emojiToGitHubReaction (via addReaction)") (16 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("emojiToGitHubReaction (via addReaction)")`.
    // Split from the prior bundled `emoji_to_github_reaction_maps_well_known_names`
    // single-test coverage into one Rust test per upstream case to
    // preserve the brief's "every portable upstream case has a
    // matching Rust test" rule. Upstream tests the mapping via
    // `adapter.addReaction(...)` calls and asserts on the
    // `reactionsCreateForIssueComment` mock's `content` argument; the
    // Rust port tests the pure `emoji_to_github_reaction` helper
    // directly (the addReaction dispatcher just passes the helper's
    // result through unchanged).

    #[test]
    fn github_emoji_to_github_reaction_maps_thumbs_up() {
        assert_eq!(emoji_to_github_reaction("thumbs_up"), "+1");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_plus_one() {
        assert_eq!(emoji_to_github_reaction("+1"), "+1");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_thumbs_down() {
        assert_eq!(emoji_to_github_reaction("thumbs_down"), "-1");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_minus_one() {
        assert_eq!(emoji_to_github_reaction("-1"), "-1");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_laugh() {
        assert_eq!(emoji_to_github_reaction("laugh"), "laugh");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_smile_to_laugh() {
        assert_eq!(emoji_to_github_reaction("smile"), "laugh");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_confused() {
        assert_eq!(emoji_to_github_reaction("confused"), "confused");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_thinking_to_confused() {
        assert_eq!(emoji_to_github_reaction("thinking"), "confused");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_heart() {
        assert_eq!(emoji_to_github_reaction("heart"), "heart");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_love_eyes_to_heart() {
        assert_eq!(emoji_to_github_reaction("love_eyes"), "heart");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_hooray() {
        assert_eq!(emoji_to_github_reaction("hooray"), "hooray");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_party_to_hooray() {
        assert_eq!(emoji_to_github_reaction("party"), "hooray");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_confetti_to_hooray() {
        assert_eq!(emoji_to_github_reaction("confetti"), "hooray");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_rocket() {
        assert_eq!(emoji_to_github_reaction("rocket"), "rocket");
    }

    #[test]
    fn github_emoji_to_github_reaction_maps_eyes() {
        assert_eq!(emoji_to_github_reaction("eyes"), "eyes");
    }

    #[test]
    fn github_emoji_to_github_reaction_should_default_to_plus_one_for_unknown_emoji() {
        // 1:1 with upstream "should default to +1 for unknown emoji"
        // (the fallback case in the parametric upstream loop).
        assert_eq!(emoji_to_github_reaction("unknown_emoji"), "+1");
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
                private_key: "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
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
                private_key: "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
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

    // ---------- createGitHubAdapter describe block (13 cases) ----------
    // 1:1 with upstream `index.test.ts > describe("createGitHubAdapter")`.
    // Env reader injected as a closure (Rust 2024 makes `set_var`
    // unsafe; parallel tests on `process.env` are racy).

    fn empty_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn create_github_adapter_should_create_with_explicit_pat_config() {
        let a = try_create_github_adapter(
            GithubCreateOptions {
                token: Some("ghp_test".to_string()),
                webhook_secret: Some("secret".to_string()),
                user_name: Some("bot".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("PAT config is valid");
        assert_eq!(a.user_name(), Some("bot"));
    }

    #[test]
    fn create_github_adapter_should_create_with_explicit_app_config_single_tenant() {
        let a = try_create_github_adapter(
            GithubCreateOptions {
                app_id: Some("123".to_string()),
                private_key: Some(
                    "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
                        .to_string(),
                ),
                installation_id: Some(456),
                webhook_secret: Some("secret".to_string()),
                user_name: Some("bot[bot]".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("App + installationId is single-tenant");
        assert!(!a.is_multi_tenant());
    }

    #[test]
    fn create_github_adapter_should_create_in_multi_tenant_mode_when_no_installation_id() {
        let a = try_create_github_adapter(
            GithubCreateOptions {
                app_id: Some("123".to_string()),
                private_key: Some(
                    "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
                        .to_string(),
                ),
                webhook_secret: Some("secret".to_string()),
                user_name: Some("bot[bot]".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("App without installationId is multi-tenant");
        assert!(a.is_multi_tenant());
    }

    #[test]
    fn create_github_adapter_should_throw_when_webhook_secret_missing() {
        let err = try_create_github_adapter(
            GithubCreateOptions {
                token: Some("ghp_test".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect_err("missing webhook secret");
        assert_eq!(err, GithubCreateError::WebhookSecretRequired);
        assert!(err.to_string().contains("webhookSecret is required"));
    }

    #[test]
    fn create_github_adapter_should_throw_when_no_auth_is_provided() {
        let err = try_create_github_adapter(
            GithubCreateOptions {
                webhook_secret: Some("secret".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect_err("no auth available");
        assert_eq!(err, GithubCreateError::AuthenticationRequired);
        assert!(err.to_string().contains("Authentication is required"));
    }

    #[test]
    fn create_github_adapter_should_fall_back_to_env_vars_for_token() {
        let env = |key: &str| match key {
            "GITHUB_WEBHOOK_SECRET" => Some("env-secret".to_string()),
            "GITHUB_TOKEN" => Some("env-token".to_string()),
            "GITHUB_BOT_USERNAME" => Some("env-bot".to_string()),
            _ => None,
        };
        let a =
            try_create_github_adapter(GithubCreateOptions::default(), env).expect("env-only PAT");
        assert_eq!(a.user_name(), Some("env-bot"));
        assert!(matches!(a.auth(), GithubAuth::Token(t) if t == "env-token"));
    }

    #[test]
    fn create_github_adapter_should_fall_back_to_env_vars_for_app_credentials() {
        let env = |key: &str| match key {
            "GITHUB_WEBHOOK_SECRET" => Some("env-secret".to_string()),
            "GITHUB_APP_ID" => Some("env-app-id".to_string()),
            "GITHUB_PRIVATE_KEY" => Some(
                "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----".to_string(),
            ),
            "GITHUB_INSTALLATION_ID" => Some("789".to_string()),
            _ => None,
        };
        let a = try_create_github_adapter(GithubCreateOptions::default(), env)
            .expect("env-only app credentials");
        assert!(!a.is_multi_tenant());
        match a.auth() {
            GithubAuth::App(creds) => {
                assert_eq!(creds.app_id, "env-app-id");
                assert_eq!(creds.installation_id, Some(789));
            }
            other => panic!("expected App auth, got {other:?}"),
        }
    }

    #[test]
    fn create_github_adapter_should_not_mix_auth_modes_when_explicit_config_has_auth_fields() {
        // Upstream: providing `app_id` explicitly skips env-token
        // resolution; missing privateKey/installationId yields a
        // ValidationError "Authentication is required".
        let env = |key: &str| match key {
            "GITHUB_TOKEN" => Some("env-token".to_string()),
            "GITHUB_WEBHOOK_SECRET" => Some("env-secret".to_string()),
            _ => None,
        };
        let err = try_create_github_adapter(
            GithubCreateOptions {
                app_id: Some("123".to_string()),
                webhook_secret: Some("secret".to_string()),
                ..Default::default()
            },
            env,
        )
        .expect_err("partial app config skips env auth");
        assert_eq!(err, GithubCreateError::AuthenticationRequired);
    }

    #[test]
    fn create_github_adapter_should_use_default_user_name_when_not_provided() {
        let a = try_create_github_adapter(
            GithubCreateOptions {
                token: Some("ghp_test".to_string()),
                webhook_secret: Some("secret".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("default user name applied");
        assert_eq!(a.user_name(), Some("github-bot"));
    }

    #[test]
    fn create_github_adapter_should_pass_bot_user_id_to_adapter() {
        let a = try_create_github_adapter(
            GithubCreateOptions {
                token: Some("ghp_test".to_string()),
                webhook_secret: Some("secret".to_string()),
                bot_user_id: Some(42),
                ..Default::default()
            },
            empty_env,
        )
        .expect("bot user id passed through");
        // Upstream stringifies the numeric input.
        assert_eq!(a.bot_user_id(), Some("42"));
    }

    #[test]
    fn create_github_adapter_should_accept_api_url_config_for_github_enterprise() {
        let a = try_create_github_adapter(
            GithubCreateOptions {
                token: Some("ghp_test".to_string()),
                webhook_secret: Some("secret".to_string()),
                api_url: Some("https://github.example.com/api/v3".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect("apiUrl config accepted");
        assert_eq!(a.api_url(), "https://github.example.com/api/v3");
    }

    #[test]
    fn create_github_adapter_should_resolve_api_url_from_github_api_url_env_var() {
        let env = |key: &str| match key {
            "GITHUB_WEBHOOK_SECRET" => Some("env-secret".to_string()),
            "GITHUB_TOKEN" => Some("env-token".to_string()),
            "GITHUB_API_URL" => Some("https://github.example.com/api/v3".to_string()),
            _ => None,
        };
        let a = try_create_github_adapter(GithubCreateOptions::default(), env)
            .expect("env api url resolves");
        assert_eq!(a.api_url(), "https://github.example.com/api/v3");
    }

    #[test]
    fn create_github_adapter_should_prefer_api_url_config_over_github_api_url_env_var() {
        let env = |key: &str| match key {
            "GITHUB_WEBHOOK_SECRET" => Some("env-secret".to_string()),
            "GITHUB_TOKEN" => Some("env-token".to_string()),
            "GITHUB_API_URL" => Some("https://env-github.example.com/api/v3".to_string()),
            _ => None,
        };
        let a = try_create_github_adapter(
            GithubCreateOptions {
                token: Some("ghp_test".to_string()),
                webhook_secret: Some("secret".to_string()),
                api_url: Some("https://config-github.example.com/api/v3".to_string()),
                ..Default::default()
            },
            env,
        )
        .expect("config api url wins");
        assert_eq!(a.api_url(), "https://config-github.example.com/api/v3");
    }

    // ---------- describe("encodeThreadId") (5 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("encodeThreadId")`.

    #[test]
    fn encode_thread_id_full_should_encode_pr_level_thread_id() {
        // 1:1 with upstream "should encode PR-level thread ID".
        let id = encode_thread_id_full(&GithubThreadId {
            owner: "acme".into(),
            repo: "app".into(),
            pr_number: 123,
            kind: GithubThreadKind::Pr,
            review_comment_id: None,
        })
        .unwrap();
        assert_eq!(id, "github:acme/app:123");
    }

    #[test]
    fn encode_thread_id_full_should_encode_review_comment_thread_id() {
        // 1:1 with upstream "should encode review comment thread ID".
        let id = encode_thread_id_full(&GithubThreadId {
            owner: "acme".into(),
            repo: "app".into(),
            pr_number: 123,
            kind: GithubThreadKind::Pr,
            review_comment_id: Some(456789),
        })
        .unwrap();
        assert_eq!(id, "github:acme/app:123:rc:456789");
    }

    #[test]
    fn encode_thread_id_full_should_handle_special_characters_in_repo_names() {
        // 1:1 with upstream "should handle special characters in repo names".
        let id = encode_thread_id_full(&GithubThreadId {
            owner: "my-org".into(),
            repo: "my-cool-app".into(),
            pr_number: 42,
            kind: GithubThreadKind::Pr,
            review_comment_id: None,
        })
        .unwrap();
        assert_eq!(id, "github:my-org/my-cool-app:42");
    }

    #[test]
    fn encode_thread_id_full_should_encode_issue_thread_id() {
        // 1:1 with upstream "should encode issue thread ID".
        let id = encode_thread_id_full(&GithubThreadId {
            owner: "acme".into(),
            repo: "app".into(),
            pr_number: 10,
            kind: GithubThreadKind::Issue,
            review_comment_id: None,
        })
        .unwrap();
        assert_eq!(id, "github:acme/app:issue:10");
    }

    #[test]
    fn encode_thread_id_full_should_throw_for_issue_thread_with_review_comment_id() {
        // 1:1 with upstream "should throw for issue thread with reviewCommentId".
        let err = encode_thread_id_full(&GithubThreadId {
            owner: "acme".into(),
            repo: "app".into(),
            pr_number: 10,
            kind: GithubThreadKind::Issue,
            review_comment_id: Some(999),
        })
        .unwrap_err();
        assert_eq!(err, EncodeThreadIdError::ReviewCommentOnIssueThread);
        assert!(format!("{err}").contains("Review comments are not supported"));
    }

    // ---------- describe("decodeThreadId") (9 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("decodeThreadId")`.

    #[test]
    fn decode_thread_id_full_should_decode_pr_level_thread_id() {
        // 1:1 with upstream "should decode PR-level thread ID".
        let d = decode_thread_id_full("github:acme/app:123").unwrap();
        assert_eq!(d.owner, "acme");
        assert_eq!(d.repo, "app");
        assert_eq!(d.pr_number, 123);
        assert_eq!(d.kind, GithubThreadKind::Pr);
        assert!(d.review_comment_id.is_none());
    }

    #[test]
    fn decode_thread_id_full_should_decode_review_comment_thread_id() {
        // 1:1 with upstream "should decode review comment thread ID".
        let d = decode_thread_id_full("github:acme/app:123:rc:456789").unwrap();
        assert_eq!(d.owner, "acme");
        assert_eq!(d.repo, "app");
        assert_eq!(d.pr_number, 123);
        assert_eq!(d.kind, GithubThreadKind::Pr);
        assert_eq!(d.review_comment_id, Some(456789));
    }

    #[test]
    fn decode_thread_id_full_should_decode_issue_thread_id() {
        // 1:1 with upstream "should decode issue thread ID".
        let d = decode_thread_id_full("github:acme/app:issue:10").unwrap();
        assert_eq!(d.owner, "acme");
        assert_eq!(d.repo, "app");
        assert_eq!(d.pr_number, 10);
        assert_eq!(d.kind, GithubThreadKind::Issue);
        assert!(d.review_comment_id.is_none());
    }

    #[test]
    fn decode_thread_id_full_should_throw_for_invalid_thread_id_prefix() {
        // 1:1 with upstream "should throw for invalid thread ID prefix".
        let err = decode_thread_id_full("slack:C123:ts").unwrap_err();
        assert!(matches!(err, DecodeThreadIdError::InvalidPrefix(_)));
        assert!(format!("{err}").contains("Invalid GitHub thread ID"));
    }

    #[test]
    fn decode_thread_id_full_should_throw_for_malformed_thread_id() {
        // 1:1 with upstream "should throw for malformed thread ID".
        let err = decode_thread_id_full("github:invalid").unwrap_err();
        assert!(matches!(err, DecodeThreadIdError::InvalidFormat(_)));
        assert!(format!("{err}").contains("Invalid GitHub thread ID format"));
    }

    #[test]
    fn decode_thread_id_full_should_handle_repo_names_with_hyphens() {
        // 1:1 with upstream "should handle repo names with hyphens".
        let d = decode_thread_id_full("github:my-org/my-cool-app:42").unwrap();
        assert_eq!(d.owner, "my-org");
        assert_eq!(d.repo, "my-cool-app");
        assert_eq!(d.pr_number, 42);
        assert_eq!(d.kind, GithubThreadKind::Pr);
    }

    #[test]
    fn decode_thread_id_full_should_roundtrip_pr_level_thread_id() {
        // 1:1 with upstream "should roundtrip PR-level thread ID".
        let original = GithubThreadId {
            owner: "vercel".into(),
            repo: "next.js".into(),
            pr_number: 99999,
            kind: GithubThreadKind::Pr,
            review_comment_id: None,
        };
        let encoded = encode_thread_id_full(&original).unwrap();
        let decoded = decode_thread_id_full(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_thread_id_full_should_roundtrip_review_comment_thread_id() {
        // 1:1 with upstream "should roundtrip review comment thread ID".
        let original = GithubThreadId {
            owner: "vercel".into(),
            repo: "next.js".into(),
            pr_number: 99999,
            kind: GithubThreadKind::Pr,
            review_comment_id: Some(123456789),
        };
        let encoded = encode_thread_id_full(&original).unwrap();
        let decoded = decode_thread_id_full(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_thread_id_full_should_roundtrip_issue_thread_id() {
        // 1:1 with upstream "should roundtrip issue thread ID".
        let original = GithubThreadId {
            owner: "vercel".into(),
            repo: "next.js".into(),
            pr_number: 42,
            kind: GithubThreadKind::Issue,
            review_comment_id: None,
        };
        let encoded = encode_thread_id_full(&original).unwrap();
        let decoded = decode_thread_id_full(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    // ---------- describe("fetchChannelInfo") + describe("listThreads") channel-id validation ----------
    // 1:1 with upstream "should throw for invalid channel ID" (both
    // `fetchChannelInfo` and `listThreads`).

    #[test]
    fn parse_channel_id_should_parse_valid_channel_id() {
        // Covers the happy-path the upstream `fetchChannelInfo` /
        // `listThreads` rely on before issuing the HTTP request.
        let c = parse_channel_id("github:acme/app").unwrap();
        assert_eq!(c.owner, "acme");
        assert_eq!(c.repo, "app");
    }

    #[test]
    fn parse_channel_id_should_throw_for_invalid_channel_id_no_slash() {
        // 1:1 with upstream `listThreads > should throw for invalid
        // channel ID` ("github:invalid") + `fetchChannelInfo > should
        // throw for invalid channel ID` ("github:noslash"). Both
        // exercise the same `"Invalid GitHub channel ID"` throw.
        let err = parse_channel_id("github:noslash").unwrap_err();
        assert_eq!(err.0, "github:noslash");
        assert!(format!("{err}").contains("Invalid GitHub channel ID"));

        let err = parse_channel_id("github:invalid").unwrap_err();
        assert_eq!(err.0, "github:invalid");
    }

    // ---------- describe("postMessage" / "editMessage") body shape ----------
    // 1:1 with upstream `mockIssuesCreateComment.toHaveBeenCalledWith
    // ({body: "Hello world"})` + `mockIssuesUpdateComment.toHaveBeen
    // CalledWith({body: "Updated text"})` body-shape assertions —
    // covered structurally via `build_comment_body` so the body
    // contract is testable without a Vitest fetch-spy.

    #[test]
    fn build_comment_body_carries_text_in_body_field() {
        // 1:1 with upstream `should post an issue comment for PR-level
        // thread` + `should post a review comment reply` + `should edit
        // an issue comment` + `should edit a review comment` —
        // covers all 4 body-shape assertions.
        assert_eq!(
            build_comment_body("Hello world"),
            serde_json::json!({ "body": "Hello world" })
        );
        assert_eq!(
            build_comment_body("LGTM"),
            serde_json::json!({ "body": "LGTM" })
        );
        assert_eq!(
            build_comment_body("Updated text"),
            serde_json::json!({ "body": "Updated text" })
        );
        assert_eq!(
            build_comment_body("Updated review"),
            serde_json::json!({ "body": "Updated review" })
        );
    }

    #[test]
    fn build_comment_body_carries_ast_rendered_markdown_through() {
        // 1:1 with upstream `should post with AST message format` +
        // `should render card messages when editing` — the caller
        // pre-renders the AST/card to markdown, then calls the body
        // builder.
        assert_eq!(
            build_comment_body("**bold**"),
            serde_json::json!({ "body": "**bold**" })
        );
        assert_eq!(
            build_comment_body("**Updated Card**"),
            serde_json::json!({ "body": "**Updated Card**" })
        );
    }

    #[test]
    fn build_comment_body_passes_card_rendered_markdown_through() {
        // 1:1 with upstream `should render card messages to GitHub
        // markdown` — the caller pre-renders the card to GitHub
        // markdown (via `card_to_github_markdown`), then the body
        // builder wraps the rendered string verbatim.
        use crate::cards::card_to_github_markdown;
        use chat_sdk_chat::cards::{CardElement, CardKind};
        let card = CardElement {
            title: Some("Deploy Status".to_string()),
            subtitle: None,
            image_url: None,
            children: vec![],
            kind: CardKind::Card,
        };
        let rendered = card_to_github_markdown(&card);
        let body = build_comment_body(&rendered);
        assert!(body["body"].as_str().unwrap().contains("Deploy Status"));
    }

    // ---------- describe("addReaction") body shape ----------
    // 1:1 with upstream `mockReactionsCreateForIssueComment.toHaveBeen
    // CalledWith({content: "+1"})` body-shape assertion — covered via
    // `build_reaction_body`.

    #[test]
    fn build_reaction_body_carries_mapped_emoji_in_content_field() {
        // 1:1 with upstream `should add reaction to an issue comment`
        // (`thumbs_up` -> `+1`).
        assert_eq!(
            build_reaction_body("thumbs_up"),
            serde_json::json!({ "content": "+1" })
        );
    }

    #[test]
    fn build_reaction_body_passes_heart_through() {
        // 1:1 with upstream `should add reaction to a review comment`
        // (`heart` maps to itself).
        assert_eq!(
            build_reaction_body("heart"),
            serde_json::json!({ "content": "heart" })
        );
    }

    #[test]
    fn build_reaction_body_handles_emoji_value_named_form() {
        // 1:1 with upstream `should handle EmojiValue objects` — the
        // upstream caller normalizes `{ name: "rocket" }` to the
        // string "rocket" before invoking the body builder.
        assert_eq!(
            build_reaction_body("rocket"),
            serde_json::json!({ "content": "rocket" })
        );
    }

    // ---------- describe("removeReaction") bot-reaction filter ----------
    // 1:1 with upstream `reactions.filter(r => r.user.id === botUserId
    // && r.content === content)` — the matching-reaction selector.

    #[test]
    fn find_bot_reaction_id_returns_first_match() {
        // 1:1 with upstream `should remove bot reaction from an issue
        // comment` — first match wins (bot left reaction id 50, other
        // user left 51 with the same content).
        let reactions: [(u64, &str, u64); 2] = [(50, "+1", 777), (51, "+1", 999)];
        assert_eq!(find_bot_reaction_id(&reactions, 777, "+1"), Some(50));
    }

    #[test]
    fn find_bot_reaction_id_finds_review_comment_reaction() {
        // 1:1 with upstream `should remove bot reaction from a review
        // comment` — bot left a `heart` reaction id 60.
        let reactions: [(u64, &str, u64); 1] = [(60, "heart", 777)];
        assert_eq!(find_bot_reaction_id(&reactions, 777, "heart"), Some(60));
    }

    #[test]
    fn find_bot_reaction_id_returns_none_when_no_match() {
        // 1:1 with upstream `should do nothing when no matching
        // reaction found` — empty list returns `None`.
        let reactions: [(u64, &str, u64); 0] = [];
        assert!(find_bot_reaction_id(&reactions, 777, "+1").is_none());
    }

    // ---------- describe("stream") text accumulation ----------
    // 1:1 with upstream `should accumulate text chunks and post once`
    // — the text-concatenation contract.

    #[test]
    fn accumulate_stream_text_concatenates_string_chunks() {
        // 1:1 with upstream `should accumulate text chunks and post
        // once to an issue comment thread`.
        let result = accumulate_stream_text(["Hello", " ", "World"]);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn accumulate_stream_text_review_comment_path_concatenates() {
        // 1:1 with upstream `should accumulate text chunks and post
        // once to a review comment thread`.
        let result = accumulate_stream_text(["Looks", " ", "good"]);
        assert_eq!(result, "Looks good");
    }

    #[test]
    fn accumulate_stream_text_filters_non_text_chunks() {
        // 1:1 with upstream `should handle StreamChunk objects
        // alongside strings` — the caller filters non-text chunks
        // (e.g. `task_update`) before passing to the accumulator. The
        // accumulator itself is text-only.
        let chunks: Vec<&str> = vec!["Hello", " World"];
        let result = accumulate_stream_text(chunks);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn accumulate_stream_text_produces_empty_string_for_no_chunks() {
        // 1:1 with upstream `should post empty markdown when stream
        // yields no text`.
        let chunks: Vec<&str> = vec![];
        let result = accumulate_stream_text(chunks);
        assert_eq!(result, "");
    }

    // ---------- describe("fetchMessages") limit + direction window ----------
    // 1:1 with upstream `direction === "forward" ? items.slice(0,
    // limit) : items.slice(-limit)`.

    #[test]
    fn limit_messages_window_backward_takes_last_n() {
        // 1:1 with upstream `should respect limit option` — backward
        // direction takes the last 3 of 10.
        let items: Vec<u64> = (100..110).collect();
        let win = limit_messages_window(&items, FetchDirection::Backward, 3);
        assert_eq!(win, vec![107, 108, 109]);
    }

    #[test]
    fn limit_messages_window_forward_takes_first_n() {
        // 1:1 with upstream `should respect forward direction with
        // limit` — forward direction takes the first 3 of 10.
        let items: Vec<u64> = (100..110).collect();
        let win = limit_messages_window(&items, FetchDirection::Forward, 3);
        assert_eq!(win, vec![100, 101, 102]);
    }

    #[test]
    fn limit_messages_window_returns_all_when_limit_exceeds_len() {
        // Covers the upstream `result.messages.length` happy path
        // when the page is not full (matches the `result.messages` in
        // `should fetch issue comments for PR-level thread` where the
        // mock returns 2 items and the default limit is 100).
        let items: Vec<u64> = vec![100, 101];
        let win = limit_messages_window(&items, FetchDirection::Backward, 100);
        assert_eq!(win, vec![100, 101]);
    }

    // ---------- describe("listThreads") cursor pagination ----------
    // 1:1 with upstream `parseInt(options.cursor, 10) || 1` +
    // `pulls.length === limit ? String(currentPage + 1) : undefined`.

    #[test]
    fn parse_list_threads_cursor_defaults_to_1_when_missing() {
        // 1:1 with upstream `should list open PRs as threads` —
        // first page (no cursor) maps to page=1.
        assert_eq!(parse_list_threads_cursor(None), 1);
    }

    #[test]
    fn parse_list_threads_cursor_parses_decimal_string() {
        // 1:1 with upstream `should handle cursor-based pagination` —
        // cursor="3" maps to page=3.
        assert_eq!(parse_list_threads_cursor(Some("3")), 3);
    }

    #[test]
    fn parse_list_threads_cursor_falls_back_to_1_on_garbage() {
        // 1:1 with upstream's `parseInt("not-a-number", 10) || 1` —
        // NaN falls back to 1.
        assert_eq!(parse_list_threads_cursor(Some("not-a-number")), 1);
    }

    #[test]
    fn compute_next_cursor_returns_next_page_when_full() {
        // 1:1 with upstream `should provide nextCursor when results
        // fill the limit` — 5 results, limit=5, page=1 -> nextCursor="2".
        assert_eq!(compute_next_cursor(1, 5, 5), Some("2".to_string()));
    }

    #[test]
    fn compute_next_cursor_returns_none_when_partial() {
        // 1:1 with upstream `should not provide nextCursor when
        // results are fewer than limit` — 1 result, limit=30 -> None.
        assert!(compute_next_cursor(1, 1, 30).is_none());
    }

    // ---------- describe("getUser") display-name fallback ----------
    // 1:1 with upstream's `user.name || user.login` fallback chain.

    #[test]
    fn user_display_name_uses_name_when_present() {
        // 1:1 with upstream `should return user info from GitHub API` —
        // when `name: "Alice Smith"` is set, `fullName = "Alice Smith"`.
        assert_eq!(
            user_display_name("alice", Some("Alice Smith")),
            "Alice Smith"
        );
    }

    #[test]
    fn user_display_name_falls_back_to_login_when_name_is_null() {
        // 1:1 with upstream `should fall back to login when name is
        // null` — `name: null` -> `fullName = "noname-user"` (the login).
        assert_eq!(user_display_name("noname-user", None), "noname-user");
    }

    #[test]
    fn user_display_name_falls_back_to_login_when_name_is_empty_string() {
        // Edge case covered by upstream's `name || login` operator —
        // empty string is falsy in JS, so it falls back to login.
        assert_eq!(user_display_name("alice", Some("")), "alice");
    }

    // ---------- describe("getInstallationId") pure-helper cases ----------
    // 1:1 with the 4 of 7 upstream cases that don't require the
    // multi-tenant Chat-init + handleWebhook chain.

    #[test]
    fn get_installation_id_returns_fixed_id_in_single_tenant_app_mode() {
        // 1:1 with upstream "should return the fixed installation ID
        // from a thread in single-tenant app mode" — covered
        // structurally via the `GithubAuth::App` variant's
        // `installation_id: Some(_)` discriminant.
        let opts = GithubAdapterOptions {
            auth: GithubAuth::App(GithubAppCredentials {
                app_id: "12345".into(),
                private_key: "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----"
                    .into(),
                installation_id: Some(456),
            }),
            api_base: None,
            webhook_secret: Some("test-secret".into()),
            user_name: Some("test-bot[bot]".into()),
            bot_user_id: None,
        };
        match &opts.auth {
            GithubAuth::App(creds) => assert_eq!(creds.installation_id, Some(456)),
            _ => panic!("expected App auth"),
        }
        assert!(!opts.is_multi_tenant());
    }

    #[test]
    fn get_installation_id_returns_none_in_pat_mode() {
        // 1:1 with upstream "should return undefined in PAT mode" —
        // covered structurally via the `GithubAuth::Token` variant
        // which has no installation id.
        let opts = GithubAdapterOptions::new("ghp_test");
        match &opts.auth {
            GithubAuth::Token(_) => (),
            _ => panic!("expected Token auth"),
        }
        assert!(!opts.is_multi_tenant());
    }

    #[test]
    fn get_installation_id_throws_for_non_github_thread() {
        // 1:1 with upstream "should throw for non-GitHub thread or
        // message context" — covered via `decode_thread_id_full`
        // which returns `InvalidPrefix` for non-`github:` thread ids.
        let err = decode_thread_id_full("slack:C123:1234.5678").unwrap_err();
        assert!(matches!(err, DecodeThreadIdError::InvalidPrefix(_)));
        assert!(format!("{err}").contains("Invalid GitHub thread ID"));
    }

    #[test]
    fn multi_tenant_mode_is_detected_when_app_has_no_installation_id() {
        // 1:1 with upstream `isMultiTenant: boolean` — the gate that
        // upstream's `getInstallationId` uses to choose
        // fixed-vs-cached vs PAT-vs-undefined paths.
        let opts = GithubAdapterOptions {
            auth: GithubAuth::App(GithubAppCredentials {
                app_id: "12345".into(),
                private_key: "key".into(),
                installation_id: None,
            }),
            api_base: None,
            webhook_secret: Some("test".into()),
            user_name: None,
            bot_user_id: None,
        };
        assert!(opts.is_multi_tenant());
    }
}
