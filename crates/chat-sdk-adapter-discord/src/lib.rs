//! Discord adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-discord/src/index.ts`.
//!
//! Discord maps each (guild, channel) pair to one chat-sdk thread.
//! The thread id encoding is `discord:<guild_id>:<channel_id>` (DMs
//! use the literal `@me` for the guild id).

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

/// Options for [`DiscordAdapter::new`].
#[derive(Debug, Clone)]
pub struct DiscordAdapterOptions {
    /// Discord bot token (`Bot <token>`).
    pub bot_token: String,
    /// Discord application id (for slash-command registration).
    pub application_id: String,
    /// Optional API base URL override.
    pub api_base: Option<String>,
}

impl DiscordAdapterOptions {
    /// Construct options. API base URL defaults to
    /// [`DEFAULT_API_BASE`].
    pub fn new(bot_token: impl Into<String>, application_id: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            application_id: application_id.into(),
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

    /// Effective API base URL.
    pub fn api_base(&self) -> &str {
        self.options.effective_api_base()
    }

    /// Build the channel-messages URL. 1:1 with upstream's inline
    /// `<api_base>/channels/<channel_id>/messages` template.
    fn channel_messages_url(&self, channel_id: &str) -> String {
        format!("{}/channels/{channel_id}/messages", self.api_base())
    }
}

#[async_trait]
impl Adapter for DiscordAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a text message to a Discord channel. 1:1 with upstream's
    /// `adapter.postMessage`:
    ///
    /// - Decodes `discord:<guild_id>:<channel_id>` (guild is opaque
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

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let url = self.channel_messages_url(&decoded.channel_id);
        let body = serde_json::json!({ "content": text });

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
    /// `/channels/<channel_id>/messages/<message_id>`. 1:1 with
    /// upstream's text-only path (cards/components deferred).
    /// Returns the (unchanged) message id.
    async fn edit_message(
        &self,
        thread_id: &str,
        message_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let url = format!(
            "{}/{}",
            self.channel_messages_url(&decoded.channel_id),
            message_id
        );
        let body = serde_json::json!({ "content": text });

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
    /// `/channels/<channel_id>/messages/<message_id>`. 1:1 with
    /// upstream's `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        thread_id: &str,
        message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let url = format!(
            "{}/{}",
            self.channel_messages_url(&decoded.channel_id),
            message_id
        );

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

    /// Add a reaction via PUT `/channels/<channel_id>/messages/
    /// <message_id>/reactions/<url-encoded-emoji>/@me`. 1:1 with
    /// upstream's `adapter.addReaction`. The emoji is URL-encoded
    /// (Discord accepts either raw glyphs or `<name:id>` for
    /// custom emoji).
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let emoji_encoded = url_encode_emoji(emoji);
        let url = format!(
            "{}/{}/reactions/{}/@me",
            self.channel_messages_url(&decoded.channel_id),
            message_id,
            emoji_encoded
        );

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

    /// Send a Discord typing indicator via POST
    /// `/channels/<channel_id>/typing`. 1:1 with upstream's
    /// `adapter.startTyping` (status arg ignored — Discord has
    /// no per-action status text; upstream ignores it too).
    async fn start_typing(
        &self,
        thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not Discord-encoded"))
        })?;

        let url = format!("{}/channels/{}/typing", self.api_base(), decoded.channel_id);

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

/// Components of a decoded Discord thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedDiscordThreadId {
    /// Discord guild id (or [`DM_GUILD`] for DMs).
    pub guild_id: String,
    /// Discord channel id.
    pub channel_id: String,
}

impl DecodedDiscordThreadId {
    /// Whether this thread id encodes a DM (guild id == `@me`).
    pub fn is_dm(&self) -> bool {
        self.guild_id == DM_GUILD
    }
}

/// Decode a Discord thread id.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedDiscordThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let guild_id = parts.next()?;
    let channel_id = parts.next()?;
    if guild_id.is_empty() || channel_id.is_empty() {
        return None;
    }
    Some(DecodedDiscordThreadId {
        guild_id: guild_id.to_string(),
        channel_id: channel_id.to_string(),
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
    fn adapter_channel_messages_url_builds_the_upstream_endpoint() {
        let adapter = DiscordAdapter::new(
            DiscordAdapterOptions::new("b", "a")
                .with_api_base("https://discord.example.test/api/v10"),
        );
        assert_eq!(
            adapter.channel_messages_url("456"),
            "https://discord.example.test/api/v10/channels/456/messages"
        );
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
}
