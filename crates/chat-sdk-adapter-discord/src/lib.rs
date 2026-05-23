//! Discord adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-discord/src/index.ts`.
//!
//! Discord maps each (guild, channel) pair to one chat-sdk thread.
//! The thread id encoding is `discord:<guild_id>:<channel_id>` (DMs
//! use the literal `@me` for the guild id).

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
}

impl DiscordAdapter {
    /// 1:1 port of upstream
    /// `new DiscordAdapter({ botToken, applicationId, apiBase? })`.
    pub fn new(options: DiscordAdapterOptions) -> Self {
        Self { options }
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
}

#[async_trait]
impl Adapter for DiscordAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }
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
    fn adapter_default_methods_return_unsupported() {
        let adapter = DiscordAdapter::new(DiscordAdapterOptions::new("b", "a"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("discord:123:456", "hi"));
        assert!(matches!(
            err,
            Err(AdapterError::Unsupported("post_message"))
        ));
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
