//! Open Agents runtime adapter around the portable AI SDK tool-loop agent.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use ai_sdk_rust::{
    FinishReason, GenerateTextResult, GenerateTextTool, Instructions, InvalidPromptError,
    JsonObject, JsonValue, LanguageModel, LanguageModelStreamPart, LanguageModelUsage, Prompt,
    PromptInput, ProviderOptions, StreamTextResult, ToolLoopAgent, ToolLoopAgentCallOptions,
    ToolLoopAgentModelSettings, ToolLoopAgentPreparedCall, ToolLoopAgentSettings,
};
use open_agents_core::AgentModelSelection;
use open_agents_sandbox::SandboxContext;

/// Upstream Open Agents default model label.
pub const DEFAULT_OPEN_AGENT_MODEL_LABEL: &str = "anthropic/claude-opus-4.6";

/// Skill invocation options exposed in the system prompt.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAgentSkillOptions {
    /// If true, the model must not invoke this skill automatically.
    #[serde(default, skip_serializing_if = "is_false")]
    pub disable_model_invocation: bool,

    /// If false, users cannot invoke this skill with a slash command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_invocable: Option<bool>,

    /// Tools allowed while the skill is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,

    /// Execution context for the skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Agent type used for execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

impl OpenAgentSkillOptions {
    /// Creates empty skill options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether model invocation is disabled.
    pub fn with_disable_model_invocation(mut self, disabled: bool) -> Self {
        self.disable_model_invocation = disabled;
        self
    }

    /// Sets whether the skill is user-invocable.
    pub fn with_user_invocable(mut self, user_invocable: bool) -> Self {
        self.user_invocable = Some(user_invocable);
        self
    }

    /// Sets allowed tools for this skill.
    pub fn with_allowed_tools(
        mut self,
        allowed_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_tools = allowed_tools.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the execution context label.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Sets the agent type.
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }
}

/// Skill metadata stored in Open Agent call context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAgentSkillMetadata {
    pub name: String,
    pub description: String,
    pub path: String,
    pub filename: String,
    pub options: OpenAgentSkillOptions,
}

impl OpenAgentSkillMetadata {
    /// Creates skill metadata.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        path: impl Into<String>,
        filename: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            path: path.into(),
            filename: filename.into(),
            options: OpenAgentSkillOptions::new(),
        }
    }

    /// Sets normalized skill options.
    pub fn with_options(mut self, options: OpenAgentSkillOptions) -> Self {
        self.options = options;
        self
    }
}

/// Sanitized plugin MCP server metadata exposed to planning/runtime context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAgentPluginMcpServer {
    /// Open Plugin package name.
    pub plugin_name: String,

    /// MCP server name inside the plugin.
    pub server_name: String,

    /// Recommended MCP tool identifier prefix for tools from this server.
    pub tool_prefix: String,

    /// Manifest or config file source used for discovery.
    pub source: String,

    /// Expanded command, when the config uses a stdio-like command field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Expanded cwd, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Environment variable names configured for the server. Values are not
    /// exposed to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,

    /// Whether the discovered config included argv entries. Argument values are
    /// intentionally not exposed because they may contain operator secrets.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_args: bool,
}

impl OpenAgentPluginMcpServer {
    /// Creates a sanitized plugin MCP planning surface.
    pub fn new(
        plugin_name: impl Into<String>,
        server_name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let plugin_name = plugin_name.into();
        let server_name = server_name.into();
        Self {
            tool_prefix: format!("mcp__plugin_{plugin_name}_{server_name}__"),
            plugin_name,
            server_name,
            source: source.into(),
            command: None,
            cwd: None,
            env_keys: Vec::new(),
            has_args: false,
        }
    }

    /// Sets the expanded command field.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Sets the expanded cwd field.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Sets configured environment variable names.
    pub fn with_env_keys(mut self, env_keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.env_keys = env_keys.into_iter().map(Into::into).collect();
        self
    }

    /// Records whether argv entries were present.
    pub const fn with_has_args(mut self, has_args: bool) -> Self {
        self.has_args = has_args;
        self
    }
}

/// Usage event emitted when an Open Agent run finishes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenAgentUsageEvent {
    pub model_id: String,
    pub subagent_model_id: Option<String>,
    pub usage: LanguageModelUsage,
    pub finish_reason: FinishReason,
}

type OpenAgentUsageFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked with aggregate usage for later persistence.
pub struct OpenAgentUsageHook<'a> {
    on_usage: Rc<dyn Fn(OpenAgentUsageEvent) -> OpenAgentUsageFuture<'a> + 'a>,
}

impl<'a> OpenAgentUsageHook<'a> {
    /// Creates a usage hook.
    pub fn new<F, Fut>(on_usage: F) -> Self
    where
        F: Fn(OpenAgentUsageEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_usage: Rc::new(move |event| Box::pin(on_usage(event))),
        }
    }

    /// Records one usage event.
    pub fn record(&self, event: OpenAgentUsageEvent) -> OpenAgentUsageFuture<'a> {
        (self.on_usage)(event)
    }
}

impl Clone for OpenAgentUsageHook<'_> {
    fn clone(&self) -> Self {
        Self {
            on_usage: Rc::clone(&self.on_usage),
        }
    }
}

impl fmt::Debug for OpenAgentUsageHook<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAgentUsageHook")
            .finish_non_exhaustive()
    }
}

