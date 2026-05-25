use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_provider::json::{JsonObject, JsonValue};
use ai_sdk_provider::{
    FinishReason, LanguageModelAssistantContentPart, LanguageModelAssistantMessage,
    LanguageModelFinishReason, LanguageModelMessage, LanguageModelResponseMetadata,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelSystemMessage,
    LanguageModelTextPart, LanguageModelToolCall, LanguageModelToolCallPart,
    LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultPart,
    LanguageModelUsage, ProviderMetadata, ProviderOptions, Warning,
};
use ai_sdk_rust::{StopCondition, TelemetryOptions, ToolCallRepairFunction, ToolCallRepairOptions};
use serde::{Deserialize, Serialize};

use crate::{SerializableToolSet, serialize_tool_set};

/// Runtime context shared across a workflow agent loop.
pub type WorkflowRuntimeContext = JsonObject;

/// Per-tool context shared across a workflow agent loop.
pub type WorkflowToolsContext = BTreeMap<String, Option<JsonObject>>;

/// A standardized model prompt used by the workflow stream-text iterator.
pub type WorkflowPrompt = Vec<LanguageModelMessage>;

/// Model identity recorded on a workflow stream step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowModelInfo {
    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model id.
    pub model_id: String,
}

impl WorkflowModelInfo {
    /// Creates workflow model information.
    pub fn new(provider: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
        }
    }
}

impl Default for WorkflowModelInfo {
    fn default() -> Self {
        Self::new("unknown", "unknown")
    }
}

/// Provider-executed tool result captured from a workflow stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExecutedToolResult {
    /// Matching tool call id.
    pub tool_call_id: String,

    /// Name of the tool that produced the result.
    pub tool_name: String,

    /// Provider-produced result or error payload.
    pub result: JsonValue,

    /// Whether this result represents an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Tool call parsed from a workflow model stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedToolCall {
    /// Matching stream part type.
    #[serde(rename = "type")]
    pub kind: String,

    /// Unique model tool call id.
    pub tool_call_id: String,

    /// Tool name requested by the model.
    pub tool_name: String,

    /// Parsed JSON input, or the raw input string when parsing fails.
    pub input: JsonValue,

    /// Whether the provider executes the tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Provider metadata emitted with the tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Whether the tool was dynamic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Whether the tool call input was invalid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid: Option<bool>,

    /// Error message for invalid tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ParsedToolCall {
    /// Converts a provider stream tool-call part into a workflow parsed tool call.
    pub fn from_language_model_tool_call(tool_call: &LanguageModelToolCall) -> Self {
        let (input, invalid, error) = match serde_json::from_str::<JsonValue>(&tool_call.input) {
            Ok(input) => (input, None, None),
            Err(error) => (
                JsonValue::String(tool_call.input.clone()),
                Some(true),
                Some(format!(
                    "Tool call '{}' did not contain valid JSON input: {error}",
                    tool_call.tool_name
                )),
            ),
        };

        Self {
            kind: "tool-call".to_string(),
            tool_call_id: tool_call.tool_call_id.clone(),
            tool_name: tool_call.tool_name.clone(),
            input,
            provider_executed: tool_call.provider_executed,
            provider_metadata: tool_call.provider_metadata.clone(),
            dynamic: tool_call.dynamic,
            invalid,
            error,
        }
    }

    fn is_valid(&self) -> bool {
        self.invalid != Some(true)
    }
}

/// Finish metadata captured from a workflow stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamFinish {
    /// Unified and raw finish reason.
    pub finish_reason: LanguageModelFinishReason,

    /// Usage information reported by the model call.
    pub usage: LanguageModelUsage,

    /// Provider metadata emitted with the finish part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Content collected for one workflow stream step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WorkflowStreamStepContent {
    /// Generated text content.
    Text { text: String },

    /// Generated tool call content.
    ToolCall(ParsedToolCall),
}

/// Per-step result collected by the workflow stream-text iterator.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStreamStep {
    /// Stable call id used by upstream workflow telemetry.
    pub call_id: String,

    /// Zero-based workflow step number.
    pub step_number: usize,

    /// Model identity for the step.
    pub model: WorkflowModelInfo,

    /// Runtime context used for this step.
    #[serde(default)]
    pub runtime_context: WorkflowRuntimeContext,

    /// Tool contexts used for this step.
    #[serde(default)]
    pub tools_context: WorkflowToolsContext,

    /// Generated content for this step.
    pub content: Vec<WorkflowStreamStepContent>,

    /// Generated text.
    pub text: String,

    /// Reasoning deltas collected for this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning: Vec<String>,

    /// Concatenated reasoning text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,

    /// Parsed valid tool calls for this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ParsedToolCall>,

    /// Unified finish reason for this step.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Usage reported by the model call.
    pub usage: LanguageModelUsage,

    /// Warnings emitted at stream start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,

    /// Provider response metadata.
    pub response: LanguageModelResponseMetadata,

    /// Provider metadata emitted with the finish part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Options passed to a workflow stream step executor.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoStreamStepOptions {
    /// Current model identity for this workflow step.
    #[serde(default)]
    pub model: WorkflowModelInfo,

    /// Generation settings that survive the workflow step boundary.
    pub generation_settings: WorkflowGenerationSettings,

    /// Serialized tool choice representation, if one is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<JsonValue>,

    /// Whether raw provider chunks should be included.
    #[serde(default)]
    pub include_raw_chunks: bool,

    /// Serialized response format, if one is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<JsonValue>,

    /// Stop conditions carried through to the workflow iterator.
    #[serde(skip, default)]
    pub stop_conditions: Vec<StopCondition>,

    /// Tool-call repair callback carried through to the workflow iterator.
    #[serde(skip, default)]
    pub repair_tool_call: Option<WorkflowToolCallRepairCallback>,

    /// Error callback carried through to the workflow iterator.
    #[serde(skip, default)]
    pub on_error: Option<WorkflowStreamTextOnErrorCallback>,

    /// Current runtime context.
    #[serde(default)]
    pub runtime_context: WorkflowRuntimeContext,

    /// Current per-tool context.
    #[serde(default)]
    pub tools_context: WorkflowToolsContext,

    /// Telemetry settings carried through to the workflow iterator.
    #[serde(skip, default)]
    pub telemetry: Option<TelemetryOptions>,

    /// Zero-based workflow step number.
    pub step_number: usize,
}

