use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::{Future, ready};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::{ToolLoopAgent, ToolLoopAgentCallOptions, ToolLoopAgentSettings};
use crate::generate_text::GenerateTextTool;
use crate::json::{JsonSchema, JsonValue};
use crate::language_model::{
    InputTokenUsage, LanguageModel, LanguageModelAbortController, LanguageModelAbortSignal,
    LanguageModelAssistantContentPart, LanguageModelAssistantMessage, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelTextPart, LanguageModelTool, LanguageModelToolResultOutput,
    LanguageModelUsage, OutputTokenUsage,
};
use crate::provider_utils::{
    ExecuteToolOutput, ExperimentalSandbox, Tool, ToolExecutionError, ToolExecutionOptions,
    ToolModelOutputOptions,
};

/// Maximum number of model/tool-loop steps used by Open Agents subagents.
pub const SUBAGENT_STEP_LIMIT: usize = 100;

/// Name of the Open Agents task delegation tool.
pub const TASK_TOOL_NAME: &str = "task";

const EXPLORER_SYSTEM_PROMPT: &str = "You are an explorer agent - a fast, read-only subagent specialized for exploring codebases. Work autonomously, do not ask follow-up questions, do not modify files, and return a concise Summary and Answer.";
const EXECUTOR_SYSTEM_PROMPT: &str = "You are an executor agent - a fire-and-forget subagent that completes specific, well-defined implementation tasks autonomously. Work through the task, validate changes where possible, and return a concise Summary and Answer.";
const DESIGN_SYSTEM_PROMPT: &str = "You are a design agent - a specialized subagent for distinctive, production-grade frontend implementation. Make deliberate visual choices, complete the requested implementation, validate where possible, and return a concise Summary and Answer.";

const EXPLORER_TOOLS: &[&str] = &["read", "grep", "glob", "bash"];
const WRITE_TOOLS: &[&str] = &["read", "write", "edit", "grep", "glob", "bash"];

/// Open Agents subagent type names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SubagentType {
    Explorer,
    Executor,
    Design,
}

impl SubagentType {
    /// Returns the upstream string identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::Executor => "executor",
            Self::Design => "design",
        }
    }

    /// Returns all known Open Agents subagent types.
    pub const fn all() -> &'static [Self] {
        &[Self::Explorer, Self::Executor, Self::Design]
    }
}

impl fmt::Display for SubagentType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SubagentType {
    type Err = SubagentTaskError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "explorer" => Ok(Self::Explorer),
            "executor" => Ok(Self::Executor),
            "design" => Ok(Self::Design),
            other => Err(SubagentTaskError::unknown_subagent(other)),
        }
    }
}

/// Static registry entry for an Open Agents subagent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubagentProfile {
    pub subagent_type: SubagentType,
    pub short_description: &'static str,
    pub system_prompt: &'static str,
    pub default_model_id: &'static str,
    pub max_steps: usize,
    pub allowed_tool_names: &'static [&'static str],
    pub can_modify_files: bool,
}

impl SubagentProfile {
    /// Returns whether this profile can use the supplied tool name.
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        self.allowed_tool_names.contains(&tool_name)
    }

    /// Filters inherited tools to the bounded tool set for this subagent.
    pub fn filter_tools(&self, tools: &[GenerateTextTool]) -> Vec<GenerateTextTool> {
        let allowed: BTreeSet<&str> = self.allowed_tool_names.iter().copied().collect();
        tools
            .iter()
            .filter(|tool| {
                generate_text_tool_name(tool).is_some_and(|tool_name| allowed.contains(tool_name))
            })
            .cloned()
            .collect()
    }
}

/// Registry for Open Agents subagent profiles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubagentRegistry {
    profiles: BTreeMap<SubagentType, SubagentProfile>,
}

impl SubagentRegistry {
    /// Creates a registry from explicit profiles.
    pub fn new(profiles: impl IntoIterator<Item = SubagentProfile>) -> Self {
        Self {
            profiles: profiles
                .into_iter()
                .map(|profile| (profile.subagent_type, profile))
                .collect(),
        }
    }

    /// Creates the default Open Agents registry.
    pub fn open_agents() -> Self {
        Self::new(default_subagent_profiles())
    }

    /// Looks up a profile by type.
    pub fn get(&self, subagent_type: SubagentType) -> Option<&SubagentProfile> {
        self.profiles.get(&subagent_type)
    }

    /// Resolves a profile or returns an upstream-aligned unknown-subagent error.
    pub fn require(
        &self,
        subagent_type: SubagentType,
    ) -> Result<&SubagentProfile, SubagentTaskError> {
        self.get(subagent_type)
            .ok_or_else(|| SubagentTaskError::unknown_subagent(subagent_type.as_str()))
    }

    /// Returns registered types in stable order.
    pub fn types(&self) -> Vec<SubagentType> {
        self.profiles.keys().copied().collect()
    }

