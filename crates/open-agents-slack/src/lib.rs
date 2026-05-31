//! Slack surface contracts for the Rust Open Agents remote agent.
//!
//! This crate owns Slack identity, ingress, outbound rendering, interactions,
//! and session mapping. It must call the runtime for agent work rather than
//! talking to a sandbox directly.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Bucket that owns the first Slack ingress implementation.
pub const INGRESS_OWNER_BUCKET: u8 = 10;

/// Bucket that owns Slack outbound messages and interactions.
pub const OUTBOUND_OWNER_BUCKET: u8 = 11;

/// Bucket that owns Slack session lifecycle behavior.
pub const SESSION_OWNER_BUCKET: u8 = 12;

/// Bot OAuth token environment variable.
pub const SLACK_BOT_TOKEN_ENV: &str = "SLACK_BOT_TOKEN";

/// Request signing secret environment variable.
pub const SLACK_SIGNING_SECRET_ENV: &str = "SLACK_SIGNING_SECRET";

/// Optional Socket Mode app token environment variable.
pub const SLACK_APP_TOKEN_ENV: &str = "SLACK_APP_TOKEN";

/// Optional live-test channel id environment variable.
pub const SLACK_TEST_CHANNEL_ID_ENV: &str = "SLACK_TEST_CHANNEL_ID";

/// Optional live-test user id environment variable.
pub const SLACK_TEST_USER_ID_ENV: &str = "SLACK_TEST_USER_ID";

/// Slack thread identity used to route one Slack conversation to one session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackThreadAddress {
    /// Slack team/workspace id, when present in the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// Slack channel id.
    pub channel_id: String,
    /// Parent thread timestamp. Top-level messages use their own `ts`.
    pub thread_ts: String,
}

impl SlackThreadAddress {
    /// Creates a Slack thread address from channel and thread timestamp.
    pub fn new(channel_id: impl Into<String>, thread_ts: impl Into<String>) -> Self {
        Self {
            team_id: None,
            channel_id: channel_id.into(),
            thread_ts: thread_ts.into(),
        }
    }

    /// Attaches a Slack team/workspace id.
    pub fn with_team_id(mut self, team_id: impl Into<String>) -> Self {
        self.team_id = Some(team_id.into());
        self
    }

    /// Returns the `chat-sdk-adapter-slack` thread id shape.
    pub fn chat_thread_id(&self) -> String {
        format!(
            "{}{}:{}",
            chat_sdk_adapter_slack::THREAD_ID_PREFIX,
            self.channel_id,
            self.thread_ts
        )
    }
}

/// Slack delivery mode selected by service configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackIngressMode {
    /// HTTP webhook/Event API delivery.
    Webhook,
    /// Slack Socket Mode delivery.
    SocketMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_thread_address_uses_adapter_thread_id_shape() {
        let address = SlackThreadAddress::new("C123", "1710000000.000100");

        assert_eq!(address.chat_thread_id(), "slack:C123:1710000000.000100");
    }

    #[test]
    fn ingress_mode_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&SlackIngressMode::SocketMode).unwrap();

        assert_eq!(encoded, "\"socket_mode\"");
    }
}
