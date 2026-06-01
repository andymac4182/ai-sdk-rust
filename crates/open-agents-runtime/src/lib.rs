//! Durable runtime contract for the Rust Open Agents Slack remote agent.
//!
//! Behavior lands in later buckets. This crate already anchors the runtime
//! dependency direction: Slack and service code call the runtime, while tools
//! reach sandboxes only through `open-agents-sandbox`.

#![forbid(unsafe_code)]

mod chat_state;
mod model_catalog;
mod open_agent;
mod session_title;

pub use chat_state::{
    CancelableReadableStream, CancelableStreamError, ChatRouteCleanupDependencies, ChatUiStatus,
    GitAction, GitFinalizationState, MERGE_READINESS_TRANSIENT_MAX_POLLS,
    MergeReadinessPollingState, NavbarGitActionState, WorkspaceStatusStore,
    WorkspaceStatusSubscription, cleanup_chat_route_on_unmount, dedupe_message_reasoning,
    get_git_finalization_state, get_navbar_git_action_state, has_renderable_assistant_part,
    is_abort_like_stream_error, is_chat_in_flight,
    should_increment_merge_readiness_transient_poll_count,
    should_keep_collapsed_reasoning_streaming, should_poll_merge_readiness,
    should_refresh_after_ready_transition, should_render_git_data_part,
    should_show_thinking_indicator, should_use_chat_list_streaming_state,
};
pub use model_catalog::{
    APP_DEFAULT_MODEL_ID, AvailableModel, AvailableModelCost, AvailableModelCostTier,
    BUILT_IN_VARIANT_ID_PREFIX, DEFAULT_MODEL_ID, GatewayAvailableModel, GatewayModelsError,
    MODEL_VARIANT_ID_PREFIX, ModelAccessSession, ModelGroup, ModelOption, ModelUsage, ModelVariant,
    ProviderOptionsByProvider, ResolvedModelSelection, UserModelPreferences,
    available_models_route_models, build_model_options, built_in_variants,
    estimate_model_usage_cost, filter_disabled_models, filter_model_variants_for_session,
    filter_models_for_session, get_all_variants, get_default_model_option_id,
    get_model_display_name, get_models_from_gateway_error, group_by_provider, is_built_in_variant,
    is_model_disabled, is_restricted_model_id_for_session, resolve_available_model_id,
    resolve_model_selection, sanitize_selected_model_id_for_session,
    sanitize_user_preferences_for_session, to_provider_options_by_provider,
    with_missing_model_option,
};
pub use open_agent::{
    DEFAULT_OPEN_AGENT_MODEL_LABEL, OpenAgent, OpenAgentCallOptions, OpenAgentError,
    OpenAgentModelVariant, OpenAgentPreparedCall, OpenAgentSettings, OpenAgentSkillMetadata,
    OpenAgentSkillOptions, OpenAgentSystemPromptOptions, OpenAgentUsageEvent, OpenAgentUsageHook,
    ResolvedChatModelSelection, build_open_agent_system_prompt,
    get_open_agent_provider_options_for_model, resolve_chat_model_selection,
};
pub use open_agents_core::{AgentModelSelection, RemoteAgentIdentity};
use open_agents_sandbox::SandboxContext;
use serde::{Deserialize, Serialize};
pub use session_title::{
    GenerateTitleRouteOutput, build_session_title_prompt, handle_generate_title_request,
    normalize_session_title_message, parse_generated_session_title,
};

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
