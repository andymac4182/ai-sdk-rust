//! Discord adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-discord/src/index.ts`.
//!
//! Discord maps each (guild, channel) pair to one chat-sdk thread.
//! The thread id encoding is `discord:<guild_id>:<channel_id>` (DMs
//! use the literal `@me` for the guild id).

pub mod cards;
pub mod markdown;
pub mod webhook;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "discord";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "discord:";

/// Default Discord REST API base URL.
pub const DEFAULT_API_BASE: &str = "https://discord.com/api/v10";

/// Sentinel guild id used by upstream to encode DMs.
pub const DM_GUILD: &str = "@me";

/// Maximum content length the Discord create-message endpoint
/// accepts in a single send. 1:1 with upstream's private
/// `DISCORD_MAX_CONTENT_LENGTH = 2000`.
pub const DISCORD_MAX_CONTENT_LENGTH: usize = 2000;

/// 1:1 with upstream's default `userName ?? "bot"` constant.
pub const DEFAULT_USER_NAME: &str = "bot";

/// Options for [`DiscordAdapter::new`].
#[derive(Debug, Clone)]
pub struct DiscordAdapterOptions {
    /// Discord bot token (`Bot <token>`).
    pub bot_token: String,
    /// Discord application id (for slash-command registration).
    pub application_id: String,
    /// Optional API base URL override.
    pub api_base: Option<String>,
    /// Optional public key (hex-encoded ed25519 verifying key) for
    /// interaction webhook signature verification.
    pub public_key: Option<String>,
    /// Optional display name (defaults to [`DEFAULT_USER_NAME`]).
    pub user_name: Option<String>,
    /// 1:1 with upstream `mentionRoleIds: string[]`. Role ids the
    /// adapter should mention by default.
    pub mention_role_ids: Vec<String>,
}

impl DiscordAdapterOptions {
    /// Construct options. API base URL defaults to
    /// [`DEFAULT_API_BASE`].
    pub fn new(bot_token: impl Into<String>, application_id: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            application_id: application_id.into(),
            api_base: None,
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
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

    /// Effective `userName` with default applied. 1:1 with upstream's
    /// `userName ?? "bot"`.
    pub fn effective_user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(DEFAULT_USER_NAME)
    }
}

