//! Cross-crate contracts for the Rust Open Agents Slack remote agent.
//!
//! This crate is intentionally small: it defines ids, statuses, event labels,
//! model selection, and source metadata that every Open Agents crate can share
//! without depending on Slack, workflow, or sandbox implementation details.

#![forbid(unsafe_code)]

pub mod open_plugin;
pub mod plugin;

use serde::{Deserialize, Serialize};

pub use open_plugin::{
    OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH, OPEN_PLUGIN_INVALID_OBJECT_EVENT,
    OPEN_PLUGIN_MANIFEST_PATH, OPEN_PLUGIN_MCP_NAME_CONFLICT_EVENT, OpenPluginDiagnostic,
    OpenPluginDiagnosticLevel, OpenPluginManifest, OpenPluginMcpConfigSource,
    OpenPluginMcpDiscovery, OpenPluginMcpDiscoveryOptions, OpenPluginMcpHttpConfig,
    OpenPluginMcpLoadError, OpenPluginMcpManifestAdapter, OpenPluginMcpServerConfig,
    OpenPluginMcpStdioConfig, OpenPluginMcpTransportConfig,
    discover_open_plugin_mcp_servers_from_manifest, load_open_plugin_mcp_servers,
    open_plugin_mcp_tool_id,
};

/// Upstream Open Agents repository verified for the initial architecture pass.
pub const OPEN_AGENTS_SOURCE_REPOSITORY: &str = "github.com/vercel-labs/open-agents";

/// Upstream Open Agents ref fetched by OpenSrc.
pub const OPEN_AGENTS_SOURCE_REF: &str = "main";

/// Remote HEAD verified with `git ls-remote` on 2026-05-31.
pub const OPEN_AGENTS_SOURCE_HEAD: &str = "24d679c7ba3d274aa73814c15673aeffcbe3c1c2";

/// OpenSrc registry timestamp for the source mirror used by bucket 01.
pub const OPEN_AGENTS_SOURCE_FETCHED_AT: &str = "2026-05-31T09:58:27.398325+00:00";

/// Source metadata that can be attached to docs, state migrations, or tests.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAgentsSourceSnapshot {
    /// Repository identifier.
    pub repository: String,
    /// Ref fetched into the local mirror.
    pub ref_name: String,
    /// Verified remote HEAD.
    pub head: String,
    /// OpenSrc registry timestamp.
    pub fetched_at: String,
}

impl Default for OpenAgentsSourceSnapshot {
    fn default() -> Self {
        Self {
            repository: OPEN_AGENTS_SOURCE_REPOSITORY.to_string(),
            ref_name: OPEN_AGENTS_SOURCE_REF.to_string(),
            head: OPEN_AGENTS_SOURCE_HEAD.to_string(),
            fetched_at: OPEN_AGENTS_SOURCE_FETCHED_AT.to_string(),
        }
    }
}

/// Model selection passed to the remote-agent runtime.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelSelection {
    /// Gateway model id, for example `anthropic/claude-opus-4.6`.
    pub id: String,
    /// Provider-specific options that survive durable-run serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<serde_json::Value>,
}

impl AgentModelSelection {
    /// Creates a model selection with no provider option overrides.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider_options: None,
        }
    }

    /// Attaches provider option overrides.
    pub fn with_provider_options(mut self, provider_options: serde_json::Value) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Durable remote-agent run status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentRunStatus {
    /// The run is accepted but not yet executing model steps.
    Queued,
    /// The run is actively streaming or executing tools.
    Running,
    /// The run is waiting for a Slack approval decision.
    WaitingForApproval,
    /// The run is waiting for a Slack user answer.
    WaitingForUser,
    /// The run finished successfully.
    Completed,
    /// The run failed and no more steps will execute.
    Failed,
    /// The run was cancelled by the user or service.
    Cancelled,
}

/// Durable session lifecycle status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentSessionStatus {
    /// Session can accept user messages.
    Active,
    /// Session is being provisioned or resumed.
    Provisioning,
    /// Session is paused while its sandbox is hibernated.
    Hibernated,
    /// Session was archived and must not start new runs.
    Archived,
    /// Session setup failed.
    Failed,
}

/// Event kinds emitted by the runtime and rendered by surfaces such as Slack.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentEventKind {
    /// A user message was accepted.
    UserMessage,
    /// Assistant text or reasoning streamed.
    AssistantDelta,
    /// A tool call entered an executing state.
    ToolStarted,
    /// A tool call produced output or an error.
    ToolFinished,
    /// A tool requires an explicit approval decision.
    ApprovalRequested,
    /// The agent asked the user a structured question.
    UserQuestionRequested,
    /// Usage or cost metadata was recorded.
    UsageRecorded,
    /// The run completed, failed, or was cancelled.
    Finished,
}

/// Stable identifiers shared by persistence, runtime, and Slack routing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentIdentity {
    /// Durable agent session id.
    pub session_id: String,
    /// Durable chat id inside the session.
    pub chat_id: String,
    /// Current run id, when one is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl RemoteAgentIdentity {
    /// Creates an identity before a run is started.
    pub fn new(session_id: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            chat_id: chat_id.into(),
            run_id: None,
        }
    }

    /// Attaches an active run id.
    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
}

/// Secret names that may be persisted by name, never by value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemoteAgentSecretName {
    /// Slack bot OAuth token.
    SlackBotToken,
    /// Slack request signing secret.
    SlackSigningSecret,
    /// Slack Socket Mode app token.
    SlackAppToken,
    /// Vercel AI Gateway model credential.
    AiGatewayApiKey,
    /// Existing repository alias for the AI Gateway model credential.
    AiSdkRustAiGatewayApiKey,
    /// GitHub App id.
    GitHubAppId,
    /// GitHub App private key.
    GitHubAppPrivateKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_snapshot_uses_verified_remote_head() {
        let snapshot = OpenAgentsSourceSnapshot::default();

        assert_eq!(snapshot.repository, OPEN_AGENTS_SOURCE_REPOSITORY);
        assert_eq!(snapshot.ref_name, "main");
        assert_eq!(snapshot.head.len(), 40);
        assert!(snapshot.fetched_at.starts_with("2026-05-31T"));
    }

    #[test]
    fn run_status_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&RemoteAgentRunStatus::WaitingForApproval).unwrap();

        assert_eq!(encoded, "\"waiting_for_approval\"");
    }
}