    /// Formats the task-tool subagent summary lines used in descriptions.
    pub fn summary_lines(&self) -> String {
        self.types()
            .into_iter()
            .filter_map(|subagent_type| self.get(subagent_type))
            .map(|profile| {
                format!(
                    "- `{}` - {}",
                    profile.subagent_type, profile.short_description
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for SubagentRegistry {
    fn default() -> Self {
        Self::open_agents()
    }
}

/// Returns the default Open Agents profiles as independent values.
pub fn default_subagent_profiles() -> Vec<SubagentProfile> {
    vec![
        SubagentProfile {
            subagent_type: SubagentType::Explorer,
            short_description: "Use for read-only codebase exploration, tracing behavior, and answering questions without changing files",
            system_prompt: EXPLORER_SYSTEM_PROMPT,
            default_model_id: "anthropic/claude-haiku-4.5",
            max_steps: SUBAGENT_STEP_LIMIT,
            allowed_tool_names: EXPLORER_TOOLS,
            can_modify_files: false,
        },
        SubagentProfile {
            subagent_type: SubagentType::Executor,
            short_description: "Use for well-scoped implementation work, including edits, scaffolding, refactors, and other file changes",
            system_prompt: EXECUTOR_SYSTEM_PROMPT,
            default_model_id: "anthropic/claude-haiku-4.5",
            max_steps: SUBAGENT_STEP_LIMIT,
            allowed_tool_names: WRITE_TOOLS,
            can_modify_files: true,
        },
        SubagentProfile {
            subagent_type: SubagentType::Design,
            short_description: "Use for creating distinctive, production-grade frontend interfaces with high design quality",
            system_prompt: DESIGN_SYSTEM_PROMPT,
            default_model_id: "anthropic/claude-opus-4.6",
            max_steps: SUBAGENT_STEP_LIMIT,
            allowed_tool_names: WRITE_TOOLS,
            can_modify_files: true,
        },
    ]
}

/// Lightweight skill metadata inherited by subagents.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSkillContext {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl SubagentSkillContext {
    /// Creates inherited skill metadata.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }

    /// Sets a short skill description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Context cloned from a parent agent into nested task subagents.
#[derive(Clone)]
pub struct SubagentInheritedContext<M: LanguageModel + ?Sized> {
    pub model: Arc<M>,
    pub subagent_model: Option<Arc<M>>,
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
    pub tools: Vec<GenerateTextTool>,
    pub skills: Vec<SubagentSkillContext>,
    pub custom_instructions: Option<String>,
}

impl<M: LanguageModel + ?Sized> SubagentInheritedContext<M> {
    /// Creates inherited context with a parent model.
    pub fn new(model: Arc<M>) -> Self {
        Self {
            model,
            subagent_model: None,
            experimental_sandbox: None,
            tools: Vec::new(),
            skills: Vec::new(),
            custom_instructions: None,
        }
    }

    /// Sets the model used by subagents, falling back to the parent model when omitted.
    pub fn with_subagent_model(mut self, subagent_model: Arc<M>) -> Self {
        self.subagent_model = Some(subagent_model);
        self
    }

    /// Sets the sandbox inherited by subagents.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }

    /// Adds one inherited tool.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        self.tools.push(tool.into());
        self
    }

    /// Replaces inherited tools.
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = GenerateTextTool>) -> Self {
        self.tools = tools.into_iter().collect();
        self
    }

    /// Adds one inherited skill.
    pub fn with_skill(mut self, skill: SubagentSkillContext) -> Self {
        self.skills.push(skill);
        self
    }

    /// Sets inherited custom instructions.
    pub fn with_custom_instructions(mut self, custom_instructions: impl Into<String>) -> Self {
        self.custom_instructions = Some(custom_instructions.into());
        self
    }

    /// Returns the model selected for subagent execution.
    pub fn subagent_model(&self) -> Arc<M> {
        self.subagent_model
            .as_ref()
            .map_or_else(|| Arc::clone(&self.model), Arc::clone)
    }

    /// Renders profile, task, inherited skills, and caller instructions.
    pub fn render_instructions(
        &self,
        profile: &SubagentProfile,
        task: &str,
        instructions: &str,
    ) -> String {
        let mut rendered = String::new();
        rendered.push_str(profile.system_prompt);
        rendered.push_str("\n\nWorking directory: . (workspace root)\nUse workspace-relative paths for all file operations.");

        if let Some(custom_instructions) = &self.custom_instructions {
            rendered.push_str("\n\n## Parent Instructions\n");
            rendered.push_str(custom_instructions);
        }

        if !self.skills.is_empty() {
            rendered.push_str("\n\n## Available Skills\n");
            for skill in &self.skills {
                rendered.push_str("- ");
                rendered.push_str(&skill.name);
                if let Some(description) = &skill.description {
                    rendered.push_str(": ");
                    rendered.push_str(description);
                }
                rendered.push('\n');
            }
        }

        rendered.push_str("\n\n## Your Task\n");
        rendered.push_str(task);
        rendered.push_str("\n\n## Detailed Instructions\n");
        rendered.push_str(instructions);
        rendered.push_str("\n\n## Reminder\n- You cannot ask questions.\n- Complete the task before returning.\n- Your final message must include a Summary and Answer.");
        rendered
    }
}

/// Error returned by subagent task setup or execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubagentTaskError {
    UnknownSubagent {
        requested: String,
    },
    RecursionLimit {
        depth: usize,
        max_depth: usize,
    },
    FanOutLimit {
        active_tasks: usize,
        max_concurrent_tasks: usize,
    },
    InvalidInput {
        message: String,
    },
    Execution {
        message: String,
    },
}

impl SubagentTaskError {
    fn unknown_subagent(requested: impl Into<String>) -> Self {
        Self::UnknownSubagent {
            requested: requested.into(),
        }
    }

    fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }

    fn execution(message: impl Into<String>) -> Self {
        Self::Execution {
            message: message.into(),
        }
    }
}

impl fmt::Display for SubagentTaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSubagent { requested } => {
                write!(formatter, "Unknown subagent type `{requested}`.")
            }
            Self::RecursionLimit { depth, max_depth } => write!(
                formatter,
                "Subagent recursion depth {depth} reached the configured limit of {max_depth}."
            ),
            Self::FanOutLimit {
                active_tasks,
                max_concurrent_tasks,
            } => write!(
                formatter,
                "Subagent task fan-out limit reached ({active_tasks}/{max_concurrent_tasks} active tasks)."
            ),
            Self::InvalidInput { message } | Self::Execution { message } => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for SubagentTaskError {}

impl From<SubagentTaskError> for ToolExecutionError {
    fn from(error: SubagentTaskError) -> Self {
        Self::new(error.to_string())
    }
}

/// Shared recursion and fan-out state for task tool executions.
#[derive(Debug)]
pub struct SubagentTaskState {
    max_depth: usize,
    max_concurrent_tasks: usize,
    active_tasks: AtomicUsize,
}

impl SubagentTaskState {
    /// Creates state with explicit recursion and fan-out limits.
    pub fn new(max_depth: usize, max_concurrent_tasks: usize) -> Self {
        Self {
            max_depth,
            max_concurrent_tasks: max_concurrent_tasks.max(1),
            active_tasks: AtomicUsize::new(0),
        }
    }

