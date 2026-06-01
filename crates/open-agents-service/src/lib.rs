//! Service wiring contract for the Rust Open Agents Slack remote agent.
//!
//! This package owns the deployable binary. The first implementation bucket for
//! each subsystem should wire through this crate instead of adding another
//! process entrypoint.

#![forbid(unsafe_code)]

pub mod config;
pub mod health;
pub mod local_fixture;
pub mod plugin;
pub mod sandbox_lifecycle;
pub mod service;
pub mod session_routes;
pub mod vercel_adapter;

pub use open_agents_core as core;
pub use open_agents_runtime as runtime;
pub use open_agents_sandbox as sandbox;
pub use open_agents_slack as slack;

pub use config::{ConfigError, OpenAgentsServiceConfig, SandboxMode, StateStore};
pub use health::{
    HealthCheck, HealthError, HealthSnapshot, bind_health_listener, serve_health_checks,
};
pub use local_fixture::{
    FixtureError, FixtureHarness, FixtureOutbound, FixtureOutboundKind, FixtureReply, FixtureRun,
    FixtureRunStatus, SLACK_ACTION_ANSWER, SLACK_ACTION_CANCEL,
};
pub use plugin::{
    OPEN_AGENTS_PLUGIN_DATA_DIR_ENV, OPEN_AGENTS_PLUGIN_ROOTS_ENV, OpenPluginCatalog,
    OpenPluginDiagnostic, OpenPluginDiagnosticLevel, OpenPluginError, OpenPluginMcpServer,
    OpenPluginPackage, OpenPluginSkill,
};
pub use sandbox_lifecycle::{
    ArchiveRecoveryPatch, LifecycleEvaluation, LifecycleKickDecision, LifecycleTiming,
    ReconnectProbeStatus, ReconnectRouteDecision, RouteResponse, SANDBOX_EXPIRES_BUFFER_MS,
    SANDBOX_INACTIVITY_TIMEOUT_MS, SnapshotRouteDecision, VercelProjectSelection,
    archive_failure_recovery_patch, evaluate_hibernation_decision, is_supported_github_repo_url,
    is_supported_sandbox_type, lifecycle_due_at_ms, lifecycle_kick_decision,
    persistent_sandbox_name, reconnect_route_decision, refreshed_archive_pr_status,
    repo_sandbox_uses_setup_only_installation_token, sandbox_creation_installs_global_skills,
    sandbox_status_decision, select_vercel_project_id, should_sync_linked_development_env_vars,
    snapshot_post_decision, snapshot_put_decision, vercel_project_env_not_found_response,
    vercel_repo_projects_invalid_token_response,
};
pub use service::{
    LocalOutbound, LocalOutboundKind, LocalRuntimeRouter, OpenAgentsService, ServiceError,
    ServiceHttpRequest, ServiceHttpResponse, bind_service_listener, serve_open_agents_service,
};

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
