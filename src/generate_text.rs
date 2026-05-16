use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

use crate::file_data::FileData;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelCustomPart, LanguageModelFile, LanguageModelFileData, LanguageModelFilePart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelReasoning, LanguageModelReasoningEffort,
    LanguageModelReasoningFile, LanguageModelReasoningFilePart, LanguageModelReasoningPart,
    LanguageModelRequest, LanguageModelResponse, LanguageModelResponseFormat, LanguageModelSource,
    LanguageModelText, LanguageModelTextPart, LanguageModelTool, LanguageModelToolCall,
    LanguageModelToolCallPart, LanguageModelToolChoice, LanguageModelToolContentPart,
    LanguageModelToolMessage, LanguageModelToolResult, LanguageModelToolResultOutput,
    LanguageModelToolResultPart, LanguageModelUsage, OutputTokenUsage,
};
use crate::provider::JsonParseError;
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::provider_utils::{Tool, ToolExecutionOptions, prepare_tools};
use crate::warning::Warning;

const DEFAULT_MAX_STEPS: usize = 1;

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

/// Reasoning content emitted during a high-level generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GenerateTextReasoning {
    /// Text reasoning emitted by the model.
    Reasoning(LanguageModelReasoning),

    /// File emitted by the model as part of reasoning.
    ReasoningFile(LanguageModelReasoningFile),
}

/// Tool input accepted by [`GenerateTextOptions::with_tool`].
#[derive(Clone, Debug)]
pub enum GenerateTextTool {
    /// High-level Rust function tool.
    Rust(Tool),

    /// Already prepared provider-facing language model tool.
    LanguageModel(LanguageModelTool),
}

impl From<Tool> for GenerateTextTool {
    fn from(tool: Tool) -> Self {
        Self::Rust(tool)
    }
}

impl From<LanguageModelTool> for GenerateTextTool {
    fn from(tool: LanguageModelTool) -> Self {
        Self::LanguageModel(tool)
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

    /// Maximum number of model-call steps to run.
    pub max_steps: usize,
}

impl<'a, M: LanguageModel + ?Sized> GenerateTextOptions<'a, M> {
    /// Creates generation options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self {
            model,
            call_options: LanguageModelCallOptions::new(prompt),
            tools: Vec::new(),
            max_steps: DEFAULT_MAX_STEPS,
        }
    }

    /// Creates generation options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, call_options: LanguageModelCallOptions) -> Self {
        Self {
            model,
            call_options,
            tools: Vec::new(),
            max_steps: DEFAULT_MAX_STEPS,
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
            GenerateTextTool::Rust(tool) => self.tools.push(tool),
            GenerateTextTool::LanguageModel(tool) => self
                .call_options
                .tools
                .get_or_insert_with(Vec::new)
                .push(tool),
        }

        self
    }

    /// Sets the tool selection strategy.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.call_options.tool_choice = Some(tool_choice);
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
    fn from_language_model_tool_call(tool_call: &LanguageModelToolCall) -> Self {
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
            is_error: Some(true),
            provider_executed: tool_call.provider_executed,
            dynamic: tool_call.dynamic,
            preliminary: None,
            provider_metadata: tool_call.provider_metadata.clone(),
            tool_metadata: tool_call.tool_metadata.clone(),
        }
    }
}

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

/// Result of a single non-streaming generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextStep {
    /// Zero-based index of this step.
    pub step_number: usize,

    /// Model that produced this step.
    pub model: GenerateTextModelInfo,

    /// Content generated in this step.
    pub content: Vec<LanguageModelContent>,

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
    pub files: Vec<LanguageModelFile>,

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
    fn from_language_model_result(
        step_number: usize,
        model: GenerateTextModelInfo,
        result: LanguageModelGenerateResult,
    ) -> Self {
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

        Self {
            step_number,
            model,
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
            warnings,
            request,
            response,
            provider_metadata,
        }
    }
}

/// Result of a high-level non-streaming text generation call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextResult {
    /// Content generated across all steps.
    pub content: Vec<LanguageModelContent>,

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
    pub files: Vec<LanguageModelFile>,

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
}

