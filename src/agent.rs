use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use crate::chat_transport::{
    ChatTransportError, ConvertUiMessagesToModelMessagesOptions,
    convert_ui_messages_to_model_messages_with_tools,
};
use crate::generate_text::{
    ActiveTools, GenerateTextFinishEvent, GenerateTextInclude, GenerateTextOnFinish,
    GenerateTextOnStart, GenerateTextOnStepFinish, GenerateTextOnStepStart,
    GenerateTextOnToolExecutionEnd, GenerateTextOnToolExecutionStart, GenerateTextOptions,
    GenerateTextResult, GenerateTextStartEvent, GenerateTextStep, GenerateTextStepStartEvent,
    GenerateTextTool, GenerateTextToolExecutionEndEvent, GenerateTextToolExecutionStartEvent,
    PrepareStep, PrepareStepOptions, PrepareStepResult, StopCondition, ToolApprovalConfiguration,
    ToolCallRepair, ToolInputRefinement, generate_text,
};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    LanguageModel, LanguageModelAbortSignal, LanguageModelCallOptions,
    LanguageModelReasoningEffort, LanguageModelResponseFormat, LanguageModelStreamPart,
    LanguageModelToolChoice,
};
use crate::prompt::{Instructions, Prompt, PromptInput, TimeoutConfiguration};
use crate::provider::{InvalidPromptError, ProviderOptions, TypeValidationContext};
use crate::provider_utils::{ExperimentalSandbox, FlexibleSchema, validate_types};
use crate::stream_text::{
    StreamTextOptions, StreamTextResult, StreamTextUiMessageStreamOptions, stream_text,
};
use crate::telemetry::TelemetryOptions;
use crate::ui_message_stream::{UiMessage, UiMessageStreamResponse, UiMessageStreamResponseInit};

/// Upstream version tag for `ToolLoopAgent`.
pub const TOOL_LOOP_AGENT_VERSION: &str = "agent-v1";

/// Agent implementation that delegates each call to `generate_text` or `stream_text`.
///
/// This ports the portable core of upstream `ToolLoopAgent`: shared settings,
/// call preparation, default twenty-step tool loops, non-streaming generation,
/// streaming generation, and Rust-native abort/timeout request controls.
pub struct ToolLoopAgent<'a, M: LanguageModel + ?Sized> {
    settings: ToolLoopAgentSettings<'a, M>,
}

impl<'a, M: LanguageModel + ?Sized> ToolLoopAgent<'a, M> {
    /// Creates an agent from explicit settings.
    pub fn new(settings: ToolLoopAgentSettings<'a, M>) -> Self {
        Self { settings }
    }

    /// Creates an agent for a model with default settings.
    pub fn for_model(model: &'a M) -> Self {
        Self::new(ToolLoopAgentSettings::new(model))
    }

    /// Returns the upstream agent protocol version.
    pub const fn version(&self) -> &'static str {
        TOOL_LOOP_AGENT_VERSION
    }

    /// Returns the optional agent identifier.
    pub fn id(&self) -> Option<&str> {
        self.settings.id.as_deref()
    }

    /// Returns configured high-level tools.
    pub fn tools(&self) -> &[GenerateTextTool] {
        &self.settings.tools
    }

    /// Generates text through the configured tool-loop settings.
    pub async fn generate(
        &self,
        options: impl Into<ToolLoopAgentCallOptions<'a, M>>,
    ) -> Result<GenerateTextResult, InvalidPromptError> {
        let prepared = self.prepare_call(options.into())?;
        let options = generate_options_from_prepared(prepared)?;
        Ok(generate_text(options).await)
    }

    /// Streams text through the configured tool-loop settings.
    pub async fn stream(
        &self,
        options: impl Into<ToolLoopAgentCallOptions<'a, M>>,
    ) -> Result<StreamTextResult, InvalidPromptError>
    where
        M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
    {
        let prepared = self.prepare_call(options.into())?;
        let options = stream_options_from_prepared(prepared)?;
        Ok(stream_text(options).await)
    }

    fn prepare_call(
        &self,
        options: ToolLoopAgentCallOptions<'a, M>,
    ) -> Result<ToolLoopAgentPreparedCall<'a, M>, InvalidPromptError> {
        let call_options = validate_agent_call_options(
            options.call_options,
            self.settings.call_options_schema.as_ref(),
        )?;
        let mut prepared = ToolLoopAgentPreparedCall {
            model: options.model.unwrap_or(self.settings.model),
            prompt: options.prompt,
            call_options,
            instructions: options
                .instructions
                .or_else(|| self.settings.instructions.clone()),
            model_settings: self
                .settings
                .model_settings
                .clone()
                .merge(options.model_settings),
            tools: merged_tools(&self.settings.tools, options.tools),
            runtime_context: merge_json_objects(
                self.settings.runtime_context.clone(),
                options.runtime_context,
            ),
            tools_context: merge_json_objects(
                self.settings.tools_context.clone(),
                options.tools_context,
            ),
            experimental_sandbox: options
                .experimental_sandbox
                .or_else(|| self.settings.experimental_sandbox.clone()),
            active_tools: options
                .active_tools
                .or_else(|| self.settings.active_tools.clone()),
            tool_approval: options
                .tool_approval
                .or_else(|| self.settings.tool_approval.clone()),
            tool_input_refinements: merge_tool_input_refinements(
                self.settings.tool_input_refinements.clone(),
                options.tool_input_refinements,
            ),
            tool_call_repair: options
                .tool_call_repair
                .or_else(|| self.settings.tool_call_repair.clone()),
            prepare_step: options
                .prepare_step
                .or_else(|| self.settings.prepare_step.clone()),
            on_start: merge_on_start(self.settings.on_start.clone(), options.on_start),
            on_step_start: merge_on_step_start(
                self.settings.on_step_start.clone(),
                options.on_step_start,
            ),
            on_tool_execution_start: merge_on_tool_execution_start(
                self.settings.on_tool_execution_start.clone(),
                options.on_tool_execution_start,
            ),
            on_tool_execution_end: merge_on_tool_execution_end(
                self.settings.on_tool_execution_end.clone(),
                options.on_tool_execution_end,
            ),
            on_step_finish: merge_on_step_finish(
                self.settings.on_step_finish.clone(),
                options.on_step_finish,
            ),
            on_finish: merge_on_finish(self.settings.on_finish.clone(), options.on_finish),
            telemetry: options
                .telemetry
                .or_else(|| self.settings.telemetry.clone()),
            abort_signal: options.abort_signal,
            timeout: options.timeout,
            max_steps: options.max_steps.or(self.settings.max_steps).unwrap_or(20),
            stop_conditions: if options.stop_conditions.is_empty() {
                self.settings.stop_conditions.clone()
            } else {
                options.stop_conditions
            },
            include: options.include.or(self.settings.include),
        };

        if let Some(prepare_call) = &self.settings.prepare_call {
            prepared = prepare_call.prepare(prepared);
        }

        Ok(prepared)
    }
}

/// Options for [`create_agent_ui_stream_response`].
pub struct AgentUiStreamResponseOptions<'options, 'agent, M: LanguageModel + ?Sized> {
    /// Agent used to stream the response.
    pub agent: &'options ToolLoopAgent<'agent, M>,

    /// Existing UI messages to convert into the next model prompt.
    pub ui_messages: Vec<UiMessage>,

    /// HTTP response initialization options.
    pub response_init: UiMessageStreamResponseInit,

    /// UI-message stream conversion options.
    pub ui_message_stream_options: StreamTextUiMessageStreamOptions,

    /// Per-call sandbox passed through to local tool execution.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

impl<'options, 'agent, M: LanguageModel + ?Sized>
    AgentUiStreamResponseOptions<'options, 'agent, M>
{
    /// Creates response options for an agent and UI-message history.
    pub fn new(
        agent: &'options ToolLoopAgent<'agent, M>,
        ui_messages: impl IntoIterator<Item = UiMessage>,
    ) -> Self {
        Self {
            agent,
            ui_messages: ui_messages.into_iter().collect(),
            response_init: UiMessageStreamResponseInit::new(),
            ui_message_stream_options: StreamTextUiMessageStreamOptions::default(),
            experimental_sandbox: None,
        }
    }

    /// Sets HTTP response initialization options.
    pub fn with_response_init(mut self, response_init: UiMessageStreamResponseInit) -> Self {
        self.response_init = response_init;
        self
    }

    /// Sets UI-message stream conversion options.
    pub fn with_ui_message_stream_options(
        mut self,
        ui_message_stream_options: StreamTextUiMessageStreamOptions,
    ) -> Self {
        self.ui_message_stream_options = ui_message_stream_options;
        self
    }

    /// Sets a per-call sandbox.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }
}

/// Streams an agent response as upstream-compatible UI-message SSE chunks.
///
/// This ports the portable core of upstream `createAgentUIStreamResponse`.
/// Prior UI messages are converted back into model messages, assistant tool
/// outputs are resolved through matching Rust tool `toModelOutput` callbacks,
/// the agent is streamed in-process, and the collected UI-message chunks are
/// encoded with the standard UI-message stream response headers.
pub async fn create_agent_ui_stream_response<M>(
    options: AgentUiStreamResponseOptions<'_, '_, M>,
) -> Result<UiMessageStreamResponse, ChatTransportError>
where
    M: LanguageModel + ?Sized,
    M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
{
    let AgentUiStreamResponseOptions {
        agent,
        ui_messages,
        response_init,
        mut ui_message_stream_options,
        experimental_sandbox,
    } = options;

    let model_messages = convert_ui_messages_to_model_messages_with_tools(
        &ui_messages,
        ConvertUiMessagesToModelMessagesOptions::default(),
        &agent.settings.tools,
    )
    .await?;

    let mut call_options = ToolLoopAgentCallOptions::from_messages(model_messages);
    if let Some(experimental_sandbox) = experimental_sandbox {
        call_options = call_options.with_experimental_sandbox(experimental_sandbox);
    }

    let result = agent
        .stream(call_options)
        .await
        .map_err(|error| ChatTransportError::Agent(error.to_string()))?;

    if ui_message_stream_options.original_messages.is_none() {
        ui_message_stream_options =
            ui_message_stream_options.with_original_messages(ui_messages.clone());
    }

    Ok(result.to_ui_message_stream_response_with_options(response_init, ui_message_stream_options))
}

/// Shared settings for a [`ToolLoopAgent`].
pub struct ToolLoopAgentSettings<'a, M: LanguageModel + ?Sized> {
    /// Optional agent identifier.
    pub id: Option<String>,

    /// Model used by default for agent calls.
    pub model: &'a M,

    /// Instructions prepended to each call unless the call supplies its own.
    pub instructions: Option<Instructions>,

    /// Default model call settings.
    pub model_settings: ToolLoopAgentModelSettings,

    /// Tools made available to each call.
    pub tools: Vec<GenerateTextTool>,

    /// Runtime context attached to generated steps.
    pub runtime_context: JsonObject,

    /// Tool-specific context keyed by tool name.
    pub tools_context: JsonObject,

    /// Schema used to validate per-call agent options before model invocation.
    pub call_options_schema: Option<FlexibleSchema<JsonValue>>,

    /// Experimental sandbox passed through to local Rust tool execution.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,

    /// Active tool names used to restrict the available tools.
    pub active_tools: ActiveTools,

    /// Tool approval configuration.
    pub tool_approval: Option<ToolApprovalConfiguration>,

    /// Per-tool input refinements.
    pub tool_input_refinements: BTreeMap<String, ToolInputRefinement>,

    /// Optional tool-call repair callback.
    pub tool_call_repair: Option<ToolCallRepair>,

    /// Optional step preparation callback.
    pub prepare_step: Option<PrepareStep<'a, M>>,

    /// Callback invoked before model work begins.
    pub on_start: Option<GenerateTextOnStart<'a>>,

    /// Callback invoked before each model step begins.
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,

    /// Callback invoked before each local Rust tool executor starts.
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,

    /// Callback invoked after each local Rust tool executor completes.
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,

    /// Callback invoked after each completed model step.
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,

    /// Callback invoked after the full call finishes.
    pub on_finish: Option<GenerateTextOnFinish<'a>>,

    /// Telemetry settings.
    pub telemetry: Option<TelemetryOptions>,

    /// Maximum model-call steps. Defaults to upstream's twenty-step agent loop.
    pub max_steps: Option<usize>,

    /// Additional stop conditions.
    pub stop_conditions: Vec<StopCondition>,

    /// Provider payload retention settings for non-streaming calls.
    pub include: Option<GenerateTextInclude>,

    /// Optional call preparation callback.
    pub prepare_call: Option<ToolLoopAgentPrepareCall<'a, M>>,
}