    /// Returns active task count.
    pub fn active_tasks(&self) -> usize {
        self.active_tasks.load(Ordering::SeqCst)
    }

    /// Returns the configured maximum subagent nesting depth.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Returns the configured maximum concurrent task fan-out.
    pub fn max_concurrent_tasks(&self) -> usize {
        self.max_concurrent_tasks
    }

    /// Attempts to enter one subagent task at the supplied current depth.
    pub fn try_enter(
        self: &Arc<Self>,
        depth: usize,
    ) -> Result<SubagentTaskPermit, SubagentTaskError> {
        if depth >= self.max_depth {
            return Err(SubagentTaskError::RecursionLimit {
                depth,
                max_depth: self.max_depth,
            });
        }

        let mut active = self.active_tasks.load(Ordering::SeqCst);
        loop {
            if active >= self.max_concurrent_tasks {
                return Err(SubagentTaskError::FanOutLimit {
                    active_tasks: active,
                    max_concurrent_tasks: self.max_concurrent_tasks,
                });
            }

            match self.active_tasks.compare_exchange(
                active,
                active + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Ok(SubagentTaskPermit {
                        state: Arc::clone(self),
                    });
                }
                Err(observed) => active = observed,
            }
        }
    }
}

impl Default for SubagentTaskState {
    fn default() -> Self {
        Self::new(1, 4)
    }
}

/// Permit that releases one active subagent slot when dropped.
#[derive(Debug)]
pub struct SubagentTaskPermit {
    state: Arc<SubagentTaskState>,
}

impl Drop for SubagentTaskPermit {
    fn drop(&mut self) {
        self.state.active_tasks.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Propagates parent cancellation into a new child subagent abort signal.
pub fn inherited_subagent_abort_signal(
    parent: Option<&LanguageModelAbortSignal>,
) -> Option<LanguageModelAbortSignal> {
    let parent = parent?;
    let controller = LanguageModelAbortController::new();
    let child = controller.signal();
    parent.aborts_signal(&child);
    Some(child)
}

/// Input accepted by the Open Agents task tool.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskToolInput {
    #[serde(alias = "subagent_type")]
    pub subagent_type: SubagentType,
    pub task: String,
    pub instructions: String,
}

/// Pending tool call metadata emitted by the task tool.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPendingToolCall {
    pub name: String,
    pub input: JsonValue,
}

/// Output emitted by the Open Agents task tool.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskToolOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<TaskPendingToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, rename = "final", skip_serializing_if = "Option::is_none")]
    pub final_messages: Option<Vec<LanguageModelMessage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<LanguageModelUsage>,
}

impl TaskToolOutput {
    /// Creates an initial task-tool output for UI elapsed-time rendering.
    pub fn initial(started_at: u64, model_id: Option<String>) -> Self {
        Self {
            tool_call_count: Some(0),
            started_at: Some(started_at),
            model_id,
            ..Self::default()
        }
    }

