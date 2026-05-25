use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_provider::json::JsonValue;
use ai_sdk_provider::{
    FinishReason, InputTokenUsage, LanguageModelAbortSignal, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelMessage, LanguageModelSystemMessage,
    LanguageModelTextPart, LanguageModelToolApprovalResponsePart, LanguageModelToolCallPart,
    LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultOutput,
    LanguageModelToolResultPart, LanguageModelUsage, LanguageModelUserContentPart,
    LanguageModelUserMessage, OutputTokenUsage,
};
use ai_sdk_provider_utils::{
    ExecuteToolOutput, Tool, ToolExecutionOptions, ToolModelOutputOptions,
    ToolNeedsApprovalOptions, execute_tool,
};
use ai_sdk_rust::{StopCondition, TelemetryOptions};
use serde::{Deserialize, Serialize};

use crate::{
    ParsedToolCall, ProviderExecutedToolResult, StreamTextIterator, WorkflowGenerationSettings,
    WorkflowModelInfo, WorkflowPrepareStepCallback, WorkflowPrompt, WorkflowRuntimeContext,
    WorkflowStreamStep, WorkflowStreamTextError, WorkflowStreamTextOnErrorCallback,
    WorkflowStreamTextStepExecutor, WorkflowToolCallRepairCallback, WorkflowToolsContext,
};

/// Constructor options for [`WorkflowAgent`].
#[derive(Clone, Debug)]
pub struct WorkflowAgentOptions {
    /// Agent identifier exposed to callers.
    pub id: Option<String>,

    /// Default model identity for this agent.
    pub model: WorkflowModelInfo,

    /// Runtime tools available to the agent.
    pub tools: BTreeMap<String, Tool>,

    /// Default system instructions for every stream call on this agent.
    pub instructions: Option<String>,

    /// Default generation settings.
    pub generation_settings: WorkflowGenerationSettings,

    /// Default active tools list.
    pub active_tools: Option<Vec<String>>,

    /// Default serialized tool-choice value.
    pub tool_choice: Option<JsonValue>,

    /// Default stop conditions.
    pub stop_conditions: Vec<StopCondition>,

    /// Default repair callback for malformed tool calls.
    pub experimental_repair_tool_call: Option<WorkflowToolCallRepairCallback>,

    /// Default stream error callback.
    pub on_error: Option<WorkflowStreamTextOnErrorCallback>,

    /// Default telemetry settings.
    pub telemetry: Option<TelemetryOptions>,

    /// Default prepare-step callback.
    pub prepare_step: Option<WorkflowPrepareStepCallback>,

    /// Default stream-start callback.
    pub on_start: Option<WorkflowAgentOnStartCallback>,

    /// Default step-start callback.
    pub on_step_start: Option<WorkflowAgentOnStepStartCallback>,

    /// Default step-finish callback.
    pub on_step_finish: Option<WorkflowAgentOnStepFinishCallback>,

    /// Default tool-execution start callback.
    pub on_tool_execution_start: Option<WorkflowAgentOnToolExecutionStartCallback>,

    /// Default tool-execution end callback.
    pub on_tool_execution_end: Option<WorkflowAgentOnToolExecutionEndCallback>,

    /// Default finish callback.
    pub on_finish: Option<WorkflowAgentOnFinishCallback>,
}

impl WorkflowAgentOptions {
    /// Creates workflow-agent options with a default model.
    pub fn new(model: WorkflowModelInfo) -> Self {
        Self {
            id: None,
            model,
            tools: BTreeMap::new(),
            instructions: None,
            generation_settings: WorkflowGenerationSettings::default(),
            active_tools: None,
            tool_choice: None,
            stop_conditions: Vec::new(),
            experimental_repair_tool_call: None,
            on_error: None,
            telemetry: None,
            prepare_step: None,
            on_start: None,
            on_step_start: None,
            on_step_finish: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_finish: None,
        }
    }

    /// Sets the optional agent id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Adds one runtime tool.
    pub fn with_tool(mut self, tool: Tool) -> Self {
        self.tools.insert(tool.name.clone(), tool);
        self
    }

    /// Adds runtime tools.
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.tools
            .extend(tools.into_iter().map(|tool| (tool.name.clone(), tool)));
        self
    }

    /// Sets constructor-level instructions.
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Sets constructor-level generation settings.
    pub fn with_generation_settings(
        mut self,
        generation_settings: WorkflowGenerationSettings,
    ) -> Self {
        self.generation_settings = generation_settings;
        self
    }

    /// Sets constructor-level active tools.
    pub fn with_active_tools(mut self, active_tools: impl IntoIterator<Item = String>) -> Self {
        self.active_tools = Some(active_tools.into_iter().collect());
        self
    }

    /// Sets constructor-level tool choice.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<JsonValue>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }

    /// Sets constructor-level stop conditions.
    pub fn with_stop_conditions(
        mut self,
        stop_conditions: impl IntoIterator<Item = StopCondition>,
    ) -> Self {
        self.stop_conditions = stop_conditions.into_iter().collect();
        self
    }

    /// Sets constructor-level repair callback.
    pub fn with_experimental_repair_tool_call(
        mut self,
        experimental_repair_tool_call: WorkflowToolCallRepairCallback,
    ) -> Self {
        self.experimental_repair_tool_call = Some(experimental_repair_tool_call);
        self
    }

    /// Sets constructor-level stream error callback.
    pub fn with_on_error(mut self, on_error: WorkflowStreamTextOnErrorCallback) -> Self {
        self.on_error = Some(on_error);
        self
    }

    /// Sets constructor-level telemetry settings.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Sets a constructor-level prepare-step callback.
    pub fn with_prepare_step(mut self, prepare_step: WorkflowPrepareStepCallback) -> Self {
        self.prepare_step = Some(prepare_step);
        self
    }

    /// Sets a constructor-level stream-start callback.
    pub fn with_on_start(mut self, on_start: WorkflowAgentOnStartCallback) -> Self {
        self.on_start = Some(on_start);
        self
    }

    /// Sets a constructor-level step-start callback.
    pub fn with_on_step_start(mut self, on_step_start: WorkflowAgentOnStepStartCallback) -> Self {
        self.on_step_start = Some(on_step_start);
        self
    }

    /// Sets a constructor-level step-finish callback.
    pub fn with_on_step_finish(
        mut self,
        on_step_finish: WorkflowAgentOnStepFinishCallback,
    ) -> Self {
        self.on_step_finish = Some(on_step_finish);
        self
    }

    /// Sets a constructor-level tool-execution start callback.
    pub fn with_on_tool_execution_start(
        mut self,
        on_tool_execution_start: WorkflowAgentOnToolExecutionStartCallback,
    ) -> Self {
        self.on_tool_execution_start = Some(on_tool_execution_start);
        self
    }

    /// Sets a constructor-level tool-execution end callback.
    pub fn with_on_tool_execution_end(
        mut self,
        on_tool_execution_end: WorkflowAgentOnToolExecutionEndCallback,
    ) -> Self {
        self.on_tool_execution_end = Some(on_tool_execution_end);
        self
    }

    /// Sets a constructor-level finish callback.
    pub fn with_on_finish(mut self, on_finish: WorkflowAgentOnFinishCallback) -> Self {
        self.on_finish = Some(on_finish);
        self
    }
}

/// Input accepted by [`WorkflowAgentStreamOptions::new`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowPromptInput {
    /// A simple text prompt.
    Text(String),

    /// A list of already-standardized messages.
    Messages(WorkflowPrompt),
}

impl WorkflowPromptInput {
    fn into_prompt(self) -> WorkflowPrompt {
        match self {
            Self::Text(text) => vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new(text),
                )],
            ))],
            Self::Messages(messages) => messages,
        }
    }
}

impl From<String> for WorkflowPromptInput {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for WorkflowPromptInput {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<WorkflowPrompt> for WorkflowPromptInput {
    fn from(messages: WorkflowPrompt) -> Self {
        Self::Messages(messages)
    }
}

/// Deterministic Rust equivalent of upstream `WorkflowAgent`.
#[derive(Clone, Debug)]
pub struct WorkflowAgent {
    id: Option<String>,
    model: WorkflowModelInfo,
    tools: BTreeMap<String, Tool>,
    instructions: Option<String>,
    generation_settings: WorkflowGenerationSettings,
    active_tools: Option<Vec<String>>,
    tool_choice: Option<JsonValue>,
    stop_conditions: Vec<StopCondition>,
    experimental_repair_tool_call: Option<WorkflowToolCallRepairCallback>,
    on_error: Option<WorkflowStreamTextOnErrorCallback>,
    telemetry: Option<TelemetryOptions>,
    prepare_step: Option<WorkflowPrepareStepCallback>,
    on_start: Option<WorkflowAgentOnStartCallback>,
    on_step_start: Option<WorkflowAgentOnStepStartCallback>,
    on_step_finish: Option<WorkflowAgentOnStepFinishCallback>,
    on_tool_execution_start: Option<WorkflowAgentOnToolExecutionStartCallback>,
    on_tool_execution_end: Option<WorkflowAgentOnToolExecutionEndCallback>,
    on_finish: Option<WorkflowAgentOnFinishCallback>,
}

impl WorkflowAgent {
    /// Creates a workflow agent.
    pub fn new(options: WorkflowAgentOptions) -> Self {
        Self {
            id: options.id,
            model: options.model,
            tools: options.tools,
            instructions: options.instructions,
            generation_settings: options.generation_settings,
            active_tools: options.active_tools,
            tool_choice: options.tool_choice,
            stop_conditions: options.stop_conditions,
            experimental_repair_tool_call: options.experimental_repair_tool_call,
            on_error: options.on_error,
            telemetry: options.telemetry,
            prepare_step: options.prepare_step,
            on_start: options.on_start,
            on_step_start: options.on_step_start,
            on_step_finish: options.on_step_finish,
            on_tool_execution_start: options.on_tool_execution_start,
            on_tool_execution_end: options.on_tool_execution_end,
            on_finish: options.on_finish,
        }
    }

    /// Returns the optional agent id.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the default model identity.
    pub fn model(&self) -> &WorkflowModelInfo {
        &self.model
    }

    /// Returns the configured runtime tools.
    pub fn tools(&self) -> &BTreeMap<String, Tool> {
        &self.tools
    }

    /// Runs the agent loop with a supplied deterministic stream-step executor.
    pub async fn stream<E>(
        &self,
        options: WorkflowAgentStreamOptions<E>,
    ) -> Result<WorkflowAgentStreamResult, WorkflowAgentError>
    where
        E: WorkflowStreamTextStepExecutor,
    {
        let generation_settings = options
            .generation_settings
            .unwrap_or_else(|| self.generation_settings.clone());
        let active_tools = options
            .active_tools
            .or_else(|| self.active_tools.clone())
            .unwrap_or_default();
        let tool_choice = options.tool_choice.or_else(|| self.tool_choice.clone());
        let stop_conditions = if options.stop_conditions.is_empty() {
            self.stop_conditions.clone()
        } else {
            options.stop_conditions
        };
        let experimental_repair_tool_call = options
            .experimental_repair_tool_call
            .or_else(|| self.experimental_repair_tool_call.clone());
        let on_error = options.on_error.or_else(|| self.on_error.clone());
        let telemetry = options.telemetry.or_else(|| self.telemetry.clone());
        let include_raw_chunks = options.include_raw_chunks;
        let prepare_step = options.prepare_step.or_else(|| self.prepare_step.clone());
        let constructor_on_start = self.on_start.clone();
        let stream_on_start = options.on_start;
        let constructor_on_step_start = self.on_step_start.clone();
        let stream_on_step_start = options.on_step_start;
        let constructor_on_step_finish = self.on_step_finish.clone();
        let stream_on_step_finish = options.on_step_finish;
        let constructor_on_tool_execution_start = self.on_tool_execution_start.clone();
        let stream_on_tool_execution_start = options.on_tool_execution_start;
        let constructor_on_tool_execution_end = self.on_tool_execution_end.clone();
        let stream_on_tool_execution_end = options.on_tool_execution_end;
        let constructor_on_finish = self.on_finish.clone();
        let stream_on_finish = options.on_finish;
        let abort_signal = options.abort_signal.clone();
        let on_abort = options.on_abort;

        let initial_runtime_context = options.runtime_context.clone();
        let initial_tools_context = options.tools_context.clone();
        let mut prompt = options.prompt;

        if let Some(instructions) = self.instructions.as_ref() {
            prompt.insert(
                0,
                LanguageModelMessage::System(LanguageModelSystemMessage::new(instructions.clone())),
            );
        }

        apply_tool_approvals_before_stream(
            &mut prompt,
            &self.tools,
            &initial_tools_context,
            &constructor_on_tool_execution_start,
            &stream_on_tool_execution_start,
            &constructor_on_tool_execution_end,
            &stream_on_tool_execution_end,
        )
        .await?;

        let initial_prompt = prompt.clone();

        let mut iterator = StreamTextIterator::from_runtime_tools(
            prompt,
            self.tools.values().cloned(),
            options.executor,
        )
        .with_model(self.model.clone())
        .with_generation_settings(generation_settings.clone())
        .with_runtime_context(options.runtime_context)
        .with_tools_context(options.tools_context)
        .with_include_raw_chunks(include_raw_chunks);

        if let Some(telemetry) = telemetry {
            iterator = iterator.with_telemetry(telemetry);
        }

        if !active_tools.is_empty() {
            iterator = iterator.with_active_tools(active_tools);
        }
        if let Some(tool_choice) = tool_choice {
            iterator = iterator.with_tool_choice(tool_choice);
        }
        if !stop_conditions.is_empty() {
            iterator = iterator.with_stop_conditions(stop_conditions);
        }
        if let Some(experimental_repair_tool_call) = experimental_repair_tool_call {
            iterator = iterator.with_repair_tool_call(experimental_repair_tool_call);
        }
        if let Some(on_error) = on_error {
            iterator = iterator.with_on_error(on_error);
        }
        if let Some(prepare_step) = prepare_step {
            iterator = iterator.with_prepare_step(prepare_step);
        }

        call_start_callbacks(
            &constructor_on_start,
            &stream_on_start,
            &WorkflowAgentStartInfo {
                model: self.model.clone(),
                messages: initial_prompt.clone(),
                generation_settings: generation_settings.clone(),
                runtime_context: initial_runtime_context.clone(),
                tools_context: initial_tools_context.clone(),
            },
        );

        let mut pending_tool_results = None;
        let mut steps = Vec::new();
        let mut messages = iterator.prompt().to_vec();
        let mut runtime_context = initial_runtime_context;
        let mut tools_context = initial_tools_context;
        let mut last_tool_calls = Vec::new();
        let mut last_tool_results = Vec::new();
        let mut missing_provider_executed_tool_results = Vec::new();

        if abort_signal
            .as_ref()
            .is_some_and(LanguageModelAbortSignal::is_aborted)
        {
            if let Some(on_abort) = on_abort {
                on_abort.call(WorkflowAgentAbortInfo { steps: Vec::new() });
            }

            return Ok(WorkflowAgentStreamResult {
                messages,
                steps,
                tool_calls: last_tool_calls,
                tool_results: last_tool_results,
                runtime_context,
                tools_context,
                missing_provider_executed_tool_results,
            });
        }

        loop {
            call_step_start_callbacks(
                &constructor_on_step_start,
                &stream_on_step_start,
                &WorkflowAgentStepStartInfo {
                    model: self.model.clone(),
                    step_number: steps.len(),
                    steps: steps.clone(),
                    messages: messages.clone(),
                    generation_settings: generation_settings.clone(),
                    runtime_context: runtime_context.clone(),
                    tools_context: tools_context.clone(),
                },
            );

            let Some(yield_value) = iterator
                .next(pending_tool_results.take())
                .map_err(WorkflowAgentError::Stream)?
            else {
                break;
            };

            steps.push(yield_value.step.clone());
            messages = yield_value.messages.clone();
            runtime_context = yield_value.runtime_context.clone();
            tools_context = yield_value.tools_context.clone();

            if yield_value.tool_calls.is_empty() {
                last_tool_calls.clear();
                last_tool_results.clear();
                call_step_finish_callbacks(
                    &constructor_on_step_finish,
                    &stream_on_step_finish,
                    &yield_value.step,
                );
                break;
            }

            let execution = self
                .execute_tool_calls(
                    &yield_value,
                    &constructor_on_tool_execution_start,
                    &stream_on_tool_execution_start,
                    &constructor_on_tool_execution_end,
                    &stream_on_tool_execution_end,
                )
                .await?;
            missing_provider_executed_tool_results
                .extend(execution.missing_provider_executed_tool_results);

            last_tool_calls = yield_value.tool_calls.clone();
            last_tool_results = execution.tool_results.clone();

            call_step_finish_callbacks(
                &constructor_on_step_finish,
                &stream_on_step_finish,
                &yield_value.step,
            );

            if execution.has_unresolved_client_tools {
                break;
            }

            pending_tool_results = Some(execution.tool_results);
        }

        let result = WorkflowAgentStreamResult {
            messages,
            steps,
            tool_calls: last_tool_calls,
            tool_results: last_tool_results,
            runtime_context,
            tools_context,
            missing_provider_executed_tool_results,
        };

        if let Some(on_finish) = constructor_on_finish {
            on_finish.call(WorkflowAgentFinishInfo::from(&result));
        }
        if let Some(on_finish) = stream_on_finish {
            on_finish.call(WorkflowAgentFinishInfo::from(&result));
        }

        Ok(result)
    }

