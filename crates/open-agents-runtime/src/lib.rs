//! Durable runtime contract for the Rust Open Agents Slack remote agent.
//!
//! Behavior lands in later buckets. This crate already anchors the runtime
//! dependency direction: Slack and service code call the runtime, while tools
//! reach sandboxes only through `open-agents-sandbox`.

#![forbid(unsafe_code)]

use open_agents_core::{AgentModelSelection, RemoteAgentIdentity};
use open_agents_sandbox::SandboxContext;
use serde::{Deserialize, Serialize};

pub use ai_sdk_rust::ToolLoopAgent;
pub use ai_sdk_workflow::WorkflowAgent;

/// Bucket that owns the first runtime implementation.
pub const OWNER_BUCKET: u8 = 2;

/// Open Agents subagents use up to 100 tool steps.
pub const DEFAULT_SUBAGENT_STEP_LIMIT: usize = 100;

/// The main Open Agents TypeScript agent starts one model step per workflow
/// iteration and lets the durable loop decide whether to continue.
pub const DEFAULT_WORKFLOW_AGENT_STEP_LIMIT: usize = 1;

/// Input required to start or resume a durable remote-agent run.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentRunRequest {
    /// Durable identity for session, chat, and optional run.
    pub identity: RemoteAgentIdentity,
    /// Model selected for the main agent.
    pub model: AgentModelSelection,
    /// Optional model selected for delegated subagents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_model: Option<AgentModelSelection>,
    /// Sandbox context exposed to tools and system prompt construction.
    pub sandbox: SandboxContext,
    /// Prompt or UI-message parts already normalized by the chat bridge.
    pub messages: Vec<serde_json::Value>,
    /// Optional user or workspace instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
}

impl RemoteAgentRunRequest {
    /// Creates a request with no messages.
    pub fn new(
        identity: RemoteAgentIdentity,
        model: AgentModelSelection,
        sandbox: SandboxContext,
    ) -> Self {
        Self {
            identity,
            model,
            subagent_model: None,
            sandbox,
            messages: Vec::new(),
            custom_instructions: None,
        }
    }
}

/// Durable timing and finish metadata for one model/tool-loop step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgentStepRecord {
    /// One-based step number inside the durable run.
    pub step_number: u32,
    /// RFC 3339 timestamp captured at step start.
    pub started_at: String,
    /// RFC 3339 timestamp captured at step finish.
    pub finished_at: String,
    /// Wall-clock step duration in milliseconds.
    pub duration_ms: u64,
    /// Portable AI SDK finish reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Raw provider finish reason, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_agents_core::RemoteAgentIdentity;
    use open_agents_sandbox::SandboxContext;
    use serde_json::json;

    #[test]
    fn run_request_preserves_runtime_inputs() {
        let request = RemoteAgentRunRequest::new(
            RemoteAgentIdentity::new("session-1", "chat-1").with_run_id("run-1"),
            AgentModelSelection::new("anthropic/claude-opus-4.6"),
            SandboxContext::new(json!({"type": "vercel"}), "/workspace"),
        );

        assert_eq!(request.identity.run_id.as_deref(), Some("run-1"));
        assert_eq!(request.model.id, "anthropic/claude-opus-4.6");
        assert_eq!(request.sandbox.working_directory, "/workspace");
    }
}