impl PartialEq for DoStreamStepOptions {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model
            && self.generation_settings == other.generation_settings
            && self.tool_choice == other.tool_choice
            && self.include_raw_chunks == other.include_raw_chunks
            && self.response_format == other.response_format
            && self.stop_conditions == other.stop_conditions
            && self.repair_tool_call == other.repair_tool_call
            && self.on_error == other.on_error
            && self.runtime_context == other.runtime_context
            && self.tools_context == other.tools_context
            && self.step_number == other.step_number
    }
}

/// Portable generation settings accepted by the workflow iterator.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowGenerationSettings {
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub stop_sequences: Option<Vec<String>>,
    pub seed: Option<u64>,
    pub max_retries: Option<u64>,
    pub headers: Option<BTreeMap<String, String>>,
    pub provider_options: Option<ProviderOptions>,
}

/// Result returned by one workflow stream step executor call.
#[derive(Clone, Debug, PartialEq)]
pub struct DoStreamStepOutput {
    /// Tool calls parsed from the model stream.
    pub tool_calls: Vec<ParsedToolCall>,

    /// Finish metadata, when the stream provided it.
    pub finish: Option<StreamFinish>,

    /// Step result collected from the stream.
    pub step: WorkflowStreamStep,

    /// Non-lifecycle chunks retained for debugging or UI conversion.
    pub chunks: Vec<LanguageModelStreamPart>,

    /// Provider-executed tool results keyed by tool-call id.
    pub provider_executed_tool_results: BTreeMap<String, ProviderExecutedToolResult>,
}

/// Executor used by [`StreamTextIterator`] to perform one model stream step.
pub trait WorkflowStreamTextStepExecutor {
    /// Executes one stream step.
    fn do_stream_step(
        &mut self,
        prompt: &[LanguageModelMessage],
        tools: &SerializableToolSet,
        options: &DoStreamStepOptions,
    ) -> Result<DoStreamStepOutput, WorkflowStreamTextError>;
}

/// Error returned by workflow stream-text iteration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowStreamTextError {
    /// Tool results were supplied when the iterator was not waiting for them.
    UnexpectedToolResults,

    /// The iterator needed tool results to continue.
    MissingToolResults,

    /// The executor had no remaining scripted step.
    MissingScriptedStep,

    /// Step execution failed.
    StepExecution(String),
}

impl fmt::Display for WorkflowStreamTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedToolResults => {
                write!(formatter, "tool results were supplied before tool calls")
            }
            Self::MissingToolResults => {
                write!(
                    formatter,
                    "workflow stream iterator is waiting for tool results"
                )
            }
            Self::MissingScriptedStep => write!(formatter, "no scripted workflow stream step left"),
            Self::StepExecution(error) => write!(formatter, "workflow stream step failed: {error}"),
        }
    }
}

impl Error for WorkflowStreamTextError {}

/// Value yielded by the workflow stream-text iterator.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamTextIteratorYieldValue {
    /// Tool calls requested by the model.
    pub tool_calls: Vec<ParsedToolCall>,

    /// Conversation messages through the current yield point.
    pub messages: WorkflowPrompt,

    /// Step result from the current model call.
    pub step: WorkflowStreamStep,

    /// Current runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Current per-tool context.
    pub tools_context: WorkflowToolsContext,

    /// Provider-executed results emitted by the model stream.
    pub provider_executed_tool_results: BTreeMap<String, ProviderExecutedToolResult>,
}

/// Callback invoked before each workflow stream-text step.
#[derive(Clone)]
pub struct WorkflowPrepareStepCallback {
    callback:
        Arc<dyn Fn(WorkflowPrepareStepInfo) -> WorkflowPrepareStepResult + Send + Sync + 'static>,
}

impl WorkflowPrepareStepCallback {
    /// Creates a prepare-step callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(WorkflowPrepareStepInfo) -> WorkflowPrepareStepResult + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    fn call(&self, info: WorkflowPrepareStepInfo) -> WorkflowPrepareStepResult {
        (self.callback)(info)
    }
}

impl fmt::Debug for WorkflowPrepareStepCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowPrepareStepCallback")
            .finish_non_exhaustive()
    }
}

/// Information passed to a workflow prepare-step callback.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowPrepareStepInfo {
    /// Current model identity.
    pub model: WorkflowModelInfo,

    /// Zero-based workflow step number.
    pub step_number: usize,

    /// Completed steps before the current step.
    pub steps: Vec<WorkflowStreamStep>,

    /// Messages that will be sent to the model.
    pub messages: WorkflowPrompt,

    /// Current runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Current per-tool context.
    pub tools_context: WorkflowToolsContext,
}

/// Prepare-step overrides for the current and subsequent workflow steps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkflowPrepareStepResult {
    /// Override the model identity for this step.
    pub model: Option<WorkflowModelInfo>,

    /// Override or prepend the system message for this step.
    pub system: Option<String>,

    /// Override the messages for this step.
    pub messages: Option<WorkflowPrompt>,

    /// Generation settings to merge into the current settings.
    pub generation_settings: WorkflowGenerationSettings,

    /// Override the active tools for this and subsequent steps.
    pub active_tools: Option<Vec<String>>,

    /// Override the serialized tool choice for this and subsequent steps.
    pub tool_choice: Option<JsonValue>,

    /// Override the runtime context for this and subsequent steps.
    pub runtime_context: Option<WorkflowRuntimeContext>,

    /// Override the per-tool context for this and subsequent steps.
    pub tools_context: Option<WorkflowToolsContext>,
}

/// Callback invoked for workflow stream-text errors.
#[derive(Clone)]
pub struct WorkflowStreamTextOnErrorCallback {
    callback: Arc<dyn Fn(String) + Send + Sync + 'static>,
}

impl WorkflowStreamTextOnErrorCallback {
    /// Creates a stream-text error callback from a synchronous Rust function.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    #[allow(dead_code)]
    fn call(&self, error: String) {
        (self.callback)(error);
    }
}

impl fmt::Debug for WorkflowStreamTextOnErrorCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowStreamTextOnErrorCallback")
            .finish_non_exhaustive()
    }
}

impl PartialEq for WorkflowStreamTextOnErrorCallback {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.callback, &other.callback)
    }
}

/// Callback invoked to repair workflow stream-text tool calls.
#[derive(Clone)]
pub struct WorkflowToolCallRepairCallback {
    repair: Arc<ToolCallRepairFunction>,
}