    async fn execute_tool_calls(
        &self,
        yield_value: &crate::StreamTextIteratorYieldValue,
        constructor_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
        stream_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
        constructor_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
        stream_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
    ) -> Result<WorkflowAgentToolExecution, WorkflowAgentError> {
        let mut execution = WorkflowAgentToolExecution::default();

        for tool_call in &yield_value.tool_calls {
            if tool_call.provider_executed == Some(true) {
                execution.tool_results.push(provider_executed_tool_result(
                    tool_call,
                    yield_value
                        .provider_executed_tool_results
                        .get(&tool_call.tool_call_id),
                    &mut execution.missing_provider_executed_tool_results,
                ));
                continue;
            }

            if tool_call.invalid == Some(true) {
                execution
                    .tool_results
                    .push(LanguageModelToolResultPart::new(
                        tool_call.tool_call_id.clone(),
                        tool_call.tool_name.clone(),
                        LanguageModelToolResultOutput::error_text(
                            tool_call.error.clone().unwrap_or_else(|| {
                                format!("Invalid input for tool {}", tool_call.tool_name)
                            }),
                        ),
                    ));
                continue;
            }

            let Some(tool) = self.tools.get(&tool_call.tool_name) else {
                execution.has_unresolved_client_tools = true;
                continue;
            };

            if !tool.is_executable() {
                execution.has_unresolved_client_tools = true;
                continue;
            }

            let context = validated_tool_context(tool, tool_call, &yield_value.tools_context)?;
            if tool_needs_approval(
                tool,
                tool_call,
                yield_value.messages.clone(),
                context.clone(),
            )
            .await
            {
                execution.has_unresolved_client_tools = true;
                continue;
            }

            let tool_result = execute_local_tool_with_callbacks(
                tool,
                tool_call,
                yield_value.messages.clone(),
                context,
                yield_value.step.step_number,
                constructor_on_tool_execution_start,
                stream_on_tool_execution_start,
                constructor_on_tool_execution_end,
                stream_on_tool_execution_end,
            )
            .await?;

            execution.tool_results.push(tool_result.tool_result);
        }

        Ok(execution)
    }
}

/// Per-call options for [`WorkflowAgent::stream`].
#[derive(Clone, Debug)]
pub struct WorkflowAgentStreamOptions<E> {
    /// Initial prompt.
    pub prompt: WorkflowPrompt,

    /// Deterministic stream-step executor.
    pub executor: E,

    /// Stream-level generation settings that override constructor defaults.
    pub generation_settings: Option<WorkflowGenerationSettings>,

    /// Stream-level telemetry settings that override constructor defaults.
    pub telemetry: Option<TelemetryOptions>,

    /// Stream-level timeout in milliseconds.
    pub timeout: Option<u64>,

    /// Whether raw provider chunks should be included in step results.
    pub include_raw_chunks: bool,

    /// Stream-level runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Stream-level per-tool context.
    pub tools_context: WorkflowToolsContext,

    /// Stream-level active tools that override constructor defaults.
    pub active_tools: Option<Vec<String>>,

    /// Stream-level tool choice that overrides constructor defaults.
    pub tool_choice: Option<JsonValue>,

    /// Stream-level stop conditions that override constructor defaults.
    pub stop_conditions: Vec<StopCondition>,

    /// Stream-level repair callback that overrides constructor defaults.
    pub experimental_repair_tool_call: Option<WorkflowToolCallRepairCallback>,

    /// Stream-level error callback that overrides constructor defaults.
    pub on_error: Option<WorkflowStreamTextOnErrorCallback>,

    /// Stream-level prepare-step callback that overrides constructor defaults.
    pub prepare_step: Option<WorkflowPrepareStepCallback>,

    /// Stream-level start callback that runs after any constructor callback.
    pub on_start: Option<WorkflowAgentOnStartCallback>,

    /// Stream-level step-start callback that runs after any constructor callback.
    pub on_step_start: Option<WorkflowAgentOnStepStartCallback>,

    /// Stream-level step-finish callback that runs after any constructor callback.
    pub on_step_finish: Option<WorkflowAgentOnStepFinishCallback>,

    /// Stream-level tool-execution start callback that runs after constructor callbacks.
    pub on_tool_execution_start: Option<WorkflowAgentOnToolExecutionStartCallback>,

    /// Stream-level tool-execution end callback that runs after constructor callbacks.
    pub on_tool_execution_end: Option<WorkflowAgentOnToolExecutionEndCallback>,

    /// Stream-level finish callback that runs after any constructor callback.
    pub on_finish: Option<WorkflowAgentOnFinishCallback>,

    /// Stream-level abort signal that short-circuits the agent before the first step.
    pub abort_signal: Option<LanguageModelAbortSignal>,

    /// Stream-level abort callback that runs when the stream is already aborted.
    pub on_abort: Option<WorkflowAgentOnAbortCallback>,
}

impl<E> WorkflowAgentStreamOptions<E> {
    /// Creates agent stream options.
    pub fn new(prompt: impl Into<WorkflowPromptInput>, executor: E) -> Self {
        let prompt = prompt.into().into_prompt();
        Self {
            prompt,
            executor,
            generation_settings: None,
            telemetry: None,
            timeout: None,
            include_raw_chunks: false,
            runtime_context: WorkflowRuntimeContext::new(),
            tools_context: WorkflowToolsContext::new(),
            active_tools: None,
            tool_choice: None,
            stop_conditions: Vec::new(),
            experimental_repair_tool_call: None,
            on_error: None,
            prepare_step: None,
            on_start: None,
            on_step_start: None,
            on_step_finish: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_finish: None,
            abort_signal: None,
            on_abort: None,
        }
    }

    /// Sets stream-level generation settings.
    pub fn with_generation_settings(
        mut self,
        generation_settings: WorkflowGenerationSettings,
    ) -> Self {
        self.generation_settings = Some(generation_settings);
        self
    }

    /// Sets stream-level telemetry settings.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Sets the stream-level timeout in milliseconds.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout = Some(timeout_ms);
        self
    }

    /// Sets whether raw provider chunks should be included.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.include_raw_chunks = include_raw_chunks;
        self
    }

    /// Sets runtime context.
    pub fn with_runtime_context(mut self, runtime_context: WorkflowRuntimeContext) -> Self {
        self.runtime_context = runtime_context;
        self
    }

    /// Sets per-tool context.
    pub fn with_tools_context(mut self, tools_context: WorkflowToolsContext) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets stream-level active tools.
    pub fn with_active_tools(mut self, active_tools: impl IntoIterator<Item = String>) -> Self {
        self.active_tools = Some(active_tools.into_iter().collect());
        self
    }

    /// Sets stream-level tool choice.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<JsonValue>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }

    /// Sets stream-level stop conditions.
    pub fn with_stop_conditions(
        mut self,
        stop_conditions: impl IntoIterator<Item = StopCondition>,
    ) -> Self {
        self.stop_conditions = stop_conditions.into_iter().collect();
        self
    }

    /// Sets stream-level repair callback.
    pub fn with_experimental_repair_tool_call(
        mut self,
        experimental_repair_tool_call: WorkflowToolCallRepairCallback,
    ) -> Self {
        self.experimental_repair_tool_call = Some(experimental_repair_tool_call);
        self
    }

    /// Sets stream-level error callback.
    pub fn with_on_error(mut self, on_error: WorkflowStreamTextOnErrorCallback) -> Self {
        self.on_error = Some(on_error);
        self
    }

    /// Sets a stream-level prepare-step callback.
    pub fn with_prepare_step(mut self, prepare_step: WorkflowPrepareStepCallback) -> Self {
        self.prepare_step = Some(prepare_step);
        self
    }

    /// Sets a stream-level start callback.
    pub fn with_on_start(mut self, on_start: WorkflowAgentOnStartCallback) -> Self {
        self.on_start = Some(on_start);
        self
    }

    /// Sets a stream-level step-start callback.
    pub fn with_on_step_start(mut self, on_step_start: WorkflowAgentOnStepStartCallback) -> Self {
        self.on_step_start = Some(on_step_start);
        self
    }

    /// Sets a stream-level step-finish callback.
    pub fn with_on_step_finish(
        mut self,
        on_step_finish: WorkflowAgentOnStepFinishCallback,
    ) -> Self {
        self.on_step_finish = Some(on_step_finish);
        self
    }

    /// Sets a stream-level tool-execution start callback.
    pub fn with_on_tool_execution_start(
        mut self,
        on_tool_execution_start: WorkflowAgentOnToolExecutionStartCallback,
    ) -> Self {
        self.on_tool_execution_start = Some(on_tool_execution_start);
        self
    }

    /// Sets a stream-level tool-execution end callback.
    pub fn with_on_tool_execution_end(
        mut self,
        on_tool_execution_end: WorkflowAgentOnToolExecutionEndCallback,
    ) -> Self {
        self.on_tool_execution_end = Some(on_tool_execution_end);
        self
    }

    /// Sets a stream-level finish callback.
    pub fn with_on_finish(mut self, on_finish: WorkflowAgentOnFinishCallback) -> Self {
        self.on_finish = Some(on_finish);
        self
    }

    /// Sets a stream-level abort signal.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets a stream-level abort callback.
    pub fn with_on_abort(mut self, on_abort: WorkflowAgentOnAbortCallback) -> Self {
        self.on_abort = Some(on_abort);
        self
    }
}

/// Callback invoked when [`WorkflowAgent::stream`] observes an already aborted signal.
#[derive(Clone)]
pub struct WorkflowAgentOnAbortCallback {
    callback: Arc<dyn Fn(WorkflowAgentAbortInfo) + Send + Sync + 'static>,
}

impl WorkflowAgentOnAbortCallback {
    /// Creates an abort callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowAgentAbortInfo) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowAgentAbortInfo) {
        (self.callback)(info);
    }
}

impl fmt::Debug for WorkflowAgentOnAbortCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnAbortCallback")
            .finish_non_exhaustive()
    }
}

/// Abort information passed to workflow-agent abort callbacks.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentAbortInfo {
    /// Steps completed before the abort was observed.
    pub steps: Vec<WorkflowStreamStep>,
}

/// Callback invoked before [`WorkflowAgent::stream`] starts iterating steps.
#[derive(Clone)]
pub struct WorkflowAgentOnStartCallback {
    callback: Arc<dyn Fn(WorkflowAgentStartInfo) + Send + Sync + 'static>,
}

impl WorkflowAgentOnStartCallback {
    /// Creates a start callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowAgentStartInfo) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowAgentStartInfo) {
        (self.callback)(info);
    }
}

impl fmt::Debug for WorkflowAgentOnStartCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnStartCallback")
            .finish_non_exhaustive()
    }
}

/// Callback invoked before each [`WorkflowAgent`] stream step starts.
#[derive(Clone)]
pub struct WorkflowAgentOnStepStartCallback {
    callback: Arc<dyn Fn(WorkflowAgentStepStartInfo) + Send + Sync + 'static>,
}