/// Settings used to construct an [`OpenAgent`].
pub struct OpenAgentSettings<'a, M: LanguageModel + ?Sized> {
    pub model: &'a M,
    pub model_id: String,
    pub registered_models: BTreeMap<String, &'a M>,
    pub model_settings: ToolLoopAgentModelSettings,
    pub tools: Vec<GenerateTextTool>,
    pub custom_instructions: Option<String>,
    pub usage_hook: Option<OpenAgentUsageHook<'a>>,
}

/// Model variant record used by the chat route before starting a workflow.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenAgentModelVariant {
    pub id: String,
    pub base_model_id: String,
    pub provider_options: Option<JsonValue>,
}

impl OpenAgentModelVariant {
    pub fn new(id: impl Into<String>, base_model_id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base_model_id: base_model_id.into(),
            provider_options: None,
        }
    }

    pub fn with_provider_options(mut self, provider_options: JsonValue) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Result of resolving a chat-selected model id into an agent selection.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedChatModelSelection {
    pub selection: AgentModelSelection,
    pub warning: Option<String>,
}

/// Resolves direct model ids, configured variants, built-in OpenAI variants,
/// and missing values into the remote-agent model selection shape.
pub fn resolve_chat_model_selection(
    selected_model_id: Option<&str>,
    model_variants: &[OpenAgentModelVariant],
    missing_variant_label: &str,
) -> ResolvedChatModelSelection {
    let requested_model_id = selected_model_id.unwrap_or(DEFAULT_OPEN_AGENT_MODEL_LABEL);
    if !requested_model_id.starts_with("variant:") {
        return ResolvedChatModelSelection {
            selection: AgentModelSelection::new(requested_model_id),
            warning: None,
        };
    }

    if requested_model_id == "variant:builtin:gpt-5.4-xhigh" {
        return ResolvedChatModelSelection {
            selection: AgentModelSelection::new("openai/gpt-5.4").with_provider_options(json!({
                "openai": {
                    "reasoningEffort": "xhigh",
                    "reasoningSummary": "auto",
                    "store": false
                }
            })),
            warning: None,
        };
    }

    if let Some(variant) = model_variants
        .iter()
        .find(|variant| variant.id == requested_model_id)
    {
        let mut selection = AgentModelSelection::new(variant.base_model_id.clone());
        if let Some(provider_options) = provider_options_for_variant(variant) {
            selection = selection.with_provider_options(provider_options);
        }
        return ResolvedChatModelSelection {
            selection,
            warning: None,
        };
    }

    ResolvedChatModelSelection {
        selection: AgentModelSelection::new(DEFAULT_OPEN_AGENT_MODEL_LABEL),
        warning: Some(format!(
            "{missing_variant_label} \"{requested_model_id}\" was not found. Falling back to default model."
        )),
    }
}

fn provider_options_for_variant(variant: &OpenAgentModelVariant) -> Option<JsonValue> {
    let provider = variant.base_model_id.split_once('/')?.0;
    let mut options = variant
        .provider_options
        .clone()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if provider == "openai" {
        options.insert("store".to_string(), JsonValue::Bool(false));
    }
    if options.is_empty() {
        return None;
    }
    Some(json!({ provider: JsonValue::Object(options) }))
}

impl<'a, M: LanguageModel + ?Sized> OpenAgentSettings<'a, M> {
    /// Creates Open Agent settings for the default model.
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            model_id: DEFAULT_OPEN_AGENT_MODEL_LABEL.to_string(),
            registered_models: BTreeMap::new(),
            model_settings: ToolLoopAgentModelSettings::new(),
            tools: Vec::new(),
            custom_instructions: None,
            usage_hook: None,
        }
    }

    /// Sets the id associated with the default model reference.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Registers an additional model id for per-call model resolution.
    pub fn with_registered_model(mut self, model_id: impl Into<String>, model: &'a M) -> Self {
        self.registered_models.insert(model_id.into(), model);
        self
    }

    /// Sets default model call settings.
    pub fn with_model_settings(mut self, model_settings: ToolLoopAgentModelSettings) -> Self {
        self.model_settings = model_settings;
        self
    }

    /// Adds a tool to every Open Agent call.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        self.tools.push(tool.into());
        self
    }

    /// Sets default project-specific instructions appended to each call.
    pub fn with_custom_instructions(mut self, custom_instructions: impl Into<String>) -> Self {
        self.custom_instructions = Some(custom_instructions.into());
        self
    }

    /// Sets a usage accounting hook.
    pub fn with_usage_hook<F, Fut>(mut self, on_usage: F) -> Self
    where
        F: Fn(OpenAgentUsageEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.usage_hook = Some(OpenAgentUsageHook::new(on_usage));
        self
    }
}

/// Per-call Open Agent options plus lower-level tool-loop options.
pub struct OpenAgentCallOptions<'a, M: LanguageModel + ?Sized> {
    pub tool_loop_options: ToolLoopAgentCallOptions<'a, M>,
    pub sandbox: Option<SandboxContext>,
    pub model: Option<AgentModelSelection>,
    pub subagent_model: Option<AgentModelSelection>,
    pub custom_instructions: Option<String>,
    pub skills: Vec<OpenAgentSkillMetadata>,
    pub plugin_mcp_servers: Vec<OpenAgentPluginMcpServer>,
}

impl<'a, M: LanguageModel + ?Sized> OpenAgentCallOptions<'a, M> {
    /// Creates call options with required sandbox context.
    pub fn new(prompt: Prompt, sandbox: SandboxContext) -> Self {
        Self {
            tool_loop_options: ToolLoopAgentCallOptions::new(prompt),
            sandbox: Some(sandbox),
            model: None,
            subagent_model: None,
            custom_instructions: None,
            skills: Vec::new(),
            plugin_mcp_servers: Vec::new(),
        }
    }