impl WorkflowToolCallRepairCallback {
    /// Creates a tool-call repair callback from a synchronous Rust function.
    pub fn new<F, Fut>(repair: F) -> Self
    where
        F: Fn(ToolCallRepairOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<ai_sdk_provider::LanguageModelToolCall>, String>>
            + Send
            + 'static,
    {
        Self {
            repair: Arc::new(move |options| Box::pin(repair(options))),
        }
    }

    #[allow(dead_code)]
    fn call(
        &self,
        options: ToolCallRepairOptions,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Option<ai_sdk_provider::LanguageModelToolCall>, String>>
                + Send,
        >,
    > {
        (self.repair)(options)
    }
}

impl fmt::Debug for WorkflowToolCallRepairCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkflowToolCallRepairCallback")
            .finish_non_exhaustive()
    }
}

impl PartialEq for WorkflowToolCallRepairCallback {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.repair, &other.repair)
    }
}

impl WorkflowPrepareStepResult {
    /// Sets the model override.
    pub fn with_model(mut self, model: WorkflowModelInfo) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets the system message override.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets the message override.
    pub fn with_messages(mut self, messages: WorkflowPrompt) -> Self {
        self.messages = Some(messages);
        self
    }

    /// Sets generation setting overrides.
    pub fn with_generation_settings(
        mut self,
        generation_settings: WorkflowGenerationSettings,
    ) -> Self {
        self.generation_settings = generation_settings;
        self
    }

    /// Sets active tool overrides.
    pub fn with_active_tools(mut self, active_tools: impl IntoIterator<Item = String>) -> Self {
        self.active_tools = Some(active_tools.into_iter().collect());
        self
    }

    /// Sets the tool-choice override.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<JsonValue>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }

    /// Sets the runtime context override.
    pub fn with_runtime_context(mut self, runtime_context: WorkflowRuntimeContext) -> Self {
        self.runtime_context = Some(runtime_context);
        self
    }

    /// Sets the per-tool context override.
    pub fn with_tools_context(mut self, tools_context: WorkflowToolsContext) -> Self {
        self.tools_context = Some(tools_context);
        self
    }
}

/// Deterministic Rust equivalent of upstream `streamTextIterator`.
pub struct StreamTextIterator<E> {
    executor: E,
    model: WorkflowModelInfo,
    prompt: WorkflowPrompt,
    tools: SerializableToolSet,
    generation_settings: WorkflowGenerationSettings,
    runtime_context: WorkflowRuntimeContext,
    tools_context: WorkflowToolsContext,
    active_tools: Option<Vec<String>>,
    tool_choice: Option<JsonValue>,
    stop_conditions: Vec<StopCondition>,
    repair_tool_call: Option<WorkflowToolCallRepairCallback>,
    on_error: Option<WorkflowStreamTextOnErrorCallback>,
    prepare_step: Option<WorkflowPrepareStepCallback>,
    include_raw_chunks: bool,
    telemetry: Option<TelemetryOptions>,
    response_format: Option<JsonValue>,
    steps: Vec<WorkflowStreamStep>,
    step_number: usize,
    done: bool,
    waiting_for_tool_results: bool,
}

impl<E> StreamTextIterator<E> {
    /// Creates a workflow stream-text iterator with serialized tools.
    pub fn new(prompt: WorkflowPrompt, tools: SerializableToolSet, executor: E) -> Self {
        Self {
            executor,
            model: WorkflowModelInfo::default(),
            prompt,
            tools,
            generation_settings: WorkflowGenerationSettings::default(),
            runtime_context: WorkflowRuntimeContext::new(),
            tools_context: WorkflowToolsContext::new(),
            active_tools: None,
            tool_choice: None,
            stop_conditions: Vec::new(),
            repair_tool_call: None,
            on_error: None,
            prepare_step: None,
            include_raw_chunks: false,
            telemetry: None,
            response_format: None,
            steps: Vec::new(),
            step_number: 0,
            done: false,
            waiting_for_tool_results: false,
        }
    }

    /// Creates an iterator from runtime tools by serializing them first.
    pub fn from_runtime_tools(
        prompt: WorkflowPrompt,
        tools: impl IntoIterator<Item = ai_sdk_provider_utils::Tool>,
        executor: E,
    ) -> Self {
        Self::new(prompt, serialize_tool_set(tools), executor)
    }

    /// Sets generation settings.
    pub fn with_generation_settings(
        mut self,
        generation_settings: WorkflowGenerationSettings,
    ) -> Self {
        self.generation_settings = generation_settings;
        self
    }

    /// Sets the current model identity.
    pub fn with_model(mut self, model: WorkflowModelInfo) -> Self {
        self.model = model;
        self
    }

    /// Sets current runtime context.
    pub fn with_runtime_context(mut self, runtime_context: WorkflowRuntimeContext) -> Self {
        self.runtime_context = runtime_context;
        self
    }

    /// Sets current per-tool context.
    pub fn with_tools_context(mut self, tools_context: WorkflowToolsContext) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Restricts the active tool set for subsequent steps.
    pub fn with_active_tools(mut self, active_tools: impl IntoIterator<Item = String>) -> Self {
        self.active_tools = Some(active_tools.into_iter().collect());
        self
    }

    /// Sets serialized tool choice data.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<JsonValue>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }

    /// Sets stop conditions carried through this iterator.
    pub fn with_stop_conditions(
        mut self,
        stop_conditions: impl IntoIterator<Item = StopCondition>,
    ) -> Self {
        self.stop_conditions = stop_conditions.into_iter().collect();
        self
    }

    /// Sets the tool-call repair callback.
    pub fn with_repair_tool_call(
        mut self,
        repair_tool_call: WorkflowToolCallRepairCallback,
    ) -> Self {
        self.repair_tool_call = Some(repair_tool_call);
        self
    }

    /// Sets the error callback.
    pub fn with_on_error(mut self, on_error: WorkflowStreamTextOnErrorCallback) -> Self {
        self.on_error = Some(on_error);
        self
    }

    /// Sets a prepare-step callback.
    pub fn with_prepare_step(mut self, prepare_step: WorkflowPrepareStepCallback) -> Self {
        self.prepare_step = Some(prepare_step);
        self
    }