    /// Converts final task output to the text shown to the parent model.
    pub fn to_model_text(&self) -> String {
        let Some(messages) = &self.final_messages else {
            return "Task completed.".to_string();
        };

        messages
            .iter()
            .rev()
            .find_map(|message| match message {
                LanguageModelMessage::Assistant(message) => {
                    message.content.iter().rev().find_map(|part| match part {
                        LanguageModelAssistantContentPart::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "Task completed.".to_string())
    }
}

/// Options passed to a task runner from a task-tool execution.
#[derive(Clone)]
pub struct SubagentTaskToolRunOptions {
    pub tool_call_id: String,
    pub messages: LanguageModelPrompt,
    pub context: Option<JsonValue>,
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
    pub abort_signal: Option<LanguageModelAbortSignal>,
    pub started_at: u64,
    pub depth: usize,
}

impl SubagentTaskToolRunOptions {
    /// Creates task runner options from provider-utils tool execution options.
    pub fn from_tool_execution(options: ToolExecutionOptions) -> Self {
        let depth = options
            .context
            .as_ref()
            .and_then(|context| context.get("subagentDepth"))
            .and_then(JsonValue::as_u64)
            .unwrap_or(0) as usize;

        Self {
            tool_call_id: options.tool_call_id,
            messages: options.messages,
            context: options.context,
            experimental_sandbox: options.experimental_sandbox,
            abort_signal: options.abort_signal,
            started_at: now_ms(),
            depth,
        }
    }
}

/// Result returned by a task runner.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskToolRunResult {
    pub updates: Vec<TaskToolOutput>,
    pub final_output: TaskToolOutput,
}

impl TaskToolRunResult {
    /// Creates a result with only a final output.
    pub fn final_output(final_output: TaskToolOutput) -> Self {
        Self {
            updates: Vec::new(),
            final_output,
        }
    }
}

/// Future returned by task runners.
pub type TaskToolRunFuture =
    Pin<Box<dyn Future<Output = Result<TaskToolRunResult, SubagentTaskError>> + Send>>;

/// Runtime boundary used by the task tool to launch a nested subagent.
pub trait TaskToolRunner: Send + Sync {
    fn run(&self, input: TaskToolInput, options: SubagentTaskToolRunOptions) -> TaskToolRunFuture;
}

/// Creates the Open Agents task tool with a supplied task runner.
pub fn task_tool(runner: Arc<dyn TaskToolRunner>) -> Tool {
    let registry = SubagentRegistry::open_agents();
    let description = format!(
        "Launch a specialized subagent to handle complex tasks autonomously.\n\nAVAILABLE SUBAGENTS:\n{}\n\nSubagents work autonomously, inherit sandbox/model context from the parent, and return only a concise summary.",
        registry.summary_lines()
    );

    Tool::new(TASK_TOOL_NAME, task_input_schema())
        .with_description(description)
        .with_output_schema(task_output_schema())
        .with_execute_outputs(move |input, options| {
            let runner = Arc::clone(&runner);
            async move {
                let input = parse_task_tool_input(input)?;
                let run_options = SubagentTaskToolRunOptions::from_tool_execution(options);
                let initial = TaskToolOutput::initial(run_options.started_at, None);
                let result = runner.run(input, run_options).await?;

                let mut outputs = Vec::with_capacity(result.updates.len() + 2);
                outputs.push(ExecuteToolOutput::preliminary(task_output_json(&initial)?));
                for update in result.updates {
                    outputs.push(ExecuteToolOutput::preliminary(task_output_json(&update)?));
                }
                outputs.push(ExecuteToolOutput::final_output(task_output_json(
                    &result.final_output,
                )?));
                Ok(outputs)
            }
        })
        .with_to_model_output(|options| async move { task_tool_to_model_output(options) })
}

/// Task runner that launches nested [`ToolLoopAgent`] calls.
pub struct ToolLoopSubagentRunner<M: LanguageModel + Send + Sync + 'static> {
    registry: SubagentRegistry,
    context: SubagentInheritedContext<M>,
    task_state: Arc<SubagentTaskState>,
}

impl<M: LanguageModel + Send + Sync + 'static> ToolLoopSubagentRunner<M> {
    /// Creates a ToolLoop-backed subagent runner.
    pub fn new(context: SubagentInheritedContext<M>) -> Self {
        Self {
            registry: SubagentRegistry::open_agents(),
            context,
            task_state: Arc::new(SubagentTaskState::default()),
        }
    }

    /// Replaces the subagent registry.
    pub fn with_registry(mut self, registry: SubagentRegistry) -> Self {
        self.registry = registry;
        self
    }

    /// Replaces the shared task guard state.
    pub fn with_task_state(mut self, task_state: Arc<SubagentTaskState>) -> Self {
        self.task_state = task_state;
        self
    }

    fn run_blocking(
        &self,
        input: TaskToolInput,
        options: SubagentTaskToolRunOptions,
    ) -> Result<TaskToolRunResult, SubagentTaskError> {
        let profile = self.registry.require(input.subagent_type)?;
        let _permit = self.task_state.try_enter(options.depth)?;
        let model = self.context.subagent_model();
        let mut settings = ToolLoopAgentSettings::new(model.as_ref())
            .with_instructions(self.context.render_instructions(
                profile,
                &input.task,
                &input.instructions,
            ))
            .with_max_steps(profile.max_steps);

        let sandbox = options
            .experimental_sandbox
            .clone()
            .or_else(|| self.context.experimental_sandbox.clone());
        if let Some(sandbox) = sandbox {
            settings = settings.with_experimental_sandbox(sandbox);
        }

        for tool in profile.filter_tools(&self.context.tools) {
            settings = settings.with_tool(tool);
        }

        let agent = ToolLoopAgent::new(settings);
        let mut call_options: ToolLoopAgentCallOptions<'_, M> =
            ToolLoopAgentCallOptions::from_prompt(
                "Complete this task and provide a summary of what you accomplished.",
            );
        if let Some(abort_signal) = inherited_subagent_abort_signal(options.abort_signal.as_ref()) {
            call_options = call_options.with_abort_signal(abort_signal);
        }

        let result = futures_executor::block_on(agent.generate(call_options))
            .map_err(|error| SubagentTaskError::execution(error.to_string()))?;
        let final_messages = if result.response_messages.is_empty() {
            vec![assistant_text_message(result.text.clone())]
        } else {
            result.response_messages.clone()
        };

        Ok(TaskToolRunResult::final_output(TaskToolOutput {
            pending: None,
            tool_call_count: Some(result.tool_calls.len()),
            started_at: Some(options.started_at),
            model_id: Some(model.model_id().to_string()),
            final_messages: Some(final_messages),
            usage: Some(result.usage.clone()),
        }))
    }
}

impl<M: LanguageModel + Send + Sync + 'static> TaskToolRunner for ToolLoopSubagentRunner<M> {
    fn run(&self, input: TaskToolInput, options: SubagentTaskToolRunOptions) -> TaskToolRunFuture {
        Box::pin(ready(self.run_blocking(input, options)))
    }
}