    /// Creates call options from text.
    pub fn from_prompt(prompt: impl Into<PromptInput>, sandbox: SandboxContext) -> Self {
        Self::new(Prompt::from_prompt(prompt), sandbox)
    }

    /// Creates call options without sandbox context for validation tests.
    pub fn without_sandbox(prompt: impl Into<PromptInput>) -> Self {
        Self {
            tool_loop_options: ToolLoopAgentCallOptions::from_prompt(prompt),
            sandbox: None,
            model: None,
            subagent_model: None,
            custom_instructions: None,
            skills: Vec::new(),
            plugin_mcp_servers: Vec::new(),
        }
    }

    /// Sets the main model selection.
    pub fn with_model(mut self, model: AgentModelSelection) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets the subagent model selection stored in runtime context.
    pub fn with_subagent_model(mut self, subagent_model: AgentModelSelection) -> Self {
        self.subagent_model = Some(subagent_model);
        self
    }

    /// Sets custom instructions for this call.
    pub fn with_custom_instructions(mut self, custom_instructions: impl Into<String>) -> Self {
        self.custom_instructions = Some(custom_instructions.into());
        self
    }

    /// Adds one selected skill.
    pub fn with_skill(mut self, skill: OpenAgentSkillMetadata) -> Self {
        self.skills.push(skill);
        self
    }

    /// Sets selected skills.
    pub fn with_skills(mut self, skills: impl IntoIterator<Item = OpenAgentSkillMetadata>) -> Self {
        self.skills = skills.into_iter().collect();
        self
    }

    /// Sets discovered plugin MCP servers for planning/runtime context.
    pub fn with_plugin_mcp_servers(
        mut self,
        servers: impl IntoIterator<Item = OpenAgentPluginMcpServer>,
    ) -> Self {
        self.plugin_mcp_servers = servers.into_iter().collect();
        self
    }

    /// Sets per-call model settings.
    pub fn with_model_settings(mut self, model_settings: ToolLoopAgentModelSettings) -> Self {
        self.tool_loop_options.model_settings = model_settings;
        self
    }

    /// Adds a per-call tool.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        self.tool_loop_options = self.tool_loop_options.with_tool(tool);
        self
    }
}

/// Prepared Open Agent call with typed metadata and tool-loop settings.
pub struct OpenAgentPreparedCall<'a, M: LanguageModel + ?Sized> {
    pub model_id: String,
    pub subagent_model_id: Option<String>,
    pub sandbox: SandboxContext,
    pub skills: Vec<OpenAgentSkillMetadata>,
    pub plugin_mcp_servers: Vec<OpenAgentPluginMcpServer>,
    pub tool_loop: ToolLoopAgentPreparedCall<'a, M>,
}

/// Error returned by Open Agent preparation and execution.
#[derive(Debug)]
pub enum OpenAgentError {
    MissingSandbox,
    UnknownModel {
        model_id: String,
    },
    InvalidProviderOptions {
        model_id: String,
        source: serde_json::Error,
    },
    InvalidPrompt(InvalidPromptError),
}

impl fmt::Display for OpenAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSandbox => formatter.write_str("Open Agent requires sandbox context"),
            Self::UnknownModel { model_id } => {
                write!(formatter, "Open Agent model '{model_id}' is not registered")
            }
            Self::InvalidProviderOptions { model_id, source } => {
                write!(
                    formatter,
                    "Open Agent provider options for model '{model_id}' are invalid: {source}"
                )
            }
            Self::InvalidPrompt(error) => error.fmt(formatter),
        }
    }
}

impl Error for OpenAgentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidProviderOptions { source, .. } => Some(source),
            Self::InvalidPrompt(error) => Some(error),
            Self::MissingSandbox | Self::UnknownModel { .. } => None,
        }
    }
}

impl PartialEq for OpenAgentError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MissingSandbox, Self::MissingSandbox) => true,
            (Self::UnknownModel { model_id: left }, Self::UnknownModel { model_id: right }) => {
                left == right
            }
            (
                Self::InvalidProviderOptions { model_id: left, .. },
                Self::InvalidProviderOptions {
                    model_id: right, ..
                },
            ) => left == right,
            _ => false,
        }
    }
}

impl From<InvalidPromptError> for OpenAgentError {
    fn from(error: InvalidPromptError) -> Self {
        Self::InvalidPrompt(error)
    }
}

/// Rust equivalent of Open Agents `openAgent` around [`ToolLoopAgent`].
pub struct OpenAgent<'a, M: LanguageModel + ?Sized> {
    tool_loop_agent: ToolLoopAgent<'a, M>,
    default_model: &'a M,
    default_model_id: String,
    registered_models: BTreeMap<String, &'a M>,
    default_custom_instructions: Option<String>,
    usage_hook: Option<OpenAgentUsageHook<'a>>,
}

impl<'a, M: LanguageModel + ?Sized> OpenAgent<'a, M> {
    /// Creates an Open Agent from explicit settings.
    pub fn new(settings: OpenAgentSettings<'a, M>) -> Self {
        let mut tool_loop_settings = ToolLoopAgentSettings::new(settings.model)
            .with_instructions(build_open_agent_system_prompt(
                OpenAgentSystemPromptOptions {
                    model_id: Some(settings.model_id.clone()),
                    ..OpenAgentSystemPromptOptions::default()
                },
            ))
            .with_model_settings(settings.model_settings)
            .with_max_steps(1);
        for tool in settings.tools {
            tool_loop_settings = tool_loop_settings.with_tool(tool);
        }

        Self {
            tool_loop_agent: ToolLoopAgent::new(tool_loop_settings),
            default_model: settings.model,
            default_model_id: settings.model_id,
            registered_models: settings.registered_models,
            default_custom_instructions: settings.custom_instructions,
            usage_hook: settings.usage_hook,
        }
    }