/// Discord adapter.
#[derive(Debug, Clone)]
pub struct DiscordAdapter {
    options: DiscordAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl DiscordAdapter {
    /// 1:1 port of upstream
    /// `new DiscordAdapter({ botToken, applicationId, apiBase? })`.
    pub fn new(options: DiscordAdapterOptions) -> Self {
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

    /// Read the bot token.
    pub fn bot_token(&self) -> &str {
        &self.options.bot_token
    }

    /// Read the application id.
    pub fn application_id(&self) -> &str {
        &self.options.application_id
    }

    /// 1:1 with upstream `readonly userName: string`. Returns the
    /// configured value or [`DEFAULT_USER_NAME`].
    pub fn user_name(&self) -> &str {
        self.options.effective_user_name()
    }

    /// 1:1 with upstream `readonly publicKey: string`. Returns
    /// `None` when not configured (interaction-webhook verification
    /// will then fail-closed).
    pub fn public_key(&self) -> Option<&str> {
        self.options.public_key.as_deref()
    }

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Build the create-message URL (post target). 1:1 with
    /// upstream's inline `<api_base>/channels/<target>/messages`.
    /// `target` is the sub-thread id when the thread id encodes
    /// one, otherwise the channel id — matches upstream's
    /// `targetChannelId = discordThreadId || channelId`. Returns
    /// `None` when `thread_id` isn't Discord-encoded.
    pub fn post_message_url(&self, thread_id: &str) -> Option<String> {
        let decoded = decode_thread_id(thread_id)?;
        let target = decoded.thread_id.as_deref().unwrap_or(&decoded.channel_id);
        Some(format!("{}/channels/{}/messages", self.api_base(), target))
    }

    /// Build the per-message URL (edit/delete target). 1:1 with
    /// upstream's inline `<api_base>/channels/<target>/messages/
    /// <message_id>`. `target` is the sub-thread id when the thread
    /// id encodes one, otherwise the channel id — matches upstream's
    /// `targetChannelId = discordThreadId || channelId`. Returns
    /// `None` when `thread_id` isn't Discord-encoded.
    pub fn message_url(&self, thread_id: &str, message_id: &str) -> Option<String> {
        let decoded = decode_thread_id(thread_id)?;
        let target = decoded.thread_id.as_deref().unwrap_or(&decoded.channel_id);
        Some(format!(
            "{}/channels/{}/messages/{}",
            self.api_base(),
            target,
            message_id
        ))
    }

    /// Build the typing-indicator URL for a thread. 1:1 with
    /// upstream's inline `<api_base>/channels/<target>/typing`.
    /// `target` is the sub-thread id when the thread id encodes one,
    /// otherwise the channel id. Returns `None` when `thread_id`
    /// isn't Discord-encoded.
    pub fn typing_url(&self, thread_id: &str) -> Option<String> {
        let decoded = decode_thread_id(thread_id)?;
        let target = decoded.thread_id.as_deref().unwrap_or(&decoded.channel_id);
        Some(format!("{}/channels/{}/typing", self.api_base(), target))
    }

    /// Build the per-emoji reaction URL for a message. 1:1 with
    /// upstream's inline `<api_base>/channels/<target>/messages/
    /// <message_id>/reactions/<url-encoded-emoji>/@me`. `target` is
    /// the sub-thread id when the thread id encodes one,
    /// otherwise the channel id. Returns `None` when `thread_id`
    /// isn't Discord-encoded.
    pub fn reaction_url(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Option<String> {
        let decoded = decode_thread_id(thread_id)?;
        let target = decoded.thread_id.as_deref().unwrap_or(&decoded.channel_id);
        Some(format!(
            "{}/channels/{}/messages/{}/reactions/{}/@me",
            self.api_base(),
            target,
            message_id,
            url_encode_emoji(emoji),
        ))
    }

    /// Derive channel id from a Discord thread id. 1:1 with upstream
    /// `adapter.channelIdFromThreadId(threadId)` — splits on `:` and
    /// joins the first 3 parts: `discord:<guild_id>:<channel_id>`.
    /// If `thread_id` has fewer than 3 parts, returns the input
    /// unchanged (upstream's `slice(0,3).join(":")` behavior).
    pub fn channel_id_from_thread_id(&self, thread_id: &str) -> String {
        thread_id.splitn(4, ':').take(3).collect::<Vec<_>>().join(":")
    }

    /// Predicate: is the conversation a 1:1 DM? 1:1 with upstream's
    /// `adapter.isDM(threadId)` which decodes and tests `guildId ==
    /// "@me"`. Returns `None` when `thread_id` isn't a Discord-encoded
    /// value.
    pub fn is_dm(&self, thread_id: &str) -> Option<bool> {
        let decoded = decode_thread_id(thread_id)?;
        Some(decoded.is_dm())
    }

    /// Render formatted content to Discord-flavored markdown. 1:1
    /// with upstream `adapter.renderFormatted(content)` which
    /// delegates to `formatConverter.fromAst(content)`.
    pub fn render_formatted(&self, ast: &chat_sdk_chat::markdown::Node) -> String {
        crate::markdown::DiscordFormatConverter::new().from_ast(ast)
    }

    /// 1:1 with upstream `protected readonly mentionRoleIds:
    /// string[]`. Reading from this list is upstream-visible behavior
    /// via the gateway "mention" handlers; this accessor exposes it
    /// for parity tests.
    pub fn mention_role_ids(&self) -> &[String] {
        &self.options.mention_role_ids
    }
}

/// 1:1 with upstream `interface DiscordAdapterConfig` — all fields
/// optional so the constructor can fall back to environment
/// variables. Used by [`try_create_discord_adapter`].
#[derive(Debug, Clone, Default)]
pub struct DiscordCreateOptions {
    /// Discord bot token. Falls back to `DISCORD_BOT_TOKEN`.
    pub bot_token: Option<String>,
    /// Discord application id. Falls back to `DISCORD_APPLICATION_ID`.
    pub application_id: Option<String>,
    /// Hex-encoded ed25519 public key. Falls back to
    /// `DISCORD_PUBLIC_KEY`.
    pub public_key: Option<String>,
    /// API base URL. Falls back to `DISCORD_API_URL`, then
    /// [`DEFAULT_API_BASE`].
    pub api_url: Option<String>,
    /// Mention role ids. Falls back to a comma-separated
    /// `DISCORD_MENTION_ROLE_IDS`.
    pub mention_role_ids: Option<Vec<String>>,
    /// Display name. Falls back to [`DEFAULT_USER_NAME`].
    pub user_name: Option<String>,
}

/// Errors returned by [`try_create_discord_adapter`] when a required
/// field is missing from both the explicit options and the environment.
/// 1:1 with upstream `throw new ValidationError("discord", "... is
/// required")` cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscordCreateError {
    /// `botToken` missing and `DISCORD_BOT_TOKEN` not set.
    BotTokenRequired,
    /// `publicKey` missing and `DISCORD_PUBLIC_KEY` not set.
    PublicKeyRequired,
    /// `applicationId` missing and `DISCORD_APPLICATION_ID` not set.
    ApplicationIdRequired,
}

impl std::fmt::Display for DiscordCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BotTokenRequired => write!(
                f,
                "botToken is required. Set DISCORD_BOT_TOKEN or provide it in config."
            ),
            Self::PublicKeyRequired => write!(
                f,
                "publicKey is required. Set DISCORD_PUBLIC_KEY or provide it in config."
            ),
            Self::ApplicationIdRequired => write!(
                f,
                "applicationId is required. Set DISCORD_APPLICATION_ID or provide it in config."
            ),
        }
    }
}

impl std::error::Error for DiscordCreateError {}