impl<'a, M: LanguageModel + ?Sized> ToolLoopAgentSettings<'a, M> {
    /// Creates default settings for a model.
    pub fn new(model: &'a M) -> Self {
        Self {
            id: None,
            model,
            instructions: None,
            model_settings: ToolLoopAgentModelSettings::default(),
            tools: Vec::new(),
            runtime_context: JsonObject::new(),
            tools_context: JsonObject::new(),
            call_options_schema: None,
            experimental_sandbox: None,
            active_tools: None,
            tool_approval: None,
            tool_input_refinements: BTreeMap::new(),
            tool_call_repair: None,
            prepare_step: None,
            on_start: None,
            on_step_start: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
            max_steps: Some(20),
            stop_conditions: Vec::new(),
            include: None,
            prepare_call: None,
        }
    }

    /// Sets the agent identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets default instructions for every call.
    pub fn with_instructions(mut self, instructions: impl Into<Instructions>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Adds a tool to every call.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        self.tools.push(tool.into());
        self
    }

    /// Sets runtime context for every call.
    pub fn with_runtime_context(mut self, runtime_context: JsonObject) -> Self {
        self.runtime_context = runtime_context;
        self
    }

    /// Sets tool-specific context for every call.
    pub fn with_tools_context(mut self, tools_context: JsonObject) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets the schema used to validate per-call agent options.
    pub fn with_call_options_schema(
        mut self,
        call_options_schema: impl Into<FlexibleSchema<JsonValue>>,
    ) -> Self {
        self.call_options_schema = Some(call_options_schema.into());
        self
    }

    /// Sets the experimental sandbox for every call.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }

    /// Sets active tool names for every call.
    pub fn with_active_tools(
        mut self,
        active_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.active_tools = Some(active_tools.into_iter().map(Into::into).collect());
        self
    }

    /// Sets default tool approval configuration.
    pub fn with_tool_approval(mut self, tool_approval: ToolApprovalConfiguration) -> Self {
        self.tool_approval = Some(tool_approval);
        self
    }

    /// Adds or replaces a default input refinement for one tool.
    pub fn with_tool_input_refinement<F, Fut>(
        mut self,
        tool_name: impl Into<String>,
        refine: F,
    ) -> Self
    where
        F: Fn(JsonValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, crate::generate_text::ToolInputRefinementError>>
            + Send
            + 'static,
    {
        self.tool_input_refinements
            .insert(tool_name.into(), ToolInputRefinement::new(refine));
        self
    }

    /// Sets a default tool-call repair callback.
    pub fn with_tool_call_repair<F, Fut, E>(mut self, repair: F) -> Self
    where
        F: Fn(crate::generate_text::ToolCallRepairOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<crate::language_model::LanguageModelToolCall>, E>>
            + Send
            + 'static,
        E: fmt::Display,
    {
        self.tool_call_repair = Some(ToolCallRepair::new(repair));
        self
    }

    /// Sets a default step preparation callback.
    pub fn with_prepare_step<F, Fut>(mut self, prepare_step: F) -> Self
    where
        F: Fn(PrepareStepOptions<'a, M>) -> Fut + 'a,
        Fut: Future<Output = PrepareStepResult<'a, M>> + 'a,
    {
        self.prepare_step = Some(PrepareStep::new(prepare_step));
        self
    }

    /// Sets a default start callback.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(GenerateTextStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(GenerateTextOnStart::new(on_start));
        self
    }

    /// Sets a default step-start callback.
    pub fn with_on_step_start<F, Fut>(mut self, on_step_start: F) -> Self
    where
        F: Fn(GenerateTextStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_start = Some(GenerateTextOnStepStart::new(on_step_start));
        self
    }

    /// Sets a default tool-execution start callback.
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

    /// Sets a default tool-execution end callback.
    pub fn with_on_tool_execution_end<F, Fut>(mut self, on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_tool_execution_end =
            Some(GenerateTextOnToolExecutionEnd::new(on_tool_execution_end));
        self
    }

    /// Sets a default step-finish callback.
    pub fn with_on_step_finish<F, Fut>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(GenerateTextStep) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_finish = Some(GenerateTextOnStepFinish::new(on_step_finish));
        self
    }

    /// Sets a default finish callback.
    pub fn with_on_finish<F, Fut>(mut self, on_finish: F) -> Self
    where
        F: Fn(GenerateTextFinishEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_finish = Some(GenerateTextOnFinish::new(on_finish));
        self
    }

    /// Sets default telemetry.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Sets the default maximum number of model-call steps.
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = Some(max_steps.max(1));
        self
    }

    /// Adds a default stop condition.
    pub fn with_stop_condition(mut self, stop_condition: StopCondition) -> Self {
        self.stop_conditions.push(stop_condition);
        self
    }

    /// Sets non-streaming provider payload retention.
    pub fn with_include(mut self, include: GenerateTextInclude) -> Self {
        self.include = Some(include);
        self
    }

    /// Sets the default model call settings.
    pub fn with_model_settings(mut self, model_settings: ToolLoopAgentModelSettings) -> Self {
        self.model_settings = model_settings;
        self
    }

    /// Sets a call preparation callback.
    pub fn with_prepare_call<F>(mut self, prepare_call: F) -> Self
    where
        F: Fn(ToolLoopAgentPreparedCall<'a, M>) -> ToolLoopAgentPreparedCall<'a, M> + 'a,
    {
        self.prepare_call = Some(ToolLoopAgentPrepareCall::new(prepare_call));
        self
    }
}

/// Model-call settings shared by agent generation and streaming.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolLoopAgentModelSettings {
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub stop_sequences: Option<Vec<String>>,
    pub top_p: Option<f64>,
    pub top_k: Option<u64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub response_format: Option<LanguageModelResponseFormat>,
    pub seed: Option<u64>,
    pub tool_choice: Option<LanguageModelToolChoice>,
    pub include_raw_chunks: Option<bool>,
    pub headers: Option<Headers>,
    pub reasoning: Option<LanguageModelReasoningEffort>,
    pub provider_options: Option<ProviderOptions>,
}

impl ToolLoopAgentModelSettings {
    /// Creates empty model call settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets temperature.
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Adds a stop sequence.
    pub fn with_stop_sequence(mut self, stop_sequence: impl Into<String>) -> Self {
        self.stop_sequences
            .get_or_insert_with(Vec::new)
            .push(stop_sequence.into());
        self
    }

    /// Sets top-p.
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Sets top-k.
    pub fn with_top_k(mut self, top_k: u64) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets presence penalty.
    pub fn with_presence_penalty(mut self, presence_penalty: f64) -> Self {
        self.presence_penalty = Some(presence_penalty);
        self
    }

    /// Sets frequency penalty.
    pub fn with_frequency_penalty(mut self, frequency_penalty: f64) -> Self {
        self.frequency_penalty = Some(frequency_penalty);
        self
    }

    /// Sets response format.
    pub fn with_response_format(mut self, response_format: LanguageModelResponseFormat) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Sets seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets tool choice.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Sets raw chunk inclusion.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.include_raw_chunks = Some(include_raw_chunks);
        self
    }

    /// Sets headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds one header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets reasoning effort.
    pub fn with_reasoning(mut self, reasoning: LanguageModelReasoningEffort) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    /// Sets provider options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    fn merge(mut self, other: Self) -> Self {
        if other.max_output_tokens.is_some() {
            self.max_output_tokens = other.max_output_tokens;
        }
        if other.temperature.is_some() {
            self.temperature = other.temperature;
        }
        if other.stop_sequences.is_some() {
            self.stop_sequences = other.stop_sequences;
        }
        if other.top_p.is_some() {
            self.top_p = other.top_p;
        }
        if other.top_k.is_some() {
            self.top_k = other.top_k;
        }
        if other.presence_penalty.is_some() {
            self.presence_penalty = other.presence_penalty;
        }
        if other.frequency_penalty.is_some() {
            self.frequency_penalty = other.frequency_penalty;
        }
        if other.response_format.is_some() {
            self.response_format = other.response_format;
        }
        if other.seed.is_some() {
            self.seed = other.seed;
        }
        if other.tool_choice.is_some() {
            self.tool_choice = other.tool_choice;
        }
        if other.include_raw_chunks.is_some() {
            self.include_raw_chunks = other.include_raw_chunks;
        }
        if other.headers.is_some() {
            self.headers = other.headers;
        }
        if other.reasoning.is_some() {
            self.reasoning = other.reasoning;
        }
        if other.provider_options.is_some() {
            self.provider_options = other.provider_options;
        }
        self
    }

    fn apply_to_call_options(&self, call_options: &mut LanguageModelCallOptions) {
        call_options.max_output_tokens = self.max_output_tokens;
        call_options.temperature = self.temperature;
        call_options.stop_sequences = self.stop_sequences.clone();
        call_options.top_p = self.top_p;
        call_options.top_k = self.top_k;
        call_options.presence_penalty = self.presence_penalty;
        call_options.frequency_penalty = self.frequency_penalty;
        call_options.response_format = self.response_format.clone();
        call_options.seed = self.seed;
        call_options.tool_choice = self.tool_choice.clone();
        call_options.include_raw_chunks = self.include_raw_chunks;
        call_options.headers = self.headers.clone();
        call_options.reasoning = self.reasoning.clone();
        call_options.provider_options = self.provider_options.clone();
    }
}

/// Per-call agent options.
pub struct ToolLoopAgentCallOptions<'a, M: LanguageModel + ?Sized> {
    pub model: Option<&'a M>,
    pub prompt: Prompt,
    pub call_options: Option<JsonValue>,
    pub instructions: Option<Instructions>,
    pub model_settings: ToolLoopAgentModelSettings,
    pub tools: Vec<GenerateTextTool>,
    pub runtime_context: JsonObject,
    pub tools_context: JsonObject,
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
    pub active_tools: ActiveTools,
    pub tool_approval: Option<ToolApprovalConfiguration>,
    pub tool_input_refinements: BTreeMap<String, ToolInputRefinement>,
    pub tool_call_repair: Option<ToolCallRepair>,
    pub prepare_step: Option<PrepareStep<'a, M>>,
    pub on_start: Option<GenerateTextOnStart<'a>>,
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,
    pub on_finish: Option<GenerateTextOnFinish<'a>>,
    pub telemetry: Option<TelemetryOptions>,
    pub abort_signal: Option<LanguageModelAbortSignal>,
    pub timeout: Option<TimeoutConfiguration>,
    pub max_steps: Option<usize>,
    pub stop_conditions: Vec<StopCondition>,
    pub include: Option<GenerateTextInclude>,
}

impl<'a, M: LanguageModel + ?Sized> ToolLoopAgentCallOptions<'a, M> {
    /// Creates per-call options from a prompt.
    pub fn new(prompt: Prompt) -> Self {
        Self {
            model: None,
            prompt,
            call_options: None,
            instructions: None,
            model_settings: ToolLoopAgentModelSettings::default(),
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
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
            abort_signal: None,
            timeout: None,
            max_steps: None,
            stop_conditions: Vec::new(),
            include: None,
        }
    }

    /// Creates per-call options from text.
    pub fn from_prompt(prompt: impl Into<PromptInput>) -> Self {
        Self::new(Prompt::from_prompt(prompt))
    }

    /// Creates per-call options from model messages.
    pub fn from_messages(messages: crate::language_model::LanguageModelPrompt) -> Self {
        Self::new(Prompt::from_messages(messages))
    }

    /// Overrides the model for this call.
    pub fn with_model(mut self, model: &'a M) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets per-call agent options validated by the configured call-options schema.
    pub fn with_options(mut self, options: impl Into<JsonValue>) -> Self {
        self.call_options = Some(options.into());
        self
    }

    /// Overrides instructions for this call.
    pub fn with_instructions(mut self, instructions: impl Into<Instructions>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Sets per-call model settings.
    pub fn with_model_settings(mut self, model_settings: ToolLoopAgentModelSettings) -> Self {
        self.model_settings = model_settings;
        self
    }

    /// Adds a per-call tool.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        self.tools.push(tool.into());
        self
    }

    /// Sets per-call runtime context.
    pub fn with_runtime_context(mut self, runtime_context: JsonObject) -> Self {
        self.runtime_context = runtime_context;
        self
    }

    /// Sets per-call tool context.
    pub fn with_tools_context(mut self, tools_context: JsonObject) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets per-call experimental sandbox.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }

    /// Sets per-call active tool names.
    pub fn with_active_tools(
        mut self,
        active_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.active_tools = Some(active_tools.into_iter().map(Into::into).collect());
        self
    }

    /// Sets per-call tool approval configuration.
    pub fn with_tool_approval(mut self, tool_approval: ToolApprovalConfiguration) -> Self {
        self.tool_approval = Some(tool_approval);
        self
    }