    /// Creates an Open Agent for the upstream default model id.
    pub fn for_model(model: &'a M) -> Self {
        Self::new(OpenAgentSettings::new(model))
    }

    /// Returns the wrapped tool-loop agent.
    pub fn tool_loop_agent(&self) -> &ToolLoopAgent<'a, M> {
        &self.tool_loop_agent
    }

    /// Validates and prepares a call without invoking the model.
    pub fn prepare_call(
        &self,
        options: OpenAgentCallOptions<'a, M>,
    ) -> Result<OpenAgentPreparedCall<'a, M>, OpenAgentError> {
        let (tool_loop_options, metadata) = self.prepare_tool_loop_options(options)?;
        let tool_loop = self.tool_loop_agent.prepare_call(tool_loop_options)?;

        Ok(OpenAgentPreparedCall {
            model_id: metadata.model_id,
            subagent_model_id: metadata.subagent_model_id,
            sandbox: metadata.sandbox,
            skills: metadata.skills,
            plugin_mcp_servers: metadata.plugin_mcp_servers,
            tool_loop,
        })
    }

    /// Generates a non-streaming response.
    pub async fn generate(
        &self,
        options: OpenAgentCallOptions<'a, M>,
    ) -> Result<GenerateTextResult, OpenAgentError> {
        let (tool_loop_options, _) = self.prepare_tool_loop_options(options)?;
        self.tool_loop_agent
            .generate(tool_loop_options)
            .await
            .map_err(Into::into)
    }

    /// Streams a response.
    pub async fn stream(
        &self,
        options: OpenAgentCallOptions<'a, M>,
    ) -> Result<StreamTextResult, OpenAgentError>
    where
        M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
    {
        let (tool_loop_options, _) = self.prepare_tool_loop_options(options)?;
        self.tool_loop_agent
            .stream(tool_loop_options)
            .await
            .map_err(Into::into)
    }

    fn resolve_model(&self, model_id: &str) -> Result<&'a M, OpenAgentError> {
        if model_id == self.default_model_id {
            return Ok(self.default_model);
        }
        self.registered_models
            .get(model_id)
            .copied()
            .ok_or_else(|| OpenAgentError::UnknownModel {
                model_id: model_id.to_string(),
            })
    }

    fn prepare_tool_loop_options(
        &self,
        options: OpenAgentCallOptions<'a, M>,
    ) -> Result<(ToolLoopAgentCallOptions<'a, M>, OpenAgentPreparedMetadata), OpenAgentError> {
        let OpenAgentCallOptions {
            mut tool_loop_options,
            sandbox,
            model,
            subagent_model,
            custom_instructions,
            skills,
            plugin_mcp_servers,
        } = options;
        let sandbox = sandbox.ok_or(OpenAgentError::MissingSandbox)?;
        let model_selection =
            model.unwrap_or_else(|| AgentModelSelection::new(self.default_model_id.clone()));
        let provider_options_overrides = provider_options_from_selection(&model_selection)?;
        let call_model = self.resolve_model(&model_selection.id)?;
        let subagent_model_id = if let Some(selection) = subagent_model {
            self.resolve_model(&selection.id)?;
            Some(selection.id)
        } else {
            None
        };
        let custom_instructions =
            custom_instructions.or_else(|| self.default_custom_instructions.clone());
        let prompt_options = OpenAgentSystemPromptOptions {
            cwd: Some(sandbox.working_directory.clone()),
            current_branch: sandbox.current_branch.clone(),
            custom_instructions,
            environment_details: sandbox.environment_details.clone(),
            skills: skills.clone(),
            plugin_mcp_servers: plugin_mcp_servers.clone(),
            model_id: Some(model_selection.id.clone()),
        };
        let instructions = build_open_agent_system_prompt(prompt_options);
        let runtime_context = open_agent_runtime_context(
            &sandbox,
            &skills,
            &plugin_mcp_servers,
            &model_selection.id,
            subagent_model_id.as_deref(),
        );

        tool_loop_options.model = Some(call_model);
        tool_loop_options.instructions = Some(Instructions::text(instructions));
        tool_loop_options
            .runtime_context
            .insert("openAgent".to_string(), runtime_context.clone());
        tool_loop_options.call_options = Some(runtime_context);

        let provider_options = get_open_agent_provider_options_for_model(
            &model_selection.id,
            provider_options_overrides,
        );
        if !provider_options.is_empty() {
            tool_loop_options.model_settings.provider_options = Some(
                if let Some(explicit) = tool_loop_options.model_settings.provider_options.take() {
                    merge_provider_options(provider_options, Some(explicit))
                } else {
                    provider_options
                },
            );
        }

        if let Some(usage_hook) = self.usage_hook.clone() {
            let model_id = model_selection.id.clone();
            let subagent_model_id_for_hook = subagent_model_id.clone();
            tool_loop_options = tool_loop_options.with_on_finish(move |event| {
                let usage_hook = usage_hook.clone();
                let model_id = model_id.clone();
                let subagent_model_id = subagent_model_id_for_hook.clone();
                async move {
                    usage_hook
                        .record(OpenAgentUsageEvent {
                            model_id,
                            subagent_model_id,
                            usage: event.total_usage,
                            finish_reason: event.finish_reason,
                        })
                        .await;
                }
            });
        }

        Ok((
            tool_loop_options,
            OpenAgentPreparedMetadata {
                model_id: model_selection.id,
                subagent_model_id,
                sandbox,
                skills,
                plugin_mcp_servers,
            },
        ))
    }
}

