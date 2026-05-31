//! Service wiring contract for the Rust Open Agents Slack remote agent.
//!
//! This package owns the deployable binary. The first implementation bucket for
//! each subsystem should wire through this crate instead of adding another
//! process entrypoint.

#![forbid(unsafe_code)]

pub use open_agents_core as core;
pub use open_agents_runtime as runtime;
pub use open_agents_sandbox as sandbox;
pub use open_agents_slack as slack;

/// Binary name for the Slack remote-agent service.
pub const SERVICE_NAME: &str = "open-agents-slack";

/// Environment variables required to accept Slack traffic.
pub const REQUIRED_SLACK_ENV: &[&str] =
    &[slack::SLACK_BOT_TOKEN_ENV, slack::SLACK_SIGNING_SECRET_ENV];

/// Environment variables that enable optional live Slack proof.
pub const OPTIONAL_SLACK_TEST_ENV: &[&str] = &[
    slack::SLACK_APP_TOKEN_ENV,
    slack::SLACK_TEST_CHANNEL_ID_ENV,
    slack::SLACK_TEST_USER_ID_ENV,
];

/// Returns the required Slack environment variable names.
pub fn required_slack_env() -> &'static [&'static str] {
    REQUIRED_SLACK_ENV
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_slack_env_names_match_slack_contract() {
        assert_eq!(
            required_slack_env(),
            ["SLACK_BOT_TOKEN", "SLACK_SIGNING_SECRET"]
        );
    }
}