/// 1:1 with upstream `new DiscordAdapter(config)` env-var resolution
/// path. Prefer explicit options; otherwise fall through to the
/// supplied `env` reader. The `env` parameter is a closure rather
/// than `std::env::var` directly so tests don't have to mutate
/// process-global state (which is `unsafe` in Rust 2024 edition).
///
/// Resolution rules (1:1 with upstream):
/// - `bot_token` ← `opts.bot_token` ?? `env("DISCORD_BOT_TOKEN")`
/// - `public_key` ← `opts.public_key` ?? `env("DISCORD_PUBLIC_KEY")`
/// - `application_id` ← `opts.application_id` ??
///   `env("DISCORD_APPLICATION_ID")`
/// - `api_base` ← `opts.api_url` ?? `env("DISCORD_API_URL")` ??
///   [`DEFAULT_API_BASE`]
/// - `mention_role_ids` ← `opts.mention_role_ids` ?? comma-split
///   `env("DISCORD_MENTION_ROLE_IDS")` ?? `[]`
/// - `user_name` ← `opts.user_name` ?? [`DEFAULT_USER_NAME`]
pub fn try_create_discord_adapter(
    opts: DiscordCreateOptions,
    env: impl Fn(&str) -> Option<String>,
) -> Result<DiscordAdapter, DiscordCreateError> {
    let bot_token = opts
        .bot_token
        .or_else(|| env("DISCORD_BOT_TOKEN"))
        .ok_or(DiscordCreateError::BotTokenRequired)?;
    let public_key = opts
        .public_key
        .or_else(|| env("DISCORD_PUBLIC_KEY"))
        .ok_or(DiscordCreateError::PublicKeyRequired)?;
    let application_id = opts
        .application_id
        .or_else(|| env("DISCORD_APPLICATION_ID"))
        .ok_or(DiscordCreateError::ApplicationIdRequired)?;

    let api_base = opts.api_url.or_else(|| env("DISCORD_API_URL"));
    let mention_role_ids = opts.mention_role_ids.unwrap_or_else(|| {
        env("DISCORD_MENTION_ROLE_IDS")
            .map(|raw| {
                raw.split(',')
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });

    Ok(DiscordAdapter::new(DiscordAdapterOptions {
        bot_token,
        application_id,
        api_base,
        public_key: Some(public_key),
        user_name: opts.user_name,
        mention_role_ids,
    }))
}

/// Discord interaction response types. 1:1 with upstream
/// `export const InteractionResponseType = { ... } as const`.
/// Only the two types the SDK currently emits are defined here.
///
/// - [`Self::DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE`] (`5`) — ACK
///   and edit later (deferred).
/// - [`Self::DEFERRED_UPDATE_MESSAGE`] (`6`) — ACK component
///   interaction, update message later.
pub struct InteractionResponseType;

impl InteractionResponseType {
    /// ACK and edit later (deferred). 1:1 with upstream
    /// `DeferredChannelMessageWithSource: 5`.
    pub const DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE: u8 = 5;
    /// ACK component interaction, update message later. 1:1 with
    /// upstream `DeferredUpdateMessage: 6`.
    pub const DEFERRED_UPDATE_MESSAGE: u8 = 6;
}

#[async_trait]
impl Adapter for DiscordAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a text message to a Discord channel (or sub-thread). 1:1
    /// with upstream's `adapter.postMessage`:
    ///
    /// - Decodes `discord:<guild_id>:<channel_id>[:sub-thread]` (guild is opaque
    ///   here; Discord routes by channel_id alone).
    /// - POSTs JSON `{content: text}` to
    ///   `<api_base>/channels/<channel_id>/messages`.
    /// - Auth via `Authorization: Bot <bot_token>` (Discord's
    ///   "Bot " auth-scheme prefix is non-standard; reqwest's
    ///   `.bearer_auth` uses "Bearer ", so we set the header
    ///   manually).
    /// - Returns the response's `id` field (Discord message
    ///   snowflake).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let url = self.post_message_url(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;
        let body = serde_json::json!({ "content": truncate_content(text) });

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token()))
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
            let msg = json["message"]
                .as_str()
                .unwrap_or("Discord API call failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {msg}")));
        }

        json["id"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload("Discord message-create response missing id".to_string())
        })
    }

    /// Edit a Discord message via PATCH
    /// `/channels/<target>/messages/<message_id>` (target =
    /// sub-thread id when encoded, else channel id). 1:1 with
    /// upstream's text-only path (cards/components deferred).
    /// Returns the (unchanged) message id.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let url = self.message_url(thread_id, message_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;
        let body = serde_json::json!({ "content": truncate_content(text) });

        let response = self
            .http
            .patch(&url)
            .header("Authorization", format!("Bot {}", self.bot_token()))
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
            let msg = json["message"]
                .as_str()
                .unwrap_or("Discord API call failed");
            return Err(AdapterError::InvalidPayload(format!("{status}: {msg}")));
        }

        json["id"].as_str().map(str::to_owned).ok_or_else(|| {
            AdapterError::InvalidPayload("Discord message-update response missing id".to_string())
        })
    }

    /// Delete a Discord message via DELETE
    /// `/channels/<target>/messages/<message_id>` (target =
    /// sub-thread id when encoded, else channel id). 1:1 with
    /// upstream's `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let url = self.message_url(thread_id, message_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let response = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bot {}", self.bot_token()))
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

    /// Add a reaction via PUT `/channels/<target>/messages/
    /// <message_id>/reactions/<url-encoded-emoji>/@me`. 1:1 with
    /// upstream's `adapter.addReaction`. `target` is the sub-thread
    /// id when the thread id encodes one
    /// (`discord:<guild>:<channel>:<sub-thread>`), otherwise the
    /// channel id — matches upstream's `targetChannelId =
    /// discordThreadId || channelId`. The emoji is URL-encoded
    /// (Discord accepts either raw glyphs or `<name:id>` for
    /// custom emoji).
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let url = self
            .reaction_url(thread_id, message_id, emoji)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(format!(
                    "thread_id {thread_id:?} is not Discord-encoded"
                ))
            })?;

        let response = self
            .http
            .put(&url)
            .header("Authorization", format!("Bot {}", self.bot_token()))
            .header("Content-Length", "0")
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

    /// Remove a reaction via DELETE `/channels/<target>/messages/
    /// <message_id>/reactions/<url-encoded-emoji>/@me`. 1:1 with
    /// upstream's `adapter.removeReaction`. Like `add_reaction` but
    /// uses DELETE; `target` is the sub-thread id when the thread
    /// id encodes one (`discord:<guild>:<channel>:<sub-thread>`),
    /// otherwise the channel id (`discord:<guild>:<channel>`).
    async fn remove_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let url = self
            .reaction_url(thread_id, message_id, emoji)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(format!(
                    "thread_id {thread_id:?} is not Discord-encoded"
                ))
            })?;

        let response = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bot {}", self.bot_token()))
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

    /// Send a Discord typing indicator via POST
    /// `/channels/<target>/typing`. 1:1 with upstream's
    /// `adapter.startTyping` (status arg ignored — Discord has
    /// no per-action status text; upstream ignores it too).
    /// `target` is the sub-thread id when the thread id encodes one
    /// (`discord:<guild>:<channel>:<sub-thread>`), otherwise the
    /// channel id — matches upstream's `targetChannelId =
    /// discordThreadId || channelId` routing.
    async fn start_typing(
        &self,
        thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let url = self.typing_url(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token()))
            .header("Content-Length", "0")
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