impl GenerateTextResult {
    fn from_steps(steps: Vec<GenerateTextStep>) -> Self {
        let final_step = steps
            .last()
            .expect("generate_text always creates at least one step");
        let total_usage = add_step_usage(&steps);

        Self {
            content: steps
                .iter()
                .flat_map(|step| step.content.iter().cloned())
                .collect(),
            text: final_step.text.clone(),
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
            steps,
        }
    }

    /// Returns the final step, when the result contains at least one step.
    pub fn final_step(&self) -> Option<&GenerateTextStep> {
        self.steps.last()
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
        max_steps,
    } = options;
    let model_info = GenerateTextModelInfo::new(model.provider(), model.model_id());

    if let Some(mut prepared_tools) = prepare_tools(&tools) {
        call_options
            .tools
            .get_or_insert_with(Vec::new)
            .append(&mut prepared_tools);
    }

    let max_steps = max_steps.max(1);
    let mut steps = Vec::new();

    for step_number in 0..max_steps {
        let step_prompt = call_options.prompt.clone();
        let result = model.do_generate(call_options.clone()).await;
        let mut step =
            GenerateTextStep::from_language_model_result(step_number, model_info.clone(), result);
        mark_unavailable_tool_calls(&mut step.tool_calls, call_options.tools.as_deref());
        mark_runtime_dynamic_tool_calls(&mut step.tool_calls, &tools);
        mark_tool_call_titles(&mut step.tool_calls, &tools);
        mark_tool_call_metadata(&mut step.tool_calls, &tools);
        mark_tool_result_metadata(&mut step.tool_results, &step.tool_calls, &tools);
        refresh_tool_call_views(&mut step);
        let tool_results = execute_tool_calls(&tools, &step.tool_calls, &step_prompt).await;
        let should_continue = should_continue_after_tool_results(&step, &tool_results);
        step.tool_results.extend(tool_results);
        mark_tool_result_metadata(&mut step.tool_results, &step.tool_calls, &tools);
        refresh_tool_result_views(&mut step);
        step.response_messages = response_messages_for_step(&step).unwrap_or_default();

        if should_continue && step_number + 1 < max_steps {
            if step.response_messages.is_empty() {
                steps.push(step);
                break;
            } else {
                call_options.prompt.extend(step.response_messages.clone());
            }

            steps.push(step);
        } else {
            steps.push(step);
            break;
        }
    }

    GenerateTextResult::from_steps(steps)
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

fn refresh_tool_call_views(step: &mut GenerateTextStep) {
    step.static_tool_calls = static_tool_calls(&step.tool_calls);
    step.dynamic_tool_calls = dynamic_tool_calls(&step.tool_calls);
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

fn extract_files(content: &[LanguageModelContent]) -> Vec<LanguageModelFile> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::File(file) => Some(file.clone()),
            _ => None,
        })
        .collect()
}