impl WorkflowAgentOnStepStartCallback {
    /// Creates a step-start callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowAgentStepStartInfo) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowAgentStepStartInfo) {
        (self.callback)(info);
    }
}

impl fmt::Debug for WorkflowAgentOnStepStartCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnStepStartCallback")
            .finish_non_exhaustive()
    }
}

/// Information passed when a workflow agent stream starts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentStartInfo {
    /// Model identity configured for the stream.
    pub model: WorkflowModelInfo,

    /// Initial prompt messages.
    pub messages: WorkflowPrompt,

    /// Generation settings active at stream start.
    pub generation_settings: WorkflowGenerationSettings,

    /// Runtime context supplied at stream start.
    pub runtime_context: WorkflowRuntimeContext,

    /// Tool contexts supplied at stream start.
    pub tools_context: WorkflowToolsContext,
}

/// Information passed before a workflow agent stream step starts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentStepStartInfo {
    /// Model identity configured for the stream.
    pub model: WorkflowModelInfo,

    /// Zero-based step number about to run.
    pub step_number: usize,

    /// Steps completed before this step.
    pub steps: Vec<WorkflowStreamStep>,

    /// Current conversation messages before this step starts.
    pub messages: WorkflowPrompt,

    /// Generation settings active for this stream.
    pub generation_settings: WorkflowGenerationSettings,

    /// Current runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Current tool contexts.
    pub tools_context: WorkflowToolsContext,
}

/// Callback invoked after each [`WorkflowAgent`] stream step completes.
#[derive(Clone)]
pub struct WorkflowAgentOnStepFinishCallback {
    callback: Arc<dyn Fn(WorkflowStreamStep) + Send + Sync + 'static>,
}

impl WorkflowAgentOnStepFinishCallback {
    /// Creates a step-finish callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowStreamStep) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, step: WorkflowStreamStep) {
        (self.callback)(step);
    }
}

impl fmt::Debug for WorkflowAgentOnStepFinishCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnStepFinishCallback")
            .finish_non_exhaustive()
    }
}

/// Callback invoked before a local workflow tool executor runs.
#[derive(Clone)]
pub struct WorkflowAgentOnToolExecutionStartCallback {
    callback: Arc<dyn Fn(WorkflowAgentToolExecutionStartInfo) + Send + Sync + 'static>,
}

impl WorkflowAgentOnToolExecutionStartCallback {
    /// Creates a tool-execution start callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowAgentToolExecutionStartInfo) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowAgentToolExecutionStartInfo) {
        (self.callback)(info);
    }
}

impl fmt::Debug for WorkflowAgentOnToolExecutionStartCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnToolExecutionStartCallback")
            .finish_non_exhaustive()
    }
}

/// Callback invoked after a local workflow tool executor completes.
#[derive(Clone)]
pub struct WorkflowAgentOnToolExecutionEndCallback {
    callback: Arc<dyn Fn(WorkflowAgentToolExecutionEndInfo) + Send + Sync + 'static>,
}

impl WorkflowAgentOnToolExecutionEndCallback {
    /// Creates a tool-execution end callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowAgentToolExecutionEndInfo) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowAgentToolExecutionEndInfo) {
        (self.callback)(info);
    }
}

impl fmt::Debug for WorkflowAgentOnToolExecutionEndCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnToolExecutionEndCallback")
            .finish_non_exhaustive()
    }
}

/// Information passed before a workflow tool is executed locally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentToolExecutionStartInfo {
    /// Tool call about to be executed.
    pub tool_call: ParsedToolCall,

    /// Zero-based workflow step number.
    pub step_number: usize,

    /// Prompt messages that produced the tool call.
    pub messages: WorkflowPrompt,

    /// Tool-specific context supplied for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_context: Option<JsonValue>,
}

/// Information passed after a workflow tool is executed locally.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentToolExecutionEndInfo {
    /// Tool call that was executed.
    pub tool_call: ParsedToolCall,

    /// Zero-based workflow step number.
    pub step_number: usize,

    /// Prompt messages that produced the tool call.
    pub messages: WorkflowPrompt,

    /// Tool-specific context supplied for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_context: Option<JsonValue>,

    /// Execution time in milliseconds.
    pub duration_ms: u64,

    /// Whether the tool executor completed successfully.
    pub success: bool,

    /// Raw tool output for successful executions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<JsonValue>,

    /// Error message for failed executions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Callback invoked when [`WorkflowAgent::stream`] finishes successfully.
#[derive(Clone)]
pub struct WorkflowAgentOnFinishCallback {
    callback: Arc<dyn Fn(WorkflowAgentFinishInfo) + Send + Sync + 'static>,
}

impl WorkflowAgentOnFinishCallback {
    /// Creates a finish callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowAgentFinishInfo) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowAgentFinishInfo) {
        (self.callback)(info);
    }
}

impl fmt::Debug for WorkflowAgentOnFinishCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowAgentOnFinishCallback")
            .finish_non_exhaustive()
    }
}

/// Finish information passed to workflow-agent finish callbacks.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentFinishInfo {
    /// Concatenated generated text from completed steps.
    pub text: String,

    /// Final unified finish reason.
    pub finish_reason: FinishReason,

    /// Final raw provider finish reason, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Aggregate usage across all completed steps.
    pub total_usage: LanguageModelUsage,

    /// Final conversation messages observed by the agent loop.
    pub messages: Vec<LanguageModelMessage>,

    /// Completed stream steps.
    pub steps: Vec<WorkflowStreamStep>,

    /// Last unresolved or executed tool calls observed by the loop.
    pub tool_calls: Vec<ParsedToolCall>,

    /// Tool results generated by the last tool-call round.
    pub tool_results: Vec<LanguageModelToolResultPart>,

    /// Final runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Final per-tool context.
    pub tools_context: WorkflowToolsContext,

    /// Provider-executed tool calls that had no matching provider result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_provider_executed_tool_results: Vec<String>,
}

impl From<&WorkflowAgentStreamResult> for WorkflowAgentFinishInfo {
    fn from(result: &WorkflowAgentStreamResult) -> Self {
        let final_step = result.steps.last();
        Self {
            text: result.steps.iter().map(|step| step.text.as_str()).collect(),
            finish_reason: final_step
                .map(|step| step.finish_reason.clone())
                .unwrap_or(FinishReason::Other),
            raw_finish_reason: final_step.and_then(|step| step.raw_finish_reason.clone()),
            total_usage: add_workflow_step_usage(&result.steps),
            messages: result.messages.clone(),
            steps: result.steps.clone(),
            tool_calls: result.tool_calls.clone(),
            tool_results: result.tool_results.clone(),
            runtime_context: result.runtime_context.clone(),
            tools_context: result.tools_context.clone(),
            missing_provider_executed_tool_results: result
                .missing_provider_executed_tool_results
                .clone(),
        }
    }
}

/// Result returned by [`WorkflowAgent::stream`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentStreamResult {
    /// Final conversation messages observed by the agent loop.
    pub messages: Vec<LanguageModelMessage>,

    /// Completed stream steps.
    pub steps: Vec<WorkflowStreamStep>,

    /// Last unresolved or executed tool calls observed by the loop.
    pub tool_calls: Vec<ParsedToolCall>,

    /// Tool results generated by the last tool-call round.
    pub tool_results: Vec<LanguageModelToolResultPart>,

    /// Final runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Final per-tool context.
    pub tools_context: WorkflowToolsContext,

    /// Provider-executed tool calls that had no matching provider result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_provider_executed_tool_results: Vec<String>,
}

/// Error returned by workflow-agent execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowAgentError {
    /// Stream iterator failed.
    Stream(WorkflowStreamTextError),

    /// A tool-specific context failed validation.
    InvalidToolContext {
        /// Tool whose context failed validation.
        tool_name: String,

        /// Validation message.
        message: String,
    },
}

impl fmt::Display for WorkflowAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stream(error) => write!(formatter, "{error}"),
            Self::InvalidToolContext { tool_name, message } => {
                write!(
                    formatter,
                    "invalid context for tool '{tool_name}': {message}"
                )
            }
        }
    }
}

impl Error for WorkflowAgentError {}

fn call_start_callbacks(
    constructor_on_start: &Option<WorkflowAgentOnStartCallback>,
    stream_on_start: &Option<WorkflowAgentOnStartCallback>,
    info: &WorkflowAgentStartInfo,
) {
    if let Some(on_start) = constructor_on_start {
        on_start.call(info.clone());
    }
    if let Some(on_start) = stream_on_start {
        on_start.call(info.clone());
    }
}

fn call_step_start_callbacks(
    constructor_on_step_start: &Option<WorkflowAgentOnStepStartCallback>,
    stream_on_step_start: &Option<WorkflowAgentOnStepStartCallback>,
    info: &WorkflowAgentStepStartInfo,
) {
    if let Some(on_step_start) = constructor_on_step_start {
        on_step_start.call(info.clone());
    }
    if let Some(on_step_start) = stream_on_step_start {
        on_step_start.call(info.clone());
    }
}

fn call_step_finish_callbacks(
    constructor_on_step_finish: &Option<WorkflowAgentOnStepFinishCallback>,
    stream_on_step_finish: &Option<WorkflowAgentOnStepFinishCallback>,
    step: &WorkflowStreamStep,
) {
    if let Some(on_step_finish) = constructor_on_step_finish {
        on_step_finish.call(step.clone());
    }
    if let Some(on_step_finish) = stream_on_step_finish {
        on_step_finish.call(step.clone());
    }
}

fn call_tool_execution_start_callbacks(
    constructor_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
    stream_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
    info: &WorkflowAgentToolExecutionStartInfo,
) {
    if let Some(on_tool_execution_start) = constructor_on_tool_execution_start {
        on_tool_execution_start.call(info.clone());
    }
    if let Some(on_tool_execution_start) = stream_on_tool_execution_start {
        on_tool_execution_start.call(info.clone());
    }
}

fn call_tool_execution_end_callbacks(
    constructor_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
    stream_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
    info: &WorkflowAgentToolExecutionEndInfo,
) {
    if let Some(on_tool_execution_end) = constructor_on_tool_execution_end {
        on_tool_execution_end.call(info.clone());
    }
    if let Some(on_tool_execution_end) = stream_on_tool_execution_end {
        on_tool_execution_end.call(info.clone());
    }
}

fn add_workflow_step_usage(steps: &[WorkflowStreamStep]) -> LanguageModelUsage {
    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: sum_optional_u64(steps.iter().map(|step| step.usage.input_tokens.total)),
            no_cache: sum_optional_u64(steps.iter().map(|step| step.usage.input_tokens.no_cache)),
            cache_read: sum_optional_u64(
                steps.iter().map(|step| step.usage.input_tokens.cache_read),
            ),
            cache_write: sum_optional_u64(
                steps.iter().map(|step| step.usage.input_tokens.cache_write),
            ),
        },
        output_tokens: OutputTokenUsage {
            total: sum_optional_u64(steps.iter().map(|step| step.usage.output_tokens.total)),
            text: sum_optional_u64(steps.iter().map(|step| step.usage.output_tokens.text)),
            reasoning: sum_optional_u64(
                steps.iter().map(|step| step.usage.output_tokens.reasoning),
            ),
        },
        raw: None,
    }
}

fn sum_optional_u64(values: impl IntoIterator<Item = Option<u64>>) -> Option<u64> {
    let mut total = 0_u64;
    let mut saw_value = false;
    for value in values.into_iter().flatten() {
        saw_value = true;
        total = total.saturating_add(value);
    }
    saw_value.then_some(total)
}