struct OpenAgentPreparedMetadata {
    model_id: String,
    subagent_model_id: Option<String>,
    sandbox: SandboxContext,
    skills: Vec<OpenAgentSkillMetadata>,
    plugin_mcp_servers: Vec<OpenAgentPluginMcpServer>,
}

/// Inputs used to build an Open Agent system prompt.
#[derive(Default)]
pub struct OpenAgentSystemPromptOptions {
    pub cwd: Option<String>,
    pub current_branch: Option<String>,
    pub custom_instructions: Option<String>,
    pub environment_details: Option<String>,
    pub skills: Vec<OpenAgentSkillMetadata>,
    pub plugin_mcp_servers: Vec<OpenAgentPluginMcpServer>,
    pub model_id: Option<String>,
}

/// Builds the Open Agent system prompt from sandbox, skill, and model context.
pub fn build_open_agent_system_prompt(options: OpenAgentSystemPromptOptions) -> String {
    let mut parts = vec![
        CORE_SYSTEM_PROMPT.to_string(),
        model_overlay(options.model_id.as_deref()).to_string(),
    ];

    if let Some(cwd) = options.cwd {
        parts.push(format!(
            "\n# Environment\n\nWorking directory: {cwd}\nUse workspace-relative paths for file operations."
        ));
        if let Some(environment_details) = options.environment_details {
            parts.push(format!("\n{environment_details}"));
        }
    }

    if let Some(current_branch) = options.current_branch {
        parts.push(format!("\nCurrent branch: {current_branch}"));
        parts.push(CLOUD_SANDBOX_INSTRUCTIONS.to_string());
    }

    if let Some(custom_instructions) = options.custom_instructions {
        parts.push(format!(
            "\n# Project-Specific Instructions\n\n{custom_instructions}"
        ));
    }

    if let Some(skills) = build_skills_prompt(&options.skills) {
        parts.push(skills);
    }

    if let Some(plugin_mcp_servers) = build_plugin_mcp_prompt(&options.plugin_mcp_servers) {
        parts.push(plugin_mcp_servers);
    }

    parts.join("\n")
}

/// Returns Open Agents provider defaults for a model, merged with overrides.
pub fn get_open_agent_provider_options_for_model(
    model_id: &str,
    provider_options_overrides: Option<ProviderOptions>,
) -> ProviderOptions {
    let mut defaults = ProviderOptions::new();

    if model_id.starts_with("anthropic/") {
        let thinking = if supports_adaptive_anthropic_thinking(model_id) {
            json_object(json!({
                "effort": "medium",
                "thinking": { "type": "adaptive" }
            }))
        } else {
            json_object(json!({
                "thinking": {
                    "type": "enabled",
                    "budgetTokens": 8000
                }
            }))
        };
        defaults.insert("anthropic".to_string(), thinking);
    }

    if model_id.starts_with("openai/") {
        defaults.insert(
            "openai".to_string(),
            json_object(json!({
                "store": false
            })),
        );
    }

    if should_apply_openai_reasoning_defaults(model_id) {
        let existing = defaults.remove("openai").unwrap_or_default();
        defaults.insert(
            "openai".to_string(),
            merge_json_object(
                existing,
                json_object(json!({
                    "reasoningSummary": "detailed",
                    "include": ["reasoning.encrypted_content"]
                })),
            ),
        );
    }

    if should_apply_openai_text_verbosity_defaults(model_id) {
        let existing = defaults.remove("openai").unwrap_or_default();
        defaults.insert(
            "openai".to_string(),
            merge_json_object(
                existing,
                json_object(json!({
                    "textVerbosity": "low"
                })),
            ),
        );
    }

    let mut provider_options = merge_provider_options(defaults, provider_options_overrides);

    if model_id.starts_with("openai/") {
        provider_options
            .entry("openai".to_string())
            .or_default()
            .insert("store".to_string(), JsonValue::Bool(false));
    }

    provider_options
}

fn open_agent_runtime_context(
    sandbox: &SandboxContext,
    skills: &[OpenAgentSkillMetadata],
    plugin_mcp_servers: &[OpenAgentPluginMcpServer],
    model_id: &str,
    subagent_model_id: Option<&str>,
) -> JsonValue {
    json!({
        "sandbox": sandbox,
        "skills": skills,
        "pluginMcpServers": plugin_mcp_servers,
        "modelId": model_id,
        "subagentModelId": subagent_model_id,
    })
}

fn provider_options_from_selection(
    selection: &AgentModelSelection,
) -> Result<Option<ProviderOptions>, OpenAgentError> {
    selection
        .provider_options
        .clone()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|source| OpenAgentError::InvalidProviderOptions {
            model_id: selection.id.clone(),
            source,
        })
}

fn supports_adaptive_anthropic_thinking(model_id: &str) -> bool {
    model_id.contains("4.6") || model_id.contains("4.7")
}

fn should_apply_openai_reasoning_defaults(model_id: &str) -> bool {
    model_id.starts_with("openai/gpt-5")
}