fn extract_reasoning(content: &[LanguageModelContent]) -> Vec<GenerateTextReasoning> {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::Reasoning(reasoning) => {
                Some(GenerateTextReasoning::Reasoning(reasoning.clone()))
            }
            LanguageModelContent::ReasoningFile(file) => {
                Some(GenerateTextReasoning::ReasoningFile(file.clone()))
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

fn refresh_tool_result_views(step: &mut GenerateTextStep) {
    step.static_tool_results = static_tool_results(&step.tool_results);
    step.dynamic_tool_results = dynamic_tool_results(&step.tool_results);
}

async fn execute_tool_calls(
    tools: &[Tool],
    tool_calls: &[GenerateTextToolCall],
    messages: &LanguageModelPrompt,
) -> Vec<GenerateTextToolResult> {
    let mut tool_results = Vec::new();

    for tool_call in tool_calls {
        if tool_call.provider_executed == Some(true) {
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

        let Some(execute) = tool.execute(
            tool_call.input.clone(),
            ToolExecutionOptions::new(tool_call.tool_call_id.clone(), messages.clone()),
        ) else {
            continue;
        };

        match execute.await {
            Ok(output) => tool_results.push(GenerateTextToolResult::success(tool_call, output)),
            Err(error) => {
                tool_results.push(GenerateTextToolResult::error(
                    tool_call,
                    error.into_message(),
                ));
            }
        }
    }

    tool_results
}

fn should_continue_after_tool_results(
    step: &GenerateTextStep,
    tool_results: &[GenerateTextToolResult],
) -> bool {
    let client_tool_call_count = step
        .tool_calls
        .iter()
        .filter(|tool_call| tool_call.provider_executed != Some(true))
        .count();

    client_tool_call_count > 0 && tool_results.len() == client_tool_call_count
}

fn response_messages_for_step(step: &GenerateTextStep) -> Option<Vec<LanguageModelMessage>> {
    let mut messages = Vec::new();

    if let Some(message) = assistant_message_from_step(step) {
        messages.push(message);
    }

    let client_tool_results = step
        .tool_results
        .iter()
        .filter(|tool_result| tool_result.provider_executed != Some(true))
        .cloned()
        .collect::<Vec<_>>();

    if let Some(message) = tool_message_from_results(&client_tool_results) {
        messages.push(message);
    }

    if messages.is_empty() {
        None
    } else {
        Some(messages)
    }
}

fn assistant_message_from_step(step: &GenerateTextStep) -> Option<LanguageModelMessage> {
    let parts = step
        .content
        .iter()
        .filter_map(|content| assistant_content_part_from_content(content, &step.tool_calls))
        .collect::<Vec<_>>();

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
        LanguageModelContent::ToolApprovalRequest(_) | LanguageModelContent::Source(_) => None,
    }
}

fn tool_message_from_results(
    tool_results: &[GenerateTextToolResult],
) -> Option<LanguageModelMessage> {
    let content = tool_results
        .iter()
        .map(|tool_result| {
            let mut part = LanguageModelToolResultPart::new(
                tool_result.tool_call_id.clone(),
                tool_result.tool_name.clone(),
                tool_result_output(tool_result),
            );

            if let Some(provider_metadata) = &tool_result.provider_metadata {
                part = part.with_provider_options(provider_metadata.clone());
            }

            LanguageModelToolContentPart::ToolResult(part)
        })
        .collect::<Vec<_>>();

    if content.is_empty() {
        None
    } else {
        Some(LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            content,
        )))
    }
}