    /// Adds or replaces a per-call input refinement for one tool.
    pub fn with_tool_input_refinement<F, Fut>(
        mut self,
        tool_name: impl Into<String>,
        refine: F,
    ) -> Self
    where
        F: Fn(JsonValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, crate::generate_text::ToolInputRefinementError>>
            + Send
            + 'static,
    {
        self.tool_input_refinements
            .insert(tool_name.into(), ToolInputRefinement::new(refine));
        self
    }

    /// Sets a per-call tool-call repair callback.
    pub fn with_tool_call_repair<F, Fut, E>(mut self, repair: F) -> Self
    where
        F: Fn(crate::generate_text::ToolCallRepairOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<crate::language_model::LanguageModelToolCall>, E>>
            + Send
            + 'static,
        E: fmt::Display,
    {
        self.tool_call_repair = Some(ToolCallRepair::new(repair));
        self
    }

    /// Sets a per-call step preparation callback.
    pub fn with_prepare_step<F, Fut>(mut self, prepare_step: F) -> Self
    where
        F: Fn(PrepareStepOptions<'a, M>) -> Fut + 'a,
        Fut: Future<Output = PrepareStepResult<'a, M>> + 'a,
    {
        self.prepare_step = Some(PrepareStep::new(prepare_step));
        self
    }

    /// Sets a per-call start callback.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(GenerateTextStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(GenerateTextOnStart::new(on_start));
        self
    }

    /// Sets a per-call step-start callback.
    pub fn with_on_step_start<F, Fut>(mut self, on_step_start: F) -> Self
    where
        F: Fn(GenerateTextStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_start = Some(GenerateTextOnStepStart::new(on_step_start));
        self
    }

    /// Sets a per-call tool-execution start callback.
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

    /// Sets a per-call tool-execution end callback.
    pub fn with_on_tool_execution_end<F, Fut>(mut self, on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_tool_execution_end =
            Some(GenerateTextOnToolExecutionEnd::new(on_tool_execution_end));
        self
    }

    /// Sets a per-call step-finish callback.
    pub fn with_on_step_finish<F, Fut>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(GenerateTextStep) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_finish = Some(GenerateTextOnStepFinish::new(on_step_finish));
        self
    }

    /// Sets a per-call finish callback.
    pub fn with_on_finish<F, Fut>(mut self, on_finish: F) -> Self
    where
        F: Fn(GenerateTextFinishEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_finish = Some(GenerateTextOnFinish::new(on_finish));
        self
    }

    /// Sets per-call telemetry.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Sets a per-call abort signal.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets a per-call timeout configuration.
    pub fn with_timeout(mut self, timeout: impl Into<TimeoutConfiguration>) -> Self {
        self.timeout = Some(timeout.into());
        self
    }

    /// Sets per-call maximum model-call steps.
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = Some(max_steps.max(1));
        self
    }

    /// Adds a per-call stop condition.
    pub fn with_stop_condition(mut self, stop_condition: StopCondition) -> Self {
        self.stop_conditions.push(stop_condition);
        self
    }

    /// Sets per-call non-streaming provider payload retention.
    pub fn with_include(mut self, include: GenerateTextInclude) -> Self {
        self.include = Some(include);
        self
    }
}

impl<'a, M: LanguageModel + ?Sized> From<&str> for ToolLoopAgentCallOptions<'a, M> {
    fn from(prompt: &str) -> Self {
        Self::from_prompt(prompt)
    }
}

impl<'a, M: LanguageModel + ?Sized> From<String> for ToolLoopAgentCallOptions<'a, M> {
    fn from(prompt: String) -> Self {
        Self::from_prompt(prompt)
    }
}

impl<'a, M: LanguageModel + ?Sized> From<Prompt> for ToolLoopAgentCallOptions<'a, M> {
    fn from(prompt: Prompt) -> Self {
        Self::new(prompt)
    }
}

/// Fully prepared agent call passed to `prepare_call`.
pub struct ToolLoopAgentPreparedCall<'a, M: LanguageModel + ?Sized> {
    pub model: &'a M,
    pub prompt: Prompt,
    pub call_options: Option<JsonValue>,
    pub instructions: Option<Instructions>,
    pub model_settings: ToolLoopAgentModelSettings,
    pub tools: Vec<GenerateTextTool>,
    pub runtime_context: JsonObject,
    pub tools_context: JsonObject,
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
    pub active_tools: ActiveTools,
    pub tool_approval: Option<ToolApprovalConfiguration>,
    pub tool_input_refinements: BTreeMap<String, ToolInputRefinement>,
    pub tool_call_repair: Option<ToolCallRepair>,
    pub prepare_step: Option<PrepareStep<'a, M>>,
    pub on_start: Option<GenerateTextOnStart<'a>>,
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,
    pub on_finish: Option<GenerateTextOnFinish<'a>>,
    pub telemetry: Option<TelemetryOptions>,
    pub abort_signal: Option<LanguageModelAbortSignal>,
    pub timeout: Option<TimeoutConfiguration>,
    pub max_steps: usize,
    pub stop_conditions: Vec<StopCondition>,
    pub include: Option<GenerateTextInclude>,
}

impl<'a, M: LanguageModel + ?Sized> ToolLoopAgentPreparedCall<'a, M> {
    /// Overrides provider options for the prepared call.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.model_settings.provider_options = Some(provider_options);
        self
    }

    /// Overrides the prepared prompt.
    pub fn with_prompt(mut self, prompt: Prompt) -> Self {
        self.prompt = prompt;
        self
    }

    /// Adds a prepared tool.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        self.tools.push(tool.into());
        self
    }

    /// Sets the prepared step callback.
    pub fn with_prepare_step(mut self, prepare_step: PrepareStep<'a, M>) -> Self {
        self.prepare_step = Some(prepare_step);
        self
    }
}

/// Call preparation callback for [`ToolLoopAgent`].
pub struct ToolLoopAgentPrepareCall<'a, M: LanguageModel + ?Sized> {
    prepare_call:
        Rc<dyn Fn(ToolLoopAgentPreparedCall<'a, M>) -> ToolLoopAgentPreparedCall<'a, M> + 'a>,
}

impl<'a, M: LanguageModel + ?Sized> ToolLoopAgentPrepareCall<'a, M> {
    /// Creates a call preparation callback.
    pub fn new<F>(prepare_call: F) -> Self
    where
        F: Fn(ToolLoopAgentPreparedCall<'a, M>) -> ToolLoopAgentPreparedCall<'a, M> + 'a,
    {
        Self {
            prepare_call: Rc::new(prepare_call),
        }
    }

    /// Runs call preparation.
    pub fn prepare(
        &self,
        prepared_call: ToolLoopAgentPreparedCall<'a, M>,
    ) -> ToolLoopAgentPreparedCall<'a, M> {
        (self.prepare_call)(prepared_call)
    }
}

impl<M: LanguageModel + ?Sized> fmt::Debug for ToolLoopAgentPrepareCall<'_, M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolLoopAgentPrepareCall")
            .finish_non_exhaustive()
    }
}

fn generate_options_from_prepared<'a, M: LanguageModel + ?Sized>(
    prepared: ToolLoopAgentPreparedCall<'a, M>,
) -> Result<GenerateTextOptions<'a, M>, InvalidPromptError> {
    let mut options = GenerateTextOptions::from_prompt(
        prepared.model,
        prompt_with_instructions(prepared.prompt.clone(), prepared.instructions.clone()),
    )?;
    prepared
        .model_settings
        .apply_to_call_options(&mut options.call_options);
    Ok(apply_generate_prepared_options(options, prepared))
}

fn stream_options_from_prepared<'a, M: LanguageModel + ?Sized>(
    prepared: ToolLoopAgentPreparedCall<'a, M>,
) -> Result<StreamTextOptions<'a, M>, InvalidPromptError> {
    let mut options = StreamTextOptions::from_prompt(
        prepared.model,
        prompt_with_instructions(prepared.prompt.clone(), prepared.instructions.clone()),
    )?;
    prepared
        .model_settings
        .apply_to_call_options(&mut options.call_options);
    Ok(apply_stream_prepared_options(options, prepared))
}

fn apply_generate_prepared_options<'a, M: LanguageModel + ?Sized>(
    mut options: GenerateTextOptions<'a, M>,
    prepared: ToolLoopAgentPreparedCall<'a, M>,
) -> GenerateTextOptions<'a, M> {
    for tool in prepared.tools {
        options = options.with_tool(tool);
    }
    options.runtime_context = prepared.runtime_context;
    options.tools_context = prepared.tools_context;
    options.experimental_sandbox = prepared.experimental_sandbox;
    options.active_tools = prepared.active_tools;
    options.tool_approval = prepared.tool_approval;
    options.tool_input_refinements = prepared.tool_input_refinements;
    options.tool_call_repair = prepared.tool_call_repair;
    options.prepare_step = prepared.prepare_step;
    options.on_start = prepared.on_start;
    options.on_step_start = prepared.on_step_start;
    options.on_tool_execution_start = prepared.on_tool_execution_start;
    options.on_tool_execution_end = prepared.on_tool_execution_end;
    options.on_step_finish = prepared.on_step_finish;
    options.on_finish = prepared.on_finish;
    options.telemetry = prepared.telemetry;
    if let Some(abort_signal) = prepared.abort_signal {
        options.call_options.abort_signal = Some(abort_signal);
    }
    options.timeout = prepared.timeout;
    options.max_steps = prepared.max_steps.max(1);
    options.stop_conditions = prepared.stop_conditions;
    if let Some(include) = prepared.include {
        options.include = include;
    }
    options
}

fn apply_stream_prepared_options<'a, M: LanguageModel + ?Sized>(
    mut options: StreamTextOptions<'a, M>,
    prepared: ToolLoopAgentPreparedCall<'a, M>,
) -> StreamTextOptions<'a, M> {
    for tool in prepared.tools {
        options = options.with_tool(tool);
    }
    options.runtime_context = prepared.runtime_context;
    options.tools_context = prepared.tools_context;
    options.experimental_sandbox = prepared.experimental_sandbox;
    options.active_tools = prepared.active_tools;
    options.tool_approval = prepared.tool_approval;
    options.tool_input_refinements = prepared.tool_input_refinements;
    options.tool_call_repair = prepared.tool_call_repair;
    options.prepare_step = prepared.prepare_step;
    options.on_start = prepared.on_start;
    options.on_step_start = prepared.on_step_start;
    options.on_tool_execution_start = prepared.on_tool_execution_start;
    options.on_tool_execution_end = prepared.on_tool_execution_end;
    options.on_step_finish = prepared.on_step_finish;
    options.on_finish = prepared.on_finish;
    options.telemetry = prepared.telemetry;
    if let Some(abort_signal) = prepared.abort_signal {
        options = options.with_abort_signal(abort_signal);
    }
    options.timeout = prepared.timeout;
    options.max_steps = prepared.max_steps.max(1);
    options.stop_conditions = prepared.stop_conditions;
    options
}

fn validate_agent_call_options(
    call_options: Option<JsonValue>,
    schema: Option<&FlexibleSchema<JsonValue>>,
) -> Result<Option<JsonValue>, InvalidPromptError> {
    let Some(call_options) = call_options else {
        return Ok(None);
    };
    let Some(schema) = schema else {
        return Ok(Some(call_options));
    };

    validate_types(
        call_options,
        schema.clone(),
        Some(TypeValidationContext::new().with_field("options")),
    )
    .map(Some)
    .map_err(|error| InvalidPromptError::new(JsonValue::Null, error.message()))
}

fn prompt_with_instructions(mut prompt: Prompt, instructions: Option<Instructions>) -> Prompt {
    if prompt.instructions.is_none() && prompt.system.is_none() {
        prompt.instructions = instructions;
    }
    prompt
}

fn merged_tools(
    default_tools: &[GenerateTextTool],
    call_tools: Vec<GenerateTextTool>,
) -> Vec<GenerateTextTool> {
    default_tools.iter().cloned().chain(call_tools).collect()
}

fn merge_json_objects(mut base: JsonObject, overrides: JsonObject) -> JsonObject {
    base.extend(overrides);
    base
}

fn merge_tool_input_refinements(
    mut base: BTreeMap<String, ToolInputRefinement>,
    overrides: BTreeMap<String, ToolInputRefinement>,
) -> BTreeMap<String, ToolInputRefinement> {
    base.extend(overrides);
    base
}

fn merge_on_start<'a>(
    first: Option<GenerateTextOnStart<'a>>,
    second: Option<GenerateTextOnStart<'a>>,
) -> Option<GenerateTextOnStart<'a>> {
    match (first, second) {
        (Some(first), Some(second)) => Some(GenerateTextOnStart::new(move |event| {
            let first = first.clone();
            let second = second.clone();
            async move {
                first.start(event.clone()).await;
                second.start(event).await;
            }
        })),
        (Some(callback), None) | (None, Some(callback)) => Some(callback),
        (None, None) => None,
    }
}

fn merge_on_step_start<'a>(
    first: Option<GenerateTextOnStepStart<'a>>,
    second: Option<GenerateTextOnStepStart<'a>>,
) -> Option<GenerateTextOnStepStart<'a>> {
    match (first, second) {
        (Some(first), Some(second)) => Some(GenerateTextOnStepStart::new(move |event| {
            let first = first.clone();
            let second = second.clone();
            async move {
                first.start(event.clone()).await;
                second.start(event).await;
            }
        })),
        (Some(callback), None) | (None, Some(callback)) => Some(callback),
        (None, None) => None,
    }
}