    /// Sets whether raw chunks are included.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.include_raw_chunks = include_raw_chunks;
        self
    }

    /// Sets telemetry settings.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Sets serialized response format data.
    pub fn with_response_format(mut self, response_format: impl Into<JsonValue>) -> Self {
        self.response_format = Some(response_format.into());
        self
    }

    /// Returns the executor for test inspection.
    pub fn executor(&self) -> &E {
        &self.executor
    }

    /// Returns the final conversation prompt once iteration is complete.
    pub fn prompt(&self) -> &[LanguageModelMessage] {
        &self.prompt
    }

    /// Returns completed steps.
    pub fn steps(&self) -> &[WorkflowStreamStep] {
        &self.steps
    }

    /// Returns configured stop conditions.
    pub fn stop_conditions(&self) -> &[StopCondition] {
        &self.stop_conditions
    }

    /// Returns the configured repair callback.
    pub fn repair_tool_call(&self) -> Option<&WorkflowToolCallRepairCallback> {
        self.repair_tool_call.as_ref()
    }

    /// Returns the configured error callback.
    pub fn on_error(&self) -> Option<&WorkflowStreamTextOnErrorCallback> {
        self.on_error.as_ref()
    }

    fn apply_prepare_step(&mut self) {
        let Some(prepare_step) = self.prepare_step.clone() else {
            return;
        };

        let result = prepare_step.call(WorkflowPrepareStepInfo {
            model: self.model.clone(),
            step_number: self.step_number,
            steps: self.steps.clone(),
            messages: self.prompt.clone(),
            runtime_context: self.runtime_context.clone(),
            tools_context: self.tools_context.clone(),
        });

        if let Some(model) = result.model {
            self.model = model;
        }
        if let Some(messages) = result.messages {
            self.prompt = messages;
        }
        if let Some(system) = result.system {
            apply_system_message(&mut self.prompt, system);
        }
        if let Some(runtime_context) = result.runtime_context {
            self.runtime_context = runtime_context;
        }
        if let Some(tools_context) = result.tools_context {
            self.tools_context = tools_context;
        }
        if let Some(active_tools) = result.active_tools {
            self.active_tools = Some(active_tools);
        }
        merge_generation_settings(&mut self.generation_settings, result.generation_settings);
        if let Some(tool_choice) = result.tool_choice {
            self.tool_choice = Some(tool_choice);
        }
    }
}

impl<E: WorkflowStreamTextStepExecutor> StreamTextIterator<E> {
    /// Advances the iterator one yield point.
    ///
    /// Pass `None` for the first call. After a yield that contains tool calls,
    /// pass the matching tool-result prompt parts to continue the conversation.
    pub fn next(
        &mut self,
        tool_results: Option<Vec<LanguageModelToolResultPart>>,
    ) -> Result<Option<StreamTextIteratorYieldValue>, WorkflowStreamTextError> {
        if self.done {
            if tool_results.is_some() {
                return Err(WorkflowStreamTextError::UnexpectedToolResults);
            }
            return Ok(None);
        }

        if self.waiting_for_tool_results {
            let tool_results = tool_results.ok_or(WorkflowStreamTextError::MissingToolResults)?;
            self.prompt
                .push(LanguageModelMessage::Tool(LanguageModelToolMessage::new(
                    tool_results
                        .into_iter()
                        .map(LanguageModelToolContentPart::ToolResult)
                        .collect(),
                )));
            self.waiting_for_tool_results = false;
        } else if tool_results.is_some() {
            return Err(WorkflowStreamTextError::UnexpectedToolResults);
        }

        self.apply_prepare_step();

        let tools = self.effective_tools();
        let options = DoStreamStepOptions {
            model: self.model.clone(),
            generation_settings: self.generation_settings.clone(),
            tool_choice: self.tool_choice.clone(),
            include_raw_chunks: self.include_raw_chunks,
            response_format: self.response_format.clone(),
            stop_conditions: self.stop_conditions.clone(),
            repair_tool_call: self.repair_tool_call.clone(),
            on_error: self.on_error.clone(),
            runtime_context: self.runtime_context.clone(),
            tools_context: self.tools_context.clone(),
            telemetry: self.telemetry.clone(),
            step_number: self.step_number,
        };

        let output = self
            .executor
            .do_stream_step(&self.prompt, &tools, &options)?;
        let finish_reason = output
            .finish
            .as_ref()
            .map(|finish| finish.finish_reason.unified.clone())
            .unwrap_or_else(|| output.step.finish_reason.clone());
        let step = output.step.clone();

        self.step_number += 1;
        self.steps.push(step.clone());

        match finish_reason {
            FinishReason::ToolCalls => {
                self.prompt.push(LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(
                        output
                            .tool_calls
                            .iter()
                            .map(tool_call_prompt_part)
                            .map(LanguageModelAssistantContentPart::ToolCall)
                            .collect(),
                    ),
                ));
                self.waiting_for_tool_results = true;

                Ok(Some(StreamTextIteratorYieldValue {
                    tool_calls: output.tool_calls,
                    messages: self.prompt.clone(),
                    step,
                    runtime_context: self.runtime_context.clone(),
                    tools_context: self.tools_context.clone(),
                    provider_executed_tool_results: output.provider_executed_tool_results,
                }))
            }
            FinishReason::Stop => {
                if !step.text.is_empty() {
                    self.prompt.push(LanguageModelMessage::Assistant(
                        LanguageModelAssistantMessage::new(vec![
                            LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                                step.text.clone(),
                            )),
                        ]),
                    ));
                }
                self.done = true;

                Ok(Some(StreamTextIteratorYieldValue {
                    tool_calls: Vec::new(),
                    messages: self.prompt.clone(),
                    step,
                    runtime_context: self.runtime_context.clone(),
                    tools_context: self.tools_context.clone(),
                    provider_executed_tool_results: output.provider_executed_tool_results,
                }))
            }
            FinishReason::Length
            | FinishReason::ContentFilter
            | FinishReason::Error
            | FinishReason::Other => {
                self.done = true;

                Ok(Some(StreamTextIteratorYieldValue {
                    tool_calls: Vec::new(),
                    messages: self.prompt.clone(),
                    step,
                    runtime_context: self.runtime_context.clone(),
                    tools_context: self.tools_context.clone(),
                    provider_executed_tool_results: output.provider_executed_tool_results,
                }))
            }
        }
    }

    fn effective_tools(&self) -> SerializableToolSet {
        let Some(active_tools) = &self.active_tools else {
            return self.tools.clone();
        };
        if active_tools.is_empty() {
            return self.tools.clone();
        }

        active_tools
            .iter()
            .filter_map(|name| {
                self.tools
                    .get(name)
                    .map(|tool| (name.clone(), tool.clone()))
            })
            .collect()
    }
}