/// Truncate `content` to [`DISCORD_MAX_CONTENT_LENGTH`] with a
/// `"..."` tail when over the limit. 1:1 with upstream's private
/// `truncateContent(content)` helper — returns the input
/// unchanged when within limit, otherwise slices to
/// `limit - 3` and appends three dots so the final length is
/// exactly the limit. Operates on chars, not bytes, to handle
/// multibyte Unicode safely.
pub fn truncate_content(content: &str) -> String {
    let char_count = content.chars().count();
    if char_count <= DISCORD_MAX_CONTENT_LENGTH {
        return content.to_string();
    }
    let head: String = content
        .chars()
        .take(DISCORD_MAX_CONTENT_LENGTH - 3)
        .collect();
    format!("{head}...")
}

/// Percent-encode an emoji glyph (or `<name:id>` custom emoji
/// token) for inclusion in a Discord reaction URL path. 1:1 with
/// upstream's `encodeURIComponent(emoji)`.
fn url_encode_emoji(emoji: &str) -> String {
    let mut out = String::with_capacity(emoji.len() * 3);
    for byte in emoji.as_bytes() {
        let b = *byte;
        let unreserved = b.is_ascii_alphanumeric()
            || b == b'-'
            || b == b'_'
            || b == b'.'
            || b == b'~'
            || b == b'*'
            || b == b'\''
            || b == b'('
            || b == b')'
            || b == b'!';
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Encode a Discord thread id. 1:1 with upstream's inline format:
/// `discord:<guild_id>:<channel_id>`.
pub fn encode_thread_id(guild_id: &str, channel_id: &str) -> String {
    format!("{THREAD_ID_PREFIX}{guild_id}:{channel_id}")
}

/// Encode a Discord DM thread id (guild id = `@me`). Convenience
/// wrapper since the DM case is common at handler callsites.
pub fn encode_dm_thread_id(channel_id: &str) -> String {
    encode_thread_id(DM_GUILD, channel_id)
}

/// Components of a decoded Discord thread id. 1:1 with upstream
/// `interface DiscordThreadId { guildId; channelId; threadId? }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedDiscordThreadId {
    /// Discord guild id (or [`DM_GUILD`] for DMs).
    pub guild_id: String,
    /// Discord parent channel id.
    pub channel_id: String,
    /// Optional sub-thread id (the 4th colon-segment of the encoded
    /// thread id). Present when the thread is a sub-thread under a
    /// channel; absent for channel-level threads.
    pub thread_id: Option<String>,
}

impl DecodedDiscordThreadId {
    /// Whether this thread id encodes a DM (guild id == `@me`).
    pub fn is_dm(&self) -> bool {
        self.guild_id == DM_GUILD
    }
}

/// Decode a Discord thread id. 1:1 with upstream
/// `decodeThreadId(threadId)` which splits on `:` and requires at
/// least 3 segments (`discord`, guild, channel) with an optional
/// 4th sub-thread segment. Returns `None` for any malformed input;
/// upstream throws `ValidationError` in the same cases.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedDiscordThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.split(':');
    let guild_id = parts.next()?;
    let channel_id = parts.next()?;
    let sub_thread = parts.next();
    if guild_id.is_empty() || channel_id.is_empty() {
        return None;
    }
    Some(DecodedDiscordThreadId {
        guild_id: guild_id.to_string(),
        channel_id: channel_id.to_string(),
        thread_id: sub_thread.filter(|s| !s.is_empty()).map(str::to_string),
    })
}