fn tool_result_output(tool_result: &GenerateTextToolResult) -> LanguageModelToolResultOutput {
    if tool_result.is_error == Some(true) {
        return match &tool_result.output {
            JsonValue::String(message) => {
                LanguageModelToolResultOutput::error_text(message.clone())
            }
            output => LanguageModelToolResultOutput::error_json(output.clone()),
        };
    }

    match &tool_result.output {
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
    format!(
        "Invalid input for tool {tool_name}: {}",
        JsonParseError::new(input, cause)
    )
}

fn mark_unavailable_tool_calls(
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

fn mark_runtime_dynamic_tool_calls(tool_calls: &mut [GenerateTextToolCall], tools: &[Tool]) {
    for tool_call in tool_calls {
        if tools
            .iter()
            .any(|tool| tool.name == tool_call.tool_name && tool.is_dynamic())
        {
            tool_call.dynamic = Some(true);
        }
    }
}

fn mark_tool_call_titles(tool_calls: &mut [GenerateTextToolCall], tools: &[Tool]) {
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

fn mark_tool_call_metadata(tool_calls: &mut [GenerateTextToolCall], tools: &[Tool]) {
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

fn mark_tool_result_metadata(
    tool_results: &mut [GenerateTextToolResult],
    tool_calls: &[GenerateTextToolCall],
    tools: &[Tool],
) {
    for tool_result in tool_results {
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
        GenerateTextModelInfo, GenerateTextOptions, GenerateTextReasoning, GenerateTextResult,
        GenerateTextStep, GenerateTextToolCall, GenerateTextToolResult, NoSuchToolError,
        generate_text,
    };
    use crate::file_data::FileDataContent;
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelCallOptions, LanguageModelContent, LanguageModelFile, LanguageModelFileData,
        LanguageModelFinishReason, LanguageModelFunctionTool, LanguageModelGenerateResult,
        LanguageModelMessage, LanguageModelProviderTool, LanguageModelReasoning,
        LanguageModelReasoningFile, LanguageModelSource, LanguageModelStreamPart,
        LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelText,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolCall, LanguageModelToolCallPart,
        LanguageModelToolContentPart, LanguageModelToolResult, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUsage, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::provider::{ProviderMetadata, SpecificationVersion};
    use crate::provider_utils::{Tool, ToolExecutionError, dynamic_tool};
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::task::{Context, Poll, Waker};

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

    struct FakeLanguageModel {
        calls: RefCell<Vec<LanguageModelCallOptions>>,
    }

    impl FakeLanguageModel {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
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

            ready(LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Text(LanguageModelText::new("Hello ")),
                    LanguageModelContent::Text(LanguageModelText::new("world")),
                ],
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
            ))
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

    fn user_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
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
        assert_eq!(
            result.response_messages,
            vec![LanguageModelMessage::Assistant(
                crate::language_model::LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Hello ")),
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("world")),
                ])
            )]
        );
        assert_eq!(result.final_step().expect("step exists").step_number, 0);
        assert_eq!(
            result.final_step().expect("step exists").response_messages,
            result.response_messages
        );
        assert_eq!(
            result.final_step().expect("step exists").model,
            GenerateTextModelInfo::new("test-provider", "test-model")
        );
    }

    #[test]
    fn generate_text_result_serializes_as_camel_case_step_record() {
        let result = GenerateTextResult::from_steps(vec![GenerateTextStep {
            step_number: 0,
            model: GenerateTextModelInfo::new("test-provider", "test-model"),
            content: vec![LanguageModelContent::Text(LanguageModelText::new("Hello"))],
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
                        "stepNumber": 0,
                        "model": {
                            "provider": "test-provider",
                            "modelId": "test-model"
                        },
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
                        "warnings": []
                    }
                ]
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
    fn generate_text_splits_static_and_dynamic_tool_calls_and_results() {
        let step = GenerateTextStep::from_language_model_result(
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
            "mediaType": "text/plain",
            "data": {
                "type": "data",
                "data": "notes"
            }
        }))
        .expect("reasoning file deserializes");

        assert_eq!(
            reasoning,
            GenerateTextReasoning::Reasoning(LanguageModelReasoning::new("thinking"))
        );
        assert_eq!(
            reasoning_file,
            GenerateTextReasoning::ReasoningFile(LanguageModelReasoningFile::new(
                "text/plain",
                LanguageModelFileData::Data {
                    data: FileDataContent::Base64("notes".to_string())
                }
            ))
        );
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
            ]
        }))
        .expect("result deserializes");

        assert_eq!(result.text, "");
        assert_eq!(result.finish_reason, FinishReason::Length);
        assert_eq!(result.raw_finish_reason, None);
        assert_eq!(result.total_usage.input_tokens.total, Some(12));
        assert_eq!(result.total_usage.output_tokens.total, Some(4));
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
        assert_eq!(
            result.steps[0].model,
            GenerateTextModelInfo::new("test-provider", "test-model")
        );
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

        assert_eq!(result.files, vec![first_file.clone(), second_file.clone()]);
        assert_eq!(result.steps[0].files, vec![first_file]);
        assert_eq!(result.steps[1].files, vec![second_file]);
        assert_eq!(
            serde_json::to_value(&result.files).expect("files serialize"),
            json!([
                {
                    "type": "file",
                    "mediaType": "image/png",
                    "data": {
                        "type": "data",
                        "data": "AQID"
                    }
                },
                {
                    "type": "file",
                    "mediaType": "image/jpeg",
                    "data": {
                        "type": "data",
                        "data": [4, 5, 6]
                    }
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
            vec![GenerateTextReasoning::Reasoning(
                LanguageModelReasoning::new("first thoughts")
            )]
        );
        assert_eq!(
            result.steps[0].reasoning_text.as_deref(),
            Some("first thoughts")
        );
        assert_eq!(result.reasoning_text.as_deref(), Some("final thoughts"));
        assert_eq!(
            result.reasoning,
            vec![
                GenerateTextReasoning::Reasoning(LanguageModelReasoning::new("final ")),
                GenerateTextReasoning::ReasoningFile(reasoning_file.clone()),
                GenerateTextReasoning::Reasoning(LanguageModelReasoning::new("thoughts")),
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
                    "mediaType": "image/png",
                    "data": {
                        "type": "data",
                        "data": "cmVhc29uaW5n"
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
    fn generate_text_includes_non_text_content_in_content_but_not_text() {
        let content = vec![LanguageModelContent::ToolCall(
            crate::language_model::LanguageModelToolCall::new("call-1", "lookup", "{}"),
        )];
        let step = GenerateTextStep::from_language_model_result(
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