#[derive(Clone, Debug, Default)]
struct WorkflowAgentToolExecution {
    tool_results: Vec<LanguageModelToolResultPart>,
    has_unresolved_client_tools: bool,
    missing_provider_executed_tool_results: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct WorkflowAgentLocalToolResult {
    tool_result: LanguageModelToolResultPart,
    success: bool,
    output: Option<JsonValue>,
    error: Option<String>,
}

async fn execute_local_tool(
    tool: &Tool,
    tool_call: &ParsedToolCall,
    messages: WorkflowPrompt,
    context: Option<JsonValue>,
) -> Result<WorkflowAgentLocalToolResult, WorkflowAgentError> {
    let mut options = ToolExecutionOptions::new(tool_call.tool_call_id.clone(), messages);
    if let Some(context) = context.clone() {
        options = options.with_context(context);
    }

    let (output, success, raw_output, error) =
        match execute_tool(tool, tool_call.input.clone(), options.clone()).await {
            Ok(outputs) => {
                let raw_output = final_tool_output(outputs).unwrap_or(JsonValue::Null);
                let output = if let Some(model_output) =
                    tool.model_output(ToolModelOutputOptions::new(
                        tool_call.tool_call_id.clone(),
                        tool_call.input.clone(),
                        raw_output.clone(),
                    )) {
                    model_output.await
                } else {
                    json_value_to_tool_result_output(raw_output.clone())
                };
                (output, true, Some(raw_output), None)
            }
            Err(error) => {
                let message = error.into_message();
                (
                    LanguageModelToolResultOutput::error_text(message.clone()),
                    false,
                    None,
                    Some(message),
                )
            }
        };

    Ok(WorkflowAgentLocalToolResult {
        tool_result: LanguageModelToolResultPart::new(
            tool_call.tool_call_id.clone(),
            tool_call.tool_name.clone(),
            output,
        ),
        success,
        output: raw_output,
        error,
    })
}

async fn execute_local_tool_with_callbacks(
    tool: &Tool,
    tool_call: &ParsedToolCall,
    messages: WorkflowPrompt,
    context: Option<JsonValue>,
    step_number: usize,
    constructor_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
    stream_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
    constructor_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
    stream_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
) -> Result<WorkflowAgentLocalToolResult, WorkflowAgentError> {
    let start_info = WorkflowAgentToolExecutionStartInfo {
        tool_call: tool_call.clone(),
        messages: messages.clone(),
        tool_context: context.clone(),
        step_number,
    };
    call_tool_execution_start_callbacks(
        constructor_on_tool_execution_start,
        stream_on_tool_execution_start,
        &start_info,
    );

    let started_at = Instant::now();
    let tool_result =
        execute_local_tool(tool, tool_call, messages.clone(), context.clone()).await?;
    let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    let end_info = WorkflowAgentToolExecutionEndInfo {
        tool_call: tool_call.clone(),
        messages,
        tool_context: context,
        step_number,
        duration_ms,
        success: tool_result.success,
        output: tool_result.output.clone(),
        error: tool_result.error.clone(),
    };
    call_tool_execution_end_callbacks(
        constructor_on_tool_execution_end,
        stream_on_tool_execution_end,
        &end_info,
    );

    Ok(tool_result)
}

#[derive(Clone, Debug)]
struct CollectedToolApproval {
    approval_response: LanguageModelToolApprovalResponsePart,
    tool_call: LanguageModelToolCallPart,
}

fn parsed_tool_call_from_language_model_tool_call(
    tool_call: &LanguageModelToolCallPart,
) -> ParsedToolCall {
    ParsedToolCall {
        kind: "tool-call".to_string(),
        tool_call_id: tool_call.tool_call_id.clone(),
        tool_name: tool_call.tool_name.clone(),
        input: tool_call.input.clone(),
        provider_executed: tool_call.provider_executed,
        provider_metadata: None,
        dynamic: None,
        invalid: None,
        error: None,
    }
}

fn collect_workflow_tool_approvals(
    messages: &[LanguageModelMessage],
) -> (Vec<CollectedToolApproval>, Vec<CollectedToolApproval>) {
    let Some(LanguageModelMessage::Tool(last_message)) = messages.last() else {
        return (Vec::new(), Vec::new());
    };

    let mut tool_calls_by_id = BTreeMap::new();
    let mut approval_requests_by_id = BTreeMap::new();
    for message in messages {
        let LanguageModelMessage::Assistant(message) = message else {
            continue;
        };

        for part in &message.content {
            match part {
                LanguageModelAssistantContentPart::ToolCall(part) => {
                    tool_calls_by_id.insert(part.tool_call_id.clone(), part.clone());
                }
                LanguageModelAssistantContentPart::ToolApprovalRequest(part) => {
                    approval_requests_by_id.insert(part.approval_id.clone(), part.clone());
                }
                _ => {}
            }
        }
    }

    let existing_tool_results = last_message
        .content
        .iter()
        .filter_map(|part| match part {
            LanguageModelToolContentPart::ToolResult(part) => Some(part.tool_call_id.clone()),
            LanguageModelToolContentPart::ToolApprovalResponse(_) => None,
        })
        .collect::<std::collections::BTreeSet<_>>();

    let mut approved = Vec::new();
    let mut denied = Vec::new();

    for response in last_message.content.iter().filter_map(|part| match part {
        LanguageModelToolContentPart::ToolApprovalResponse(part) => Some(part.clone()),
        LanguageModelToolContentPart::ToolResult(_) => None,
    }) {
        let Some(approval_request) = approval_requests_by_id.get(&response.approval_id) else {
            continue;
        };
        if existing_tool_results.contains(&approval_request.tool_call_id) {
            continue;
        }
        let Some(tool_call) = tool_calls_by_id.get(&approval_request.tool_call_id) else {
            continue;
        };

        let approval = CollectedToolApproval {
            approval_response: response,
            tool_call: tool_call.clone(),
        };

        if approval.approval_response.approved {
            approved.push(approval);
        } else {
            denied.push(approval);
        }
    }

    (approved, denied)
}

async fn apply_tool_approvals_before_stream(
    prompt: &mut WorkflowPrompt,
    tools: &BTreeMap<String, Tool>,
    tools_context: &WorkflowToolsContext,
    constructor_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
    stream_on_tool_execution_start: &Option<WorkflowAgentOnToolExecutionStartCallback>,
    constructor_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
    stream_on_tool_execution_end: &Option<WorkflowAgentOnToolExecutionEndCallback>,
) -> Result<(), WorkflowAgentError> {
    let (approved_tool_approvals, denied_tool_approvals) =
        collect_workflow_tool_approvals(prompt.as_slice());

    if approved_tool_approvals.is_empty() && denied_tool_approvals.is_empty() {
        return Ok(());
    }

    let mut tool_result_content = Vec::new();

    for approval in approved_tool_approvals {
        let Some(tool) = tools.get(&approval.tool_call.tool_name) else {
            continue;
        };
        if !tool.is_executable() {
            continue;
        }

        let parsed_tool_call = parsed_tool_call_from_language_model_tool_call(&approval.tool_call);
        let context = validated_tool_context(tool, &parsed_tool_call, tools_context)?;
        let result = execute_local_tool_with_callbacks(
            tool,
            &parsed_tool_call,
            prompt.clone(),
            context,
            0,
            constructor_on_tool_execution_start,
            stream_on_tool_execution_start,
            constructor_on_tool_execution_end,
            stream_on_tool_execution_end,
        )
        .await?;
        tool_result_content.push(LanguageModelToolContentPart::ToolResult(result.tool_result));
    }

    for denial in denied_tool_approvals {
        let mut output = LanguageModelToolResultOutput::execution_denied();
        if let Some(reason) = &denial.approval_response.reason {
            output = output.with_reason(reason.clone());
        }
        tool_result_content.push(LanguageModelToolContentPart::ToolResult(
            LanguageModelToolResultPart::new(
                denial.tool_call.tool_call_id.clone(),
                denial.tool_call.tool_name.clone(),
                output,
            ),
        ));
    }

    let mut cleaned_messages = Vec::new();
    for message in prompt.iter() {
        match message {
            LanguageModelMessage::Assistant(assistant_message) => {
                let filtered: Vec<LanguageModelAssistantContentPart> = assistant_message
                    .content
                    .iter()
                    .filter(|part| {
                        !matches!(
                            part,
                            LanguageModelAssistantContentPart::ToolApprovalRequest(_)
                        )
                    })
                    .cloned()
                    .collect();
                if !filtered.is_empty() {
                    let provider_options = assistant_message.provider_options.clone();
                    let mut cleaned_assistant_message =
                        LanguageModelAssistantMessage::new(filtered);
                    if let Some(provider_options) = provider_options {
                        cleaned_assistant_message =
                            cleaned_assistant_message.with_provider_options(provider_options);
                    }
                    cleaned_messages
                        .push(LanguageModelMessage::Assistant(cleaned_assistant_message));
                }
            }
            LanguageModelMessage::Tool(tool_message) => {
                let filtered: Vec<LanguageModelToolContentPart> = tool_message
                    .content
                    .iter()
                    .filter(|part| {
                        !matches!(part, LanguageModelToolContentPart::ToolApprovalResponse(_))
                    })
                    .cloned()
                    .collect();
                if !filtered.is_empty() {
                    let provider_options = tool_message.provider_options.clone();
                    let mut cleaned_tool_message = LanguageModelToolMessage::new(filtered);
                    if let Some(provider_options) = provider_options {
                        cleaned_tool_message =
                            cleaned_tool_message.with_provider_options(provider_options);
                    }
                    cleaned_messages.push(LanguageModelMessage::Tool(cleaned_tool_message));
                }
            }
            _ => cleaned_messages.push(message.clone()),
        }
    }

    if !tool_result_content.is_empty() {
        cleaned_messages.push(LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            tool_result_content,
        )));
    }

    *prompt = cleaned_messages;
    Ok(())
}

fn validated_tool_context(
    tool: &Tool,
    tool_call: &ParsedToolCall,
    tools_context: &WorkflowToolsContext,
) -> Result<Option<JsonValue>, WorkflowAgentError> {
    let context = tools_context
        .get(&tool_call.tool_name)
        .cloned()
        .flatten()
        .map(JsonValue::Object);

    let Some(context_schema) = tool.context_schema() else {
        return Ok(context);
    };

    let value = context.clone().unwrap_or(JsonValue::Null);
    let schema = context_schema.clone().into_schema();
    if let Some(result) = schema.validate(&value) {
        return result.into_result().map(Some).map_err(|message| {
            WorkflowAgentError::InvalidToolContext {
                tool_name: tool_call.tool_name.clone(),
                message,
            }
        });
    }

    Ok(context)
}

async fn tool_needs_approval(
    tool: &Tool,
    tool_call: &ParsedToolCall,
    messages: WorkflowPrompt,
    context: Option<JsonValue>,
) -> bool {
    if let Some(needs_approval) = tool.needs_approval() {
        return needs_approval;
    }

    let Some(approval_future) = tool.resolve_needs_approval(tool_call.input.clone(), {
        let mut options = ToolNeedsApprovalOptions::new(tool_call.tool_call_id.clone(), messages);
        if let Some(context) = context {
            options = options.with_context(context);
        }
        options
    }) else {
        return false;
    };

    approval_future.await
}

fn final_tool_output(outputs: Vec<ExecuteToolOutput>) -> Option<JsonValue> {
    outputs.into_iter().rev().find_map(|output| match output {
        ExecuteToolOutput::Final { output } => Some(output),
        ExecuteToolOutput::Preliminary { .. } => None,
    })
}

fn json_value_to_tool_result_output(value: JsonValue) -> LanguageModelToolResultOutput {
    match value {
        JsonValue::String(value) => LanguageModelToolResultOutput::text(value),
        value => LanguageModelToolResultOutput::json(value),
    }
}

