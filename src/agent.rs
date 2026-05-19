use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use crate::generate_text::{
    ActiveTools, GenerateTextFinishEvent, GenerateTextInclude, GenerateTextOnFinish,
    GenerateTextOnStart, GenerateTextOnStepFinish, GenerateTextOnStepStart,
    GenerateTextOnToolExecutionEnd, GenerateTextOnToolExecutionStart, GenerateTextOptions,
    GenerateTextResult, GenerateTextStartEvent, GenerateTextStep, GenerateTextStepStartEvent,
    GenerateTextTool, GenerateTextToolExecutionEndEvent, GenerateTextToolExecutionStartEvent,
    StopCondition, ToolApprovalConfiguration, ToolCallRepair, ToolInputRefinement, generate_text,
};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    LanguageModel, LanguageModelCallOptions, LanguageModelReasoningEffort,
    LanguageModelResponseFormat, LanguageModelStreamPart, LanguageModelToolChoice,
};
use crate::prompt::{Instructions, Prompt, PromptInput};
use crate::provider::{InvalidPromptError, ProviderOptions};
use crate::provider_utils::ExperimentalSandbox;
use crate::stream_text::{StreamTextOptions, StreamTextResult, stream_text};
use crate::telemetry::TelemetryOptions;

/// Upstream version tag for `ToolLoopAgent`.
pub const TOOL_LOOP_AGENT_VERSION: &str = "agent-v1";

/// Agent implementation that delegates each call to `generate_text` or `stream_text`.
///
/// This ports the portable core of upstream `ToolLoopAgent`: shared settings,
/// call preparation, default twenty-step tool loops, non-streaming generation,
/// and streaming generation. JavaScript-only abort signal and timeout handling
/// remain intentionally outside the Rust surface.
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
        let prepared = self.prepare_call(options.into());
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
        let prepared = self.prepare_call(options.into());
        let options = stream_options_from_prepared(prepared)?;
        Ok(stream_text(options).await)
    }

    fn prepare_call(
        &self,
        options: ToolLoopAgentCallOptions<'a, M>,
    ) -> ToolLoopAgentPreparedCall<'a, M> {
        let mut prepared = ToolLoopAgentPreparedCall {
            model: options.model.unwrap_or(self.settings.model),
            prompt: options.prompt,
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

        prepared
    }
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
            experimental_sandbox: None,
            active_tools: None,
            tool_approval: None,
            tool_input_refinements: BTreeMap::new(),
            tool_call_repair: None,
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
    pub on_start: Option<GenerateTextOnStart<'a>>,
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,
    pub on_finish: Option<GenerateTextOnFinish<'a>>,
    pub telemetry: Option<TelemetryOptions>,
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
            on_start: None,
            on_step_start: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
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
    pub on_start: Option<GenerateTextOnStart<'a>>,
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,
    pub on_finish: Option<GenerateTextOnFinish<'a>>,
    pub telemetry: Option<TelemetryOptions>,
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
    options.on_start = prepared.on_start;
    options.on_step_start = prepared.on_step_start;
    options.on_tool_execution_start = prepared.on_tool_execution_start;
    options.on_tool_execution_end = prepared.on_tool_execution_end;
    options.on_step_finish = prepared.on_step_finish;
    options.on_finish = prepared.on_finish;
    options.telemetry = prepared.telemetry;
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
    options.on_start = prepared.on_start;
    options.on_step_start = prepared.on_step_start;
    options.on_tool_execution_start = prepared.on_tool_execution_start;
    options.on_tool_execution_end = prepared.on_tool_execution_end;
    options.on_step_finish = prepared.on_step_finish;
    options.on_finish = prepared.on_finish;
    options.telemetry = prepared.telemetry;
    options.max_steps = prepared.max_steps.max(1);
    options.stop_conditions = prepared.stop_conditions;
    options
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
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::*;
    use crate::language_model::{
        FinishReason, LanguageModelContent, LanguageModelFinishReason, LanguageModelGenerateResult,
        LanguageModelStreamFinish, LanguageModelStreamPart, LanguageModelStreamResult,
        LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
        LanguageModelToolCall, LanguageModelUsage,
    };
    use crate::mock_models::MockLanguageModel;
    use crate::provider_utils::Tool;

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
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    LanguageModelUsage::default(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("stop".to_string()),
                    },
                )),
            ]));
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
    fn tool_loop_agent_merges_stream_finish_callbacks_in_order() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    LanguageModelUsage::default(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("stop".to_string()),
                    },
                )),
            ]));
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
}