fn should_apply_openai_text_verbosity_defaults(model_id: &str) -> bool {
    model_id.starts_with("openai/gpt-5.4")
}

fn merge_provider_options(
    defaults: ProviderOptions,
    overrides: Option<ProviderOptions>,
) -> ProviderOptions {
    let Some(overrides) = overrides else {
        return defaults;
    };
    let mut merged = defaults;
    for (provider, provider_overrides) in overrides {
        let provider_defaults = merged.remove(&provider).unwrap_or_default();
        merged.insert(
            provider,
            merge_json_object(provider_defaults, provider_overrides),
        );
    }
    merged
}

fn merge_json_object(mut base: JsonObject, overrides: JsonObject) -> JsonObject {
    for (key, value) in overrides {
        if let Some(JsonValue::Object(existing_object)) = base.get_mut(&key) {
            if let JsonValue::Object(override_object) = &value {
                let merged =
                    merge_json_object(std::mem::take(existing_object), override_object.clone());
                *existing_object = merged;
                continue;
            }
        }
        base.insert(key, value);
    }
    base
}

fn json_object(value: JsonValue) -> JsonObject {
    value
        .as_object()
        .cloned()
        .expect("static JSON value is an object")
}

fn build_skills_prompt(skills: &[OpenAgentSkillMetadata]) -> Option<String> {
    let invocable_skills = skills
        .iter()
        .filter(|skill| !skill.options.disable_model_invocation)
        .collect::<Vec<_>>();
    if invocable_skills.is_empty() {
        return None;
    }

    let skills_list = invocable_skills
        .into_iter()
        .map(|skill| {
            let suffix = if skill.options.user_invocable == Some(false) {
                " (model-only)"
            } else {
                ""
            };
            format!("- {}: {}{}", skill.name, skill.description, suffix)
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "\n## Skills\n- `skill` - Execute a skill to extend your capabilities\n- Use the `skill` tool when a listed skill is relevant\n\nAvailable skills:\n{skills_list}"
    ))
}

fn build_plugin_mcp_prompt(servers: &[OpenAgentPluginMcpServer]) -> Option<String> {
    if servers.is_empty() {
        return None;
    }

    let server_list = servers
        .iter()
        .map(|server| {
            let command = server
                .command
                .as_ref()
                .map(|command| format!(" command={command}"))
                .unwrap_or_default();
            format!(
                "- {}:{} as `{}{}` from {}{}",
                server.plugin_name,
                server.server_name,
                server.tool_prefix,
                "<tool>",
                server.source,
                command
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "\n## Plugin MCP Servers\n\
         The service discovered these Open Plugin MCP server configs and expanded plugin placeholders. \
         MCP subprocess adapters are not attached in this runtime yet, so treat them as planned external capabilities unless matching tools are present.\n\n\
         {server_list}"
    ))
}

fn model_overlay(model_id: Option<&str>) -> &'static str {
    match detect_model_family(model_id) {
        ModelFamily::Claude => CLAUDE_OVERLAY,
        ModelFamily::Gpt => GPT_OVERLAY,
        ModelFamily::Gemini => GEMINI_OVERLAY,
        ModelFamily::Other => OTHER_OVERLAY,
    }
}

fn detect_model_family(model_id: Option<&str>) -> ModelFamily {
    let Some(model_id) = model_id else {
        return ModelFamily::Other;
    };
    let id = model_id.to_ascii_lowercase();
    if id.contains("claude") {
        ModelFamily::Claude
    } else if id.contains("gpt-") || id.contains("o1") || id.contains("o3") || id.contains("o4") {
        ModelFamily::Gpt
    } else if id.contains("gemini") {
        ModelFamily::Gemini
    } else {
        ModelFamily::Other
    }
}

enum ModelFamily {
    Claude,
    Gpt,
    Gemini,
    Other,
}

fn is_false(value: &bool) -> bool {
    !*value
}

const CORE_SYSTEM_PROMPT: &str = r#"You are Open Agent -- an AI coding assistant that completes complex, multi-step tasks through planning, context management, and delegation.

# Role & Agency

Complete tasks end-to-end. Prefer acting when the request implies work, ask only when genuinely blocked, and keep changes scoped to the user's task.

# Guardrails

- Prefer minimal local fixes over broad architecture changes
- Search for existing patterns before adding new ones
- Avoid surprise edits across unrelated subsystems
- Do not add new dependencies unless explicitly approved

# Tool Usage

Use file and shell operations carefully. Read files before editing them, use project verification commands, and keep sandbox changes in the workspace.

# Verification Loop

After code changes, run the relevant checks, fix failures caused by the change, and report what passed or what remains blocked."#;

const CLAUDE_OVERLAY: &str = r#"
# Task Management

Use task tracking for multi-step work and keep it updated as each item completes."#;

const GPT_OVERLAY: &str = r#"
# Autonomous Completion

Keep working until the request is completely handled and verified."#;

const GEMINI_OVERLAY: &str = r#"
# Conciseness

Keep routine status output brief while still completing the full task."#;

const OTHER_OVERLAY: &str = r#"
# Completion

Stay concise, follow existing code conventions, and continue until the work is complete."#;

const CLOUD_SANDBOX_INSTRUCTIONS: &str = r#"
# Cloud Sandbox

The sandbox is ephemeral. Persisted commit, PR, and merge operations are handled by the broker outside the sandbox.

## Git Write Rules

- Do not configure credentials or call remote write APIs from sandbox tools
- Make filesystem changes only unless the outer runtime explicitly permits git operations

## On Task Completion