/// Adds two optional token counts.
pub fn add_token_counts(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0) + right.unwrap_or(0)),
    }
}

/// Adds two language-model usage records.
pub fn add_language_model_usage(
    left: &LanguageModelUsage,
    right: &LanguageModelUsage,
) -> LanguageModelUsage {
    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: add_token_counts(left.input_tokens.total, right.input_tokens.total),
            no_cache: add_token_counts(left.input_tokens.no_cache, right.input_tokens.no_cache),
            cache_read: add_token_counts(
                left.input_tokens.cache_read,
                right.input_tokens.cache_read,
            ),
            cache_write: add_token_counts(
                left.input_tokens.cache_write,
                right.input_tokens.cache_write,
            ),
        },
        output_tokens: OutputTokenUsage {
            total: add_token_counts(left.output_tokens.total, right.output_tokens.total),
            text: add_token_counts(left.output_tokens.text, right.output_tokens.text),
            reasoning: add_token_counts(
                left.output_tokens.reasoning,
                right.output_tokens.reasoning,
            ),
        },
        raw: None,
    }
}

/// Adds optional usage records, preserving whichever side is present.
pub fn sum_language_model_usage(
    left: Option<LanguageModelUsage>,
    right: Option<LanguageModelUsage>,
) -> Option<LanguageModelUsage> {
    match (left, right) {
        (Some(left), Some(right)) => Some(add_language_model_usage(&left, &right)),
        (Some(usage), None) | (None, Some(usage)) => Some(usage),
        (None, None) => None,
    }
}

fn generate_text_tool_name(tool: &GenerateTextTool) -> Option<&str> {
    match tool {
        GenerateTextTool::Rust(tool) => Some(tool.name.as_str()),
        GenerateTextTool::LanguageModel(LanguageModelTool::Function(tool)) => {
            Some(tool.name.as_str())
        }
        GenerateTextTool::LanguageModel(LanguageModelTool::Provider(tool)) => {
            Some(tool.name.as_str())
        }
    }
}

fn task_tool_to_model_output(options: ToolModelOutputOptions) -> LanguageModelToolResultOutput {
    let output = serde_json::from_value::<TaskToolOutput>(options.output).ok();
    LanguageModelToolResultOutput::text(
        output
            .as_ref()
            .map(TaskToolOutput::to_model_text)
            .unwrap_or_else(|| "Task completed.".to_string()),
    )
}

fn parse_task_tool_input(input: JsonValue) -> Result<TaskToolInput, ToolExecutionError> {
    serde_json::from_value::<TaskToolInput>(input)
        .map_err(|error| SubagentTaskError::invalid_input(error.to_string()).into())
}

fn task_output_json(output: &TaskToolOutput) -> Result<JsonValue, ToolExecutionError> {
    serde_json::to_value(output)
        .map_err(|error| SubagentTaskError::execution(error.to_string()).into())
}

fn task_input_schema() -> JsonSchema {
    json!({
        "type": "object",
        "properties": {
            "subagentType": {
                "type": "string",
                "enum": SubagentType::all().iter().map(|kind| kind.as_str()).collect::<Vec<_>>(),
                "description": "Subagent to launch."
            },
            "task": {
                "type": "string",
                "description": "Short description of the task."
            },
            "instructions": {
                "type": "string",
                "description": "Detailed instructions for the subagent."
            }
        },
        "required": ["subagentType", "task", "instructions"],
        "additionalProperties": false
    })
    .as_object()
    .expect("task input schema is an object")
    .clone()
}

fn task_output_schema() -> JsonSchema {
    json!({
        "type": "object",
        "properties": {
            "pending": { "type": "object" },
            "toolCallCount": { "type": "integer", "minimum": 0 },
            "startedAt": { "type": "integer", "minimum": 0 },
            "modelId": { "type": "string" },
            "final": { "type": "array" },
            "usage": { "type": "object" }
        },
        "additionalProperties": true
    })
    .as_object()
    .expect("task output schema is an object")
    .clone()
}

