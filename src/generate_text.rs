use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

use crate::file_data::{FileData, FileDataContent};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonSchema, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelCustomContent, LanguageModelCustomPart, LanguageModelFile, LanguageModelFileData,
    LanguageModelFilePart, LanguageModelFinishReason, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelPrompt, LanguageModelReasoning,
    LanguageModelReasoningEffort, LanguageModelReasoningFile, LanguageModelReasoningFilePart,
    LanguageModelReasoningPart, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelSource, LanguageModelStreamPart, LanguageModelText,
    LanguageModelTextPart, LanguageModelTool, LanguageModelToolApprovalRequest,
    LanguageModelToolApprovalRequestPart, LanguageModelToolApprovalResponsePart,
    LanguageModelToolCall, LanguageModelToolCallPart, LanguageModelToolChoice,
    LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResult,
    LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUsage,
    OutputTokenUsage,
};
use crate::prompt::{Prompt, standardize_prompt};
use crate::provider::{
    InvalidPromptError, JsonParseError, TypeValidationContext, TypeValidationError,
    get_error_message,
};
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::provider_utils::{
    Base64DecodeError, ExperimentalSandbox, IdGeneratorOptions, Tool, ToolExecutionOptions,
    ToolModelOutputOptions, ToolNeedsApprovalOptions, convert_base64_to_bytes,
    convert_bytes_to_base64, create_id_generator, generate_id, prepare_tools_with_context,
};
use crate::warning::Warning;

const DEFAULT_MAX_STEPS: usize = 1;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Tool names that are enabled for a generation step.
///
/// `None` means no tool restriction is applied.
pub type ActiveTools = Option<Vec<String>>;

/// Future returned by a high-level tool input refinement function.
pub type ToolInputRefinementFuture =
    Pin<Box<dyn Future<Output = Result<JsonValue, ToolInputRefinementError>> + Send>>;

/// Function used to refine a parsed tool input before execution and result shaping.
pub type ToolInputRefinementFunction =
    dyn Fn(JsonValue) -> ToolInputRefinementFuture + Send + Sync + 'static;

/// Error returned by a tool input refinement function.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputRefinementError {
    /// Human-readable refinement failure message.
    pub message: String,
}

impl ToolInputRefinementError {
    /// Creates a tool input refinement error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the refinement failure message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for ToolInputRefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolInputRefinementError {}

impl From<String> for ToolInputRefinementError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for ToolInputRefinementError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

/// A per-tool input refinement callback.
///
/// This is the Rust-native equivalent of upstream
/// `experimental_refineToolInput`: it receives the parsed JSON input for a
/// valid tool call and returns the input that should be used for execution,
/// result records, and continuation messages.
#[derive(Clone)]
pub struct ToolInputRefinement {
    refine: Arc<ToolInputRefinementFunction>,
}

impl ToolInputRefinement {
    /// Creates a tool input refinement callback.
    pub fn new<F, Fut>(refine: F) -> Self
    where
        F: Fn(JsonValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, ToolInputRefinementError>> + Send + 'static,
    {
        Self {
            refine: Arc::new(move |input| Box::pin(refine(input))),
        }
    }

    /// Refines a parsed tool input.
    pub fn refine(&self, input: JsonValue) -> ToolInputRefinementFuture {
        (self.refine)(input)
    }
}

impl fmt::Debug for ToolInputRefinement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolInputRefinement")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level tool-call repair function.
pub type ToolCallRepairFuture =
    Pin<Box<dyn Future<Output = Result<Option<LanguageModelToolCall>, String>> + Send>>;

/// Function used to repair an unavailable or invalid tool call before execution.
pub type ToolCallRepairFunction =
    dyn Fn(ToolCallRepairOptions) -> ToolCallRepairFuture + Send + Sync + 'static;

/// Options passed to a tool-call repair callback.
#[derive(Clone, Debug)]
pub struct ToolCallRepairOptions {
    /// Original provider tool call that failed parsing or lookup.
    pub tool_call: LanguageModelToolCall,

    /// High-level Rust tools available for this step after active-tool filtering.
    pub tools: Vec<Tool>,

    /// Prompt messages that were sent to the model for this step.
    pub messages: LanguageModelPrompt,

    /// Original parsing or lookup error that triggered repair.
    pub error: ToolCallRepairOriginalError,
}

impl ToolCallRepairOptions {
    /// Creates tool-call repair options.
    pub fn new(
        tool_call: LanguageModelToolCall,
        tools: Vec<Tool>,
        messages: LanguageModelPrompt,
        error: ToolCallRepairOriginalError,
    ) -> Self {
        Self {
            tool_call,
            tools,
            messages,
            error,
        }
    }

    /// Returns the JSON Schema for a named high-level tool, when available.
    pub fn input_schema(&self, tool_name: &str) -> Option<&JsonSchema> {
        self.tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .map(|tool| &tool.input_schema)
    }
}

/// Callback wrapper for repairing failed model tool calls.
#[derive(Clone)]
pub struct ToolCallRepair {
    repair: Arc<ToolCallRepairFunction>,
}

impl ToolCallRepair {
    /// Creates a tool-call repair callback.
    pub fn new<F, Fut, E>(repair: F) -> Self
    where
        F: Fn(ToolCallRepairOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<LanguageModelToolCall>, E>> + Send + 'static,
        E: fmt::Display,
    {
        Self {
            repair: Arc::new(move |options| {
                let future = repair(options);
                Box::pin(async move { future.await.map_err(|error| error.to_string()) })
            }),
        }
    }

    /// Runs the repair callback.
    pub fn repair(&self, options: ToolCallRepairOptions) -> ToolCallRepairFuture {
        (self.repair)(options)
    }
}

impl fmt::Debug for ToolCallRepair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolCallRepair")
            .finish_non_exhaustive()
    }
}

/// Future returned by a per-step preparation callback.
pub type PrepareStepFuture<'a, M> = Pin<Box<dyn Future<Output = PrepareStepResult<'a, M>> + 'a>>;

/// Function used to override settings for one generate-text model step.
pub type PrepareStepFunction<'a, M> =
    dyn Fn(PrepareStepOptions<'a, M>) -> PrepareStepFuture<'a, M> + 'a;

/// Context passed to a per-step preparation callback.
///
/// This is the Rust-native equivalent of upstream `PrepareStepFunction`: it
/// exposes the completed steps, the step number about to run, the current
/// prompt messages, accumulated response messages, and mutable contexts.
pub struct PrepareStepOptions<'a, M: LanguageModel + ?Sized> {
    /// Steps that have already completed.
    pub steps: Vec<GenerateTextStep>,

    /// Zero-based step number that is about to run.
    pub step_number: usize,

    /// Default model for the generation call.
    pub model: &'a M,

    /// Messages that will be sent for this step unless overridden.
    pub messages: LanguageModelPrompt,

    /// Initial messages passed into generate_text before any response messages.
    pub initial_messages: LanguageModelPrompt,

    /// Response messages accumulated from initial approvals and previous steps.
    pub response_messages: Vec<LanguageModelMessage>,

    /// Runtime context carried by the generation loop.
    pub runtime_context: JsonObject,

    /// Tool context carried by the generation loop.
    pub tools_context: JsonObject,

    /// Experimental sandbox environment available for this generation step.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

/// Per-step settings returned by a preparation callback.
///
/// Missing fields keep the outer generate-text settings. Message, runtime
/// context, and tool context overrides carry forward to later steps, matching
/// upstream `prepareStep` behavior.
#[derive(Debug)]
pub struct PrepareStepResult<'a, M: LanguageModel + ?Sized> {
    /// Optional same-type model override for this step.
    pub model: Option<&'a M>,

    /// Optional tool-choice override for this step.
    pub tool_choice: Option<LanguageModelToolChoice>,

    /// Optional active-tool override for this step.
    pub active_tools: ActiveTools,

    /// Optional full prompt-message override. Carries forward after this step.
    pub messages: Option<LanguageModelPrompt>,

    /// Optional runtime context override. Carries forward after this step.
    pub runtime_context: Option<JsonObject>,

    /// Optional tool context override. Carries forward after this step.
    pub tools_context: Option<JsonObject>,

    /// Optional provider-specific option override for this step.
    pub provider_options: Option<ProviderOptions>,

    /// Optional sandbox override for this step only.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

impl<'a, M: LanguageModel + ?Sized> PrepareStepResult<'a, M> {
    /// Creates an empty result that keeps all outer settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the model used for this step.
    pub fn with_model(mut self, model: &'a M) -> Self {
        self.model = Some(model);
        self
    }

    /// Overrides the tool-choice strategy for this step.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Overrides the active tool names for this step.
    pub fn with_active_tools(
        mut self,
        active_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.active_tools = Some(active_tools.into_iter().map(Into::into).collect());
        self
    }

    /// Overrides the prompt messages for this step and subsequent steps.
    pub fn with_messages(mut self, messages: LanguageModelPrompt) -> Self {
        self.messages = Some(messages);
        self
    }

    /// Overrides runtime context for this step and subsequent steps.
    pub fn with_runtime_context(mut self, runtime_context: JsonObject) -> Self {
        self.runtime_context = Some(runtime_context);
        self
    }

    /// Overrides tool context for this step and subsequent steps.
    pub fn with_tools_context(mut self, tools_context: JsonObject) -> Self {
        self.tools_context = Some(tools_context);
        self
    }

    /// Adds provider-specific options for this step.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Overrides the experimental sandbox for this step only.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }
}

impl<M: LanguageModel + ?Sized> Default for PrepareStepResult<'_, M> {
    fn default() -> Self {
        Self {
            model: None,
            tool_choice: None,
            active_tools: None,
            messages: None,
            runtime_context: None,
            tools_context: None,
            provider_options: None,
            experimental_sandbox: None,
        }
    }
}

/// Per-step preparation callback for high-level generate-text calls.
pub struct PrepareStep<'a, M: LanguageModel + ?Sized> {
    prepare: Rc<PrepareStepFunction<'a, M>>,
}

impl<'a, M: LanguageModel + ?Sized> PrepareStep<'a, M> {
    /// Creates a per-step preparation callback.
    pub fn new<F, Fut>(prepare: F) -> Self
    where
        F: Fn(PrepareStepOptions<'a, M>) -> Fut + 'a,
        Fut: Future<Output = PrepareStepResult<'a, M>> + 'a,
    {
        Self {
            prepare: Rc::new(move |options| Box::pin(prepare(options))),
        }
    }

    /// Runs the preparation callback.
    pub fn prepare(&self, options: PrepareStepOptions<'a, M>) -> PrepareStepFuture<'a, M> {
        (self.prepare)(options)
    }
}

impl<M: LanguageModel + ?Sized> Clone for PrepareStep<'_, M> {
    fn clone(&self) -> Self {
        Self {
            prepare: Rc::clone(&self.prepare),
        }
    }
}

impl<M: LanguageModel + ?Sized> fmt::Debug for PrepareStep<'_, M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrepareStep")
            .finish_non_exhaustive()
    }
}

/// Event sent when a high-level non-streaming generate-text call starts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextStartEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Upstream operation identifier.
    pub operation_id: String,

    /// Provider identifier for the initial model.
    pub provider: String,

    /// Provider-specific model id for the initial model.
    pub model_id: String,

    /// Prompt messages at the start of the generation.
    pub messages: LanguageModelPrompt,

    /// Tools available to the generation before active-tool filtering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<LanguageModelTool>,

    /// Tool-choice strategy configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<LanguageModelToolChoice>,

    /// Optional active-tool restriction for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tools: ActiveTools,

    /// Maximum output tokens configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Sampling temperature configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus sampling value configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Top-k sampling value configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,

    /// Presence penalty configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Frequency penalty configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Stop sequences configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Deterministic sampling seed configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Reasoning effort configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<LanguageModelReasoningEffort>,

    /// Additional HTTP headers configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider-specific options configured for the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Runtime context at the start of the generation.
    #[serde(default)]
    pub runtime_context: JsonObject,

    /// Tool context at the start of the generation.
    #[serde(default)]
    pub tools_context: JsonObject,
}

/// Event sent before each high-level non-streaming generate-text model step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextStepStartEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Provider identifier for the step model.
    pub provider: String,

    /// Provider-specific model id for the step model.
    pub model_id: String,

    /// Zero-based step index.
    pub step_number: usize,

    /// Prompt messages that will be sent for this step.
    pub messages: LanguageModelPrompt,

    /// Tools available to the model for this step after active-tool filtering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<LanguageModelTool>,

    /// Tool-choice strategy for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<LanguageModelToolChoice>,

    /// Active-tool restriction used for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tools: ActiveTools,

    /// Previously completed steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<GenerateTextStep>,

    /// Provider-specific options for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Runtime context used for this step.
    #[serde(default)]
    pub runtime_context: JsonObject,

    /// Tool context used for this step.
    #[serde(default)]
    pub tools_context: JsonObject,
}

/// Event sent immediately before a provider language model call begins.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCallStartEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Provider identifier for the step model.
    pub provider: String,

    /// Provider-specific model id for the step model.
    pub model_id: String,

    /// Prompt messages sent to the provider for this model call.
    pub messages: LanguageModelPrompt,

    /// Prepared tool definitions sent to the provider for this model call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<LanguageModelTool>,

    /// Maximum output tokens configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Sampling temperature configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Stop sequences configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Nucleus sampling value configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Top-k sampling value configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,

    /// Presence penalty configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Frequency penalty configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Requested provider response format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<LanguageModelResponseFormat>,

    /// Deterministic sampling seed configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Tool-choice strategy configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<LanguageModelToolChoice>,

    /// Whether raw stream chunks should be included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_raw_chunks: Option<bool>,

    /// Additional HTTP headers configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Reasoning effort configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<LanguageModelReasoningEffort>,

    /// Provider-specific options configured for the provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelCallStartEvent {
    pub(crate) fn from_call_options(
        call_id: &str,
        provider: &str,
        model_id: &str,
        call_options: &LanguageModelCallOptions,
    ) -> Self {
        Self {
            call_id: call_id.to_string(),
            provider: provider.to_string(),
            model_id: model_id.to_string(),
            messages: call_options.prompt.clone(),
            tools: call_options.tools.clone().unwrap_or_default(),
            max_output_tokens: call_options.max_output_tokens,
            temperature: call_options.temperature,
            stop_sequences: call_options.stop_sequences.clone(),
            top_p: call_options.top_p,
            top_k: call_options.top_k,
            presence_penalty: call_options.presence_penalty,
            frequency_penalty: call_options.frequency_penalty,
            response_format: call_options.response_format.clone(),
            seed: call_options.seed,
            tool_choice: call_options.tool_choice.clone(),
            include_raw_chunks: call_options.include_raw_chunks,
            headers: call_options.headers.clone(),
            reasoning: call_options.reasoning.clone(),
            provider_options: call_options.provider_options.clone(),
        }
    }
}

/// Performance metrics for a provider language model call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCallPerformance {
    /// Time spent waiting for the language model response in milliseconds.
    pub response_time_ms: u64,

    /// Effective number of output tokens per second over the full model response.
    pub effective_output_tokens_per_second: f64,

    /// Output tokens per second after the first output token was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_per_second: Option<f64>,

    /// Input tokens per second before the first output token was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_per_second: Option<f64>,

    /// Effective input and output tokens per second over the full model response.
    pub effective_total_tokens_per_second: f64,

    /// Time until the first text, reasoning, or tool input delta was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_first_output_token_ms: Option<u64>,
}

impl LanguageModelCallPerformance {
    fn from_usage(usage: &LanguageModelUsage, response_time_ms: u64) -> Self {
        Self {
            response_time_ms,
            effective_output_tokens_per_second: calculate_tokens_per_second(
                usage.output_tokens.total,
                response_time_ms,
            ),
            output_tokens_per_second: None,
            input_tokens_per_second: None,
            effective_total_tokens_per_second: calculate_tokens_per_second(
                sum_token_counts(usage.input_tokens.total, usage.output_tokens.total),
                response_time_ms,
            ),
            time_to_first_output_token_ms: None,
        }
    }
}

/// Event sent after a provider language model call completes, before local tool execution.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCallEndEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Provider identifier for the step model.
    pub provider: String,

    /// Provider-specific model id for the step model.
    pub model_id: String,

    /// Unified reason why the model call finished.
    pub finish_reason: FinishReason,

    /// Usage reported by the model call.
    pub usage: LanguageModelUsage,

    /// Content parts produced by the model call.
    pub content: Vec<GenerateTextContentPart>,

    /// Provider response id for this model call.
    pub response_id: String,

    /// Provider-call performance metrics.
    pub performance: LanguageModelCallPerformance,
}

impl LanguageModelCallEndEvent {
    pub(crate) fn from_step(step: &GenerateTextStep, response_time_ms: u64) -> Self {
        Self {
            call_id: step.call_id.clone(),
            provider: step.model.provider.clone(),
            model_id: step.model.model_id.clone(),
            finish_reason: step.finish_reason.clone(),
            usage: step.usage.clone(),
            content: step.content.clone(),
            response_id: step
                .response
                .as_ref()
                .and_then(|response| response.id.clone())
                .expect("generate_text assigns a response id before language-model-call end"),
            performance: LanguageModelCallPerformance::from_usage(&step.usage, response_time_ms),
        }
    }
}

/// Upstream callback alias for [`LanguageModelCallStartEvent`].
pub type OnLanguageModelCallStartCallback<'a> = GenerateTextOnLanguageModelCallStartFunction<'a>;

/// Upstream callback alias for [`LanguageModelCallEndEvent`].
pub type OnLanguageModelCallEndCallback<'a> = GenerateTextOnLanguageModelCallEndFunction<'a>;

/// Future returned by a high-level generate-text start callback.
pub type GenerateTextOnStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before a non-streaming generate-text call performs model work.
pub type GenerateTextOnStartFunction<'a> =
    dyn Fn(GenerateTextStartEvent) -> GenerateTextOnStartFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateTextOnStartFunction`].
pub type GenerateTextOnStartCallback<'a> = GenerateTextOnStartFunction<'a>;

/// Callback wrapper for upstream `experimental_onStart`.
pub struct GenerateTextOnStart<'a> {
    on_start: Rc<GenerateTextOnStartFunction<'a>>,
}

impl<'a> GenerateTextOnStart<'a> {
    /// Creates a generation-start callback.
    pub fn new<F, Fut>(on_start: F) -> Self
    where
        F: Fn(GenerateTextStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_start: Rc::new(move |event| Box::pin(on_start(event))),
        }
    }

    /// Runs the generation-start callback.
    pub fn start(&self, event: GenerateTextStartEvent) -> GenerateTextOnStartFuture<'a> {
        (self.on_start)(event)
    }
}

impl fmt::Debug for GenerateTextOnStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-text step-start callback.
pub type GenerateTextOnStepStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before each non-streaming generate-text model step starts.
pub type GenerateTextOnStepStartFunction<'a> =
    dyn Fn(GenerateTextStepStartEvent) -> GenerateTextOnStepStartFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateTextOnStepStartFunction`].
pub type GenerateTextOnStepStartCallback<'a> = GenerateTextOnStepStartFunction<'a>;

/// Callback wrapper for upstream `experimental_onStepStart`.
pub struct GenerateTextOnStepStart<'a> {
    on_step_start: Rc<GenerateTextOnStepStartFunction<'a>>,
}

impl<'a> GenerateTextOnStepStart<'a> {
    /// Creates a step-start callback.
    pub fn new<F, Fut>(on_step_start: F) -> Self
    where
        F: Fn(GenerateTextStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_step_start: Rc::new(move |event| Box::pin(on_step_start(event))),
        }
    }

    /// Runs the step-start callback.
    pub fn start(&self, event: GenerateTextStepStartEvent) -> GenerateTextOnStepStartFuture<'a> {
        (self.on_step_start)(event)
    }
}

impl fmt::Debug for GenerateTextOnStepStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnStepStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a language-model-call start callback.
pub type GenerateTextOnLanguageModelCallStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked immediately before a provider language model call begins.
pub type GenerateTextOnLanguageModelCallStartFunction<'a> =
    dyn Fn(LanguageModelCallStartEvent) -> GenerateTextOnLanguageModelCallStartFuture<'a> + 'a;

/// Callback wrapper for upstream `experimental_onLanguageModelCallStart`.
pub struct GenerateTextOnLanguageModelCallStart<'a> {
    on_language_model_call_start: Rc<GenerateTextOnLanguageModelCallStartFunction<'a>>,
}

impl<'a> GenerateTextOnLanguageModelCallStart<'a> {
    /// Creates a language-model-call start callback.
    pub fn new<F, Fut>(on_language_model_call_start: F) -> Self
    where
        F: Fn(LanguageModelCallStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_language_model_call_start: Rc::new(move |event| {
                Box::pin(on_language_model_call_start(event))
            }),
        }
    }

    /// Runs the language-model-call start callback.
    pub fn start(
        &self,
        event: LanguageModelCallStartEvent,
    ) -> GenerateTextOnLanguageModelCallStartFuture<'a> {
        (self.on_language_model_call_start)(event)
    }
}

impl fmt::Debug for GenerateTextOnLanguageModelCallStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnLanguageModelCallStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a language-model-call end callback.
pub type GenerateTextOnLanguageModelCallEndFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after a provider language model call completes.
pub type GenerateTextOnLanguageModelCallEndFunction<'a> =
    dyn Fn(LanguageModelCallEndEvent) -> GenerateTextOnLanguageModelCallEndFuture<'a> + 'a;

/// Callback wrapper for upstream `experimental_onLanguageModelCallEnd`.
pub struct GenerateTextOnLanguageModelCallEnd<'a> {
    on_language_model_call_end: Rc<GenerateTextOnLanguageModelCallEndFunction<'a>>,
}

impl<'a> GenerateTextOnLanguageModelCallEnd<'a> {
    /// Creates a language-model-call end callback.
    pub fn new<F, Fut>(on_language_model_call_end: F) -> Self
    where
        F: Fn(LanguageModelCallEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_language_model_call_end: Rc::new(move |event| {
                Box::pin(on_language_model_call_end(event))
            }),
        }
    }

    /// Runs the language-model-call end callback.
    pub fn end(
        &self,
        event: LanguageModelCallEndEvent,
    ) -> GenerateTextOnLanguageModelCallEndFuture<'a> {
        (self.on_language_model_call_end)(event)
    }
}

impl fmt::Debug for GenerateTextOnLanguageModelCallEnd<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnLanguageModelCallEnd")
            .finish_non_exhaustive()
    }
}

/// Event sent before a Rust tool executor is invoked by `generate_text`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextToolExecutionStartEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Prompt messages sent to the model for the response that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool call about to be executed.
    pub tool_call: GenerateTextToolCall,

    /// Tool-specific context configured for the executed tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_context: Option<JsonValue>,
}

/// Event sent after a Rust tool executor completes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextToolExecutionEndEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Prompt messages sent to the model for the response that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool call that was executed.
    pub tool_call: GenerateTextToolCall,

    /// Tool-specific context configured for the executed tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_context: Option<JsonValue>,

    /// Execution time of the tool call in milliseconds.
    pub tool_execution_ms: u64,

    /// Tool result or tool error produced by the execution.
    pub tool_output: GenerateTextToolResult,
}

/// Upstream generate-text tool execution start event name.
pub type ToolExecutionStartEvent = GenerateTextToolExecutionStartEvent;

/// Upstream generate-text tool execution end event name.
pub type ToolExecutionEndEvent = GenerateTextToolExecutionEndEvent;

/// Deprecated upstream alias for [`ToolExecutionStartEvent`].
#[deprecated(note = "use ToolExecutionStartEvent instead")]
pub type OnToolCallStartEvent = ToolExecutionStartEvent;

/// Deprecated upstream alias for [`ToolExecutionEndEvent`].
#[deprecated(note = "use ToolExecutionEndEvent instead")]
pub type OnToolCallFinishEvent = ToolExecutionEndEvent;

/// Future returned by a high-level tool-execution start callback.
pub type GenerateTextOnToolExecutionStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before a Rust tool executor is invoked.
pub type GenerateTextOnToolExecutionStartFunction<'a> =
    dyn Fn(GenerateTextToolExecutionStartEvent) -> GenerateTextOnToolExecutionStartFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateTextOnToolExecutionStartFunction`].
pub type OnToolExecutionStartCallback<'a> = GenerateTextOnToolExecutionStartFunction<'a>;

/// Callback wrapper for upstream `onToolExecutionStart`.
pub struct GenerateTextOnToolExecutionStart<'a> {
    on_tool_execution_start: Rc<GenerateTextOnToolExecutionStartFunction<'a>>,
}

impl<'a> GenerateTextOnToolExecutionStart<'a> {
    /// Creates a tool-execution start callback.
    pub fn new<F, Fut>(on_tool_execution_start: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_tool_execution_start: Rc::new(move |event| Box::pin(on_tool_execution_start(event))),
        }
    }

    /// Runs the tool-execution start callback.
    pub fn start(
        &self,
        event: GenerateTextToolExecutionStartEvent,
    ) -> GenerateTextOnToolExecutionStartFuture<'a> {
        (self.on_tool_execution_start)(event)
    }
}

impl fmt::Debug for GenerateTextOnToolExecutionStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnToolExecutionStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level tool-execution end callback.
pub type GenerateTextOnToolExecutionEndFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after a Rust tool executor completes.
pub type GenerateTextOnToolExecutionEndFunction<'a> =
    dyn Fn(GenerateTextToolExecutionEndEvent) -> GenerateTextOnToolExecutionEndFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateTextOnToolExecutionEndFunction`].
pub type OnToolExecutionEndCallback<'a> = GenerateTextOnToolExecutionEndFunction<'a>;

/// Callback wrapper for upstream `onToolExecutionEnd`.
pub struct GenerateTextOnToolExecutionEnd<'a> {
    on_tool_execution_end: Rc<GenerateTextOnToolExecutionEndFunction<'a>>,
}

impl<'a> GenerateTextOnToolExecutionEnd<'a> {
    /// Creates a tool-execution end callback.
    pub fn new<F, Fut>(on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_tool_execution_end: Rc::new(move |event| Box::pin(on_tool_execution_end(event))),
        }
    }

    /// Runs the tool-execution end callback.
    pub fn end(
        &self,
        event: GenerateTextToolExecutionEndEvent,
    ) -> GenerateTextOnToolExecutionEndFuture<'a> {
        (self.on_tool_execution_end)(event)
    }
}

impl fmt::Debug for GenerateTextOnToolExecutionEnd<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnToolExecutionEnd")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-text step-finish callback.
pub type GenerateTextOnStepFinishFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after each non-streaming generate-text step is complete.
pub type GenerateTextOnStepFinishFunction<'a> =
    dyn Fn(GenerateTextStep) -> GenerateTextOnStepFinishFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateTextOnStepFinishFunction`].
pub type GenerateTextOnStepFinishCallback<'a> = GenerateTextOnStepFinishFunction<'a>;

/// Callback wrapper for upstream `onStepFinish`.
///
/// The callback receives the fully constructed step result after response
/// messages, include filtering, and performance metrics have been populated.
pub struct GenerateTextOnStepFinish<'a> {
    on_step_finish: Rc<GenerateTextOnStepFinishFunction<'a>>,
}

impl<'a> GenerateTextOnStepFinish<'a> {
    /// Creates a step-finish callback.
    pub fn new<F, Fut>(on_step_finish: F) -> Self
    where
        F: Fn(GenerateTextStep) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_step_finish: Rc::new(move |step| Box::pin(on_step_finish(step))),
        }
    }

    /// Runs the step-finish callback.
    pub fn finish(&self, step: GenerateTextStep) -> GenerateTextOnStepFinishFuture<'a> {
        (self.on_step_finish)(step)
    }
}

impl fmt::Debug for GenerateTextOnStepFinish<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnStepFinish")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-text finish callback.
pub type GenerateTextOnFinishFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after a non-streaming generate-text call is complete.
pub type GenerateTextOnFinishFunction<'a> =
    dyn Fn(GenerateTextFinishEvent) -> GenerateTextOnFinishFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateTextOnFinishFunction`].
pub type GenerateTextOnFinishCallback<'a> = GenerateTextOnFinishFunction<'a>;

/// Callback wrapper for upstream `onFinish`.
///
/// The callback receives the final step fields plus all accumulated response
/// messages, steps, and total usage before high-level output parsing.
pub struct GenerateTextOnFinish<'a> {
    on_finish: Rc<GenerateTextOnFinishFunction<'a>>,
}

impl<'a> GenerateTextOnFinish<'a> {
    /// Creates a finish callback.
    pub fn new<F, Fut>(on_finish: F) -> Self
    where
        F: Fn(GenerateTextFinishEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_finish: Rc::new(move |result| Box::pin(on_finish(result))),
        }
    }

    /// Runs the finish callback.
    pub fn finish(&self, event: GenerateTextFinishEvent) -> GenerateTextOnFinishFuture<'a> {
        (self.on_finish)(event)
    }
}

impl fmt::Debug for GenerateTextOnFinish<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateTextOnFinish")
            .finish_non_exhaustive()
    }
}

/// Predicate-style stop condition for high-level generate-text tool loops.
///
/// The upstream SDK models stop conditions as async predicates. This Rust
/// contract ports the public built-in predicates as data so callers can use
/// them without committing the crate to an async trait or boxed closure API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StopCondition {
    /// Stop when the number of completed steps exactly matches this count.
    StepCount(usize),

    /// Never stop because of this condition.
    LoopFinished,

    /// Stop when the most recent step includes any of these tool names.
    HasToolCall(Vec<String>),
}

impl StopCondition {
    /// Returns whether this condition is met for the completed steps.
    pub fn is_met(&self, steps: &[GenerateTextStep]) -> bool {
        match self {
            Self::StepCount(step_count) => steps.len() == *step_count,
            Self::LoopFinished => false,
            Self::HasToolCall(tool_names) => {
                let Some(last_step) = steps.last() else {
                    return false;
                };

                last_step
                    .tool_calls
                    .iter()
                    .any(|tool_call| tool_names.iter().any(|name| name == &tool_call.tool_name))
            }
        }
    }
}

/// Creates a stop condition that is met after exactly `step_count` completed steps.
pub fn is_step_count(step_count: usize) -> StopCondition {
    StopCondition::StepCount(step_count)
}

/// Deprecated upstream alias for [`is_step_count`].
pub fn step_count_is(step_count: usize) -> StopCondition {
    is_step_count(step_count)
}

/// Creates a stop condition that never stops the loop by itself.
pub fn is_loop_finished() -> StopCondition {
    StopCondition::LoopFinished
}

/// Creates a stop condition that is met when the last step calls any named tool.
pub fn has_tool_call(tool_names: impl IntoIterator<Item = impl Into<String>>) -> StopCondition {
    StopCondition::HasToolCall(tool_names.into_iter().map(Into::into).collect())
}

/// Returns whether any stop condition is met for the completed steps.
pub fn is_stop_condition_met(
    stop_conditions: &[StopCondition],
    steps: &[GenerateTextStep],
) -> bool {
    stop_conditions
        .iter()
        .any(|condition| condition.is_met(steps))
}

/// Filters high-level tools to the active tool subset.
///
/// This mirrors upstream `filterActiveTools`: missing tools or missing active
/// tool names return the original tool set, while an active list keeps only
/// tools whose names appear in that list.
pub fn filter_active_tools(
    tools: Option<Vec<Tool>>,
    active_tools: Option<&[String]>,
) -> Option<Vec<Tool>> {
    let tools = tools?;

    let Some(active_tools) = active_tools else {
        return Some(tools);
    };

    Some(
        tools
            .into_iter()
            .filter(|tool| {
                active_tools
                    .iter()
                    .any(|active_tool| active_tool == &tool.name)
            })
            .collect(),
    )
}

/// Experimental upstream alias for [`filter_active_tools`].
pub fn experimental_filter_active_tools(
    tools: Option<Vec<Tool>>,
    active_tools: Option<&[String]>,
) -> Option<Vec<Tool>> {
    filter_active_tools(tools, active_tools)
}

/// How reasoning prompt parts should be removed by [`prune_messages`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PruneReasoning {
    /// Remove reasoning from every assistant message.
    All,

    /// Remove reasoning from every assistant message except the final message.
    BeforeLastMessage,

    /// Keep all reasoning parts.
    #[default]
    None,
}

/// How empty messages should be handled by [`prune_messages`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PruneEmptyMessages {
    /// Keep messages whose content is empty after pruning.
    Keep,

    /// Remove messages whose content is empty after pruning.
    #[default]
    Remove,
}

/// Tool-call pruning scope used inside [`PruneToolCallRule`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PruneToolCallRuleMode {
    /// Remove matching tool calls/results from all messages.
    All,

    /// Remove matching tool calls/results before the final message.
    BeforeLastMessage,

    /// Remove matching tool calls/results before the final `n` messages.
    BeforeLastMessages(usize),
}

impl PruneToolCallRuleMode {
    fn as_str(self) -> String {
        match self {
            Self::All => "all".to_string(),
            Self::BeforeLastMessage => "before-last-message".to_string(),
            Self::BeforeLastMessages(count) => format!("before-last-{count}-messages"),
        }
    }

    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "all" => Ok(Self::All),
            "before-last-message" => Ok(Self::BeforeLastMessage),
            value => parse_before_last_messages(value)
                .map(Self::BeforeLastMessages)
                .ok_or_else(|| format!("invalid prune tool-call rule mode: {value}")),
        }
    }

    fn keep_last_messages_count(self) -> Option<usize> {
        match self {
            Self::All => None,
            Self::BeforeLastMessage => Some(1),
            Self::BeforeLastMessages(count) => Some(count),
        }
    }
}

impl Serialize for PruneToolCallRuleMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for PruneToolCallRuleMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(serde::de::Error::custom)
    }
}

/// A single tool-call pruning rule.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneToolCallRule {
    /// Which messages should be pruned.
    #[serde(rename = "type")]
    pub mode: PruneToolCallRuleMode,

    /// Optional tool names to prune. Missing means all tool calls/results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}

impl PruneToolCallRule {
    /// Creates a rule that prunes matching tool calls/results from all messages.
    pub fn all() -> Self {
        Self {
            mode: PruneToolCallRuleMode::All,
            tools: None,
        }
    }

    /// Creates a rule that keeps the final message's tool calls/results.
    pub fn before_last_message() -> Self {
        Self {
            mode: PruneToolCallRuleMode::BeforeLastMessage,
            tools: None,
        }
    }

    /// Creates a rule that keeps tool calls/results in the final `count` messages.
    pub fn before_last_messages(count: usize) -> Self {
        Self {
            mode: PruneToolCallRuleMode::BeforeLastMessages(count),
            tools: None,
        }
    }

    /// Restricts this rule to the supplied tool names.
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }
}

/// How tool calls, tool results, and approval responses should be removed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PruneToolCalls {
    /// Keep all tool-call related parts.
    #[default]
    None,

    /// Remove all tool-call related parts.
    All,

    /// Remove tool-call related parts before the final message.
    BeforeLastMessage,

    /// Remove tool-call related parts before the final `n` messages.
    BeforeLastMessages(usize),

    /// Apply explicit pruning rules in order.
    Rules(Vec<PruneToolCallRule>),
}

impl PruneToolCalls {
    fn rules(&self) -> Vec<PruneToolCallRule> {
        match self {
            Self::None => Vec::new(),
            Self::All => vec![PruneToolCallRule::all()],
            Self::BeforeLastMessage => vec![PruneToolCallRule::before_last_message()],
            Self::BeforeLastMessages(count) => {
                vec![PruneToolCallRule::before_last_messages(*count)]
            }
            Self::Rules(rules) => rules.clone(),
        }
    }
}

impl Serialize for PruneToolCalls {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::None => serializer.serialize_str("none"),
            Self::All => serializer.serialize_str("all"),
            Self::BeforeLastMessage => serializer.serialize_str("before-last-message"),
            Self::BeforeLastMessages(count) => {
                serializer.serialize_str(&format!("before-last-{count}-messages"))
            }
            Self::Rules(rules) => rules.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PruneToolCalls {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PruneToolCallsWire {
            String(String),
            Rules(Vec<PruneToolCallRule>),
        }

        match PruneToolCallsWire::deserialize(deserializer)? {
            PruneToolCallsWire::String(value) => parse_prune_tool_calls(&value)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid toolCalls: {value}"))),
            PruneToolCallsWire::Rules(rules) => Ok(Self::Rules(rules)),
        }
    }
}

/// Options for pruning model messages before another generation call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneMessagesOptions {
    /// Messages to prune.
    pub messages: Vec<LanguageModelMessage>,

    /// Reasoning pruning behavior.
    #[serde(default)]
    pub reasoning: PruneReasoning,

    /// Tool-call/result pruning behavior.
    #[serde(default)]
    pub tool_calls: PruneToolCalls,

    /// Empty-message pruning behavior.
    #[serde(default)]
    pub empty_messages: PruneEmptyMessages,
}

impl PruneMessagesOptions {
    /// Creates pruning options with upstream defaults.
    pub fn new(messages: Vec<LanguageModelMessage>) -> Self {
        Self {
            messages,
            reasoning: PruneReasoning::None,
            tool_calls: PruneToolCalls::None,
            empty_messages: PruneEmptyMessages::Remove,
        }
    }

    /// Sets reasoning pruning behavior.
    pub fn with_reasoning(mut self, reasoning: PruneReasoning) -> Self {
        self.reasoning = reasoning;
        self
    }

    /// Sets tool-call/result pruning behavior.
    pub fn with_tool_calls(mut self, tool_calls: PruneToolCalls) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    /// Sets empty-message pruning behavior.
    pub fn with_empty_messages(mut self, empty_messages: PruneEmptyMessages) -> Self {
        self.empty_messages = empty_messages;
        self
    }
}

/// Prunes model messages using the upstream `pruneMessages` behavior.
pub fn prune_messages(options: PruneMessagesOptions) -> Vec<LanguageModelMessage> {
    let PruneMessagesOptions {
        mut messages,
        reasoning,
        tool_calls,
        empty_messages,
    } = options;

    messages = prune_reasoning_messages(messages, reasoning);

    for rule in tool_calls.rules() {
        messages = prune_tool_call_messages(messages, &rule);
    }

    if empty_messages == PruneEmptyMessages::Remove {
        messages.retain(message_has_content);
    }

    messages
}

/// Tool approval response paired with its original request and tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectedToolApproval {
    /// Tool approval request found in an assistant message.
    pub approval_request: LanguageModelToolApprovalRequestPart,

    /// Tool approval response found in the latest tool message.
    pub approval_response: LanguageModelToolApprovalResponsePart,

    /// Tool call referenced by the approval request.
    pub tool_call: LanguageModelToolCallPart,
}

/// Tool approvals collected from the latest tool message.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectedToolApprovals {
    /// Approvals where the latest response granted execution.
    pub approved_tool_approvals: Vec<CollectedToolApproval>,

    /// Approvals where the latest response denied execution.
    pub denied_tool_approvals: Vec<CollectedToolApproval>,
}

/// Error returned while collecting tool approval responses from prompt messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollectToolApprovalsError {
    /// A tool approval response referenced an unknown approval request.
    InvalidToolApproval(InvalidToolApprovalError),

    /// A tool approval request referenced a missing tool call.
    ToolCallNotFoundForApproval(ToolCallNotFoundForApprovalError),
}

impl fmt::Display for CollectToolApprovalsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToolApproval(error) => error.fmt(formatter),
            Self::ToolCallNotFoundForApproval(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CollectToolApprovalsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidToolApproval(error) => Some(error),
            Self::ToolCallNotFoundForApproval(error) => Some(error),
        }
    }
}

impl From<InvalidToolApprovalError> for CollectToolApprovalsError {
    fn from(error: InvalidToolApprovalError) -> Self {
        Self::InvalidToolApproval(error)
    }
}

impl From<ToolCallNotFoundForApprovalError> for CollectToolApprovalsError {
    fn from(error: ToolCallNotFoundForApprovalError) -> Self {
        Self::ToolCallNotFoundForApproval(error)
    }
}

/// Collects tool approval responses from the latest tool message.
///
/// This mirrors upstream `collectToolApprovals`: if the final message is not a
/// tool message, no approvals are returned. Approval responses whose tool call
/// already has a result in that final tool message are treated as processed and
/// omitted from the returned approval lists.
pub fn collect_tool_approvals(
    messages: &[LanguageModelMessage],
) -> Result<CollectedToolApprovals, CollectToolApprovalsError> {
    let Some(LanguageModelMessage::Tool(last_message)) = messages.last() else {
        return Ok(CollectedToolApprovals::default());
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

    let tool_results = last_message
        .content
        .iter()
        .filter_map(|part| match part {
            LanguageModelToolContentPart::ToolResult(part) => Some(part.tool_call_id.clone()),
            LanguageModelToolContentPart::ToolApprovalResponse(_) => None,
        })
        .collect::<BTreeSet<_>>();

    let mut collected = CollectedToolApprovals::default();
    for part in &last_message.content {
        let LanguageModelToolContentPart::ToolApprovalResponse(approval_response) = part else {
            continue;
        };

        let approval_request = approval_requests_by_id
            .get(&approval_response.approval_id)
            .cloned()
            .ok_or_else(|| InvalidToolApprovalError::new(&approval_response.approval_id))?;

        if tool_results.contains(&approval_request.tool_call_id) {
            continue;
        }

        let tool_call = tool_calls_by_id
            .get(&approval_request.tool_call_id)
            .cloned()
            .ok_or_else(|| {
                ToolCallNotFoundForApprovalError::new(
                    &approval_request.tool_call_id,
                    &approval_request.approval_id,
                )
            })?;

        let approval = CollectedToolApproval {
            approval_request,
            approval_response: approval_response.clone(),
            tool_call,
        };

        if approval.approval_response.approved {
            collected.approved_tool_approvals.push(approval);
        } else {
            collected.denied_tool_approvals.push(approval);
        }
    }

    Ok(collected)
}

/// Named approval states accepted by upstream tool approval configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolApprovalStatusKind {
    /// The tool call does not require approval.
    NotApplicable,

    /// The tool call has been approved.
    Approved,

    /// The tool call has been denied.
    Denied,

    /// The tool call requires explicit user approval.
    UserApproval,
}

/// Normalized object form of an upstream tool approval status.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NormalizedToolApprovalStatus {
    /// The tool call does not require approval.
    NotApplicable,

    /// The tool call has been approved, optionally with a reason.
    Approved {
        /// Optional reason for the approval.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// The tool call has been denied, optionally with a reason.
    Denied {
        /// Optional reason for the denial.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// The tool call requires explicit user approval.
    UserApproval,
}

impl NormalizedToolApprovalStatus {
    /// Creates an approved status without a reason.
    pub const fn approved() -> Self {
        Self::Approved { reason: None }
    }

    /// Creates an approved status with a reason.
    pub fn approved_with_reason(reason: impl Into<String>) -> Self {
        Self::Approved {
            reason: Some(reason.into()),
        }
    }

    /// Creates a denied status without a reason.
    pub const fn denied() -> Self {
        Self::Denied { reason: None }
    }

    /// Creates a denied status with a reason.
    pub fn denied_with_reason(reason: impl Into<String>) -> Self {
        Self::Denied {
            reason: Some(reason.into()),
        }
    }

    /// Returns the optional reason for approved or denied statuses.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Approved { reason } | Self::Denied { reason } => reason.as_deref(),
            Self::NotApplicable | Self::UserApproval => None,
        }
    }
}

/// Upstream tool approval status input.
///
/// Upstream accepts either a string status such as `"approved"` or an object
/// status such as `{ "type": "denied", "reason": "policy" }`. Rust callers
/// can normalize both forms with [`normalize_tool_approval_status`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ToolApprovalStatus {
    /// String approval status form.
    Kind(ToolApprovalStatusKind),

    /// Object approval status form.
    Object(NormalizedToolApprovalStatus),
}

impl ToolApprovalStatus {
    /// Converts this status into the normalized object form used by upstream resolution.
    pub fn normalized(self) -> NormalizedToolApprovalStatus {
        match self {
            Self::Kind(kind) => kind.into(),
            Self::Object(status) => status,
        }
    }
}

impl From<ToolApprovalStatusKind> for ToolApprovalStatus {
    fn from(kind: ToolApprovalStatusKind) -> Self {
        Self::Kind(kind)
    }
}

impl From<NormalizedToolApprovalStatus> for ToolApprovalStatus {
    fn from(status: NormalizedToolApprovalStatus) -> Self {
        Self::Object(status)
    }
}

impl From<ToolApprovalStatusKind> for NormalizedToolApprovalStatus {
    fn from(kind: ToolApprovalStatusKind) -> Self {
        match kind {
            ToolApprovalStatusKind::NotApplicable => Self::NotApplicable,
            ToolApprovalStatusKind::Approved => Self::approved(),
            ToolApprovalStatusKind::Denied => Self::denied(),
            ToolApprovalStatusKind::UserApproval => Self::UserApproval,
        }
    }
}

/// Normalizes an optional upstream approval status to its object form.
pub fn normalize_tool_approval_status(
    status: Option<ToolApprovalStatus>,
) -> NormalizedToolApprovalStatus {
    status.map_or(
        NormalizedToolApprovalStatus::NotApplicable,
        ToolApprovalStatus::normalized,
    )
}

/// Future returned by a generate-text approval callback.
pub type ToolApprovalFuture = Pin<Box<dyn Future<Output = Option<ToolApprovalStatus>> + Send>>;

/// Options passed to a per-tool approval callback.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleToolApprovalOptions {
    /// Identifier of the model tool call being approved.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool-specific context configured for the called tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_context: Option<JsonValue>,

    /// User-defined runtime context for the generation.
    pub runtime_context: JsonObject,
}

impl SingleToolApprovalOptions {
    /// Creates per-tool approval callback options.
    pub fn new(tool_call_id: impl Into<String>, messages: LanguageModelPrompt) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
            tool_context: None,
            runtime_context: JsonObject::new(),
        }
    }

    /// Sets tool-specific context for the approval callback.
    pub fn with_tool_context(mut self, tool_context: impl Into<JsonValue>) -> Self {
        self.tool_context = Some(tool_context.into());
        self
    }

    /// Sets runtime context for the approval callback.
    pub fn with_runtime_context(mut self, runtime_context: JsonObject) -> Self {
        self.runtime_context = runtime_context;
        self
    }
}

/// Options passed to a generic approval callback.
#[derive(Clone, Debug)]
pub struct GenericToolApprovalOptions {
    /// Valid high-level tool call whose approval status should be resolved.
    pub tool_call: GenerateTextToolCall,

    /// Tools available to the model for the step.
    pub tools: Option<Vec<Tool>>,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool-specific context keyed by tool name.
    pub tools_context: JsonObject,

    /// User-defined runtime context for the generation.
    pub runtime_context: JsonObject,
}

impl GenericToolApprovalOptions {
    /// Creates generic approval callback options.
    pub fn new(tool_call: GenerateTextToolCall) -> Self {
        Self {
            tool_call,
            tools: None,
            messages: Vec::new(),
            tools_context: JsonObject::new(),
            runtime_context: JsonObject::new(),
        }
    }

    /// Sets the tools available to the model for the step.
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.tools = Some(tools.into_iter().collect());
        self
    }

    /// Sets prompt messages for the approval callback.
    pub fn with_messages(mut self, messages: LanguageModelPrompt) -> Self {
        self.messages = messages;
        self
    }

    /// Sets tool-specific context for the approval callback.
    pub fn with_tools_context(mut self, tools_context: JsonObject) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets runtime context for the approval callback.
    pub fn with_runtime_context(mut self, runtime_context: JsonObject) -> Self {
        self.runtime_context = runtime_context;
        self
    }
}

/// Function that resolves approval for one named tool.
pub type SingleToolApprovalFunction =
    dyn Fn(JsonValue, SingleToolApprovalOptions) -> ToolApprovalFuture + Send + Sync + 'static;

/// Function that resolves approval for any valid tool call.
pub type GenericToolApprovalFunction =
    dyn Fn(GenericToolApprovalOptions) -> ToolApprovalFuture + Send + Sync + 'static;

/// Per-tool or callback approval configuration for high-level generate-text calls.
///
/// Upstream `toolApproval` accepts either a generic callback or a per-tool map.
/// Rust preserves the JSON/data per-tool status shape and also supports
/// callback entries that run at generation time.
#[derive(Clone, Default)]
pub struct ToolApprovalConfiguration {
    tool_statuses: BTreeMap<String, ToolApprovalStatus>,
    tool_callbacks: BTreeMap<String, Arc<SingleToolApprovalFunction>>,
    generic_callback: Option<Arc<GenericToolApprovalFunction>>,
}

impl ToolApprovalConfiguration {
    /// Creates an empty approval configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an approval configuration from tool-name statuses.
    pub fn from_statuses(
        statuses: impl IntoIterator<Item = (impl Into<String>, ToolApprovalStatus)>,
    ) -> Self {
        Self {
            tool_statuses: statuses
                .into_iter()
                .map(|(tool_name, status)| (tool_name.into(), status))
                .collect(),
            tool_callbacks: BTreeMap::new(),
            generic_callback: None,
        }
    }

    /// Adds or replaces the approval status for one tool.
    pub fn with_tool_status(
        mut self,
        tool_name: impl Into<String>,
        status: impl Into<ToolApprovalStatus>,
    ) -> Self {
        let tool_name = tool_name.into();
        self.tool_callbacks.remove(&tool_name);
        self.tool_statuses.insert(tool_name, status.into());
        self
    }

    /// Adds or replaces the approval callback for one tool.
    pub fn with_tool_approval_function<F, Fut>(
        mut self,
        tool_name: impl Into<String>,
        approve: F,
    ) -> Self
    where
        F: Fn(JsonValue, SingleToolApprovalOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<ToolApprovalStatus>> + Send + 'static,
    {
        let tool_name = tool_name.into();
        self.tool_statuses.remove(&tool_name);
        self.tool_callbacks.insert(
            tool_name,
            Arc::new(move |input, options| Box::pin(approve(input, options))),
        );
        self
    }

    /// Sets a generic approval callback that is called for all valid tool calls.
    ///
    /// When present, this callback takes precedence over per-tool static
    /// statuses and per-tool callbacks, matching upstream's function-form
    /// `toolApproval` behavior.
    pub fn with_generic_tool_approval<F, Fut>(mut self, approve: F) -> Self
    where
        F: Fn(GenericToolApprovalOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<ToolApprovalStatus>> + Send + 'static,
    {
        self.generic_callback = Some(Arc::new(move |options| Box::pin(approve(options))));
        self
    }

    /// Inserts or replaces the approval status for one tool.
    pub fn insert_tool_status(
        &mut self,
        tool_name: impl Into<String>,
        status: impl Into<ToolApprovalStatus>,
    ) -> Option<ToolApprovalStatus> {
        let tool_name = tool_name.into();
        self.tool_callbacks.remove(&tool_name);
        self.tool_statuses.insert(tool_name, status.into())
    }

    /// Returns the configured approval status for one tool.
    pub fn tool_status(&self, tool_name: &str) -> Option<&ToolApprovalStatus> {
        self.tool_statuses.get(tool_name)
    }

    /// Returns whether the named tool has a configured approval callback.
    pub fn has_tool_approval_function(&self, tool_name: &str) -> bool {
        self.tool_callbacks.contains_key(tool_name)
    }

    /// Returns whether a generic approval callback is configured.
    pub fn has_generic_tool_approval(&self) -> bool {
        self.generic_callback.is_some()
    }

    /// Returns the configured tool-status map.
    pub fn tool_statuses(&self) -> &BTreeMap<String, ToolApprovalStatus> {
        &self.tool_statuses
    }

    /// Returns whether this configuration has no per-tool statuses.
    pub fn is_empty(&self) -> bool {
        self.tool_statuses.is_empty()
            && self.tool_callbacks.is_empty()
            && self.generic_callback.is_none()
    }
}

impl fmt::Debug for ToolApprovalConfiguration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolApprovalConfiguration")
            .field("tool_statuses", &self.tool_statuses)
            .field(
                "tool_callback_names",
                &self.tool_callbacks.keys().collect::<Vec<_>>(),
            )
            .field(
                "has_generic_callback",
                &self
                    .generic_callback
                    .as_ref()
                    .map(|_| true)
                    .unwrap_or(false),
            )
            .finish()
    }
}

impl PartialEq for ToolApprovalConfiguration {
    fn eq(&self, other: &Self) -> bool {
        self.tool_statuses == other.tool_statuses
            && self
                .generic_callback
                .as_ref()
                .zip(other.generic_callback.as_ref())
                .map_or(
                    self.generic_callback.is_none() && other.generic_callback.is_none(),
                    |(left, right)| Arc::ptr_eq(left, right),
                )
            && self.tool_callbacks.len() == other.tool_callbacks.len()
            && self.tool_callbacks.iter().all(|(name, callback)| {
                other
                    .tool_callbacks
                    .get(name)
                    .is_some_and(|other_callback| Arc::ptr_eq(callback, other_callback))
            })
    }
}

impl Eq for ToolApprovalConfiguration {}

impl Serialize for ToolApprovalConfiguration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.generic_callback.is_some() || !self.tool_callbacks.is_empty() {
            return Err(<S::Error as serde::ser::Error>::custom(
                "tool approval callbacks cannot be serialized",
            ));
        }

        self.tool_statuses.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ToolApprovalConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self {
            tool_statuses: BTreeMap::deserialize(deserializer)?,
            tool_callbacks: BTreeMap::new(),
            generic_callback: None,
        })
    }
}

/// Inputs used by [`resolve_tool_approval`].
#[derive(Clone, Copy, Debug)]
pub struct ResolveToolApprovalOptions<'a> {
    /// Tools available to the model call.
    pub tools: Option<&'a [Tool]>,

    /// Valid tool call whose approval status should be resolved.
    pub tool_call: &'a GenerateTextToolCall,

    /// User-defined generate-text approval configuration.
    pub tool_approval: Option<&'a ToolApprovalConfiguration>,

    /// Messages sent to the model for the step that produced this tool call.
    pub messages: Option<&'a LanguageModelPrompt>,

    /// Tool-specific context keyed by tool name.
    pub tools_context: Option<&'a JsonObject>,

    /// User-defined runtime context for the generation.
    pub runtime_context: Option<&'a JsonObject>,
}

impl<'a> ResolveToolApprovalOptions<'a> {
    /// Creates resolve options for a tool call.
    pub fn new(tool_call: &'a GenerateTextToolCall) -> Self {
        Self {
            tools: None,
            tool_call,
            tool_approval: None,
            messages: None,
            tools_context: None,
            runtime_context: None,
        }
    }

    /// Sets the available tools.
    pub fn with_tools(mut self, tools: &'a [Tool]) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Sets the user-defined approval configuration.
    pub fn with_tool_approval(mut self, tool_approval: &'a ToolApprovalConfiguration) -> Self {
        self.tool_approval = Some(tool_approval);
        self
    }

    /// Sets the prompt messages sent to the model for the step.
    pub fn with_messages(mut self, messages: &'a LanguageModelPrompt) -> Self {
        self.messages = Some(messages);
        self
    }

    /// Sets the tool contexts available for the generation.
    pub fn with_tools_context(mut self, tools_context: &'a JsonObject) -> Self {
        self.tools_context = Some(tools_context);
        self
    }

    /// Sets the runtime context available for the generation.
    pub fn with_runtime_context(mut self, runtime_context: &'a JsonObject) -> Self {
        self.runtime_context = Some(runtime_context);
        self
    }

    fn with_optional_tool_approval(
        mut self,
        tool_approval: Option<&'a ToolApprovalConfiguration>,
    ) -> Self {
        self.tool_approval = tool_approval;
        self
    }
}

/// Resolves the approval status for a valid tool call.
///
/// This mirrors upstream `resolveToolApproval`: a generic generate-text
/// callback wins first, then per-tool generate-text approval configuration,
/// then tool-defined boolean approval, and missing configuration normalizes to
/// `not-applicable`.
pub async fn resolve_tool_approval(
    options: ResolveToolApprovalOptions<'_>,
) -> NormalizedToolApprovalStatus {
    if let Some(configuration) = options.tool_approval {
        if let Some(approve) = &configuration.generic_callback {
            return normalize_tool_approval_status(
                approve(GenericToolApprovalOptions {
                    tool_call: options.tool_call.clone(),
                    tools: options.tools.map(|tools| tools.to_vec()),
                    messages: options.messages.cloned().unwrap_or_default(),
                    tools_context: options.tools_context.cloned().unwrap_or_default(),
                    runtime_context: options.runtime_context.cloned().unwrap_or_default(),
                })
                .await,
            );
        }

        if let Some(status) = configuration.tool_status(&options.tool_call.tool_name) {
            return normalize_tool_approval_status(Some(status.clone()));
        }

        if let Some(approve) = configuration
            .tool_callbacks
            .get(&options.tool_call.tool_name)
        {
            let tool_context = options
                .tools_context
                .and_then(|tools_context| tools_context.get(&options.tool_call.tool_name))
                .cloned();

            return normalize_tool_approval_status(
                approve(
                    options.tool_call.input.clone(),
                    SingleToolApprovalOptions {
                        tool_call_id: options.tool_call.tool_call_id.clone(),
                        messages: options.messages.cloned().unwrap_or_default(),
                        tool_context,
                        runtime_context: options.runtime_context.cloned().unwrap_or_default(),
                    },
                )
                .await,
            );
        }
    }

    if let Some(tool) = options.tools.and_then(|tools| {
        tools
            .iter()
            .find(|tool| tool.name == options.tool_call.tool_name)
    }) {
        let context = options
            .tools_context
            .and_then(|tools_context| tools_context.get(&options.tool_call.tool_name))
            .cloned();
        let mut needs_approval_options = ToolNeedsApprovalOptions::new(
            options.tool_call.tool_call_id.clone(),
            options.messages.cloned().unwrap_or_default(),
        );
        if let Some(context) = context {
            needs_approval_options = needs_approval_options.with_context(context);
        }

        if let Some(needs_approval) =
            tool.resolve_needs_approval(options.tool_call.input.clone(), needs_approval_options)
        {
            return if needs_approval.await {
                NormalizedToolApprovalStatus::UserApproval
            } else {
                NormalizedToolApprovalStatus::NotApplicable
            };
        }
    }

    NormalizedToolApprovalStatus::NotApplicable
}

/// Error returned when a model tries to call a tool that is not available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoSuchToolError {
    tool_name: String,
    available_tools: Option<Vec<String>>,
    message: String,
}

impl NoSuchToolError {
    /// Creates an unavailable-tool error when no tool list was available.
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self::from_available_tools(tool_name, None)
    }

    /// Creates an unavailable-tool error with the known available tools.
    pub fn with_available_tools(
        tool_name: impl Into<String>,
        available_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let available_tools = available_tools
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();

        Self::from_available_tools(tool_name, Some(available_tools))
    }

    /// Creates an unavailable-tool error with a caller-supplied message.
    pub fn with_message(
        tool_name: impl Into<String>,
        available_tools: Option<Vec<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            available_tools,
            message: message.into(),
        }
    }

    fn from_available_tools(
        tool_name: impl Into<String>,
        available_tools: Option<Vec<String>>,
    ) -> Self {
        let tool_name = tool_name.into();
        let message = no_such_tool_default_message(&tool_name, available_tools.as_deref());

        Self {
            tool_name,
            available_tools,
            message,
        }
    }

    /// Returns the missing tool name.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Returns the available tools when the caller had a concrete tool list.
    pub fn available_tools(&self) -> Option<&[String]> {
        self.available_tools.as_deref()
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its parts.
    pub fn into_parts(self) -> (String, Option<Vec<String>>, String) {
        (self.tool_name, self.available_tools, self.message)
    }
}

impl fmt::Display for NoSuchToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoSuchToolError {}

/// Error returned when a model supplies invalid input for a tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidToolInputError {
    tool_name: String,
    tool_input: String,
    cause_message: String,
    message: String,
}

impl InvalidToolInputError {
    /// Creates an invalid-tool-input error with the upstream default message.
    pub fn new(
        tool_name: impl Into<String>,
        tool_input: impl Into<String>,
        cause: impl fmt::Display,
    ) -> Self {
        let tool_name = tool_name.into();
        let tool_input = tool_input.into();
        let cause_message = cause.to_string();
        let message = invalid_tool_input_default_message(&tool_name, &cause_message);

        Self {
            tool_name,
            tool_input,
            cause_message,
            message,
        }
    }

    /// Creates an invalid-tool-input error with a caller-supplied message.
    pub fn with_message(
        tool_name: impl Into<String>,
        tool_input: impl Into<String>,
        cause: impl fmt::Display,
        message: impl Into<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_input: tool_input.into(),
            cause_message: cause.to_string(),
            message: message.into(),
        }
    }

    /// Returns the tool name whose input was invalid.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Returns the raw tool input that failed parsing or validation.
    pub fn tool_input(&self) -> &str {
        &self.tool_input
    }

    /// Returns the retained cause message.
    pub fn cause_message(&self) -> &str {
        &self.cause_message
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its parts.
    pub fn into_parts(self) -> (String, String, String, String) {
        (
            self.tool_name,
            self.tool_input,
            self.cause_message,
            self.message,
        )
    }
}

impl fmt::Display for InvalidToolInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidToolInputError {}

/// Original tool-call parsing error that a repair attempt tried to fix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolCallRepairOriginalError {
    /// The model tried to call a tool that was not available.
    NoSuchTool(NoSuchToolError),

    /// The model supplied invalid input for an available tool.
    InvalidToolInput(InvalidToolInputError),
}

impl From<NoSuchToolError> for ToolCallRepairOriginalError {
    fn from(error: NoSuchToolError) -> Self {
        Self::NoSuchTool(error)
    }
}

impl From<InvalidToolInputError> for ToolCallRepairOriginalError {
    fn from(error: InvalidToolInputError) -> Self {
        Self::InvalidToolInput(error)
    }
}

impl fmt::Display for ToolCallRepairOriginalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTool(error) => error.fmt(formatter),
            Self::InvalidToolInput(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ToolCallRepairOriginalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoSuchTool(error) => Some(error),
            Self::InvalidToolInput(error) => Some(error),
        }
    }
}

/// Error returned when repairing an invalid tool call fails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallRepairError {
    original_error: ToolCallRepairOriginalError,
    cause_message: String,
    message: String,
}

impl ToolCallRepairError {
    /// Creates a tool-call repair error with the upstream default message.
    pub fn new(
        original_error: impl Into<ToolCallRepairOriginalError>,
        cause: impl fmt::Display,
    ) -> Self {
        let cause_message = get_error_message(Some(&cause));
        let message = tool_call_repair_default_message(&cause_message);

        Self {
            original_error: original_error.into(),
            cause_message,
            message,
        }
    }

    /// Creates a tool-call repair error with a caller-supplied message.
    pub fn with_message(
        original_error: impl Into<ToolCallRepairOriginalError>,
        cause: impl fmt::Display,
        message: impl Into<String>,
    ) -> Self {
        Self {
            original_error: original_error.into(),
            cause_message: get_error_message(Some(&cause)),
            message: message.into(),
        }
    }

    /// Returns the original tool-call parsing error that triggered repair.
    pub fn original_error(&self) -> &ToolCallRepairOriginalError {
        &self.original_error
    }

    /// Returns the retained repair failure cause message.
    pub fn cause_message(&self) -> &str {
        &self.cause_message
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its parts.
    pub fn into_parts(self) -> (ToolCallRepairOriginalError, String, String) {
        (self.original_error, self.cause_message, self.message)
    }
}

impl fmt::Display for ToolCallRepairError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolCallRepairError {}

/// Error returned when tool call results are missing from a prompt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingToolResultsError {
    tool_call_ids: Vec<String>,
    message: String,
}

impl MissingToolResultsError {
    /// Creates a missing-tool-results error with the upstream default message.
    pub fn new(tool_call_ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let tool_call_ids = tool_call_ids
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let message = missing_tool_results_default_message(&tool_call_ids);

        Self {
            tool_call_ids,
            message,
        }
    }

    /// Returns the tool call IDs whose results were missing.
    pub fn tool_call_ids(&self) -> &[String] {
        &self.tool_call_ids
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its parts.
    pub fn into_parts(self) -> (Vec<String>, String) {
        (self.tool_call_ids, self.message)
    }
}

impl fmt::Display for MissingToolResultsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MissingToolResultsError {}

/// Error returned when a tool approval response references an unknown approval request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidToolApprovalError {
    approval_id: String,
    message: String,
}

impl InvalidToolApprovalError {
    /// Creates an invalid-tool-approval error with the upstream default message.
    pub fn new(approval_id: impl Into<String>) -> Self {
        let approval_id = approval_id.into();
        let message = invalid_tool_approval_default_message(&approval_id);

        Self {
            approval_id,
            message,
        }
    }

    /// Returns the unknown approval request ID.
    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its parts.
    pub fn into_parts(self) -> (String, String) {
        (self.approval_id, self.message)
    }
}

impl fmt::Display for InvalidToolApprovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidToolApprovalError {}

/// Error returned when an approval request references an unknown tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallNotFoundForApprovalError {
    tool_call_id: String,
    approval_id: String,
    message: String,
}

impl ToolCallNotFoundForApprovalError {
    /// Creates a missing-tool-call-for-approval error with the upstream default message.
    pub fn new(tool_call_id: impl Into<String>, approval_id: impl Into<String>) -> Self {
        let tool_call_id = tool_call_id.into();
        let approval_id = approval_id.into();
        let message = tool_call_not_found_for_approval_default_message(&tool_call_id, &approval_id);

        Self {
            tool_call_id,
            approval_id,
            message,
        }
    }

    /// Returns the missing tool call ID.
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    /// Returns the approval request ID.
    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its parts.
    pub fn into_parts(self) -> (String, String, String) {
        (self.tool_call_id, self.approval_id, self.message)
    }
}

impl fmt::Display for ToolCallNotFoundForApprovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolCallNotFoundForApprovalError {}

/// Error returned when a high-level generation result has no output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoOutputGeneratedError {
    message: String,
}

impl NoOutputGeneratedError {
    /// Creates a no-output error with the upstream default message.
    pub fn new() -> Self {
        Self {
            message: "No output generated.".to_string(),
        }
    }

    /// Creates a no-output error with a caller-supplied message.
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the human-readable error message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl Default for NoOutputGeneratedError {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NoOutputGeneratedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoOutputGeneratedError {}

/// Error returned when high-level object generation produces no object.
///
/// Upstream uses this for responses that are missing text, cannot be parsed, or
/// fail schema validation. The Rust contract retains the generated text when it
/// exists, response metadata, usage, finish reason, and an optional cause
/// message for parse or validation failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoObjectGeneratedError {
    message: String,
    cause_message: Option<String>,
    text: Option<String>,
    response: LanguageModelResponse,
    usage: LanguageModelUsage,
    finish_reason: FinishReason,
}

impl NoObjectGeneratedError {
    /// Creates a no-object error with the upstream default message.
    pub fn new(
        response: LanguageModelResponse,
        usage: LanguageModelUsage,
        finish_reason: FinishReason,
    ) -> Self {
        Self {
            message: "No object generated.".to_string(),
            cause_message: None,
            text: None,
            response,
            usage,
            finish_reason,
        }
    }

    /// Creates a no-object error with a caller-supplied message.
    pub fn with_message(
        message: impl Into<String>,
        response: LanguageModelResponse,
        usage: LanguageModelUsage,
        finish_reason: FinishReason,
    ) -> Self {
        Self {
            message: message.into(),
            cause_message: None,
            text: None,
            response,
            usage,
            finish_reason,
        }
    }

    /// Adds the generated text that failed parsing or validation.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Adds the parse or validation failure cause message.
    pub fn with_cause(mut self, cause: impl fmt::Display) -> Self {
        self.cause_message = Some(get_error_message(Some(&cause)));
        self
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the parse or validation cause message, when available.
    pub fn cause_message(&self) -> Option<&str> {
        self.cause_message.as_deref()
    }

    /// Returns the generated text that failed parsing or validation, when available.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Returns response metadata for the model call.
    pub fn response(&self) -> &LanguageModelResponse {
        &self.response
    }

    /// Returns usage reported for the model call.
    pub fn usage(&self) -> &LanguageModelUsage {
        &self.usage
    }

    /// Returns the unified finish reason for the model call.
    pub fn finish_reason(&self) -> &FinishReason {
        &self.finish_reason
    }

    /// Converts this error into its retained parts.
    pub fn into_parts(
        self,
    ) -> (
        String,
        Option<String>,
        Option<String>,
        LanguageModelResponse,
        LanguageModelUsage,
        FinishReason,
    ) {
        (
            self.message,
            self.cause_message,
            self.text,
            self.response,
            self.usage,
            self.finish_reason,
        )
    }
}

impl fmt::Display for NoObjectGeneratedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoObjectGeneratedError {}

/// Error returned when a language model stream emits an invalid part.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidStreamPartError {
    chunk: LanguageModelStreamPart,
    message: String,
}

impl InvalidStreamPartError {
    /// Creates an invalid-stream-part error with the offending stream chunk and message.
    pub fn new(chunk: LanguageModelStreamPart, message: impl Into<String>) -> Self {
        Self {
            chunk,
            message: message.into(),
        }
    }

    /// Returns the stream chunk that caused the error.
    pub fn chunk(&self) -> &LanguageModelStreamPart {
        &self.chunk
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained chunk and message.
    pub fn into_parts(self) -> (LanguageModelStreamPart, String) {
        (self.chunk, self.message)
    }
}

impl fmt::Display for InvalidStreamPartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidStreamPartError {}

/// Error returned when a UI message stream emits an invalid or out-of-sequence chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiMessageStreamError {
    chunk_type: String,
    chunk_id: String,
    message: String,
}

impl UiMessageStreamError {
    /// Creates a UI message stream error with the failing chunk context and message.
    pub fn new(
        chunk_type: impl Into<String>,
        chunk_id: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            chunk_type: chunk_type.into(),
            chunk_id: chunk_id.into(),
            message: message.into(),
        }
    }

    /// Returns the type of stream chunk that caused the error.
    pub fn chunk_type(&self) -> &str {
        &self.chunk_type
    }

    /// Returns the part ID or tool call ID associated with the failing chunk.
    pub fn chunk_id(&self) -> &str {
        &self.chunk_id
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained chunk context and message.
    pub fn into_parts(self) -> (String, String, String) {
        (self.chunk_type, self.chunk_id, self.message)
    }
}

impl fmt::Display for UiMessageStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for UiMessageStreamError {}

/// Error returned when a high-level API receives an unsupported model version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedModelVersionError {
    version: String,
    provider: String,
    model_id: String,
    message: String,
}

impl UnsupportedModelVersionError {
    /// Creates an unsupported-model-version error with the upstream default message.
    pub fn new(
        version: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        let version = version.into();
        let provider = provider.into();
        let model_id = model_id.into();
        let message = unsupported_model_version_default_message(&version, &provider, &model_id);

        Self {
            version,
            provider,
            model_id,
            message,
        }
    }

    /// Returns the unsupported specification version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the model provider.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the model ID.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained parts.
    pub fn into_parts(self) -> (String, String, String, String) {
        (self.version, self.provider, self.model_id, self.message)
    }
}

impl fmt::Display for UnsupportedModelVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for UnsupportedModelVersionError {}

/// A file generated by a high-level text generation call.
///
/// This is the Rust-native equivalent of upstream `GeneratedFile` and
/// `DefaultGeneratedFile`: callers can construct it from base64 text or raw
/// bytes and access either representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedFile {
    media_type: String,
    data: FileDataContent,
}

impl GeneratedFile {
    /// Creates a generated file from base64-encoded file data.
    pub fn from_base64(media_type: impl Into<String>, base64: impl Into<String>) -> Self {
        Self {
            media_type: media_type.into(),
            data: FileDataContent::Base64(base64.into()),
        }
    }

    /// Creates a generated file from raw bytes.
    pub fn from_bytes(media_type: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            media_type: media_type.into(),
            data: FileDataContent::Bytes(bytes.into()),
        }
    }

    /// Creates a generated file from existing file-data content.
    pub fn new(media_type: impl Into<String>, data: FileDataContent) -> Self {
        Self {
            media_type: media_type.into(),
            data,
        }
    }

    /// Converts a provider-v4 generated file content part into a high-level file.
    pub fn from_language_model_file(file: &LanguageModelFile) -> Self {
        match &file.data {
            LanguageModelFileData::Data { data } => {
                Self::new(file.media_type.clone(), data.clone())
            }
            LanguageModelFileData::Url { url } => {
                Self::from_base64(file.media_type.clone(), url.to_string())
            }
        }
    }

    /// Returns the IANA media type of the generated file.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// Returns the generated file as base64-encoded data.
    pub fn base64(&self) -> String {
        match &self.data {
            FileDataContent::Base64(base64) => base64.clone(),
            FileDataContent::Bytes(bytes) => convert_bytes_to_base64(bytes),
        }
    }

    /// Returns the generated file as raw bytes.
    pub fn bytes(&self) -> Result<Vec<u8>, Base64DecodeError> {
        match &self.data {
            FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
            FileDataContent::Base64(base64) => convert_base64_to_bytes(base64),
        }
    }

    /// Upstream-named alias for [`GeneratedFile::bytes`].
    pub fn uint8_array(&self) -> Result<Vec<u8>, Base64DecodeError> {
        self.bytes()
    }

    /// Returns the retained file data representation.
    pub fn data(&self) -> &FileDataContent {
        &self.data
    }

    /// Converts this file into its retained file data representation.
    pub fn into_data(self) -> FileDataContent {
        self.data
    }
}

impl Serialize for GeneratedFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("GeneratedFile", 2)?;
        state.serialize_field("base64", &self.base64())?;
        state.serialize_field("mediaType", &self.media_type)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for GeneratedFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct GeneratedFileFields {
            base64: String,
            media_type: String,
        }

        let file = GeneratedFileFields::deserialize(deserializer)?;
        Ok(Self::from_base64(file.media_type, file.base64))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum GenerateTextFileContentKind {
    #[serde(rename = "file")]
    File,
}

/// High-level generated file content of a text generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextFileContent {
    #[serde(rename = "type")]
    kind: GenerateTextFileContentKind,

    /// The generated file.
    pub file: GeneratedFile,

    /// Optional provider-specific metadata for the generated file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl GenerateTextFileContent {
    /// Creates high-level generated file content.
    pub fn new(file: GeneratedFile) -> Self {
        Self {
            kind: GenerateTextFileContentKind::File,
            file,
            provider_metadata: None,
        }
    }

    /// Converts a provider-v4 generated file part into high-level content.
    pub fn from_language_model_file(file: &LanguageModelFile) -> Self {
        Self {
            kind: GenerateTextFileContentKind::File,
            file: GeneratedFile::from_language_model_file(file),
            provider_metadata: file.provider_metadata.clone(),
        }
    }

    /// Adds provider-specific metadata to this file content.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ReasoningOutputKind {
    #[serde(rename = "reasoning")]
    Reasoning,
}

/// High-level reasoning output of a text generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningOutput {
    #[serde(rename = "type")]
    kind: ReasoningOutputKind,

    /// The reasoning text.
    pub text: String,

    /// Optional provider-specific metadata for the reasoning output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ReasoningOutput {
    /// Creates a high-level reasoning output.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: ReasoningOutputKind::Reasoning,
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Converts a provider-v4 reasoning part into high-level reasoning output.
    pub fn from_language_model_reasoning(reasoning: &LanguageModelReasoning) -> Self {
        Self {
            kind: ReasoningOutputKind::Reasoning,
            text: reasoning.text.clone(),
            provider_metadata: reasoning.provider_metadata.clone(),
        }
    }

    /// Adds provider-specific metadata to this reasoning output.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ReasoningFileOutputKind {
    #[serde(rename = "reasoning-file")]
    ReasoningFile,
}

/// High-level reasoning file output of a text generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningFileOutput {
    #[serde(rename = "type")]
    kind: ReasoningFileOutputKind,

    /// The generated reasoning file.
    pub file: GeneratedFile,

    /// Optional provider-specific metadata for the reasoning file output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ReasoningFileOutput {
    /// Creates a high-level reasoning file output.
    pub fn new(file: GeneratedFile) -> Self {
        Self {
            kind: ReasoningFileOutputKind::ReasoningFile,
            file,
            provider_metadata: None,
        }
    }

    /// Converts a provider-v4 reasoning-file part into high-level reasoning output.
    pub fn from_language_model_reasoning_file(file: &LanguageModelReasoningFile) -> Self {
        let generated_file = match &file.data {
            LanguageModelFileData::Data { data } => {
                GeneratedFile::new(file.media_type.clone(), data.clone())
            }
            LanguageModelFileData::Url { url } => {
                GeneratedFile::from_base64(file.media_type.clone(), url.to_string())
            }
        };

        Self {
            kind: ReasoningFileOutputKind::ReasoningFile,
            file: generated_file,
            provider_metadata: file.provider_metadata.clone(),
        }
    }

    /// Adds provider-specific metadata to this reasoning file output.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// Reasoning content emitted during a high-level generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GenerateTextReasoning {
    /// Text reasoning emitted by the model.
    Reasoning(ReasoningOutput),

    /// File emitted by the model as part of reasoning.
    ReasoningFile(ReasoningFileOutput),
}

/// Upstream class name for generated files.
pub type DefaultGeneratedFile = GeneratedFile;

/// Backwards-compatible upstream generated-image alias.
pub type ExperimentalGeneratedImage = GeneratedFile;

/// Tool input accepted by [`GenerateTextOptions::with_tool`].
#[derive(Clone, Debug)]
pub enum GenerateTextTool {
    /// High-level Rust function tool.
    Rust(Box<Tool>),

    /// Already prepared provider-facing language model tool.
    LanguageModel(LanguageModelTool),
}

impl From<Tool> for GenerateTextTool {
    fn from(tool: Tool) -> Self {
        Self::Rust(Box::new(tool))
    }
}

impl From<LanguageModelTool> for GenerateTextTool {
    fn from(tool: LanguageModelTool) -> Self {
        Self::LanguageModel(tool)
    }
}

/// Settings controlling which large provider payloads are retained in step results.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextInclude {
    /// Whether to retain the request HTTP body in step results.
    #[serde(default, skip_serializing_if = "is_false")]
    pub request_body: bool,

    /// Whether to retain request messages in step results.
    #[serde(default, skip_serializing_if = "is_false")]
    pub request_messages: bool,

    /// Whether to retain the response HTTP body in step results.
    #[serde(default, skip_serializing_if = "is_false")]
    pub response_body: bool,
}

impl GenerateTextInclude {
    /// Creates include settings with all optional payload retention disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to retain provider request bodies in step results.
    pub fn with_request_body(mut self, request_body: bool) -> Self {
        self.request_body = request_body;
        self
    }

    /// Sets whether to retain request messages in step results.
    pub fn with_request_messages(mut self, request_messages: bool) -> Self {
        self.request_messages = request_messages;
        self
    }

    /// Sets whether to retain provider response bodies in step results.
    pub fn with_response_body(mut self, response_body: bool) -> Self {
        self.response_body = response_body;
        self
    }
}

/// Options for a high-level non-streaming text generation call.
#[derive(Debug)]
pub struct GenerateTextOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for the generation.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,

    /// High-level Rust tools made available to the model.
    pub tools: Vec<Tool>,

    /// User-defined runtime context attached to every generated step.
    pub runtime_context: JsonObject,

    /// Tool-specific context keyed by tool name.
    pub tools_context: JsonObject,

    /// Experimental sandbox environment passed through to Rust tool execution.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,

    /// Optional active tool names used to restrict the available tool set.
    pub active_tools: ActiveTools,

    /// Static approval configuration for tool calls.
    pub tool_approval: Option<ToolApprovalConfiguration>,

    /// Per-tool input refinements applied after parsing valid tool calls.
    pub tool_input_refinements: BTreeMap<String, ToolInputRefinement>,

    /// Optional callback used to repair invalid model tool calls before execution.
    pub tool_call_repair: Option<ToolCallRepair>,

    /// Optional per-step preparation callback.
    pub prepare_step: Option<PrepareStep<'a, M>>,

    /// Optional callback invoked before any model work begins.
    pub on_start: Option<GenerateTextOnStart<'a>>,

    /// Optional callback invoked before each model step begins.
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,

    /// Optional callback invoked immediately before each provider model call begins.
    pub on_language_model_call_start: Option<GenerateTextOnLanguageModelCallStart<'a>>,

    /// Optional callback invoked after each provider model call completes.
    pub on_language_model_call_end: Option<GenerateTextOnLanguageModelCallEnd<'a>>,

    /// Optional callback invoked before a Rust tool executor is invoked.
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,

    /// Optional callback invoked after a Rust tool executor completes.
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,

    /// Optional callback invoked after each completed generation step.
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,

    /// Optional callback invoked after the full generation result is complete.
    pub on_finish: Option<GenerateTextOnFinish<'a>>,

    /// Maximum number of model-call steps to run.
    pub max_steps: usize,

    /// Additional stop conditions checked after every completed step.
    pub stop_conditions: Vec<StopCondition>,

    /// Settings controlling which large provider payloads are retained in step results.
    pub include: GenerateTextInclude,
}

impl<'a, M: LanguageModel + ?Sized> GenerateTextOptions<'a, M> {
    /// Creates generation options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self {
            model,
            call_options: LanguageModelCallOptions::new(prompt),
            tools: Vec::new(),
            runtime_context: JsonObject::new(),
            tools_context: JsonObject::new(),
            experimental_sandbox: None,
            active_tools: None,
            tool_approval: None,
            tool_input_refinements: BTreeMap::new(),
            tool_call_repair: None,
            prepare_step: None,
            on_start: None,
            on_step_start: None,
            on_language_model_call_start: None,
            on_language_model_call_end: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            max_steps: DEFAULT_MAX_STEPS,
            stop_conditions: Vec::new(),
            include: GenerateTextInclude::default(),
        }
    }

    /// Creates generation options from the high-level upstream prompt shape.
    ///
    /// This standardizes text prompts and instructions before delegating to
    /// the provider-v4 language model prompt boundary.
    pub fn from_prompt(model: &'a M, prompt: Prompt) -> Result<Self, InvalidPromptError> {
        let prompt = standardize_prompt(prompt)?.into_language_model_prompt();
        Ok(Self::new(model, prompt))
    }

    /// Creates generation options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, call_options: LanguageModelCallOptions) -> Self {
        Self {
            model,
            call_options,
            tools: Vec::new(),
            runtime_context: JsonObject::new(),
            tools_context: JsonObject::new(),
            experimental_sandbox: None,
            active_tools: None,
            tool_approval: None,
            tool_input_refinements: BTreeMap::new(),
            tool_call_repair: None,
            prepare_step: None,
            on_start: None,
            on_step_start: None,
            on_language_model_call_start: None,
            on_language_model_call_end: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            max_steps: DEFAULT_MAX_STEPS,
            stop_conditions: Vec::new(),
            include: GenerateTextInclude::default(),
        }
    }

    /// Sets the maximum number of output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.call_options.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.call_options.temperature = Some(temperature);
        self
    }

    /// Adds a stop sequence.
    pub fn with_stop_sequence(mut self, stop_sequence: impl Into<String>) -> Self {
        self.call_options
            .stop_sequences
            .get_or_insert_with(Vec::new)
            .push(stop_sequence.into());
        self
    }

    /// Sets nucleus sampling.
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.call_options.top_p = Some(top_p);
        self
    }

    /// Sets top-k sampling.
    pub fn with_top_k(mut self, top_k: u64) -> Self {
        self.call_options.top_k = Some(top_k);
        self
    }

    /// Sets the presence penalty.
    pub fn with_presence_penalty(mut self, presence_penalty: f64) -> Self {
        self.call_options.presence_penalty = Some(presence_penalty);
        self
    }

    /// Sets the frequency penalty.
    pub fn with_frequency_penalty(mut self, frequency_penalty: f64) -> Self {
        self.call_options.frequency_penalty = Some(frequency_penalty);
        self
    }

    /// Sets the response format.
    pub fn with_response_format(mut self, response_format: LanguageModelResponseFormat) -> Self {
        self.call_options.response_format = Some(response_format);
        self
    }

    /// Sets the deterministic sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.call_options.seed = Some(seed);
        self
    }

    /// Adds a tool that is available to the model.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        match tool.into() {
            GenerateTextTool::Rust(tool) => self.tools.push(*tool),
            GenerateTextTool::LanguageModel(tool) => self
                .call_options
                .tools
                .get_or_insert_with(Vec::new)
                .push(tool),
        }

        self
    }

    /// Sets the user-defined runtime context attached to every generated step.
    pub fn with_runtime_context(mut self, runtime_context: JsonObject) -> Self {
        self.runtime_context = runtime_context;
        self
    }

    /// Sets the tool-specific context map keyed by tool name.
    pub fn with_tools_context(mut self, tools_context: JsonObject) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets the experimental sandbox available to Rust tool executors.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }

    /// Adds or replaces context for a single tool.
    pub fn with_tool_context(
        mut self,
        tool_name: impl Into<String>,
        context: impl Into<JsonValue>,
    ) -> Self {
        self.tools_context.insert(tool_name.into(), context.into());
        self
    }

    /// Sets the tool selection strategy.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.call_options.tool_choice = Some(tool_choice);
        self
    }

    /// Sets the active tool names for this generation.
    ///
    /// When set, only tools with matching names are sent to the model or
    /// considered for local Rust execution.
    pub fn with_active_tools(
        mut self,
        active_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.active_tools = Some(active_tools.into_iter().map(Into::into).collect());
        self
    }

    /// Sets static approval configuration for tool calls.
    pub fn with_tool_approval(mut self, tool_approval: ToolApprovalConfiguration) -> Self {
        self.tool_approval = Some(tool_approval);
        self
    }

    /// Adds or replaces an input refinement for one tool.
    ///
    /// The refinement runs after the model tool input has been parsed and
    /// before local tool execution, result shaping, and continuation messages.
    pub fn with_tool_input_refinement<F, Fut>(
        mut self,
        tool_name: impl Into<String>,
        refine: F,
    ) -> Self
    where
        F: Fn(JsonValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, ToolInputRefinementError>> + Send + 'static,
    {
        self.tool_input_refinements
            .insert(tool_name.into(), ToolInputRefinement::new(refine));
        self
    }

    /// Sets a callback that can repair unavailable or invalid tool calls.
    ///
    /// The callback receives the original provider tool call, the step's
    /// active high-level tools, the prompt messages sent to the model, and the
    /// original lookup or parse error. Returning `None` keeps the original
    /// invalid tool-call behavior.
    pub fn with_tool_call_repair<F, Fut, E>(mut self, repair: F) -> Self
    where
        F: Fn(ToolCallRepairOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<LanguageModelToolCall>, E>> + Send + 'static,
        E: fmt::Display,
    {
        self.tool_call_repair = Some(ToolCallRepair::new(repair));
        self
    }

    /// Sets a per-step preparation callback.
    ///
    /// The callback can override the same-type model, tool choice, active
    /// tools, prompt messages, contexts, and provider options for each step.
    pub fn with_prepare_step<F, Fut>(mut self, prepare: F) -> Self
    where
        F: Fn(PrepareStepOptions<'a, M>) -> Fut + 'a,
        Fut: Future<Output = PrepareStepResult<'a, M>> + 'a,
    {
        self.prepare_step = Some(PrepareStep::new(prepare));
        self
    }

    /// Sets a callback that is invoked when generation starts before model work.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(GenerateTextStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(GenerateTextOnStart::new(on_start));
        self
    }

    /// Sets a callback that is invoked before every model step.
    pub fn with_on_step_start<F, Fut>(mut self, on_step_start: F) -> Self
    where
        F: Fn(GenerateTextStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_start = Some(GenerateTextOnStepStart::new(on_step_start));
        self
    }

    /// Sets a callback that is invoked immediately before each provider model call begins.
    pub fn with_experimental_on_language_model_call_start<F, Fut>(
        mut self,
        on_language_model_call_start: F,
    ) -> Self
    where
        F: Fn(LanguageModelCallStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_language_model_call_start = Some(GenerateTextOnLanguageModelCallStart::new(
            on_language_model_call_start,
        ));
        self
    }

    /// Sets a callback that is invoked after each provider model call completes.
    pub fn with_experimental_on_language_model_call_end<F, Fut>(
        mut self,
        on_language_model_call_end: F,
    ) -> Self
    where
        F: Fn(LanguageModelCallEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_language_model_call_end = Some(GenerateTextOnLanguageModelCallEnd::new(
            on_language_model_call_end,
        ));
        self
    }

    /// Sets a callback that is invoked before each local Rust tool execution.
    pub fn with_on_tool_execution_start<F, Fut>(mut self, on_tool_execution_start: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_tool_execution_start = Some(GenerateTextOnToolExecutionStart::new(
            on_tool_execution_start,
        ));
        self
    }

    /// Sets a callback that is invoked after each local Rust tool execution completes.
    pub fn with_on_tool_execution_end<F, Fut>(mut self, on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_tool_execution_end =
            Some(GenerateTextOnToolExecutionEnd::new(on_tool_execution_end));
        self
    }

    /// Deprecated upstream alias for [`GenerateTextOptions::with_on_tool_execution_start`].
    pub fn with_experimental_on_tool_call_start<F, Fut>(self, on_tool_execution_start: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_tool_execution_start(on_tool_execution_start)
    }

    /// Deprecated upstream alias for [`GenerateTextOptions::with_on_tool_execution_end`].
    pub fn with_experimental_on_tool_call_finish<F, Fut>(self, on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_tool_execution_end(on_tool_execution_end)
    }

    /// Sets a callback that is invoked after every completed step.
    pub fn with_on_step_finish<F, Fut>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(GenerateTextStep) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_finish = Some(GenerateTextOnStepFinish::new(on_step_finish));
        self
    }

    /// Sets a callback that is invoked after the generation result is complete.
    pub fn with_on_finish<F, Fut>(mut self, on_finish: F) -> Self
    where
        F: Fn(GenerateTextFinishEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_finish = Some(GenerateTextOnFinish::new(on_finish));
        self
    }

    /// Sets the maximum number of model-call steps.
    ///
    /// Values lower than 1 are clamped to one step so every call still invokes
    /// the model at least once.
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps.max(1);
        self
    }

    /// Adds a stop condition that is checked after every completed step.
    pub fn with_stop_condition(mut self, stop_condition: StopCondition) -> Self {
        self.stop_conditions.push(stop_condition);
        self
    }

    /// Replaces the additional stop conditions checked after every completed step.
    pub fn with_stop_conditions(
        mut self,
        stop_conditions: impl IntoIterator<Item = StopCondition>,
    ) -> Self {
        self.stop_conditions = stop_conditions.into_iter().collect();
        self
    }

    /// Sets which provider payloads are retained in step results.
    pub fn with_include(mut self, include: GenerateTextInclude) -> Self {
        self.include = include;
        self
    }

    /// Sets whether raw stream chunks should be included.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.call_options.include_raw_chunks = Some(include_raw_chunks);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.call_options
            .headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the reasoning effort.
    pub fn with_reasoning(mut self, reasoning: LanguageModelReasoningEffort) -> Self {
        self.call_options.reasoning = Some(reasoning);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.call_options.provider_options = Some(provider_options);
        self
    }
}

/// Tool call emitted during a high-level generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextToolCall {
    /// Identifier of the model tool call.
    pub tool_call_id: String,

    /// Name of the tool the model requested.
    pub tool_name: String,

    /// Parsed JSON input for the tool call, or the raw input string when it was
    /// not valid JSON.
    pub input: JsonValue,

    /// Optional display title from the matched high-level tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Whether the provider executed this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool was dynamically defined by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Whether this tool call could not be matched or parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid: Option<bool>,

    /// Error message explaining why this tool call is invalid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Provider-specific metadata returned with the tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// High-level metadata from the matched tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
}

impl Serialize for GenerateTextToolCall {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut field_count = 4;
        field_count += usize::from(self.title.is_some());
        field_count += usize::from(self.provider_executed.is_some());
        field_count += usize::from(self.dynamic.is_some());
        field_count += usize::from(self.invalid.is_some());
        field_count += usize::from(self.error.is_some());
        field_count += usize::from(self.provider_metadata.is_some());
        field_count += usize::from(self.tool_metadata.is_some());

        let mut state = serializer.serialize_struct("GenerateTextToolCall", field_count)?;
        state.serialize_field("type", "tool-call")?;
        state.serialize_field("toolCallId", &self.tool_call_id)?;
        state.serialize_field("toolName", &self.tool_name)?;
        state.serialize_field("input", &self.input)?;

        if let Some(title) = &self.title {
            state.serialize_field("title", title)?;
        }

        if let Some(provider_executed) = self.provider_executed {
            state.serialize_field("providerExecuted", &provider_executed)?;
        }

        if let Some(dynamic) = self.dynamic {
            state.serialize_field("dynamic", &dynamic)?;
        }

        if let Some(invalid) = self.invalid {
            state.serialize_field("invalid", &invalid)?;
        }

        if let Some(error) = &self.error {
            state.serialize_field("error", error)?;
        }

        if let Some(provider_metadata) = &self.provider_metadata {
            state.serialize_field("providerMetadata", provider_metadata)?;
        }

        if let Some(tool_metadata) = &self.tool_metadata {
            state.serialize_field("toolMetadata", tool_metadata)?;
        }

        state.end()
    }
}

impl GenerateTextToolCall {
    pub(crate) fn from_language_model_tool_call(tool_call: &LanguageModelToolCall) -> Self {
        let (input, dynamic, invalid, error) = match parse_tool_input(&tool_call.input) {
            Ok(input) => (input, tool_call.dynamic, None, None),
            Err(error) => (
                JsonValue::String(tool_call.input.clone()),
                Some(true),
                Some(true),
                Some(invalid_tool_input_message(
                    &tool_call.tool_name,
                    &tool_call.input,
                    error,
                )),
            ),
        };

        Self {
            tool_call_id: tool_call.tool_call_id.clone(),
            tool_name: tool_call.tool_name.clone(),
            input,
            title: None,
            provider_executed: tool_call.provider_executed,
            dynamic,
            invalid,
            error,
            provider_metadata: tool_call.provider_metadata.clone(),
            tool_metadata: None,
        }
    }
}

/// Upstream typed static tool-call alias.
pub type StaticToolCall = GenerateTextToolCall;

/// Upstream typed dynamic tool-call alias.
pub type DynamicToolCall = GenerateTextToolCall;

/// Upstream typed tool-call alias.
pub type TypedToolCall = GenerateTextToolCall;

/// Result produced by executing a Rust tool during a generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextToolResult {
    /// Identifier of the matching tool call.
    pub tool_call_id: String,

    /// Name of the executed tool.
    pub tool_name: String,

    /// Input passed to the Rust tool executor.
    pub input: JsonValue,

    /// JSON-serializable tool output, or the error message when `is_error` is
    /// true.
    pub output: JsonValue,

    /// Optional display title from the matched high-level tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Whether this result represents a tool execution error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,

    /// Whether the provider executed this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool was dynamically defined by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Whether this provider tool result is preliminary and may be replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preliminary: Option<bool>,

    /// Provider-specific metadata returned with the tool result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// High-level metadata from the matched tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
}

impl Serialize for GenerateTextToolResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut field_count = 5;
        field_count += usize::from(self.title.is_some());
        field_count += usize::from(self.is_error.is_some());
        field_count += usize::from(self.provider_executed.is_some());
        field_count += usize::from(self.dynamic.is_some());
        field_count += usize::from(self.preliminary.is_some());
        field_count += usize::from(self.provider_metadata.is_some());
        field_count += usize::from(self.tool_metadata.is_some());

        let mut state = serializer.serialize_struct("GenerateTextToolResult", field_count)?;
        state.serialize_field("type", "tool-result")?;
        state.serialize_field("toolCallId", &self.tool_call_id)?;
        state.serialize_field("toolName", &self.tool_name)?;
        state.serialize_field("input", &self.input)?;
        state.serialize_field("output", &self.output)?;

        if let Some(title) = &self.title {
            state.serialize_field("title", title)?;
        }

        if let Some(is_error) = self.is_error {
            state.serialize_field("isError", &is_error)?;
        }

        if let Some(provider_executed) = self.provider_executed {
            state.serialize_field("providerExecuted", &provider_executed)?;
        }

        if let Some(dynamic) = self.dynamic {
            state.serialize_field("dynamic", &dynamic)?;
        }

        if let Some(preliminary) = self.preliminary {
            state.serialize_field("preliminary", &preliminary)?;
        }

        if let Some(provider_metadata) = &self.provider_metadata {
            state.serialize_field("providerMetadata", provider_metadata)?;
        }

        if let Some(tool_metadata) = &self.tool_metadata {
            state.serialize_field("toolMetadata", tool_metadata)?;
        }

        state.end()
    }
}

impl GenerateTextToolResult {
    fn success(tool_call: &GenerateTextToolCall, output: JsonValue) -> Self {
        Self {
            tool_call_id: tool_call.tool_call_id.clone(),
            tool_name: tool_call.tool_name.clone(),
            input: tool_call.input.clone(),
            output,
            title: tool_call.title.clone(),
            is_error: None,
            provider_executed: tool_call.provider_executed,
            dynamic: tool_call.dynamic,
            preliminary: None,
            provider_metadata: tool_call.provider_metadata.clone(),
            tool_metadata: tool_call.tool_metadata.clone(),
        }
    }

    fn error(tool_call: &GenerateTextToolCall, message: String) -> Self {
        Self {
            tool_call_id: tool_call.tool_call_id.clone(),
            tool_name: tool_call.tool_name.clone(),
            input: tool_call.input.clone(),
            output: JsonValue::String(message),
            title: tool_call.title.clone(),
            is_error: Some(true),
            provider_executed: tool_call.provider_executed,
            dynamic: tool_call.dynamic,
            preliminary: None,
            provider_metadata: tool_call.provider_metadata.clone(),
            tool_metadata: tool_call.tool_metadata.clone(),
        }
    }
}

/// Upstream typed static tool-result alias.
pub type StaticToolResult = GenerateTextToolResult;

/// Upstream typed dynamic tool-result alias.
pub type DynamicToolResult = GenerateTextToolResult;

/// Upstream typed tool-result alias.
pub type TypedToolResult = GenerateTextToolResult;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum GenerateTextToolErrorKind {
    #[serde(rename = "tool-error")]
    ToolError,
}

/// Error output produced for an invalid or failed tool call in generate-text content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextToolError {
    #[serde(rename = "type")]
    kind: GenerateTextToolErrorKind,

    /// Identifier of the matching tool call.
    pub tool_call_id: String,

    /// Name of the failed tool.
    pub tool_name: String,

    /// Input associated with the failed tool call.
    pub input: JsonValue,

    /// JSON-serializable error payload or message.
    pub error: JsonValue,

    /// Optional display title from the matched high-level tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Whether the provider executed this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool was dynamically defined by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Provider-specific metadata returned with the tool error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// High-level metadata from the matched tool definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<JsonObject>,
}

impl GenerateTextToolError {
    /// Creates a high-level tool error content part.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: impl Into<JsonValue>,
        error: impl Into<JsonValue>,
    ) -> Self {
        Self {
            kind: GenerateTextToolErrorKind::ToolError,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input: input.into(),
            error: error.into(),
            title: None,
            provider_executed: None,
            dynamic: None,
            provider_metadata: None,
            tool_metadata: None,
        }
    }

    /// Adds the display title from the matched high-level tool definition.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets whether the provider executed this tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Sets whether the tool was dynamically defined by the provider.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }

    /// Adds provider-specific metadata to this tool error.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Adds high-level metadata from the matched tool definition.
    pub fn with_tool_metadata(mut self, tool_metadata: JsonObject) -> Self {
        self.tool_metadata = Some(tool_metadata);
        self
    }

    fn from_tool_result(tool_result: &GenerateTextToolResult) -> Self {
        Self {
            kind: GenerateTextToolErrorKind::ToolError,
            tool_call_id: tool_result.tool_call_id.clone(),
            tool_name: tool_result.tool_name.clone(),
            input: tool_result.input.clone(),
            error: tool_result.output.clone(),
            title: tool_result.title.clone(),
            provider_executed: tool_result.provider_executed,
            dynamic: tool_result.dynamic,
            provider_metadata: tool_result.provider_metadata.clone(),
            tool_metadata: tool_result.tool_metadata.clone(),
        }
    }
}

/// Upstream typed static tool-error alias.
pub type StaticToolError = GenerateTextToolError;

/// Upstream typed dynamic tool-error alias.
pub type DynamicToolError = GenerateTextToolError;

/// Upstream typed tool-error alias.
pub type TypedToolError = GenerateTextToolError;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum GenerateTextToolOutputDeniedKind {
    #[serde(rename = "tool-output-denied")]
    ToolOutputDenied,
}

/// Output indicating that a tool execution was denied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextToolOutputDenied {
    #[serde(rename = "type")]
    kind: GenerateTextToolOutputDeniedKind,

    /// Identifier of the denied tool call.
    pub tool_call_id: String,

    /// Name of the denied tool.
    pub tool_name: String,

    /// Whether the provider would have executed this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool was dynamically defined by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
}

impl GenerateTextToolOutputDenied {
    /// Creates a denied tool-output record.
    pub fn new(tool_call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            kind: GenerateTextToolOutputDeniedKind::ToolOutputDenied,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            provider_executed: None,
            dynamic: None,
        }
    }

    /// Sets whether the provider would have executed this tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Sets whether the tool was dynamically defined by the provider.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }
}

/// Upstream static denied tool-output alias.
pub type StaticToolOutputDenied = GenerateTextToolOutputDenied;

/// Upstream typed denied tool-output alias.
pub type TypedToolOutputDenied = GenerateTextToolOutputDenied;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ToolApprovalRequestOutputKind {
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest,
}

/// Output part indicating that a tool approval request has been made.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequestOutput {
    #[serde(rename = "type")]
    kind: ToolApprovalRequestOutputKind,

    /// ID of the tool approval request.
    pub approval_id: String,

    /// Tool call that the approval request is for.
    pub tool_call: GenerateTextToolCall,

    /// Whether the approval status was decided automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_automatic: Option<bool>,
}

impl ToolApprovalRequestOutput {
    /// Creates a tool approval request output.
    pub fn new(approval_id: impl Into<String>, tool_call: GenerateTextToolCall) -> Self {
        Self {
            kind: ToolApprovalRequestOutputKind::ToolApprovalRequest,
            approval_id: approval_id.into(),
            tool_call,
            is_automatic: None,
        }
    }

    /// Sets whether this request was automatically approved or denied.
    pub fn with_automatic(mut self, is_automatic: bool) -> Self {
        self.is_automatic = Some(is_automatic);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ToolApprovalResponseOutputKind {
    #[serde(rename = "tool-approval-response")]
    ToolApprovalResponse,
}

/// Output part indicating that a tool approval response is available.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResponseOutput {
    #[serde(rename = "type")]
    kind: ToolApprovalResponseOutputKind,

    /// ID of the tool approval request.
    pub approval_id: String,

    /// Tool call that the approval response is for.
    pub tool_call: GenerateTextToolCall,

    /// Whether the approval was granted.
    pub approved: bool,

    /// Optional approval or denial reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Whether the approved or denied tool call is provider-executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
}

impl ToolApprovalResponseOutput {
    /// Creates a tool approval response output.
    pub fn new(
        approval_id: impl Into<String>,
        tool_call: GenerateTextToolCall,
        approved: bool,
    ) -> Self {
        Self {
            kind: ToolApprovalResponseOutputKind::ToolApprovalResponse,
            approval_id: approval_id.into(),
            tool_call,
            approved,
            reason: None,
            provider_executed: None,
        }
    }

    /// Adds an approval or denial reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Sets whether the tool call is provider-executed.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }
}

/// High-level content part produced by `generate_text`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GenerateTextContentPart {
    /// Generated text content.
    Text(LanguageModelText),

    /// Provider-specific generated content.
    Custom(LanguageModelCustomContent),

    /// High-level reasoning content.
    Reasoning(ReasoningOutput),

    /// High-level reasoning file content.
    ReasoningFile(ReasoningFileOutput),

    /// Generated file content.
    File(GenerateTextFileContent),

    /// Source content used to generate the response.
    Source(LanguageModelSource),

    /// Tool error content.
    ToolError(GenerateTextToolError),

    /// Tool call content.
    ToolCall(GenerateTextToolCall),

    /// Tool result content.
    ToolResult(GenerateTextToolResult),

    /// Tool approval request content.
    ToolApprovalRequest(ToolApprovalRequestOutput),

    /// Tool approval response content.
    ToolApprovalResponse(ToolApprovalResponseOutput),
}

/// Upstream generate-text content part alias.
pub type ContentPart = GenerateTextContentPart;

/// Information about the model that produced a generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextModelInfo {
    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model id.
    pub model_id: String,
}

impl GenerateTextModelInfo {
    /// Creates model information for a step result.
    pub fn new(provider: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
        }
    }
}

/// Upstream model info event name.
pub type ModelInfo = GenerateTextModelInfo;

/// Performance metrics for a single generate-text step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextStepPerformance {
    /// Effective number of output tokens per second over the full model response.
    pub effective_output_tokens_per_second: f64,

    /// Output tokens per second after the first output token was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_per_second: Option<f64>,

    /// Input tokens per second before the first output token was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_per_second: Option<f64>,

    /// Effective input and output tokens per second over the full model response.
    pub effective_total_tokens_per_second: f64,

    /// Total time spent on the step in milliseconds.
    pub step_time_ms: u64,

    /// Time spent waiting for the language model response in milliseconds.
    pub response_time_ms: u64,

    /// Time spent executing each client-side tool call, keyed by tool call id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_execution_ms: BTreeMap<String, u64>,

    /// Time until the first text, reasoning, or tool input delta was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_first_output_token_ms: Option<u64>,
}

impl Default for GenerateTextStepPerformance {
    fn default() -> Self {
        Self {
            effective_output_tokens_per_second: 0.0,
            output_tokens_per_second: None,
            input_tokens_per_second: None,
            effective_total_tokens_per_second: 0.0,
            step_time_ms: 0,
            response_time_ms: 0,
            tool_execution_ms: BTreeMap::new(),
            time_to_first_output_token_ms: None,
        }
    }
}

impl GenerateTextStepPerformance {
    fn from_usage(
        usage: &LanguageModelUsage,
        response_time_ms: u64,
        step_time_ms: u64,
        tool_execution_ms: BTreeMap<String, u64>,
    ) -> Self {
        Self {
            effective_output_tokens_per_second: calculate_tokens_per_second(
                usage.output_tokens.total,
                response_time_ms,
            ),
            effective_total_tokens_per_second: calculate_tokens_per_second(
                sum_token_counts(usage.input_tokens.total, usage.output_tokens.total),
                response_time_ms,
            ),
            step_time_ms,
            response_time_ms,
            tool_execution_ms,
            ..Self::default()
        }
    }
}

/// Result of a single non-streaming generate-text step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextStep {
    /// Unique identifier for the generation call this step belongs to.
    pub call_id: String,

    /// Zero-based index of this step.
    pub step_number: usize,

    /// Model that produced this step.
    pub model: GenerateTextModelInfo,

    /// Tool context used for this step.
    #[serde(default)]
    pub tools_context: JsonObject,

    /// Runtime context used for this step.
    #[serde(default)]
    pub runtime_context: JsonObject,

    /// Content generated in this step.
    pub content: Vec<GenerateTextContentPart>,

    /// Tool calls generated in this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<GenerateTextToolCall>,

    /// Static tool calls generated in this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_tool_calls: Vec<GenerateTextToolCall>,

    /// Dynamic tool calls generated in this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_tool_calls: Vec<GenerateTextToolCall>,

    /// Rust tool results produced for this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<GenerateTextToolResult>,

    /// Static tool results produced for this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_tool_results: Vec<GenerateTextToolResult>,

    /// Dynamic tool results produced for this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_tool_results: Vec<GenerateTextToolResult>,

    /// Response messages generated by this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_messages: Vec<LanguageModelMessage>,

    /// Files generated during this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<GeneratedFile>,

    /// Reasoning generated during this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning: Vec<GenerateTextReasoning>,

    /// Text from reasoning parts generated during this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,

    /// Sources used to generate this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LanguageModelSource>,

    /// Text content generated in this step, formed by concatenating all text parts.
    pub text: String,

    /// Unified reason why this step finished.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Usage reported for this step.
    pub usage: LanguageModelUsage,

    /// Performance metrics for this step.
    #[serde(default)]
    pub performance: GenerateTextStepPerformance,

    /// Warnings reported by the provider for this step.
    pub warnings: Vec<Warning>,

    /// Optional request information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelResponse>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl GenerateTextStep {
    pub(crate) fn from_language_model_result(
        call_id: impl Into<String>,
        step_number: usize,
        model: GenerateTextModelInfo,
        result: LanguageModelGenerateResult,
    ) -> Self {
        let call_id = call_id.into();
        let LanguageModelGenerateResult {
            content,
            finish_reason:
                LanguageModelFinishReason {
                    unified,
                    raw: raw_finish_reason,
                },
            usage,
            provider_metadata,
            request,
            response,
            warnings,
        } = result;

        let text = extract_text(&content);
        let tool_calls = extract_tool_calls(&content);
        let static_tool_calls = static_tool_calls(&tool_calls);
        let dynamic_tool_calls = dynamic_tool_calls(&tool_calls);
        let tool_results = extract_provider_tool_results(&content, &tool_calls);
        let static_tool_results = static_tool_results(&tool_results);
        let dynamic_tool_results = dynamic_tool_results(&tool_results);
        let files = extract_files(&content);
        let reasoning = extract_reasoning(&content);
        let reasoning_text = extract_reasoning_text(&reasoning);
        let sources = extract_sources(&content);
        let content = generate_text_content_parts(
            &content,
            &tool_calls,
            &tool_results,
            &StepToolApprovals::default(),
        );

        Self {
            call_id,
            step_number,
            model,
            tools_context: JsonObject::new(),
            runtime_context: JsonObject::new(),
            content,
            tool_calls,
            static_tool_calls,
            dynamic_tool_calls,
            tool_results,
            static_tool_results,
            dynamic_tool_results,
            response_messages: Vec::new(),
            files,
            reasoning,
            reasoning_text,
            sources,
            text,
            finish_reason: unified,
            raw_finish_reason,
            usage,
            performance: GenerateTextStepPerformance::default(),
            warnings,
            request,
            response,
            provider_metadata,
        }
    }
}

/// Event sent to a high-level non-streaming generate-text finish callback.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextFinishEvent {
    /// Unique identifier for the generation call.
    pub call_id: String,

    /// Zero-based index of the final step.
    pub step_number: usize,

    /// Model that produced the final step.
    pub model: GenerateTextModelInfo,

    /// Runtime context used for the final step.
    #[serde(default)]
    pub runtime_context: JsonObject,

    /// Tool context used for the final step.
    #[serde(default)]
    pub tools_context: JsonObject,

    /// Unified reason why the final step finished.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason from the final step, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Usage reported by the final step.
    pub usage: LanguageModelUsage,

    /// Total usage across all steps.
    pub total_usage: LanguageModelUsage,

    /// Content generated in the final step.
    pub content: Vec<GenerateTextContentPart>,

    /// Text generated in the final step.
    pub text: String,

    /// Text from reasoning parts generated in the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,

    /// Reasoning generated in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning: Vec<GenerateTextReasoning>,

    /// Files generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<GeneratedFile>,

    /// Sources used to generate the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LanguageModelSource>,

    /// Tool calls generated in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<GenerateTextToolCall>,

    /// Static tool calls generated in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_tool_calls: Vec<GenerateTextToolCall>,

    /// Dynamic tool calls generated in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_tool_calls: Vec<GenerateTextToolCall>,

    /// Tool results produced in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<GenerateTextToolResult>,

    /// Static tool results produced in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_tool_results: Vec<GenerateTextToolResult>,

    /// Dynamic tool results produced in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_tool_results: Vec<GenerateTextToolResult>,

    /// Accumulated response messages generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_messages: Vec<LanguageModelMessage>,

    /// Warnings reported across all steps.
    pub warnings: Vec<Warning>,

    /// Optional request information from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelResponse>,

    /// Provider-specific metadata from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Details for all generation steps.
    pub steps: Vec<GenerateTextStep>,
}

impl GenerateTextFinishEvent {
    pub(crate) fn from_steps(
        initial_response_messages: &[LanguageModelMessage],
        steps: &[GenerateTextStep],
    ) -> Self {
        let final_step = steps
            .last()
            .expect("generate_text always creates at least one step");
        let mut response_messages = initial_response_messages.to_vec();
        response_messages.extend(
            steps
                .iter()
                .flat_map(|step| step.response_messages.iter().cloned()),
        );

        Self {
            call_id: final_step.call_id.clone(),
            step_number: final_step.step_number,
            model: final_step.model.clone(),
            runtime_context: final_step.runtime_context.clone(),
            tools_context: final_step.tools_context.clone(),
            finish_reason: final_step.finish_reason.clone(),
            raw_finish_reason: final_step.raw_finish_reason.clone(),
            usage: final_step.usage.clone(),
            total_usage: add_step_usage(steps),
            content: final_step.content.clone(),
            text: final_step.text.clone(),
            reasoning_text: final_step.reasoning_text.clone(),
            reasoning: final_step.reasoning.clone(),
            files: steps
                .iter()
                .flat_map(|step| step.files.iter().cloned())
                .collect(),
            sources: final_step.sources.clone(),
            tool_calls: final_step.tool_calls.clone(),
            static_tool_calls: final_step.static_tool_calls.clone(),
            dynamic_tool_calls: final_step.dynamic_tool_calls.clone(),
            tool_results: final_step.tool_results.clone(),
            static_tool_results: final_step.static_tool_results.clone(),
            dynamic_tool_results: final_step.dynamic_tool_results.clone(),
            response_messages,
            warnings: steps
                .iter()
                .flat_map(|step| step.warnings.iter().cloned())
                .collect(),
            request: final_step.request.clone(),
            response: final_step.response.clone(),
            provider_metadata: final_step.provider_metadata.clone(),
            steps: steps.to_vec(),
        }
    }
}

/// Upstream event name for a completed generate-text step.
pub type GenerateTextStepEndEvent = GenerateTextStep;

/// Upstream event name for a completed generate-text call.
pub type GenerateTextEndEvent = GenerateTextFinishEvent;

/// Deprecated upstream alias for [`GenerateTextStartEvent`].
#[deprecated(note = "use GenerateTextStartEvent instead")]
pub type OnStartEvent = GenerateTextStartEvent;

/// Deprecated upstream alias for [`GenerateTextStepStartEvent`].
#[deprecated(note = "use GenerateTextStepStartEvent instead")]
pub type OnStepStartEvent = GenerateTextStepStartEvent;

/// Deprecated upstream alias for [`GenerateTextStepEndEvent`].
#[deprecated(note = "use GenerateTextStepEndEvent instead")]
pub type OnStepFinishEvent = GenerateTextStepEndEvent;

/// Deprecated upstream alias for [`GenerateTextEndEvent`].
#[deprecated(note = "use GenerateTextEndEvent instead")]
pub type OnFinishEvent = GenerateTextEndEvent;

/// Result of a high-level non-streaming text generation call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextResult {
    /// Content generated across all steps.
    pub content: Vec<GenerateTextContentPart>,

    /// Tool calls generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<GenerateTextToolCall>,

    /// Static tool calls generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_tool_calls: Vec<GenerateTextToolCall>,

    /// Dynamic tool calls generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_tool_calls: Vec<GenerateTextToolCall>,

    /// Rust tool results produced across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<GenerateTextToolResult>,

    /// Static tool results produced across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_tool_results: Vec<GenerateTextToolResult>,

    /// Dynamic tool results produced across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_tool_results: Vec<GenerateTextToolResult>,

    /// Accumulated response messages generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_messages: Vec<LanguageModelMessage>,

    /// Files generated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<GeneratedFile>,

    /// Reasoning generated in the final step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning: Vec<GenerateTextReasoning>,

    /// Text from reasoning parts generated in the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,

    /// Sources used to generate the response across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LanguageModelSource>,

    /// Text generated in the final step.
    pub text: String,

    /// Parsed high-level output from the final step.
    ///
    /// The current Rust surface has only the upstream default text output mode,
    /// so successful stop completions expose the final text as a JSON string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<JsonValue>,

    /// Unified reason why the final step finished.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason from the final step, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Total usage across all steps.
    pub usage: LanguageModelUsage,

    /// Deprecated upstream alias for [`GenerateTextResult::usage`].
    #[serde(default)]
    pub total_usage: LanguageModelUsage,

    /// Warnings reported across all steps.
    pub warnings: Vec<Warning>,

    /// Optional request information from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelResponse>,

    /// Provider-specific metadata from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Details for all generation steps.
    pub steps: Vec<GenerateTextStep>,

    /// The final step, mirroring upstream `GenerateTextResult.finalStep`.
    pub final_step: GenerateTextStep,
}

impl GenerateTextResult {
    fn from_steps(steps: Vec<GenerateTextStep>) -> Self {
        let final_step = steps
            .last()
            .expect("generate_text always creates at least one step");
        let total_usage = add_step_usage(&steps);
        let output = (final_step.finish_reason == FinishReason::Stop)
            .then(|| JsonValue::String(final_step.text.clone()));

        Self {
            content: steps
                .iter()
                .flat_map(|step| step.content.iter().cloned())
                .collect(),
            text: final_step.text.clone(),
            output,
            finish_reason: final_step.finish_reason.clone(),
            raw_finish_reason: final_step.raw_finish_reason.clone(),
            usage: total_usage.clone(),
            total_usage,
            tool_calls: steps
                .iter()
                .flat_map(|step| step.tool_calls.iter().cloned())
                .collect(),
            static_tool_calls: steps
                .iter()
                .flat_map(|step| step.static_tool_calls.iter().cloned())
                .collect(),
            dynamic_tool_calls: steps
                .iter()
                .flat_map(|step| step.dynamic_tool_calls.iter().cloned())
                .collect(),
            tool_results: steps
                .iter()
                .flat_map(|step| step.tool_results.iter().cloned())
                .collect(),
            static_tool_results: steps
                .iter()
                .flat_map(|step| step.static_tool_results.iter().cloned())
                .collect(),
            dynamic_tool_results: steps
                .iter()
                .flat_map(|step| step.dynamic_tool_results.iter().cloned())
                .collect(),
            response_messages: steps
                .iter()
                .flat_map(|step| step.response_messages.iter().cloned())
                .collect(),
            files: steps
                .iter()
                .flat_map(|step| step.files.iter().cloned())
                .collect(),
            reasoning: final_step.reasoning.clone(),
            reasoning_text: final_step.reasoning_text.clone(),
            sources: steps
                .iter()
                .flat_map(|step| step.sources.iter().cloned())
                .collect(),
            warnings: steps
                .iter()
                .flat_map(|step| step.warnings.iter().cloned())
                .collect(),
            request: final_step.request.clone(),
            response: final_step.response.clone(),
            provider_metadata: final_step.provider_metadata.clone(),
            final_step: final_step.clone(),
            steps,
        }
    }

    /// Returns the final step recorded on this result.
    pub fn final_step(&self) -> Option<&GenerateTextStep> {
        Some(&self.final_step)
    }

    /// Returns the generated output or an upstream-aligned error when none was produced.
    pub fn output(&self) -> Result<&JsonValue, NoOutputGeneratedError> {
        self.output.as_ref().ok_or_else(NoOutputGeneratedError::new)
    }

    /// Consumes the result and returns the generated output.
    pub fn into_output(self) -> Result<JsonValue, NoOutputGeneratedError> {
        self.output.ok_or_else(NoOutputGeneratedError::new)
    }
}

/// Runs a non-streaming text generation call against a language model.
pub async fn generate_text<M: LanguageModel + ?Sized>(
    options: GenerateTextOptions<'_, M>,
) -> GenerateTextResult {
    let GenerateTextOptions {
        model,
        mut call_options,
        tools,
        mut runtime_context,
        mut tools_context,
        experimental_sandbox,
        active_tools,
        tool_approval,
        tool_input_refinements,
        tool_call_repair,
        prepare_step,
        on_start,
        on_step_start,
        on_language_model_call_start,
        on_language_model_call_end,
        on_tool_execution_start,
        on_tool_execution_end,
        on_step_finish,
        on_finish,
        max_steps,
        stop_conditions,
        include,
    } = options;
    let base_language_model_tools = call_options.tools.take();
    let initial_messages = call_options.prompt.clone();
    let mut current_prompt = initial_messages.clone();
    let active_tools_for_start = active_tools.clone();
    let active_tools = active_tools.as_deref();
    let base_provider_options = call_options.provider_options.clone();

    let max_steps = max_steps.max(1);
    let call_id = generate_text_call_id();

    if let Some(on_start) = &on_start {
        let mut start_tools = base_language_model_tools.clone().unwrap_or_default();
        if let Some(mut prepared_tools) =
            prepare_tools_with_context(&tools, Some(&tools_context), experimental_sandbox.as_ref())
        {
            start_tools.append(&mut prepared_tools);
        }

        on_start
            .start(GenerateTextStartEvent {
                call_id: call_id.clone(),
                operation_id: "ai.generateText".to_string(),
                provider: model.provider().to_string(),
                model_id: model.model_id().to_string(),
                messages: initial_messages.clone(),
                tools: start_tools,
                tool_choice: call_options.tool_choice.clone(),
                active_tools: active_tools_for_start,
                max_output_tokens: call_options.max_output_tokens,
                temperature: call_options.temperature,
                top_p: call_options.top_p,
                top_k: call_options.top_k,
                presence_penalty: call_options.presence_penalty,
                frequency_penalty: call_options.frequency_penalty,
                stop_sequences: call_options.stop_sequences.clone(),
                seed: call_options.seed,
                reasoning: call_options.reasoning.clone(),
                headers: call_options.headers.clone(),
                provider_options: call_options.provider_options.clone(),
                runtime_context: runtime_context.clone(),
                tools_context: tools_context.clone(),
            })
            .await;
    }

    let mut initial_response_messages = Vec::new();
    if let Some(message) = initial_tool_approval_response_message(
        &call_id,
        &current_prompt,
        &tools,
        &tools_context,
        experimental_sandbox.as_ref(),
        (
            on_tool_execution_start.as_ref(),
            on_tool_execution_end.as_ref(),
        ),
    )
    .await
    {
        current_prompt.push(message.clone());
        initial_response_messages.push(message);
    }

    let mut steps = Vec::new();
    let mut pending_deferred_provider_tool_call_ids = BTreeSet::new();

    for step_number in 0..max_steps {
        let accumulated_response_messages =
            accumulated_response_messages(&initial_response_messages, &steps);
        let prepare_step_result = if let Some(prepare_step) = &prepare_step {
            prepare_step
                .prepare(PrepareStepOptions {
                    steps: steps.clone(),
                    step_number,
                    model,
                    messages: current_prompt.clone(),
                    initial_messages: initial_messages.clone(),
                    response_messages: accumulated_response_messages,
                    runtime_context: runtime_context.clone(),
                    tools_context: tools_context.clone(),
                    experimental_sandbox: experimental_sandbox.clone(),
                })
                .await
        } else {
            PrepareStepResult::default()
        };

        let PrepareStepResult {
            model: step_model,
            tool_choice: step_tool_choice,
            active_tools: step_active_tools,
            messages: step_messages,
            runtime_context: step_runtime_context,
            tools_context: step_tools_context,
            provider_options: step_provider_options,
            experimental_sandbox: step_experimental_sandbox,
        } = prepare_step_result;

        if let Some(runtime_context_override) = step_runtime_context {
            runtime_context = runtime_context_override;
        }

        if let Some(tools_context_override) = step_tools_context {
            tools_context = tools_context_override;
        }

        if let Some(messages_override) = step_messages {
            current_prompt = messages_override;
        }

        let step_model = step_model.unwrap_or(model);
        let step_experimental_sandbox =
            step_experimental_sandbox.or_else(|| experimental_sandbox.clone());
        let step_model_info =
            GenerateTextModelInfo::new(step_model.provider(), step_model.model_id());
        let step_active_tools = step_active_tools.as_deref().or(active_tools);
        let step_tools =
            filter_active_tools(Some(tools.clone()), step_active_tools).unwrap_or_default();
        let mut step_language_model_tools = filter_active_language_model_tools(
            base_language_model_tools.clone(),
            step_active_tools,
        );

        if let Some(mut prepared_tools) = prepare_tools_with_context(
            &step_tools,
            Some(&tools_context),
            step_experimental_sandbox.as_ref(),
        ) {
            step_language_model_tools
                .get_or_insert_with(Vec::new)
                .append(&mut prepared_tools);
        }

        let mut step_call_options = call_options.clone();
        step_call_options.prompt = current_prompt.clone();
        step_call_options.tools = step_language_model_tools;

        if let Some(tool_choice) = step_tool_choice {
            step_call_options.tool_choice = Some(tool_choice);
        }

        step_call_options.provider_options =
            merge_provider_options(base_provider_options.as_ref(), step_provider_options);

        let step_prompt = step_call_options.prompt.clone();
        if let Some(on_step_start) = &on_step_start {
            on_step_start
                .start(GenerateTextStepStartEvent {
                    call_id: call_id.clone(),
                    provider: step_model.provider().to_string(),
                    model_id: step_model.model_id().to_string(),
                    step_number,
                    messages: step_prompt.clone(),
                    tools: step_call_options.tools.clone().unwrap_or_default(),
                    tool_choice: step_call_options.tool_choice.clone(),
                    active_tools: step_active_tools.map(|tools| tools.to_vec()),
                    steps: steps.clone(),
                    provider_options: step_call_options.provider_options.clone(),
                    runtime_context: runtime_context.clone(),
                    tools_context: tools_context.clone(),
                })
                .await;
        }

        if let Some(on_language_model_call_start) = &on_language_model_call_start {
            on_language_model_call_start
                .start(LanguageModelCallStartEvent::from_call_options(
                    &call_id,
                    step_model.provider(),
                    step_model.model_id(),
                    &step_call_options,
                ))
                .await;
        }

        let step_started_at = Instant::now();
        let result = step_model.do_generate(step_call_options.clone()).await;
        let response_time_ms = duration_ms(step_started_at.elapsed());
        let provider_content = result.content.clone();
        let mut step = GenerateTextStep::from_language_model_result(
            call_id.clone(),
            step_number,
            step_model_info,
            result,
        );
        step.runtime_context = runtime_context.clone();
        step.tools_context = tools_context.clone();
        mark_unavailable_tool_calls(&mut step.tool_calls, step_call_options.tools.as_deref());
        repair_tool_calls(
            &mut step.tool_calls,
            &provider_content,
            tool_call_repair.as_ref(),
            &step_tools,
            step_call_options.tools.as_deref(),
            &step_prompt,
        )
        .await;
        refine_tool_inputs(&mut step.tool_calls, &tool_input_refinements).await;
        sync_tool_result_inputs(&mut step.tool_results, &step.tool_calls);
        mark_runtime_dynamic_tool_calls(&mut step.tool_calls, &step_tools);
        mark_tool_call_titles(&mut step.tool_calls, &step_tools);
        mark_tool_call_metadata(&mut step.tool_calls, &step_tools);
        mark_tool_result_metadata(&mut step.tool_results, &step.tool_calls, &step_tools);
        refresh_tool_call_views(&mut step);
        refresh_generate_text_content(&mut step, &provider_content, &StepToolApprovals::default());
        ensure_generate_text_response_identity(&mut step);

        if let Some(on_language_model_call_end) = &on_language_model_call_end {
            on_language_model_call_end
                .end(LanguageModelCallEndEvent::from_step(
                    &step,
                    response_time_ms,
                ))
                .await;
        }

        let tool_approvals = resolve_tool_approvals_for_step(
            &step.tool_calls,
            &step_tools,
            tool_approval.as_ref(),
            &step_prompt,
            &tools_context,
            &runtime_context,
        )
        .await;
        update_pending_deferred_provider_tool_calls(
            &mut pending_deferred_provider_tool_call_ids,
            &step,
            &step_tools,
        );
        let (tool_results, tool_execution_ms) = execute_tool_calls(
            &call_id,
            &step_tools,
            &step.tool_calls,
            &step_prompt,
            &tools_context,
            &tool_approvals.blocked_tool_call_ids,
            (
                step_experimental_sandbox.as_ref(),
                on_tool_execution_start.as_ref(),
                on_tool_execution_end.as_ref(),
            ),
        )
        .await;
        let should_continue = should_continue_after_tool_results(
            &step,
            &tool_results,
            tool_approvals.denied_client_tool_call_count,
            !pending_deferred_provider_tool_call_ids.is_empty(),
        );
        step.tool_results.extend(tool_results);
        mark_tool_result_metadata(&mut step.tool_results, &step.tool_calls, &step_tools);
        refresh_tool_result_views(&mut step);
        step.response_messages =
            response_messages_for_step(&step, &provider_content, &tool_approvals, &step_tools)
                .await
                .unwrap_or_default();
        refresh_generate_text_content(&mut step, &provider_content, &tool_approvals);
        apply_generate_text_response_metadata(&mut step);
        apply_generate_text_include(&mut step, include, &step_prompt);
        step.performance = GenerateTextStepPerformance::from_usage(
            &step.usage,
            response_time_ms,
            duration_ms(step_started_at.elapsed()),
            tool_execution_ms,
        );

        if let Some(on_step_finish) = &on_step_finish {
            on_step_finish.finish(step.clone()).await;
        }

        let response_messages = step.response_messages.clone();
        steps.push(step);

        if should_continue
            && !is_stop_condition_met(&stop_conditions, &steps)
            && step_number + 1 < max_steps
        {
            if response_messages.is_empty() {
                break;
            } else {
                current_prompt = step_prompt;
                current_prompt.extend(response_messages);
            }
        } else {
            break;
        }
    }

    if let Some(on_finish) = &on_finish {
        on_finish
            .finish(GenerateTextFinishEvent::from_steps(
                &initial_response_messages,
                &steps,
            ))
            .await;
    }

    GenerateTextResult::from_steps(steps)
}

fn accumulated_response_messages(
    initial_response_messages: &[LanguageModelMessage],
    steps: &[GenerateTextStep],
) -> Vec<LanguageModelMessage> {
    initial_response_messages
        .iter()
        .chain(steps.iter().flat_map(|step| step.response_messages.iter()))
        .cloned()
        .collect()
}

fn merge_provider_options(
    base_provider_options: Option<&ProviderOptions>,
    step_provider_options: Option<ProviderOptions>,
) -> Option<ProviderOptions> {
    if base_provider_options.is_none() && step_provider_options.is_none() {
        return None;
    }

    let mut provider_options = base_provider_options.cloned().unwrap_or_default();

    if let Some(step_provider_options) = step_provider_options {
        provider_options.extend(step_provider_options);
    }

    Some(provider_options)
}

pub(crate) fn generate_text_call_id() -> String {
    let generate_call_id =
        create_id_generator(IdGeneratorOptions::new().with_prefix("call").with_size(24))
            .expect("default generate_text call id configuration is valid");

    generate_call_id()
}

pub(crate) fn apply_generate_text_response_metadata(step: &mut GenerateTextStep) {
    ensure_generate_text_response_identity(step);
    let response = step
        .response
        .as_mut()
        .expect("generate_text response identity creates response metadata");

    response.messages = Some(step.response_messages.clone());
}

pub(crate) fn ensure_generate_text_response_identity(step: &mut GenerateTextStep) {
    let response = step.response.get_or_insert_with(LanguageModelResponse::new);

    if response.id.is_none() {
        response.id = Some(generate_id());
    }

    if response.timestamp.is_none() {
        response.timestamp = Some(time::OffsetDateTime::now_utc());
    }

    if response.model_id.is_none() {
        response.model_id = Some(step.model.model_id.clone());
    }
}

fn apply_generate_text_include(
    step: &mut GenerateTextStep,
    include: GenerateTextInclude,
    step_prompt: &LanguageModelPrompt,
) {
    if include.request_messages {
        step.request
            .get_or_insert_with(LanguageModelRequest::new)
            .messages = Some(step_prompt.to_vec());
    }

    if !include.request_body {
        if let Some(request) = &mut step.request {
            request.body = None;
        }
    }

    if !include.response_body {
        if let Some(response) = &mut step.response {
            response.body = None;
        }
    }
}

fn extract_text(content: &[LanguageModelContent]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::Text(LanguageModelText { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn extract_tool_calls(content: &[LanguageModelContent]) -> Vec<GenerateTextToolCall> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::ToolCall(tool_call) => Some(
                GenerateTextToolCall::from_language_model_tool_call(tool_call),
            ),
            _ => None,
        })
        .collect()
}

fn static_tool_calls(tool_calls: &[GenerateTextToolCall]) -> Vec<GenerateTextToolCall> {
    tool_calls
        .iter()
        .filter(|tool_call| tool_call.dynamic != Some(true))
        .cloned()
        .collect()
}

fn dynamic_tool_calls(tool_calls: &[GenerateTextToolCall]) -> Vec<GenerateTextToolCall> {
    tool_calls
        .iter()
        .filter(|tool_call| tool_call.dynamic == Some(true))
        .cloned()
        .collect()
}

pub(crate) fn refresh_tool_call_views(step: &mut GenerateTextStep) {
    step.static_tool_calls = static_tool_calls(&step.tool_calls);
    step.dynamic_tool_calls = dynamic_tool_calls(&step.tool_calls);
}

pub(crate) fn refresh_generate_text_content(
    step: &mut GenerateTextStep,
    provider_content: &[LanguageModelContent],
    tool_approvals: &StepToolApprovals,
) {
    step.content = generate_text_content_parts(
        provider_content,
        &step.tool_calls,
        &step.tool_results,
        tool_approvals,
    );
}

fn generate_text_content_parts(
    provider_content: &[LanguageModelContent],
    tool_calls: &[GenerateTextToolCall],
    tool_results: &[GenerateTextToolResult],
    tool_approvals: &StepToolApprovals,
) -> Vec<GenerateTextContentPart> {
    let mut content_parts = Vec::new();

    for part in provider_content {
        match part {
            LanguageModelContent::Text(text) => {
                content_parts.push(GenerateTextContentPart::Text(text.clone()));
            }
            LanguageModelContent::Reasoning(reasoning) => {
                content_parts.push(GenerateTextContentPart::Reasoning(
                    ReasoningOutput::from_language_model_reasoning(reasoning),
                ));
            }
            LanguageModelContent::Custom(custom) => {
                content_parts.push(GenerateTextContentPart::Custom(custom.clone()));
            }
            LanguageModelContent::ReasoningFile(file) => {
                content_parts.push(GenerateTextContentPart::ReasoningFile(
                    ReasoningFileOutput::from_language_model_reasoning_file(file),
                ));
            }
            LanguageModelContent::File(file) => {
                content_parts.push(GenerateTextContentPart::File(
                    GenerateTextFileContent::from_language_model_file(file),
                ));
            }
            LanguageModelContent::ToolApprovalRequest(request) => {
                if let Some(output) =
                    tool_approval_request_output_from_model_request(request, tool_calls)
                {
                    content_parts.push(GenerateTextContentPart::ToolApprovalRequest(output));
                }
            }
            LanguageModelContent::Source(source) => {
                content_parts.push(GenerateTextContentPart::Source(source.clone()));
            }
            LanguageModelContent::ToolCall(tool_call) => {
                if let Some(tool_call) = tool_calls
                    .iter()
                    .find(|parsed| parsed.tool_call_id == tool_call.tool_call_id)
                    .cloned()
                {
                    content_parts.push(GenerateTextContentPart::ToolCall(tool_call));
                }
            }
            LanguageModelContent::ToolResult(tool_result) => {
                let result = tool_results
                    .iter()
                    .find(|result| {
                        result.provider_executed == Some(true)
                            && result.tool_call_id == tool_result.tool_call_id
                    })
                    .cloned()
                    .unwrap_or_else(|| {
                        generate_text_tool_result_from_language_model_tool_result(
                            tool_result,
                            tool_calls,
                        )
                    });

                content_parts.push(generate_text_tool_result_content_part(&result));
            }
        }
    }

    let tool_call_ids_with_approval_responses = tool_approvals
        .responses
        .iter()
        .map(|approval| approval.tool_call.tool_call_id.clone())
        .collect::<BTreeSet<_>>();
    let mut tool_results_with_approval_responses = Vec::new();
    let mut tool_results_without_approval_responses = Vec::new();

    for tool_result in tool_results
        .iter()
        .filter(|tool_result| tool_result.provider_executed != Some(true))
    {
        if tool_call_ids_with_approval_responses.contains(&tool_result.tool_call_id) {
            tool_results_with_approval_responses
                .push(generate_text_tool_result_content_part(tool_result));
        } else {
            tool_results_without_approval_responses
                .push(generate_text_tool_result_content_part(tool_result));
        }
    }

    content_parts.extend(tool_results_without_approval_responses);
    content_parts.extend(
        tool_approvals
            .requests
            .iter()
            .filter_map(|request| {
                tool_approval_request_output_from_prompt_part(request, tool_calls)
            })
            .map(GenerateTextContentPart::ToolApprovalRequest),
    );
    content_parts.extend(
        tool_approvals
            .responses
            .iter()
            .map(tool_approval_response_output)
            .map(GenerateTextContentPart::ToolApprovalResponse),
    );
    content_parts.extend(tool_results_with_approval_responses);

    content_parts
}

fn tool_approval_request_output_from_model_request(
    request: &LanguageModelToolApprovalRequest,
    tool_calls: &[GenerateTextToolCall],
) -> Option<ToolApprovalRequestOutput> {
    tool_calls
        .iter()
        .find(|tool_call| tool_call.tool_call_id == request.tool_call_id)
        .cloned()
        .map(|tool_call| ToolApprovalRequestOutput::new(request.approval_id.clone(), tool_call))
}

fn tool_approval_request_output_from_prompt_part(
    request: &LanguageModelToolApprovalRequestPart,
    tool_calls: &[GenerateTextToolCall],
) -> Option<ToolApprovalRequestOutput> {
    tool_calls
        .iter()
        .find(|tool_call| tool_call.tool_call_id == request.tool_call_id)
        .cloned()
        .map(|tool_call| {
            let output = ToolApprovalRequestOutput::new(request.approval_id.clone(), tool_call);
            if request.is_automatic == Some(true) {
                output.with_automatic(true)
            } else {
                output
            }
        })
}

fn tool_approval_response_output(
    approval_response: &StepToolApprovalResponse,
) -> ToolApprovalResponseOutput {
    let mut output = ToolApprovalResponseOutput::new(
        approval_response.response.approval_id.clone(),
        approval_response.tool_call.clone(),
        approval_response.response.approved,
    );

    if let Some(reason) = &approval_response.response.reason {
        output = output.with_reason(reason.clone());
    }

    if let Some(provider_executed) = approval_response.tool_call.provider_executed {
        output = output.with_provider_executed(provider_executed);
    }

    output
}

pub(crate) fn generate_text_tool_result_from_language_model_tool_result(
    tool_result: &LanguageModelToolResult,
    tool_calls: &[GenerateTextToolCall],
) -> GenerateTextToolResult {
    let matching_tool_call = tool_calls
        .iter()
        .find(|tool_call| tool_call.tool_call_id == tool_result.tool_call_id);

    GenerateTextToolResult {
        tool_call_id: tool_result.tool_call_id.clone(),
        tool_name: tool_result.tool_name.clone(),
        input: matching_tool_call.map_or(JsonValue::Null, |tool_call| tool_call.input.clone()),
        output: tool_result.result.as_value().clone(),
        title: matching_tool_call.and_then(|tool_call| tool_call.title.clone()),
        is_error: tool_result.is_error,
        provider_executed: Some(true),
        dynamic: matching_tool_call
            .and_then(|tool_call| tool_call.dynamic)
            .or(tool_result.dynamic),
        preliminary: tool_result.preliminary,
        provider_metadata: tool_result.provider_metadata.clone(),
        tool_metadata: matching_tool_call.and_then(|tool_call| tool_call.tool_metadata.clone()),
    }
}

fn generate_text_tool_result_content_part(
    tool_result: &GenerateTextToolResult,
) -> GenerateTextContentPart {
    if tool_result.is_error == Some(true) {
        GenerateTextContentPart::ToolError(GenerateTextToolError::from_tool_result(tool_result))
    } else {
        GenerateTextContentPart::ToolResult(tool_result.clone())
    }
}

fn extract_sources(content: &[LanguageModelContent]) -> Vec<LanguageModelSource> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::Source(source) => Some(source.clone()),
            _ => None,
        })
        .collect()
}

fn extract_files(content: &[LanguageModelContent]) -> Vec<GeneratedFile> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::File(file) => Some(GeneratedFile::from_language_model_file(file)),
            _ => None,
        })
        .collect()
}

fn extract_reasoning(content: &[LanguageModelContent]) -> Vec<GenerateTextReasoning> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::Reasoning(reasoning) => Some(GenerateTextReasoning::Reasoning(
                ReasoningOutput::from_language_model_reasoning(reasoning),
            )),
            LanguageModelContent::ReasoningFile(file) => {
                Some(GenerateTextReasoning::ReasoningFile(
                    ReasoningFileOutput::from_language_model_reasoning_file(file),
                ))
            }
            _ => None,
        })
        .collect()
}

fn extract_reasoning_text(reasoning: &[GenerateTextReasoning]) -> Option<String> {
    let text = reasoning
        .iter()
        .filter_map(|part| match part {
            GenerateTextReasoning::Reasoning(reasoning) => Some(reasoning.text.as_str()),
            GenerateTextReasoning::ReasoningFile(_) => None,
        })
        .collect::<String>();

    if text.is_empty() { None } else { Some(text) }
}

fn extract_provider_tool_results(
    content: &[LanguageModelContent],
    tool_calls: &[GenerateTextToolCall],
) -> Vec<GenerateTextToolResult> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::ToolResult(tool_result) => {
                let input = tool_calls
                    .iter()
                    .find(|tool_call| tool_call.tool_call_id == tool_result.tool_call_id)
                    .map_or(JsonValue::Null, |tool_call| tool_call.input.clone());

                Some(GenerateTextToolResult {
                    tool_call_id: tool_result.tool_call_id.clone(),
                    tool_name: tool_result.tool_name.clone(),
                    input,
                    output: tool_result.result.as_value().clone(),
                    title: tool_calls
                        .iter()
                        .find(|tool_call| tool_call.tool_call_id == tool_result.tool_call_id)
                        .and_then(|tool_call| tool_call.title.clone()),
                    is_error: tool_result.is_error,
                    provider_executed: Some(true),
                    dynamic: tool_result.dynamic,
                    preliminary: tool_result.preliminary,
                    provider_metadata: tool_result.provider_metadata.clone(),
                    tool_metadata: None,
                })
            }
            _ => None,
        })
        .collect()
}

fn static_tool_results(tool_results: &[GenerateTextToolResult]) -> Vec<GenerateTextToolResult> {
    tool_results
        .iter()
        .filter(|tool_result| tool_result.dynamic != Some(true))
        .cloned()
        .collect()
}

fn dynamic_tool_results(tool_results: &[GenerateTextToolResult]) -> Vec<GenerateTextToolResult> {
    tool_results
        .iter()
        .filter(|tool_result| tool_result.dynamic == Some(true))
        .cloned()
        .collect()
}

pub(crate) fn refresh_tool_result_views(step: &mut GenerateTextStep) {
    step.static_tool_results = static_tool_results(&step.tool_results);
    step.dynamic_tool_results = dynamic_tool_results(&step.tool_results);
}

fn update_pending_deferred_provider_tool_calls(
    pending_tool_call_ids: &mut BTreeSet<String>,
    step: &GenerateTextStep,
    tools: &[Tool],
) {
    let provider_tool_result_ids = step
        .tool_results
        .iter()
        .filter(|tool_result| tool_result.provider_executed == Some(true))
        .map(|tool_result| tool_result.tool_call_id.clone())
        .collect::<BTreeSet<_>>();

    for tool_call_id in &provider_tool_result_ids {
        pending_tool_call_ids.remove(tool_call_id);
    }

    for tool_call in step
        .tool_calls
        .iter()
        .filter(|tool_call| tool_call.provider_executed == Some(true))
    {
        let supports_deferred_results = tools
            .iter()
            .find(|tool| tool.name == tool_call.tool_name)
            .and_then(Tool::supports_deferred_results)
            == Some(true);

        if supports_deferred_results && !provider_tool_result_ids.contains(&tool_call.tool_call_id)
        {
            pending_tool_call_ids.insert(tool_call.tool_call_id.clone());
        }
    }
}

async fn initial_tool_approval_response_message(
    call_id: &str,
    prompt: &LanguageModelPrompt,
    tools: &[Tool],
    tools_context: &JsonObject,
    experimental_sandbox: Option<&Arc<dyn ExperimentalSandbox>>,
    tool_execution_callbacks: (
        Option<&GenerateTextOnToolExecutionStart<'_>>,
        Option<&GenerateTextOnToolExecutionEnd<'_>>,
    ),
) -> Option<LanguageModelMessage> {
    let approvals = collect_tool_approvals(prompt).ok()?;
    let mut approved_tool_calls = approvals
        .approved_tool_approvals
        .iter()
        .filter(|approval| approval.tool_call.provider_executed != Some(true))
        .map(|approval| generate_text_tool_call_from_prompt_part(&approval.tool_call))
        .collect::<Vec<_>>();

    mark_runtime_dynamic_tool_calls(&mut approved_tool_calls, tools);
    mark_tool_call_titles(&mut approved_tool_calls, tools);
    mark_tool_call_metadata(&mut approved_tool_calls, tools);

    let (tool_results, _) = execute_tool_calls(
        call_id,
        tools,
        &approved_tool_calls,
        prompt,
        tools_context,
        &BTreeSet::new(),
        (
            experimental_sandbox,
            tool_execution_callbacks.0,
            tool_execution_callbacks.1,
        ),
    )
    .await;

    let mut content = Vec::new();
    for tool_result in &tool_results {
        let tool = find_tool_for_result(tools, tool_result);
        let mut part = LanguageModelToolResultPart::new(
            tool_result.tool_call_id.clone(),
            tool_result.tool_name.clone(),
            tool_result_output(tool_result, tool).await,
        );

        if let Some(provider_metadata) = &tool_result.provider_metadata {
            part = part.with_provider_options(provider_metadata.clone());
        }

        content.push(LanguageModelToolContentPart::ToolResult(part));
    }

    content.extend(
        approvals
            .denied_tool_approvals
            .iter()
            .map(denied_initial_tool_approval_result_part)
            .map(LanguageModelToolContentPart::ToolResult),
    );

    if content.is_empty() {
        None
    } else {
        Some(LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            content,
        )))
    }
}

fn generate_text_tool_call_from_prompt_part(
    part: &LanguageModelToolCallPart,
) -> GenerateTextToolCall {
    GenerateTextToolCall {
        tool_call_id: part.tool_call_id.clone(),
        tool_name: part.tool_name.clone(),
        input: part.input.clone(),
        title: None,
        provider_executed: part.provider_executed,
        dynamic: None,
        invalid: None,
        error: None,
        provider_metadata: part.provider_options.clone(),
        tool_metadata: None,
    }
}

fn denied_initial_tool_approval_result_part(
    approval: &CollectedToolApproval,
) -> LanguageModelToolResultPart {
    LanguageModelToolResultPart::new(
        approval.tool_call.tool_call_id.clone(),
        approval.tool_call.tool_name.clone(),
        denied_initial_tool_approval_output(approval),
    )
}

fn denied_initial_tool_approval_output(
    approval: &CollectedToolApproval,
) -> LanguageModelToolResultOutput {
    let mut output = LanguageModelToolResultOutput::execution_denied();

    if let Some(reason) = &approval.approval_response.reason {
        output = output.with_reason(reason.clone());
    }

    if approval.tool_call.provider_executed == Some(true) {
        output = output.with_provider_options(provider_executed_approval_provider_options(
            &approval.approval_response.approval_id,
        ));
    }

    output
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StepToolApprovals {
    pub(crate) requests: Vec<LanguageModelToolApprovalRequestPart>,
    pub(crate) responses: Vec<StepToolApprovalResponse>,
    pub(crate) blocked_tool_call_ids: BTreeSet<String>,
    pub(crate) denied_client_tool_call_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct StepToolApprovalResponse {
    pub(crate) response: LanguageModelToolApprovalResponsePart,
    pub(crate) tool_call: GenerateTextToolCall,
}

pub(crate) async fn resolve_tool_approvals_for_step(
    tool_calls: &[GenerateTextToolCall],
    tools: &[Tool],
    tool_approval: Option<&ToolApprovalConfiguration>,
    messages: &LanguageModelPrompt,
    tools_context: &JsonObject,
    runtime_context: &JsonObject,
) -> StepToolApprovals {
    let mut approvals = StepToolApprovals::default();

    for tool_call in tool_calls {
        if tool_call.invalid == Some(true) {
            continue;
        }

        if !tools.iter().any(|tool| tool.name == tool_call.tool_name) {
            continue;
        }

        let status = resolve_tool_approval(
            ResolveToolApprovalOptions::new(tool_call)
                .with_tools(tools)
                .with_optional_tool_approval(tool_approval)
                .with_messages(messages)
                .with_tools_context(tools_context)
                .with_runtime_context(runtime_context),
        )
        .await;

        match status {
            NormalizedToolApprovalStatus::NotApplicable => {}
            NormalizedToolApprovalStatus::UserApproval => {
                approvals
                    .blocked_tool_call_ids
                    .insert(tool_call.tool_call_id.clone());
                approvals
                    .requests
                    .push(LanguageModelToolApprovalRequestPart::new(
                        generate_id(),
                        tool_call.tool_call_id.clone(),
                    ));
            }
            NormalizedToolApprovalStatus::Approved { reason } => {
                let approval_id = generate_id();
                approvals.requests.push(
                    LanguageModelToolApprovalRequestPart::new(
                        approval_id.clone(),
                        tool_call.tool_call_id.clone(),
                    )
                    .with_automatic(true),
                );
                approvals.responses.push(StepToolApprovalResponse {
                    response: tool_approval_response_part(approval_id, true, reason),
                    tool_call: tool_call.clone(),
                });
            }
            NormalizedToolApprovalStatus::Denied { reason } => {
                let approval_id = generate_id();
                approvals
                    .blocked_tool_call_ids
                    .insert(tool_call.tool_call_id.clone());
                if tool_call.provider_executed != Some(true) {
                    approvals.denied_client_tool_call_count += 1;
                }
                approvals.requests.push(
                    LanguageModelToolApprovalRequestPart::new(
                        approval_id.clone(),
                        tool_call.tool_call_id.clone(),
                    )
                    .with_automatic(true),
                );
                approvals.responses.push(StepToolApprovalResponse {
                    response: tool_approval_response_part(approval_id, false, reason),
                    tool_call: tool_call.clone(),
                });
            }
        }
    }

    approvals
}

fn tool_approval_response_part(
    approval_id: String,
    approved: bool,
    reason: Option<String>,
) -> LanguageModelToolApprovalResponsePart {
    let mut response = LanguageModelToolApprovalResponsePart::new(approval_id, approved);
    if let Some(reason) = reason {
        response = response.with_reason(reason);
    }
    response
}

pub(crate) async fn execute_tool_calls(
    call_id: &str,
    tools: &[Tool],
    tool_calls: &[GenerateTextToolCall],
    messages: &LanguageModelPrompt,
    tools_context: &JsonObject,
    blocked_tool_call_ids: &BTreeSet<String>,
    tool_execution_context: (
        Option<&Arc<dyn ExperimentalSandbox>>,
        Option<&GenerateTextOnToolExecutionStart<'_>>,
        Option<&GenerateTextOnToolExecutionEnd<'_>>,
    ),
) -> (Vec<GenerateTextToolResult>, BTreeMap<String, u64>) {
    let mut tool_results = Vec::new();
    let mut tool_execution_ms = BTreeMap::new();
    let (experimental_sandbox, on_tool_execution_start, on_tool_execution_end) =
        tool_execution_context;

    for tool_call in tool_calls {
        if tool_call.provider_executed == Some(true) {
            continue;
        }

        if blocked_tool_call_ids.contains(&tool_call.tool_call_id) {
            continue;
        }

        if tool_call.invalid == Some(true) {
            tool_results.push(GenerateTextToolResult::error(
                tool_call,
                tool_call
                    .error
                    .clone()
                    .unwrap_or_else(|| "Invalid tool call.".to_string()),
            ));
            continue;
        }

        let Some(tool) = tools.iter().find(|tool| tool.name == tool_call.tool_name) else {
            continue;
        };

        if !tool.is_executable() {
            continue;
        }

        let tool_context =
            match validate_tool_execution_context(tool, tools_context.get(&tool_call.tool_name)) {
                Ok(tool_context) => tool_context,
                Err(error) => {
                    tool_results.push(GenerateTextToolResult::error(tool_call, error.to_string()));
                    continue;
                }
            };
        if let Some(on_tool_execution_start) = on_tool_execution_start {
            on_tool_execution_start
                .start(GenerateTextToolExecutionStartEvent {
                    call_id: call_id.to_string(),
                    messages: messages.clone(),
                    tool_call: tool_call.clone(),
                    tool_context: tool_context.clone(),
                })
                .await;
        }

        let mut execution_options =
            ToolExecutionOptions::new(tool_call.tool_call_id.clone(), messages.clone());
        if let Some(context) = &tool_context {
            execution_options = execution_options.with_context(context.clone());
        }
        if let Some(experimental_sandbox) = experimental_sandbox {
            execution_options =
                execution_options.with_experimental_sandbox(Arc::clone(experimental_sandbox));
        }

        let Some(execute) = tool.execute(tool_call.input.clone(), execution_options) else {
            continue;
        };

        let tool_started_at = Instant::now();
        let tool_result = match execute.await {
            Ok(output) => GenerateTextToolResult::success(tool_call, output),
            Err(error) => GenerateTextToolResult::error(tool_call, error.into_message()),
        };
        let elapsed_ms = duration_ms(tool_started_at.elapsed());

        if let Some(on_tool_execution_end) = on_tool_execution_end {
            on_tool_execution_end
                .end(GenerateTextToolExecutionEndEvent {
                    call_id: call_id.to_string(),
                    messages: messages.clone(),
                    tool_call: tool_call.clone(),
                    tool_context,
                    tool_execution_ms: elapsed_ms,
                    tool_output: tool_result.clone(),
                })
                .await;
        }

        tool_execution_ms.insert(tool_call.tool_call_id.clone(), elapsed_ms);
        tool_results.push(tool_result);
    }

    (tool_results, tool_execution_ms)
}

fn validate_tool_execution_context(
    tool: &Tool,
    context: Option<&JsonValue>,
) -> Result<Option<JsonValue>, TypeValidationError> {
    let Some(context_schema) = tool.context_schema() else {
        return Ok(context.cloned());
    };

    let value = context.cloned().unwrap_or(JsonValue::Null);

    crate::provider_utils::validate_types(
        value,
        context_schema.clone(),
        Some(
            TypeValidationContext::new()
                .with_field("tool context")
                .with_entity_name(tool.name.clone()),
        ),
    )
    .map(Some)
}

pub(crate) fn should_continue_after_tool_results(
    step: &GenerateTextStep,
    tool_results: &[GenerateTextToolResult],
    denied_client_tool_call_count: usize,
    has_pending_deferred_provider_tool_call: bool,
) -> bool {
    let client_tool_call_count = step
        .tool_calls
        .iter()
        .filter(|tool_call| tool_call.provider_executed != Some(true))
        .count();

    has_pending_deferred_provider_tool_call
        || (client_tool_call_count > 0
            && tool_results.len() + denied_client_tool_call_count == client_tool_call_count)
}

pub(crate) async fn response_messages_for_step(
    step: &GenerateTextStep,
    provider_content: &[LanguageModelContent],
    tool_approvals: &StepToolApprovals,
    tools: &[Tool],
) -> Option<Vec<LanguageModelMessage>> {
    let mut messages = Vec::new();

    if let Some(message) =
        assistant_message_from_step(step, provider_content, &tool_approvals.requests)
    {
        messages.push(message);
    }

    let client_tool_results = step
        .tool_results
        .iter()
        .filter(|tool_result| tool_result.provider_executed != Some(true))
        .cloned()
        .collect::<Vec<_>>();

    if let Some(message) = tool_message_from_results_and_approvals(
        &client_tool_results,
        &tool_approvals.responses,
        tools,
    )
    .await
    {
        messages.push(message);
    }

    if messages.is_empty() {
        None
    } else {
        Some(messages)
    }
}

fn assistant_message_from_step(
    step: &GenerateTextStep,
    provider_content: &[LanguageModelContent],
    approval_requests: &[LanguageModelToolApprovalRequestPart],
) -> Option<LanguageModelMessage> {
    let mut parts = provider_content
        .iter()
        .filter_map(|content| assistant_content_part_from_content(content, &step.tool_calls))
        .collect::<Vec<_>>();
    parts.extend(
        approval_requests
            .iter()
            .cloned()
            .map(LanguageModelAssistantContentPart::ToolApprovalRequest),
    );

    if parts.is_empty() {
        None
    } else {
        Some(LanguageModelMessage::Assistant(
            LanguageModelAssistantMessage::new(parts),
        ))
    }
}

fn assistant_content_part_from_content(
    content: &LanguageModelContent,
    tool_calls: &[GenerateTextToolCall],
) -> Option<LanguageModelAssistantContentPart> {
    match content {
        LanguageModelContent::Text(text) => {
            if text.text.is_empty() {
                return None;
            }

            let mut part = LanguageModelTextPart::new(text.text.clone());

            if let Some(provider_metadata) = &text.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::Text(part))
        }
        LanguageModelContent::Reasoning(reasoning) => {
            let mut part = LanguageModelReasoningPart::new(reasoning.text.clone());

            if let Some(provider_metadata) = &reasoning.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::Reasoning(part))
        }
        LanguageModelContent::Custom(custom) => {
            let mut part = LanguageModelCustomPart::new(custom.kind.clone());

            if let Some(provider_metadata) = &custom.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::Custom(part))
        }
        LanguageModelContent::File(file) => {
            let mut part = LanguageModelFilePart::new(
                file_data_from_language_model_file_data(file.data.clone()),
                file.media_type.clone(),
            );

            if let Some(provider_metadata) = &file.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::File(part))
        }
        LanguageModelContent::ReasoningFile(file) => {
            let mut part =
                LanguageModelReasoningFilePart::new(file.data.clone(), file.media_type.clone());

            if let Some(provider_metadata) = &file.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::ReasoningFile(part))
        }
        LanguageModelContent::ToolCall(tool_call) => {
            let input = tool_calls
                .iter()
                .find(|parsed| parsed.tool_call_id == tool_call.tool_call_id)
                .map_or_else(
                    || parse_tool_input_or_raw(&tool_call.input),
                    tool_call_response_input,
                );
            let mut part = LanguageModelToolCallPart::new(
                tool_call.tool_call_id.clone(),
                tool_call.tool_name.clone(),
                input,
            );

            if let Some(provider_executed) = tool_call.provider_executed {
                part = part.with_provider_executed(provider_executed);
            }

            if let Some(provider_metadata) = &tool_call.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::ToolCall(part))
        }
        LanguageModelContent::ToolResult(tool_result) => {
            let mut part = LanguageModelToolResultPart::new(
                tool_result.tool_call_id.clone(),
                tool_result.tool_name.clone(),
                provider_tool_result_output(tool_result),
            );

            if let Some(provider_metadata) = &tool_result.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            Some(LanguageModelAssistantContentPart::ToolResult(part))
        }
        LanguageModelContent::ToolApprovalRequest(request) => {
            Some(LanguageModelAssistantContentPart::ToolApprovalRequest(
                LanguageModelToolApprovalRequestPart::new(
                    request.approval_id.clone(),
                    request.tool_call_id.clone(),
                ),
            ))
        }
        LanguageModelContent::Source(_) => None,
    }
}

async fn tool_message_from_results_and_approvals(
    tool_results: &[GenerateTextToolResult],
    approval_responses: &[StepToolApprovalResponse],
    tools: &[Tool],
) -> Option<LanguageModelMessage> {
    let approval_tool_call_ids = approval_responses
        .iter()
        .map(|approval| approval.tool_call.tool_call_id.clone())
        .collect::<BTreeSet<_>>();
    let mut content = Vec::new();

    for tool_result in tool_results
        .iter()
        .filter(|tool_result| !approval_tool_call_ids.contains(&tool_result.tool_call_id))
    {
        let tool = find_tool_for_result(tools, tool_result);
        let mut part = LanguageModelToolResultPart::new(
            tool_result.tool_call_id.clone(),
            tool_result.tool_name.clone(),
            tool_result_output(tool_result, tool).await,
        );

        if let Some(provider_metadata) = &tool_result.provider_metadata {
            part = part.with_provider_options(provider_metadata.clone());
        }

        content.push(LanguageModelToolContentPart::ToolResult(part));
    }

    content.extend(
        approval_responses
            .iter()
            .flat_map(tool_approval_response_content),
    );

    for tool_result in tool_results
        .iter()
        .filter(|tool_result| approval_tool_call_ids.contains(&tool_result.tool_call_id))
    {
        let tool = find_tool_for_result(tools, tool_result);
        let mut part = LanguageModelToolResultPart::new(
            tool_result.tool_call_id.clone(),
            tool_result.tool_name.clone(),
            tool_result_output(tool_result, tool).await,
        );

        if let Some(provider_metadata) = &tool_result.provider_metadata {
            part = part.with_provider_options(provider_metadata.clone());
        }

        content.push(LanguageModelToolContentPart::ToolResult(part));
    }

    if content.is_empty() {
        None
    } else {
        Some(LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            content,
        )))
    }
}

fn tool_approval_response_content(
    approval_response: &StepToolApprovalResponse,
) -> Vec<LanguageModelToolContentPart> {
    let mut content = vec![LanguageModelToolContentPart::ToolApprovalResponse(
        approval_response.response.clone(),
    )];

    if !approval_response.response.approved {
        content.push(LanguageModelToolContentPart::ToolResult(
            LanguageModelToolResultPart::new(
                approval_response.tool_call.tool_call_id.clone(),
                approval_response.tool_call.tool_name.clone(),
                denied_tool_result_output(approval_response),
            ),
        ));
    }

    content
}

fn denied_tool_result_output(
    approval_response: &StepToolApprovalResponse,
) -> LanguageModelToolResultOutput {
    let mut output = LanguageModelToolResultOutput::execution_denied();

    if let Some(reason) = &approval_response.response.reason {
        output = output.with_reason(reason.clone());
    }

    if approval_response.tool_call.provider_executed == Some(true) {
        output = output.with_provider_options(provider_executed_approval_provider_options(
            &approval_response.response.approval_id,
        ));
    }

    output
}

fn provider_executed_approval_provider_options(approval_id: &str) -> ProviderOptions {
    let mut openai_options = JsonObject::new();
    openai_options.insert(
        "approvalId".to_string(),
        JsonValue::String(approval_id.to_string()),
    );

    let mut provider_options = ProviderOptions::new();
    provider_options.insert("openai".to_string(), openai_options);
    provider_options
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolModelOutputErrorMode {
    None,
    Text,
}

fn find_tool_for_result<'a>(
    tools: &'a [Tool],
    tool_result: &GenerateTextToolResult,
) -> Option<&'a Tool> {
    tools.iter().find(|tool| tool.name == tool_result.tool_name)
}

async fn tool_result_output(
    tool_result: &GenerateTextToolResult,
    tool: Option<&Tool>,
) -> LanguageModelToolResultOutput {
    if tool_result.is_error == Some(true) {
        return create_tool_model_output(
            &tool_result.tool_call_id,
            &tool_result.input,
            &tool_result.output,
            tool,
            ToolModelOutputErrorMode::Text,
        )
        .await;
    }

    create_tool_model_output(
        &tool_result.tool_call_id,
        &tool_result.input,
        &tool_result.output,
        tool,
        ToolModelOutputErrorMode::None,
    )
    .await
}

async fn create_tool_model_output(
    tool_call_id: &str,
    input: &JsonValue,
    output: &JsonValue,
    tool: Option<&Tool>,
    error_mode: ToolModelOutputErrorMode,
) -> LanguageModelToolResultOutput {
    match error_mode {
        ToolModelOutputErrorMode::Text => {
            if let JsonValue::String(message) = output {
                return LanguageModelToolResultOutput::error_text(message.clone());
            }

            return LanguageModelToolResultOutput::error_text(get_error_message(Some(
                output as &dyn fmt::Display,
            )));
        }
        ToolModelOutputErrorMode::None => {}
    }

    if let Some(model_output) = tool.and_then(|tool| {
        tool.model_output(ToolModelOutputOptions::new(
            tool_call_id,
            input.clone(),
            output.clone(),
        ))
    }) {
        return model_output.await;
    }

    match output {
        JsonValue::String(output) => LanguageModelToolResultOutput::text(output.clone()),
        output => LanguageModelToolResultOutput::json(output.clone()),
    }
}

fn provider_tool_result_output(
    tool_result: &LanguageModelToolResult,
) -> LanguageModelToolResultOutput {
    if tool_result.is_error == Some(true) {
        return LanguageModelToolResultOutput::error_json(tool_result.result.as_value().clone());
    }

    match tool_result.result.as_value() {
        JsonValue::String(output) => LanguageModelToolResultOutput::text(output.clone()),
        output => LanguageModelToolResultOutput::json(output.clone()),
    }
}

fn file_data_from_language_model_file_data(data: LanguageModelFileData) -> FileData {
    match data {
        LanguageModelFileData::Data { data } => FileData::Data { data },
        LanguageModelFileData::Url { url } => FileData::Url { url },
    }
}

fn parse_tool_input(input: &str) -> Result<JsonValue, serde_json::Error> {
    if input.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }

    serde_json::from_str(input)
}

fn parse_tool_input_or_raw(input: &str) -> JsonValue {
    parse_tool_input(input).unwrap_or_else(|_| JsonValue::String(input.to_string()))
}

fn tool_call_response_input(tool_call: &GenerateTextToolCall) -> JsonValue {
    if tool_call.invalid == Some(true)
        && matches!(
            tool_call.input,
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_)
        )
    {
        JsonValue::Object(Default::default())
    } else {
        tool_call.input.clone()
    }
}

fn invalid_tool_input_message(
    tool_name: &str,
    input: &str,
    cause: impl std::fmt::Display,
) -> String {
    InvalidToolInputError::new(tool_name, input, JsonParseError::new(input, cause)).to_string()
}

pub(crate) fn mark_unavailable_tool_calls(
    tool_calls: &mut [GenerateTextToolCall],
    available_tools: Option<&[LanguageModelTool]>,
) {
    let available_tool_names = available_tools.map(|tools| {
        tools
            .iter()
            .map(language_model_tool_name)
            .collect::<Vec<_>>()
    });
    let available_tool_names_slice = available_tool_names.as_deref().unwrap_or_default();

    for tool_call in tool_calls {
        if tool_call.provider_executed == Some(true) || tool_call.invalid == Some(true) {
            continue;
        }

        if available_tool_names_slice
            .iter()
            .any(|tool_name| tool_name == &tool_call.tool_name)
        {
            continue;
        }

        tool_call.dynamic = Some(true);
        tool_call.invalid = Some(true);
        tool_call.error = Some(no_such_tool_message(
            &tool_call.tool_name,
            available_tool_names.as_deref(),
        ));
    }
}

pub(crate) async fn repair_tool_calls(
    tool_calls: &mut [GenerateTextToolCall],
    content: &[LanguageModelContent],
    repair: Option<&ToolCallRepair>,
    tools: &[Tool],
    available_tools: Option<&[LanguageModelTool]>,
    messages: &LanguageModelPrompt,
) {
    let Some(repair) = repair else {
        return;
    };

    for tool_call in tool_calls {
        if tool_call.invalid != Some(true) {
            continue;
        }

        let Some(original_tool_call) = original_language_model_tool_call(content, tool_call) else {
            continue;
        };
        let Some(original_error) =
            tool_call_repair_original_error(original_tool_call, available_tools)
        else {
            continue;
        };

        let options = ToolCallRepairOptions::new(
            original_tool_call.clone(),
            tools.to_vec(),
            messages.clone(),
            original_error.clone(),
        );

        match repair.repair(options).await {
            Ok(Some(repaired_tool_call)) => {
                match parse_repaired_tool_call(&repaired_tool_call, available_tools) {
                    Ok(repaired_tool_call) => {
                        *tool_call = repaired_tool_call;
                    }
                    Err(repaired_error) => {
                        tool_call.dynamic = Some(true);
                        tool_call.invalid = Some(true);
                        tool_call.error = Some(repaired_error.to_string());
                    }
                }
            }
            Ok(None) => {
                tool_call.error = Some(original_error.to_string());
            }
            Err(cause_message) => {
                tool_call.error =
                    Some(ToolCallRepairError::new(original_error, cause_message).to_string());
            }
        }
    }
}

fn original_language_model_tool_call<'a>(
    content: &'a [LanguageModelContent],
    tool_call: &GenerateTextToolCall,
) -> Option<&'a LanguageModelToolCall> {
    content.iter().find_map(|part| match part {
        LanguageModelContent::ToolCall(original)
            if original.tool_call_id == tool_call.tool_call_id =>
        {
            Some(original)
        }
        _ => None,
    })
}

fn parse_repaired_tool_call(
    tool_call: &LanguageModelToolCall,
    available_tools: Option<&[LanguageModelTool]>,
) -> Result<GenerateTextToolCall, ToolCallRepairOriginalError> {
    if let Some(error) = tool_call_repair_original_error(tool_call, available_tools) {
        Err(error)
    } else {
        Ok(GenerateTextToolCall::from_language_model_tool_call(
            tool_call,
        ))
    }
}

fn tool_call_repair_original_error(
    tool_call: &LanguageModelToolCall,
    available_tools: Option<&[LanguageModelTool]>,
) -> Option<ToolCallRepairOriginalError> {
    if !is_provider_executed_dynamic_tool_call(tool_call) {
        match available_tool_names(available_tools) {
            None => return Some(NoSuchToolError::new(&tool_call.tool_name).into()),
            Some(available_tool_names)
                if !available_tool_names
                    .iter()
                    .any(|tool_name| tool_name == &tool_call.tool_name) =>
            {
                return Some(
                    NoSuchToolError::with_available_tools(
                        &tool_call.tool_name,
                        available_tool_names,
                    )
                    .into(),
                );
            }
            Some(_) => {}
        }
    }

    parse_tool_input(&tool_call.input).err().map(|error| {
        InvalidToolInputError::new(
            &tool_call.tool_name,
            &tool_call.input,
            JsonParseError::new(&tool_call.input, error),
        )
        .into()
    })
}

fn is_provider_executed_dynamic_tool_call(tool_call: &LanguageModelToolCall) -> bool {
    tool_call.provider_executed == Some(true) && tool_call.dynamic == Some(true)
}

fn available_tool_names(available_tools: Option<&[LanguageModelTool]>) -> Option<Vec<String>> {
    available_tools.map(|tools| tools.iter().map(language_model_tool_name).collect())
}

pub(crate) async fn refine_tool_inputs(
    tool_calls: &mut [GenerateTextToolCall],
    refinements: &BTreeMap<String, ToolInputRefinement>,
) {
    if refinements.is_empty() {
        return;
    }

    for tool_call in tool_calls {
        if tool_call.invalid == Some(true) {
            continue;
        }

        let Some(refinement) = refinements.get(&tool_call.tool_name) else {
            continue;
        };

        match refinement.refine(tool_call.input.clone()).await {
            Ok(input) => {
                tool_call.input = input;
            }
            Err(error) => {
                tool_call.dynamic = Some(true);
                tool_call.invalid = Some(true);
                tool_call.error = Some(error.into_message());
            }
        }
    }
}

pub(crate) fn sync_tool_result_inputs(
    tool_results: &mut [GenerateTextToolResult],
    tool_calls: &[GenerateTextToolCall],
) {
    for tool_result in tool_results {
        if let Some(tool_call) = tool_calls
            .iter()
            .find(|tool_call| tool_call.tool_call_id == tool_result.tool_call_id)
        {
            tool_result.input = tool_call.input.clone();
        }
    }
}

pub(crate) fn mark_runtime_dynamic_tool_calls(
    tool_calls: &mut [GenerateTextToolCall],
    tools: &[Tool],
) {
    for tool_call in tool_calls {
        if tools
            .iter()
            .any(|tool| tool.name == tool_call.tool_name && tool.is_dynamic())
        {
            tool_call.dynamic = Some(true);
        }
    }
}

pub(crate) fn mark_tool_call_titles(tool_calls: &mut [GenerateTextToolCall], tools: &[Tool]) {
    for tool_call in tool_calls {
        if tool_call.title.is_some() {
            continue;
        }

        if let Some(title) = tools
            .iter()
            .find(|tool| tool.name == tool_call.tool_name)
            .and_then(Tool::title)
        {
            tool_call.title = Some(title.to_string());
        }
    }
}

pub(crate) fn mark_tool_call_metadata(tool_calls: &mut [GenerateTextToolCall], tools: &[Tool]) {
    for tool_call in tool_calls {
        if tool_call.tool_metadata.is_some() {
            continue;
        }

        if let Some(metadata) = tools
            .iter()
            .find(|tool| tool.name == tool_call.tool_name)
            .and_then(Tool::metadata)
        {
            tool_call.tool_metadata = Some(metadata.clone());
        }
    }
}

pub(crate) fn mark_tool_result_metadata(
    tool_results: &mut [GenerateTextToolResult],
    tool_calls: &[GenerateTextToolCall],
    tools: &[Tool],
) {
    for tool_result in tool_results {
        if tool_result.title.is_none() {
            if let Some(title) = tool_calls
                .iter()
                .find(|tool_call| tool_call.tool_call_id == tool_result.tool_call_id)
                .and_then(|tool_call| tool_call.title.as_deref())
                .or_else(|| {
                    tools
                        .iter()
                        .find(|tool| tool.name == tool_result.tool_name)
                        .and_then(Tool::title)
                })
            {
                tool_result.title = Some(title.to_string());
            }
        }

        if tool_result.tool_metadata.is_some() {
            continue;
        }

        if let Some(metadata) = tool_calls
            .iter()
            .find(|tool_call| tool_call.tool_call_id == tool_result.tool_call_id)
            .and_then(|tool_call| tool_call.tool_metadata.as_ref())
            .or_else(|| {
                tools
                    .iter()
                    .find(|tool| tool.name == tool_result.tool_name)
                    .and_then(Tool::metadata)
            })
        {
            tool_result.tool_metadata = Some(metadata.clone());
        }
    }
}

fn language_model_tool_name(tool: &LanguageModelTool) -> String {
    match tool {
        LanguageModelTool::Function(tool) => tool.name.clone(),
        LanguageModelTool::Provider(tool) => tool.name.clone(),
    }
}

pub(crate) fn filter_active_language_model_tools(
    tools: Option<Vec<LanguageModelTool>>,
    active_tools: Option<&[String]>,
) -> Option<Vec<LanguageModelTool>> {
    let tools = tools?;

    let Some(active_tools) = active_tools else {
        return Some(tools);
    };

    let tools = tools
        .into_iter()
        .filter(|tool| {
            let tool_name = language_model_tool_name(tool);
            active_tools
                .iter()
                .any(|active_tool| active_tool == &tool_name)
        })
        .collect::<Vec<_>>();

    if tools.is_empty() { None } else { Some(tools) }
}

fn no_such_tool_message(tool_name: &str, available_tool_names: Option<&[String]>) -> String {
    match available_tool_names {
        Some(available_tool_names) => {
            NoSuchToolError::with_available_tools(tool_name, available_tool_names.iter().cloned())
                .to_string()
        }
        None => NoSuchToolError::new(tool_name).to_string(),
    }
}

fn no_such_tool_default_message(
    tool_name: &str,
    available_tool_names: Option<&[String]>,
) -> String {
    match available_tool_names {
        Some(available_tool_names) => format!(
            "Model tried to call unavailable tool '{tool_name}'. Available tools: {}.",
            available_tool_names.join(", ")
        ),
        None => {
            format!("Model tried to call unavailable tool '{tool_name}'. No tools are available.")
        }
    }
}

fn invalid_tool_input_default_message(tool_name: &str, cause_message: &str) -> String {
    format!("Invalid input for tool {tool_name}: {cause_message}")
}

fn tool_call_repair_default_message(cause_message: &str) -> String {
    format!("Error repairing tool call: {cause_message}")
}

fn missing_tool_results_default_message(tool_call_ids: &[String]) -> String {
    let plural = tool_call_ids.len() > 1;
    format!(
        "Tool result{} {} missing for tool call{} {}.",
        if plural { "s" } else { "" },
        if plural { "are" } else { "is" },
        if plural { "s" } else { "" },
        tool_call_ids.join(", ")
    )
}

fn invalid_tool_approval_default_message(approval_id: &str) -> String {
    format!(
        "Tool approval response references unknown approvalId: \"{approval_id}\". \
         No matching tool-approval-request found in message history."
    )
}

fn tool_call_not_found_for_approval_default_message(
    tool_call_id: &str,
    approval_id: &str,
) -> String {
    format!("Tool call \"{tool_call_id}\" not found for approval request \"{approval_id}\".")
}

fn unsupported_model_version_default_message(
    version: &str,
    provider: &str,
    model_id: &str,
) -> String {
    format!(
        "Unsupported model version {version} for provider \"{provider}\" and model \"{model_id}\". \
         AI SDK 5 only supports models that implement specification version \"v2\"."
    )
}

fn parse_prune_tool_calls(value: &str) -> Option<PruneToolCalls> {
    match value {
        "none" => Some(PruneToolCalls::None),
        "all" => Some(PruneToolCalls::All),
        "before-last-message" => Some(PruneToolCalls::BeforeLastMessage),
        value => parse_before_last_messages(value).map(PruneToolCalls::BeforeLastMessages),
    }
}

fn parse_before_last_messages(value: &str) -> Option<usize> {
    value
        .strip_prefix("before-last-")?
        .strip_suffix("-messages")?
        .parse()
        .ok()
}

fn prune_reasoning_messages(
    messages: Vec<LanguageModelMessage>,
    reasoning: PruneReasoning,
) -> Vec<LanguageModelMessage> {
    if reasoning == PruneReasoning::None {
        return messages;
    }

    let final_index = messages.len().saturating_sub(1);

    messages
        .into_iter()
        .enumerate()
        .map(|(message_index, message)| match message {
            LanguageModelMessage::Assistant(mut message)
                if reasoning == PruneReasoning::All || message_index != final_index =>
            {
                message.content.retain(|part| {
                    !matches!(part, LanguageModelAssistantContentPart::Reasoning(_))
                });
                LanguageModelMessage::Assistant(message)
            }
            message => message,
        })
        .collect()
}

fn prune_tool_call_messages(
    messages: Vec<LanguageModelMessage>,
    rule: &PruneToolCallRule,
) -> Vec<LanguageModelMessage> {
    let keep_last_messages_count = rule.mode.keep_last_messages_count();
    let (kept_tool_call_ids, kept_approval_ids) =
        collect_kept_tool_references(&messages, keep_last_messages_count);
    let prune_before_index =
        keep_last_messages_count.map(|count| messages.len().saturating_sub(count));

    messages
        .into_iter()
        .enumerate()
        .map(|(message_index, message)| {
            if prune_before_index.is_some_and(|index| message_index >= index) {
                return message;
            }

            match message {
                LanguageModelMessage::Assistant(mut message) => {
                    message.content = prune_assistant_tool_parts(
                        message.content,
                        rule,
                        &kept_tool_call_ids,
                        &kept_approval_ids,
                    );
                    LanguageModelMessage::Assistant(message)
                }
                LanguageModelMessage::Tool(mut message) => {
                    message.content = prune_tool_message_parts(
                        message.content,
                        rule,
                        &kept_tool_call_ids,
                        &kept_approval_ids,
                    );
                    LanguageModelMessage::Tool(message)
                }
                message => message,
            }
        })
        .collect()
}

fn collect_kept_tool_references(
    messages: &[LanguageModelMessage],
    keep_last_messages_count: Option<usize>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut kept_tool_call_ids = BTreeSet::new();
    let mut kept_approval_ids = BTreeSet::new();

    let Some(keep_last_messages_count) = keep_last_messages_count else {
        return (kept_tool_call_ids, kept_approval_ids);
    };

    for message in messages.iter().rev().take(keep_last_messages_count) {
        match message {
            LanguageModelMessage::Assistant(message) => {
                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::ToolCall(part) => {
                            kept_tool_call_ids.insert(part.tool_call_id.clone());
                        }
                        LanguageModelAssistantContentPart::ToolResult(part) => {
                            kept_tool_call_ids.insert(part.tool_call_id.clone());
                        }
                        LanguageModelAssistantContentPart::ToolApprovalRequest(part) => {
                            kept_approval_ids.insert(part.approval_id.clone());
                        }
                        _ => {}
                    }
                }
            }
            LanguageModelMessage::Tool(message) => {
                for part in &message.content {
                    match part {
                        LanguageModelToolContentPart::ToolResult(part) => {
                            kept_tool_call_ids.insert(part.tool_call_id.clone());
                        }
                        LanguageModelToolContentPart::ToolApprovalResponse(part) => {
                            kept_approval_ids.insert(part.approval_id.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    (kept_tool_call_ids, kept_approval_ids)
}

fn prune_assistant_tool_parts(
    content: Vec<LanguageModelAssistantContentPart>,
    rule: &PruneToolCallRule,
    kept_tool_call_ids: &BTreeSet<String>,
    kept_approval_ids: &BTreeSet<String>,
) -> Vec<LanguageModelAssistantContentPart> {
    let mut tool_call_id_to_tool_name = BTreeMap::new();
    let mut approval_id_to_tool_name = BTreeMap::new();

    content
        .into_iter()
        .filter(|part| match part {
            LanguageModelAssistantContentPart::ToolCall(part) => {
                tool_call_id_to_tool_name.insert(part.tool_call_id.clone(), part.tool_name.clone());

                should_keep_tool_related_part(
                    Some(&part.tool_call_id),
                    None,
                    Some(&part.tool_name),
                    rule,
                    kept_tool_call_ids,
                    kept_approval_ids,
                )
            }
            LanguageModelAssistantContentPart::ToolResult(part) => should_keep_tool_related_part(
                Some(&part.tool_call_id),
                None,
                Some(&part.tool_name),
                rule,
                kept_tool_call_ids,
                kept_approval_ids,
            ),
            LanguageModelAssistantContentPart::ToolApprovalRequest(part) => {
                if let Some(tool_name) = tool_call_id_to_tool_name.get(&part.tool_call_id) {
                    approval_id_to_tool_name.insert(part.approval_id.clone(), tool_name.clone());
                }

                should_keep_tool_related_part(
                    None,
                    Some(&part.approval_id),
                    approval_id_to_tool_name
                        .get(&part.approval_id)
                        .map(String::as_str),
                    rule,
                    kept_tool_call_ids,
                    kept_approval_ids,
                )
            }
            _ => true,
        })
        .collect()
}

fn prune_tool_message_parts(
    content: Vec<LanguageModelToolContentPart>,
    rule: &PruneToolCallRule,
    kept_tool_call_ids: &BTreeSet<String>,
    kept_approval_ids: &BTreeSet<String>,
) -> Vec<LanguageModelToolContentPart> {
    content
        .into_iter()
        .filter(|part| match part {
            LanguageModelToolContentPart::ToolResult(part) => should_keep_tool_related_part(
                Some(&part.tool_call_id),
                None,
                Some(&part.tool_name),
                rule,
                kept_tool_call_ids,
                kept_approval_ids,
            ),
            LanguageModelToolContentPart::ToolApprovalResponse(part) => {
                should_keep_tool_related_part(
                    None,
                    Some(&part.approval_id),
                    None,
                    rule,
                    kept_tool_call_ids,
                    kept_approval_ids,
                )
            }
        })
        .collect()
}

fn should_keep_tool_related_part(
    tool_call_id: Option<&str>,
    approval_id: Option<&str>,
    tool_name: Option<&str>,
    rule: &PruneToolCallRule,
    kept_tool_call_ids: &BTreeSet<String>,
    kept_approval_ids: &BTreeSet<String>,
) -> bool {
    if tool_call_id.is_some_and(|id| kept_tool_call_ids.contains(id))
        || approval_id.is_some_and(|id| kept_approval_ids.contains(id))
    {
        return true;
    }

    let Some(tools) = rule.tools.as_deref() else {
        return false;
    };

    let Some(tool_name) = tool_name else {
        return true;
    };

    !tools.iter().any(|tool| tool == tool_name)
}

fn message_has_content(message: &LanguageModelMessage) -> bool {
    match message {
        LanguageModelMessage::System(message) => !message.content.is_empty(),
        LanguageModelMessage::User(message) => !message.content.is_empty(),
        LanguageModelMessage::Assistant(message) => !message.content.is_empty(),
        LanguageModelMessage::Tool(message) => !message.content.is_empty(),
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn calculate_tokens_per_second(tokens: Option<u64>, duration_ms: u64) -> f64 {
    if duration_ms == 0 {
        return 0.0;
    }

    let token_rate = (1000.0 * tokens.unwrap_or(0) as f64) / duration_ms as f64;

    if token_rate.is_finite() {
        token_rate
    } else {
        0.0
    }
}

fn sum_token_counts(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0) + right.unwrap_or(0)),
    }
}

fn add_step_usage(steps: &[GenerateTextStep]) -> LanguageModelUsage {
    steps
        .iter()
        .fold(LanguageModelUsage::default(), |usage, step| {
            add_usage(usage, &step.usage)
        })
}

fn add_usage(mut usage: LanguageModelUsage, next: &LanguageModelUsage) -> LanguageModelUsage {
    usage.input_tokens = add_input_token_usage(usage.input_tokens, &next.input_tokens);
    usage.output_tokens = add_output_token_usage(usage.output_tokens, &next.output_tokens);
    usage
}

fn add_input_token_usage(usage: InputTokenUsage, next: &InputTokenUsage) -> InputTokenUsage {
    InputTokenUsage {
        total: add_optional_counts(usage.total, next.total),
        no_cache: add_optional_counts(usage.no_cache, next.no_cache),
        cache_read: add_optional_counts(usage.cache_read, next.cache_read),
        cache_write: add_optional_counts(usage.cache_write, next.cache_write),
    }
}

fn add_output_token_usage(usage: OutputTokenUsage, next: &OutputTokenUsage) -> OutputTokenUsage {
    OutputTokenUsage {
        total: add_optional_counts(usage.total, next.total),
        text: add_optional_counts(usage.text, next.text),
        reasoning: add_optional_counts(usage.reasoning, next.reasoning),
    }
}

fn add_optional_counts(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0) + right.unwrap_or(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveTools, ContentPart, DefaultGeneratedFile, DynamicToolCall, DynamicToolError,
        DynamicToolResult, ExperimentalGeneratedImage, GenerateTextContentPart,
        GenerateTextEndEvent, GenerateTextFileContent, GenerateTextFinishEvent,
        GenerateTextInclude, GenerateTextModelInfo, GenerateTextOptions, GenerateTextReasoning,
        GenerateTextResult, GenerateTextStartEvent, GenerateTextStep, GenerateTextStepEndEvent,
        GenerateTextStepPerformance, GenerateTextStepStartEvent, GenerateTextToolCall,
        GenerateTextToolError, GenerateTextToolExecutionEndEvent,
        GenerateTextToolExecutionStartEvent, GenerateTextToolOutputDenied, GenerateTextToolResult,
        GeneratedFile, GenericToolApprovalOptions, InvalidStreamPartError,
        InvalidToolApprovalError, InvalidToolInputError, LanguageModelCallEndEvent,
        LanguageModelCallPerformance, LanguageModelCallStartEvent, MissingToolResultsError,
        ModelInfo, NoObjectGeneratedError, NoOutputGeneratedError, NoSuchToolError,
        NormalizedToolApprovalStatus, PrepareStepResult, PruneEmptyMessages, PruneMessagesOptions,
        PruneReasoning, PruneToolCallRule, PruneToolCallRuleMode, PruneToolCalls,
        ReasoningFileOutput, ReasoningOutput, ResolveToolApprovalOptions,
        SingleToolApprovalOptions, StaticToolCall, StaticToolError, StaticToolOutputDenied,
        StaticToolResult, StopCondition, ToolApprovalConfiguration, ToolApprovalRequestOutput,
        ToolApprovalResponseOutput, ToolApprovalStatus, ToolApprovalStatusKind,
        ToolCallNotFoundForApprovalError, ToolCallRepairError, ToolCallRepairOptions,
        ToolCallRepairOriginalError, ToolExecutionEndEvent, ToolExecutionStartEvent,
        ToolInputRefinementError, TypedToolCall, TypedToolError, TypedToolOutputDenied,
        TypedToolResult, UiMessageStreamError, UnsupportedModelVersionError,
        collect_tool_approvals, experimental_filter_active_tools, filter_active_tools,
        generate_text, has_tool_call, is_loop_finished, is_step_count, is_stop_condition_met,
        normalize_tool_approval_status, prune_messages, resolve_tool_approval, step_count_is,
    };
    use crate::file_data::FileDataContent;
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFile, LanguageModelFileData, LanguageModelFinishReason,
        LanguageModelFunctionTool, LanguageModelGenerateResult, LanguageModelMessage,
        LanguageModelPrompt, LanguageModelProviderTool, LanguageModelReasoning,
        LanguageModelReasoningFile, LanguageModelReasoningPart, LanguageModelRequest,
        LanguageModelResponse, LanguageModelResponseFormat, LanguageModelSource,
        LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelSupportedUrls,
        LanguageModelSystemMessage, LanguageModelText, LanguageModelTextDelta,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolApprovalRequest,
        LanguageModelToolApprovalRequestPart, LanguageModelToolApprovalResponsePart,
        LanguageModelToolCall, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResult,
        LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUsage,
        LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::prompt::Prompt;
    use crate::provider::{
        JsonParseError, ProviderMetadata, ProviderOptions, SpecificationVersion,
    };
    use crate::provider_utils::{
        ExperimentalSandbox, SandboxCommandOptions, SandboxCommandResult, SandboxRunCommandFuture,
        Schema, Tool, ToolExecutionError, ValidationResult, dynamic_tool,
    };
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use std::task::{Context, Poll, Waker};
    use time::OffsetDateTime;

    fn no_object_response() -> LanguageModelResponse {
        let timestamp = OffsetDateTime::parse(
            "2024-01-02T03:04:05Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses");

        LanguageModelResponse::new()
            .with_id("resp_123")
            .with_timestamp(timestamp)
            .with_model_id("language-test")
            .with_header("x-request-id", "req_123")
            .with_body(json!({ "raw": true }))
    }

    fn no_object_usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(11),
                cache_read: Some(3),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(7),
                text: Some(5),
                ..OutputTokenUsage::default()
            },
            raw: Some(
                serde_json::from_value(json!({
                    "providerTokens": 18
                }))
                .expect("raw usage is an object"),
            ),
        }
    }

    fn stop_condition_step(tool_names: &[&str]) -> GenerateTextStep {
        let content = tool_names
            .iter()
            .enumerate()
            .map(|(index, tool_name)| {
                LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    format!("call-{index}"),
                    *tool_name,
                    "{}",
                ))
            })
            .collect::<Vec<_>>();
        let finish_reason = if content.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        };

        GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                content,
                LanguageModelFinishReason {
                    unified: finish_reason,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        )
    }

    #[test]
    fn stop_conditions_match_upstream_builtin_predicates() {
        let empty = stop_condition_step(&[]);
        let final_answer = stop_condition_step(&["finalAnswer"]);
        let weather = stop_condition_step(&["weather"]);

        assert!(is_step_count(2).is_met(&[empty.clone(), weather.clone()]));
        assert!(!is_step_count(2).is_met(std::slice::from_ref(&empty)));
        assert!(!is_step_count(2).is_met(&[empty.clone(), weather.clone(), final_answer.clone(),]));
        assert!(!is_loop_finished().is_met(&[]));
        assert!(!is_loop_finished().is_met(std::slice::from_ref(&empty)));

        let stop_on_final_answer = has_tool_call(["finalAnswer"]);
        assert!(stop_on_final_answer.is_met(&[empty.clone(), final_answer.clone()]));
        assert!(!stop_on_final_answer.is_met(&[final_answer, empty.clone()]));
        assert!(!stop_on_final_answer.is_met(&[]));

        assert!(has_tool_call(["search", "weather"]).is_met(std::slice::from_ref(&weather)));
        assert!(!has_tool_call(["search", "finalAnswer"]).is_met(&[empty]));
    }

    #[test]
    fn step_count_is_alias_matches_is_step_count() {
        let steps = [stop_condition_step(&[]), stop_condition_step(&["weather"])];

        assert_eq!(step_count_is(2), is_step_count(2));
        assert!(step_count_is(2).is_met(&steps));
        assert!(!step_count_is(1).is_met(&steps));
    }

    #[test]
    fn is_stop_condition_met_matches_any_condition_behavior() {
        let steps = [stop_condition_step(&[]), stop_condition_step(&["weather"])];

        assert!(is_stop_condition_met(
            &[is_loop_finished(), is_step_count(2)],
            &steps
        ));
        assert!(is_stop_condition_met(
            &[is_loop_finished(), has_tool_call(["weather"])],
            &steps
        ));
        assert!(!is_stop_condition_met(
            &[
                StopCondition::LoopFinished,
                StopCondition::HasToolCall(vec!["finalAnswer".to_string()])
            ],
            &steps
        ));
    }

    #[test]
    fn generate_text_include_serializes_optional_upstream_flags() {
        let include = GenerateTextInclude::new()
            .with_request_body(true)
            .with_request_messages(true);

        assert_eq!(
            serde_json::to_value(include).expect("include serializes"),
            json!({
                "requestBody": true,
                "requestMessages": true
            })
        );

        let include: GenerateTextInclude = serde_json::from_value(json!({
            "responseBody": true
        }))
        .expect("include deserializes");

        assert!(!include.request_body);
        assert!(!include.request_messages);
        assert!(include.response_body);
    }

    #[test]
    fn tool_input_refinement_error_retains_message() {
        let error = ToolInputRefinementError::new("refinement failed");

        assert_eq!(error.message(), "refinement failed");
        assert_eq!(error.to_string(), "refinement failed");
        assert_eq!(error.clone().into_message(), "refinement failed");
        assert_eq!(
            serde_json::to_value(error).expect("refinement error serializes"),
            json!({
                "message": "refinement failed"
            })
        );
    }

    #[test]
    fn generate_text_step_performance_serializes_as_upstream_camel_case_shape() {
        let performance = GenerateTextStepPerformance {
            effective_output_tokens_per_second: 20.0,
            output_tokens_per_second: Some(24.5),
            input_tokens_per_second: Some(40.0),
            effective_total_tokens_per_second: 60.0,
            step_time_ms: 750,
            response_time_ms: 500,
            tool_execution_ms: BTreeMap::from([
                ("call-1".to_string(), 25),
                ("call-2".to_string(), 50),
            ]),
            time_to_first_output_token_ms: Some(100),
        };

        assert_eq!(
            serde_json::to_value(&performance).expect("performance serializes"),
            json!({
                "effectiveOutputTokensPerSecond": 20.0,
                "outputTokensPerSecond": 24.5,
                "inputTokensPerSecond": 40.0,
                "effectiveTotalTokensPerSecond": 60.0,
                "stepTimeMs": 750,
                "responseTimeMs": 500,
                "toolExecutionMs": {
                    "call-1": 25,
                    "call-2": 50
                },
                "timeToFirstOutputTokenMs": 100
            })
        );

        assert_eq!(
            serde_json::from_value::<GenerateTextStepPerformance>(
                serde_json::to_value(&performance).expect("performance serializes")
            )
            .expect("performance deserializes"),
            performance
        );

        assert_eq!(
            serde_json::to_value(GenerateTextStepPerformance::default())
                .expect("default performance serializes"),
            json!({
                "effectiveOutputTokensPerSecond": 0.0,
                "effectiveTotalTokensPerSecond": 0.0,
                "stepTimeMs": 0,
                "responseTimeMs": 0
            })
        );
    }

    #[test]
    fn no_such_tool_error_matches_upstream_default_messages() {
        let missing = NoSuchToolError::new("forecast");
        assert_eq!(missing.tool_name(), "forecast");
        assert_eq!(missing.available_tools(), None);
        assert_eq!(
            missing.message(),
            "Model tried to call unavailable tool 'forecast'. No tools are available."
        );
        assert_eq!(missing.to_string(), missing.message());

        let with_tools = NoSuchToolError::with_available_tools(
            "forecast",
            ["weather".to_string(), "webSearch".to_string()],
        );
        assert_eq!(
            with_tools.available_tools(),
            Some(["weather".to_string(), "webSearch".to_string()].as_slice())
        );
        assert_eq!(
            with_tools.to_string(),
            "Model tried to call unavailable tool 'forecast'. Available tools: weather, webSearch."
        );

        let custom = NoSuchToolError::with_message(
            "forecast",
            Some(vec!["weather".to_string()]),
            "custom unavailable-tool message",
        );
        assert_eq!(custom.message(), "custom unavailable-tool message");
        assert_eq!(
            custom.into_parts(),
            (
                "forecast".to_string(),
                Some(vec!["weather".to_string()]),
                "custom unavailable-tool message".to_string()
            )
        );
    }

    #[test]
    fn invalid_tool_input_error_matches_upstream_default_message() {
        let cause = JsonParseError::new("{ bad", "expected value at line 1 column 1");
        let error = InvalidToolInputError::new("weather", "{ bad", cause);

        assert_eq!(error.tool_name(), "weather");
        assert_eq!(error.tool_input(), "{ bad");
        assert_eq!(
            error.cause_message(),
            "JSON parsing failed: Text: { bad.\nError message: expected value at line 1 column 1"
        );
        assert_eq!(
            error.message(),
            "Invalid input for tool weather: JSON parsing failed: Text: { bad.\nError message: expected value at line 1 column 1"
        );
        assert_eq!(error.to_string(), error.message());

        let custom = InvalidToolInputError::with_message(
            "weather",
            "{ bad",
            "schema mismatch",
            "custom invalid-tool-input message",
        );
        assert_eq!(
            custom.into_parts(),
            (
                "weather".to_string(),
                "{ bad".to_string(),
                "schema mismatch".to_string(),
                "custom invalid-tool-input message".to_string()
            )
        );
    }

    #[test]
    fn tool_call_repair_error_matches_upstream_default_message() {
        let original_error =
            InvalidToolInputError::new("weather", "{ bad", "expected value at line 1 column 1");
        let error = ToolCallRepairError::new(original_error.clone(), "repair model failed");

        assert_eq!(
            error.original_error(),
            &ToolCallRepairOriginalError::InvalidToolInput(original_error)
        );
        assert_eq!(error.cause_message(), "repair model failed");
        assert_eq!(
            error.message(),
            "Error repairing tool call: repair model failed"
        );
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn tool_call_repair_error_retains_original_no_such_tool_error_and_custom_message() {
        let original_error = NoSuchToolError::new("weather");
        let error = ToolCallRepairError::with_message(
            original_error.clone(),
            "repair function failed",
            "custom repair error",
        );

        assert_eq!(
            error.into_parts(),
            (
                ToolCallRepairOriginalError::NoSuchTool(original_error),
                "repair function failed".to_string(),
                "custom repair error".to_string()
            )
        );
    }

    #[test]
    fn missing_tool_results_error_matches_upstream_default_messages() {
        let single = MissingToolResultsError::new(["call-1"]);
        assert_eq!(single.tool_call_ids(), &["call-1".to_string()]);
        assert_eq!(
            single.message(),
            "Tool result is missing for tool call call-1."
        );
        assert_eq!(single.to_string(), single.message());

        let multiple = MissingToolResultsError::new(["call-1", "call-2"]);
        assert_eq!(
            multiple.tool_call_ids(),
            &["call-1".to_string(), "call-2".to_string()]
        );
        assert_eq!(
            multiple.message(),
            "Tool results are missing for tool calls call-1, call-2."
        );
        assert_eq!(
            multiple.into_parts(),
            (
                vec!["call-1".to_string(), "call-2".to_string()],
                "Tool results are missing for tool calls call-1, call-2.".to_string()
            )
        );
    }

    #[test]
    fn invalid_tool_approval_error_matches_upstream_default_message() {
        let error = InvalidToolApprovalError::new("approval-1");

        assert_eq!(error.approval_id(), "approval-1");
        assert_eq!(
            error.message(),
            "Tool approval response references unknown approvalId: \"approval-1\". No matching tool-approval-request found in message history."
        );
        assert_eq!(error.to_string(), error.message());
        assert_eq!(
            error.into_parts(),
            (
                "approval-1".to_string(),
                "Tool approval response references unknown approvalId: \"approval-1\". No matching tool-approval-request found in message history."
                    .to_string()
            )
        );
    }

    #[test]
    fn tool_call_not_found_for_approval_error_matches_upstream_default_message() {
        let error = ToolCallNotFoundForApprovalError::new("tool-call-1", "approval-1");

        assert_eq!(error.tool_call_id(), "tool-call-1");
        assert_eq!(error.approval_id(), "approval-1");
        assert_eq!(
            error.message(),
            "Tool call \"tool-call-1\" not found for approval request \"approval-1\"."
        );
        assert_eq!(error.to_string(), error.message());
        assert_eq!(
            error.into_parts(),
            (
                "tool-call-1".to_string(),
                "approval-1".to_string(),
                "Tool call \"tool-call-1\" not found for approval request \"approval-1\"."
                    .to_string()
            )
        );
    }

    #[test]
    fn tool_approval_status_accepts_upstream_string_forms_and_normalizes_them() {
        let approved = serde_json::from_value::<ToolApprovalStatus>(json!("approved"))
            .expect("approved string status deserializes");
        let denied = serde_json::from_value::<ToolApprovalStatus>(json!("denied"))
            .expect("denied string status deserializes");
        let user_approval = serde_json::from_value::<ToolApprovalStatus>(json!("user-approval"))
            .expect("user-approval string status deserializes");

        assert_eq!(
            normalize_tool_approval_status(Some(approved)),
            NormalizedToolApprovalStatus::approved()
        );
        assert_eq!(
            normalize_tool_approval_status(Some(denied)),
            NormalizedToolApprovalStatus::denied()
        );
        assert_eq!(
            normalize_tool_approval_status(Some(user_approval)),
            NormalizedToolApprovalStatus::UserApproval
        );
        assert_eq!(
            normalize_tool_approval_status(None),
            NormalizedToolApprovalStatus::NotApplicable
        );
    }

    #[test]
    fn tool_approval_status_object_forms_round_trip_upstream_json() {
        let approved = NormalizedToolApprovalStatus::approved_with_reason("owner allowed");
        let denied = NormalizedToolApprovalStatus::denied_with_reason("policy block");

        assert_eq!(approved.reason(), Some("owner allowed"));
        assert_eq!(denied.reason(), Some("policy block"));
        assert_eq!(
            serde_json::to_value(&approved).expect("approved status serializes"),
            json!({
                "type": "approved",
                "reason": "owner allowed"
            })
        );
        assert_eq!(
            serde_json::to_value(&denied).expect("denied status serializes"),
            json!({
                "type": "denied",
                "reason": "policy block"
            })
        );
        assert_eq!(
            serde_json::to_value(NormalizedToolApprovalStatus::approved())
                .expect("approved status serializes"),
            json!({
                "type": "approved"
            })
        );

        let input = serde_json::from_value::<ToolApprovalStatus>(json!({
            "type": "denied",
            "reason": "manual denial"
        }))
        .expect("object status deserializes");

        assert_eq!(
            input.normalized(),
            NormalizedToolApprovalStatus::denied_with_reason("manual denial")
        );
    }

    #[test]
    fn tool_approval_status_kind_round_trips_status_strings() {
        assert_eq!(
            serde_json::to_value(ToolApprovalStatusKind::NotApplicable)
                .expect("status kind serializes"),
            json!("not-applicable")
        );
        assert_eq!(
            serde_json::from_value::<ToolApprovalStatusKind>(json!("user-approval"))
                .expect("status kind deserializes"),
            ToolApprovalStatusKind::UserApproval
        );
    }

    fn approval_tool_call(tool_name: &str) -> GenerateTextToolCall {
        GenerateTextToolCall {
            tool_call_id: "call-1".to_string(),
            tool_name: tool_name.to_string(),
            input: json!({
                "city": "Berlin"
            }),
            title: None,
            provider_executed: None,
            dynamic: None,
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: None,
        }
    }

    fn approval_tool_schema() -> crate::json::JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    #[test]
    fn tool_approval_configuration_round_trips_static_status_map() {
        let mut configuration = ToolApprovalConfiguration::new()
            .with_tool_status("weather", ToolApprovalStatusKind::Denied)
            .with_tool_status(
                "search",
                NormalizedToolApprovalStatus::approved_with_reason("trusted source"),
            );
        assert_eq!(
            configuration.insert_tool_status("weather", ToolApprovalStatusKind::UserApproval),
            Some(ToolApprovalStatus::Kind(ToolApprovalStatusKind::Denied))
        );

        assert!(!configuration.is_empty());
        assert_eq!(
            configuration.tool_status("weather"),
            Some(&ToolApprovalStatus::Kind(
                ToolApprovalStatusKind::UserApproval
            ))
        );
        assert_eq!(configuration.tool_statuses().len(), 2);

        assert_eq!(
            serde_json::to_value(&configuration).expect("configuration serializes"),
            json!({
                "search": {
                    "type": "approved",
                    "reason": "trusted source"
                },
                "weather": "user-approval"
            })
        );

        let round_tripped: ToolApprovalConfiguration = serde_json::from_value(json!({
            "search": {
                "type": "approved",
                "reason": "trusted source"
            },
            "weather": "user-approval"
        }))
        .expect("configuration deserializes");

        assert_eq!(round_tripped, configuration);
    }

    #[test]
    fn resolve_tool_approval_prefers_user_configured_statuses() {
        let tool_call = approval_tool_call("weather");
        let tools = vec![Tool::new("weather", approval_tool_schema()).with_needs_approval(true)];
        let configuration = ToolApprovalConfiguration::new().with_tool_status(
            "weather",
            NormalizedToolApprovalStatus::denied_with_reason("policy block"),
        );

        let status = poll_ready(resolve_tool_approval(
            ResolveToolApprovalOptions::new(&tool_call)
                .with_tools(&tools)
                .with_tool_approval(&configuration),
        ));

        assert_eq!(
            status,
            NormalizedToolApprovalStatus::denied_with_reason("policy block")
        );
    }

    #[test]
    fn resolve_tool_approval_maps_tool_defined_boolean_approval() {
        let tool_call = approval_tool_call("weather");
        let approval_required =
            vec![Tool::new("weather", approval_tool_schema()).with_needs_approval(true)];
        let approval_not_required =
            vec![Tool::new("weather", approval_tool_schema()).with_needs_approval(false)];

        assert_eq!(
            poll_ready(resolve_tool_approval(
                ResolveToolApprovalOptions::new(&tool_call).with_tools(&approval_required)
            )),
            NormalizedToolApprovalStatus::UserApproval
        );
        assert_eq!(
            poll_ready(resolve_tool_approval(
                ResolveToolApprovalOptions::new(&tool_call).with_tools(&approval_not_required)
            )),
            NormalizedToolApprovalStatus::NotApplicable
        );
    }

    #[test]
    fn resolve_tool_approval_uses_tool_defined_callback_with_context() {
        let tool_call = approval_tool_call("weather");
        let prompt = vec![user_message("Weather?")];
        let tools_context =
            JsonObject::from_iter([("weather".to_string(), json!({ "risk": "high" }))]);
        let seen = Arc::new(Mutex::new(
            None::<(JsonValue, String, LanguageModelPrompt, JsonValue)>,
        ));
        let seen_for_callback = Arc::clone(&seen);
        let tools = vec![
            Tool::new("weather", approval_tool_schema()).with_needs_approval_function(
                move |input, options| {
                    let seen = Arc::clone(&seen_for_callback);
                    async move {
                        seen.lock().expect("seen lock").replace((
                            input,
                            options.tool_call_id,
                            options.messages,
                            options.context.expect("context passed"),
                        ));
                        true
                    }
                },
            ),
        ];

        let status = poll_ready(resolve_tool_approval(
            ResolveToolApprovalOptions::new(&tool_call)
                .with_tools(&tools)
                .with_messages(&prompt)
                .with_tools_context(&tools_context),
        ));

        assert_eq!(status, NormalizedToolApprovalStatus::UserApproval);
        let seen = seen.lock().expect("seen lock");
        let (input, tool_call_id, messages, context) =
            seen.as_ref().expect("callback captured options");
        assert_eq!(input["city"], json!("Berlin"));
        assert_eq!(tool_call_id, "call-1");
        assert_eq!(messages, &prompt);
        assert_eq!(context, &json!({ "risk": "high" }));
    }

    #[test]
    fn resolve_tool_approval_defaults_to_not_applicable() {
        let tool_call = approval_tool_call("weather");

        assert_eq!(
            poll_ready(resolve_tool_approval(ResolveToolApprovalOptions::new(
                &tool_call
            ))),
            NormalizedToolApprovalStatus::NotApplicable
        );
        assert_eq!(
            poll_ready(resolve_tool_approval(
                ResolveToolApprovalOptions::new(&tool_call)
                    .with_tools(&[Tool::new("search", approval_tool_schema())])
            )),
            NormalizedToolApprovalStatus::NotApplicable
        );
    }

    #[test]
    fn resolve_tool_approval_uses_generic_callback_before_static_tool_statuses() {
        let tool_call = approval_tool_call("weather");
        let tools = vec![Tool::new("weather", approval_tool_schema())];
        let prompt = vec![user_message("Weather?")];
        let runtime_context = JsonObject::from_iter([("userId".to_string(), json!("user-1"))]);
        let tools_context = JsonObject::from_iter([(
            "weather".to_string(),
            json!({ "risk": "low", "source": "test" }),
        )]);
        let seen = Arc::new(Mutex::new(None::<GenericToolApprovalOptions>));
        let seen_for_callback = Arc::clone(&seen);
        let configuration = ToolApprovalConfiguration::new()
            .with_tool_status("weather", ToolApprovalStatusKind::Denied)
            .with_generic_tool_approval(move |options| {
                let seen = Arc::clone(&seen_for_callback);
                async move {
                    seen.lock().expect("seen lock").replace(options);
                    Some(NormalizedToolApprovalStatus::approved_with_reason("generic").into())
                }
            });

        assert!(configuration.has_generic_tool_approval());
        assert!(serde_json::to_value(&configuration).is_err());

        let status = poll_ready(resolve_tool_approval(
            ResolveToolApprovalOptions::new(&tool_call)
                .with_tools(&tools)
                .with_tool_approval(&configuration)
                .with_messages(&prompt)
                .with_tools_context(&tools_context)
                .with_runtime_context(&runtime_context),
        ));

        assert_eq!(
            status,
            NormalizedToolApprovalStatus::approved_with_reason("generic")
        );

        let seen = seen.lock().expect("seen lock");
        let seen_options = seen.as_ref().expect("callback captured options");
        assert_eq!(seen_options.tool_call.tool_call_id, "call-1");
        assert_eq!(seen_options.tools.as_ref().expect("tools passed").len(), 1);
        assert_eq!(seen_options.messages, prompt);
        assert_eq!(seen_options.tools_context, tools_context);
        assert_eq!(seen_options.runtime_context, runtime_context);
    }

    #[test]
    fn resolve_tool_approval_uses_per_tool_callback_with_context() {
        let tool_call = approval_tool_call("weather");
        let tools = vec![Tool::new("weather", approval_tool_schema())];
        let prompt = vec![user_message("Weather?")];
        let runtime_context = JsonObject::from_iter([("tenant".to_string(), json!("acme"))]);
        let tools_context =
            JsonObject::from_iter([("weather".to_string(), json!({ "allow": false }))]);
        let seen = Arc::new(Mutex::new(None::<(JsonValue, SingleToolApprovalOptions)>));
        let seen_for_callback = Arc::clone(&seen);
        let configuration = ToolApprovalConfiguration::new().with_tool_approval_function(
            "weather",
            move |input, options| {
                let seen = Arc::clone(&seen_for_callback);
                async move {
                    seen.lock().expect("seen lock").replace((input, options));
                    Some(NormalizedToolApprovalStatus::denied_with_reason("context policy").into())
                }
            },
        );

        assert!(configuration.has_tool_approval_function("weather"));
        assert!(serde_json::to_value(&configuration).is_err());

        let status = poll_ready(resolve_tool_approval(
            ResolveToolApprovalOptions::new(&tool_call)
                .with_tools(&tools)
                .with_tool_approval(&configuration)
                .with_messages(&prompt)
                .with_tools_context(&tools_context)
                .with_runtime_context(&runtime_context),
        ));

        assert_eq!(
            status,
            NormalizedToolApprovalStatus::denied_with_reason("context policy")
        );

        let seen = seen.lock().expect("seen lock");
        let (input, options) = seen.as_ref().expect("callback captured options");
        assert_eq!(input["city"], json!("Berlin"));
        assert_eq!(options.tool_call_id, "call-1");
        assert_eq!(options.messages, prompt);
        assert_eq!(
            options.tool_context.as_ref().expect("tool context"),
            &json!({ "allow": false })
        );
        assert_eq!(options.runtime_context, runtime_context);
    }

    #[test]
    fn collect_tool_approvals_returns_empty_when_latest_message_is_not_tool() {
        let messages = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Hello"),
            )],
        ))];

        let approvals = collect_tool_approvals(&messages).expect("approvals collect");

        assert!(approvals.approved_tool_approvals.is_empty());
        assert!(approvals.denied_tool_approvals.is_empty());
        assert_eq!(
            serde_json::to_value(approvals).expect("approvals serialize"),
            json!({
                "approvedToolApprovals": [],
                "deniedToolApprovals": []
            })
        );
    }

    #[test]
    fn collect_tool_approvals_splits_approved_and_denied_responses() {
        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-approved",
                    "weather",
                    json!({ "city": "Brisbane" }),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-approved", "call-approved")
                        .with_automatic(true),
                ),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-denied",
                    "search",
                    json!({ "query": "forecast" }),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-denied", "call-denied"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval-approved", true),
                ),
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval-denied", false)
                        .with_reason("manual denial"),
                ),
            ])),
        ];

        let approvals = collect_tool_approvals(&messages).expect("approvals collect");

        assert_eq!(approvals.approved_tool_approvals.len(), 1);
        assert_eq!(approvals.denied_tool_approvals.len(), 1);
        assert_eq!(
            serde_json::to_value(&approvals).expect("approvals serialize"),
            json!({
                "approvedToolApprovals": [
                    {
                        "approvalRequest": {
                            "type": "tool-approval-request",
                            "approvalId": "approval-approved",
                            "toolCallId": "call-approved",
                            "isAutomatic": true
                        },
                        "approvalResponse": {
                            "type": "tool-approval-response",
                            "approvalId": "approval-approved",
                            "approved": true
                        },
                        "toolCall": {
                            "type": "tool-call",
                            "toolCallId": "call-approved",
                            "toolName": "weather",
                            "input": { "city": "Brisbane" }
                        }
                    }
                ],
                "deniedToolApprovals": [
                    {
                        "approvalRequest": {
                            "type": "tool-approval-request",
                            "approvalId": "approval-denied",
                            "toolCallId": "call-denied"
                        },
                        "approvalResponse": {
                            "type": "tool-approval-response",
                            "approvalId": "approval-denied",
                            "approved": false,
                            "reason": "manual denial"
                        },
                        "toolCall": {
                            "type": "tool-call",
                            "toolCallId": "call-denied",
                            "toolName": "search",
                            "input": { "query": "forecast" }
                        }
                    }
                ]
            })
        );

        let round_tripped =
            serde_json::from_value(serde_json::to_value(&approvals).expect("approvals serialize"))
                .expect("approvals deserialize");
        assert_eq!(approvals, round_tripped);
    }

    #[test]
    fn collect_tool_approvals_ignores_processed_approval_responses() {
        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "weather",
                    json!({ "city": "Brisbane" }),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-1", "call-1"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval-1", true),
                ),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::text("sunny"),
                )),
            ])),
        ];

        let approvals = collect_tool_approvals(&messages).expect("approvals collect");

        assert!(approvals.approved_tool_approvals.is_empty());
        assert!(approvals.denied_tool_approvals.is_empty());
    }

    #[test]
    fn collect_tool_approvals_reports_invalid_approval_references() {
        let messages = vec![LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            vec![LanguageModelToolContentPart::ToolApprovalResponse(
                LanguageModelToolApprovalResponsePart::new("missing-approval", true),
            )],
        ))];

        let error = collect_tool_approvals(&messages).expect_err("approval id is missing");
        assert_eq!(
            error.to_string(),
            "Tool approval response references unknown approvalId: \"missing-approval\". No matching tool-approval-request found in message history."
        );

        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-1", "missing-call"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval-1", true),
                ),
            ])),
        ];

        let error = collect_tool_approvals(&messages).expect_err("tool call is missing");
        assert_eq!(
            error.to_string(),
            "Tool call \"missing-call\" not found for approval request \"approval-1\"."
        );
    }

    #[test]
    fn no_output_generated_error_matches_upstream_default_message() {
        let error = NoOutputGeneratedError::new();

        assert_eq!(error.message(), "No output generated.");
        assert_eq!(error.to_string(), "No output generated.");
        assert_eq!(error.into_message(), "No output generated.");
        assert_eq!(
            NoOutputGeneratedError::default().to_string(),
            "No output generated."
        );
    }

    #[test]
    fn no_output_generated_error_accepts_custom_message() {
        let error = NoOutputGeneratedError::with_message(
            "No output generated. Check the stream for errors.",
        );

        assert_eq!(
            error.message(),
            "No output generated. Check the stream for errors."
        );
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn no_object_generated_error_matches_upstream_default_context() {
        let response = no_object_response();
        let usage = no_object_usage();
        let error =
            NoObjectGeneratedError::new(response.clone(), usage.clone(), FinishReason::Stop);

        assert_eq!(error.message(), "No object generated.");
        assert_eq!(error.to_string(), "No object generated.");
        assert_eq!(error.cause_message(), None);
        assert_eq!(error.text(), None);
        assert_eq!(error.response(), &response);
        assert_eq!(error.usage(), &usage);
        assert_eq!(error.finish_reason(), &FinishReason::Stop);
    }

    #[test]
    fn no_object_generated_error_retains_text_cause_and_custom_message() {
        let response = no_object_response();
        let usage = no_object_usage();
        let cause = JsonParseError::new("{ bad", "expected value at line 1 column 1");
        let error = NoObjectGeneratedError::with_message(
            "No object generated: could not parse the response.",
            response.clone(),
            usage.clone(),
            FinishReason::Other,
        )
        .with_text("{ bad")
        .with_cause(&cause);

        assert_eq!(
            error.message(),
            "No object generated: could not parse the response."
        );
        assert_eq!(error.text(), Some("{ bad"));
        assert_eq!(
            error.cause_message(),
            Some(
                "JSON parsing failed: Text: { bad.\nError message: expected value at line 1 column 1"
            )
        );
        assert_eq!(
            error.into_parts(),
            (
                "No object generated: could not parse the response.".to_string(),
                Some(
                    "JSON parsing failed: Text: { bad.\nError message: expected value at line 1 column 1"
                        .to_string()
                ),
                Some("{ bad".to_string()),
                response,
                usage,
                FinishReason::Other
            )
        );
    }

    #[test]
    fn no_object_generated_error_context_uses_existing_json_boundaries() {
        let response = no_object_response();
        let usage = no_object_usage();

        assert_eq!(
            serde_json::to_value(&response).expect("response serializes"),
            json!({
                "id": "resp_123",
                "timestamp": "2024-01-02T03:04:05Z",
                "modelId": "language-test",
                "headers": {
                    "x-request-id": "req_123"
                },
                "body": {
                    "raw": true
                }
            })
        );

        assert_eq!(
            serde_json::to_value(&usage).expect("usage serializes"),
            json!({
                "inputTokens": {
                    "total": 11,
                    "cacheRead": 3
                },
                "outputTokens": {
                    "total": 7,
                    "text": 5
                },
                "raw": {
                    "providerTokens": 18
                }
            })
        );

        let error =
            NoObjectGeneratedError::new(response.clone(), usage.clone(), FinishReason::Length);
        assert_eq!(error.response(), &response);
        assert_eq!(error.usage(), &usage);
    }

    #[test]
    fn invalid_stream_part_error_retains_chunk_and_message() {
        let chunk =
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello"));
        let error = InvalidStreamPartError::new(
            chunk.clone(),
            "text-delta chunk arrived without a matching text-start",
        );

        assert_eq!(error.chunk(), &chunk);
        assert_eq!(
            error.message(),
            "text-delta chunk arrived without a matching text-start"
        );
        assert_eq!(error.to_string(), error.message());
        assert_eq!(
            error.into_parts(),
            (
                chunk,
                "text-delta chunk arrived without a matching text-start".to_string()
            )
        );
    }

    #[test]
    fn ui_message_stream_error_retains_chunk_context_and_message() {
        let error = UiMessageStreamError::new(
            "text-delta",
            "text-1",
            "text-delta chunk arrived without a matching text-start",
        );

        assert_eq!(error.chunk_type(), "text-delta");
        assert_eq!(error.chunk_id(), "text-1");
        assert_eq!(
            error.message(),
            "text-delta chunk arrived without a matching text-start"
        );
        assert_eq!(error.to_string(), error.message());
        assert_eq!(
            error.into_parts(),
            (
                "text-delta".to_string(),
                "text-1".to_string(),
                "text-delta chunk arrived without a matching text-start".to_string()
            )
        );
    }

    #[test]
    fn unsupported_model_version_error_matches_upstream_message_and_context() {
        let error = UnsupportedModelVersionError::new("v1", "test-provider", "test-model-id");

        assert_eq!(error.version(), "v1");
        assert_eq!(error.provider(), "test-provider");
        assert_eq!(error.model_id(), "test-model-id");
        assert_eq!(
            error.message(),
            "Unsupported model version v1 for provider \"test-provider\" and model \"test-model-id\". AI SDK 5 only supports models that implement specification version \"v2\"."
        );
        assert_eq!(error.to_string(), error.message());
        assert_eq!(
            error.into_parts(),
            (
                "v1".to_string(),
                "test-provider".to_string(),
                "test-model-id".to_string(),
                "Unsupported model version v1 for provider \"test-provider\" and model \"test-model-id\". AI SDK 5 only supports models that implement specification version \"v2\"."
                    .to_string()
            )
        );
    }

    struct FakeLanguageModel {
        calls: RefCell<Vec<LanguageModelCallOptions>>,
        include_body_metadata: bool,
        content: Vec<LanguageModelContent>,
    }

    impl FakeLanguageModel {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                include_body_metadata: false,
                content: vec![
                    LanguageModelContent::Text(LanguageModelText::new("Hello ")),
                    LanguageModelContent::Text(LanguageModelText::new("world")),
                ],
            }
        }

        fn with_body_metadata(mut self) -> Self {
            self.include_body_metadata = true;
            self
        }

        fn with_content(mut self, content: Vec<LanguageModelContent>) -> Self {
            self.content = content;
            self
        }
    }

    impl LanguageModel for FakeLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
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
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls.borrow_mut().push(options);

            let mut result = LanguageModelGenerateResult::new(
                self.content.clone(),
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
                LanguageModelUsage {
                    input_tokens: InputTokenUsage {
                        total: Some(5),
                        ..InputTokenUsage::default()
                    },
                    output_tokens: OutputTokenUsage {
                        total: Some(2),
                        text: Some(2),
                        ..OutputTokenUsage::default()
                    },
                    raw: None,
                },
            );

            if self.include_body_metadata {
                result = result
                    .with_request(LanguageModelRequest::new().with_body(json!({
                        "messages": [
                            {
                                "role": "user",
                                "content": "Say hello"
                            }
                        ]
                    })))
                    .with_response(LanguageModelResponse::new().with_id("resp_body").with_body(
                        json!({
                            "id": "resp_body"
                        }),
                    ));
            }

            ready(result)
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

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

    fn user_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    #[test]
    #[allow(deprecated)]
    fn generate_text_event_aliases_share_upstream_json_shapes() {
        let prompt = vec![user_message("Use aliases")];
        let model_info: ModelInfo = GenerateTextModelInfo::new("test-provider", "test-model");

        assert_eq!(
            serde_json::to_value(&model_info).expect("model info serializes"),
            json!({
                "provider": "test-provider",
                "modelId": "test-model"
            })
        );

        let start_event: super::OnStartEvent = GenerateTextStartEvent {
            call_id: "call-alias".to_string(),
            operation_id: "ai.generateText".to_string(),
            provider: model_info.provider.clone(),
            model_id: model_info.model_id.clone(),
            messages: prompt.clone(),
            tools: Vec::new(),
            tool_choice: None,
            active_tools: None,
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            stop_sequences: None,
            seed: None,
            reasoning: None,
            headers: None,
            provider_options: None,
            runtime_context: JsonObject::new(),
            tools_context: JsonObject::new(),
        };

        assert_eq!(
            serde_json::to_value(&start_event).expect("start alias serializes")["operationId"],
            "ai.generateText"
        );

        let step_start_event: super::OnStepStartEvent = GenerateTextStepStartEvent {
            call_id: "call-alias".to_string(),
            provider: model_info.provider.clone(),
            model_id: model_info.model_id.clone(),
            step_number: 0,
            messages: prompt.clone(),
            tools: Vec::new(),
            tool_choice: None,
            active_tools: None,
            steps: Vec::new(),
            provider_options: None,
            runtime_context: JsonObject::new(),
            tools_context: JsonObject::new(),
        };

        assert_eq!(
            serde_json::to_value(&step_start_event).expect("step-start alias serializes")["stepNumber"],
            0
        );

        let step_event: GenerateTextStepEndEvent = stop_condition_step(&[]);
        let deprecated_step_event: super::OnStepFinishEvent = step_event.clone();

        assert_eq!(
            serde_json::to_value(&deprecated_step_event)
                .expect("deprecated step-finish alias serializes"),
            serde_json::to_value(&step_event).expect("step-end alias serializes")
        );

        let finish_event: GenerateTextEndEvent =
            GenerateTextFinishEvent::from_steps(&[], std::slice::from_ref(&step_event));
        let deprecated_finish_event: super::OnFinishEvent = finish_event.clone();

        assert_eq!(
            serde_json::to_value(&deprecated_finish_event)
                .expect("deprecated finish alias serializes"),
            serde_json::to_value(&finish_event).expect("finish alias serializes")
        );

        let tool_call = GenerateTextToolCall {
            tool_call_id: "call-tool".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            title: None,
            provider_executed: None,
            dynamic: None,
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        let tool_result = GenerateTextToolResult {
            tool_call_id: tool_call.tool_call_id.clone(),
            tool_name: tool_call.tool_name.clone(),
            input: tool_call.input.clone(),
            output: json!({ "weather": "sunny" }),
            title: tool_call.title.clone(),
            is_error: None,
            provider_executed: None,
            dynamic: None,
            preliminary: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        let tool_start_event: ToolExecutionStartEvent = GenerateTextToolExecutionStartEvent {
            call_id: "call-alias".to_string(),
            messages: prompt.clone(),
            tool_call: tool_call.clone(),
            tool_context: Some(json!({ "unit": "celsius" })),
        };
        let deprecated_tool_start_event: super::OnToolCallStartEvent = tool_start_event.clone();
        let tool_end_event: ToolExecutionEndEvent = GenerateTextToolExecutionEndEvent {
            call_id: "call-alias".to_string(),
            messages: prompt,
            tool_call,
            tool_context: Some(json!({ "unit": "celsius" })),
            tool_execution_ms: 12,
            tool_output: tool_result,
        };
        let deprecated_tool_end_event: super::OnToolCallFinishEvent = tool_end_event.clone();

        assert_eq!(
            serde_json::to_value(&deprecated_tool_start_event)
                .expect("deprecated tool-start alias serializes"),
            serde_json::to_value(&tool_start_event).expect("tool-start alias serializes")
        );
        assert_eq!(
            serde_json::to_value(&deprecated_tool_end_event)
                .expect("deprecated tool-end alias serializes"),
            serde_json::to_value(&tool_end_event).expect("tool-end alias serializes")
        );
    }

    fn approval_response_prompt(
        response: LanguageModelToolApprovalResponsePart,
        provider_executed: bool,
    ) -> Vec<LanguageModelMessage> {
        let mut tool_call =
            LanguageModelToolCallPart::new("call-1", "weather", json!({ "city": "Brisbane" }));

        if provider_executed {
            tool_call = tool_call.with_provider_executed(true);
        }

        vec![
            user_message("Weather?"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(tool_call),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-1", "call-1"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(response),
            ])),
        ]
    }

    #[test]
    fn language_model_call_events_round_trip_upstream_shape() {
        let prompt = vec![user_message("Say hello")];
        let response_format = LanguageModelResponseFormat::json().with_name("structured-answer");
        let start_event = LanguageModelCallStartEvent {
            call_id: "call-123".to_string(),
            provider: "test-provider".to_string(),
            model_id: "test-model".to_string(),
            messages: prompt.clone(),
            tools: Vec::new(),
            max_output_tokens: Some(100),
            temperature: Some(0.2),
            stop_sequences: Some(vec!["END".to_string()]),
            top_p: Some(0.9),
            top_k: Some(40),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(0.2),
            response_format: Some(response_format.clone()),
            seed: Some(42),
            tool_choice: Some(LanguageModelToolChoice::Auto),
            include_raw_chunks: Some(true),
            headers: Some(Headers::from_iter([(
                "x-test".to_string(),
                "true".to_string(),
            )])),
            reasoning: None,
            provider_options: Some(ProviderOptions::from_iter([(
                "test".to_string(),
                serde_json::from_value(json!({ "mode": "strict" }))
                    .expect("provider options object"),
            )])),
        };

        let start_value = serde_json::to_value(&start_event).expect("start event serializes");
        assert_eq!(start_value["callId"], json!("call-123"));
        assert_eq!(start_value["modelId"], json!("test-model"));
        assert_eq!(
            start_value["responseFormat"],
            json!({
                "type": "json",
                "name": "structured-answer"
            })
        );
        assert_eq!(
            serde_json::from_value::<LanguageModelCallStartEvent>(start_value)
                .expect("start event deserializes"),
            start_event
        );

        let end_event = LanguageModelCallEndEvent {
            call_id: "call-123".to_string(),
            provider: "test-provider".to_string(),
            model_id: "test-model".to_string(),
            finish_reason: FinishReason::Stop,
            usage: no_object_usage(),
            content: vec![GenerateTextContentPart::Text(LanguageModelText::new(
                "Hello",
            ))],
            response_id: "resp-123".to_string(),
            performance: LanguageModelCallPerformance {
                response_time_ms: 25,
                effective_output_tokens_per_second: 280.0,
                output_tokens_per_second: None,
                input_tokens_per_second: None,
                effective_total_tokens_per_second: 720.0,
                time_to_first_output_token_ms: None,
            },
        };

        let end_value = serde_json::to_value(&end_event).expect("end event serializes");
        assert_eq!(end_value["responseId"], json!("resp-123"));
        assert_eq!(end_value["performance"]["responseTimeMs"], json!(25));
        assert_eq!(
            serde_json::from_value::<LanguageModelCallEndEvent>(end_value)
                .expect("end event deserializes"),
            end_event
        );
    }

    #[test]
    fn generate_text_notifies_language_model_call_start_and_end() {
        let model = FakeLanguageModel::new().with_body_metadata();
        let prompt = vec![user_message("Say hello")];
        let response_format = LanguageModelResponseFormat::json().with_name("structured-answer");
        let start_events = Arc::new(Mutex::new(Vec::<LanguageModelCallStartEvent>::new()));
        let end_events = Arc::new(Mutex::new(Vec::<LanguageModelCallEndEvent>::new()));
        let event_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

        let start_events_for_callback = Arc::clone(&start_events);
        let start_order = Arc::clone(&event_order);
        let end_events_for_callback = Arc::clone(&end_events);
        let end_order = Arc::clone(&event_order);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone())
                .with_max_output_tokens(100)
                .with_response_format(response_format.clone())
                .with_experimental_on_language_model_call_start(move |event| {
                    let start_events = Arc::clone(&start_events_for_callback);
                    let event_order = Arc::clone(&start_order);
                    async move {
                        event_order.lock().expect("event order lock").push("start");
                        start_events.lock().expect("start event lock").push(event);
                    }
                })
                .with_experimental_on_language_model_call_end(move |event| {
                    let end_events = Arc::clone(&end_events_for_callback);
                    let event_order = Arc::clone(&end_order);
                    async move {
                        event_order.lock().expect("event order lock").push("end");
                        end_events.lock().expect("end event lock").push(event);
                    }
                }),
        ));

        assert_eq!(
            event_order.lock().expect("event order lock").as_slice(),
            ["start", "end"]
        );
        assert_eq!(
            model.calls.borrow()[0].response_format,
            Some(response_format.clone())
        );

        let start_events = start_events.lock().expect("start event lock");
        assert_eq!(start_events.len(), 1);
        let start = &start_events[0];
        assert_eq!(start.provider, "test-provider");
        assert_eq!(start.model_id, "test-model");
        assert_eq!(start.messages, prompt);
        assert_eq!(start.max_output_tokens, Some(100));
        assert_eq!(start.response_format, Some(response_format));
        drop(start_events);

        let end_events = end_events.lock().expect("end event lock");
        assert_eq!(end_events.len(), 1);
        let end = &end_events[0];
        assert_eq!(end.provider, "test-provider");
        assert_eq!(end.model_id, "test-model");
        assert_eq!(end.finish_reason, FinishReason::Stop);
        assert_eq!(end.response_id, "resp_body");
        assert_eq!(end.usage, result.steps[0].usage);
        assert_eq!(end.content, result.steps[0].content);
        assert!(
            end.performance
                .effective_output_tokens_per_second
                .is_finite()
        );
    }

    #[test]
    fn generate_text_calls_language_model_and_returns_plain_text_result() {
        let model = FakeLanguageModel::new();
        let prompt = vec![user_message("Say hello")];

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone())
                .with_max_output_tokens(20)
                .with_temperature(0.2),
        ));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.calls.borrow().len(), 1);
        assert_eq!(model.calls.borrow()[0].prompt, prompt);
        assert_eq!(model.calls.borrow()[0].max_output_tokens, Some(20));
        assert_eq!(model.calls.borrow()[0].temperature, Some(0.2));

        assert_eq!(result.text, "Hello world");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.raw_finish_reason.as_deref(), Some("stop"));
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.text, Some(2));
        assert_eq!(result.total_usage, result.usage);
        assert_eq!(result.warnings, Vec::new());
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.final_step, result.steps[0]);
        assert_eq!(result.output, Some(json!("Hello world")));
        assert_eq!(
            result.output().expect("default text output exists"),
            &json!("Hello world")
        );
        assert_eq!(
            result
                .clone()
                .into_output()
                .expect("default text output exists"),
            json!("Hello world")
        );
        assert_eq!(
            result.response_messages,
            vec![LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Hello ")),
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("world")),
                ])
            )]
        );
        let response = result.response.as_ref().expect("response metadata exists");
        assert_eq!(response.messages.as_ref(), Some(&result.response_messages));
        assert_eq!(response.model_id.as_deref(), Some("test-model"));
        assert!(response.id.as_ref().is_some_and(|id| id.len() == 16));
        assert!(response.timestamp.is_some());
        assert_eq!(result.final_step().expect("step exists").step_number, 0);
        assert_eq!(
            result.final_step().expect("step exists").response_messages,
            result.response_messages
        );
        assert_eq!(
            result
                .final_step()
                .expect("step exists")
                .response
                .as_ref()
                .and_then(|response| response.messages.as_ref()),
            Some(&result.response_messages)
        );
        assert_eq!(
            result.final_step().expect("step exists").model,
            GenerateTextModelInfo::new("test-provider", "test-model")
        );
        assert!(
            result
                .final_step()
                .expect("step exists")
                .runtime_context
                .is_empty()
        );
        assert!(
            result
                .final_step()
                .expect("step exists")
                .tools_context
                .is_empty()
        );
        let call_id = &result.final_step().expect("step exists").call_id;
        assert!(call_id.starts_with("call-"));
        assert_eq!(call_id.len(), "call-".len() + 24);
        let performance = &result.final_step().expect("step exists").performance;
        assert!(performance.response_time_ms <= performance.step_time_ms);
        assert!(performance.effective_output_tokens_per_second.is_finite());
        assert!(performance.effective_total_tokens_per_second.is_finite());
        assert_eq!(performance.output_tokens_per_second, None);
        assert_eq!(performance.input_tokens_per_second, None);
        assert_eq!(performance.tool_execution_ms, BTreeMap::new());
        assert_eq!(performance.time_to_first_output_token_ms, None);
    }

    #[test]
    fn generate_text_from_prompt_standardizes_text_and_instructions() {
        let model = FakeLanguageModel::new();
        let options = GenerateTextOptions::from_prompt(
            &model,
            Prompt::from_prompt("Say hello").with_instructions("Use concise JSON-free text."),
        )
        .expect("prompt standardizes");

        let result = poll_ready(generate_text(options));

        let expected_prompt = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new(
                "Use concise JSON-free text.",
            )),
            user_message("Say hello"),
        ];

        assert_eq!(model.calls.borrow()[0].prompt, expected_prompt);
        assert_eq!(result.text, "Hello world");
    }

    #[test]
    fn generate_text_from_prompt_rejects_invalid_high_level_prompt() {
        let model = FakeLanguageModel::new();
        let error =
            match GenerateTextOptions::from_prompt(&model, Prompt::from_messages(Vec::new())) {
                Ok(_) => panic!("empty messages are invalid"),
                Err(error) => error,
            };

        assert_eq!(
            error.message(),
            "Invalid prompt: messages must not be empty"
        );
        assert!(model.calls.borrow().is_empty());
    }

    #[test]
    fn generate_text_invokes_start_callbacks_with_configuration() {
        let model = FakeLanguageModel::new();
        let prompt = vec![user_message("Say hello")];
        let runtime_context = json!({
            "traceId": "trace_123"
        })
        .as_object()
        .expect("runtime context is object")
        .clone();
        let tools_context = json!({
            "weather": {
                "unit": "celsius"
            }
        })
        .as_object()
        .expect("tools context is object")
        .clone();
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "test".to_string(),
            json!({
                "mode": "fast"
            })
            .as_object()
            .expect("provider options are object")
            .clone(),
        );

        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let start_events = Arc::new(Mutex::new(Vec::<GenerateTextStartEvent>::new()));
        let step_start_events = Arc::new(Mutex::new(Vec::<GenerateTextStepStartEvent>::new()));
        let order_for_start = Arc::clone(&order);
        let order_for_step = Arc::clone(&order);
        let start_events_for_callback = Arc::clone(&start_events);
        let step_start_events_for_callback = Arc::clone(&step_start_events);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone())
                .with_tool(
                    Tool::new("weather", approval_tool_schema())
                        .with_description("Look up weather"),
                )
                .with_active_tools(["weather"])
                .with_tool_choice(LanguageModelToolChoice::Auto)
                .with_max_output_tokens(42)
                .with_temperature(0.2)
                .with_top_p(0.8)
                .with_stop_sequence("DONE")
                .with_seed(7)
                .with_header("x-trace", "trace_123")
                .with_provider_options(provider_options.clone())
                .with_runtime_context(runtime_context.clone())
                .with_tools_context(tools_context.clone())
                .with_on_start(move |event| {
                    let order = Arc::clone(&order_for_start);
                    let start_events = Arc::clone(&start_events_for_callback);
                    async move {
                        order.lock().expect("order lock").push("start");
                        start_events.lock().expect("start events lock").push(event);
                    }
                })
                .with_on_step_start(move |event| {
                    let order = Arc::clone(&order_for_step);
                    let step_start_events = Arc::clone(&step_start_events_for_callback);
                    async move {
                        order.lock().expect("order lock").push("step-start");
                        step_start_events
                            .lock()
                            .expect("step-start events lock")
                            .push(event);
                    }
                }),
        ));

        assert_eq!(
            order.lock().expect("order lock").as_slice(),
            ["start", "step-start"]
        );

        let start_events = start_events.lock().expect("start events lock");
        assert_eq!(start_events.len(), 1);
        let start = &start_events[0];
        assert_eq!(start.call_id, result.final_step.call_id);
        assert_eq!(start.operation_id, "ai.generateText");
        assert_eq!(start.provider, "test-provider");
        assert_eq!(start.model_id, "test-model");
        assert_eq!(start.messages, prompt);
        assert_eq!(start.active_tools, Some(vec!["weather".to_string()]));
        assert_eq!(start.tool_choice, Some(LanguageModelToolChoice::Auto));
        assert_eq!(start.max_output_tokens, Some(42));
        assert_eq!(start.temperature, Some(0.2));
        assert_eq!(start.top_p, Some(0.8));
        assert_eq!(start.stop_sequences, Some(vec!["DONE".to_string()]));
        assert_eq!(start.seed, Some(7));
        assert_eq!(
            start
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-trace")),
            Some(&"trace_123".to_string())
        );
        assert_eq!(start.provider_options.as_ref(), Some(&provider_options));
        assert_eq!(start.runtime_context, runtime_context);
        assert_eq!(start.tools_context, tools_context);
        assert_eq!(start.tools.len(), 1);
        match &start.tools[0] {
            LanguageModelTool::Function(tool) => {
                assert_eq!(tool.name, "weather");
                assert_eq!(tool.description.as_deref(), Some("Look up weather"));
            }
            LanguageModelTool::Provider(_) => panic!("expected prepared function tool"),
        }
        let start_value = serde_json::to_value(start).expect("start event serializes");
        assert_eq!(start_value["operationId"], json!("ai.generateText"));
        assert_eq!(
            serde_json::from_value::<GenerateTextStartEvent>(start_value)
                .expect("start event deserializes"),
            start.clone()
        );
        drop(start_events);

        let step_start_events = step_start_events.lock().expect("step-start events lock");
        assert_eq!(step_start_events.len(), 1);
        let step_start = &step_start_events[0];
        assert_eq!(step_start.call_id, result.final_step.call_id);
        assert_eq!(step_start.provider, "test-provider");
        assert_eq!(step_start.model_id, "test-model");
        assert_eq!(step_start.step_number, 0);
        assert!(step_start.steps.is_empty());
        assert_eq!(step_start.active_tools, Some(vec!["weather".to_string()]));
        assert_eq!(step_start.tool_choice, Some(LanguageModelToolChoice::Auto));
        assert_eq!(
            step_start.provider_options.as_ref(),
            Some(&provider_options)
        );
        assert_eq!(step_start.tools.len(), 1);
        assert_eq!(step_start.messages, model.calls.borrow()[0].prompt);
        assert_eq!(
            step_start.runtime_context,
            result.final_step.runtime_context
        );
        assert_eq!(step_start.tools_context, result.final_step.tools_context);
        let step_start_value =
            serde_json::to_value(step_start).expect("step-start event serializes");
        assert_eq!(step_start_value["stepNumber"], json!(0));
        assert_eq!(
            serde_json::from_value::<GenerateTextStepStartEvent>(step_start_value)
                .expect("step-start event deserializes"),
            step_start.clone()
        );
    }

    #[test]
    fn tool_execution_events_round_trip_json() {
        let tool_call = GenerateTextToolCall {
            tool_call_id: "call-1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            title: Some("Weather".to_string()),
            provider_executed: None,
            dynamic: None,
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: Some(
                json!({
                    "source": "local"
                })
                .as_object()
                .expect("metadata is object")
                .clone(),
            ),
        };
        let prompt = vec![user_message("Weather?")];
        let start_event = GenerateTextToolExecutionStartEvent {
            call_id: "call_123".to_string(),
            messages: prompt.clone(),
            tool_call: tool_call.clone(),
            tool_context: Some(json!({ "unit": "celsius" })),
        };
        let start_value = serde_json::to_value(&start_event).expect("start event serializes");
        assert_eq!(start_value["callId"], json!("call_123"));
        assert_eq!(start_value["toolContext"]["unit"], json!("celsius"));
        assert_eq!(
            serde_json::from_value::<GenerateTextToolExecutionStartEvent>(start_value)
                .expect("start event deserializes"),
            start_event
        );

        let end_event = GenerateTextToolExecutionEndEvent {
            call_id: "call_123".to_string(),
            messages: prompt,
            tool_call: tool_call.clone(),
            tool_context: None,
            tool_execution_ms: 25,
            tool_output: GenerateTextToolResult::success(
                &tool_call,
                json!({
                    "forecast": "sunny"
                }),
            ),
        };
        let end_value = serde_json::to_value(&end_event).expect("end event serializes");
        assert_eq!(end_value["toolExecutionMs"], json!(25));
        assert_eq!(end_value["toolOutput"]["type"], json!("tool-result"));
        assert!(end_value.get("toolContext").is_none());
        assert_eq!(
            serde_json::from_value::<GenerateTextToolExecutionEndEvent>(end_value)
                .expect("end event deserializes"),
            end_event
        );
    }

    #[test]
    fn generate_text_invokes_tool_execution_callbacks_around_local_tools() {
        let model = ToolLoopLanguageModel::new();
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let start_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionStartEvent>::new()));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let events_for_start = Arc::clone(&events);
        let events_for_end = Arc::clone(&events);
        let start_events_for_callback = Arc::clone(&start_events);
        let end_events_for_callback = Arc::clone(&end_events);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", approval_tool_schema())
                        .with_execute(|input, options| async move {
                            Ok(json!({
                                "city": input["city"],
                                "toolCallId": options.tool_call_id,
                                "unit": options.context.as_ref().and_then(|context| context["unit"].as_str())
                            }))
                        }),
                )
                .with_tool_context(
                    "weather",
                    json!({
                        "unit": "celsius"
                    }),
                )
                .with_max_steps(2)
                .with_on_tool_execution_start(move |event| {
                    let events = Arc::clone(&events_for_start);
                    let start_events = Arc::clone(&start_events_for_callback);
                    async move {
                        events
                            .lock()
                            .expect("events lock")
                            .push("tool-start".to_string());
                        start_events.lock().expect("start events lock").push(event);
                    }
                })
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&events_for_end);
                    let end_events = Arc::clone(&end_events_for_callback);
                    async move {
                        events
                            .lock()
                            .expect("events lock")
                            .push("tool-end".to_string());
                        end_events.lock().expect("end events lock").push(event);
                    }
                }),
        ));

        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            ["tool-start", "tool-end"]
        );
        let start_events = start_events.lock().expect("start events lock");
        assert_eq!(start_events.len(), 1);
        assert_eq!(start_events[0].call_id, result.final_step.call_id);
        assert_eq!(start_events[0].tool_call.tool_call_id, "call-1");
        assert_eq!(start_events[0].tool_call.tool_name, "weather");
        assert_eq!(
            start_events[0].tool_context,
            Some(json!({ "unit": "celsius" }))
        );
        assert_eq!(start_events[0].messages, model.calls.borrow()[0].prompt);
        drop(start_events);

        let end_events = end_events.lock().expect("end events lock");
        assert_eq!(end_events.len(), 1);
        assert_eq!(end_events[0].call_id, result.final_step.call_id);
        assert_eq!(end_events[0].tool_call.tool_call_id, "call-1");
        assert_eq!(
            end_events[0].tool_context,
            Some(json!({ "unit": "celsius" }))
        );
        assert!(end_events[0].tool_execution_ms <= result.steps[0].performance.step_time_ms);
        assert_eq!(end_events[0].tool_output, result.tool_results[0]);
        assert_eq!(end_events[0].tool_output.output["unit"], json!("celsius"));
    }

    #[test]
    fn generate_text_invokes_finish_callbacks_with_completed_records() {
        let model = FakeLanguageModel::new();
        let step_events = Arc::new(Mutex::new(Vec::<GenerateTextStep>::new()));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let step_events_for_callback = Arc::clone(&step_events);
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Say hello")])
                .with_on_step_finish(move |step| {
                    let step_events = Arc::clone(&step_events_for_callback);
                    async move {
                        step_events.lock().expect("step events lock").push(step);
                    }
                })
                .with_on_finish(move |result| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(result);
                    }
                }),
        ));

        let step_events = step_events.lock().expect("step events lock");
        assert_eq!(step_events.len(), 1);
        assert_eq!(step_events[0], result.final_step);
        assert_eq!(step_events[0].response_messages, result.response_messages);
        assert!(
            step_events[0].performance.response_time_ms <= step_events[0].performance.step_time_ms
        );
        drop(step_events);

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert_eq!(finish_events[0].call_id, result.final_step.call_id);
        assert_eq!(finish_events[0].step_number, result.final_step.step_number);
        assert_eq!(finish_events[0].model, result.final_step.model);
        assert_eq!(finish_events[0].text, result.text);
        assert_eq!(finish_events[0].finish_reason, result.finish_reason);
        assert_eq!(finish_events[0].usage, result.final_step.usage);
        assert_eq!(finish_events[0].total_usage, result.total_usage);
        assert_eq!(finish_events[0].response_messages, result.response_messages);
        assert_eq!(finish_events[0].steps, result.steps);
        let finish_value =
            serde_json::to_value(&finish_events[0]).expect("finish event serializes");
        assert!(finish_value.get("output").is_none());
        assert!(finish_value.get("performance").is_none());
        assert_eq!(
            serde_json::from_value::<GenerateTextFinishEvent>(finish_value)
                .expect("finish event deserializes"),
            finish_events[0]
        );
    }

    #[test]
    fn generate_text_response_messages_preserve_tool_approval_requests() {
        let model = FakeLanguageModel::new().with_content(vec![
            LanguageModelContent::ToolCall(
                LanguageModelToolCall::new("tool_call_123", "webSearch", r#"{"query":"weather"}"#)
                    .with_provider_executed(true),
            ),
            LanguageModelContent::ToolApprovalRequest(LanguageModelToolApprovalRequest::new(
                "approval_123",
                "tool_call_123",
            )),
        ]);

        let result = poll_ready(generate_text(GenerateTextOptions::new(
            &model,
            vec![user_message("Search the web")],
        )));

        assert_eq!(
            result.response_messages,
            vec![LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::ToolCall(
                        LanguageModelToolCallPart::new(
                            "tool_call_123",
                            "webSearch",
                            json!({ "query": "weather" }),
                        )
                        .with_provider_executed(true)
                    ),
                    LanguageModelAssistantContentPart::ToolApprovalRequest(
                        LanguageModelToolApprovalRequestPart::new("approval_123", "tool_call_123",)
                    ),
                ])
            )]
        );
    }

    #[test]
    fn generate_text_include_controls_retained_provider_bodies() {
        let model = FakeLanguageModel::new().with_body_metadata();
        let prompt = vec![user_message("Say hello")];

        let excluded = poll_ready(generate_text(GenerateTextOptions::new(
            &model,
            prompt.clone(),
        )));

        let excluded_step = excluded.final_step().expect("step exists");
        assert_eq!(
            excluded_step
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref()),
            None
        );
        assert_eq!(
            excluded_step
                .request
                .as_ref()
                .and_then(|request| request.messages.as_ref()),
            None
        );
        assert_eq!(
            excluded_step
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("resp_body")
        );
        assert_eq!(
            excluded_step
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref()),
            None
        );
        assert_eq!(
            excluded
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref()),
            None
        );

        let included = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone()).with_include(
                GenerateTextInclude::new()
                    .with_request_body(true)
                    .with_request_messages(true)
                    .with_response_body(true),
            ),
        ));

        let included_step = included.final_step().expect("step exists");
        assert_eq!(
            included_step
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref()),
            Some(&json!({
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ]
            }))
        );
        assert_eq!(
            included_step
                .request
                .as_ref()
                .and_then(|request| request.messages.as_ref()),
            Some(&prompt)
        );
        assert_eq!(
            included_step
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref()),
            Some(&json!({
                "id": "resp_body"
            }))
        );
        assert_eq!(
            included
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref()),
            Some(&json!({
                "id": "resp_body"
            }))
        );
    }

    #[test]
    fn generate_text_result_serializes_as_camel_case_step_record() {
        let result = GenerateTextResult::from_steps(vec![GenerateTextStep {
            call_id: "call-test".to_string(),
            step_number: 0,
            model: GenerateTextModelInfo::new("test-provider", "test-model"),
            tools_context: crate::JsonObject::new(),
            runtime_context: crate::JsonObject::new(),
            content: vec![GenerateTextContentPart::Text(LanguageModelText::new(
                "Hello",
            ))],
            tool_calls: Vec::new(),
            static_tool_calls: Vec::new(),
            dynamic_tool_calls: Vec::new(),
            tool_results: Vec::new(),
            static_tool_results: Vec::new(),
            dynamic_tool_results: Vec::new(),
            response_messages: vec![LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Hello")),
                ]),
            )],
            files: Vec::new(),
            reasoning: Vec::new(),
            reasoning_text: None,
            sources: Vec::new(),
            text: "Hello".to_string(),
            finish_reason: FinishReason::Stop,
            raw_finish_reason: Some("stop".to_string()),
            usage: LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(3),
                    ..InputTokenUsage::default()
                },
                output_tokens: OutputTokenUsage {
                    total: Some(1),
                    ..OutputTokenUsage::default()
                },
                raw: None,
            },
            performance: GenerateTextStepPerformance {
                effective_output_tokens_per_second: 2.5,
                effective_total_tokens_per_second: 10.0,
                step_time_ms: 750,
                response_time_ms: 400,
                tool_execution_ms: BTreeMap::from([("call-1".to_string(), 25)]),
                ..GenerateTextStepPerformance::default()
            },
            warnings: Vec::new(),
            request: None,
            response: None,
            provider_metadata: None,
        }]);

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": "Hello"
                    }
                ],
                "responseMessages": [
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "text",
                                "text": "Hello"
                            }
                        ]
                    }
                ],
                "text": "Hello",
                "output": "Hello",
                "finishReason": "stop",
                "rawFinishReason": "stop",
                "usage": {
                    "inputTokens": {
                        "total": 3
                    },
                    "outputTokens": {
                        "total": 1
                    }
                },
                "totalUsage": {
                    "inputTokens": {
                        "total": 3
                    },
                    "outputTokens": {
                        "total": 1
                    }
                },
                "warnings": [],
                "steps": [
                    {
                        "callId": "call-test",
                        "stepNumber": 0,
                        "model": {
                            "provider": "test-provider",
                            "modelId": "test-model"
                        },
                        "toolsContext": {},
                        "runtimeContext": {},
                        "content": [
                            {
                                "type": "text",
                                "text": "Hello"
                            }
                        ],
                        "responseMessages": [
                            {
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Hello"
                                    }
                                ]
                            }
                        ],
                        "text": "Hello",
                        "finishReason": "stop",
                        "rawFinishReason": "stop",
                        "usage": {
                            "inputTokens": {
                                "total": 3
                            },
                            "outputTokens": {
                                "total": 1
                            }
                        },
                        "performance": {
                            "effectiveOutputTokensPerSecond": 2.5,
                            "effectiveTotalTokensPerSecond": 10.0,
                            "stepTimeMs": 750,
                            "responseTimeMs": 400,
                            "toolExecutionMs": {
                                "call-1": 25
                            }
                        },
                        "warnings": []
                    }
                ],
                "finalStep": {
                    "callId": "call-test",
                    "stepNumber": 0,
                    "model": {
                        "provider": "test-provider",
                        "modelId": "test-model"
                    },
                    "toolsContext": {},
                    "runtimeContext": {},
                    "content": [
                        {
                            "type": "text",
                            "text": "Hello"
                        }
                    ],
                    "responseMessages": [
                        {
                            "role": "assistant",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Hello"
                                }
                            ]
                        }
                    ],
                    "text": "Hello",
                    "finishReason": "stop",
                    "rawFinishReason": "stop",
                    "usage": {
                        "inputTokens": {
                            "total": 3
                        },
                        "outputTokens": {
                            "total": 1
                        }
                    },
                    "performance": {
                        "effectiveOutputTokensPerSecond": 2.5,
                        "effectiveTotalTokensPerSecond": 10.0,
                        "stepTimeMs": 750,
                        "responseTimeMs": 400,
                        "toolExecutionMs": {
                            "call-1": 25
                        }
                    },
                    "warnings": []
                }
            })
        );
    }

    #[test]
    fn generate_text_tool_call_and_result_serialize_as_camel_case_contracts() {
        let tool_metadata = json!({ "source": "mcp" })
            .as_object()
            .expect("metadata is an object")
            .clone();
        let tool_call = GenerateTextToolCall {
            tool_call_id: "call-1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            title: Some("Weather information".to_string()),
            provider_executed: Some(false),
            dynamic: Some(false),
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: Some(tool_metadata.clone()),
        };
        let tool_result = GenerateTextToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            output: json!({ "forecast": "sunny" }),
            title: Some("Weather information".to_string()),
            is_error: None,
            provider_executed: None,
            dynamic: None,
            preliminary: None,
            provider_metadata: None,
            tool_metadata: Some(tool_metadata),
        };

        assert_eq!(
            serde_json::to_value(&tool_call).expect("tool call serializes"),
            json!({
                "type": "tool-call",
                "toolCallId": "call-1",
                "toolName": "weather",
                "input": { "city": "Brisbane" },
                "title": "Weather information",
                "providerExecuted": false,
                "dynamic": false,
                "toolMetadata": {
                    "source": "mcp"
                }
            })
        );
        assert_eq!(
            serde_json::to_value(&tool_result).expect("tool result serializes"),
            json!({
                "type": "tool-result",
                "toolCallId": "call-1",
                "toolName": "weather",
                "input": { "city": "Brisbane" },
                "output": { "forecast": "sunny" },
                "title": "Weather information",
                "toolMetadata": {
                    "source": "mcp"
                }
            })
        );

        assert_eq!(
            serde_json::from_value::<GenerateTextToolCall>(
                serde_json::to_value(tool_call.clone()).expect("tool call serializes")
            )
            .expect("tool call deserializes"),
            tool_call
        );
        assert_eq!(
            serde_json::from_value::<GenerateTextToolResult>(
                serde_json::to_value(tool_result.clone()).expect("tool result serializes")
            )
            .expect("tool result deserializes"),
            tool_result
        );
    }

    #[test]
    fn generate_text_upstream_tool_aliases_and_denied_output_match_contracts() {
        let active_tools: ActiveTools = Some(vec!["weather".to_string()]);
        assert_eq!(active_tools, Some(vec!["weather".to_string()]));

        let tool_call = GenerateTextToolCall {
            tool_call_id: "call-1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            title: None,
            provider_executed: None,
            dynamic: Some(false),
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        let static_call: StaticToolCall = tool_call.clone();
        let dynamic_call: DynamicToolCall = tool_call.clone();
        let typed_call: TypedToolCall = tool_call.clone();

        let tool_result = GenerateTextToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            output: json!("sunny"),
            title: None,
            is_error: None,
            provider_executed: None,
            dynamic: Some(false),
            preliminary: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        let static_result: StaticToolResult = tool_result.clone();
        let dynamic_result: DynamicToolResult = tool_result.clone();
        let typed_result: TypedToolResult = tool_result.clone();

        let tool_error = GenerateTextToolError::new(
            "call-1",
            "weather",
            json!({ "city": "Brisbane" }),
            json!("denied"),
        )
        .with_dynamic(false);
        let static_error: StaticToolError = tool_error.clone();
        let dynamic_error: DynamicToolError = tool_error.clone();
        let typed_error: TypedToolError = tool_error.clone();

        let denied: StaticToolOutputDenied = GenerateTextToolOutputDenied::new("call-1", "weather")
            .with_provider_executed(true)
            .with_dynamic(false);
        let typed_denied: TypedToolOutputDenied = denied.clone();

        assert_eq!(static_call, tool_call);
        assert_eq!(dynamic_call, tool_call);
        assert_eq!(typed_call, dynamic_call);
        assert_eq!(static_result, tool_result);
        assert_eq!(dynamic_result, tool_result);
        assert_eq!(typed_result, dynamic_result);
        assert_eq!(static_error, tool_error);
        assert_eq!(dynamic_error, tool_error);
        assert_eq!(typed_error, dynamic_error);
        assert_eq!(typed_denied, denied);

        let denied_value = serde_json::to_value(&denied).expect("denied output serializes");
        assert_eq!(
            denied_value,
            json!({
                "type": "tool-output-denied",
                "toolCallId": "call-1",
                "toolName": "weather",
                "providerExecuted": true,
                "dynamic": false
            })
        );
        assert_eq!(
            serde_json::from_value::<GenerateTextToolOutputDenied>(denied_value)
                .expect("denied output deserializes"),
            denied
        );
    }

    #[test]
    fn generate_text_content_parts_round_trip_upstream_high_level_shapes() {
        let mut provider_metadata = ProviderMetadata::new();
        provider_metadata.insert(
            "test".to_string(),
            json!({ "requestId": "req-123" })
                .as_object()
                .expect("provider metadata object")
                .clone(),
        );
        let tool_metadata = json!({ "source": "mcp" })
            .as_object()
            .expect("tool metadata object")
            .clone();
        let tool_call = GenerateTextToolCall {
            tool_call_id: "call-1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "Brisbane" }),
            title: Some("Weather information".to_string()),
            provider_executed: Some(true),
            dynamic: Some(true),
            invalid: None,
            error: None,
            provider_metadata: Some(provider_metadata.clone()),
            tool_metadata: Some(tool_metadata.clone()),
        };
        let parts = vec![
            ContentPart::File(
                GenerateTextFileContent::new(GeneratedFile::from_base64("image/png", "aGVsbG8="))
                    .with_provider_metadata(provider_metadata.clone()),
            ),
            ContentPart::ToolError(
                GenerateTextToolError::new(
                    "call-1",
                    "weather",
                    json!({ "city": "Brisbane" }),
                    json!("failed"),
                )
                .with_title("Weather information")
                .with_provider_executed(true)
                .with_dynamic(true)
                .with_provider_metadata(provider_metadata.clone())
                .with_tool_metadata(tool_metadata.clone()),
            ),
            ContentPart::ToolApprovalRequest(
                ToolApprovalRequestOutput::new("approval-1", tool_call.clone())
                    .with_automatic(true),
            ),
            ContentPart::ToolApprovalResponse(
                ToolApprovalResponseOutput::new("approval-1", tool_call, false)
                    .with_reason("policy block")
                    .with_provider_executed(true),
            ),
        ];

        let value = serde_json::to_value(&parts).expect("content parts serialize");
        assert_eq!(
            value,
            json!([
                {
                    "type": "file",
                    "file": {
                        "base64": "aGVsbG8=",
                        "mediaType": "image/png"
                    },
                    "providerMetadata": {
                        "test": {
                            "requestId": "req-123"
                        }
                    }
                },
                {
                    "type": "tool-error",
                    "toolCallId": "call-1",
                    "toolName": "weather",
                    "input": {
                        "city": "Brisbane"
                    },
                    "error": "failed",
                    "title": "Weather information",
                    "providerExecuted": true,
                    "dynamic": true,
                    "providerMetadata": {
                        "test": {
                            "requestId": "req-123"
                        }
                    },
                    "toolMetadata": {
                        "source": "mcp"
                    }
                },
                {
                    "type": "tool-approval-request",
                    "approvalId": "approval-1",
                    "toolCall": {
                        "type": "tool-call",
                        "toolCallId": "call-1",
                        "toolName": "weather",
                        "input": {
                            "city": "Brisbane"
                        },
                        "title": "Weather information",
                        "providerExecuted": true,
                        "dynamic": true,
                        "providerMetadata": {
                            "test": {
                                "requestId": "req-123"
                            }
                        },
                        "toolMetadata": {
                            "source": "mcp"
                        }
                    },
                    "isAutomatic": true
                },
                {
                    "type": "tool-approval-response",
                    "approvalId": "approval-1",
                    "toolCall": {
                        "type": "tool-call",
                        "toolCallId": "call-1",
                        "toolName": "weather",
                        "input": {
                            "city": "Brisbane"
                        },
                        "title": "Weather information",
                        "providerExecuted": true,
                        "dynamic": true,
                        "providerMetadata": {
                            "test": {
                                "requestId": "req-123"
                            }
                        },
                        "toolMetadata": {
                            "source": "mcp"
                        }
                    },
                    "approved": false,
                    "reason": "policy block",
                    "providerExecuted": true
                }
            ])
        );
        assert_eq!(
            serde_json::from_value::<Vec<ContentPart>>(value).expect("content parts deserialize"),
            parts
        );
    }

    #[test]
    fn generate_text_splits_static_and_dynamic_tool_calls_and_results() {
        let step = GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                        "static-call",
                        "weather",
                        r#"{"city":"Brisbane"}"#,
                    )),
                    LanguageModelContent::ToolResult(LanguageModelToolResult::new(
                        "static-call",
                        "weather",
                        crate::NonNullJsonValue::new(json!("sunny"))
                            .expect("tool result is non-null"),
                    )),
                    LanguageModelContent::ToolCall(
                        LanguageModelToolCall::new("dynamic-call", "webSearch", "{}")
                            .with_dynamic(true)
                            .with_provider_executed(true),
                    ),
                    LanguageModelContent::ToolResult(
                        LanguageModelToolResult::new(
                            "dynamic-call",
                            "webSearch",
                            crate::NonNullJsonValue::new(json!({ "results": 3 }))
                                .expect("tool result is non-null"),
                        )
                        .with_dynamic(true),
                    ),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: Some("tool-calls".to_string()),
                },
                LanguageModelUsage::default(),
            ),
        );

        let result = GenerateTextResult::from_steps(vec![step]);

        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.static_tool_calls.len(), 1);
        assert_eq!(result.static_tool_calls[0].tool_name, "weather");
        assert_eq!(result.dynamic_tool_calls.len(), 1);
        assert_eq!(result.dynamic_tool_calls[0].tool_name, "webSearch");
        assert_eq!(result.tool_results.len(), 2);
        assert_eq!(result.static_tool_results.len(), 1);
        assert_eq!(result.static_tool_results[0].tool_name, "weather");
        assert_eq!(result.dynamic_tool_results.len(), 1);
        assert_eq!(result.dynamic_tool_results[0].tool_name, "webSearch");

        let value = serde_json::to_value(&result).expect("result serializes");
        assert_eq!(value["staticToolCalls"][0]["toolName"], json!("weather"));
        assert_eq!(value["dynamicToolCalls"][0]["toolName"], json!("webSearch"));
        assert_eq!(
            value["steps"][0]["staticToolResults"][0]["toolName"],
            json!("weather")
        );
        assert_eq!(
            value["steps"][0]["dynamicToolResults"][0]["toolName"],
            json!("webSearch")
        );
    }

    #[test]
    fn generate_text_reasoning_deserializes_generated_reasoning_shapes() {
        let reasoning: GenerateTextReasoning = serde_json::from_value(json!({
            "type": "reasoning",
            "text": "thinking"
        }))
        .expect("reasoning deserializes");
        let reasoning_file: GenerateTextReasoning = serde_json::from_value(json!({
            "type": "reasoning-file",
            "file": {
                "base64": "notes",
                "mediaType": "text/plain"
            }
        }))
        .expect("reasoning file deserializes");

        assert_eq!(
            reasoning,
            GenerateTextReasoning::Reasoning(ReasoningOutput::new("thinking"))
        );
        assert_eq!(
            reasoning_file,
            GenerateTextReasoning::ReasoningFile(ReasoningFileOutput::new(
                GeneratedFile::from_base64("text/plain", "notes")
            ))
        );
    }

    #[test]
    fn reasoning_outputs_round_trip_upstream_high_level_shapes() {
        let metadata: ProviderMetadata = serde_json::from_value(json!({
            "test": {
                "traceId": "trace-1"
            }
        }))
        .expect("provider metadata deserializes");
        let reasoning = ReasoningOutput::new("thinking").with_provider_metadata(metadata.clone());
        let reasoning_file =
            ReasoningFileOutput::new(GeneratedFile::from_base64("text/plain", "bm90ZXM="))
                .with_provider_metadata(metadata.clone());

        assert_eq!(
            serde_json::to_value(&reasoning).expect("reasoning output serializes"),
            json!({
                "type": "reasoning",
                "text": "thinking",
                "providerMetadata": {
                    "test": {
                        "traceId": "trace-1"
                    }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ReasoningOutput>(
                serde_json::to_value(reasoning.clone()).expect("reasoning output serializes")
            )
            .expect("reasoning output deserializes"),
            reasoning
        );
        assert_eq!(
            serde_json::to_value(&reasoning_file).expect("reasoning file output serializes"),
            json!({
                "type": "reasoning-file",
                "file": {
                    "base64": "bm90ZXM=",
                    "mediaType": "text/plain"
                },
                "providerMetadata": {
                    "test": {
                        "traceId": "trace-1"
                    }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ReasoningFileOutput>(
                serde_json::to_value(reasoning_file.clone())
                    .expect("reasoning file output serializes")
            )
            .expect("reasoning file output deserializes"),
            reasoning_file
        );
    }

    #[test]
    fn generated_file_exposes_base64_and_bytes_views() {
        let from_base64 = GeneratedFile::from_base64("text/plain", "SGVsbG8=");
        let from_bytes = GeneratedFile::from_bytes("text/plain", b"Hello".to_vec());

        assert_eq!(from_base64.media_type(), "text/plain");
        assert_eq!(from_base64.base64(), "SGVsbG8=");
        assert_eq!(
            from_base64.bytes().expect("base64 decodes"),
            b"Hello".to_vec()
        );
        assert_eq!(
            from_base64.uint8_array().expect("uint8 array decodes"),
            b"Hello".to_vec()
        );
        assert_eq!(from_bytes.base64(), "SGVsbG8=");
        assert_eq!(
            from_bytes.data(),
            &FileDataContent::Bytes(b"Hello".to_vec())
        );
        assert_eq!(
            from_bytes.clone().into_data(),
            FileDataContent::Bytes(b"Hello".to_vec())
        );
    }

    #[test]
    fn generated_file_serializes_upstream_shape() {
        let file = GeneratedFile::from_bytes("image/png", vec![251, 255]);

        assert_eq!(
            serde_json::to_value(&file).expect("generated file serializes"),
            json!({
                "base64": "+/8=",
                "mediaType": "image/png"
            })
        );

        let deserialized: GeneratedFile = serde_json::from_value(json!({
            "base64": "SGVsbG8=",
            "mediaType": "text/plain"
        }))
        .expect("generated file deserializes");

        assert_eq!(deserialized.media_type(), "text/plain");
        assert_eq!(deserialized.base64(), "SGVsbG8=");
        assert_eq!(
            deserialized.bytes().expect("base64 decodes"),
            b"Hello".to_vec()
        );
    }

    #[test]
    fn generated_file_aliases_share_the_same_contract() {
        let default_file: DefaultGeneratedFile =
            GeneratedFile::from_base64("image/jpeg", "anBlZw==");
        let experimental_image: ExperimentalGeneratedImage =
            GeneratedFile::from_bytes("image/jpeg", b"jpeg".to_vec());

        assert_eq!(default_file.base64(), experimental_image.base64());
        assert_eq!(default_file.media_type(), experimental_image.media_type());
    }

    #[test]
    fn generated_file_converts_from_provider_v4_file_parts() {
        let data_file = LanguageModelFile::new(
            "text/plain",
            LanguageModelFileData::Data {
                data: FileDataContent::Bytes(b"Hello".to_vec()),
            },
        );
        let url_file = LanguageModelFile::new(
            "image/png",
            LanguageModelFileData::Url {
                url: "https://example.com/image.png".parse().expect("valid URL"),
            },
        );

        let generated_data_file = GeneratedFile::from_language_model_file(&data_file);
        let generated_url_file = GeneratedFile::from_language_model_file(&url_file);

        assert_eq!(generated_data_file.base64(), "SGVsbG8=");
        assert_eq!(generated_url_file.media_type(), "image/png");
        assert_eq!(generated_url_file.base64(), "https://example.com/image.png");
    }

    #[test]
    fn generate_text_result_deserializes_minimal_contract() {
        let result: GenerateTextResult = serde_json::from_value(json!({
            "content": [],
            "text": "",
            "finishReason": "length",
            "usage": {
                "inputTokens": {},
                "outputTokens": {}
            },
            "totalUsage": {
                "inputTokens": {
                    "total": 12
                },
                "outputTokens": {
                    "total": 4
                }
            },
            "warnings": [],
            "steps": [
                {
                    "callId": "call-test",
                    "stepNumber": 0,
                    "model": {
                        "provider": "test-provider",
                        "modelId": "test-model"
                    },
                    "content": [],
                    "text": "",
                    "finishReason": "length",
                    "usage": {
                        "inputTokens": {},
                        "outputTokens": {}
                    },
                    "warnings": []
                }
            ],
            "finalStep": {
                "callId": "call-test",
                "stepNumber": 0,
                "model": {
                    "provider": "test-provider",
                    "modelId": "test-model"
                },
                "content": [],
                "text": "",
                "finishReason": "length",
                "usage": {
                    "inputTokens": {},
                    "outputTokens": {}
                },
                "warnings": []
            }
        }))
        .expect("result deserializes");

        assert_eq!(result.text, "");
        assert_eq!(result.finish_reason, FinishReason::Length);
        assert_eq!(result.raw_finish_reason, None);
        assert_eq!(result.total_usage.input_tokens.total, Some(12));
        assert_eq!(result.total_usage.output_tokens.total, Some(4));
        assert_eq!(result.output, None);
        assert_eq!(
            result
                .output()
                .expect_err("length finish has no parsed output"),
            NoOutputGeneratedError::new()
        );
        assert_eq!(result.static_tool_calls, Vec::new());
        assert_eq!(result.dynamic_tool_calls, Vec::new());
        assert_eq!(result.static_tool_results, Vec::new());
        assert_eq!(result.dynamic_tool_results, Vec::new());
        assert_eq!(result.files, Vec::new());
        assert_eq!(result.sources, Vec::new());
        assert_eq!(result.steps[0].raw_finish_reason, None);
        assert_eq!(result.steps[0].static_tool_calls, Vec::new());
        assert_eq!(result.steps[0].dynamic_tool_calls, Vec::new());
        assert_eq!(result.steps[0].static_tool_results, Vec::new());
        assert_eq!(result.steps[0].dynamic_tool_results, Vec::new());
        assert_eq!(result.steps[0].files, Vec::new());
        assert_eq!(result.steps[0].sources, Vec::new());
        assert_eq!(result.steps[0].call_id, "call-test");
        assert!(result.steps[0].runtime_context.is_empty());
        assert!(result.steps[0].tools_context.is_empty());
        assert_eq!(
            result.steps[0].model,
            GenerateTextModelInfo::new("test-provider", "test-model")
        );
        assert_eq!(result.final_step, result.steps[0]);
    }

    #[test]
    fn generate_text_surfaces_files_across_result_and_steps() {
        let first_file = LanguageModelFile::new(
            "image/png",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("AQID".to_string()),
            },
        );
        let second_file = LanguageModelFile::new(
            "image/jpeg",
            LanguageModelFileData::Data {
                data: FileDataContent::Bytes(vec![4, 5, 6]),
            },
        );
        let first_step = GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Text(LanguageModelText::new("First")),
                    LanguageModelContent::File(first_file.clone()),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );
        let second_step = GenerateTextStep::from_language_model_result(
            "call-test",
            1,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::File(second_file.clone()),
                    LanguageModelContent::Text(LanguageModelText::new("Done")),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );

        let result = GenerateTextResult::from_steps(vec![first_step, second_step]);
        let first_generated_file = GeneratedFile::from_language_model_file(&first_file);
        let second_generated_file = GeneratedFile::from_language_model_file(&second_file);

        assert_eq!(
            result.files,
            vec![first_generated_file.clone(), second_generated_file.clone()]
        );
        assert_eq!(result.steps[0].files, vec![first_generated_file]);
        assert_eq!(result.steps[1].files, vec![second_generated_file]);
        assert_eq!(
            serde_json::to_value(&result.files).expect("files serialize"),
            json!([
                {
                    "base64": "AQID",
                    "mediaType": "image/png",
                },
                {
                    "base64": "BAUG",
                    "mediaType": "image/jpeg",
                }
            ])
        );
    }

    #[test]
    fn generate_text_surfaces_final_step_reasoning_and_reasoning_text() {
        let reasoning_file = LanguageModelReasoningFile::new(
            "image/png",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("cmVhc29uaW5n".to_string()),
            },
        );
        let first_step = GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Reasoning(
                    LanguageModelReasoning::new("first thoughts"),
                )],
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );
        let second_step = GenerateTextStep::from_language_model_result(
            "call-test",
            1,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Reasoning(LanguageModelReasoning::new("final ")),
                    LanguageModelContent::ReasoningFile(reasoning_file.clone()),
                    LanguageModelContent::Reasoning(LanguageModelReasoning::new("thoughts")),
                    LanguageModelContent::Text(LanguageModelText::new("Done")),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );

        let result = GenerateTextResult::from_steps(vec![first_step, second_step]);

        assert_eq!(
            result.steps[0].reasoning,
            vec![GenerateTextReasoning::Reasoning(ReasoningOutput::new(
                "first thoughts"
            ))]
        );
        assert_eq!(
            result.steps[0].reasoning_text.as_deref(),
            Some("first thoughts")
        );
        assert_eq!(result.reasoning_text.as_deref(), Some("final thoughts"));
        assert_eq!(
            result.reasoning,
            vec![
                GenerateTextReasoning::Reasoning(ReasoningOutput::new("final ")),
                GenerateTextReasoning::ReasoningFile(
                    ReasoningFileOutput::from_language_model_reasoning_file(&reasoning_file)
                ),
                GenerateTextReasoning::Reasoning(ReasoningOutput::new("thoughts")),
            ]
        );
        assert_eq!(
            serde_json::to_value(&result.reasoning).expect("reasoning serializes"),
            json!([
                {
                    "type": "reasoning",
                    "text": "final "
                },
                {
                    "type": "reasoning-file",
                    "file": {
                        "base64": "cmVhc29uaW5n",
                        "mediaType": "image/png"
                    }
                },
                {
                    "type": "reasoning",
                    "text": "thoughts"
                }
            ])
        );
    }

    #[test]
    fn generate_text_surfaces_sources_across_result_and_steps() {
        let first_source = LanguageModelSource::url("source-1", "https://example.com/one");
        let second_source =
            LanguageModelSource::document("source-2", "application/pdf", "Reference PDF");
        let first_step = GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Text(LanguageModelText::new("First")),
                    LanguageModelContent::Source(first_source.clone()),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );
        let second_step = GenerateTextStep::from_language_model_result(
            "call-test",
            1,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Source(second_source.clone()),
                    LanguageModelContent::Text(LanguageModelText::new("Done")),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );

        let result = GenerateTextResult::from_steps(vec![first_step, second_step]);

        assert_eq!(
            result.sources,
            vec![first_source.clone(), second_source.clone()]
        );
        assert_eq!(result.steps[0].sources, vec![first_source]);
        assert_eq!(result.steps[1].sources, vec![second_source]);
        assert_eq!(
            serde_json::to_value(&result.sources).expect("sources serialize"),
            json!([
                {
                    "type": "source",
                    "sourceType": "url",
                    "id": "source-1",
                    "url": "https://example.com/one"
                },
                {
                    "type": "source",
                    "sourceType": "document",
                    "id": "source-2",
                    "mediaType": "application/pdf",
                    "title": "Reference PDF"
                }
            ])
        );
    }

    #[test]
    fn generate_text_concatenates_only_final_step_text_parts() {
        let step = GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Text(LanguageModelText::new("visible")),
                    LanguageModelContent::Text(LanguageModelText::new(" text")),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );

        assert_eq!(step.text, "visible text");
    }

    #[test]
    fn generate_text_options_can_wrap_prepared_call_options() {
        let model = FakeLanguageModel::new();
        let call_options = LanguageModelCallOptions::new(vec![user_message("Hello")])
            .with_seed(7)
            .with_response_format(crate::language_model::LanguageModelResponseFormat::text());

        let result = poll_ready(generate_text(GenerateTextOptions::from_call_options(
            &model,
            call_options,
        )));

        assert_eq!(result.text, "Hello world");
        assert_eq!(model.calls.borrow()[0].seed, Some(7));
        assert_eq!(
            model.calls.borrow()[0].response_format,
            Some(crate::language_model::LanguageModelResponseFormat::text())
        );
    }

    #[test]
    fn generate_text_passes_high_level_rust_tools_to_language_model() {
        let model = FakeLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let _ = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")]).with_tool(
                Tool::new("weather", input_schema.clone())
                    .with_description("Look up weather.")
                    .with_strict(true),
            ),
        ));

        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", input_schema)
                    .with_description("Look up weather.")
                    .with_strict(true)
            )])
        );
    }

    #[test]
    fn generate_text_passes_provider_tools_to_language_model() {
        let model = FakeLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let output_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let args = json!({ "location": "AU" })
            .as_object()
            .expect("args are an object")
            .clone();

        let _ = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Search?")]).with_tool(
                Tool::provider_executed(
                    "webSearch",
                    "provider.web_search",
                    args.clone(),
                    input_schema,
                    output_schema,
                )
                .with_supports_deferred_results(true),
            ),
        ));

        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Provider(
                LanguageModelProviderTool::new("provider.web_search", "webSearch", args)
            )])
        );
    }

    #[test]
    fn generate_text_prepare_step_overrides_step_settings_and_carries_contexts() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema.clone()).with_execute(
                    |_input, options| async move {
                        Ok(options.context.unwrap_or_else(|| json!("missing context")))
                    },
                ))
                .with_tool(Tool::new("forecast", input_schema))
                .with_prepare_step(|options| async move {
                    assert_eq!(options.initial_messages, vec![user_message("Weather?")]);

                    let mut runtime_context = serde_json::Map::new();
                    runtime_context.insert(
                        "tenant".to_string(),
                        JsonValue::String(format!("step-{}", options.step_number)),
                    );

                    let mut weather_context = serde_json::Map::new();
                    weather_context.insert(
                        "unit".to_string(),
                        JsonValue::String(format!("unit-{}", options.step_number)),
                    );

                    let mut tools_context = serde_json::Map::new();
                    tools_context.insert("weather".to_string(), JsonValue::Object(weather_context));

                    let mut test_options = serde_json::Map::new();
                    test_options.insert(
                        "step".to_string(),
                        JsonValue::Number(options.step_number.into()),
                    );

                    let mut provider_options = BTreeMap::new();
                    provider_options.insert("test".to_string(), test_options);

                    let result = PrepareStepResult::new()
                        .with_runtime_context(runtime_context)
                        .with_tools_context(tools_context)
                        .with_provider_options(provider_options);

                    if options.step_number == 0 {
                        result.with_active_tools(["weather"]).with_tool_choice(
                            LanguageModelToolChoice::Tool {
                                tool_name: "weather".to_string(),
                            },
                        )
                    } else {
                        assert_eq!(options.steps.len(), 1);
                        assert!(!options.response_messages.is_empty());
                        result
                            .with_active_tools(["forecast"])
                            .with_messages(vec![user_message("Prepared second step")])
                    }
                })
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new(
                    "weather",
                    json!({ "type": "object" })
                        .as_object()
                        .expect("schema is an object")
                        .clone()
                )
            )])
        );
        assert_eq!(
            model.calls.borrow()[0].tool_choice,
            Some(LanguageModelToolChoice::Tool {
                tool_name: "weather".to_string()
            })
        );
        assert_eq!(
            model.calls.borrow()[0].provider_options.as_ref(),
            Some(&BTreeMap::from([(
                "test".to_string(),
                json!({ "step": 0 })
                    .as_object()
                    .expect("provider options are an object")
                    .clone()
            )]))
        );
        assert_eq!(
            model.calls.borrow()[1].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new(
                    "forecast",
                    json!({ "type": "object" })
                        .as_object()
                        .expect("schema is an object")
                        .clone()
                )
            )])
        );
        assert_eq!(
            model.calls.borrow()[1].prompt,
            vec![user_message("Prepared second step")]
        );
        assert_eq!(
            model.calls.borrow()[1].provider_options.as_ref(),
            Some(&BTreeMap::from([(
                "test".to_string(),
                json!({ "step": 1 })
                    .as_object()
                    .expect("provider options are an object")
                    .clone()
            )]))
        );
        assert_eq!(
            result.steps[0].runtime_context,
            json!({ "tenant": "step-0" })
                .as_object()
                .expect("runtime context is an object")
                .clone()
        );
        assert_eq!(
            result.steps[0].tool_results[0].output,
            json!({ "unit": "unit-0" })
        );
        assert_eq!(
            result.steps[1].runtime_context,
            json!({ "tenant": "step-1" })
                .as_object()
                .expect("runtime context is an object")
                .clone()
        );
    }

    #[test]
    fn generate_text_prepare_step_can_override_model_with_same_model_type() {
        let primary = FakeLanguageModel::new().with_content(vec![LanguageModelContent::Text(
            LanguageModelText::new("primary"),
        )]);
        let secondary = FakeLanguageModel::new().with_content(vec![LanguageModelContent::Text(
            LanguageModelText::new("secondary"),
        )]);
        let secondary_model = &secondary;

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&primary, vec![user_message("Hello")]).with_prepare_step(
                move |_options| async move { PrepareStepResult::new().with_model(secondary_model) },
            ),
        ));

        assert_eq!(result.text, "secondary");
        assert!(primary.calls.borrow().is_empty());
        assert_eq!(secondary.calls.borrow().len(), 1);
    }

    #[test]
    fn filter_active_tools_filters_high_level_tool_sets_by_name() {
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let tools = vec![
            Tool::new("weather", input_schema.clone()),
            dynamic_tool("forecast", input_schema),
        ];
        let active_tools = vec!["forecast".to_string()];
        let no_active_tools = None::<&[String]>;
        let empty_active_tools = Vec::<String>::new();

        let unchanged = filter_active_tools(Some(tools.clone()), no_active_tools)
            .expect("missing active tools preserve the tool set");
        assert_eq!(
            unchanged
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["weather", "forecast"]
        );

        let filtered = filter_active_tools(Some(tools.clone()), Some(&active_tools))
            .expect("filtered tools are present");
        assert_eq!(
            filtered
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["forecast"]
        );
        assert!(filter_active_tools(None, Some(&active_tools)).is_none());
        assert!(
            filter_active_tools(Some(tools), Some(&empty_active_tools))
                .expect("empty active tools produce an empty tool set")
                .is_empty()
        );
    }

    #[test]
    fn experimental_filter_active_tools_alias_matches_filter_active_tools() {
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let tools = vec![
            Tool::new("weather", input_schema.clone()),
            dynamic_tool("forecast", input_schema),
        ];
        let active_tools = vec!["weather".to_string()];

        let direct = filter_active_tools(Some(tools.clone()), Some(&active_tools))
            .expect("direct filter keeps tools");
        let aliased = experimental_filter_active_tools(Some(tools), Some(&active_tools))
            .expect("alias filter keeps tools");

        assert_eq!(
            aliased
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            direct
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(aliased.len(), 1);
        assert_eq!(aliased[0].name, "weather");
    }

    #[test]
    fn prune_messages_options_round_trip_upstream_json() {
        let options: PruneMessagesOptions = serde_json::from_value(json!({
            "messages": [],
            "reasoning": "before-last-message",
            "toolCalls": [
                {
                    "type": "before-last-2-messages",
                    "tools": ["weather"]
                }
            ],
            "emptyMessages": "keep"
        }))
        .expect("prune options deserialize");

        assert_eq!(options.reasoning, PruneReasoning::BeforeLastMessage);
        assert_eq!(
            options.tool_calls,
            PruneToolCalls::Rules(vec![
                PruneToolCallRule::before_last_messages(2).with_tools(["weather"])
            ])
        );
        assert_eq!(options.empty_messages, PruneEmptyMessages::Keep);

        let serialized = serde_json::to_value(
            PruneMessagesOptions::new(Vec::new())
                .with_reasoning(PruneReasoning::All)
                .with_tool_calls(PruneToolCalls::BeforeLastMessages(3)),
        )
        .expect("prune options serialize");

        assert_eq!(
            serialized,
            json!({
                "messages": [],
                "reasoning": "all",
                "toolCalls": "before-last-3-messages",
                "emptyMessages": "remove"
            })
        );

        let rule_json = serde_json::to_value(PruneToolCallRule {
            mode: PruneToolCallRuleMode::BeforeLastMessage,
            tools: Some(vec!["weather".to_string()]),
        })
        .expect("tool-call rule serializes");

        assert_eq!(
            rule_json,
            json!({
                "type": "before-last-message",
                "tools": ["weather"]
            })
        );
    }

    #[test]
    fn prune_messages_removes_reasoning_before_last_message() {
        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Reasoning(LanguageModelReasoningPart::new(
                    "hidden",
                )),
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("visible")),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Reasoning(LanguageModelReasoningPart::new(
                    "final reasoning",
                )),
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("final")),
            ])),
        ];

        let pruned = prune_messages(
            PruneMessagesOptions::new(messages).with_reasoning(PruneReasoning::BeforeLastMessage),
        );

        let LanguageModelMessage::Assistant(first) = &pruned[0] else {
            panic!("first message is assistant");
        };
        let LanguageModelMessage::Assistant(second) = &pruned[1] else {
            panic!("second message is assistant");
        };

        assert_eq!(first.content.len(), 1);
        assert!(matches!(
            &first.content[0],
            LanguageModelAssistantContentPart::Text(_)
        ));
        assert_eq!(second.content.len(), 2);
        assert!(matches!(
            &second.content[0],
            LanguageModelAssistantContentPart::Reasoning(_)
        ));
    }

    #[test]
    fn prune_messages_removes_all_tool_parts_and_empty_messages_by_default() {
        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-weather",
                    "weather",
                    json!({ "city": "Brisbane" }),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-weather",
                    "weather",
                    LanguageModelToolResultOutput::text("sunny"),
                )),
            ])),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("next")),
            ])),
        ];

        let pruned = prune_messages(
            PruneMessagesOptions::new(messages).with_tool_calls(PruneToolCalls::All),
        );

        assert_eq!(pruned.len(), 1);
        assert!(matches!(&pruned[0], LanguageModelMessage::User(_)));
    }

    #[test]
    fn prune_messages_keeps_tool_references_needed_by_trailing_messages() {
        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-old",
                    "weather",
                    json!({}),
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-keep",
                    "search",
                    json!({}),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-old",
                    "weather",
                    LanguageModelToolResultOutput::text("old"),
                )),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-keep",
                    "search",
                    LanguageModelToolResultOutput::text("keep"),
                )),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("latest")),
                LanguageModelAssistantContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-keep",
                    "search",
                    LanguageModelToolResultOutput::text("latest"),
                )),
            ])),
        ];

        let pruned = prune_messages(
            PruneMessagesOptions::new(messages).with_tool_calls(PruneToolCalls::BeforeLastMessage),
        );

        let LanguageModelMessage::Assistant(first) = &pruned[0] else {
            panic!("first message is assistant");
        };
        let LanguageModelMessage::Tool(second) = &pruned[1] else {
            panic!("second message is tool");
        };
        let LanguageModelMessage::Assistant(third) = &pruned[2] else {
            panic!("third message is assistant");
        };

        assert_eq!(first.content.len(), 1);
        assert!(matches!(
            &first.content[0],
            LanguageModelAssistantContentPart::ToolCall(part)
                if part.tool_call_id == "call-keep"
        ));
        assert_eq!(second.content.len(), 1);
        assert!(matches!(
            &second.content[0],
            LanguageModelToolContentPart::ToolResult(part)
                if part.tool_call_id == "call-keep"
        ));
        assert_eq!(third.content.len(), 2);
    }

    #[test]
    fn prune_messages_tool_specific_rules_preserve_other_tools() {
        let messages = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-weather",
                    "weather",
                    json!({}),
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-search",
                    "search",
                    json!({}),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-weather", "call-weather"),
                ),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval-search", "call-search"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-weather",
                    "weather",
                    LanguageModelToolResultOutput::text("sunny"),
                )),
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval-search", true),
                ),
            ])),
        ];

        let pruned = prune_messages(
            PruneMessagesOptions::new(messages)
                .with_tool_calls(PruneToolCalls::Rules(vec![
                    PruneToolCallRule::all().with_tools(["weather"]),
                ]))
                .with_empty_messages(PruneEmptyMessages::Keep),
        );

        let LanguageModelMessage::Assistant(first) = &pruned[0] else {
            panic!("first message is assistant");
        };
        let LanguageModelMessage::Tool(second) = &pruned[1] else {
            panic!("second message is tool");
        };

        assert_eq!(first.content.len(), 2);
        assert!(matches!(
            &first.content[0],
            LanguageModelAssistantContentPart::ToolCall(part)
                if part.tool_name == "search"
        ));
        assert!(matches!(
            &first.content[1],
            LanguageModelAssistantContentPart::ToolApprovalRequest(part)
                if part.approval_id == "approval-search"
        ));
        assert_eq!(second.content.len(), 1);
        assert!(matches!(
            &second.content[0],
            LanguageModelToolContentPart::ToolApprovalResponse(_)
        ));
    }

    #[test]
    fn generate_text_filters_active_tools_before_calling_language_model() {
        let model = FakeLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let provider_args = json!({ "location": "AU" })
            .as_object()
            .expect("args are an object")
            .clone();
        let provider_tool = LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "provider.web_search",
            "webSearch",
            provider_args.clone(),
        ));

        let _ = poll_ready(generate_text(
            GenerateTextOptions::from_call_options(
                &model,
                LanguageModelCallOptions::new(vec![user_message("Weather?")])
                    .with_tool(provider_tool),
            )
            .with_tool(Tool::new("weather", input_schema.clone()))
            .with_tool(Tool::new("forecast", input_schema))
            .with_active_tools(["forecast", "webSearch"]),
        ));

        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "provider.web_search",
                    "webSearch",
                    provider_args,
                )),
                LanguageModelTool::Function(LanguageModelFunctionTool::new(
                    "forecast",
                    json!({ "type": "object" })
                        .as_object()
                        .expect("schema is an object")
                        .clone(),
                )),
            ])
        );
    }

    #[test]
    fn generate_text_includes_non_text_content_in_content_but_not_text() {
        let content = vec![LanguageModelContent::ToolCall(
            crate::language_model::LanguageModelToolCall::new("call-1", "lookup", "{}"),
        )];
        let step = GenerateTextStep::from_language_model_result(
            "call-test",
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                content,
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: Some("tool_calls".to_string()),
                },
                LanguageModelUsage::default(),
            ),
        );

        assert_eq!(step.text, "");
        assert_eq!(step.content.len(), 1);
        assert_eq!(step.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn generate_text_allows_assistant_prompt_messages_for_continuations() {
        let model = FakeLanguageModel::new();
        let prompt = vec![LanguageModelMessage::Assistant(
            crate::language_model::LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("previous")),
            ]),
        )];

        let _ = poll_ready(generate_text(GenerateTextOptions::new(
            &model,
            prompt.clone(),
        )));

        assert_eq!(model.calls.borrow()[0].prompt, prompt);
    }

    struct ToolLoopLanguageModel {
        calls: RefCell<Vec<LanguageModelCallOptions>>,
        tool_name: String,
        tool_input: String,
        tool_call_provider_metadata: Option<ProviderMetadata>,
        first_step_prefix: Vec<LanguageModelContent>,
    }

    impl ToolLoopLanguageModel {
        fn new() -> Self {
            Self::with_tool_call("weather", r#"{"city":"Brisbane"}"#)
        }

        fn with_tool_call(tool_name: impl Into<String>, tool_input: impl Into<String>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                tool_name: tool_name.into(),
                tool_input: tool_input.into(),
                tool_call_provider_metadata: None,
                first_step_prefix: Vec::new(),
            }
        }

        fn with_tool_call_metadata(
            tool_name: impl Into<String>,
            tool_input: impl Into<String>,
            provider_metadata: ProviderMetadata,
        ) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                tool_name: tool_name.into(),
                tool_input: tool_input.into(),
                tool_call_provider_metadata: Some(provider_metadata),
                first_step_prefix: Vec::new(),
            }
        }

        fn with_first_step_prefix(
            tool_name: impl Into<String>,
            tool_input: impl Into<String>,
            first_step_prefix: Vec<LanguageModelContent>,
        ) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                tool_name: tool_name.into(),
                tool_input: tool_input.into(),
                tool_call_provider_metadata: None,
                first_step_prefix,
            }
        }
    }

    impl LanguageModel for ToolLoopLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
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
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            let step_number = self.calls.borrow().len();
            self.calls.borrow_mut().push(options);

            if step_number == 0 {
                let mut content = self.first_step_prefix.clone();
                let mut tool_call = LanguageModelToolCall::new(
                    "call-1",
                    self.tool_name.clone(),
                    self.tool_input.clone(),
                );

                if let Some(provider_metadata) = &self.tool_call_provider_metadata {
                    tool_call = tool_call.with_provider_metadata(provider_metadata.clone());
                }

                content.push(LanguageModelContent::ToolCall(tool_call));

                ready(LanguageModelGenerateResult::new(
                    content,
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: Some("tool_calls".to_string()),
                    },
                    LanguageModelUsage {
                        input_tokens: InputTokenUsage {
                            total: Some(4),
                            ..InputTokenUsage::default()
                        },
                        output_tokens: OutputTokenUsage {
                            total: Some(1),
                            ..OutputTokenUsage::default()
                        },
                        raw: None,
                    },
                ))
            } else {
                ready(LanguageModelGenerateResult::new(
                    vec![LanguageModelContent::Text(LanguageModelText::new(
                        "The weather in Brisbane is sunny.",
                    ))],
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("stop".to_string()),
                    },
                    LanguageModelUsage {
                        input_tokens: InputTokenUsage {
                            total: Some(9),
                            ..InputTokenUsage::default()
                        },
                        output_tokens: OutputTokenUsage {
                            total: Some(7),
                            text: Some(7),
                            ..OutputTokenUsage::default()
                        },
                        raw: None,
                    },
                ))
            }
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    #[test]
    fn generate_text_executes_tool_result_and_continues_to_final_text() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, options| async move {
                        Ok(json!({
                            "forecast": "sunny",
                            "city": input["city"],
                            "toolCallId": options.tool_call_id
                        }))
                    },
                ))
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(model.calls.borrow()[1].prompt.len(), 3);
        assert_eq!(
            model.calls.borrow()[1].prompt[1],
            LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                        "call-1",
                        "weather",
                        json!({ "city": "Brisbane" })
                    ))
                ])
            )
        );
        assert_eq!(
            model.calls.borrow()[1].prompt[2],
            LanguageModelMessage::Tool(crate::language_model::LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::json(json!({
                        "forecast": "sunny",
                        "city": "Brisbane",
                        "toolCallId": "call-1"
                    }))
                ))
            ]))
        );

        let continuation_prompt = model.calls.borrow()[1].prompt.clone();
        assert_eq!(result.response_messages.len(), 3);
        assert_eq!(result.response_messages[0], continuation_prompt[1]);
        assert_eq!(result.response_messages[1], continuation_prompt[2]);
        assert_eq!(
            result.response_messages[2],
            LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                        "The weather in Brisbane is sunny.",
                    ))
                ])
            )
        );

        assert_eq!(result.text, "The weather in Brisbane is sunny.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.static_tool_calls.len(), 1);
        assert_eq!(result.static_tool_calls[0].tool_name, "weather");
        assert_eq!(result.dynamic_tool_calls, Vec::new());
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.static_tool_results.len(), 1);
        assert_eq!(result.static_tool_results[0].tool_name, "weather");
        assert_eq!(result.dynamic_tool_results, Vec::new());
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
        assert_eq!(result.usage.input_tokens.total, Some(13));
        assert_eq!(result.usage.output_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.text, Some(7));
        assert!(
            result.steps[0]
                .performance
                .tool_execution_ms
                .contains_key("call-1")
        );
        assert_eq!(
            result.steps[1].performance.tool_execution_ms,
            BTreeMap::new()
        );
    }

    #[test]
    fn generate_text_uses_tool_model_output_for_continuation_messages() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|input, _options| async move {
                            Ok(json!({
                                "forecast": "sunny",
                                "city": input["city"]
                            }))
                        })
                        .with_to_model_output(|options| async move {
                            LanguageModelToolResultOutput::json(json!({
                                "modelFacing": true,
                                "toolCallId": options.tool_call_id,
                                "city": options.input["city"],
                                "forecast": options.output["forecast"]
                            }))
                        }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(
            result.tool_results[0].output,
            json!({
                "forecast": "sunny",
                "city": "Brisbane"
            })
        );

        let calls = model.calls.borrow();
        let LanguageModelMessage::Tool(tool_message) = &calls[1].prompt[2] else {
            panic!("second prompt includes tool response");
        };

        assert_eq!(
            tool_message.content[0],
            LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                "call-1",
                "weather",
                LanguageModelToolResultOutput::json(json!({
                    "modelFacing": true,
                    "toolCallId": "call-1",
                    "city": "Brisbane",
                    "forecast": "sunny"
                }))
            ))
        );
    }

    #[test]
    fn generate_text_refines_tool_input_before_execution_results_and_continuation() {
        let model = ToolLoopLanguageModel::with_tool_call("weather", r#"{ "city": " Brisbane " }"#);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_tool_input_refinement("weather", |mut input| async move {
                    let city = input
                        .get("city")
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if let Some(object) = input.as_object_mut() {
                        object.insert("city".to_string(), JsonValue::String(city));
                    }

                    Ok(input)
                })
                .with_max_steps(2),
        ));

        assert_eq!(result.tool_calls[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_results[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_results[0].output["city"], "Brisbane");

        let calls = model.calls.borrow();
        let LanguageModelMessage::Assistant(assistant_message) = &calls[1].prompt[1] else {
            panic!("second prompt includes assistant response");
        };
        assert_eq!(
            assistant_message.content[0],
            LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                "call-1",
                "weather",
                json!({ "city": "Brisbane" })
            ))
        );
        let LanguageModelMessage::Tool(tool_message) = &calls[1].prompt[2] else {
            panic!("second prompt includes tool response");
        };
        assert_eq!(
            tool_message.content[0],
            LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                "call-1",
                "weather",
                LanguageModelToolResultOutput::json(json!({
                    "city": "Brisbane",
                    "forecast": "sunny"
                }))
            ))
        );
    }

    #[test]
    fn generate_text_turns_failed_tool_input_refinement_into_invalid_tool_result() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let executed = Arc::new(AtomicBool::new(false));
        let executed_clone = Arc::clone(&executed);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let executed = Arc::clone(&executed_clone);
                        async move {
                            executed.store(true, Ordering::SeqCst);
                            Ok(json!("should not run"))
                        }
                    },
                ))
                .with_tool_input_refinement("weather", |_input| async move {
                    Err::<JsonValue, ToolInputRefinementError>(ToolInputRefinementError::new(
                        "city cannot be refined",
                    ))
                })
                .with_max_steps(2),
        ));

        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(result.tool_calls[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_calls[0].dynamic, Some(true));
        assert_eq!(result.tool_calls[0].invalid, Some(true));
        assert_eq!(
            result.tool_calls[0].error.as_deref(),
            Some("city cannot be refined")
        );
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(
            result.tool_results[0].output,
            json!("city cannot be refined")
        );

        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 2);
        let LanguageModelMessage::Tool(tool_message) = &calls[1].prompt[2] else {
            panic!("second prompt includes tool response");
        };
        assert_eq!(
            tool_message.content[0],
            LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                "call-1",
                "weather",
                LanguageModelToolResultOutput::error_text("city cannot be refined")
            ))
        );
    }

    #[test]
    fn generate_text_repairs_invalid_tool_call_before_execution() {
        let model = ToolLoopLanguageModel::with_tool_call("weather", "invalid json");
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let repair_options = Arc::new(Mutex::new(Vec::<ToolCallRepairOptions>::new()));
        let repair_options_for_closure = Arc::clone(&repair_options);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema.clone()).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_tool_call_repair(move |options| {
                    let repair_options = Arc::clone(&repair_options_for_closure);
                    let input_schema = input_schema.clone();
                    async move {
                        assert_eq!(options.input_schema("weather"), Some(&input_schema));
                        repair_options
                            .lock()
                            .expect("repair options lock")
                            .push(options);
                        Ok::<Option<LanguageModelToolCall>, String>(Some(
                            LanguageModelToolCall::new(
                                "call-1",
                                "weather",
                                r#"{"city":"Brisbane"}"#,
                            ),
                        ))
                    }
                })
                .with_max_steps(2),
        ));

        let repair_options = repair_options.lock().expect("repair options lock");
        assert_eq!(repair_options.len(), 1);
        assert_eq!(repair_options[0].tool_call.tool_name, "weather");
        assert_eq!(repair_options[0].tool_call.input, "invalid json");
        assert_eq!(repair_options[0].messages, vec![user_message("Weather?")]);
        assert!(matches!(
            &repair_options[0].error,
            ToolCallRepairOriginalError::InvalidToolInput(error)
                if error.tool_name() == "weather" && error.tool_input() == "invalid json"
        ));
        drop(repair_options);

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(result.tool_calls[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_calls[0].invalid, None);
        assert_eq!(result.tool_calls[0].error, None);
        assert_eq!(result.tool_results[0].output["city"], "Brisbane");
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_repairs_unknown_tool_name_before_execution() {
        let model = ToolLoopLanguageModel::with_tool_call("forecast", "{}");
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let repair_options = Arc::new(Mutex::new(Vec::<ToolCallRepairOptions>::new()));
        let repair_options_for_closure = Arc::clone(&repair_options);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "calledWith": input,
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_tool_call_repair(move |options| {
                    let repair_options = Arc::clone(&repair_options_for_closure);
                    async move {
                        repair_options
                            .lock()
                            .expect("repair options lock")
                            .push(options);
                        Ok::<Option<LanguageModelToolCall>, String>(Some(
                            LanguageModelToolCall::new("call-1", "weather", "{}"),
                        ))
                    }
                })
                .with_max_steps(2),
        ));

        let repair_options = repair_options.lock().expect("repair options lock");
        assert_eq!(repair_options.len(), 1);
        assert!(matches!(
            &repair_options[0].error,
            ToolCallRepairOriginalError::NoSuchTool(error)
                if error.tool_name() == "forecast"
                    && error.available_tools() == Some(&["weather".to_string()][..])
        ));
        drop(repair_options);

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(result.tool_calls[0].tool_name, "weather");
        assert_eq!(result.tool_calls[0].input, json!({}));
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
    }

    #[test]
    fn generate_text_turns_failed_tool_call_repair_into_invalid_tool_result() {
        let model = ToolLoopLanguageModel::with_tool_call("weather", "invalid json");
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_closure = Arc::clone(&executed);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let executed = Arc::clone(&executed_for_closure);
                        async move {
                            executed.store(true, Ordering::SeqCst);
                            Ok(json!("should not run"))
                        }
                    },
                ))
                .with_tool_call_repair(|_options| async move {
                    Err::<Option<LanguageModelToolCall>, _>("repair failed")
                })
                .with_max_steps(2),
        ));

        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(result.tool_calls[0].invalid, Some(true));
        assert_eq!(
            result.tool_calls[0].error.as_deref(),
            Some("Error repairing tool call: repair failed")
        );
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(
            result.tool_results[0].output,
            json!("Error repairing tool call: repair failed")
        );
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_auto_approves_tool_calls_and_executes_tools() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_tool_approval(ToolApprovalConfiguration::new().with_tool_status(
                    "weather",
                    NormalizedToolApprovalStatus::approved_with_reason("trusted tool"),
                ))
                .with_max_steps(2),
        ));

        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 2);
        let LanguageModelMessage::Assistant(assistant_message) = &calls[1].prompt[1] else {
            panic!("second prompt includes assistant response");
        };
        let LanguageModelAssistantContentPart::ToolApprovalRequest(approval_request) =
            &assistant_message.content[1]
        else {
            panic!("assistant response includes automatic approval request");
        };
        assert_eq!(approval_request.tool_call_id, "call-1");
        assert_eq!(approval_request.is_automatic, Some(true));

        let LanguageModelMessage::Tool(tool_message) = &calls[1].prompt[2] else {
            panic!("second prompt includes tool response");
        };
        assert_eq!(
            tool_message.content[0],
            LanguageModelToolContentPart::ToolApprovalResponse(
                LanguageModelToolApprovalResponsePart::new(
                    approval_request.approval_id.clone(),
                    true
                )
                .with_reason("trusted tool")
            )
        );
        assert_eq!(
            tool_message.content[1],
            LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                "call-1",
                "weather",
                LanguageModelToolResultOutput::text("sunny")
            ))
        );
        assert!(matches!(
            &result.steps[0].content[..],
            [
                GenerateTextContentPart::ToolCall(_),
                GenerateTextContentPart::ToolApprovalRequest(_),
                GenerateTextContentPart::ToolApprovalResponse(response),
                GenerateTextContentPart::ToolResult(_),
            ] if response.approved
                && response.reason.as_deref() == Some("trusted tool")
                && response.tool_call.tool_call_id == "call-1"
        ));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_uses_per_tool_approval_callback_before_tool_execution() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let callback_invoked = Arc::new(AtomicBool::new(false));
        let callback_invoked_for_closure = Arc::clone(&callback_invoked);
        let runtime_context = JsonObject::from_iter([("requestId".to_string(), json!("req-1"))]);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_tool_context("weather", json!({ "risk": "low" }))
                .with_runtime_context(runtime_context.clone())
                .with_tool_approval(
                    ToolApprovalConfiguration::new().with_tool_approval_function(
                        "weather",
                        move |input, options| {
                            let callback_invoked = Arc::clone(&callback_invoked_for_closure);
                            async move {
                                callback_invoked.store(true, Ordering::SeqCst);
                                assert_eq!(input["city"], json!("Brisbane"));
                                assert_eq!(options.messages.len(), 1);
                                assert_eq!(
                                    options.tool_context.as_ref().expect("tool context"),
                                    &json!({ "risk": "low" })
                                );
                                assert_eq!(options.runtime_context["requestId"], json!("req-1"));
                                Some(
                                    NormalizedToolApprovalStatus::approved_with_reason(
                                        "callback approved",
                                    )
                                    .into(),
                                )
                            }
                        },
                    ),
                )
                .with_max_steps(2),
        ));

        assert!(callback_invoked.load(Ordering::SeqCst));
        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output, json!("sunny"));
        assert!(matches!(
            &result.steps[0].content[..],
            [
                GenerateTextContentPart::ToolCall(_),
                GenerateTextContentPart::ToolApprovalRequest(_),
                GenerateTextContentPart::ToolApprovalResponse(response),
                GenerateTextContentPart::ToolResult(_),
            ] if response.approved
                && response.reason.as_deref() == Some("callback approved")
        ));
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_auto_denies_tool_calls_without_executing_tools() {
        let model = ToolLoopLanguageModel::new();
        let tool_executed = Arc::new(AtomicBool::new(false));
        let tool_executed_for_closure = Arc::clone(&tool_executed);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let tool_executed = Arc::clone(&tool_executed_for_closure);
                        async move {
                            tool_executed.store(true, Ordering::SeqCst);
                            Ok(json!("should not run"))
                        }
                    },
                ))
                .with_tool_approval(ToolApprovalConfiguration::new().with_tool_status(
                    "weather",
                    NormalizedToolApprovalStatus::denied_with_reason("policy block"),
                ))
                .with_max_steps(2),
        ));

        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert!(!tool_executed.load(Ordering::SeqCst));
        assert!(result.tool_results.is_empty());

        let LanguageModelMessage::Assistant(assistant_message) = &calls[1].prompt[1] else {
            panic!("second prompt includes assistant response");
        };
        let LanguageModelAssistantContentPart::ToolApprovalRequest(approval_request) =
            &assistant_message.content[1]
        else {
            panic!("assistant response includes automatic approval request");
        };
        assert_eq!(approval_request.is_automatic, Some(true));

        let LanguageModelMessage::Tool(tool_message) = &calls[1].prompt[2] else {
            panic!("second prompt includes tool response");
        };
        assert_eq!(
            tool_message.content,
            vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new(
                        approval_request.approval_id.clone(),
                        false,
                    )
                    .with_reason("policy block"),
                ),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::execution_denied().with_reason("policy block"),
                )),
            ]
        );
        assert!(matches!(
            &result.steps[0].content[..],
            [
                GenerateTextContentPart::ToolCall(_),
                GenerateTextContentPart::ToolApprovalRequest(_),
                GenerateTextContentPart::ToolApprovalResponse(response),
            ] if !response.approved
                && response.reason.as_deref() == Some("policy block")
                && response.tool_call.tool_call_id == "call-1"
        ));
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_user_approval_blocks_tool_execution_until_response() {
        let model = ToolLoopLanguageModel::new();
        let tool_executed = Arc::new(AtomicBool::new(false));
        let tool_executed_for_closure = Arc::clone(&tool_executed);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let tool_executed = Arc::clone(&tool_executed_for_closure);
                        async move {
                            tool_executed.store(true, Ordering::SeqCst);
                            Ok(json!("should not run"))
                        }
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("weather", ToolApprovalStatusKind::UserApproval),
                )
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 1);
        assert!(!tool_executed.load(Ordering::SeqCst));
        assert!(result.tool_results.is_empty());
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.response_messages.len(), 1);

        let LanguageModelMessage::Assistant(assistant_message) = &result.response_messages[0]
        else {
            panic!("response messages include assistant approval request");
        };
        assert!(matches!(
            assistant_message.content[1],
            LanguageModelAssistantContentPart::ToolApprovalRequest(_)
        ));
    }

    #[test]
    fn generate_text_executes_initial_approved_tool_approval_before_first_model_call() {
        let model = FakeLanguageModel::new();
        let prompt = approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("approval-1", true),
            false,
        );
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let start_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionStartEvent>::new()));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let events_for_start = Arc::clone(&events);
        let events_for_end = Arc::clone(&events);
        let start_events_for_callback = Arc::clone(&start_events);
        let end_events_for_callback = Arc::clone(&end_events);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone())
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, options| async move {
                        Ok(json!({
                            "forecast": "sunny",
                            "city": input["city"],
                            "toolCallId": options.tool_call_id
                        }))
                    },
                ))
                .with_on_tool_execution_start(move |event| {
                    let events = Arc::clone(&events_for_start);
                    let start_events = Arc::clone(&start_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push("start");
                        start_events.lock().expect("start events lock").push(event);
                    }
                })
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&events_for_end);
                    let end_events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push("end");
                        end_events.lock().expect("end events lock").push(event);
                    }
                }),
        ));

        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0].prompt[..3], prompt.as_slice());
        assert_eq!(
            calls[0].prompt[3],
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::json(json!({
                        "forecast": "sunny",
                        "city": "Brisbane",
                        "toolCallId": "call-1"
                    }))
                ))
            ]))
        );
        assert!(result.tool_results.is_empty());
        assert_eq!(result.text, "Hello world");
        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            ["start", "end"]
        );
        assert_eq!(
            start_events.lock().expect("start events lock")[0].messages,
            prompt
        );
        assert_eq!(
            end_events.lock().expect("end events lock")[0]
                .tool_output
                .output["forecast"],
            json!("sunny")
        );
    }

    #[test]
    fn generate_text_turns_initial_denied_provider_approval_into_execution_denied_result() {
        let model = FakeLanguageModel::new();
        let tool_executed = Arc::new(AtomicBool::new(false));
        let tool_executed_for_closure = Arc::clone(&tool_executed);
        let prompt = approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("approval-1", false)
                .with_reason("policy block"),
            true,
        );
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone()).with_tool(
                Tool::new("weather", input_schema).with_execute(move |_input, _options| {
                    let tool_executed = Arc::clone(&tool_executed_for_closure);
                    async move {
                        tool_executed.store(true, Ordering::SeqCst);
                        Ok(json!("should not run"))
                    }
                }),
            ),
        ));

        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(!tool_executed.load(Ordering::SeqCst));
        assert_eq!(&calls[0].prompt[..3], prompt.as_slice());
        assert_eq!(
            serde_json::to_value(&calls[0].prompt[3]).expect("message serializes"),
            json!({
                "role": "tool",
                "content": [
                    {
                        "type": "tool-result",
                        "toolCallId": "call-1",
                        "toolName": "weather",
                        "output": {
                            "type": "execution-denied",
                            "reason": "policy block",
                            "providerOptions": {
                                "openai": {
                                    "approvalId": "approval-1"
                                }
                            }
                        }
                    }
                ]
            })
        );
        assert!(result.tool_results.is_empty());
        assert_eq!(result.text, "Hello world");
    }

    #[test]
    fn generate_text_propagates_runtime_and_tools_context_to_steps_and_tool_execution() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let runtime_context = json!({
            "requestId": "req-1"
        })
        .as_object()
        .expect("runtime context is an object")
        .clone();
        let tools_context = json!({
            "weather": {
                "apiKey": "secret"
            }
        })
        .as_object()
        .expect("tools context is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_runtime_context(runtime_context.clone())
                .with_tools_context(tools_context.clone())
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, options| async move {
                        Ok(json!({
                            "toolCallId": options.tool_call_id,
                            "context": options.context
                        }))
                    },
                ))
                .with_max_steps(2),
        ));

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.steps[0].runtime_context, runtime_context);
        assert_eq!(result.steps[1].runtime_context, runtime_context);
        assert_eq!(result.steps[0].tools_context, tools_context);
        assert_eq!(result.steps[1].tools_context, tools_context);
        assert_eq!(
            result.tool_results[0].output["context"],
            json!({
                "apiKey": "secret"
            })
        );
        assert_eq!(
            serde_json::to_value(&result.steps[0]).expect("step serializes")["runtimeContext"],
            json!({
                "requestId": "req-1"
            })
        );
        assert_eq!(
            serde_json::to_value(&result.steps[0]).expect("step serializes")["toolsContext"],
            json!({
                "weather": {
                    "apiKey": "secret"
                }
            })
        );
    }

    #[test]
    fn generate_text_passes_experimental_sandbox_to_prepare_step_and_tool_execution() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let default_sandbox: Arc<dyn ExperimentalSandbox> =
            Arc::new(TestSandbox::new("default sandbox"));
        let step_sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("step sandbox"));
        let prepare_sandbox_descriptions = Arc::new(Mutex::new(Vec::new()));
        let prepare_sandbox_descriptions_clone = Arc::clone(&prepare_sandbox_descriptions);
        let step_sandbox_clone = Arc::clone(&step_sandbox);

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_experimental_sandbox(Arc::clone(&default_sandbox))
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, options| async move {
                        let sandbox = options
                            .experimental_sandbox
                            .expect("sandbox is passed to tool execution");
                        let command_result =
                            sandbox.run_command(SandboxCommandOptions::new("pwd")).await;

                        Ok(json!({
                            "description": sandbox.description(),
                            "stdout": command_result.stdout
                        }))
                    },
                ))
                .with_prepare_step(move |options| {
                    let descriptions = Arc::clone(&prepare_sandbox_descriptions_clone);
                    let step_sandbox = Arc::clone(&step_sandbox_clone);

                    async move {
                        descriptions.lock().expect("lock succeeds").push(
                            options
                                .experimental_sandbox
                                .as_ref()
                                .map(|sandbox| sandbox.description().to_string()),
                        );

                        if options.step_number == 0 {
                            PrepareStepResult::new().with_experimental_sandbox(step_sandbox)
                        } else {
                            PrepareStepResult::new()
                        }
                    }
                })
                .with_max_steps(2),
        ));

        assert_eq!(
            *prepare_sandbox_descriptions.lock().expect("lock succeeds"),
            vec![
                Some("default sandbox".to_string()),
                Some("default sandbox".to_string())
            ]
        );
        assert_eq!(result.tool_results[0].output["description"], "step sandbox");
        assert_eq!(result.tool_results[0].output["stdout"], "pwd");
    }

    #[test]
    fn generate_text_resolves_dynamic_tool_descriptions_with_step_context_and_sandbox() {
        let model = FakeLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let step_sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("step shell"));
        let step_sandbox_clone = Arc::clone(&step_sandbox);

        let _ = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema.clone()).with_dynamic_description(
                        |options| {
                            let region = options
                                .context
                                .as_ref()
                                .and_then(|context| context.get("region"))
                                .and_then(JsonValue::as_str)
                                .unwrap_or("missing");
                            let sandbox = options
                                .experimental_sandbox
                                .as_ref()
                                .map(|sandbox| sandbox.description())
                                .unwrap_or("no sandbox");

                            format!("Look up weather for {region} using {sandbox}.")
                        },
                    ),
                )
                .with_prepare_step(move |_| {
                    let step_sandbox = Arc::clone(&step_sandbox_clone);

                    async move {
                        let mut weather_context = JsonObject::new();
                        weather_context
                            .insert("region".to_string(), JsonValue::String("Brisbane".into()));

                        let mut tools_context = JsonObject::new();
                        tools_context
                            .insert("weather".to_string(), JsonValue::Object(weather_context));

                        PrepareStepResult::new()
                            .with_tools_context(tools_context)
                            .with_experimental_sandbox(step_sandbox)
                    }
                }),
        ));

        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", input_schema)
                    .with_description("Look up weather for Brisbane using step shell.")
            )])
        );
    }

    #[test]
    fn generate_text_validates_tool_context_schema_before_execution() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let context_schema = Schema::new(
            json!({
                "type": "object",
                "properties": {
                    "apiKey": { "type": "string" }
                },
                "required": ["apiKey"]
            })
            .as_object()
            .expect("context schema is an object")
            .clone(),
        )
        .with_validator(|value| {
            let Some(api_key) = value.get("apiKey").and_then(JsonValue::as_str) else {
                return ValidationResult::failure("expected apiKey string");
            };

            ValidationResult::success(json!({
                "apiKey": api_key,
                "region": "ap-southeast-2"
            }))
        });
        let tools_context = json!({
            "weather": {
                "apiKey": "secret"
            }
        })
        .as_object()
        .expect("tools context is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_context_schema(context_schema)
                        .with_execute(|_input, options| async move {
                            Ok(json!({
                                "context": options.context
                            }))
                        }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(
            result.tool_results[0].output["context"],
            json!({
                "apiKey": "secret",
                "region": "ap-southeast-2"
            })
        );
    }

    #[test]
    fn generate_text_returns_tool_error_when_tool_context_schema_fails() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let context_schema = Schema::new(
            json!({
                "type": "object",
                "properties": {
                    "apiKey": { "type": "string" }
                },
                "required": ["apiKey"]
            })
            .as_object()
            .expect("context schema is an object")
            .clone(),
        )
        .with_validator(|value| {
            if value.get("apiKey").and_then(JsonValue::as_str).is_some() {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("expected apiKey string")
            }
        });
        let executed = Arc::new(AtomicBool::new(false));
        let executed_clone = Arc::clone(&executed);
        let tools_context = json!({
            "weather": {
                "apiKey": 123
            }
        })
        .as_object()
        .expect("tools context is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_context_schema(context_schema)
                        .with_execute(move |_input, _options| {
                            let executed = Arc::clone(&executed_clone);
                            async move {
                                executed.store(true, Ordering::SeqCst);
                                Ok(json!("should not run"))
                            }
                        }),
                )
                .with_max_steps(2),
        ));

        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(
            result.tool_results[0].output.as_str(),
            Some(
                "Type validation failed for tool context (weather): Value: {\"apiKey\":123}.\nError message: expected apiKey string"
            )
        );
        assert_eq!(model.calls.borrow().len(), 2);
    }

    #[test]
    fn generate_text_marks_runtime_dynamic_tool_calls_and_results() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(dynamic_tool("weather", input_schema.clone()).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "forecast": "sunny",
                            "city": input["city"]
                        }))
                    },
                ))
                .with_max_steps(2),
        ));

        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", input_schema)
            )])
        );
        assert_eq!(result.tool_calls[0].dynamic, Some(true));
        assert_eq!(result.tool_results[0].dynamic, Some(true));
        assert_eq!(result.static_tool_calls, Vec::new());
        assert_eq!(result.static_tool_results, Vec::new());
        assert_eq!(result.dynamic_tool_calls.len(), 1);
        assert_eq!(result.dynamic_tool_results.len(), 1);
    }

    #[test]
    fn generate_text_propagates_tool_metadata_to_tool_calls_and_results() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let tool_metadata = json!({
            "source": "mcp",
            "server": "weather-tools"
        })
        .as_object()
        .expect("metadata is an object")
        .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema.clone())
                        .with_title("Weather information")
                        .with_metadata(tool_metadata.clone())
                        .with_execute(|input, _options| async move {
                            Ok(json!({
                                "forecast": "sunny",
                                "city": input["city"]
                            }))
                        }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", input_schema)
            )])
        );
        assert_eq!(
            result.tool_calls[0].tool_metadata,
            Some(tool_metadata.clone())
        );
        assert_eq!(
            result.tool_calls[0].title.as_deref(),
            Some("Weather information")
        );
        assert_eq!(
            result.tool_results[0].tool_metadata,
            Some(tool_metadata.clone())
        );
        assert_eq!(
            serde_json::to_value(&result.tool_calls[0]).expect("tool call serializes")["toolMetadata"],
            json!({
                "source": "mcp",
                "server": "weather-tools"
            })
        );
        assert_eq!(
            serde_json::to_value(&result.tool_calls[0]).expect("tool call serializes")["title"],
            json!("Weather information")
        );
        assert_eq!(
            serde_json::to_value(&result.tool_results[0]).expect("tool result serializes")["toolMetadata"],
            json!({
                "source": "mcp",
                "server": "weather-tools"
            })
        );
    }

    #[test]
    fn generate_text_records_tool_execution_errors_and_continues() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, _options| async move {
                        Err::<serde_json::Value, ToolExecutionError>(ToolExecutionError::new(
                            "weather service timed out",
                        ))
                    },
                ))
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].tool_name, "weather");
        assert_eq!(result.tool_results[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(
            result.tool_results[0].output,
            json!("weather service timed out")
        );
        assert_eq!(result.steps[0].call_id, result.steps[1].call_id);
        assert!(result.steps[0].call_id.starts_with("call-"));
        assert_eq!(
            model.calls.borrow()[1].prompt[2],
            LanguageModelMessage::Tool(crate::language_model::LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::error_text("weather service timed out")
                ))
            ]))
        );
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_preserves_tool_result_provider_metadata_in_continuation_message() {
        let provider_metadata: ProviderMetadata =
            serde_json::from_value(json!({ "testProvider": { "signature": "sig" } }))
                .expect("provider metadata deserializes");
        let model = ToolLoopLanguageModel::with_tool_call_metadata(
            "weather",
            r#"{"city":"Brisbane"}"#,
            provider_metadata.clone(),
        );
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(
            result.tool_results[0].provider_metadata,
            Some(provider_metadata.clone())
        );
        assert_eq!(
            model.calls.borrow()[1].prompt[2],
            LanguageModelMessage::Tool(crate::language_model::LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(
                    LanguageModelToolResultPart::new(
                        "call-1",
                        "weather",
                        LanguageModelToolResultOutput::text("sunny")
                    )
                    .with_provider_options(provider_metadata)
                )
            ]))
        );
    }

    #[test]
    fn generate_text_converts_provider_executed_tool_results_for_continuation_messages() {
        let model = ToolLoopLanguageModel::with_first_step_prefix(
            "weather",
            r#"{"city":"Brisbane"}"#,
            vec![
                LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new("provider-call-1", "providerSearch", "{}")
                        .with_provider_executed(true)
                        .with_dynamic(true),
                ),
                LanguageModelContent::ToolResult(LanguageModelToolResult::new(
                    "provider-call-1",
                    "providerSearch",
                    crate::NonNullJsonValue::new(json!("done"))
                        .expect("provider result is non-null"),
                )),
                LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new("provider-call-2", "providerCode", "{}")
                        .with_provider_executed(true)
                        .with_dynamic(true),
                ),
                LanguageModelContent::ToolResult(
                    LanguageModelToolResult::new(
                        "provider-call-2",
                        "providerCode",
                        crate::NonNullJsonValue::new(json!("failed"))
                            .expect("provider error is non-null"),
                    )
                    .with_is_error(true),
                ),
            ],
        );
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let _ = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(
            model.calls.borrow()[1].prompt[1],
            LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::ToolCall(
                        LanguageModelToolCallPart::new(
                            "provider-call-1",
                            "providerSearch",
                            json!({})
                        )
                        .with_provider_executed(true)
                    ),
                    LanguageModelAssistantContentPart::ToolResult(
                        LanguageModelToolResultPart::new(
                            "provider-call-1",
                            "providerSearch",
                            LanguageModelToolResultOutput::text("done")
                        )
                    ),
                    LanguageModelAssistantContentPart::ToolCall(
                        LanguageModelToolCallPart::new(
                            "provider-call-2",
                            "providerCode",
                            json!({})
                        )
                        .with_provider_executed(true)
                    ),
                    LanguageModelAssistantContentPart::ToolResult(
                        LanguageModelToolResultPart::new(
                            "provider-call-2",
                            "providerCode",
                            LanguageModelToolResultOutput::error_json(json!("failed"))
                        )
                    ),
                    LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                        "call-1",
                        "weather",
                        json!({ "city": "Brisbane" })
                    ))
                ])
            )
        );
    }

    struct ProviderExecutedToolLanguageModel {
        calls: RefCell<Vec<LanguageModelCallOptions>>,
    }

    impl ProviderExecutedToolLanguageModel {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl LanguageModel for ProviderExecutedToolLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
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
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls.borrow_mut().push(options);

            ready(LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::ToolCall(
                        LanguageModelToolCall::new(
                            "provider-call-1",
                            "providerTool",
                            r#"{"city":"Brisbane"}"#,
                        )
                        .with_provider_executed(true)
                        .with_dynamic(true),
                    ),
                    LanguageModelContent::ToolResult(
                        LanguageModelToolResult::new(
                            "provider-call-1",
                            "providerTool",
                            crate::NonNullJsonValue::new(json!({
                                "forecast": "sunny"
                            }))
                            .expect("provider tool result is non-null"),
                        )
                        .with_dynamic(true),
                    ),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    struct DeferredProviderExecutedToolLanguageModel {
        calls: RefCell<Vec<LanguageModelCallOptions>>,
    }

    impl DeferredProviderExecutedToolLanguageModel {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl LanguageModel for DeferredProviderExecutedToolLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
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
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            let step_number = self.calls.borrow().len();
            self.calls.borrow_mut().push(options);

            if step_number == 0 {
                return ready(LanguageModelGenerateResult::new(
                    vec![LanguageModelContent::ToolCall(
                        LanguageModelToolCall::new(
                            "provider-call-1",
                            "providerTool",
                            r#"{"city":"Brisbane"}"#,
                        )
                        .with_provider_executed(true),
                    )],
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: Some("tool_calls".to_string()),
                    },
                    LanguageModelUsage::default(),
                ));
            }

            ready(LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::ToolResult(LanguageModelToolResult::new(
                        "provider-call-1",
                        "providerTool",
                        crate::NonNullJsonValue::new(json!({
                            "forecast": "sunny"
                        }))
                        .expect("provider deferred result is non-null"),
                    )),
                    LanguageModelContent::Text(LanguageModelText::new(
                        "The deferred provider tool result is ready.",
                    )),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    #[test]
    fn generate_text_surfaces_provider_executed_tool_results_without_local_execution() {
        let model = ProviderExecutedToolLanguageModel::new();
        let tool_executed = Arc::new(AtomicBool::new(false));
        let tool_executed_for_closure = Arc::clone(&tool_executed);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("providerTool", input_schema).with_execute(
                    move |_input, _options| {
                        let tool_executed = Arc::clone(&tool_executed_for_closure);
                        async move {
                            tool_executed.store(true, Ordering::SeqCst);
                            Ok(json!("should not execute"))
                        }
                    },
                ))
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 1);
        assert!(!tool_executed.load(Ordering::SeqCst));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_calls[0].dynamic, Some(true));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "provider-call-1");
        assert_eq!(result.tool_results[0].tool_name, "providerTool");
        assert_eq!(result.tool_results[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(
            result.tool_results[0].output,
            json!({ "forecast": "sunny" })
        );
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
        assert_eq!(result.tool_results[0].dynamic, Some(true));
    }

    #[test]
    fn generate_text_refines_provider_executed_tool_result_inputs() {
        let model = ProviderExecutedToolLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("providerTool", input_schema))
                .with_tool_input_refinement("providerTool", |mut input| async move {
                    if let Some(object) = input.as_object_mut() {
                        object.insert("refined".to_string(), json!(true));
                    }

                    Ok(input)
                }),
        ));

        assert_eq!(
            result.tool_calls[0].input,
            json!({
                "city": "Brisbane",
                "refined": true
            })
        );
        assert_eq!(
            result.tool_results[0].input,
            json!({
                "city": "Brisbane",
                "refined": true
            })
        );
        assert_eq!(model.calls.borrow().len(), 1);
    }

    #[test]
    fn generate_text_continues_for_deferred_provider_executed_tool_results() {
        let model = DeferredProviderExecutedToolLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let output_schema = input_schema.clone();
        let provider_args = json!({ "mode": "deferred" })
            .as_object()
            .expect("provider args are an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::provider_executed(
                        "providerTool",
                        "test.providerTool",
                        provider_args,
                        input_schema,
                        output_schema,
                    )
                    .with_supports_deferred_results(true),
                )
                .with_max_steps(3),
        ));

        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].prompt.len(), 2);
        assert_eq!(
            calls[1].prompt[1],
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "provider-call-1",
                        "providerTool",
                        json!({ "city": "Brisbane" })
                    )
                    .with_provider_executed(true)
                )
            ]))
        );

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "provider-call-1");
        assert_eq!(result.tool_results[0].tool_name, "providerTool");
        assert_eq!(result.tool_results[0].input, json!(null));
        assert_eq!(
            result.tool_results[0].output,
            json!({ "forecast": "sunny" })
        );
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
        assert_eq!(result.text, "The deferred provider tool result is ready.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn generate_text_stops_after_max_steps_even_when_tool_calls_continue() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_max_steps(1),
        ));

        assert_eq!(model.calls.borrow().len(), 1);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
    }

    #[test]
    fn generate_text_stops_after_matching_stop_condition() {
        let model = ToolLoopLanguageModel::new();
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_max_steps(3)
                .with_stop_condition(has_tool_call(["weather"])),
        ));

        assert_eq!(model.calls.borrow().len(), 1);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output, json!("sunny"));
    }

    #[test]
    fn generate_text_reports_invalid_json_tool_input_and_continues() {
        let model = ToolLoopLanguageModel::with_tool_call("weather", "{ invalid json");
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema))
                .with_max_steps(2),
        ));

        let tool_call = &result.tool_calls[0];
        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(tool_call.input, json!("{ invalid json"));
        assert_eq!(tool_call.dynamic, Some(true));
        assert_eq!(tool_call.invalid, Some(true));
        assert!(
            tool_call
                .error
                .as_deref()
                .expect("invalid tool call carries an error")
                .starts_with(
                    "Invalid input for tool weather: JSON parsing failed: Text: { invalid json."
                )
        );

        let tool_result = &result.tool_results[0];
        assert_eq!(tool_result.is_error, Some(true));
        assert_eq!(tool_result.input, json!("{ invalid json"));
        let error_message = tool_result
            .output
            .as_str()
            .expect("error output is a string");
        assert!(error_message.starts_with("Invalid input for tool weather:"));

        assert_eq!(
            model.calls.borrow()[1].prompt[1],
            LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                        "call-1",
                        "weather",
                        json!({})
                    ))
                ])
            )
        );
        assert_eq!(
            model.calls.borrow()[1].prompt[2],
            LanguageModelMessage::Tool(crate::language_model::LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::error_text(error_message)
                ))
            ]))
        );
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_reports_unknown_tool_and_continues_with_error_result() {
        let model = ToolLoopLanguageModel::with_tool_call("forecast", r#"{"city":"Brisbane"}"#);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema))
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(result.tool_calls[0].tool_name, "forecast");
        assert_eq!(result.tool_calls[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_calls[0].dynamic, Some(true));
        assert_eq!(result.tool_calls[0].invalid, Some(true));
        assert_eq!(result.static_tool_calls, Vec::new());
        assert_eq!(result.dynamic_tool_calls.len(), 1);
        assert_eq!(result.dynamic_tool_calls[0].tool_name, "forecast");
        assert_eq!(
            result.tool_calls[0].error.as_deref(),
            Some("Model tried to call unavailable tool 'forecast'. Available tools: weather.")
        );

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_name, "forecast");
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(result.static_tool_results, Vec::new());
        assert_eq!(result.dynamic_tool_results.len(), 1);
        assert_eq!(result.dynamic_tool_results[0].tool_name, "forecast");
        assert_eq!(
            result.tool_results[0].output,
            json!("Model tried to call unavailable tool 'forecast'. Available tools: weather.")
        );
        assert_eq!(result.text, "The weather in Brisbane is sunny.");
    }

    #[test]
    fn generate_text_omits_empty_text_from_continuation_assistant_messages() {
        let model = ToolLoopLanguageModel::with_first_step_prefix(
            "weather",
            r#"{"city":"Brisbane"}"#,
            vec![LanguageModelContent::Text(LanguageModelText::new(""))],
        );
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let _ = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("sunny")) }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(model.calls.borrow().len(), 2);
        assert_eq!(
            model.calls.borrow()[1].prompt[1],
            LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                        "call-1",
                        "weather",
                        json!({ "city": "Brisbane" })
                    ))
                ])
            )
        );
    }
}