Leave working tree changes in place and report verification."#;

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::{
        DEFAULT_OPEN_AGENT_MODEL_LABEL, OpenAgent, OpenAgentCallOptions, OpenAgentError,
        OpenAgentModelVariant, OpenAgentPluginMcpServer, OpenAgentSettings, OpenAgentSkillMetadata,
        OpenAgentSkillOptions, resolve_chat_model_selection,
    };
    use ai_sdk_rust::{
        FinishReason, GatewayProvider, GenerateTextTool, InputTokenUsage, Instructions, JsonSchema,
        LanguageModel, LanguageModelContent, LanguageModelFinishReason,
        LanguageModelGenerateResult, LanguageModelText, LanguageModelUsage, MockLanguageModel,
        OutputTokenUsage, Tool, ToolLoopAgentModelSettings,
    };
    use open_agents_core::AgentModelSelection;
    use open_agents_sandbox::SandboxContext;

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn object_schema() -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    fn sandbox_context() -> SandboxContext {
        SandboxContext::new(
            json!({
                "type": "vercel",
                "sandboxName": "session-123"
            }),
            "/workspace/repo",
        )
        .with_current_branch("codex/open-agents-02-agent-core-runtime")
        .with_environment_details("Linux sandbox with Rust toolchain")
    }

    fn review_skill() -> OpenAgentSkillMetadata {
        OpenAgentSkillMetadata::new(
            "review",
            "Review code for correctness",
            ".codex/skills/review",
            "SKILL.md",
        )
        .with_options(OpenAgentSkillOptions::new().with_user_invocable(false))
    }

    fn text_result_with_usage(
        text: &str,
        usage: LanguageModelUsage,
    ) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            usage,
        )
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(11),
                cache_read: Some(3),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(7),
                text: Some(6),
                ..OutputTokenUsage::default()
            },
            raw: None,
        }
    }

    fn instructions_text(instructions: &Instructions) -> &str {
        match instructions {
            Instructions::Text(text) => text,
            Instructions::Message(message) => &message.content,
            Instructions::Messages(messages) => &messages[0].content,
        }
    }

    fn live_gateway_api_key() -> Option<String> {
        env::var("AI_SDK_RUST_AI_GATEWAY_API_KEY")
            .or_else(|_| env::var("AI_GATEWAY_API_KEY"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(load_gateway_api_key_from_dotenv)
    }

    fn load_gateway_api_key_from_dotenv() -> Option<String> {
        let contents = fs::read_to_string(".env.local").ok()?;
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if matches!(
                name.trim(),
                "AI_SDK_RUST_AI_GATEWAY_API_KEY" | "AI_GATEWAY_API_KEY"
            ) {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }

    #[test]
    fn open_agent_prepare_rejects_missing_sandbox() {
        let model = MockLanguageModel::new();
        let agent = OpenAgent::for_model(&model);

        let error = match agent.prepare_call(OpenAgentCallOptions::without_sandbox("hello")) {
            Ok(_) => panic!("missing sandbox should fail"),
            Err(error) => error,
        };

        assert_eq!(error, OpenAgentError::MissingSandbox);
    }

    #[test]
    fn open_agent_prepare_composes_prompt_context_model_and_tools() {
        let default_model = MockLanguageModel::new().with_model_id(DEFAULT_OPEN_AGENT_MODEL_LABEL);
        let call_model = MockLanguageModel::new().with_model_id("openai/gpt-5.4");
        let agent = OpenAgent::new(
            OpenAgentSettings::new(&default_model)
                .with_registered_model("openai/gpt-5.4", &call_model)
                .with_tool(Tool::new("read", object_schema())),
        );
        let options = OpenAgentCallOptions::from_prompt("fix the bug", sandbox_context())
            .with_model(
                AgentModelSelection::new("openai/gpt-5.4").with_provider_options(json!({
                    "openai": {
                        "store": true,
                        "textVerbosity": "high"
                    },
                    "gateway": {
                        "order": ["openai"]
                    }
                })),
            )
            .with_subagent_model(AgentModelSelection::new(DEFAULT_OPEN_AGENT_MODEL_LABEL))
            .with_custom_instructions("Always run the targeted tests.")
            .with_skill(review_skill())
            .with_plugin_mcp_servers([OpenAgentPluginMcpServer::new(
                "hello-plugin",
                "echo",
                "./.mcp.json",
            )
            .with_command("/plugins/hello/bin/echo-mcp")
            .with_has_args(true)]);

        let prepared = agent.prepare_call(options).expect("prepare succeeds");

        assert_eq!(prepared.model_id, "openai/gpt-5.4");
        assert_eq!(
            prepared.subagent_model_id.as_deref(),
            Some(DEFAULT_OPEN_AGENT_MODEL_LABEL)
        );
        assert_eq!(prepared.tool_loop.model.model_id(), "openai/gpt-5.4");
        assert!(
            prepared
                .tool_loop
                .tools
                .iter()
                .any(|tool| matches!(tool, GenerateTextTool::Rust(tool) if tool.name == "read"))
        );

        let instructions = prepared
            .tool_loop
            .instructions
            .as_ref()
            .map(instructions_text)
            .expect("instructions are set");
        assert!(instructions.contains("You are Open Agent"));
        assert!(instructions.contains("Working directory: /workspace/repo"));
        assert!(instructions.contains("Current branch: codex/open-agents-02-agent-core-runtime"));
        assert!(instructions.contains("Always run the targeted tests."));
        assert!(instructions.contains("- review: Review code for correctness (model-only)"));
        assert!(instructions.contains("hello-plugin:echo"));
        assert!(instructions.contains("mcp__plugin_hello-plugin_echo__<tool>"));

        let context = prepared
            .tool_loop
            .runtime_context
            .get("openAgent")
            .expect("open agent context exists");
        assert_eq!(
            context
                .pointer("/sandbox/workingDirectory")
                .and_then(serde_json::Value::as_str),
            Some("/workspace/repo")
        );
        assert_eq!(
            context
                .pointer("/sandbox/state/type")
                .and_then(serde_json::Value::as_str),
            Some("vercel")
        );
        assert_eq!(
            context
                .pointer("/skills/0/name")
                .and_then(serde_json::Value::as_str),
            Some("review")
        );
        assert_eq!(
            context
                .pointer("/pluginMcpServers/0/serverName")
                .and_then(serde_json::Value::as_str),
            Some("echo")
        );
        assert_eq!(prepared.plugin_mcp_servers.len(), 1);

        let provider_options = prepared
            .tool_loop
            .model_settings
            .provider_options
            .expect("provider options are set");
        assert_eq!(
            provider_options["openai"].get("store"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            provider_options["openai"].get("textVerbosity"),
            Some(&json!("high"))
        );
        assert_eq!(
            provider_options["openai"].get("reasoningSummary"),
            Some(&json!("detailed"))
        );
        assert_eq!(
            provider_options["gateway"].get("order"),
            Some(&json!(["openai"]))
        );
    }

    #[test]
    fn chat_model_selection_resolves_direct_variant_builtin_missing_and_default_cases() {
        let direct =
            resolve_chat_model_selection(Some("openai/gpt-5"), &[], "Selected model variant");
        assert_eq!(direct.selection, AgentModelSelection::new("openai/gpt-5"));
        assert_eq!(direct.warning, None);

        let variants = [
            OpenAgentModelVariant::new("variant:openai-medium", "openai/gpt-5")
                .with_provider_options(json!({
                    "reasoningEffort": "medium"
                })),
        ];
        let variant = resolve_chat_model_selection(
            Some("variant:openai-medium"),
            &variants,
            "Selected model variant",
        );
        assert_eq!(variant.selection.id, "openai/gpt-5");
        assert_eq!(
            variant.selection.provider_options,
            Some(json!({
                "openai": {
                    "reasoningEffort": "medium",
                    "store": false
                }
            }))
        );

        let builtin = resolve_chat_model_selection(
            Some("variant:builtin:gpt-5.4-xhigh"),
            &[],
            "Selected model variant",
        );
        assert_eq!(builtin.selection.id, "openai/gpt-5.4");
        assert_eq!(
            builtin.selection.provider_options,
            Some(json!({
                "openai": {
                    "reasoningEffort": "xhigh",
                    "reasoningSummary": "auto",
                    "store": false
                }
            }))
        );

        let missing =
            resolve_chat_model_selection(Some("variant:missing"), &[], "Selected model variant");
        assert_eq!(
            missing.selection,
            AgentModelSelection::new(DEFAULT_OPEN_AGENT_MODEL_LABEL)
        );
        assert_eq!(
            missing.warning.as_deref(),
            Some(
                "Selected model variant \"variant:missing\" was not found. Falling back to default model."
            )
        );

        let default = resolve_chat_model_selection(None, &[], "Selected model variant");
        assert_eq!(
            default.selection,
            AgentModelSelection::new(DEFAULT_OPEN_AGENT_MODEL_LABEL)
        );
        assert_eq!(default.warning, None);
    }

    #[test]
    fn open_agent_generate_records_usage_from_fake_model() {
        let usage = usage();
        let model = MockLanguageModel::new()
            .with_model_id(DEFAULT_OPEN_AGENT_MODEL_LABEL)
            .with_generate_result(text_result_with_usage("done", usage.clone()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_hook = Arc::clone(&events);
        let agent = OpenAgent::new(
            OpenAgentSettings::new(&model).with_usage_hook(move |event| {
                events_for_hook
                    .lock()
                    .expect("events mutex is not poisoned")
                    .push(event);
                ready(())
            }),
        );

        let result = poll_ready(agent.generate(OpenAgentCallOptions::from_prompt(
            "finish",
            sandbox_context(),
        )))
        .expect("generation succeeds");

        assert_eq!(result.text, "done");
        let events = events.lock().expect("events mutex is not poisoned");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model_id, DEFAULT_OPEN_AGENT_MODEL_LABEL);
        assert_eq!(events[0].usage, usage);
        assert_eq!(events[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live Open Agents model call"]
    fn live_open_agent_gateway_generate_text_smoke() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Open Agents Gateway smoke because no API key is configured");
            return;
        };
        let model_id = env::var("OPEN_AGENTS_LIVE_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id.clone());
        let agent = OpenAgent::new(
            OpenAgentSettings::new(&model)
                .with_model_id(model_id)
                .with_model_settings(
                    ToolLoopAgentModelSettings::new()
                        .with_max_output_tokens(32)
                        .with_temperature(0.0),
                ),
        );

        let result = poll_ready(agent.generate(OpenAgentCallOptions::from_prompt(
            "Reply with exactly: open-agents-gateway-ok",
            sandbox_context(),
        )))
        .expect("live Open Agents Gateway generation succeeds");

        assert!(
            result
                .text
                .to_lowercase()
                .contains("open-agents-gateway-ok"),
            "gateway response did not contain expected marker: {:?}",
            result.text
        );
    }
}