/// Executor that returns pre-collected stream steps.
#[derive(Clone, Debug, Default)]
pub struct ScriptedStreamTextStepExecutor {
    steps: VecDeque<DoStreamStepOutput>,
    calls: Vec<ScriptedStreamTextStepCall>,
}

impl ScriptedStreamTextStepExecutor {
    /// Creates a scripted executor from stream step outputs.
    pub fn new(steps: impl IntoIterator<Item = DoStreamStepOutput>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            calls: Vec::new(),
        }
    }

    /// Returns recorded executor calls.
    pub fn calls(&self) -> &[ScriptedStreamTextStepCall] {
        &self.calls
    }
}

impl WorkflowStreamTextStepExecutor for ScriptedStreamTextStepExecutor {
    fn do_stream_step(
        &mut self,
        prompt: &[LanguageModelMessage],
        tools: &SerializableToolSet,
        options: &DoStreamStepOptions,
    ) -> Result<DoStreamStepOutput, WorkflowStreamTextError> {
        self.calls.push(ScriptedStreamTextStepCall {
            prompt: prompt.to_vec(),
            tools: tools.clone(),
            options: options.clone(),
        });
        self.steps
            .pop_front()
            .ok_or(WorkflowStreamTextError::MissingScriptedStep)
    }
}

/// Recorded call made to [`ScriptedStreamTextStepExecutor`].
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptedStreamTextStepCall {
    pub prompt: WorkflowPrompt,
    pub tools: SerializableToolSet,
    pub options: DoStreamStepOptions,
}

/// Collects workflow stream parts into one step output.
pub fn do_stream_step_from_parts(
    parts: impl IntoIterator<Item = LanguageModelStreamPart>,
    options: DoStreamStepOptions,
) -> DoStreamStepOutput {
    let mut text = String::new();
    let mut reasoning = Vec::new();
    let mut chunks = Vec::new();
    let mut tool_calls = Vec::new();
    let mut provider_executed_tool_results = BTreeMap::new();
    let mut finish = None;
    let mut response_metadata = LanguageModelStreamResponseMetadata::new();
    let mut warnings = Vec::new();

    for part in parts {
        match &part {
            LanguageModelStreamPart::StreamStart(part) => {
                warnings = part.warnings.clone();
            }
            LanguageModelStreamPart::ResponseMetadata(part) => {
                response_metadata = part.clone();
            }
            LanguageModelStreamPart::TextDelta(part) => {
                text.push_str(&part.delta);
                chunks.push(LanguageModelStreamPart::TextDelta(part.clone()));
            }
            LanguageModelStreamPart::ReasoningDelta(part) => {
                reasoning.push(part.delta.clone());
                chunks.push(LanguageModelStreamPart::ReasoningDelta(part.clone()));
            }
            LanguageModelStreamPart::ToolCall(part) => {
                let parsed = ParsedToolCall::from_language_model_tool_call(part);
                tool_calls.push(parsed);
                chunks.push(LanguageModelStreamPart::ToolCall(part.clone()));
            }
            LanguageModelStreamPart::ToolResult(part) => {
                provider_executed_tool_results.insert(
                    part.tool_call_id.clone(),
                    ProviderExecutedToolResult {
                        tool_call_id: part.tool_call_id.clone(),
                        tool_name: part.tool_name.clone(),
                        result: part.result.as_value().clone(),
                        is_error: part.is_error,
                    },
                );
                chunks.push(LanguageModelStreamPart::ToolResult(part.clone()));
            }
            LanguageModelStreamPart::Finish(part) => {
                finish = Some(StreamFinish {
                    finish_reason: part.finish_reason.clone(),
                    usage: part.usage.clone(),
                    provider_metadata: part.provider_metadata.clone(),
                });
            }
            LanguageModelStreamPart::Raw(_)
            | LanguageModelStreamPart::TextStart(_)
            | LanguageModelStreamPart::TextEnd(_)
            | LanguageModelStreamPart::ReasoningStart(_)
            | LanguageModelStreamPart::ReasoningEnd(_)
            | LanguageModelStreamPart::ToolInputStart(_)
            | LanguageModelStreamPart::ToolInputDelta(_)
            | LanguageModelStreamPart::ToolInputEnd(_)
            | LanguageModelStreamPart::ToolApprovalRequest(_)
            | LanguageModelStreamPart::Custom(_)
            | LanguageModelStreamPart::File(_)
            | LanguageModelStreamPart::ReasoningFile(_)
            | LanguageModelStreamPart::Source(_)
            | LanguageModelStreamPart::Error(_) => {
                chunks.push(part.clone());
            }
        }
    }

    let finish_reason = finish
        .as_ref()
        .map(|finish| finish.finish_reason.unified.clone())
        .unwrap_or(FinishReason::Other);
    let raw_finish_reason = finish
        .as_ref()
        .and_then(|finish| finish.finish_reason.raw.clone());
    let usage = finish
        .as_ref()
        .map(|finish| finish.usage.clone())
        .unwrap_or_default();
    let provider_metadata = finish
        .as_ref()
        .and_then(|finish| finish.provider_metadata.clone());
    let model_id = response_metadata
        .model_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let provider = model_id
        .split(':')
        .next()
        .filter(|provider| !provider.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let valid_tool_calls: Vec<_> = tool_calls
        .iter()
        .filter(|tool_call| tool_call.is_valid())
        .cloned()
        .collect();
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(WorkflowStreamStepContent::Text { text: text.clone() });
    }
    content.extend(
        valid_tool_calls
            .iter()
            .cloned()
            .map(WorkflowStreamStepContent::ToolCall),
    );
    let reasoning_text = if reasoning.is_empty() {
        None
    } else {
        Some(reasoning.join(""))
    };

    DoStreamStepOutput {
        tool_calls,
        finish,
        step: WorkflowStreamStep {
            call_id: "workflow-agent".to_string(),
            step_number: options.step_number,
            model: WorkflowModelInfo::new(provider, model_id.clone()),
            runtime_context: options.runtime_context.clone(),
            tools_context: options.tools_context.clone(),
            content,
            text,
            reasoning,
            reasoning_text,
            tool_calls: valid_tool_calls,
            finish_reason,
            raw_finish_reason,
            usage,
            warnings,
            response: LanguageModelResponseMetadata {
                id: response_metadata.id,
                timestamp: response_metadata.timestamp,
                model_id: Some(model_id),
            },
            provider_metadata,
        },
        chunks,
        provider_executed_tool_results,
    }
}