fn assistant_text_message(text: impl Into<String>) -> LanguageModelMessage {
    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(text)),
    ]))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::future::{Ready, ready};
    use std::sync::Mutex;

    use crate::language_model::{
        FinishReason, LanguageModelCallOptions, LanguageModelContent, LanguageModelFinishReason,
        LanguageModelGenerateResult, LanguageModelStreamPart, LanguageModelStreamResult,
        LanguageModelText,
    };
    use crate::provider_utils::{
        SandboxCommandOptions, SandboxCommandResult, SandboxRunCommandFuture, execute_tool,
    };

    #[derive(Debug)]
    struct TestSandbox {
        description: String,
    }

    impl TestSandbox {
        fn new(description: impl Into<String>) -> Self {
            Self {
                description: description.into(),
            }
        }
    }

    impl ExperimentalSandbox for TestSandbox {
        fn description(&self) -> &str {
            &self.description
        }

        fn run_command(&self, options: SandboxCommandOptions) -> SandboxRunCommandFuture {
            Box::pin(ready(
                SandboxCommandResult::new(0).with_stdout(options.command),
            ))
        }
    }

    #[derive(Debug)]
    struct SendMockLanguageModel {
        provider: String,
        model_id: String,
        calls: Arc<Mutex<Vec<LanguageModelCallOptions>>>,
        results: Arc<Mutex<VecDeque<LanguageModelGenerateResult>>>,
    }

    impl SendMockLanguageModel {
        fn new(model_id: impl Into<String>) -> Self {
            Self {
                provider: "test-provider".to_string(),
                model_id: model_id.into(),
                calls: Arc::new(Mutex::new(Vec::new())),
                results: Arc::new(Mutex::new(VecDeque::new())),
            }
        }

        fn push_result(&self, result: LanguageModelGenerateResult) {
            self.results
                .lock()
                .expect("result lock is not poisoned")
                .push_back(result);
        }

        fn calls(&self) -> Vec<LanguageModelCallOptions> {
            self.calls
                .lock()
                .expect("call lock is not poisoned")
                .clone()
        }
    }

    impl LanguageModel for SendMockLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<crate::language_model::LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls
                .lock()
                .expect("call lock is not poisoned")
                .push(options);
            ready(
                self.results
                    .lock()
                    .expect("result lock is not poisoned")
                    .pop_front()
                    .expect("mock result is scripted"),
            )
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    #[derive(Clone, Default)]
    struct RecordingRunner {
        calls: Arc<Mutex<Vec<(TaskToolInput, SubagentTaskToolRunOptions)>>>,
    }

    impl RecordingRunner {
        fn calls(&self) -> Vec<(TaskToolInput, SubagentTaskToolRunOptions)> {
            self.calls
                .lock()
                .expect("runner call lock is not poisoned")
                .clone()
        }
    }

    impl TaskToolRunner for RecordingRunner {
        fn run(
            &self,
            input: TaskToolInput,
            options: SubagentTaskToolRunOptions,
        ) -> TaskToolRunFuture {
            self.calls
                .lock()
                .expect("runner call lock is not poisoned")
                .push((input, options.clone()));
            Box::pin(ready(Ok(TaskToolRunResult {
                updates: vec![TaskToolOutput {
                    pending: Some(TaskPendingToolCall {
                        name: "read".to_string(),
                        input: json!({ "path": "src/lib.rs" }),
                    }),
                    tool_call_count: Some(1),
                    started_at: Some(options.started_at),
                    model_id: Some("test-model".to_string()),
                    ..TaskToolOutput::default()
                }],
                final_output: TaskToolOutput {
                    tool_call_count: Some(1),
                    started_at: Some(options.started_at),
                    model_id: Some("test-model".to_string()),
                    final_messages: Some(vec![assistant_text_message("nested summary")]),
                    usage: Some(usage(10, 4)),
                    ..TaskToolOutput::default()
                },
            })))
        }
    }

    fn object_schema() -> JsonSchema {
        json!({ "type": "object", "additionalProperties": true })
            .as_object()
            .expect("schema object")
            .clone()
    }

    fn text_result(text: &str, usage: LanguageModelUsage) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            usage,
        )
    }

    fn usage(input: u64, output: u64) -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(input),
                no_cache: Some(input),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(output),
                text: Some(output),
                ..OutputTokenUsage::default()
            },
            raw: None,
        }
    }

    #[test]
    fn subagent_registry_lists_open_agents_profiles() {
        let registry = SubagentRegistry::open_agents();

        assert_eq!(
            registry.types(),
            vec![
                SubagentType::Explorer,
                SubagentType::Executor,
                SubagentType::Design
            ]
        );
        assert!(registry.summary_lines().contains("`explorer`"));
        assert!(registry.get(SubagentType::Design).is_some());
        assert_eq!(
            SubagentType::from_str("missing").expect_err("unknown type is rejected"),
            SubagentTaskError::UnknownSubagent {
                requested: "missing".to_string()
            }
        );
    }

    #[test]
    fn subagent_context_inherits_model_sandbox_skills_and_instructions() {
        let parent_model = Arc::new(SendMockLanguageModel::new("parent-model"));
        let subagent_model = Arc::new(SendMockLanguageModel::new("subagent-model"));
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("sandbox"));
        let context = SubagentInheritedContext::new(parent_model)
            .with_subagent_model(Arc::clone(&subagent_model))
            .with_experimental_sandbox(Arc::clone(&sandbox))
            .with_skill(SubagentSkillContext::new("rust").with_description("Rust workflow"))
            .with_custom_instructions("Prefer narrow changes.");

        assert_eq!(context.subagent_model().model_id(), "subagent-model");
        assert!(Arc::ptr_eq(
            context
                .experimental_sandbox
                .as_ref()
                .expect("sandbox is inherited"),
            &sandbox
        ));
        let instructions = context.render_instructions(
            SubagentRegistry::open_agents()
                .get(SubagentType::Executor)
                .expect("executor exists"),
            "Implement task",
            "Run focused tests",
        );

        assert!(instructions.contains("Prefer narrow changes."));
        assert!(instructions.contains("rust: Rust workflow"));
        assert!(instructions.contains("Implement task"));
    }

    #[test]
    fn subagent_profiles_restrict_inherited_tools() {
        let registry = SubagentRegistry::open_agents();
        let explorer = registry
            .get(SubagentType::Explorer)
            .expect("explorer profile exists");
        let executor = registry
            .get(SubagentType::Executor)
            .expect("executor profile exists");
        let tools = vec![
            GenerateTextTool::from(Tool::new("read", object_schema())),
            GenerateTextTool::from(Tool::new("write", object_schema())),
            GenerateTextTool::from(Tool::new("task", object_schema())),
            GenerateTextTool::from(Tool::new("grep", object_schema())),
        ];

        let explorer_tools = explorer
            .filter_tools(&tools)
            .into_iter()
            .filter_map(|tool| generate_text_tool_name(&tool).map(str::to_string))
            .collect::<Vec<_>>();
        let executor_tools = executor
            .filter_tools(&tools)
            .into_iter()
            .filter_map(|tool| generate_text_tool_name(&tool).map(str::to_string))
            .collect::<Vec<_>>();

        assert_eq!(explorer_tools, vec!["read", "grep"]);
        assert_eq!(executor_tools, vec!["read", "write", "grep"]);
    }

    #[test]
    fn subagent_usage_aggregation_adds_nested_counts() {
        let total = add_language_model_usage(&usage(10, 4), &usage(7, 3));

        assert_eq!(total.input_tokens.total, Some(17));
        assert_eq!(total.input_tokens.no_cache, Some(17));
        assert_eq!(total.output_tokens.total, Some(7));
        assert_eq!(total.output_tokens.text, Some(7));
    }

    #[test]
    fn subagent_task_state_guards_recursion_and_fanout() {
        let recursive_state = Arc::new(SubagentTaskState::new(1, 2));
        assert_eq!(
            recursive_state
                .try_enter(1)
                .expect_err("grandchild recursion is rejected"),
            SubagentTaskError::RecursionLimit {
                depth: 1,
                max_depth: 1
            }
        );

        let fanout_state = Arc::new(SubagentTaskState::new(2, 1));
        let permit = fanout_state.try_enter(0).expect("first task gets a permit");
        assert_eq!(fanout_state.active_tasks(), 1);
        assert_eq!(
            fanout_state
                .try_enter(0)
                .expect_err("second task is bounded"),
            SubagentTaskError::FanOutLimit {
                active_tasks: 1,
                max_concurrent_tasks: 1
            }
        );
        drop(permit);
        assert_eq!(fanout_state.active_tasks(), 0);
        assert!(fanout_state.try_enter(0).is_ok());
    }

    #[test]
    fn subagent_abort_signal_follows_parent_cancellation() {
        let controller = LanguageModelAbortController::new();
        let parent = controller.signal();
        let child = inherited_subagent_abort_signal(Some(&parent)).expect("child signal");

        assert!(!child.is_aborted());
        controller.abort_with_reason(json!("cancelled"));

        assert!(child.is_aborted());
        assert_eq!(child.reason(), Some(json!("cancelled")));
    }

    #[test]
    fn task_tool_emits_initial_updates_and_final_output() {
        let runner = RecordingRunner::default();
        let runner_calls = runner.clone();
        let tool = task_tool(Arc::new(runner));
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("sandbox"));
        let abort_controller = LanguageModelAbortController::new();
        let outputs = futures_executor::block_on(execute_tool(
            &tool,
            json!({
                "subagentType": "explorer",
                "task": "Inspect lib",
                "instructions": "Read src/lib.rs"
            }),
            ToolExecutionOptions::new("task-call", Vec::new())
                .with_context(json!({ "subagentDepth": 0 }))
                .with_experimental_sandbox(Arc::clone(&sandbox))
                .with_abort_signal(abort_controller.signal()),
        ))
        .expect("task tool executes");

        assert_eq!(outputs.len(), 3);
        assert!(matches!(outputs[0], ExecuteToolOutput::Preliminary { .. }));
        assert!(matches!(outputs[2], ExecuteToolOutput::Final { .. }));

        let calls = runner_calls.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.subagent_type, SubagentType::Explorer);
        assert_eq!(calls[0].1.depth, 0);
        assert!(Arc::ptr_eq(
            calls[0]
                .1
                .experimental_sandbox
                .as_ref()
                .expect("sandbox propagated"),
            &sandbox
        ));

        let final_output = match outputs.last().expect("final output") {
            ExecuteToolOutput::Final { output } => {
                serde_json::from_value::<TaskToolOutput>(output.clone())
                    .expect("task output deserializes")
            }
            ExecuteToolOutput::Preliminary { .. } => panic!("expected final output"),
        };
        assert_eq!(final_output.to_model_text(), "nested summary");
        assert_eq!(final_output.usage, Some(usage(10, 4)));
    }

    #[test]
    fn tool_loop_subagent_runner_launches_nested_agent_with_inherited_context() {
        let model = Arc::new(SendMockLanguageModel::new("parent-model"));
        model.push_result(text_result("nested done", usage(11, 5)));
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("sandbox"));
        let context = SubagentInheritedContext::new(Arc::clone(&model))
            .with_experimental_sandbox(Arc::clone(&sandbox))
            .with_tool(Tool::new("read", object_schema()))
            .with_tool(Tool::new("write", object_schema()))
            .with_tool(Tool::new("task", object_schema()))
            .with_custom_instructions("Keep it small.");
        let runner = ToolLoopSubagentRunner::new(context);

        let result = futures_executor::block_on(runner.run(
            TaskToolInput {
                subagent_type: SubagentType::Explorer,
                task: "Find entrypoint".to_string(),
                instructions: "Inspect src/lib.rs".to_string(),
            },
            SubagentTaskToolRunOptions {
                tool_call_id: "task-call".to_string(),
                messages: Vec::new(),
                context: None,
                experimental_sandbox: None,
                abort_signal: None,
                started_at: 123,
                depth: 0,
            },
        ))
        .expect("runner completes");

        assert_eq!(
            result.final_output.model_id.as_deref(),
            Some("parent-model")
        );
        assert_eq!(result.final_output.started_at, Some(123));
        assert_eq!(result.final_output.usage, Some(usage(11, 5)));
        assert_eq!(result.final_output.to_model_text(), "nested done");

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        let tool_names = calls[0]
            .tools
            .as_ref()
            .expect("tools are sent")
            .iter()
            .map(|tool| match tool {
                LanguageModelTool::Function(tool) => tool.name.clone(),
                LanguageModelTool::Provider(tool) => tool.name.clone(),
            })
            .collect::<Vec<_>>();
        assert_eq!(tool_names, vec!["read"]);
        assert!(
            calls[0].prompt.iter().any(|message| {
                matches!(message, LanguageModelMessage::System(system) if system.content.contains("Keep it small."))
            })
        );
    }

    #[test]
    fn tool_loop_subagent_runner_rejects_unknown_subagent() {
        let model = Arc::new(SendMockLanguageModel::new("parent-model"));
        let context = SubagentInheritedContext::new(model);
        let registry = SubagentRegistry::new([SubagentProfile {
            subagent_type: SubagentType::Explorer,
            short_description: "only explorer",
            system_prompt: EXPLORER_SYSTEM_PROMPT,
            default_model_id: "test",
            max_steps: 1,
            allowed_tool_names: EXPLORER_TOOLS,
            can_modify_files: false,
        }]);
        let runner = ToolLoopSubagentRunner::new(context).with_registry(registry);

        let error = futures_executor::block_on(runner.run(
            TaskToolInput {
                subagent_type: SubagentType::Design,
                task: "Design".to_string(),
                instructions: "Design".to_string(),
            },
            SubagentTaskToolRunOptions {
                tool_call_id: "task-call".to_string(),
                messages: Vec::new(),
                context: None,
                experimental_sandbox: None,
                abort_signal: None,
                started_at: 0,
                depth: 0,
            },
        ))
        .expect_err("missing registry entry is rejected");

        assert_eq!(
            error,
            SubagentTaskError::UnknownSubagent {
                requested: "design".to_string()
            }
        );
    }

    #[test]
    fn tool_loop_subagent_runner_propagates_parent_cancellation_to_model_call() {
        let model = Arc::new(SendMockLanguageModel::new("parent-model"));
        model.push_result(text_result("cancel observed", usage(1, 1)));
        let context = SubagentInheritedContext::new(Arc::clone(&model));
        let runner = ToolLoopSubagentRunner::new(context);
        let controller = LanguageModelAbortController::new();
        controller.abort_with_reason(json!("stop"));

        let _ = futures_executor::block_on(runner.run(
            TaskToolInput {
                subagent_type: SubagentType::Explorer,
                task: "Observe".to_string(),
                instructions: "Observe".to_string(),
            },
            SubagentTaskToolRunOptions {
                tool_call_id: "task-call".to_string(),
                messages: Vec::new(),
                context: None,
                experimental_sandbox: None,
                abort_signal: Some(controller.signal()),
                started_at: 0,
                depth: 0,
            },
        ))
        .expect("runner completes");

        let calls = model.calls();
        let signal = calls[0]
            .abort_signal
            .as_ref()
            .expect("subagent model call receives child abort signal");
        assert!(signal.is_aborted());
        assert_eq!(signal.reason(), Some(json!("stop")));
    }
}