fn merge_on_tool_execution_start<'a>(
    first: Option<GenerateTextOnToolExecutionStart<'a>>,
    second: Option<GenerateTextOnToolExecutionStart<'a>>,
) -> Option<GenerateTextOnToolExecutionStart<'a>> {
    match (first, second) {
        (Some(first), Some(second)) => Some(GenerateTextOnToolExecutionStart::new(move |event| {
            let first = first.clone();
            let second = second.clone();
            async move {
                first.start(event.clone()).await;
                second.start(event).await;
            }
        })),
        (Some(callback), None) | (None, Some(callback)) => Some(callback),
        (None, None) => None,
    }
}

fn merge_on_tool_execution_end<'a>(
    first: Option<GenerateTextOnToolExecutionEnd<'a>>,
    second: Option<GenerateTextOnToolExecutionEnd<'a>>,
) -> Option<GenerateTextOnToolExecutionEnd<'a>> {
    match (first, second) {
        (Some(first), Some(second)) => Some(GenerateTextOnToolExecutionEnd::new(move |event| {
            let first = first.clone();
            let second = second.clone();
            async move {
                first.end(event.clone()).await;
                second.end(event).await;
            }
        })),
        (Some(callback), None) | (None, Some(callback)) => Some(callback),
        (None, None) => None,
    }
}

fn merge_on_step_finish<'a>(
    first: Option<GenerateTextOnStepFinish<'a>>,
    second: Option<GenerateTextOnStepFinish<'a>>,
) -> Option<GenerateTextOnStepFinish<'a>> {
    match (first, second) {
        (Some(first), Some(second)) => Some(GenerateTextOnStepFinish::new(move |step| {
            let first = first.clone();
            let second = second.clone();
            async move {
                first.finish(step.clone()).await;
                second.finish(step).await;
            }
        })),
        (Some(callback), None) | (None, Some(callback)) => Some(callback),
        (None, None) => None,
    }
}