fn provider_executed_tool_result(
    tool_call: &ParsedToolCall,
    result: Option<&ProviderExecutedToolResult>,
    missing_provider_executed_tool_results: &mut Vec<String>,
) -> LanguageModelToolResultPart {
    let Some(result) = result else {
        missing_provider_executed_tool_results.push(tool_call.tool_call_id.clone());
        return LanguageModelToolResultPart::new(
            tool_call.tool_call_id.clone(),
            tool_call.tool_name.clone(),
            LanguageModelToolResultOutput::text(""),
        );
    };

    let output = match (result.is_error == Some(true), &result.result) {
        (true, JsonValue::String(value)) => {
            LanguageModelToolResultOutput::error_text(value.clone())
        }
        (true, value) => LanguageModelToolResultOutput::error_json(value.clone()),
        (false, JsonValue::String(value)) => LanguageModelToolResultOutput::text(value.clone()),
        (false, value) => LanguageModelToolResultOutput::json(value.clone()),
    };

    LanguageModelToolResultPart::new(
        tool_call.tool_call_id.clone(),
        tool_call.tool_name.clone(),
        output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    use ai_sdk_provider::json::{JsonObject, JsonValue};
    use ai_sdk_provider::{
        FinishReason, InputTokenUsage, LanguageModelAbortController,
        LanguageModelAssistantContentPart, LanguageModelAssistantMessage,
        LanguageModelFinishReason, LanguageModelStreamFinish, LanguageModelStreamPart,
        LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
        LanguageModelToolCall, LanguageModelUsage, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage, ProviderMetadata,
    };
    use ai_sdk_provider_utils::{Schema, ToolExecutionError, ValidationResult};
    use serde_json::json;

    use crate::{
        DoStreamStepOptions, DoStreamStepOutput, ScriptedStreamTextStepCall,
        ScriptedStreamTextStepExecutor, SerializableToolSet, WorkflowPrepareStepResult,
        do_stream_step_from_parts,
    };

    fn model() -> WorkflowModelInfo {
        WorkflowModelInfo::new("test", "test-model")
    }

    fn object_schema() -> JsonObject {
        serde_json::from_value(json!({
            "type": "object",
            "additionalProperties": true
        }))
        .expect("schema is an object")
    }

    fn user_prompt() -> WorkflowPrompt {
        vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                ai_sdk_provider::LanguageModelTextPart::new("test"),
            )],
        ))]
    }

    fn user_text_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(ai_sdk_provider::LanguageModelTextPart::new(text)),
        ]))
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: Default::default(),
            output_tokens: OutputTokenUsage {
                total: Some(5),
                text: Some(5),
                reasoning: None,
            },
            raw: None,
        }
    }

    fn usage_with_totals(input_tokens: u64, output_tokens: u64) -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(input_tokens),
                no_cache: Some(input_tokens),
                cache_read: None,
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: Some(output_tokens),
                text: Some(output_tokens),
                reasoning: None,
            },
            raw: None,
        }
    }

    fn finish(reason: FinishReason) -> LanguageModelStreamPart {
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            usage(),
            LanguageModelFinishReason {
                unified: reason,
                raw: None,
            },
        ))
    }

    fn output_from_parts(
        parts: impl IntoIterator<Item = LanguageModelStreamPart>,
        step_number: usize,
    ) -> DoStreamStepOutput {
        do_stream_step_from_parts(
            parts,
            crate::DoStreamStepOptions {
                step_number,
                ..crate::DoStreamStepOptions::default()
            },
        )
    }

    fn tool_call_step(tool_call: LanguageModelToolCall) -> DoStreamStepOutput {
        output_from_parts(
            [
                LanguageModelStreamPart::ToolCall(tool_call),
                finish(FinishReason::ToolCalls),
            ],
            0,
        )
    }

    fn stop_step() -> DoStreamStepOutput {
        output_from_parts([finish(FinishReason::Stop)], 1)
    }

    #[derive(Debug)]
    struct RecordingStreamTextStepExecutor {
        steps: VecDeque<DoStreamStepOutput>,
        calls: Arc<Mutex<Vec<ScriptedStreamTextStepCall>>>,
    }

    impl RecordingStreamTextStepExecutor {
        fn new(
            steps: impl IntoIterator<Item = DoStreamStepOutput>,
        ) -> (Self, Arc<Mutex<Vec<ScriptedStreamTextStepCall>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    steps: steps.into_iter().collect(),
                    calls: calls.clone(),
                },
                calls,
            )
        }
    }

    impl WorkflowStreamTextStepExecutor for RecordingStreamTextStepExecutor {
        fn do_stream_step(
            &mut self,
            prompt: &[LanguageModelMessage],
            tools: &SerializableToolSet,
            options: &DoStreamStepOptions,
        ) -> Result<DoStreamStepOutput, WorkflowStreamTextError> {
            self.calls
                .lock()
                .expect("calls lock succeeds")
                .push(ScriptedStreamTextStepCall {
                    prompt: prompt.to_vec(),
                    tools: tools.clone(),
                    options: options.clone(),
                });
            self.steps
                .pop_front()
                .ok_or(WorkflowStreamTextError::MissingScriptedStep)
        }
    }

    fn executable_test_tool() -> Tool {
        Tool::new("testTool", object_schema())
            .with_execute(|_, _| async { Ok(json!("hello-result")) })
    }

    fn executable_tool_call_step(input: &str) -> DoStreamStepOutput {
        tool_call_step(LanguageModelToolCall::new("call-1", "testTool", input))
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn first_tool_result(
        tool_message: &ai_sdk_provider::LanguageModelToolMessage,
    ) -> &LanguageModelToolResultPart {
        match &tool_message.content[0] {
            ai_sdk_provider::LanguageModelToolContentPart::ToolResult(tool_result) => tool_result,
            ai_sdk_provider::LanguageModelToolContentPart::ToolApprovalResponse(_) => {
                panic!("expected tool result")
            }
        }
    }

    fn tool_message_from_prompt(
        prompt: &[LanguageModelMessage],
    ) -> &ai_sdk_provider::LanguageModelToolMessage {
        prompt
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended")
    }

    #[test]
    fn workflow_agent_upstream_should_expose_id_when_provided_in_constructor() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_id("my-agent"));

        assert_eq!(agent.id(), Some("my-agent"));
        assert_eq!(agent.model(), &model());
    }

    #[test]
    fn workflow_agent_upstream_should_have_undefined_id_when_not_provided() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));

        assert_eq!(agent.id(), None);
    }

    #[test]
    fn workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result() {
        let tool = Tool::new("testTool", object_schema())
            .with_execute(|_, _| async { Err(ToolExecutionError::new("This is a generic error")) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("test-call-id", "testTool", "{}")),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls, Vec::<ParsedToolCall>::new());
        let prompt = result.messages;
        let tool_message = prompt
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        assert_eq!(tool_message.content.len(), 1);
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "test-call-id");
        assert_eq!(tool_result.tool_name, "testTool");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("This is a generic error")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_convert_fatal_error_to_tool_error_result() {
        let tool = Tool::new("testTool", object_schema())
            .with_execute(|_, _| async { Err(ToolExecutionError::new("This is a fatal error")) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("test-call-id", "testTool", "{}")),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls, Vec::<ParsedToolCall>::new());
        let prompt = result.messages;
        let tool_message = prompt
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        assert_eq!(tool_message.content.len(), 1);
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "test-call-id");
        assert_eq!(tool_result.tool_name, "testTool");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("This is a fatal error")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_convert_non_fatal_error_to_tool_error_result() {
        let tool = Tool::new("testTool", object_schema())
            .with_execute(|_, _| async { Err(ToolExecutionError::new("This is a generic error")) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("test-call-id", "testTool", "{}")),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls, Vec::<ParsedToolCall>::new());
        let prompt = result.messages;
        let tool_message = prompt
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        assert_eq!(tool_message.content.len(), 1);
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "test-call-id");
        assert_eq!(tool_result.tool_name, "testTool");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("This is a generic error")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_successfully_execute_tools_that_return_normally() {
        let tool = Tool::new("testTool", object_schema()).with_execute(|_, _| async {
            Ok(json!({
                "success": true,
                "data": "test result"
            }))
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("test-call-id", "testTool", "{}")),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::json(json!({
                "success": true,
                "data": "test result"
            }))
        );
    }

    #[test]
    fn workflow_agent_upstream_should_skip_local_execution_for_provider_executed_tools() {
        let execute_calls = Arc::new(Mutex::new(0usize));
        let execute_calls_for_tool = Arc::clone(&execute_calls);
        let tool = Tool::new("localTool", object_schema()).with_execute(move |_, _| {
            let execute_calls = Arc::clone(&execute_calls_for_tool);
            async move {
                *execute_calls.lock().expect("counter lock succeeds") += 1;
                Ok(json!("should not run"))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let mut first = tool_call_step(
            LanguageModelToolCall::new("provider-call-id", "WebSearch", r#"{"query":"test"}"#)
                .with_provider_executed(true),
        );
        first.provider_executed_tool_results.insert(
            "provider-call-id".to_string(),
            ProviderExecutedToolResult {
                tool_call_id: "provider-call-id".to_string(),
                tool_name: "WebSearch".to_string(),
                result: json!("Search results for: test query"),
                is_error: Some(false),
            },
        );
        let executor = ScriptedStreamTextStepExecutor::new([first, stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(*execute_calls.lock().expect("counter lock succeeds"), 0);
        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "provider-call-id");
        assert_eq!(tool_result.tool_name, "WebSearch");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::text("Search results for: test query")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_handle_provider_executed_tool_errors_with_is_error_flag() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let mut first = tool_call_step(
            LanguageModelToolCall::new("provider-call-id", "WebSearch", r#"{"query":"test"}"#)
                .with_provider_executed(true),
        );
        first.provider_executed_tool_results.insert(
            "provider-call-id".to_string(),
            ProviderExecutedToolResult {
                tool_call_id: "provider-call-id".to_string(),
                tool_name: "WebSearch".to_string(),
                result: json!("Search failed: Rate limit exceeded"),
                is_error: Some(true),
            },
        );
        let executor = ScriptedStreamTextStepExecutor::new([first, stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("Search failed: Rate limit exceeded")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_handle_mixed_provider_executed_and_local_tools() {
        let local_execute_calls = Arc::new(Mutex::new(0usize));
        let local_execute_calls_for_tool = Arc::clone(&local_execute_calls);
        let local_tool = Tool::new("localTool", object_schema()).with_execute(move |_, _| {
            let local_execute_calls = Arc::clone(&local_execute_calls_for_tool);
            async move {
                *local_execute_calls.lock().expect("counter lock succeeds") += 1;
                Ok(json!({ "local": "result" }))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(local_tool));
        let mut first = output_from_parts(
            [
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "local-call-id",
                    "localTool",
                    "{}",
                )),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "provider-call-id",
                        "WebSearch",
                        r#"{"query":"test"}"#,
                    )
                    .with_provider_executed(true),
                ),
                finish(FinishReason::ToolCalls),
            ],
            0,
        );
        first.provider_executed_tool_results.insert(
            "provider-call-id".to_string(),
            ProviderExecutedToolResult {
                tool_call_id: "provider-call-id".to_string(),
                tool_name: "WebSearch".to_string(),
                result: json!({ "searchResults": ["result1", "result2"] }),
                is_error: Some(false),
            },
        );
        let executor = ScriptedStreamTextStepExecutor::new([first, stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(
            *local_execute_calls.lock().expect("counter lock succeeds"),
            1
        );
        let tool_message = tool_message_from_prompt(&result.messages);
        assert_eq!(tool_message.content.len(), 2);
        let local_result = match &tool_message.content[0] {
            ai_sdk_provider::LanguageModelToolContentPart::ToolResult(tool_result) => tool_result,
            ai_sdk_provider::LanguageModelToolContentPart::ToolApprovalResponse(_) => {
                panic!("expected local tool result")
            }
        };
        let provider_result = match &tool_message.content[1] {
            ai_sdk_provider::LanguageModelToolContentPart::ToolResult(tool_result) => tool_result,
            ai_sdk_provider::LanguageModelToolContentPart::ToolApprovalResponse(_) => {
                panic!("expected provider tool result")
            }
        };
        assert_eq!(
            local_result.output,
            LanguageModelToolResultOutput::json(json!({ "local": "result" }))
        );
        assert_eq!(
            provider_result.output,
            LanguageModelToolResultOutput::json(json!({
                "searchResults": ["result1", "result2"]
            }))
        );
    }

    #[test]
    fn workflow_agent_upstream_should_return_empty_result_when_provider_executed_tool_result_is_missing()
     {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(
                LanguageModelToolCall::new("missing-result-id", "WebSearch", r#"{"query":"test"}"#)
                    .with_provider_executed(true),
            ),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(
            result.missing_provider_executed_tool_results,
            vec!["missing-result-id"]
        );
        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.output, LanguageModelToolResultOutput::text(""));
    }

    #[test]
    fn workflow_agent_upstream_should_stop_the_loop_for_client_side_tools_without_execute() {
        let tool = Tool::new("askUser", object_schema());
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("ask-user-call-id", "askUser", r#"{"question":"Name?"}"#),
        )]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_call_id, "ask-user-call-id");
        assert!(result.tool_results.is_empty());
    }

    #[test]
    fn workflow_agent_upstream_should_handle_mixed_executable_and_client_side_tools_in_same_step() {
        let server_tool = Tool::new("serverTool", object_schema())
            .with_execute(|_, _| async { Ok(json!({ "data": "from-server" })) });
        let client_tool = Tool::new("clientTool", object_schema());
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tools([server_tool, client_tool]),
        );
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "server-call-id",
                    "serverTool",
                    "{}",
                )),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "client-call-id",
                    "clientTool",
                    r#"{"prompt":"confirm action"}"#,
                )),
                finish(FinishReason::ToolCalls),
            ],
            0,
        )]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_calls[0].tool_call_id, "server-call-id");
        assert_eq!(result.tool_calls[1].tool_call_id, "client-call-id");
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "server-call-id");
        assert_eq!(
            result.tool_results[0].output,
            LanguageModelToolResultOutput::json(json!({ "data": "from-server" }))
        );
    }

    #[test]
    fn workflow_agent_upstream_should_call_on_finish_when_stopping_for_client_side_tools() {
        let finish_info = Arc::new(Mutex::new(None));
        let finish_info_for_callback = Arc::clone(&finish_info);
        let tool = Tool::new("askUser", object_schema());
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("ask-id", "askUser", r#"{"question":"confirm?"}"#),
        )]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_finish(
                WorkflowAgentOnFinishCallback::new(move |info| {
                    *finish_info_for_callback
                        .lock()
                        .expect("finish info lock succeeds") = Some(info);
                }),
            ),
        ))
        .expect("agent stream succeeds");

        let info = finish_info
            .lock()
            .expect("finish info lock succeeds")
            .clone()
            .expect("on_finish was called");
        assert_eq!(info.steps.len(), 1);
        assert_eq!(info.tool_calls.len(), 1);
        assert_eq!(info.tool_calls[0].tool_call_id, "ask-id");
        assert!(info.tool_results.is_empty());
        assert!(!info.messages.is_empty());
    }

    #[test]
    fn workflow_agent_compat_should_call_on_finish_from_constructor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_finish(
            WorkflowAgentOnFinishCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_on_finish_from_stream_method() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_finish(
                WorkflowAgentOnFinishCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_both_constructor_and_method_on_finish_in_correct_order() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_finish(
            WorkflowAgentOnFinishCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_finish(
                WorkflowAgentOnFinishCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_pass_finish_event_information() {
        let finish_info = Arc::new(Mutex::new(None));
        let finish_info_for_callback = Arc::clone(&finish_info);
        let step = output_from_parts(
            [
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1", "Hello, ",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage_with_totals(3, 10),
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("stop".to_string()),
                    },
                )),
            ],
            0,
        );
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([step]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_finish(
                WorkflowAgentOnFinishCallback::new(move |info| {
                    *finish_info_for_callback
                        .lock()
                        .expect("finish info lock succeeds") = Some(info);
                }),
            ),
        ))
        .expect("agent stream succeeds");

        let info = finish_info
            .lock()
            .expect("finish info lock succeeds")
            .clone()
            .expect("on_finish was called");
        assert_eq!(info.text, "Hello, world!");
        assert_eq!(info.finish_reason, FinishReason::Stop);
        assert_eq!(info.raw_finish_reason.as_deref(), Some("stop"));
        assert_eq!(info.steps.len(), 1);
        assert_eq!(info.total_usage.input_tokens.total, Some(3));
        assert_eq!(info.total_usage.output_tokens.total, Some(10));
    }

    #[test]
    fn workflow_agent_compat_should_call_experimental_on_start_from_constructor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_start(
            WorkflowAgentOnStartCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_experimental_on_start_from_stream_method() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_start(
                WorkflowAgentOnStartCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_start_in_correct_order()
     {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_start(
            WorkflowAgentOnStartCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_start(
                WorkflowAgentOnStartCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_pass_experimental_on_start_event_information() {
        let captured_event = Arc::new(Mutex::new(None));
        let captured_event_for_callback = Arc::clone(&captured_event);
        let runtime_context: WorkflowRuntimeContext = serde_json::from_value(json!({
            "userId": "test-user"
        }))
        .expect("runtime context");
        let generation_settings = WorkflowGenerationSettings {
            temperature: Some(0.7),
            max_output_tokens: Some(500),
            ..WorkflowGenerationSettings::default()
        };
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_generation_settings(generation_settings.clone()),
        );
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_runtime_context(runtime_context.clone())
                    .with_on_start(WorkflowAgentOnStartCallback::new(move |info| {
                        *captured_event_for_callback
                            .lock()
                            .expect("captured event lock succeeds") = Some(info);
                    })),
            ),
        )
        .expect("agent stream succeeds");

        let event = captured_event
            .lock()
            .expect("captured event lock succeeds")
            .clone()
            .expect("event was captured");
        assert_eq!(event.model, model());
        assert_eq!(event.messages, user_prompt());
        assert_eq!(event.generation_settings, generation_settings);
        assert_eq!(event.runtime_context, runtime_context);
        assert!(event.tools_context.is_empty());
    }

    #[test]
    fn workflow_agent_compat_should_call_experimental_on_step_start_from_constructor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_step_start(
            WorkflowAgentOnStepStartCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_experimental_on_step_start_from_stream_method() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_step_start(
                WorkflowAgentOnStepStartCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_step_start_in_correct_order()
     {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_step_start(
            WorkflowAgentOnStepStartCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_step_start(
                WorkflowAgentOnStepStartCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_pass_experimental_on_step_start_event_information() {
        let captured_event = Arc::new(Mutex::new(None));
        let captured_event_for_callback = Arc::clone(&captured_event);
        let runtime_context: WorkflowRuntimeContext = serde_json::from_value(json!({
            "userId": "test-user"
        }))
        .expect("runtime context");
        let generation_settings = WorkflowGenerationSettings {
            temperature: Some(0.7),
            ..WorkflowGenerationSettings::default()
        };
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_generation_settings(generation_settings.clone()),
        );
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_runtime_context(runtime_context.clone())
                    .with_on_step_start(WorkflowAgentOnStepStartCallback::new(move |info| {
                        *captured_event_for_callback
                            .lock()
                            .expect("captured event lock succeeds") = Some(info);
                    })),
            ),
        )
        .expect("agent stream succeeds");

        let event = captured_event
            .lock()
            .expect("captured event lock succeeds")
            .clone()
            .expect("event was captured");
        assert_eq!(event.model, model());
        assert_eq!(event.step_number, 0);
        assert!(event.steps.is_empty());
        assert_eq!(event.messages, user_prompt());
        assert_eq!(event.generation_settings, generation_settings);
        assert_eq!(event.runtime_context, runtime_context);
        assert!(event.tools_context.is_empty());
    }

    #[test]
    fn workflow_agent_compat_should_call_on_step_finish_from_constructor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_step_finish(
            WorkflowAgentOnStepFinishCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_on_step_finish_from_stream_method() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_step_finish(
                WorkflowAgentOnStepFinishCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_both_constructor_and_method_on_step_finish_in_correct_order()
     {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_step_finish(
            WorkflowAgentOnStepFinishCallback::new(move |_| {
                calls_for_constructor
                    .lock()
                    .expect("calls lock succeeds")
                    .push("constructor".to_string());
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_step_finish(
                WorkflowAgentOnStepFinishCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_pass_step_result_to_on_step_finish_callback() {
        let captured_step = Arc::new(Mutex::new(None));
        let captured_step_for_callback = Arc::clone(&captured_step);
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "testProvider": {
                "testKey": "testValue"
            }
        }))
        .expect("provider metadata");
        let step = output_from_parts(
            [
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1", "Hello, ",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(
                    LanguageModelStreamFinish::new(
                        usage(),
                        LanguageModelFinishReason {
                            unified: FinishReason::Stop,
                            raw: Some("stop".to_string()),
                        },
                    )
                    .with_provider_metadata(provider_metadata.clone()),
                ),
            ],
            0,
        );
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([step]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_step_finish(
                WorkflowAgentOnStepFinishCallback::new(move |step| {
                    *captured_step_for_callback
                        .lock()
                        .expect("captured step lock succeeds") = Some(step);
                }),
            ),
        ))
        .expect("agent stream succeeds");

        let captured_step = captured_step
            .lock()
            .expect("captured step lock succeeds")
            .clone()
            .expect("step was captured");
        assert_eq!(captured_step.finish_reason, FinishReason::Stop);
        assert_eq!(captured_step.step_number, 0);
        assert_eq!(captured_step.text, "Hello, world!");
        assert_eq!(captured_step.usage.output_tokens.total, Some(5));
        assert_eq!(
            captured_step.provider_metadata.as_ref(),
            Some(&provider_metadata)
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_on_tool_execution_start_from_constructor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(executable_test_tool())
                .with_on_tool_execution_start(WorkflowAgentOnToolExecutionStartCallback::new(
                    move |_| {
                        calls_for_constructor
                            .lock()
                            .expect("calls lock succeeds")
                            .push("constructor".to_string());
                    },
                )),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_on_tool_execution_start_from_stream_method() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tool(executable_test_tool()),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_tool_execution_start(
                WorkflowAgentOnToolExecutionStartCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_start_in_correct_order()
     {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(executable_test_tool())
                .with_on_tool_execution_start(WorkflowAgentOnToolExecutionStartCallback::new(
                    move |_| {
                        calls_for_constructor
                            .lock()
                            .expect("calls lock succeeds")
                            .push("constructor".to_string());
                    },
                )),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_tool_execution_start(
                WorkflowAgentOnToolExecutionStartCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_pass_tool_execution_start_event_information() {
        let captured_event = Arc::new(Mutex::new(None));
        let captured_event_for_callback = Arc::clone(&captured_event);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tool(executable_test_tool()),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_tool_execution_start(
                WorkflowAgentOnToolExecutionStartCallback::new(move |info| {
                    *captured_event_for_callback
                        .lock()
                        .expect("captured event lock succeeds") = Some(info);
                }),
            ),
        ))
        .expect("agent stream succeeds");

        let event = captured_event
            .lock()
            .expect("captured event lock succeeds")
            .clone()
            .expect("event was captured");
        assert_eq!(event.tool_call.tool_name, "testTool");
        assert_eq!(event.tool_call.tool_call_id, "call-1");
        assert_eq!(event.tool_call.input, json!({ "value": "test" }));
        assert_eq!(event.messages.len(), 1);
        assert_eq!(event.tool_context, None);
    }

    #[test]
    fn workflow_agent_compat_should_call_on_tool_execution_end_from_constructor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(executable_test_tool())
                .with_on_tool_execution_end(WorkflowAgentOnToolExecutionEndCallback::new(
                    move |_| {
                        calls_for_constructor
                            .lock()
                            .expect("calls lock succeeds")
                            .push("constructor".to_string());
                    },
                )),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_on_tool_execution_end_from_stream_method() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tool(executable_test_tool()),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_tool_execution_end(
                WorkflowAgentOnToolExecutionEndCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_end_in_correct_order()
     {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_constructor = Arc::clone(&calls);
        let calls_for_method = Arc::clone(&calls);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(executable_test_tool())
                .with_on_tool_execution_end(WorkflowAgentOnToolExecutionEndCallback::new(
                    move |_| {
                        calls_for_constructor
                            .lock()
                            .expect("calls lock succeeds")
                            .push("constructor".to_string());
                    },
                )),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"test"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_tool_execution_end(
                WorkflowAgentOnToolExecutionEndCallback::new(move |_| {
                    calls_for_method
                        .lock()
                        .expect("calls lock succeeds")
                        .push("method".to_string());
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(
            *calls.lock().expect("calls lock succeeds"),
            vec!["constructor".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn workflow_agent_compat_should_pass_tool_execution_end_event_information_on_success() {
        let captured_event = Arc::new(Mutex::new(None));
        let captured_event_for_callback = Arc::clone(&captured_event);
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tool(executable_test_tool()),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"hello"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_on_tool_execution_end(
                WorkflowAgentOnToolExecutionEndCallback::new(move |info| {
                    *captured_event_for_callback
                        .lock()
                        .expect("captured event lock succeeds") = Some(info);
                }),
            ),
        ))
        .expect("agent stream succeeds");

        let event = captured_event
            .lock()
            .expect("captured event lock succeeds")
            .clone()
            .expect("event was captured");
        assert_eq!(event.tool_call.tool_name, "testTool");
        assert_eq!(event.tool_call.tool_call_id, "call-1");
        assert_eq!(event.tool_call.input, json!({ "value": "hello" }));
        assert_eq!(event.step_number, 0);
        assert!(event.success);
        assert_eq!(
            event.output,
            Some(JsonValue::String("hello-result".to_string()))
        );
        assert_eq!(event.error, None);
        assert_eq!(event.messages.len(), 1);
        assert_eq!(event.tool_context, None);
        assert!(event.duration_ms < 60_000);
    }

    #[test]
    fn workflow_agent_upstream_should_pass_step_number_to_tool_execution_start_and_use_success_union_on_end()
     {
        let start_event = Arc::new(Mutex::new(None));
        let end_event = Arc::new(Mutex::new(None));
        let start_event_for_callback = Arc::clone(&start_event);
        let end_event_for_callback = Arc::clone(&end_event);
        let tool = Tool::new("testTool", object_schema())
            .with_execute(|_, _| async { Ok(json!({ "result": "ok" })) });
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(tool)
                .with_on_tool_execution_start(WorkflowAgentOnToolExecutionStartCallback::new(
                    move |info| {
                        *start_event_for_callback
                            .lock()
                            .expect("start event lock succeeds") = Some(info);
                    },
                ))
                .with_on_tool_execution_end(WorkflowAgentOnToolExecutionEndCallback::new(
                    move |info| {
                        *end_event_for_callback
                            .lock()
                            .expect("end event lock succeeds") = Some(info);
                    },
                )),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            executable_tool_call_step(r#"{"value":"hello"}"#),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        let start = start_event
            .lock()
            .expect("start event lock succeeds")
            .clone()
            .expect("start event was captured");
        assert_eq!(start.step_number, 0);
        assert_eq!(start.tool_call.tool_call_id, "call-1");
        assert_eq!(start.tool_call.tool_name, "testTool");

        let end = end_event
            .lock()
            .expect("end event lock succeeds")
            .clone()
            .expect("end event was captured");
        assert_eq!(end.step_number, 0);
        assert_eq!(end.tool_call.tool_call_id, "call-1");
        assert_eq!(end.tool_call.tool_name, "testTool");
        assert!(end.success);
        assert_eq!(end.output, Some(json!({ "result": "ok" })));
        assert_eq!(end.error, None);
        assert!(end.duration_ms < 60_000);
    }

    #[test]
    fn workflow_agent_upstream_should_pass_success_false_in_tool_execution_end_when_tool_errors() {
        let end_event = Arc::new(Mutex::new(None));
        let end_event_for_callback = Arc::clone(&end_event);
        let tool = Tool::new("failTool", object_schema())
            .with_execute(|_, _| async { Err(ToolExecutionError::new("tool failed")) });
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(tool)
                .with_on_tool_execution_end(WorkflowAgentOnToolExecutionEndCallback::new(
                    move |info| {
                        *end_event_for_callback
                            .lock()
                            .expect("end event lock succeeds") = Some(info);
                    },
                )),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("call-1", "failTool", "{}")),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        let end = end_event
            .lock()
            .expect("end event lock succeeds")
            .clone()
            .expect("end event was captured");
        assert_eq!(end.step_number, 0);
        assert_eq!(end.tool_call.tool_name, "failTool");
        assert!(!end.success);
        assert_eq!(end.output, None);
        assert_eq!(end.error.as_deref(), Some("tool failed"));
        assert!(end.duration_ms < 60_000);
    }

    #[test]
    fn workflow_agent_upstream_should_have_empty_tool_calls_when_all_tools_complete_normally() {
        let tool = Tool::new("serverTool", object_schema())
            .with_execute(|_, _| async { Ok(json!("result")) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new(
                "server-call-id",
                "serverTool",
                "{}",
            )),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert!(result.tool_calls.is_empty());
        assert!(result.tool_results.is_empty());
    }

    #[test]
    fn workflow_agent_upstream_should_keep_invalid_tool_calls_on_error_path_without_executing() {
        let execute_calls = Arc::new(Mutex::new(0usize));
        let execute_calls_for_tool = Arc::clone(&execute_calls);
        let tool = Tool::new("testTool", object_schema()).with_execute(move |_, _| {
            let execute_calls = Arc::clone(&execute_calls_for_tool);
            async move {
                *execute_calls.lock().expect("counter lock succeeds") += 1;
                Ok(json!("should-not-run"))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let mut first = tool_call_step(LanguageModelToolCall::new(
            "invalid-call-id",
            "testTool",
            r#"{"cities":"San Francisco"}"#,
        ));
        first.tool_calls[0].invalid = Some(true);
        first.tool_calls[0].error = Some("Invalid input for tool testTool".to_string());
        let executor = ScriptedStreamTextStepExecutor::new([first, stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(*execute_calls.lock().expect("counter lock succeeds"), 0);
        let tool_message = tool_message_from_prompt(&result.messages);
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "invalid-call-id");
        assert_eq!(tool_result.tool_name, "testTool");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("Invalid input for tool testTool")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_generation_settings_from_constructor_to_stream_text_iterator()
     {
        let generation_settings = WorkflowGenerationSettings {
            temperature: Some(0.7),
            max_output_tokens: Some(1000),
            top_p: Some(0.9),
            seed: Some(42),
            ..WorkflowGenerationSettings::default()
        };
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_generation_settings(generation_settings.clone()),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .generation_settings,
            generation_settings
        );
    }

    #[test]
    fn workflow_agent_upstream_should_allow_stream_options_to_override_constructor_generation_settings()
     {
        let constructor_settings = WorkflowGenerationSettings {
            temperature: Some(0.7),
            ..WorkflowGenerationSettings::default()
        };
        let stream_settings = WorkflowGenerationSettings {
            temperature: Some(0.3),
            max_output_tokens: Some(500),
            ..WorkflowGenerationSettings::default()
        };
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_generation_settings(constructor_settings),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_generation_settings(stream_settings.clone()),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .generation_settings,
            stream_settings
        );
    }

    #[test]
    fn workflow_agent_upstream_should_use_constructor_stop_conditions_when_not_specified_in_stream()
    {
        let stop_conditions = vec![ai_sdk_rust::StopCondition::StepCount(3)];
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_stop_conditions(stop_conditions.clone()),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .stop_conditions,
            stop_conditions
        );
    }

    #[test]
    fn workflow_agent_upstream_should_allow_stream_options_to_override_constructor_stop_conditions()
    {
        let constructor_stop_conditions = vec![ai_sdk_rust::StopCondition::StepCount(1)];
        let stream_stop_conditions = vec![ai_sdk_rust::StopCondition::StepCount(2)];
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_stop_conditions(constructor_stop_conditions),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_stop_conditions(stream_stop_conditions.clone()),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .stop_conditions,
            stream_stop_conditions
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_tool_choice_from_constructor_to_stream_text_iterator() {
        let tool_choice = json!("required");
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tool_choice(tool_choice.clone()),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .tool_choice,
            Some(tool_choice)
        );
    }

    #[test]
    fn workflow_agent_upstream_should_allow_stream_options_to_override_constructor_tool_choice() {
        let agent =
            WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool_choice(json!("auto")));
        let stream_tool_choice = json!("none");
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_tool_choice(stream_tool_choice.clone()),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .tool_choice,
            Some(stream_tool_choice)
        );
    }

    #[test]
    fn workflow_agent_upstream_should_use_constructor_experimental_repair_tool_call_when_not_specified_in_stream()
     {
        let repair = WorkflowToolCallRepairCallback::new(|_| async { Ok(None) });
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_experimental_repair_tool_call(repair.clone()),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .repair_tool_call,
            Some(repair)
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_experimental_repair_tool_call_to_stream_text_iterator() {
        let repair = WorkflowToolCallRepairCallback::new(|_| async { Ok(None) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_experimental_repair_tool_call(repair.clone()),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .repair_tool_call,
            Some(repair)
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_on_error_callback_to_stream_text_iterator() {
        let on_error = WorkflowStreamTextOnErrorCallback::new(|_| {});
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_on_error(on_error.clone()),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            calls.lock().expect("calls lock succeeds")[0]
                .options
                .on_error,
            Some(on_error)
        );
    }

    #[test]
    fn workflow_agent_upstream_should_filter_tools_when_active_tools_is_specified() {
        let tools = [
            Tool::new("tool1", object_schema()),
            Tool::new("tool2", object_schema()),
            Tool::new("tool3", object_schema()),
        ];
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tools(tools));
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_active_tools(["tool1".to_string(), "tool3".to_string()]),
            ),
        )
        .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        assert_eq!(calls[0].tools.len(), 2);
        assert!(calls[0].tools.contains_key("tool1"));
        assert!(calls[0].tools.contains_key("tool3"));
        assert!(!calls[0].tools.contains_key("tool2"));
    }

    #[test]
    fn workflow_agent_upstream_should_use_constructor_active_tools_when_not_specified_in_stream() {
        let tools = [
            Tool::new("tool1", object_schema()),
            Tool::new("tool2", object_schema()),
        ];
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tools(tools)
                .with_active_tools(["tool1".to_string()]),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        assert_eq!(calls[0].tools.len(), 1);
        assert!(calls[0].tools.contains_key("tool1"));
        assert!(!calls[0].tools.contains_key("tool2"));
    }

    #[test]
    fn workflow_agent_upstream_should_pass_conversation_messages_to_tool_execute_function() {
        let received_messages = Arc::new(Mutex::new(None));
        let received_tool_call_id = Arc::new(Mutex::new(None));
        let received_messages_for_tool = Arc::clone(&received_messages);
        let received_tool_call_id_for_tool = Arc::clone(&received_tool_call_id);
        let tool = Tool::new("testTool", object_schema()).with_execute(move |_, options| {
            let received_messages = Arc::clone(&received_messages_for_tool);
            let received_tool_call_id = Arc::clone(&received_tool_call_id_for_tool);
            async move {
                *received_messages.lock().expect("messages lock succeeds") = Some(options.messages);
                *received_tool_call_id
                    .lock()
                    .expect("tool call id lock succeeds") = Some(options.tool_call_id);
                Ok(json!({ "result": "success" }))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new(
                "test-call-id",
                "testTool",
                r#"{"query":"weather"}"#,
            )),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *received_tool_call_id
                .lock()
                .expect("tool call id lock succeeds"),
            Some("test-call-id".to_string())
        );
        let messages = received_messages
            .lock()
            .expect("messages lock succeeds")
            .clone()
            .expect("messages were passed");
        assert!(matches!(
            messages.last(),
            Some(LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage { .. }
            ))
        ));
        let LanguageModelMessage::Assistant(assistant_message) =
            messages.last().expect("assistant message exists")
        else {
            unreachable!("last message checked above");
        };
        assert!(matches!(
            assistant_message.content.first(),
            Some(LanguageModelAssistantContentPart::ToolCall(tool_call))
                if tool_call.tool_call_id == "test-call-id"
        ));
    }

    #[test]
    fn workflow_agent_upstream_should_pass_through_messages_without_approval_responses_unchanged() {
        let prompt = user_prompt();
        let captured_messages = Arc::new(Mutex::new(None::<WorkflowPrompt>));
        let captured_messages_for_prepare_step = Arc::clone(&captured_messages);
        let tool = Tool::new("getWeather", object_schema())
            .with_execute(|_, _| async { Ok(json!({ "temperature": 72 })) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        let result = poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(prompt.clone(), executor).with_prepare_step(
                WorkflowPrepareStepCallback::new(move |info| {
                    *captured_messages_for_prepare_step
                        .lock()
                        .expect("prepare step capture lock succeeds") = Some(info.messages.clone());
                    WorkflowPrepareStepResult::default()
                }),
            ),
        ))
        .expect("agent stream succeeds");

        let captured_messages = captured_messages
            .lock()
            .expect("prepare step capture lock succeeds")
            .clone()
            .expect("prepare step was called");
        assert_eq!(captured_messages, prompt);
        assert_eq!(result.messages, prompt);
    }

    #[test]
    fn workflow_agent_upstream_should_pause_when_tool_needs_approval() {
        let executions = Arc::new(Mutex::new(0_usize));
        let executions_for_tool = Arc::clone(&executions);
        let tool = Tool::new("testTool", object_schema())
            .with_execute(move |_, _| {
                let executions_for_tool = Arc::clone(&executions_for_tool);
                async move {
                    *executions_for_tool
                        .lock()
                        .expect("execution count lock succeeds") += 1;
                    Ok(json!("approved-result"))
                }
            })
            .with_needs_approval(true);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("test-call-id", "testTool", r#"{"value":"test"}"#),
        )]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(
            *executions.lock().expect("execution count lock succeeds"),
            0
        );
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "testTool");
        assert!(result.tool_results.is_empty());
    }

    #[test]
    fn workflow_agent_upstream_should_support_needs_approval_as_a_function() {
        let approval_input = Arc::new(Mutex::new(None::<JsonValue>));
        let approval_input_for_tool = Arc::clone(&approval_input);
        let executions = Arc::new(Mutex::new(0_usize));
        let executions_for_tool = Arc::clone(&executions);
        let tool = Tool::new("testTool", object_schema())
            .with_execute(move |_, _| {
                let executions_for_tool = Arc::clone(&executions_for_tool);
                async move {
                    *executions_for_tool
                        .lock()
                        .expect("execution count lock succeeds") += 1;
                    Ok(json!("approved-result"))
                }
            })
            .with_needs_approval_function(move |input, _options| {
                let approval_input_for_tool = Arc::clone(&approval_input_for_tool);
                async move {
                    *approval_input_for_tool
                        .lock()
                        .expect("approval input lock succeeds") = Some(input);
                    true
                }
            });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("test-call-id", "testTool", r#"{"value":"test"}"#),
        )]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(
            *approval_input.lock().expect("approval input lock succeeds"),
            Some(json!({ "value": "test" }))
        );
        assert_eq!(
            *executions.lock().expect("execution count lock succeeds"),
            0
        );
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.tool_calls.len(), 1);
        assert!(result.tool_results.is_empty());
    }

    #[test]
    fn workflow_agent_upstream_should_execute_approved_tools_before_streaming() {
        let executions = Arc::new(Mutex::new(0_usize));
        let executions_for_tool = Arc::clone(&executions);
        let tool = Tool::new("testTool", object_schema())
            .with_execute(move |_, _| {
                let executions_for_tool = Arc::clone(&executions_for_tool);
                async move {
                    *executions_for_tool
                        .lock()
                        .expect("execution count lock succeeds") += 1;
                    Ok(json!("approved-result"))
                }
            })
            .with_needs_approval(true);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let prompt = vec![
            user_text_message("test"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    ai_sdk_provider::LanguageModelToolCallPart::new(
                        "call-1",
                        "testTool",
                        json!({ "value": "test" }),
                    ),
                ),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    ai_sdk_provider::LanguageModelToolApprovalRequestPart::new(
                        "approval-call-1",
                        "call-1",
                    ),
                ),
            ])),
            LanguageModelMessage::Tool(ai_sdk_provider::LanguageModelToolMessage::new(vec![
                ai_sdk_provider::LanguageModelToolContentPart::ToolApprovalResponse(
                    ai_sdk_provider::LanguageModelToolApprovalResponsePart::new(
                        "approval-call-1",
                        true,
                    ),
                ),
            ])),
        ];
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(prompt.clone(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(
            *executions.lock().expect("execution count lock succeeds"),
            1
        );
        assert_eq!(result.messages.len(), 3);
        let tool_message = tool_message_from_prompt(&result.messages);
        let tool_result = first_tool_result(tool_message);
        assert_eq!(
            tool_result.output,
            ai_sdk_provider::LanguageModelToolResultOutput::text("approved-result")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_messages_to_multiple_tools_in_parallel_execution() {
        let received_messages = Arc::new(Mutex::new(BTreeMap::<String, WorkflowPrompt>::new()));
        let received_messages_for_weather = Arc::clone(&received_messages);
        let received_messages_for_news = Arc::clone(&received_messages);

        let weather_tool =
            Tool::new("weatherTool", object_schema()).with_execute(move |_, options| {
                let received_messages = Arc::clone(&received_messages_for_weather);
                async move {
                    received_messages
                        .lock()
                        .expect("messages lock succeeds")
                        .insert("weatherTool".to_string(), options.messages);
                    Ok(json!({ "temp": 72 }))
                }
            });
        let news_tool = Tool::new("newsTool", object_schema()).with_execute(move |_, options| {
            let received_messages = Arc::clone(&received_messages_for_news);
            async move {
                received_messages
                    .lock()
                    .expect("messages lock succeeds")
                    .insert("newsTool".to_string(), options.messages);
                Ok(json!({ "headlines": ["News 1"] }))
            }
        });

        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_tools([weather_tool, news_tool]),
        );
        let conversation_messages = vec![
            user_text_message("Weather and news please"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    ai_sdk_provider::LanguageModelToolCallPart::new(
                        "weather-call",
                        "weatherTool",
                        json!({ "city": "NYC" }),
                    ),
                ),
                LanguageModelAssistantContentPart::ToolCall(
                    ai_sdk_provider::LanguageModelToolCallPart::new(
                        "news-call",
                        "newsTool",
                        json!({ "topic": "tech" }),
                    ),
                ),
            ])),
        ];
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                        "weather-call",
                        "weatherTool",
                        r#"{"city":"NYC"}"#,
                    )),
                    LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                        "news-call",
                        "newsTool",
                        r#"{"topic":"tech"}"#,
                    )),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(
            vec![user_text_message("Weather and news please")],
            executor,
        )))
        .expect("agent stream succeeds");

        let received_messages = received_messages.lock().expect("messages lock succeeds");
        assert_eq!(
            received_messages.get("weatherTool"),
            Some(&conversation_messages)
        );
        assert_eq!(
            received_messages.get("newsTool"),
            Some(&conversation_messages)
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_updated_messages_on_subsequent_tool_call_rounds() {
        let messages_per_round = Arc::new(Mutex::new(Vec::<WorkflowPrompt>::new()));
        let messages_per_round_for_tool = Arc::clone(&messages_per_round);

        let tool = Tool::new("searchTool", object_schema()).with_execute(move |_, options| {
            let messages_per_round = Arc::clone(&messages_per_round_for_tool);
            async move {
                messages_per_round
                    .lock()
                    .expect("messages lock succeeds")
                    .push(options.messages);
                Ok(json!({ "found": true }))
            }
        });

        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let first_round_messages = vec![
            user_text_message("Search for cats"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    ai_sdk_provider::LanguageModelToolCallPart::new(
                        "search-1",
                        "searchTool",
                        json!({ "query": "cats" }),
                    ),
                ),
            ])),
        ];
        let second_round_messages = vec![
            user_text_message("Search for cats"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    ai_sdk_provider::LanguageModelToolCallPart::new(
                        "search-1",
                        "searchTool",
                        json!({ "query": "cats" }),
                    ),
                ),
            ])),
            LanguageModelMessage::Tool(ai_sdk_provider::LanguageModelToolMessage::new(vec![
                ai_sdk_provider::LanguageModelToolContentPart::ToolResult(
                    LanguageModelToolResultPart::new(
                        "search-1",
                        "searchTool",
                        LanguageModelToolResultOutput::json(json!({ "found": true })),
                    ),
                ),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    ai_sdk_provider::LanguageModelToolCallPart::new(
                        "search-2",
                        "searchTool",
                        json!({ "query": "dogs" }),
                    ),
                ),
            ])),
        ];
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new(
                "search-1",
                "searchTool",
                r#"{"query":"cats"}"#,
            )),
            tool_call_step(LanguageModelToolCall::new(
                "search-2",
                "searchTool",
                r#"{"query":"dogs"}"#,
            )),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(
            vec![user_text_message("Search for cats")],
            executor,
        )))
        .expect("agent stream succeeds");

        let messages_per_round = messages_per_round.lock().expect("messages lock succeeds");
        assert_eq!(
            *messages_per_round,
            vec![first_round_messages, second_round_messages]
        );
    }

    #[test]
    fn workflow_agent_upstream_should_return_messages_and_steps_in_result() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                finish(FinishReason::Stop),
            ],
            0,
        )]);
        let expected_messages = vec![
            user_text_message("test"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(
                    ai_sdk_provider::LanguageModelTextPart::new("Hello"),
                ),
            ])),
        ];

        let result = poll_ready(agent.stream(WorkflowAgentStreamOptions::new(
            vec![user_text_message("test")],
            executor,
        )))
        .expect("agent stream succeeds");

        assert_eq!(result.messages, expected_messages);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].text, "Hello");
    }

    #[test]
    fn workflow_agent_upstream_should_generate_basic_text_response() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Echo: hello world",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                finish(FinishReason::Stop),
            ],
            0,
        )]);

        let result = poll_ready(agent.stream(WorkflowAgentStreamOptions::new(
            vec![user_text_message("hello world")],
            executor,
        )))
        .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].text, "Echo: hello world");
        assert_eq!(
            result.messages,
            vec![
                user_text_message("hello world"),
                LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(
                        ai_sdk_provider::LanguageModelTextPart::new("Echo: hello world"),
                    ),
                ])),
            ]
        );
    }

    #[test]
    fn workflow_agent_upstream_should_complete_within_timeout() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "fast response",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                finish(FinishReason::Stop),
            ],
            0,
        )]);

        let result =
            poll_ready(agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor).with_timeout(30_000),
            ))
            .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].text, "fast response");
    }

    #[test]
    fn workflow_agent_upstream_should_accept_a_string_prompt_in_stream() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(
            "What is the weather?",
            executor,
        )))
        .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].prompt,
            vec![user_text_message("What is the weather?")]
        );
    }

    #[test]
    fn workflow_agent_upstream_should_accept_an_array_of_messages_as_prompt() {
        let prompt = vec![
            user_text_message("What is the weather?"),
            user_text_message("Please be concise."),
        ];
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(prompt.clone(), executor)))
            .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].prompt, prompt);
    }

    #[test]
    fn workflow_agent_upstream_should_pass_string_instructions_to_the_model() {
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_instructions("You are a pirate."),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new("ahoy", executor)))
            .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].prompt,
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new("You are a pirate.")),
                user_text_message("ahoy"),
            ]
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_include_raw_chunks_to_stream_text_iterator() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_include_raw_chunks(true),
        ))
        .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        assert_eq!(calls.len(), 1);
        assert!(calls[0].options.include_raw_chunks);
    }

    #[test]
    fn workflow_agent_upstream_should_pass_telemetry_settings_from_constructor_to_stream_text_iterator()
     {
        let telemetry = ai_sdk_rust::TelemetryOptions::new()
            .with_enabled(true)
            .with_record_inputs(false)
            .with_function_id("test-agent");
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_telemetry(telemetry.clone()),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        let captured_telemetry = calls[0]
            .options
            .telemetry
            .as_ref()
            .expect("telemetry settings were passed");
        assert_eq!(captured_telemetry.is_enabled, Some(true));
        assert_eq!(captured_telemetry.record_inputs, Some(false));
        assert_eq!(
            captured_telemetry.function_id.as_deref(),
            Some("test-agent")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_allow_stream_options_to_override_constructor_telemetry() {
        let constructor_telemetry = ai_sdk_rust::TelemetryOptions::new()
            .with_enabled(true)
            .with_function_id("constructor-id");
        let stream_telemetry = ai_sdk_rust::TelemetryOptions::new()
            .with_enabled(false)
            .with_record_outputs(true)
            .with_function_id("stream-id");
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model()).with_telemetry(constructor_telemetry),
        );
        let (executor, calls) = RecordingStreamTextStepExecutor::new([stop_step()]);

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_telemetry(stream_telemetry.clone()),
            ),
        )
        .expect("agent stream succeeds");

        let calls = calls.lock().expect("calls lock succeeds");
        let captured_telemetry = calls[0]
            .options
            .telemetry
            .as_ref()
            .expect("telemetry settings were passed");
        assert_eq!(captured_telemetry.is_enabled, Some(false));
        assert_eq!(captured_telemetry.record_outputs, Some(true));
        assert_eq!(captured_telemetry.function_id.as_deref(), Some("stream-id"));
    }

    #[test]
    fn workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context() {
        let received_context = Arc::new(Mutex::new(None));
        let received_context_for_tool = Arc::clone(&received_context);
        let tool = Tool::new("weather", object_schema()).with_execute(move |_, options| {
            let received_context = Arc::clone(&received_context_for_tool);
            async move {
                *received_context.lock().expect("context lock succeeds") = options.context;
                Ok(json!({ "result": "ok" }))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("call-1", "weather", "{}")),
            stop_step(),
        ]);
        let mut tools_context = WorkflowToolsContext::new();
        tools_context.insert(
            "weather".to_string(),
            Some(
                serde_json::from_value(json!({
                    "weatherApiKey": "secret-key",
                    "defaultUnit": "celsius"
                }))
                .expect("context is an object"),
            ),
        );

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_tools_context(tools_context),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            *received_context.lock().expect("context lock succeeds"),
            Some(json!({
                "weatherApiKey": "secret-key",
                "defaultUnit": "celsius"
            }))
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_undefined_context_when_no_tools_context_entry_exists() {
        let received_context = Arc::new(Mutex::new(Some(json!({"unexpected": true}))));
        let received_context_for_tool = Arc::clone(&received_context);
        let tool = Tool::new("weather", object_schema()).with_execute(move |_, options| {
            let received_context = Arc::clone(&received_context_for_tool);
            async move {
                *received_context.lock().expect("context lock succeeds") = options.context;
                Ok(json!({ "result": "ok" }))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("call-1", "weather", "{}")),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *received_context.lock().expect("context lock succeeds"),
            None
        );
    }

    #[test]
    fn workflow_agent_upstream_should_pass_runtime_context_to_on_finish() {
        let captured_finish = Arc::new(Mutex::new(None));
        let captured_finish_for_callback = Arc::clone(&captured_finish);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_on_finish(
            WorkflowAgentOnFinishCallback::new(move |info| {
                *captured_finish_for_callback
                    .lock()
                    .expect("finish lock succeeds") = Some(info);
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);
        let runtime_context: WorkflowRuntimeContext = serde_json::from_value(json!({
            "tenantId": "tenant_123"
        }))
        .expect("runtime context");
        let tools_context = {
            let mut context = WorkflowToolsContext::new();
            context.insert(
                "weather".to_string(),
                Some(
                    serde_json::from_value(json!({
                        "unit": "fahrenheit"
                    }))
                    .expect("tools context"),
                ),
            );
            context
        };

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_runtime_context(runtime_context.clone())
                    .with_tools_context(tools_context.clone()),
            ),
        )
        .expect("agent stream succeeds");

        let captured_finish = captured_finish
            .lock()
            .expect("finish lock succeeds")
            .clone()
            .expect("finish info was captured");
        assert_eq!(captured_finish.runtime_context, runtime_context);
        assert_eq!(captured_finish.tools_context, tools_context);
    }

    #[test]
    fn workflow_agent_upstream_should_flow_through_runtime_context_and_tools_context_e2e() {
        let received_context = Arc::new(Mutex::new(None));
        let received_context_for_tool = Arc::clone(&received_context);
        let captured_finish = Arc::new(Mutex::new(None));
        let captured_finish_for_callback = Arc::clone(&captured_finish);
        let on_finish_runtime_context = Arc::new(Mutex::new(None));
        let on_finish_runtime_context_for_callback = Arc::clone(&on_finish_runtime_context);
        let on_finish_tools_context = Arc::new(Mutex::new(None));
        let on_finish_tools_context_for_callback = Arc::clone(&on_finish_tools_context);

        let tool = Tool::new("lookupCustomer", object_schema())
            .with_context_schema(Schema::new(object_schema()).with_validator(|value| {
                let api_key = value.get("apiKey").and_then(JsonValue::as_str);
                let region = value.get("region").and_then(JsonValue::as_str);
                if api_key.is_some() && matches!(region, Some("us" | "eu")) {
                    ValidationResult::success(value.clone())
                } else {
                    ValidationResult::failure("apiKey and region are required")
                }
            }))
            .with_execute(move |input, options| {
                let received_context = Arc::clone(&received_context_for_tool);
                async move {
                    *received_context.lock().expect("context lock succeeds") =
                        options.context.clone();
                    Ok(json!({
                        "customerId": input["customerId"],
                        "eligible": true
                    }))
                }
            });

        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(model())
                .with_tool(tool)
                .with_prepare_step(WorkflowPrepareStepCallback::new(
                    |info: crate::WorkflowPrepareStepInfo| WorkflowPrepareStepResult {
                        runtime_context: Some(
                            serde_json::from_value(json!({
                                "tenantId": info.runtime_context["tenantId"],
                                "requestId": info.runtime_context["requestId"],
                                "lastStep": info.step_number,
                            }))
                            .expect("runtime context serializes"),
                        ),
                        ..WorkflowPrepareStepResult::default()
                    },
                ))
                .with_on_finish(WorkflowAgentOnFinishCallback::new(move |info| {
                    *captured_finish_for_callback
                        .lock()
                        .expect("finish lock succeeds") = Some(info.clone());
                    *on_finish_runtime_context_for_callback
                        .lock()
                        .expect("runtime context lock succeeds") =
                        Some(info.runtime_context.clone());
                    *on_finish_tools_context_for_callback
                        .lock()
                        .expect("tools context lock succeeds") = Some(info.tools_context.clone());
                })),
        );

        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new(
                "call-1",
                "lookupCustomer",
                r#"{"customerId":"cust_123"}"#,
            )),
            output_from_parts(
                [
                    LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                        "text-1",
                        "Customer cust_123 is eligible.",
                    )),
                    LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                    finish(FinishReason::Stop),
                ],
                1,
            ),
        ]);
        let runtime_context: WorkflowRuntimeContext = serde_json::from_value(json!({
            "tenantId": "tenant_123",
            "requestId": "req_abc"
        }))
        .expect("runtime context");
        let tools_context = {
            let mut context = WorkflowToolsContext::new();
            context.insert(
                "lookupCustomer".to_string(),
                Some(
                    serde_json::from_value(json!({
                        "apiKey": "sk-test-key",
                        "region": "us"
                    }))
                    .expect("tools context"),
                ),
            );
            context
        };

        let result = poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_runtime_context(runtime_context.clone())
                    .with_tools_context(tools_context.clone()),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 2);
        assert_eq!(
            result.steps.last().expect("last step").text,
            "Customer cust_123 is eligible."
        );
        assert_eq!(
            *received_context.lock().expect("context lock succeeds"),
            Some(json!({
                "apiKey": "sk-test-key",
                "region": "us"
            }))
        );
        let captured_finish = captured_finish
            .lock()
            .expect("finish lock succeeds")
            .clone()
            .expect("finish info was captured");
        let on_finish_runtime_context = on_finish_runtime_context
            .lock()
            .expect("runtime context lock succeeds")
            .clone()
            .expect("runtime context captured");
        let on_finish_tools_context = on_finish_tools_context
            .lock()
            .expect("tools context lock succeeds")
            .clone()
            .expect("tools context captured");
        let expected_final_runtime_context = serde_json::from_value(json!({
            "tenantId": "tenant_123",
            "requestId": "req_abc",
            "lastStep": 1,
        }))
        .expect("expected final runtime context");
        assert_eq!(on_finish_runtime_context, expected_final_runtime_context);
        assert_eq!(on_finish_tools_context, tools_context);
        assert_eq!(
            captured_finish.runtime_context,
            expected_final_runtime_context
        );
        assert_eq!(captured_finish.tools_context, tools_context);
    }

    #[test]
    fn workflow_agent_upstream_should_call_on_abort_when_abort_signal_is_already_aborted() {
        let captured_abort = Arc::new(Mutex::new(None));
        let captured_abort_for_callback = Arc::clone(&captured_abort);
        let abort_controller = LanguageModelAbortController::new();
        abort_controller.abort();
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        let result = poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_abort_signal(abort_controller.signal())
                    .with_on_abort(WorkflowAgentOnAbortCallback::new(move |info| {
                        *captured_abort_for_callback
                            .lock()
                            .expect("abort lock succeeds") = Some(info);
                    })),
            ),
        )
        .expect("agent stream succeeds");

        let captured_abort = captured_abort
            .lock()
            .expect("abort lock succeeds")
            .clone()
            .expect("abort info was captured");
        assert!(captured_abort.steps.is_empty());
        assert_eq!(result.messages, user_prompt());
        assert!(result.steps.is_empty());
    }

    #[test]
    fn workflow_agent_upstream_should_pass_prepare_step_callback_to_stream_text_iterator() {
        let injected_message = user_text_message("injected message");
        let prepare_injected_message = injected_message.clone();
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        let result = poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_prepare_step(
                WorkflowPrepareStepCallback::new(move |info| {
                    let mut messages = info.messages;
                    messages.push(prepare_injected_message.clone());
                    WorkflowPrepareStepResult::default().with_messages(messages)
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert!(result.messages.contains(&injected_message));
    }

    #[test]
    fn workflow_agent_upstream_should_provide_step_information_to_prepare_step_callback() {
        let prepare_step_calls = Arc::new(Mutex::new(
            None::<(WorkflowModelInfo, usize, usize, WorkflowPrompt)>,
        ));
        let captured_prepare_step_calls = Arc::clone(&prepare_step_calls);
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);

        let result = poll_ready(agent.stream(
            WorkflowAgentStreamOptions::new(user_prompt(), executor).with_prepare_step(
                WorkflowPrepareStepCallback::new(move |info| {
                    *captured_prepare_step_calls
                        .lock()
                        .expect("prepare step capture lock succeeds") = Some((
                        info.model.clone(),
                        info.step_number,
                        info.steps.len(),
                        info.messages.clone(),
                    ));
                    WorkflowPrepareStepResult::default()
                }),
            ),
        ))
        .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 1);
        let (captured_model, captured_step_number, captured_step_count, captured_messages) =
            prepare_step_calls
                .lock()
                .expect("prepare step capture lock succeeds")
                .clone()
                .expect("prepareStep was called");
        assert_eq!(captured_model, model());
        assert_eq!(captured_step_number, 0);
        assert_eq!(captured_step_count, 0);
        assert_eq!(captured_messages, user_prompt());
    }

    #[test]
    fn workflow_agent_upstream_prepare_step_updates_runtime_context_for_agent_loop() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_prepare_step(
            WorkflowPrepareStepCallback::new(|info| {
                let mut runtime_context = info.runtime_context;
                runtime_context.insert("lastStep".to_string(), json!(info.step_number));
                WorkflowPrepareStepResult::default().with_runtime_context(runtime_context)
            }),
        ));
        let executor = ScriptedStreamTextStepExecutor::new([stop_step()]);
        let runtime_context: WorkflowRuntimeContext = serde_json::from_value(json!({
            "tenantId": "tenant_123"
        }))
        .expect("runtime context is an object");

        let result = poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_runtime_context(runtime_context),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(result.runtime_context["tenantId"], json!("tenant_123"));
        assert_eq!(result.runtime_context["lastStep"], json!(0));
    }

    #[test]
    fn workflow_agent_upstream_should_validate_per_tool_context_against_context_schema() {
        let schema = Schema::new(object_schema()).with_validator(|value| {
            if value.get("apiKey").and_then(JsonValue::as_str).is_some() {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("apiKey is required")
            }
        });
        let tool = Tool::new("weather", object_schema())
            .with_context_schema(schema)
            .with_execute(|_, _| async { Ok(json!({ "result": "ok" })) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("call-1", "weather", "{}"),
        )]);

        let error =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect_err("missing tool context fails");

        assert_eq!(
            error,
            WorkflowAgentError::InvalidToolContext {
                tool_name: "weather".to_string(),
                message: "apiKey is required".to_string()
            }
        );
    }
}