/// Predicate: does this thread id belong to the Discord adapter?
pub fn is_discord_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_discord() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("bot-token", "app-id"));
        assert_eq!(adapter.name(), "discord");
    }

    #[test]
    fn options_new_stores_credentials_and_defaults_api_base() {
        let opts = DiscordAdapterOptions::new("bot", "app");
        assert_eq!(opts.bot_token, "bot");
        assert_eq!(opts.application_id, "app");
        assert_eq!(opts.effective_api_base(), DEFAULT_API_BASE);
    }

    #[test]
    fn options_with_api_base_overrides_the_default() {
        let opts = DiscordAdapterOptions::new("b", "a")
            .with_api_base("https://discord.example.test/api/v9");
        assert_eq!(
            opts.effective_api_base(),
            "https://discord.example.test/api/v9"
        );
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(encode_thread_id("123", "456"), "discord:123:456");
    }

    #[test]
    fn encode_dm_thread_id_uses_the_at_me_guild() {
        assert_eq!(encode_dm_thread_id("789"), "discord:@me:789");
        assert!(
            decode_thread_id(&encode_dm_thread_id("789"))
                .unwrap()
                .is_dm()
        );
    }

    #[test]
    fn decode_thread_id_parses_guild_and_channel() {
        let decoded = decode_thread_id("discord:123:456").unwrap();
        assert_eq!(decoded.guild_id, "123");
        assert_eq!(decoded.channel_id, "456");
        assert!(!decoded.is_dm());
    }

    #[test]
    fn decoded_thread_id_is_dm_for_at_me_guild() {
        let decoded = decode_thread_id("discord:@me:789").unwrap();
        assert!(decoded.is_dm());
        assert_eq!(decoded.channel_id, "789");
    }

    #[test]
    fn decode_thread_id_captures_optional_sub_thread() {
        // 1:1 with upstream's optional 4th colon-segment for
        // sub-threads under a channel.
        let decoded = decode_thread_id("discord:GUILD:CHAN:SUB").unwrap();
        assert_eq!(decoded.guild_id, "GUILD");
        assert_eq!(decoded.channel_id, "CHAN");
        assert_eq!(decoded.thread_id.as_deref(), Some("SUB"));
    }

    #[test]
    fn decode_thread_id_treats_trailing_empty_sub_thread_as_absent() {
        // `discord:G:C:` (trailing colon, empty 4th part) should not
        // produce `thread_id: Some("")`. Matches upstream's behavior
        // where `parts[3]` would be `""` and is dropped at the
        // shape-guard layer.
        let decoded = decode_thread_id("discord:G:C:").unwrap();
        assert_eq!(decoded.thread_id, None);
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("slack:C1:1.0").is_none());
        assert!(decode_thread_id("telegram:123").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_components() {
        assert!(decode_thread_id("discord:onlyone").is_none());
        assert!(decode_thread_id("discord::456").is_none());
        assert!(decode_thread_id("discord:123:").is_none());
    }

    #[test]
    // ---------- channel_id_from_thread_id + is_dm ----------
    // 1:1 with upstream `adapter.channelIdFromThreadId(threadId)`
    // (first 3 colon-segments of any string) and `adapter.isDM(threadId)`
    // (true iff guild_id == "@me").

    #[test]
    // ---------- renderFormatted (1 upstream case) ----------
    #[test]
    fn render_formatted_should_render_markdown_from_ast() {
        use chat_sdk_chat::markdown::{Node, paragraph, root, text};
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("APP", "BOT_TOKEN"));
        let ast = Node::Root(root(vec![Node::Paragraph(paragraph(vec![Node::Text(
            text("Hello world"),
        )]))]));
        let result = adapter.render_formatted(&ast);
        assert!(result.contains("Hello world"), "got: {result}");
    }

    #[test]
    #[test]
    fn discord_max_content_length_matches_upstream() {
        // 1:1 with upstream's private `DISCORD_MAX_CONTENT_LENGTH = 2000`.
        assert_eq!(DISCORD_MAX_CONTENT_LENGTH, 2000);
    }

    #[test]
    fn interaction_response_type_constants_match_upstream() {
        // 1:1 with upstream `InteractionResponseType = { ... }`.
        // The two values currently emitted by the SDK are 5 and 6;
        // the upstream comment notes additional types
        // (`ChannelMessageWithSource: 4`, `UpdateMessage: 7`) that
        // aren't currently used by the adapter.
        assert_eq!(InteractionResponseType::DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE, 5);
        assert_eq!(InteractionResponseType::DEFERRED_UPDATE_MESSAGE, 6);
    }

    #[test]
    fn channel_id_from_thread_id_takes_first_three_colon_segments() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("APP", "BOT_TOKEN"));
        // Channel-only thread id passes through unchanged.
        assert_eq!(
            adapter.channel_id_from_thread_id("discord:G1:C1"),
            "discord:G1:C1"
        );
        // Channel-with-sub-thread strips the 4th segment.
        assert_eq!(
            adapter.channel_id_from_thread_id("discord:G1:C1:T9"),
            "discord:G1:C1"
        );
        // DM channel preserves the @me guild marker.
        assert_eq!(
            adapter.channel_id_from_thread_id("discord:@me:DM1"),
            "discord:@me:DM1"
        );
    }

    #[test]
    fn is_dm_true_for_at_me_guild_only() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("APP", "BOT_TOKEN"));
        assert_eq!(adapter.is_dm("discord:@me:DM1"), Some(true));
        assert_eq!(adapter.is_dm("discord:G1:C1"), Some(false));
    }

    #[test]
    fn is_dm_returns_false_for_threads_in_guilds() {
        // 1:1 with upstream `describe("isDM") > it("returns false for
        // threads in guilds")` — sub-thread under a guild channel is
        // not a DM (only `@me` is).
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("APP", "BOT_TOKEN"));
        assert_eq!(
            adapter.is_dm("discord:guild123:channel456:thread789"),
            Some(false)
        );
    }

    #[test]
    fn is_dm_returns_none_for_non_discord_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("APP", "BOT_TOKEN"));
        assert_eq!(adapter.is_dm("slack:C1:1.0"), None);
        assert_eq!(adapter.is_dm(""), None);
    }

    #[test]
    fn is_discord_thread_id_detects_the_prefix() {
        assert!(is_discord_thread_id("discord:123:456"));
        assert!(!is_discord_thread_id("teams:1:2"));
        assert!(!is_discord_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (g, c) in [("123", "456"), ("@me", "789"), ("g", "ch")] {
            let encoded = encode_thread_id(g, c);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.guild_id, g);
            assert_eq!(decoded.channel_id, c);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("b", "a"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Discord-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_rejects_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("b", "a"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("slack:C1:1.0", "msg", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Discord-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_rejects_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("b", "a"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("slack:C1:1.0", "msg"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Discord-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("b", "a"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("slack:C1:1.0", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Discord-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_rejects_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("b", "a"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.start_typing("slack:C1:1.0", None));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not Discord-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn url_encode_emoji_percent_encodes_unicode_glyphs() {
        // U+1F44D (👍) is bytes F0 9F 91 8D
        assert_eq!(url_encode_emoji("👍"), "%F0%9F%91%8D");
        // ASCII-friendly emoji name token
        assert_eq!(url_encode_emoji("smile"), "smile");
        // Custom emoji <name:id> includes characters that need encoding
        assert_eq!(url_encode_emoji("<custom:123>"), "%3Ccustom%3A123%3E");
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = DiscordAdapter::new(
            DiscordAdapterOptions::new("bot-tok", "app-id").with_api_base("https://example.test"),
        );
        assert_eq!(adapter.bot_token(), "bot-tok");
        assert_eq!(adapter.application_id(), "app-id");
        assert_eq!(adapter.api_base(), "https://example.test");
    }

    // ---------- createDiscordAdapter describe block (3 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("createDiscordAdapter")`.

    #[test]
    fn create_discord_adapter_creates_an_instance() {
        let opts = DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: Some("ed25519-public-key-hex".to_string()),
            user_name: None,
            mention_role_ids: Vec::new(),
        };
        let adapter = DiscordAdapter::new(opts);
        assert_eq!(adapter.name(), "discord");
    }

    #[test]
    fn create_discord_adapter_sets_default_user_name_to_bot() {
        let opts = DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: Some("ed25519-public-key-hex".to_string()),
            user_name: None,
            mention_role_ids: Vec::new(),
        };
        let adapter = DiscordAdapter::new(opts);
        assert_eq!(adapter.user_name(), "bot");
    }

    #[test]
    fn create_discord_adapter_uses_provided_user_name() {
        let opts = DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: Some("ed25519-public-key-hex".to_string()),
            user_name: Some("custombot".to_string()),
            mention_role_ids: Vec::new(),
        };
        let adapter = DiscordAdapter::new(opts);
        assert_eq!(adapter.user_name(), "custombot");
    }

    // ---------- constructor env var resolution describe block (9 cases) ----------
    // 1:1 with upstream `index.test.ts > describe("constructor env var
    // resolution")`. Uses an injected `env` closure instead of mutating
    // `std::env` (which is `unsafe` in Rust 2024 edition and racy
    // between parallel tests).

    fn empty_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn ctor_env_throws_when_bot_token_is_missing() {
        let err = try_create_discord_adapter(DiscordCreateOptions::default(), empty_env)
            .expect_err("missing bot token must fail");
        assert_eq!(err, DiscordCreateError::BotTokenRequired);
        assert!(err.to_string().contains("botToken is required"));
    }

    #[test]
    fn ctor_env_throws_when_public_key_is_missing() {
        let err = try_create_discord_adapter(
            DiscordCreateOptions {
                bot_token: Some("test".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect_err("missing public key must fail");
        assert_eq!(err, DiscordCreateError::PublicKeyRequired);
        assert!(err.to_string().contains("publicKey is required"));
    }

    #[test]
    fn ctor_env_throws_when_application_id_is_missing() {
        let err = try_create_discord_adapter(
            DiscordCreateOptions {
                bot_token: Some("test".to_string()),
                public_key: Some("ed25519-public-key-hex".to_string()),
                ..Default::default()
            },
            empty_env,
        )
        .expect_err("missing application id must fail");
        assert_eq!(err, DiscordCreateError::ApplicationIdRequired);
        assert!(err.to_string().contains("applicationId is required"));
    }

    #[test]
    fn ctor_env_resolves_all_fields_from_env_vars() {
        let env = |key: &str| match key {
            "DISCORD_BOT_TOKEN" => Some("env-token".to_string()),
            "DISCORD_PUBLIC_KEY" => Some("ed25519-public-key-hex".to_string()),
            "DISCORD_APPLICATION_ID" => Some("env-app-id".to_string()),
            _ => None,
        };
        let adapter = try_create_discord_adapter(DiscordCreateOptions::default(), env)
            .expect("all env vars set");
        assert_eq!(adapter.name(), "discord");
        assert_eq!(adapter.bot_token(), "env-token");
        assert_eq!(adapter.application_id(), "env-app-id");
        assert_eq!(adapter.user_name(), "bot");
    }

    #[test]
    fn ctor_env_resolves_mention_role_ids_from_env_var() {
        let env = |key: &str| match key {
            "DISCORD_BOT_TOKEN" => Some("env-token".to_string()),
            "DISCORD_PUBLIC_KEY" => Some("ed25519-public-key-hex".to_string()),
            "DISCORD_APPLICATION_ID" => Some("env-app-id".to_string()),
            "DISCORD_MENTION_ROLE_IDS" => Some("role1, role2, role3".to_string()),
            _ => None,
        };
        let adapter = try_create_discord_adapter(DiscordCreateOptions::default(), env)
            .expect("all env vars set");
        assert_eq!(adapter.mention_role_ids(), &["role1", "role2", "role3"]);
    }

    #[test]
    fn ctor_env_defaults_logger_when_not_provided() {
        // Upstream asserts the adapter is constructed when no logger
        // is supplied. In this port the logger is not yet a
        // first-class adapter dependency, so the equivalent is that
        // env-only construction succeeds.
        let env = |key: &str| match key {
            "DISCORD_BOT_TOKEN" => Some("env-token".to_string()),
            "DISCORD_PUBLIC_KEY" => Some("ed25519-public-key-hex".to_string()),
            "DISCORD_APPLICATION_ID" => Some("env-app-id".to_string()),
            _ => None,
        };
        let adapter = try_create_discord_adapter(DiscordCreateOptions::default(), env)
            .expect("env-only construction works");
        assert_eq!(adapter.name(), "discord");
    }

    #[test]
    fn ctor_env_prefers_config_values_over_env_vars() {
        let env = |key: &str| match key {
            "DISCORD_BOT_TOKEN" => Some("env-token".to_string()),
            "DISCORD_PUBLIC_KEY" => Some("env-public-key".to_string()),
            "DISCORD_APPLICATION_ID" => Some("env-app-id".to_string()),
            _ => None,
        };
        let adapter = try_create_discord_adapter(
            DiscordCreateOptions {
                bot_token: Some("config-token".to_string()),
                public_key: Some("ed25519-public-key-hex".to_string()),
                application_id: Some("config-app-id".to_string()),
                user_name: Some("mybot".to_string()),
                ..Default::default()
            },
            env,
        )
        .expect("config overrides env");
        assert_eq!(adapter.bot_token(), "config-token");
        assert_eq!(adapter.application_id(), "config-app-id");
        assert_eq!(adapter.user_name(), "mybot");
    }

    #[test]
    fn ctor_env_resolves_api_url_from_discord_api_url_env_var() {
        let env = |key: &str| match key {
            "DISCORD_BOT_TOKEN" => Some("env-token".to_string()),
            "DISCORD_PUBLIC_KEY" => Some("ed25519-public-key-hex".to_string()),
            "DISCORD_APPLICATION_ID" => Some("env-app-id".to_string()),
            "DISCORD_API_URL" => Some("https://custom-discord.example.com/api/v10".to_string()),
            _ => None,
        };
        let adapter = try_create_discord_adapter(DiscordCreateOptions::default(), env)
            .expect("env api url resolves");
        assert_eq!(
            adapter.api_base(),
            "https://custom-discord.example.com/api/v10"
        );
    }

    #[test]
    fn ctor_env_prefers_api_url_config_over_discord_api_url_env_var() {
        let env = |key: &str| match key {
            "DISCORD_BOT_TOKEN" => Some("env-token".to_string()),
            "DISCORD_PUBLIC_KEY" => Some("ed25519-public-key-hex".to_string()),
            "DISCORD_APPLICATION_ID" => Some("env-app-id".to_string()),
            "DISCORD_API_URL" => Some("https://env-url.example.com/api/v10".to_string()),
            _ => None,
        };
        let adapter = try_create_discord_adapter(
            DiscordCreateOptions {
                bot_token: Some("config-token".to_string()),
                public_key: Some("ed25519-public-key-hex".to_string()),
                application_id: Some("config-app-id".to_string()),
                api_url: Some("https://config-url.example.com/api/v10".to_string()),
                ..Default::default()
            },
            env,
        )
        .expect("config api url wins");
        assert_eq!(
            adapter.api_base(),
            "https://config-url.example.com/api/v10"
        );
    }

    // ---------- describe("removeReaction") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("removeReaction")`.
    // Upstream asserts the DELETE URL contains the expected channel
    // / thread / message / "/@me" path segments. The Rust port
    // exposes a pure `reaction_url` helper so the URL construction
    // can be tested without HTTP-mocking the full Adapter::remove_reaction
    // round-trip.

    #[test]
    fn discord_remove_reaction_url_uses_channel_id_for_top_level_thread() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .reaction_url("discord:guild1:channel456", "msg001", "thumbs_up")
            .unwrap();
        assert!(
            url.contains("/channels/channel456/messages/msg001/reactions/"),
            "URL was {url}"
        );
        assert!(url.ends_with("/@me"), "URL was {url}");
    }

    #[test]
    fn discord_remove_reaction_url_routes_through_sub_thread_when_encoded() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .reaction_url("discord:guild1:channel456:thread789", "msg001", "fire")
            .unwrap();
        assert!(
            url.contains("/channels/thread789/messages/msg001/reactions/"),
            "URL was {url}"
        );
    }

    // ---------- describe("postMessage") (2 of 3 upstream cases; jsx-payload case deferred) ----------
    // 1:1 with upstream `index.test.ts > describe("postMessage")`.
    // The 3rd case (cards/JSX payload) needs the cards renderer
    // wired into post_message and is deferred.

    #[test]
    fn discord_post_message_url_uses_channel_id_for_top_level_thread() {
        // 1:1 with upstream "posts a plain text message".
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter.post_message_url("discord:guild1:channel456").unwrap();
        assert_eq!(
            url,
            "https://discord.test/api/v10/channels/channel456/messages"
        );
    }

    #[test]
    fn discord_post_message_url_routes_through_sub_thread_when_encoded() {
        // 1:1 with upstream "posts to thread channel when threadId
        // is present".
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .post_message_url("discord:guild1:channel456:thread789")
            .unwrap();
        assert_eq!(
            url,
            "https://discord.test/api/v10/channels/thread789/messages"
        );
    }

    #[test]
    fn discord_post_message_url_returns_none_for_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        assert!(adapter.post_message_url("slack:C123:1.0").is_none());
    }

    // ---------- describe("editMessage") truncation (1 of 3 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("editMessage")`.
    // The 2 routing cases (channel-id / sub-thread) are already
    // covered by the `discord_delete_message_url_*` tests below
    // since `edit_message` and `delete_message` share the
    // `message_url` helper. The truncation case is exercised
    // directly via the pure `truncate_content` helper.

    #[test]
    fn discord_edit_message_truncates_content_exceeding_2000_characters() {
        let long = "b".repeat(2500);
        let truncated = truncate_content(&long);
        assert!(truncated.chars().count() <= DISCORD_MAX_CONTENT_LENGTH);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn discord_truncate_content_returns_input_unchanged_when_under_limit() {
        let short = "hello";
        assert_eq!(truncate_content(short), short);
    }

    #[test]
    fn discord_truncate_content_handles_multibyte_chars_safely() {
        // 1000 4-byte chars + 500 multibyte = 1500 chars, under
        // limit. 2500 emoji chars would exceed; verify char-count
        // boundary not byte-count.
        let unicode = "🦀".repeat(2500);
        let truncated = truncate_content(&unicode);
        assert!(truncated.chars().count() <= DISCORD_MAX_CONTENT_LENGTH);
        assert!(truncated.ends_with("..."));
    }

    // ---------- describe("deleteMessage") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("deleteMessage")`.

    #[test]
    fn discord_delete_message_url_uses_channel_id_for_top_level_thread() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .message_url("discord:guild1:channel456", "msg001")
            .unwrap();
        assert_eq!(
            url,
            "https://discord.test/api/v10/channels/channel456/messages/msg001"
        );
    }

    #[test]
    fn discord_delete_message_url_routes_through_sub_thread_when_encoded() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .message_url("discord:guild1:channel456:thread789", "msg002")
            .unwrap();
        assert_eq!(
            url,
            "https://discord.test/api/v10/channels/thread789/messages/msg002"
        );
    }

    #[test]
    fn discord_message_url_returns_none_for_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        assert!(adapter.message_url("slack:C123:1.0", "msg").is_none());
    }

    // ---------- describe("addReaction") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("addReaction")`.
    // Both `add_reaction` and `remove_reaction` route through the
    // same `reaction_url` helper, so the URL-shape tests below
    // cover the addReaction describe block's PUT path too —
    // these tests assert the same target-channel routing as
    // slice-330's remove_reaction tests.

    #[test]
    fn discord_add_reaction_url_uses_channel_id_for_top_level_thread() {
        // 1:1 with upstream "adds a reaction to a message" — channel
        // id is the URL target when no sub-thread is encoded.
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .reaction_url("discord:guild1:channel456", "msg001", "thumbs_up")
            .unwrap();
        assert!(
            url.contains("/channels/channel456/messages/msg001/reactions/"),
            "URL was {url}"
        );
        assert!(url.ends_with("/@me"), "URL was {url}");
    }

    #[test]
    fn discord_add_reaction_url_routes_through_sub_thread_when_encoded() {
        // 1:1 with upstream "adds a reaction in a thread" — sub-thread
        // id is the URL target when encoded.
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .reaction_url("discord:guild1:channel456:thread789", "msg001", "heart")
            .unwrap();
        assert!(
            url.contains("/channels/thread789/messages/msg001/reactions/"),
            "URL was {url}"
        );
    }

    // ---------- describe("startTyping") (2 upstream cases) ----------
    // 1:1 with upstream `index.test.ts > describe("startTyping")`.

    #[test]
    fn discord_typing_url_uses_channel_id_for_top_level_thread() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter.typing_url("discord:guild1:channel456").unwrap();
        assert_eq!(
            url,
            "https://discord.test/api/v10/channels/channel456/typing"
        );
    }

    #[test]
    fn discord_typing_url_routes_through_sub_thread_when_encoded() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: Some("https://discord.test/api/v10".to_string()),
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        let url = adapter
            .typing_url("discord:guild1:channel456:thread789")
            .unwrap();
        assert_eq!(
            url,
            "https://discord.test/api/v10/channels/thread789/typing"
        );
    }

    #[test]
    fn discord_typing_url_returns_none_for_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        assert!(adapter.typing_url("slack:C123:1.0").is_none());
    }

    #[test]
    fn discord_remove_reaction_url_returns_none_for_non_discord_thread_ids() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions {
            bot_token: "test-token".to_string(),
            application_id: "test-app-id".to_string(),
            api_base: None,
            public_key: None,
            user_name: None,
            mention_role_ids: Vec::new(),
        });
        assert!(adapter.reaction_url("slack:C123:1.0", "msg001", "fire").is_none());
    }
}