fn merge_on_finish<'a>(
    first: Option<GenerateTextOnFinish<'a>>,
    second: Option<GenerateTextOnFinish<'a>>,
) -> Option<GenerateTextOnFinish<'a>> {
    match (first, second) {
        (Some(first), Some(second)) => Some(GenerateTextOnFinish::new(move |event| {
            let first = first.clone();
            let second = second.clone();
            async move {
                first.finish(event.clone()).await;
                second.finish(event).await;
            }
        })),
        (Some(callback), None) | (None, Some(callback)) => Some(callback),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::*;
    use crate::language_model::{
        FinishReason, LanguageModelAbortController, LanguageModelAbortSignal,
        LanguageModelAssistantContentPart, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
        LanguageModelStreamFinish, LanguageModelStreamPart, LanguageModelStreamResult,
        LanguageModelSystemMessage, LanguageModelText, LanguageModelTextDelta,
        LanguageModelTextEnd, LanguageModelTextPart, LanguageModelTextStart, LanguageModelToolCall,
        LanguageModelToolContentPart, LanguageModelToolResultContentPart,
        LanguageModelToolResultOutput, LanguageModelUsage, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::mock_models::MockLanguageModel;
    use crate::prompt::TimeoutConfigurationOptions;
    use crate::provider_utils::{
        SandboxCommandOptions, SandboxCommandResult, SandboxRunCommandFuture, Schema, Tool,
        ValidationResult,
    };
    use crate::stream_text::TextStreamPart;
    use crate::telemetry::{
        TelemetryEvent, TelemetryEventKind, TelemetryIntegration, register_telemetry_integration,
        reset_telemetry_state_for_tests, telemetry_test_guard_for_tests,
    };

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn object_schema() -> crate::json::JsonSchema {
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

    fn value_schema() -> crate::json::JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    fn legal_topic_options_schema() -> FlexibleSchema<JsonValue> {
        FlexibleSchema::from(
            Schema::new(
                json!({
                    "type": "object",
                    "properties": {
                        "topic": { "enum": ["legal", "medical"] }
                    },
                    "required": ["topic"]
                })
                .as_object()
                .expect("schema is an object")
                .clone(),
            )
            .with_validator(|value| {
                if matches!(
                    value.get("topic").and_then(JsonValue::as_str),
                    Some("legal" | "medical")
                ) {
                    ValidationResult::success(value.clone())
                } else {
                    ValidationResult::failure("Expected 'legal' | 'medical'")
                }
            }),
        )
    }

    fn text_result(text: &str) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            LanguageModelUsage::default(),
        )
    }

    fn tool_call_result() -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                "call-weather",
                "weather",
                r#"{"city":"Brisbane"}"#,
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::ToolCalls,
                raw: None,
            },
            LanguageModelUsage::default(),
        )
    }

    fn test_tool_call_result() -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                "call-1",
                "testTool",
                r#"{ "value": "test" }"#,
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::ToolCalls,
                raw: None,
            },
            LanguageModelUsage::default(),
        )
    }

    fn stream_text_result(text: &str) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", text)),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                LanguageModelUsage::default(),
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
            )),
        ])
    }

    fn stream_tool_call_result() -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "call-weather",
                "weather",
                r#"{"city":"Brisbane"}"#,
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                LanguageModelUsage::default(),
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: Some("tool-calls".to_string()),
                },
            )),
        ])
    }

    fn stream_test_tool_call_result() -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "call-1",
                "testTool",
                r#"{ "value": "test" }"#,
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                LanguageModelUsage::default(),
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: Some("tool-calls".to_string()),
                },
            )),
        ])
    }

    fn test_tool_call_result_with_input(input: &str) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                "call-1", "testTool", input,
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::ToolCalls,
                raw: None,
            },
            LanguageModelUsage::default(),
        )
    }

    fn stream_test_tool_call_result_with_input(
        input: &str,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "call-1", "testTool", input,
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                LanguageModelUsage::default(),
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: Some("tool-calls".to_string()),
                },
            )),
        ])
    }

    fn test_value_tool() -> Tool {
        Tool::new("testTool", value_schema()).with_execute(|input, _options| async move {
            let value = input
                .get("value")
                .and_then(JsonValue::as_str)
                .expect("tool input includes value");
            Ok(json!(format!("{value}-result")))
        })
    }

    fn agent_lifecycle_integration(events: Arc<Mutex<Vec<&'static str>>>) -> TelemetryIntegration {
        let mut integration = TelemetryIntegration::new();
        for (kind, label) in [
            (TelemetryEventKind::OnStart, "onStart"),
            (TelemetryEventKind::OnStepStart, "onStepStart"),
            (
                TelemetryEventKind::OnToolExecutionStart,
                "onToolExecutionStart",
            ),
            (TelemetryEventKind::OnToolExecutionEnd, "onToolExecutionEnd"),
            (TelemetryEventKind::OnStepFinish, "onStepFinish"),
            (TelemetryEventKind::OnEnd, "onEnd"),
        ] {
            let events = Arc::clone(&events);
            integration = integration.with_callback(kind, move |_event| {
                events.lock().expect("event lock").push(label);
            });
        }

        integration
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

    fn user_message(text: &str) -> crate::language_model::LanguageModelMessage {
        crate::language_model::LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    fn system_message(text: &str) -> crate::language_model::LanguageModelMessage {
        crate::language_model::LanguageModelMessage::System(LanguageModelSystemMessage::new(text))
    }

    fn system_message_with_provider_options(
        text: &str,
        provider_options: ProviderOptions,
    ) -> crate::language_model::LanguageModelMessage {
        crate::language_model::LanguageModelMessage::System(
            LanguageModelSystemMessage::new(text).with_provider_options(provider_options),
        )
    }

    fn provider_options_value(value: &str) -> ProviderOptions {
        serde_json::from_value(json!({
            "test": { "value": value }
        }))
        .expect("provider options")
    }

    fn generate_call_with_model_settings(
        model_settings: ToolLoopAgentModelSettings,
    ) -> LanguageModelCallOptions {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model).with_model_settings(model_settings),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        model
            .generate_calls()
            .into_iter()
            .next()
            .expect("model was called")
    }

    fn stream_call_with_model_settings(
        model_settings: ToolLoopAgentModelSettings,
    ) -> LanguageModelCallOptions {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("reply"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model).with_model_settings(model_settings),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "reply");
        model
            .stream_calls()
            .into_iter()
            .next()
            .expect("model was called")
    }

    #[test]
    fn tool_loop_agent_exposes_version_id_and_tools() {
        let model = MockLanguageModel::new();
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_id("weather-agent")
                .with_tool(Tool::new("weather", object_schema())),
        );

        assert_eq!(agent.version(), "agent-v1");
        assert_eq!(agent.id(), Some("weather-agent"));
        assert_eq!(agent.tools().len(), 1);
    }

    #[test]
    fn infer_agent_ui_message_should_not_contain_arbitrary_static_tools_when_no_tools_are_provided()
    {
        let model = MockLanguageModel::new();
        let agent = ToolLoopAgent::for_model(&model);

        assert!(agent.tools().is_empty());
        assert!(crate::ui_message_stream::is_dynamic_tool_ui_part(&json!({
            "type": "dynamic-tool",
            "toolName": "runtimeTool",
            "toolCallId": "call-1",
            "state": "input-available",
            "input": {}
        })));
        assert!(crate::ui_message_stream::is_data_ui_part(&json!({
            "type": "data-status",
            "data": { "state": "pending" }
        })));
        assert!(
            !agent
                .tools()
                .iter()
                .any(|tool| matches!(tool, GenerateTextTool::Rust(tool) if tool.name == "weather"))
        );
    }

    #[test]
    fn infer_agent_ui_message_should_include_metadata_when_provided() {
        let model = MockLanguageModel::new();
        let _agent = ToolLoopAgent::for_model(&model);

        let message = UiMessage::new("msg-1", crate::ui_message_stream::UiMessageRole::User)
            .with_metadata(json!({ "foo": "bar" }))
            .with_part(json!({ "type": "text", "text": "hello" }));

        assert_eq!(message.metadata, Some(json!({ "foo": "bar" })));
        assert_eq!(
            serde_json::to_value(message).expect("UI message serializes"),
            json!({
                "id": "msg-1",
                "role": "user",
                "metadata": { "foo": "bar" },
                "parts": [{ "type": "text", "text": "hello" }]
            })
        );
    }

    #[test]
    fn tool_loop_agent_generate_forwards_settings_and_instructions() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "test": { "route": "agent" }
        }))
        .expect("provider options");
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_instructions("Use concise answers.")
                .with_model_settings(
                    ToolLoopAgentModelSettings::new()
                        .with_temperature(0.2)
                        .with_max_output_tokens(32)
                        .with_provider_options(provider_options.clone()),
                ),
        );

        let result = poll_ready(agent.generate("Hello")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let calls = model.generate_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].temperature, Some(0.2));
        assert_eq!(calls[0].max_output_tokens, Some(32));
        assert_eq!(calls[0].provider_options, Some(provider_options));
        assert_eq!(calls[0].prompt.len(), 2);
        assert!(matches!(
            &calls[0].prompt[0],
            crate::language_model::LanguageModelMessage::System(message)
                if message.content == "Use concise answers."
        ));
    }

    #[test]
    fn tool_loop_agent_generate_passes_string_instructions() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model).with_instructions("INSTRUCTIONS"),
        );

        let result =
            poll_ready(agent.generate("Hello, world!")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(
            model.generate_calls()[0].prompt,
            vec![
                system_message("INSTRUCTIONS"),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_passes_system_message_instructions() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let provider_options = provider_options_value("test");
        let instructions = LanguageModelSystemMessage::new("INSTRUCTIONS")
            .with_provider_options(provider_options.clone());
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_instructions(instructions));

        let result =
            poll_ready(agent.generate("Hello, world!")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(
            model.generate_calls()[0].prompt,
            vec![
                system_message_with_provider_options("INSTRUCTIONS", provider_options),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_passes_array_of_system_message_instructions() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let first_options = provider_options_value("test");
        let second_options = provider_options_value("test 2");
        let instructions = vec![
            LanguageModelSystemMessage::new("INSTRUCTIONS")
                .with_provider_options(first_options.clone()),
            LanguageModelSystemMessage::new("INSTRUCTIONS 2")
                .with_provider_options(second_options.clone()),
        ];
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_instructions(instructions));

        let result =
            poll_ready(agent.generate("Hello, world!")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(
            model.generate_calls()[0].prompt,
            vec![
                system_message_with_provider_options("INSTRUCTIONS", first_options),
                system_message_with_provider_options("INSTRUCTIONS 2", second_options),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_forwards_temperature_to_generate_text() {
        let call = generate_call_with_model_settings(
            ToolLoopAgentModelSettings::new().with_temperature(0.5),
        );

        assert_eq!(call.temperature, Some(0.5));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_max_output_tokens_to_generate_text() {
        let call = generate_call_with_model_settings(
            ToolLoopAgentModelSettings::new().with_max_output_tokens(256),
        );

        assert_eq!(call.max_output_tokens, Some(256));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_top_p_to_generate_text() {
        let call =
            generate_call_with_model_settings(ToolLoopAgentModelSettings::new().with_top_p(0.9));

        assert_eq!(call.top_p, Some(0.9));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_top_k_to_generate_text() {
        let call =
            generate_call_with_model_settings(ToolLoopAgentModelSettings::new().with_top_k(40));

        assert_eq!(call.top_k, Some(40));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_presence_penalty_to_generate_text() {
        let call = generate_call_with_model_settings(
            ToolLoopAgentModelSettings::new().with_presence_penalty(0.1),
        );

        assert_eq!(call.presence_penalty, Some(0.1));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_frequency_penalty_to_generate_text() {
        let call = generate_call_with_model_settings(
            ToolLoopAgentModelSettings::new().with_frequency_penalty(0.2),
        );

        assert_eq!(call.frequency_penalty, Some(0.2));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_stop_sequences_to_generate_text() {
        let call = generate_call_with_model_settings(
            ToolLoopAgentModelSettings::new()
                .with_stop_sequence("STOP")
                .with_stop_sequence("END"),
        );

        assert_eq!(
            call.stop_sequences,
            Some(vec!["STOP".to_string(), "END".to_string()])
        );
    }

    #[test]
    fn tool_loop_agent_generate_forwards_seed_to_generate_text() {
        let call =
            generate_call_with_model_settings(ToolLoopAgentModelSettings::new().with_seed(42));

        assert_eq!(call.seed, Some(42));
    }

    #[test]
    fn tool_loop_agent_generate_forwards_headers_to_generate_text() {
        let mut headers = Headers::new();
        headers.insert("x-custom".to_string(), "value".to_string());
        let call = generate_call_with_model_settings(
            ToolLoopAgentModelSettings::new().with_headers(headers),
        );

        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("x-custom")),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn tool_loop_agent_generate_forwards_include_request_messages_to_generate_text() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_include(GenerateTextInclude::new().with_request_messages(true)),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        let expected_messages = vec![user_message("test")];
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.messages.as_ref()),
            Some(&expected_messages)
        );
    }

    #[test]
    fn tool_loop_agent_stream_forwards_include_raw_chunks_to_stream_text() {
        let call = stream_call_with_model_settings(
            ToolLoopAgentModelSettings::new().with_include_raw_chunks(true),
        );

        assert_eq!(call.include_raw_chunks, Some(true));
    }

    #[test]
    fn tool_loop_agent_prepare_call_can_shape_provider_options() {
        let model = MockLanguageModel::new().with_generate_result(text_result("prepared"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_prepare_call(
            |prepared| {
                prepared.with_provider_options(
                    serde_json::from_value(json!({
                        "test": { "value": "prepared" }
                    }))
                    .expect("provider options"),
                )
            },
        ));

        let result = poll_ready(agent.generate("Hello")).expect("agent generation succeeds");

        assert_eq!(result.text, "prepared");
        assert_eq!(
            model.generate_calls()[0].provider_options,
            Some(
                serde_json::from_value(json!({
                    "test": { "value": "prepared" }
                }))
                .expect("provider options")
            )
        );
    }

    #[test]
    fn tool_loop_agent_generate_rejects_invalid_call_options_schema_before_model_call() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_call_options_schema(legal_topic_options_schema()),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("hi").with_options(json!({ "topic": "evil" }));

        let error = poll_ready(agent.generate(options)).expect_err("options are invalid");

        assert!(
            error
                .message()
                .contains("Type validation failed for options"),
            "{}",
            error.message()
        );
        assert!(model.generate_calls().is_empty());
    }

    #[test]
    fn tool_loop_agent_generate_passes_valid_call_options_schema() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let recorded_options = Rc::new(RefCell::new(None::<JsonValue>));
        let recorded_options_for_prepare_call = Rc::clone(&recorded_options);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_call_options_schema(legal_topic_options_schema())
                .with_prepare_call(move |prepared| {
                    *recorded_options_for_prepare_call.borrow_mut() = prepared.call_options.clone();
                    prepared
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("hi").with_options(json!({ "topic": "legal" }));

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(
            recorded_options.borrow().as_ref(),
            Some(&json!({ "topic": "legal" }))
        );
    }

    #[test]
    fn tool_loop_agent_generate_passes_sandbox_to_prepare_call() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("test sandbox"));
        let recorded_sandbox = Arc::new(Mutex::new(None::<Arc<dyn ExperimentalSandbox>>));
        let recorded_sandbox_for_prepare_call = Arc::clone(&recorded_sandbox);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_prepare_call(
            move |prepared| {
                *recorded_sandbox_for_prepare_call
                    .lock()
                    .expect("lock sandbox") = prepared.experimental_sandbox.clone();
                prepared
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello")
                .with_experimental_sandbox(Arc::clone(&sandbox));

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let recorded_sandbox = recorded_sandbox.lock().expect("lock sandbox");
        assert!(Arc::ptr_eq(
            recorded_sandbox
                .as_ref()
                .expect("prepare_call sees sandbox"),
            &sandbox
        ));
    }

    #[test]
    fn tool_loop_agent_stream_prepare_call_can_shape_provider_options() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("prepared"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_prepare_call(
            |prepared| {
                prepared.with_provider_options(
                    serde_json::from_value(json!({
                        "test": { "value": "prepared" }
                    }))
                    .expect("provider options"),
                )
            },
        ));

        let result = poll_ready(agent.stream("Hello")).expect("agent stream succeeds");

        assert_eq!(result.text, "prepared");
        assert_eq!(
            model.stream_calls()[0].provider_options,
            Some(
                serde_json::from_value(json!({
                    "test": { "value": "prepared" }
                }))
                .expect("provider options")
            )
        );
    }

    #[test]
    fn tool_loop_agent_generate_passes_prepare_step_to_generate_text() {
        let model = MockLanguageModel::new().with_generate_result(text_result("prepared"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_prepare_step(
            |options| async move {
                assert_eq!(options.step_number, 0);
                assert_eq!(
                    options.initial_messages,
                    vec![user_message("Original prompt")]
                );
                PrepareStepResult::new().with_messages(vec![user_message("Prepared prompt")])
            },
        ));

        let result =
            poll_ready(agent.generate("Original prompt")).expect("agent generation succeeds");

        assert_eq!(result.text, "prepared");
        assert_eq!(
            model.generate_calls()[0].prompt,
            vec![user_message("Prepared prompt")]
        );
    }

    #[test]
    fn tool_loop_agent_stream_per_call_prepare_step_overrides_default_prepare_step() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("prepared"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_prepare_step(
            |_options| async move {
                PrepareStepResult::new().with_messages(vec![user_message("Default prepared")])
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Original prompt").with_prepare_step(
                |options| async move {
                    assert_eq!(options.step_number, 0);
                    assert_eq!(
                        options.initial_messages,
                        vec![user_message("Original prompt")]
                    );
                    PrepareStepResult::new().with_messages(vec![user_message("Call prepared")])
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "prepared");
        assert_eq!(
            model.stream_calls()[0].prompt,
            vec![user_message("Call prepared")]
        );
    }

    #[test]
    fn tool_loop_agent_prepare_step_receives_merged_runtime_and_tools_context() {
        let model = MockLanguageModel::new().with_generate_result(text_result("prepared"));
        let seen_prepare_contexts = Rc::new(RefCell::new(Vec::<JsonValue>::new()));
        let seen_prepare_contexts_for_callback = Rc::clone(&seen_prepare_contexts);
        let seen_finish_contexts = Rc::new(RefCell::new(Vec::<JsonValue>::new()));
        let seen_finish_contexts_for_callback = Rc::clone(&seen_finish_contexts);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_runtime_context(JsonObject::from_iter([
                    ("tenant".to_string(), json!("default")),
                    ("region".to_string(), json!("au")),
                ]))
                .with_tools_context(JsonObject::from_iter([
                    ("weather".to_string(), json!({ "apiKey": "default-key" })),
                    ("search".to_string(), json!({ "endpoint": "default" })),
                ]))
                .with_prepare_step(move |options| {
                    seen_prepare_contexts_for_callback.borrow_mut().push(json!({
                        "runtimeContext": options.runtime_context,
                        "toolsContext": options.tools_context,
                    }));
                    async move { PrepareStepResult::new() }
                })
                .with_on_finish(move |event| {
                    seen_finish_contexts_for_callback.borrow_mut().push(json!({
                        "runtimeContext": event.runtime_context,
                        "toolsContext": event.tools_context,
                    }));
                    ready(())
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Original prompt")
                .with_runtime_context(JsonObject::from_iter([(
                    "tenant".to_string(),
                    json!("call"),
                )]))
                .with_tools_context(JsonObject::from_iter([(
                    "weather".to_string(),
                    json!({ "apiKey": "call-key" }),
                )]));

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "prepared");
        assert_eq!(
            seen_prepare_contexts.borrow().as_slice(),
            [json!({
                "runtimeContext": {
                    "tenant": "call",
                    "region": "au"
                },
                "toolsContext": {
                    "weather": { "apiKey": "call-key" },
                    "search": { "endpoint": "default" }
                }
            })]
        );
        assert_eq!(
            seen_finish_contexts.borrow().as_slice(),
            [json!({
                "runtimeContext": {
                    "tenant": "call",
                    "region": "au"
                },
                "toolsContext": {
                    "weather": { "apiKey": "call-key" },
                    "search": { "endpoint": "default" }
                }
            })]
        );
    }

    #[test]
    fn tool_loop_agent_stream_passes_sandbox_to_prepare_call() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("reply"));
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("test sandbox"));
        let recorded_sandbox = Arc::new(Mutex::new(None::<Arc<dyn ExperimentalSandbox>>));
        let recorded_sandbox_for_prepare_call = Arc::clone(&recorded_sandbox);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_prepare_call(
            move |prepared| {
                *recorded_sandbox_for_prepare_call
                    .lock()
                    .expect("lock sandbox") = prepared.experimental_sandbox.clone();
                prepared
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello")
                .with_experimental_sandbox(Arc::clone(&sandbox));

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "reply");
        let recorded_sandbox = recorded_sandbox.lock().expect("lock sandbox");
        assert!(Arc::ptr_eq(
            recorded_sandbox
                .as_ref()
                .expect("prepare_call sees sandbox"),
            &sandbox
        ));
    }

    #[test]
    fn tool_loop_agent_generate_passes_abort_signal_to_generate_text() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let abort_controller = LanguageModelAbortController::new();
        let abort_signal = abort_controller.signal();
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_abort_signal(abort_signal.clone());

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let calls = model.generate_calls();
        let captured_signal = calls[0]
            .abort_signal
            .as_ref()
            .expect("generate_text receives abort signal");
        assert!(captured_signal.is_same_signal(&abort_signal));
    }

    #[test]
    fn tool_loop_agent_generate_passes_timeout_to_tool_execution() {
        let model = MockLanguageModel::new()
            .with_generate_results([tool_call_result(), text_result("done")]);
        let received_signal = Arc::new(Mutex::new(None::<LanguageModelAbortSignal>));
        let received_signal_for_closure = Arc::clone(&received_signal);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(
            Tool::new("weather", object_schema()).with_execute(move |_input, options| {
                let received_signal = Arc::clone(&received_signal_for_closure);
                async move {
                    *received_signal.lock().expect("signal lock") = options.abort_signal;
                    Ok(json!({ "forecast": "sunny" }))
                }
            }),
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("What is the weather?").with_timeout(
                TimeoutConfigurationOptions::new().with_tool_timeout("weather", 10_000),
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        let captured_signal = received_signal
            .lock()
            .expect("signal lock")
            .clone()
            .expect("tool timeout creates abort signal");
        assert!(!captured_signal.is_aborted());
    }

    #[test]
    fn tool_loop_agent_generate_passes_sandbox_to_tool_execution() {
        let model = MockLanguageModel::new()
            .with_generate_results([tool_call_result(), text_result("done")]);
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("test sandbox"));
        let received_sandbox = Arc::new(Mutex::new(None::<Arc<dyn ExperimentalSandbox>>));
        let received_sandbox_for_closure = Arc::clone(&received_sandbox);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(
            Tool::new("weather", object_schema()).with_execute(move |_input, options| {
                let received_sandbox = Arc::clone(&received_sandbox_for_closure);
                async move {
                    *received_sandbox.lock().expect("sandbox lock") = options.experimental_sandbox;
                    Ok(json!({ "forecast": "sunny" }))
                }
            }),
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("What is the weather?")
                .with_experimental_sandbox(Arc::clone(&sandbox));

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        let received_sandbox = received_sandbox.lock().expect("sandbox lock");
        assert!(Arc::ptr_eq(
            received_sandbox
                .as_ref()
                .expect("tool execution receives sandbox"),
            &sandbox
        ));
    }

    #[test]
    fn tool_loop_agent_generate_honors_tool_approval() {
        let model = MockLanguageModel::new().with_generate_result(test_tool_call_result());
        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_tool = Arc::clone(&executed);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(Tool::new("testTool", value_schema()).with_execute(
                    move |_input, _options| {
                        let executed = Arc::clone(&executed_for_tool);
                        async move {
                            executed.store(true, Ordering::SeqCst);
                            Ok(json!("tool-result"))
                        }
                    },
                ))
                .with_tool_approval(ToolApprovalConfiguration::new().with_tool_status(
                    "testTool",
                    crate::generate_text::ToolApprovalStatusKind::UserApproval,
                )),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(model.generate_calls().len(), 1);
        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert!(result.tool_results.is_empty());
        assert_eq!(result.response_messages.len(), 1);

        let crate::language_model::LanguageModelMessage::Assistant(assistant_message) =
            &result.response_messages[0]
        else {
            panic!("response messages include assistant approval request");
        };
        assert_eq!(assistant_message.content.len(), 2);
        assert!(matches!(
            &assistant_message.content[0],
            LanguageModelAssistantContentPart::ToolCall(call)
                if call.tool_call_id == "call-1" && call.tool_name == "testTool"
        ));
        assert!(matches!(
            &assistant_message.content[1],
            LanguageModelAssistantContentPart::ToolApprovalRequest(request)
                if request.tool_call_id == "call-1" && request.is_automatic.is_none()
        ));
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_start_from_constructor() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_start(
            move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));

        let result =
            poll_ready(agent.generate("Hello, world!")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_start_from_method() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello, world!").with_on_start(move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("method");
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_generate_on_start_passes_event_information() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextStartEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let runtime_context = JsonObject::from_iter([("userId".to_string(), json!("test-user"))]);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_instructions("You are a helpful assistant")
                .with_runtime_context(runtime_context.clone())
                .with_model_settings(
                    ToolLoopAgentModelSettings::new()
                        .with_temperature(0.7)
                        .with_max_output_tokens(500),
                ),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello, world!").with_on_start(move |event| {
                let events = Rc::clone(&events_for_callback);
                async move {
                    events.borrow_mut().push(event);
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, "mock-provider");
        assert_eq!(event.model_id, "mock-model-id");
        assert_eq!(event.temperature, Some(0.7));
        assert_eq!(event.max_output_tokens, Some(500));
        assert_eq!(event.runtime_context, runtime_context);
        assert_eq!(
            event.messages,
            vec![
                system_message("You are a helpful assistant"),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_on_start_passes_messages_option() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextStartEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_messages(vec![user_message("test-message")])
                .with_on_start(move |event| {
                    let events = Rc::clone(&events_for_callback);
                    async move {
                        events.borrow_mut().push(event);
                    }
                });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].messages, vec![user_message("test-message")]);
    }

    #[test]
    fn tool_loop_agent_merges_generate_start_callbacks_in_order() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_start(
            move |_event| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("settings");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_start(move |_event| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("call");
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["settings", "call"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_step_start_from_constructor() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_step_start(
            move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));

        let result =
            poll_ready(agent.generate("Hello, world!")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_step_start_from_method() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello, world!").with_on_step_start(
                move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_generate_merges_on_step_start_callbacks_in_order() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_step_start(
            move |_event| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("settings");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_start(move |_event| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("call");
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["settings", "call"]);
    }

    #[test]
    fn tool_loop_agent_generate_on_step_start_passes_event_information() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextStepStartEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let runtime_context = JsonObject::from_iter([("tenant".to_string(), json!("acme"))]);
        let tools_context = JsonObject::from_iter([("weather".to_string(), json!("context"))]);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_runtime_context(runtime_context.clone())
                .with_tools_context(tools_context.clone())
                .with_model_settings(ToolLoopAgentModelSettings::new().with_temperature(0.25)),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_start(move |event| {
                let events = Rc::clone(&events_for_callback);
                async move {
                    events.borrow_mut().push(event);
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, "mock-provider");
        assert_eq!(event.model_id, "mock-model-id");
        assert_eq!(event.step_number, 0);
        assert_eq!(event.messages, vec![user_message("Hello")]);
        assert_eq!(event.runtime_context, runtime_context);
        assert_eq!(event.tools_context, tools_context);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_step_finish_from_constructor() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_step_finish(
            move |_step| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));

        let result =
            poll_ready(agent.generate("Hello, world!")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_step_finish_from_method() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello, world!").with_on_step_finish(
                move |_step| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_generate_merges_on_step_finish_callbacks_in_order() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_step_finish(
            move |_step| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("settings");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_finish(move |_step| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("call");
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["settings", "call"]);
    }

    #[test]
    fn tool_loop_agent_generate_on_step_finish_passes_step_result_to_callback() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let steps = Rc::new(RefCell::new(Vec::<GenerateTextStep>::new()));
        let steps_for_callback = Rc::clone(&steps);
        let runtime_context = JsonObject::from_iter([("tenant".to_string(), json!("acme"))]);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model).with_runtime_context(runtime_context.clone()),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_finish(move |step| {
                let steps = Rc::clone(&steps_for_callback);
                async move {
                    steps.borrow_mut().push(step);
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let steps = steps.borrow();
        assert_eq!(steps.len(), 1);
        let step = &steps[0];
        assert_eq!(step.step_number, 0);
        assert_eq!(step.text, "reply");
        assert_eq!(step.finish_reason, FinishReason::Stop);
        assert_eq!(step.model.provider, "mock-provider");
        assert_eq!(step.model.model_id, "mock-model-id");
        assert_eq!(step.runtime_context, runtime_context);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_finish_from_constructor() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_finish(
            move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_finish_from_method() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_finish(move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("method");
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_generate_merges_on_finish_callbacks_in_order() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_finish(
            move |_event| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_finish(move |_event| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("method");
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        assert_eq!(&*calls.borrow(), &["constructor", "method"]);
    }

    #[test]
    fn tool_loop_agent_generate_on_finish_passes_event_information() {
        let model = MockLanguageModel::new().with_generate_result(text_result("reply"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextFinishEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_finish(move |event| {
                let events = Rc::clone(&events_for_callback);
                async move {
                    events.borrow_mut().push(event);
                }
            });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "reply");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.text, "reply");
        assert_eq!(event.finish_reason, FinishReason::Stop);
        assert_eq!(event.steps.len(), 1);
        assert_eq!(event.step_number, 0);
        assert_eq!(event.total_usage, LanguageModelUsage::default());
    }

    #[test]
    fn tool_loop_agent_uses_upstream_twenty_step_default_for_tool_loop() {
        let model = MockLanguageModel::new()
            .with_generate_results([tool_call_result(), text_result("done")]);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(
                Tool::new("weather", object_schema()).with_execute(|_input, _options| async move {
                    Ok(json!({ "forecast": "sunny" }))
                }),
            ));

        let result =
            poll_ready(agent.generate("What is the weather?")).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(result.steps.len(), 2);
        assert_eq!(model.generate_calls().len(), 2);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_tool_execution_start_from_constructor() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_start(move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_tool_execution_start_from_method() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_start(
                move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_generate_merges_on_tool_execution_start_callbacks_in_order() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_start(move |_event| {
                    let calls = Rc::clone(&settings_calls);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_start(
                move |_event| {
                    let calls = Rc::clone(&call_calls);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor", "method"]);
    }

    #[test]
    fn tool_loop_agent_generate_on_tool_execution_start_passes_event_information() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let events = Rc::new(RefCell::new(
            Vec::<GenerateTextToolExecutionStartEvent>::new(),
        ));
        let events_for_callback = Rc::clone(&events);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_start(
                move |event| {
                    let events = Rc::clone(&events_for_callback);
                    async move {
                        events.borrow_mut().push(event);
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.call_id.starts_with("call"));
        assert_eq!(event.messages, vec![user_message("test")]);
        assert_eq!(event.tool_call.tool_call_id, "call-1");
        assert_eq!(event.tool_call.tool_name, "testTool");
        assert_eq!(event.tool_call.input, json!({ "value": "test" }));
        assert_eq!(event.tool_context, None);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_tool_execution_end_from_constructor() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_end(move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_on_tool_execution_end_from_method() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_end(
                move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_generate_merges_on_tool_execution_end_callbacks_in_order() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_end(move |_event| {
                    let calls = Rc::clone(&settings_calls);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_end(
                move |_event| {
                    let calls = Rc::clone(&call_calls);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor", "method"]);
    }

    #[test]
    fn tool_loop_agent_generate_on_tool_execution_end_passes_event_information_on_success() {
        let model = MockLanguageModel::new().with_generate_results([
            test_tool_call_result_with_input(r#"{ "value": "hello" }"#),
            text_result("done"),
        ]);
        let events = Rc::new(RefCell::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_end(
                move |event| {
                    let events = Rc::clone(&events_for_callback);
                    async move {
                        events.borrow_mut().push(event);
                    }
                },
            );

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.call_id.starts_with("call"));
        assert_eq!(event.messages, vec![user_message("test")]);
        assert_eq!(event.tool_call.tool_call_id, "call-1");
        assert_eq!(event.tool_call.tool_name, "testTool");
        assert_eq!(event.tool_call.input, json!({ "value": "hello" }));
        assert_eq!(event.tool_context, None);
        assert_eq!(event.tool_output.tool_call_id, "call-1");
        assert_eq!(event.tool_output.tool_name, "testTool");
        assert_eq!(event.tool_output.input, json!({ "value": "hello" }));
        assert_eq!(event.tool_output.output, json!("hello-result"));
    }

    #[test]
    fn tool_loop_agent_merges_tool_execution_callbacks_in_order() {
        let model = MockLanguageModel::new()
            .with_generate_results([tool_call_result(), text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_start_calls = Rc::clone(&calls);
        let settings_end_calls = Rc::clone(&calls);
        let call_start_calls = Rc::clone(&calls);
        let call_end_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(Tool::new("weather", object_schema()).with_execute(
                    |_input, _options| async move { Ok(json!({ "forecast": "sunny" })) },
                ))
                .with_on_tool_execution_start(move |_event| {
                    let calls = Rc::clone(&settings_start_calls);
                    async move {
                        calls.borrow_mut().push("settings-start");
                    }
                })
                .with_on_tool_execution_end(move |_event| {
                    let calls = Rc::clone(&settings_end_calls);
                    async move {
                        calls.borrow_mut().push("settings-end");
                    }
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("What is the weather?")
                .with_on_tool_execution_start(move |_event| {
                    let calls = Rc::clone(&call_start_calls);
                    async move {
                        calls.borrow_mut().push("call-start");
                    }
                })
                .with_on_tool_execution_end(move |_event| {
                    let calls = Rc::clone(&call_end_calls);
                    async move {
                        calls.borrow_mut().push("call-end");
                    }
                });

        let result = poll_ready(agent.generate(options)).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(
            &*calls.borrow(),
            &["settings-start", "call-start", "settings-end", "call-end"]
        );
    }

    #[test]
    fn tool_loop_agent_stream_delegates_to_stream_text() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_model_settings(ToolLoopAgentModelSettings::new().with_temperature(0.4)),
        );

        let result = poll_ready(agent.stream("Hello")).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(model.stream_calls()[0].temperature, Some(0.4));
    }

    #[test]
    fn tool_loop_agent_stream_passes_string_instructions() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model).with_instructions("INSTRUCTIONS"),
        );

        let result = poll_ready(agent.stream("Hello, world!")).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(
            model.stream_calls()[0].prompt,
            vec![
                system_message("INSTRUCTIONS"),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_stream_passes_system_message_instructions() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let provider_options = provider_options_value("test");
        let instructions = LanguageModelSystemMessage::new("INSTRUCTIONS")
            .with_provider_options(provider_options.clone());
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_instructions(instructions));

        let result = poll_ready(agent.stream("Hello, world!")).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(
            model.stream_calls()[0].prompt,
            vec![
                system_message_with_provider_options("INSTRUCTIONS", provider_options),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_stream_passes_abort_signal_to_stream_text() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let abort_controller = LanguageModelAbortController::new();
        let abort_signal = abort_controller.signal();
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_abort_signal(abort_signal.clone());

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        let calls = model.stream_calls();
        let captured_signal = calls[0]
            .abort_signal
            .as_ref()
            .expect("stream_text receives abort signal");
        assert!(captured_signal.is_same_signal(&abort_signal));
    }

    #[test]
    fn tool_loop_agent_stream_passes_timeout_to_tool_execution() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_tool_call_result(), stream_text_result("done")]);
        let received_signal = Arc::new(Mutex::new(None::<LanguageModelAbortSignal>));
        let received_signal_for_closure = Arc::clone(&received_signal);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(
            Tool::new("weather", object_schema()).with_execute(move |_input, options| {
                let received_signal = Arc::clone(&received_signal_for_closure);
                async move {
                    *received_signal.lock().expect("signal lock") = options.abort_signal;
                    Ok(json!({ "forecast": "sunny" }))
                }
            }),
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("What is the weather?").with_timeout(
                TimeoutConfigurationOptions::new().with_tool_timeout("weather", 10_000),
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        let captured_signal = received_signal
            .lock()
            .expect("signal lock")
            .clone()
            .expect("tool timeout creates abort signal");
        assert!(!captured_signal.is_aborted());
    }

    #[test]
    fn tool_loop_agent_stream_passes_sandbox_to_tool_execution() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_tool_call_result(), stream_text_result("done")]);
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("test sandbox"));
        let received_sandbox = Arc::new(Mutex::new(None::<Arc<dyn ExperimentalSandbox>>));
        let received_sandbox_for_closure = Arc::clone(&received_sandbox);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(
            Tool::new("weather", object_schema()).with_execute(move |_input, options| {
                let received_sandbox = Arc::clone(&received_sandbox_for_closure);
                async move {
                    *received_sandbox.lock().expect("sandbox lock") = options.experimental_sandbox;
                    Ok(json!({ "forecast": "sunny" }))
                }
            }),
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("What is the weather?")
                .with_experimental_sandbox(Arc::clone(&sandbox));

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        let received_sandbox = received_sandbox.lock().expect("sandbox lock");
        assert!(Arc::ptr_eq(
            received_sandbox
                .as_ref()
                .expect("tool execution receives sandbox"),
            &sandbox
        ));
    }

    #[test]
    fn tool_loop_agent_stream_honors_tool_approval() {
        let model = MockLanguageModel::new().with_stream_result(stream_test_tool_call_result());
        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_tool = Arc::clone(&executed);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(Tool::new("testTool", value_schema()).with_execute(
                    move |_input, _options| {
                        let executed = Arc::clone(&executed_for_tool);
                        async move {
                            executed.store(true, Ordering::SeqCst);
                            Ok(json!("tool-result"))
                        }
                    },
                ))
                .with_tool_approval(ToolApprovalConfiguration::new().with_tool_status(
                    "testTool",
                    crate::generate_text::ToolApprovalStatusKind::UserApproval,
                )),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(model.stream_calls().len(), 1);
        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert!(result.tool_results.is_empty());
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_call_id, "call-1");
        assert_eq!(result.tool_calls[0].tool_name, "testTool");
        assert!(result.parts.iter().any(|part| matches!(
            part,
            TextStreamPart::ToolApprovalRequest(request) if request.tool_call_id == "call-1"
        )));
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_tool_execution_start_from_constructor() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_start(move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_tool_execution_start_from_method() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_start(
                move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_stream_merges_on_tool_execution_start_callbacks_in_order() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_start(move |_event| {
                    let calls = Rc::clone(&settings_calls);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_start(
                move |_event| {
                    let calls = Rc::clone(&call_calls);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor", "method"]);
    }

    #[test]
    fn tool_loop_agent_stream_on_tool_execution_start_passes_event_information() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let events = Rc::new(RefCell::new(
            Vec::<GenerateTextToolExecutionStartEvent>::new(),
        ));
        let events_for_callback = Rc::clone(&events);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_start(
                move |event| {
                    let events = Rc::clone(&events_for_callback);
                    async move {
                        events.borrow_mut().push(event);
                    }
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.call_id.starts_with("call"));
        assert_eq!(event.messages, vec![user_message("test")]);
        assert_eq!(event.tool_call.tool_call_id, "call-1");
        assert_eq!(event.tool_call.tool_name, "testTool");
        assert_eq!(event.tool_call.input, json!({ "value": "test" }));
        assert_eq!(event.tool_context, None);
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_tool_execution_end_from_constructor() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_end(move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_tool_execution_end_from_method() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_end(
                move |_event| {
                    let calls = Rc::clone(&calls_for_callback);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_stream_merges_on_tool_execution_end_callbacks_in_order() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_on_tool_execution_end(move |_event| {
                    let calls = Rc::clone(&settings_calls);
                    async move {
                        calls.borrow_mut().push("constructor");
                    }
                }),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_end(
                move |_event| {
                    let calls = Rc::clone(&call_calls);
                    async move {
                        calls.borrow_mut().push("method");
                    }
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(&*calls.borrow(), &["constructor", "method"]);
    }

    #[test]
    fn tool_loop_agent_stream_on_tool_execution_end_passes_event_information_on_success() {
        let model = MockLanguageModel::new().with_stream_results([
            stream_test_tool_call_result_with_input(r#"{ "value": "hello" }"#),
            stream_text_result("done"),
        ]);
        let events = Rc::new(RefCell::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let agent =
            ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(test_value_tool()));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_tool_execution_end(
                move |event| {
                    let events = Rc::clone(&events_for_callback);
                    async move {
                        events.borrow_mut().push(event);
                    }
                },
            );

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.call_id.starts_with("call"));
        assert_eq!(event.messages, vec![user_message("test")]);
        assert_eq!(event.tool_call.tool_call_id, "call-1");
        assert_eq!(event.tool_call.tool_name, "testTool");
        assert_eq!(event.tool_call.input, json!({ "value": "hello" }));
        assert_eq!(event.tool_context, None);
        assert_eq!(event.tool_output.tool_call_id, "call-1");
        assert_eq!(event.tool_output.tool_name, "testTool");
        assert_eq!(event.tool_output.input, json!({ "value": "hello" }));
        assert_eq!(event.tool_output.output, json!("hello-result"));
    }

    #[test]
    fn create_agent_ui_stream_response_uses_tool_model_output_for_ui_tool_results() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("Done"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_tool(
            Tool::new("example", value_schema()).with_to_model_output(|options| async move {
                let value = options
                    .output
                    .get("value")
                    .and_then(JsonValue::as_str)
                    .expect("tool output contains value");
                LanguageModelToolResultOutput::content(vec![
                    LanguageModelToolResultContentPart::Text(LanguageModelTextPart::new(
                        value.to_string(),
                    )),
                ])
            }),
        ));
        let ui_messages = vec![
            UiMessage::new("msg-1", crate::ui_message_stream::UiMessageRole::User).with_part(
                json!({
                    "type": "text",
                    "text": "Hello, world!"
                }),
            ),
            UiMessage::new("msg-2", crate::ui_message_stream::UiMessageRole::Assistant).with_part(
                json!({
                    "type": "tool-example",
                    "toolCallId": "call-1",
                    "state": "output-available",
                    "input": { "input": "Hello, world!" },
                    "output": { "value": "Example tool: Hello, world!" }
                }),
            ),
        ];

        let response = poll_ready(create_agent_ui_stream_response(
            AgentUiStreamResponseOptions::new(&agent, ui_messages),
        ))
        .expect("agent UI stream response succeeds");

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].prompt.len(), 3);
        match &calls[0].prompt[2] {
            LanguageModelMessage::Tool(message) => {
                assert_eq!(message.content.len(), 1);
                match &message.content[0] {
                    LanguageModelToolContentPart::ToolResult(result) => {
                        assert_eq!(result.tool_call_id, "call-1");
                        assert_eq!(result.tool_name, "example");
                        assert!(matches!(
                            &result.output,
                            LanguageModelToolResultOutput::Content { value }
                                if value == &vec![LanguageModelToolResultContentPart::Text(
                                    LanguageModelTextPart::new("Example tool: Hello, world!")
                                )]
                        ));
                    }
                    other => panic!("expected tool result, got {other:?}"),
                }
            }
            other => panic!("expected tool message, got {other:?}"),
        }

        let decoded = response.decoded_body().expect("response body decodes");
        assert!(decoded.iter().any(|chunk| {
            chunk.contains(r#""type":"start""#) && chunk.contains(r#""messageId":"msg-2""#)
        }));
        assert!(
            decoded
                .iter()
                .any(|chunk| chunk.contains(r#""type":"finish""#))
        );
    }

    #[test]
    fn create_agent_ui_stream_response_calls_on_finish_with_auto_original_messages() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("Done"));
        let agent = ToolLoopAgent::for_model(&model);
        let finish_events = Arc::new(Mutex::new(Vec::<
            crate::ui_message_stream::UiMessageStreamFinishCallbackEvent,
        >::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);
        let ui_messages = vec![
            UiMessage::new("msg-1", crate::ui_message_stream::UiMessageRole::User).with_part(
                json!({
                    "type": "text",
                    "text": "Run test"
                }),
            ),
            UiMessage::new("msg-2", crate::ui_message_stream::UiMessageRole::Assistant).with_part(
                json!({
                    "type": "tool-testTool",
                    "toolCallId": "call-1",
                    "state": "output-available",
                    "input": { "value": "test" },
                    "output": { "result": "success" }
                }),
            ),
        ];

        let response = poll_ready(create_agent_ui_stream_response(
            AgentUiStreamResponseOptions::new(&agent, ui_messages).with_ui_message_stream_options(
                StreamTextUiMessageStreamOptions::new().with_on_finish(move |event| {
                    finish_events_for_callback
                        .lock()
                        .expect("finish events lock")
                        .push(event);
                }),
            ),
        ))
        .expect("agent UI stream response succeeds");

        assert!(
            response
                .decoded_body()
                .expect("response body decodes")
                .iter()
                .any(|chunk| chunk.contains(r#""type":"finish""#))
        );
        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert_eq!(finish_events[0].messages.len(), 2);
        assert_eq!(finish_events[0].messages[0].id, "msg-1");
        assert_eq!(finish_events[0].response_message.id, "msg-2");
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_start_from_constructor() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_start(
            move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));

        let result = poll_ready(agent.stream("Hello, world!")).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_start_from_method() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello, world!").with_on_start(move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("method");
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_stream_on_start_passes_event_information() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextStartEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let runtime_context = JsonObject::from_iter([("userId".to_string(), json!("test-user"))]);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_instructions("You are a helpful assistant")
                .with_runtime_context(runtime_context.clone())
                .with_model_settings(
                    ToolLoopAgentModelSettings::new()
                        .with_temperature(0.7)
                        .with_max_output_tokens(500),
                ),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello, world!").with_on_start(move |event| {
                let events = Rc::clone(&events_for_callback);
                async move {
                    events.borrow_mut().push(event);
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, "mock-provider");
        assert_eq!(event.model_id, "mock-model-id");
        assert_eq!(event.temperature, Some(0.7));
        assert_eq!(event.max_output_tokens, Some(500));
        assert_eq!(event.runtime_context, runtime_context);
        assert_eq!(
            event.messages,
            vec![
                system_message("You are a helpful assistant"),
                user_message("Hello, world!")
            ]
        );
    }

    #[test]
    fn tool_loop_agent_stream_merges_on_step_start_callbacks_in_order() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_step_start(
            move |_event| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("settings");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_start(move |_event| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("call");
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["settings", "call"]);
    }

    #[test]
    fn tool_loop_agent_stream_on_step_start_passes_event_information() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextStepStartEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let runtime_context = JsonObject::from_iter([("tenant".to_string(), json!("acme"))]);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_runtime_context(runtime_context.clone())
                .with_model_settings(ToolLoopAgentModelSettings::new().with_temperature(0.25)),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_start(move |event| {
                let events = Rc::clone(&events_for_callback);
                async move {
                    events.borrow_mut().push(event);
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, "mock-provider");
        assert_eq!(event.model_id, "mock-model-id");
        assert_eq!(event.step_number, 0);
        assert_eq!(event.messages, vec![user_message("Hello")]);
        assert_eq!(event.runtime_context, runtime_context);
    }

    #[test]
    fn tool_loop_agent_stream_merges_on_step_finish_callbacks_in_order() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_step_finish(
            move |_step| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("settings");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_finish(move |_step| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("call");
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["settings", "call"]);
    }

    #[test]
    fn tool_loop_agent_stream_on_step_finish_passes_step_result_to_callback() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let steps = Rc::new(RefCell::new(Vec::<GenerateTextStep>::new()));
        let steps_for_callback = Rc::clone(&steps);
        let runtime_context = JsonObject::from_iter([("tenant".to_string(), json!("acme"))]);
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model).with_runtime_context(runtime_context.clone()),
        );
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_step_finish(move |step| {
                let steps = Rc::clone(&steps_for_callback);
                async move {
                    steps.borrow_mut().push(step);
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        let steps = steps.borrow();
        assert_eq!(steps.len(), 1);
        let step = &steps[0];
        assert_eq!(step.step_number, 0);
        assert_eq!(step.text, "hello");
        assert_eq!(step.finish_reason, FinishReason::Stop);
        assert_eq!(step.model.provider, "mock-provider");
        assert_eq!(step.model.model_id, "mock-model-id");
        assert_eq!(step.runtime_context, runtime_context);
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_finish_from_constructor() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_finish(
            move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("constructor");
                }
            },
        ));

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["constructor"]);
    }

    #[test]
    fn tool_loop_agent_stream_calls_on_finish_from_method() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls_for_callback = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_finish(move |_event| {
                let calls = Rc::clone(&calls_for_callback);
                async move {
                    calls.borrow_mut().push("method");
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["method"]);
    }

    #[test]
    fn tool_loop_agent_stream_on_finish_passes_event_information() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let events = Rc::new(RefCell::new(Vec::<GenerateTextFinishEvent>::new()));
        let events_for_callback = Rc::clone(&events);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("test").with_on_finish(move |event| {
                let events = Rc::clone(&events_for_callback);
                async move {
                    events.borrow_mut().push(event);
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.text, "hello");
        assert_eq!(event.finish_reason, FinishReason::Stop);
        assert_eq!(event.steps.len(), 1);
        assert_eq!(event.step_number, 0);
        assert_eq!(event.total_usage, LanguageModelUsage::default());
    }

    #[test]
    fn tool_loop_agent_merges_stream_finish_callbacks_in_order() {
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("hello"));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let settings_calls = Rc::clone(&calls);
        let call_calls = Rc::clone(&calls);
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model).with_on_finish(
            move |_event| {
                let calls = Rc::clone(&settings_calls);
                async move {
                    calls.borrow_mut().push("settings");
                }
            },
        ));
        let options: ToolLoopAgentCallOptions<'_, MockLanguageModel> =
            ToolLoopAgentCallOptions::from_prompt("Hello").with_on_finish(move |_event| {
                let calls = Rc::clone(&call_calls);
                async move {
                    calls.borrow_mut().push("call");
                }
            });

        let result = poll_ready(agent.stream(options)).expect("agent stream succeeds");

        assert_eq!(result.text, "hello");
        assert_eq!(&*calls.borrow(), &["settings", "call"]);
    }

    #[test]
    fn tool_loop_agent_generate_calls_per_call_integration_listeners_for_all_lifecycle_events() {
        let model = MockLanguageModel::new()
            .with_generate_results([test_tool_call_result(), text_result("done")]);
        let events = Arc::new(Mutex::new(Vec::new()));
        let integration = agent_lifecycle_integration(Arc::clone(&events));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(
            &*events.lock().expect("event lock"),
            &[
                "onStart",
                "onStepStart",
                "onToolExecutionStart",
                "onToolExecutionEnd",
                "onStepFinish",
                "onStepStart",
                "onStepFinish",
                "onEnd",
            ]
        );
    }

    #[test]
    fn tool_loop_agent_stream_calls_per_call_integration_listeners_for_all_lifecycle_events() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_test_tool_call_result(), stream_text_result("done")]);
        let events = Arc::new(Mutex::new(Vec::new()));
        let integration = agent_lifecycle_integration(Arc::clone(&events));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_tool(test_value_tool())
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "done");
        assert_eq!(
            &*events.lock().expect("event lock"),
            &[
                "onStart",
                "onStepStart",
                "onToolExecutionStart",
                "onToolExecutionEnd",
                "onStepFinish",
                "onStepStart",
                "onStepFinish",
                "onEnd",
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_calls_globally_registered_integration_listeners() {
        let _guard = telemetry_test_guard_for_tests();
        reset_telemetry_state_for_tests();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut integration = TelemetryIntegration::new();
        for (kind, label) in [
            (TelemetryEventKind::OnStart, "global-onStart"),
            (TelemetryEventKind::OnStepFinish, "global-onStepFinish"),
            (TelemetryEventKind::OnEnd, "global-onEnd"),
        ] {
            let events = Arc::clone(&events);
            integration = integration.with_callback(kind, move |_event| {
                events.lock().expect("event lock").push(label);
            });
        }
        register_telemetry_integration(integration);
        let model = MockLanguageModel::new().with_generate_result(text_result("Hello!"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "Hello!");
        let events = events.lock().expect("event lock");
        assert!(events.contains(&"global-onStart"));
        assert!(events.contains(&"global-onStepFinish"));
        assert!(events.contains(&"global-onEnd"));
        reset_telemetry_state_for_tests();
    }

    #[test]
    fn tool_loop_agent_stream_calls_globally_registered_integration_listeners() {
        let _guard = telemetry_test_guard_for_tests();
        reset_telemetry_state_for_tests();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut integration = TelemetryIntegration::new();
        for (kind, label) in [
            (TelemetryEventKind::OnStart, "global-onStart"),
            (TelemetryEventKind::OnStepFinish, "global-onStepFinish"),
            (TelemetryEventKind::OnEnd, "global-onEnd"),
        ] {
            let events = Arc::clone(&events);
            integration = integration.with_callback(kind, move |_event| {
                events.lock().expect("event lock").push(label);
            });
        }
        register_telemetry_integration(integration);
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("Hello!"));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "Hello!");
        let events = events.lock().expect("event lock");
        assert!(events.contains(&"global-onStart"));
        assert!(events.contains(&"global-onStepFinish"));
        assert!(events.contains(&"global-onEnd"));
        reset_telemetry_state_for_tests();
    }

    #[test]
    fn tool_loop_agent_generate_includes_configured_runtime_context_properties_in_telemetry() {
        let callback_contexts = Rc::new(RefCell::new(Vec::<JsonValue>::new()));
        let telemetry_contexts = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let callback_contexts_for_start = Rc::clone(&callback_contexts);
        let callback_contexts_for_step_finish = Rc::clone(&callback_contexts);
        let callback_contexts_for_finish = Rc::clone(&callback_contexts);
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let telemetry_contexts = Arc::clone(&telemetry_contexts);
            integration = integration.with_callback(kind, move |event: TelemetryEvent| {
                telemetry_contexts
                    .lock()
                    .expect("event lock")
                    .push(event.event["runtimeContext"].clone());
            });
        }
        let runtime_context = JsonObject::from_iter([
            ("userId".to_string(), json!("user-123")),
            ("requestId".to_string(), json!("request-123")),
        ]);
        let model = MockLanguageModel::new().with_generate_result(text_result("Hello!"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_runtime_context(runtime_context.clone())
                .with_on_start(move |event| {
                    let callback_contexts = Rc::clone(&callback_contexts_for_start);
                    async move {
                        callback_contexts
                            .borrow_mut()
                            .push(json!(event.runtime_context));
                    }
                })
                .with_on_step_finish(move |step| {
                    let callback_contexts = Rc::clone(&callback_contexts_for_step_finish);
                    async move {
                        callback_contexts
                            .borrow_mut()
                            .push(json!(step.runtime_context));
                    }
                })
                .with_on_finish(move |event| {
                    let callback_contexts = Rc::clone(&callback_contexts_for_finish);
                    async move {
                        callback_contexts
                            .borrow_mut()
                            .push(json!(event.runtime_context));
                    }
                })
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_runtime_context_key("requestId", true)
                        .with_integration(integration),
                ),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "Hello!");
        assert_eq!(
            &*callback_contexts.borrow(),
            &[
                json!(runtime_context),
                json!(runtime_context),
                json!(runtime_context),
            ]
        );
        assert_eq!(
            &*telemetry_contexts.lock().expect("event lock"),
            &[
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
            ]
        );
    }

    #[test]
    fn tool_loop_agent_stream_includes_configured_runtime_context_properties_in_telemetry() {
        let callback_contexts = Rc::new(RefCell::new(Vec::<JsonValue>::new()));
        let telemetry_contexts = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let callback_contexts_for_start = Rc::clone(&callback_contexts);
        let callback_contexts_for_step_finish = Rc::clone(&callback_contexts);
        let callback_contexts_for_finish = Rc::clone(&callback_contexts);
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let telemetry_contexts = Arc::clone(&telemetry_contexts);
            integration = integration.with_callback(kind, move |event: TelemetryEvent| {
                telemetry_contexts
                    .lock()
                    .expect("event lock")
                    .push(event.event["runtimeContext"].clone());
            });
        }
        let runtime_context = JsonObject::from_iter([
            ("userId".to_string(), json!("user-123")),
            ("requestId".to_string(), json!("request-123")),
        ]);
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("Hello!"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_runtime_context(runtime_context.clone())
                .with_on_start(move |event| {
                    let callback_contexts = Rc::clone(&callback_contexts_for_start);
                    async move {
                        callback_contexts
                            .borrow_mut()
                            .push(json!(event.runtime_context));
                    }
                })
                .with_on_step_finish(move |step| {
                    let callback_contexts = Rc::clone(&callback_contexts_for_step_finish);
                    async move {
                        callback_contexts
                            .borrow_mut()
                            .push(json!(step.runtime_context));
                    }
                })
                .with_on_finish(move |event| {
                    let callback_contexts = Rc::clone(&callback_contexts_for_finish);
                    async move {
                        callback_contexts
                            .borrow_mut()
                            .push(json!(event.runtime_context));
                    }
                })
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_runtime_context_key("requestId", true)
                        .with_integration(integration),
                ),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "Hello!");
        assert_eq!(
            &*callback_contexts.borrow(),
            &[
                json!(runtime_context),
                json!(runtime_context),
                json!(runtime_context),
            ]
        );
        assert_eq!(
            &*telemetry_contexts.lock().expect("event lock"),
            &[
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_calls_integration_listeners_alongside_agent_callbacks() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let agent_start_events = Arc::clone(&events);
        let agent_step_finish_events = Arc::clone(&events);
        let agent_finish_events = Arc::clone(&events);
        let mut integration = TelemetryIntegration::new();
        for (kind, label) in [
            (TelemetryEventKind::OnStart, "integration-onStart"),
            (TelemetryEventKind::OnStepFinish, "integration-onStepFinish"),
            (TelemetryEventKind::OnEnd, "integration-onEnd"),
        ] {
            let events = Arc::clone(&events);
            integration = integration.with_callback(kind, move |_event| {
                events.lock().expect("event lock").push(label);
            });
        }
        let model = MockLanguageModel::new().with_generate_result(text_result("Hello!"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_on_start(move |_event| {
                    let events = Arc::clone(&agent_start_events);
                    async move {
                        events.lock().expect("event lock").push("agent-onStart");
                    }
                })
                .with_on_step_finish(move |_step| {
                    let events = Arc::clone(&agent_step_finish_events);
                    async move {
                        events
                            .lock()
                            .expect("event lock")
                            .push("agent-onStepFinish");
                    }
                })
                .with_on_finish(move |_event| {
                    let events = Arc::clone(&agent_finish_events);
                    async move {
                        events.lock().expect("event lock").push("agent-onFinish");
                    }
                })
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "Hello!");
        assert_eq!(
            &*events.lock().expect("event lock"),
            &[
                "agent-onStart",
                "integration-onStart",
                "agent-onStepFinish",
                "integration-onStepFinish",
                "agent-onFinish",
                "integration-onEnd",
            ]
        );
    }

    #[test]
    fn tool_loop_agent_stream_calls_integration_listeners_alongside_agent_callbacks() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let agent_start_events = Arc::clone(&events);
        let agent_step_finish_events = Arc::clone(&events);
        let agent_finish_events = Arc::clone(&events);
        let mut integration = TelemetryIntegration::new();
        for (kind, label) in [
            (TelemetryEventKind::OnStart, "integration-onStart"),
            (TelemetryEventKind::OnStepFinish, "integration-onStepFinish"),
            (TelemetryEventKind::OnEnd, "integration-onEnd"),
        ] {
            let events = Arc::clone(&events);
            integration = integration.with_callback(kind, move |_event| {
                events.lock().expect("event lock").push(label);
            });
        }
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("Hello!"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_on_start(move |_event| {
                    let events = Arc::clone(&agent_start_events);
                    async move {
                        events.lock().expect("event lock").push("agent-onStart");
                    }
                })
                .with_on_step_finish(move |_step| {
                    let events = Arc::clone(&agent_step_finish_events);
                    async move {
                        events
                            .lock()
                            .expect("event lock")
                            .push("agent-onStepFinish");
                    }
                })
                .with_on_finish(move |_event| {
                    let events = Arc::clone(&agent_finish_events);
                    async move {
                        events.lock().expect("event lock").push("agent-onFinish");
                    }
                })
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "Hello!");
        assert_eq!(
            &*events.lock().expect("event lock"),
            &[
                "agent-onStart",
                "integration-onStart",
                "agent-onStepFinish",
                "integration-onStepFinish",
                "agent-onFinish",
                "integration-onEnd",
            ]
        );
    }

    #[test]
    fn tool_loop_agent_generate_does_not_break_when_an_integration_listener_panics() {
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            integration = integration.with_callback(kind, move |_event| {
                panic!("integration error");
            });
        }
        let model = MockLanguageModel::new().with_generate_result(text_result("Hello!"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        );

        let result = poll_ready(agent.generate("test")).expect("agent generation succeeds");

        assert_eq!(result.text, "Hello!");
    }

    #[test]
    fn tool_loop_agent_stream_does_not_break_when_an_integration_listener_panics() {
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            integration = integration.with_callback(kind, move |_event| {
                panic!("integration error");
            });
        }
        let model = MockLanguageModel::new().with_stream_result(stream_text_result("Hello!"));
        let agent = ToolLoopAgent::new(
            ToolLoopAgentSettings::new(&model)
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        );

        let result = poll_ready(agent.stream("test")).expect("agent stream succeeds");

        assert_eq!(result.text, "Hello!");
    }
}