fn tool_call_prompt_part(tool_call: &ParsedToolCall) -> LanguageModelToolCallPart {
    let mut part = LanguageModelToolCallPart::new(
        tool_call.tool_call_id.clone(),
        tool_call.tool_name.clone(),
        tool_call.input.clone(),
    );
    if let Some(provider_executed) = tool_call.provider_executed {
        part = part.with_provider_executed(provider_executed);
    }
    if let Some(provider_options) =
        sanitize_provider_metadata_for_tool_call(tool_call.provider_metadata.as_ref())
    {
        part = part.with_provider_options(provider_options);
    }
    part
}

/// Maps tool-call provider metadata into prompt provider options while stripping
/// OpenAI `itemId`, matching upstream workflow continuation behavior.
pub fn sanitize_provider_metadata_for_tool_call(
    metadata: Option<&ProviderMetadata>,
) -> Option<ProviderOptions> {
    let metadata = metadata?;
    let mut sanitized = ProviderOptions::new();

    for (provider, provider_metadata) in metadata {
        let mut provider_options = provider_metadata.clone();
        if provider == "openai" {
            provider_options.remove("itemId");
        }
        if !provider_options.is_empty() {
            sanitized.insert(provider.clone(), provider_options);
        }
    }

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn apply_system_message(prompt: &mut WorkflowPrompt, system: String) {
    let system_message = LanguageModelMessage::System(LanguageModelSystemMessage::new(system));
    if matches!(prompt.first(), Some(LanguageModelMessage::System(_))) {
        prompt[0] = system_message;
    } else {
        prompt.insert(0, system_message);
    }
}

fn merge_generation_settings(
    settings: &mut WorkflowGenerationSettings,
    updates: WorkflowGenerationSettings,
) {
    if updates.max_output_tokens.is_some() {
        settings.max_output_tokens = updates.max_output_tokens;
    }
    if updates.temperature.is_some() {
        settings.temperature = updates.temperature;
    }
    if updates.top_p.is_some() {
        settings.top_p = updates.top_p;
    }
    if updates.top_k.is_some() {
        settings.top_k = updates.top_k;
    }
    if updates.presence_penalty.is_some() {
        settings.presence_penalty = updates.presence_penalty;
    }
    if updates.frequency_penalty.is_some() {
        settings.frequency_penalty = updates.frequency_penalty;
    }
    if updates.stop_sequences.is_some() {
        settings.stop_sequences = updates.stop_sequences;
    }
    if updates.seed.is_some() {
        settings.seed = updates.seed;
    }
    if updates.max_retries.is_some() {
        settings.max_retries = updates.max_retries;
    }
    if updates.headers.is_some() {
        settings.headers = updates.headers;
    }
    if updates.provider_options.is_some() {
        settings.provider_options = updates.provider_options;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_provider::json::NonNullJsonValue;
    use ai_sdk_provider::{
        LanguageModelStreamFinish, LanguageModelTextDelta, LanguageModelToolResult,
        LanguageModelToolResultOutput, LanguageModelUserContentPart, LanguageModelUserMessage,
        OutputTokenUsage,
    };
    use serde_json::json;

    fn user_prompt() -> WorkflowPrompt {
        vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("test"),
            )],
        ))]
    }

    fn provider_metadata(value: JsonValue) -> ProviderMetadata {
        serde_json::from_value(value).expect("provider metadata is valid")
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

    fn finish(reason: FinishReason) -> LanguageModelStreamPart {
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            usage(),
            LanguageModelFinishReason {
                unified: reason,
                raw: None,
            },
        ))
    }

    fn tool_result(
        tool_call_id: &str,
        tool_name: &str,
        output: JsonValue,
    ) -> LanguageModelToolResultPart {
        LanguageModelToolResultPart::new(
            tool_call_id,
            tool_name,
            LanguageModelToolResultOutput::json(output),
        )
    }

    fn output_from_parts(
        parts: impl IntoIterator<Item = LanguageModelStreamPart>,
        step_number: usize,
    ) -> DoStreamStepOutput {
        do_stream_step_from_parts(
            parts,
            DoStreamStepOptions {
                step_number,
                ..DoStreamStepOptions::default()
            },
        )
    }

    fn assistant_tool_call_provider_options(
        prompt: &[LanguageModelMessage],
        tool_name: &str,
    ) -> Option<Option<ProviderOptions>> {
        prompt.iter().find_map(|message| {
            let LanguageModelMessage::Assistant(message) = message else {
                return None;
            };
            message.content.iter().find_map(|part| {
                let LanguageModelAssistantContentPart::ToolCall(part) = part else {
                    return None;
                };
                if part.tool_name == tool_name {
                    Some(part.provider_options.clone())
                } else {
                    None
                }
            })
        })
    }

    fn user_text_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    fn serializable_tool() -> crate::SerializableToolDef {
        crate::SerializableToolDef::function(
            serde_json::from_value(json!({
                "type": "object",
                "additionalProperties": true
            }))
            .expect("schema is an object"),
        )
    }

    #[test]
    fn stream_text_iterator_maps_provider_metadata_to_provider_options_for_continuation() {
        let tool_call = LanguageModelToolCall::new("call-1", "weatherTool", r#"{"city":"NYC"}"#)
            .with_provider_metadata(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_weather_123"
                }
            })));
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts(
                [
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                        "text-1", "done",
                    )),
                    finish(FinishReason::Stop),
                ],
                1,
            ),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        let first = iterator.next(None).expect("first step succeeds");
        assert!(first.is_some());
        let second = iterator
            .next(Some(vec![tool_result(
                "call-1",
                "weatherTool",
                json!({ "result": "success" }),
            )]))
            .expect("continuation succeeds")
            .expect("final yield exists");

        assert_eq!(second.step.text, "done");
        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "weatherTool"),
            Some(Some(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_weather_123"
                }
            }))))
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_preserve_provider_metadata_for_multiple_parallel_tool_calls()
     {
        let weather_tool_call =
            LanguageModelToolCall::new("call-1", "weatherTool", r#"{"city":"NYC"}"#)
                .with_provider_metadata(provider_metadata(json!({
                    "google": {
                        "thoughtSignature": "sig_weather_123"
                    }
                })));
        let lookup_tool_call =
            LanguageModelToolCall::new("call-2", "lookupTool", r#"{"id":"123"}"#)
                .with_provider_metadata(provider_metadata(json!({
                    "anthropic": {
                        "cacheControl": {
                            "type": "ephemeral"
                        }
                    }
                })));
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(weather_tool_call),
                    LanguageModelStreamPart::ToolCall(lookup_tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![
                tool_result("call-1", "weatherTool", json!({ "ok": true })),
                tool_result("call-2", "lookupTool", json!({ "ok": true })),
            ]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "weatherTool"),
            Some(Some(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_weather_123"
                }
            }))))
        );
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "lookupTool"),
            Some(Some(provider_metadata(json!({
                "anthropic": {
                    "cacheControl": {
                        "type": "ephemeral"
                    }
                }
            }))))
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_handle_mixed_tool_calls_with_and_without_provider_metadata()
     {
        let metadata_tool_call =
            LanguageModelToolCall::new("call-1", "metadataTool", r#"{"query":"test"}"#)
                .with_provider_metadata(provider_metadata(json!({
                    "google": {
                        "thoughtSignature": "sig_metadata_123"
                    }
                })));
        let plain_tool_call =
            LanguageModelToolCall::new("call-2", "plainTool", r#"{"query":"test"}"#);
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(metadata_tool_call),
                    LanguageModelStreamPart::ToolCall(plain_tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![
                tool_result("call-1", "metadataTool", json!({ "ok": true })),
                tool_result("call-2", "plainTool", json!({ "ok": true })),
            ]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "metadataTool"),
            Some(Some(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_metadata_123"
                }
            }))))
        );
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "plainTool"),
            Some(None)
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_allow_prepare_step_to_modify_messages() {
        let injected_message = user_text_message("injected message");
        let prepare_injected_message = injected_message.clone();
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [finish(FinishReason::Stop)],
            0,
        )]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor)
                .with_prepare_step(WorkflowPrepareStepCallback::new(move |info| {
                    let mut messages = info.messages;
                    messages.push(prepare_injected_message.clone());
                    WorkflowPrepareStepResult::default().with_messages(messages)
                }));

        iterator
            .next(None)
            .expect("step succeeds")
            .expect("yield exists");

        let call_prompt = &iterator.executor().calls()[0].prompt;
        assert_eq!(call_prompt.len(), 2);
        assert_eq!(call_prompt[1], injected_message);
    }

    #[test]
    fn stream_text_iterator_upstream_should_apply_prepare_step_system_after_messages_override() {
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [finish(FinishReason::Stop)],
            0,
        )]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor)
                .with_prepare_step(WorkflowPrepareStepCallback::new(|_| {
                    WorkflowPrepareStepResult::default()
                        .with_messages(vec![user_text_message("replacement")])
                        .with_system("Use concise answers.")
                }));

        iterator
            .next(None)
            .expect("step succeeds")
            .expect("yield exists");

        let call_prompt = &iterator.executor().calls()[0].prompt;
        assert_eq!(
            call_prompt[0],
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Use concise answers."))
        );
        assert_eq!(call_prompt[1], user_text_message("replacement"));
    }

    #[test]
    fn stream_text_iterator_upstream_should_allow_prepare_step_to_change_model_dynamically() {
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                        "call-1",
                        "weatherTool",
                        "{}",
                    )),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor)
                .with_model(WorkflowModelInfo::new("test", "first-model"))
                .with_prepare_step(WorkflowPrepareStepCallback::new(|info| {
                    if info.step_number == 0 {
                        WorkflowPrepareStepResult::default()
                    } else {
                        WorkflowPrepareStepResult::default()
                            .with_model(WorkflowModelInfo::new("anthropic", "claude-sonnet-4.5"))
                            .with_generation_settings(WorkflowGenerationSettings {
                                temperature: Some(0.2),
                                max_output_tokens: Some(128),
                                ..WorkflowGenerationSettings::default()
                            })
                    }
                }));

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "weatherTool",
                json!({ "ok": true }),
            )]))
            .expect("second step succeeds");

        assert_eq!(
            iterator.executor().calls()[0].options.model,
            WorkflowModelInfo::new("test", "first-model")
        );
        assert_eq!(
            iterator.executor().calls()[1].options.model,
            WorkflowModelInfo::new("anthropic", "claude-sonnet-4.5")
        );
        assert_eq!(
            iterator.executor().calls()[1]
                .options
                .generation_settings
                .temperature,
            Some(0.2)
        );
        assert_eq!(
            iterator.executor().calls()[1]
                .options
                .generation_settings
                .max_output_tokens,
            Some(128)
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_allow_prepare_step_to_set_active_tools_and_tool_choice()
    {
        let tools = SerializableToolSet::from([
            ("weather".to_string(), serializable_tool()),
            ("calculator".to_string(), serializable_tool()),
        ]);
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [finish(FinishReason::Stop)],
            0,
        )]);
        let mut iterator = StreamTextIterator::new(user_prompt(), tools, executor)
            .with_prepare_step(WorkflowPrepareStepCallback::new(|_| {
                WorkflowPrepareStepResult::default()
                    .with_active_tools(["weather".to_string()])
                    .with_tool_choice(json!({
                        "type": "tool",
                        "toolName": "weather"
                    }))
            }));

        iterator
            .next(None)
            .expect("step succeeds")
            .expect("yield exists");

        let call = &iterator.executor().calls()[0];
        assert_eq!(call.tools.len(), 1);
        assert!(call.tools.contains_key("weather"));
        assert_eq!(
            call.options.tool_choice,
            Some(json!({
                "type": "tool",
                "toolName": "weather"
            }))
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_update_runtime_and_tools_context_from_prepare_step() {
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [finish(FinishReason::Stop)],
            0,
        )]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor)
                .with_runtime_context(
                    serde_json::from_value(json!({
                        "tenantId": "tenant_123"
                    }))
                    .expect("runtime context is an object"),
                )
                .with_prepare_step(WorkflowPrepareStepCallback::new(|info| {
                    let mut runtime_context = info.runtime_context;
                    runtime_context.insert("lastStep".to_string(), json!(info.step_number));
                    let mut tools_context = WorkflowToolsContext::new();
                    tools_context.insert(
                        "weather".to_string(),
                        Some(
                            serde_json::from_value(json!({
                                "region": "us"
                            }))
                            .expect("tool context is an object"),
                        ),
                    );
                    WorkflowPrepareStepResult::default()
                        .with_runtime_context(runtime_context)
                        .with_tools_context(tools_context)
                }));

        let result = iterator
            .next(None)
            .expect("step succeeds")
            .expect("yield exists");

        assert_eq!(result.runtime_context["tenantId"], json!("tenant_123"));
        assert_eq!(result.runtime_context["lastStep"], json!(0));
        assert_eq!(
            result.tools_context["weather"]
                .clone()
                .expect("tool context"),
            serde_json::from_value(json!({
                "region": "us"
            }))
            .expect("tool context is an object")
        );
        assert_eq!(
            iterator.executor().calls()[0].options.runtime_context,
            result.runtime_context
        );
    }

    #[test]
    fn stream_text_iterator_omits_provider_options_without_metadata() {
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                        "call-1",
                        "plainTool",
                        "{}",
                    )),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "plainTool",
                json!({ "ok": true }),
            )]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "plainTool"),
            Some(None)
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_not_add_provider_options_when_provider_metadata_is_undefined()
     {
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                        "call-1",
                        "testTool",
                        r#"{"query":"test"}"#,
                    )),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "testTool",
                json!({ "result": "success" }),
            )]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "testTool"),
            Some(None)
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_strip_openai_item_id_from_provider_metadata_to_avoid_reasoning_item_errors()
     {
        let tool_call = LanguageModelToolCall::new("call-1", "testTool", r#"{"query":"test"}"#)
            .with_provider_metadata(provider_metadata(json!({
                "openai": {
                    "itemId": "fc_0402bf2d292dd7ed00697a35fb10e0819ab0098545c4d0d7f5"
                }
            })));
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "testTool",
                json!({ "result": "success" }),
            )]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "testTool"),
            Some(None)
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_preserve_other_openai_metadata_while_stripping_item_id()
    {
        let tool_call = LanguageModelToolCall::new("call-1", "testTool", r#"{"query":"test"}"#)
            .with_provider_metadata(provider_metadata(json!({
                "openai": {
                    "itemId": "fc_0402bf2d292dd7ed00697a35fb10e0819ab0098545c4d0d7f5",
                    "someOtherField": "should-be-preserved"
                }
            })));
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "testTool",
                json!({ "result": "success" }),
            )]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "testTool"),
            Some(Some(provider_metadata(json!({
                "openai": {
                    "someOtherField": "should-be-preserved"
                }
            }))))
        );
    }

    #[test]
    fn stream_text_iterator_upstream_should_preserve_gemini_metadata_while_stripping_openai_item_id_in_mixed_provider_metadata()
     {
        let tool_call = LanguageModelToolCall::new("call-1", "testTool", r#"{"query":"test"}"#)
            .with_provider_metadata(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_gemini_preserved"
                },
                "openai": {
                    "itemId": "fc_should_be_stripped"
                }
            })));
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "testTool",
                json!({ "result": "success" }),
            )]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "testTool"),
            Some(Some(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_gemini_preserved"
                }
            }))))
        );
    }

    #[test]
    fn stream_text_iterator_strips_openai_item_id_and_preserves_other_metadata() {
        let tool_call = LanguageModelToolCall::new("call-1", "mixedTool", "{}")
            .with_provider_metadata(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_gemini"
                },
                "openai": {
                    "itemId": "fc_should_be_stripped",
                    "reasoningSummary": "keep"
                }
            })));
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts(
                [
                    LanguageModelStreamPart::ToolCall(tool_call),
                    finish(FinishReason::ToolCalls),
                ],
                0,
            ),
            output_from_parts([finish(FinishReason::Stop)], 1),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        iterator
            .next(Some(vec![tool_result(
                "call-1",
                "mixedTool",
                json!({ "ok": true }),
            )]))
            .expect("continuation succeeds");

        let prompt = &iterator.executor().calls()[1].prompt;
        assert_eq!(
            assistant_tool_call_provider_options(prompt, "mixedTool"),
            Some(Some(provider_metadata(json!({
                "google": {
                    "thoughtSignature": "sig_gemini"
                },
                "openai": {
                    "reasoningSummary": "keep"
                }
            }))))
        );
    }

    #[test]
    fn stream_text_iterator_passes_contexts_to_executor_and_yields_them() {
        let runtime_context: WorkflowRuntimeContext = serde_json::from_value(json!({
            "tenantId": "tenant_123"
        }))
        .expect("runtime context is an object");
        let mut tools_context = WorkflowToolsContext::new();
        tools_context.insert(
            "weather".to_string(),
            Some(
                serde_json::from_value(json!({
                    "unit": "celsius"
                }))
                .expect("tool context is an object"),
            ),
        );
        let executor = ScriptedStreamTextStepExecutor::new([output_from_parts(
            [finish(FinishReason::Stop)],
            0,
        )]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor)
                .with_runtime_context(runtime_context.clone())
                .with_tools_context(tools_context.clone());

        let result = iterator
            .next(None)
            .expect("step succeeds")
            .expect("yield exists");

        assert_eq!(result.runtime_context, runtime_context);
        assert_eq!(result.tools_context, tools_context);
        assert_eq!(iterator.executor().calls()[0].options.step_number, 0);
        assert_eq!(
            iterator.executor().calls()[0].options.runtime_context,
            result.runtime_context
        );
        assert_eq!(
            iterator.executor().calls()[0].options.tools_context,
            result.tools_context
        );
    }

    #[test]
    fn do_stream_step_from_parts_collects_provider_executed_results_and_valid_step_content() {
        let provider_result = LanguageModelToolResult::new(
            "call-1",
            "webSearch",
            NonNullJsonValue::new(json!({ "answer": "42" })).expect("non-null result"),
        );
        let output = output_from_parts(
            [
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "hello")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "webSearch",
                    r#"{"query":"life"}"#,
                )),
                LanguageModelStreamPart::ToolResult(provider_result),
                finish(FinishReason::ToolCalls),
            ],
            3,
        );

        assert_eq!(output.step.step_number, 3);
        assert_eq!(output.step.text, "hello");
        assert_eq!(output.step.tool_calls[0].input, json!({ "query": "life" }));
        assert_eq!(
            output.provider_executed_tool_results["call-1"].result,
            json!({ "answer": "42" })
        );
    }
}
