use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelCustomContent,
    LanguageModelErrorStreamPart, LanguageModelFile, LanguageModelPrompt,
    LanguageModelRawStreamPart, LanguageModelReasoningEnd, LanguageModelReasoningFile,
    LanguageModelReasoningStart, LanguageModelRequest, LanguageModelSource,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata,
    LanguageModelStreamResultResponse, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelToolApprovalRequest, LanguageModelToolCall, LanguageModelToolChoice,
    LanguageModelToolInputDelta, LanguageModelToolInputEnd, LanguageModelToolInputStart,
    LanguageModelToolResult, LanguageModelUsage,
};
use crate::prompt::{Prompt, standardize_prompt};
use crate::provider::{InvalidPromptError, ProviderMetadata, ProviderOptions};
use crate::provider_utils::with_user_agent_suffix;
use crate::warning::Warning;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamStartKind {
    #[serde(rename = "start")]
    Start,
}

/// Start of a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamStartPart {
    #[serde(rename = "type")]
    kind: TextStreamStartKind,
}

impl TextStreamStartPart {
    /// Creates a stream start part.
    pub fn new() -> Self {
        Self {
            kind: TextStreamStartKind::Start,
        }
    }
}

impl Default for TextStreamStartPart {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamStartStepKind {
    #[serde(rename = "start-step")]
    StartStep,
}

/// Start of a model-call step inside a text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamStartStepPart {
    #[serde(rename = "type")]
    kind: TextStreamStartStepKind,

    /// Provider request metadata for the step.
    pub request: LanguageModelRequest,

    /// Warnings reported by the model provider for the step.
    pub warnings: Vec<Warning>,
}

impl TextStreamStartStepPart {
    /// Creates a step start part.
    pub fn new(request: LanguageModelRequest, warnings: Vec<Warning>) -> Self {
        Self {
            kind: TextStreamStartStepKind::StartStep,
            request,
            warnings,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamTextDeltaKind {
    #[serde(rename = "text-delta")]
    TextDelta,
}

/// Text delta emitted by a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamTextDeltaPart {
    #[serde(rename = "type")]
    kind: TextStreamTextDeltaKind,

    /// Identifier for the streamed text block.
    pub id: String,

    /// Optional provider-specific metadata for the text delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Text delta emitted by the provider.
    pub text: String,
}

impl TextStreamTextDeltaPart {
    /// Creates a text delta part.
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: TextStreamTextDeltaKind::TextDelta,
            id: id.into(),
            provider_metadata: None,
            text: text.into(),
        }
    }

    /// Adds provider-specific metadata to this text delta.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamReasoningDeltaKind {
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta,
}

/// Reasoning delta emitted by a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamReasoningDeltaPart {
    #[serde(rename = "type")]
    kind: TextStreamReasoningDeltaKind,

    /// Identifier for the streamed reasoning block.
    pub id: String,

    /// Optional provider-specific metadata for the reasoning delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Reasoning delta emitted by the provider.
    pub text: String,
}

impl TextStreamReasoningDeltaPart {
    /// Creates a reasoning delta part.
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: TextStreamReasoningDeltaKind::ReasoningDelta,
            id: id.into(),
            provider_metadata: None,
            text: text.into(),
        }
    }

    /// Adds provider-specific metadata to this reasoning delta.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamFileKind {
    #[serde(rename = "file")]
    File,
}

/// Generated file emitted by a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamFilePart {
    #[serde(rename = "type")]
    kind: TextStreamFileKind,

    /// Provider-v4 file content.
    pub file: LanguageModelFile,

    /// Optional provider-specific metadata for the file part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextStreamFilePart {
    /// Creates a generated file stream part.
    pub fn new(file: LanguageModelFile) -> Self {
        Self {
            provider_metadata: file.provider_metadata.clone(),
            kind: TextStreamFileKind::File,
            file,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamReasoningFileKind {
    #[serde(rename = "reasoning-file")]
    ReasoningFile,
}

/// Generated reasoning file emitted by a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamReasoningFilePart {
    #[serde(rename = "type")]
    kind: TextStreamReasoningFileKind,

    /// Provider-v4 reasoning file content.
    pub file: LanguageModelReasoningFile,

    /// Optional provider-specific metadata for the reasoning file part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextStreamReasoningFilePart {
    /// Creates a generated reasoning file stream part.
    pub fn new(file: LanguageModelReasoningFile) -> Self {
        Self {
            provider_metadata: file.provider_metadata.clone(),
            kind: TextStreamReasoningFileKind::ReasoningFile,
            file,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextResponseMetadata {
    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Start timestamp for the generated response, when one is available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub timestamp: Option<time::OffsetDateTime>,

    /// Provider model identifier used for the response, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    /// Response headers returned with the stream envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl StreamTextResponseMetadata {
    /// Creates empty stream response metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies response metadata from a stream part.
    pub fn with_response_metadata(mut self, metadata: LanguageModelStreamResponseMetadata) -> Self {
        self.id = metadata.id;
        self.timestamp = metadata.timestamp;
        self.model_id = metadata.model_id;
        self
    }

    /// Applies stream-envelope response metadata.
    pub fn with_stream_response(mut self, response: LanguageModelStreamResultResponse) -> Self {
        self.headers = response.headers;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextStepPerformance {
    /// Elapsed wall-clock time for the collected step in milliseconds.
    pub step_time_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamFinishStepKind {
    #[serde(rename = "finish-step")]
    FinishStep,
}

/// Final metadata for one model-call step inside a text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamFinishStepPart {
    #[serde(rename = "type")]
    kind: TextStreamFinishStepKind,

    /// Response metadata for the step.
    pub response: StreamTextResponseMetadata,

    /// Usage information for the step.
    pub usage: LanguageModelUsage,

    /// Runtime measurements captured by the collector.
    pub performance: StreamTextStepPerformance,

    /// Unified finish reason for the step.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Provider-specific metadata for the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextStreamFinishStepPart {
    /// Creates a step finish part.
    pub fn new(
        response: StreamTextResponseMetadata,
        usage: LanguageModelUsage,
        performance: StreamTextStepPerformance,
        finish_reason: FinishReason,
        raw_finish_reason: Option<String>,
        provider_metadata: Option<ProviderMetadata>,
    ) -> Self {
        Self {
            kind: TextStreamFinishStepKind::FinishStep,
            response,
            usage,
            performance,
            finish_reason,
            raw_finish_reason,
            provider_metadata,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamFinishKind {
    #[serde(rename = "finish")]
    Finish,
}

/// Final metadata for a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamFinishPart {
    #[serde(rename = "type")]
    kind: TextStreamFinishKind,

    /// Unified finish reason for the stream.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Total stream usage.
    pub total_usage: LanguageModelUsage,
}

impl TextStreamFinishPart {
    /// Creates a stream finish part.
    pub fn new(
        finish_reason: FinishReason,
        raw_finish_reason: Option<String>,
        total_usage: LanguageModelUsage,
    ) -> Self {
        Self {
            kind: TextStreamFinishKind::Finish,
            finish_reason,
            raw_finish_reason,
            total_usage,
        }
    }
}

/// High-level stream part emitted by [`stream_text`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TextStreamPart {
    /// Start of the high-level text stream.
    Start(TextStreamStartPart),

    /// Start of a model-call step.
    StartStep(TextStreamStartStepPart),

    /// Start of a streamed text block.
    TextStart(LanguageModelTextStart),

    /// Text delta with the upstream high-level `text` field.
    TextDelta(TextStreamTextDeltaPart),

    /// End of a streamed text block.
    TextEnd(LanguageModelTextEnd),

    /// Start of a streamed reasoning block.
    ReasoningStart(LanguageModelReasoningStart),

    /// Reasoning delta with the upstream high-level `text` field.
    ReasoningDelta(TextStreamReasoningDeltaPart),

    /// End of a streamed reasoning block.
    ReasoningEnd(LanguageModelReasoningEnd),

    /// Start of streamed tool input.
    ToolInputStart(LanguageModelToolInputStart),

    /// Delta for streamed tool input.
    ToolInputDelta(LanguageModelToolInputDelta),

    /// End of streamed tool input.
    ToolInputEnd(LanguageModelToolInputEnd),

    /// Provider-executed tool approval request.
    ToolApprovalRequest(LanguageModelToolApprovalRequest),

    /// Generated tool call.
    ToolCall(LanguageModelToolCall),

    /// Provider-executed tool result.
    ToolResult(LanguageModelToolResult),

    /// Provider-specific generated content.
    Custom(LanguageModelCustomContent),

    /// Generated file content.
    File(TextStreamFilePart),

    /// Generated reasoning file content.
    ReasoningFile(TextStreamReasoningFilePart),

    /// Source content used to generate the response.
    Source(LanguageModelSource),

    /// Raw provider chunk.
    Raw(LanguageModelRawStreamPart),

    /// Provider stream error.
    Error(LanguageModelErrorStreamPart),

    /// Final metadata for one model-call step.
    FinishStep(TextStreamFinishStepPart),

    /// Final metadata for the high-level stream.
    Finish(TextStreamFinishPart),
}

/// Request options for a high-level text streaming call.
pub struct StreamTextOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for the streaming call.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,
}

impl<'a, M: LanguageModel + ?Sized> StreamTextOptions<'a, M> {
    /// Creates stream options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self {
            model,
            call_options: LanguageModelCallOptions::new(prompt),
        }
    }

    /// Creates stream options from the high-level upstream prompt shape.
    pub fn from_prompt(model: &'a M, prompt: Prompt) -> Result<Self, InvalidPromptError> {
        let prompt = standardize_prompt(prompt)?.into_language_model_prompt();
        Ok(Self::new(model, prompt))
    }

    /// Creates stream options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, call_options: LanguageModelCallOptions) -> Self {
        Self {
            model,
            call_options,
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

    /// Sets the deterministic sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.call_options.seed = Some(seed);
        self
    }

    /// Adds a provider-facing tool that is available to the model.
    pub fn with_tool(mut self, tool: crate::language_model::LanguageModelTool) -> Self {
        self.call_options
            .tools
            .get_or_insert_with(Vec::new)
            .push(tool);
        self
    }

    /// Sets the tool selection strategy.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.call_options.tool_choice = Some(tool_choice);
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

    /// Sets provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.call_options.provider_options = Some(provider_options);
        self
    }
}

/// Per-step information collected by [`stream_text`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextStep {
    /// Provider request metadata for the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Provider response metadata for the step.
    pub response: StreamTextResponseMetadata,

    /// Warnings reported by the provider.
    pub warnings: Vec<Warning>,

    /// Text generated in this step.
    pub text: String,

    /// Individual text deltas generated in this step.
    pub text_stream: Vec<String>,

    /// Reasoning text generated in this step, when any reasoning deltas exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,

    /// Sources emitted by the provider.
    pub sources: Vec<LanguageModelSource>,

    /// Generated files emitted by the provider.
    pub files: Vec<LanguageModelFile>,

    /// Generated reasoning files emitted by the provider.
    pub reasoning_files: Vec<LanguageModelReasoningFile>,

    /// Tool calls emitted by the provider.
    pub tool_calls: Vec<LanguageModelToolCall>,

    /// Tool results emitted by the provider.
    pub tool_results: Vec<LanguageModelToolResult>,

    /// Provider-specific custom parts emitted by the provider.
    pub custom_parts: Vec<LanguageModelCustomContent>,

    /// Stream errors emitted by the provider.
    pub errors: Vec<JsonValue>,

    /// Usage information for this step.
    pub usage: LanguageModelUsage,

    /// Unified finish reason reported by the provider.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Provider-specific metadata returned with the finish part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Runtime measurements captured by the collector.
    pub performance: StreamTextStepPerformance,
}

/// Collected result of a high-level text streaming call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextResult {
    /// All high-level stream parts emitted by the collector.
    pub parts: Vec<TextStreamPart>,

    /// Text deltas emitted by the final step.
    pub text_stream: Vec<String>,

    /// Full text generated by the final step.
    pub text: String,

    /// Reasoning text generated by the final step, when any reasoning deltas exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,

    /// Sources emitted by all steps.
    pub sources: Vec<LanguageModelSource>,

    /// Files emitted by all steps.
    pub files: Vec<LanguageModelFile>,

    /// Reasoning files emitted by all steps.
    pub reasoning_files: Vec<LanguageModelReasoningFile>,

    /// Tool calls emitted by all steps.
    pub tool_calls: Vec<LanguageModelToolCall>,

    /// Tool results emitted by all steps.
    pub tool_results: Vec<LanguageModelToolResult>,

    /// Provider-specific custom parts emitted by all steps.
    pub custom_parts: Vec<LanguageModelCustomContent>,

    /// Stream errors emitted by all steps.
    pub errors: Vec<JsonValue>,

    /// Warnings reported by the provider.
    pub warnings: Vec<Warning>,

    /// Usage information from the final step.
    pub usage: LanguageModelUsage,

    /// Total usage across all steps.
    pub total_usage: LanguageModelUsage,

    /// Unified finish reason reported by the final step.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Request metadata from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Response metadata from the final step.
    pub response: StreamTextResponseMetadata,

    /// Provider-specific metadata returned with the final finish part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Collected stream steps.
    pub steps: Vec<StreamTextStep>,
}

impl StreamTextResult {
    /// Returns the final collected step.
    pub fn final_step(&self) -> Option<&StreamTextStep> {
        self.steps.last()
    }
}

/// Runs a text streaming call against a language model and collects the stream.
pub async fn stream_text<M>(options: StreamTextOptions<'_, M>) -> StreamTextResult
where
    M: LanguageModel + ?Sized,
    M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
{
    let StreamTextOptions {
        model,
        mut call_options,
    } = options;
    let include_raw_chunks = call_options.include_raw_chunks.unwrap_or(false);
    append_stream_text_user_agent(&mut call_options);

    let stream_result = model.do_stream(call_options).await;
    let request = stream_result.request;
    let envelope_response = stream_result.response;
    let mut response = StreamTextResponseMetadata::new();
    if let Some(envelope_response) = envelope_response.clone() {
        response = response.with_stream_response(envelope_response);
    }

    let step_start = Instant::now();
    let mut parts = vec![TextStreamPart::Start(TextStreamStartPart::new())];
    let mut start_step_index = None;
    let mut warnings = Vec::new();
    let mut text = String::new();
    let mut text_stream = Vec::new();
    let mut reasoning_text = String::new();
    let mut has_reasoning_text = false;
    let mut sources = Vec::new();
    let mut files = Vec::new();
    let mut reasoning_files = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut custom_parts = Vec::new();
    let mut errors = Vec::new();
    let mut usage = LanguageModelUsage::default();
    let mut finish_reason = FinishReason::Other;
    let mut raw_finish_reason = None;
    let mut provider_metadata = None;

    for part in stream_result.stream {
        match part {
            LanguageModelStreamPart::StreamStart(part) => {
                warnings = part.warnings;
            }
            part => {
                ensure_start_step(
                    &mut parts,
                    &mut start_step_index,
                    request.clone(),
                    warnings.clone(),
                );

                match part {
                    LanguageModelStreamPart::TextStart(part) => {
                        parts.push(TextStreamPart::TextStart(part));
                    }
                    LanguageModelStreamPart::TextDelta(part) => {
                        if !part.delta.is_empty() {
                            text.push_str(&part.delta);
                            text_stream.push(part.delta.clone());
                            let mut stream_part = TextStreamTextDeltaPart::new(part.id, part.delta);
                            if let Some(provider_metadata) = part.provider_metadata {
                                stream_part = stream_part.with_provider_metadata(provider_metadata);
                            }
                            parts.push(TextStreamPart::TextDelta(stream_part));
                        }
                    }
                    LanguageModelStreamPart::TextEnd(part) => {
                        parts.push(TextStreamPart::TextEnd(part));
                    }
                    LanguageModelStreamPart::ReasoningStart(part) => {
                        parts.push(TextStreamPart::ReasoningStart(part));
                    }
                    LanguageModelStreamPart::ReasoningDelta(part) => {
                        has_reasoning_text = true;
                        reasoning_text.push_str(&part.delta);
                        let mut stream_part =
                            TextStreamReasoningDeltaPart::new(part.id, part.delta);
                        if let Some(provider_metadata) = part.provider_metadata {
                            stream_part = stream_part.with_provider_metadata(provider_metadata);
                        }
                        parts.push(TextStreamPart::ReasoningDelta(stream_part));
                    }
                    LanguageModelStreamPart::ReasoningEnd(part) => {
                        parts.push(TextStreamPart::ReasoningEnd(part));
                    }
                    LanguageModelStreamPart::ToolInputStart(part) => {
                        parts.push(TextStreamPart::ToolInputStart(part));
                    }
                    LanguageModelStreamPart::ToolInputDelta(part) => {
                        parts.push(TextStreamPart::ToolInputDelta(part));
                    }
                    LanguageModelStreamPart::ToolInputEnd(part) => {
                        parts.push(TextStreamPart::ToolInputEnd(part));
                    }
                    LanguageModelStreamPart::ToolApprovalRequest(part) => {
                        parts.push(TextStreamPart::ToolApprovalRequest(part));
                    }
                    LanguageModelStreamPart::ToolCall(part) => {
                        tool_calls.push(part.clone());
                        parts.push(TextStreamPart::ToolCall(part));
                    }
                    LanguageModelStreamPart::ToolResult(part) => {
                        tool_results.push(part.clone());
                        parts.push(TextStreamPart::ToolResult(part));
                    }
                    LanguageModelStreamPart::Custom(part) => {
                        custom_parts.push(part.clone());
                        parts.push(TextStreamPart::Custom(part));
                    }
                    LanguageModelStreamPart::File(part) => {
                        files.push(part.clone());
                        parts.push(TextStreamPart::File(TextStreamFilePart::new(part)));
                    }
                    LanguageModelStreamPart::ReasoningFile(part) => {
                        reasoning_files.push(part.clone());
                        parts.push(TextStreamPart::ReasoningFile(
                            TextStreamReasoningFilePart::new(part),
                        ));
                    }
                    LanguageModelStreamPart::Source(part) => {
                        sources.push(part.clone());
                        parts.push(TextStreamPart::Source(part));
                    }
                    LanguageModelStreamPart::ResponseMetadata(part) => {
                        response = response.with_response_metadata(part);
                        if let Some(envelope_response) = envelope_response.clone() {
                            response = response.with_stream_response(envelope_response);
                        }
                    }
                    LanguageModelStreamPart::Finish(part) => {
                        usage = part.usage;
                        finish_reason = part.finish_reason.unified;
                        raw_finish_reason = part.finish_reason.raw;
                        provider_metadata = part.provider_metadata;
                    }
                    LanguageModelStreamPart::Raw(part) => {
                        if include_raw_chunks {
                            parts.push(TextStreamPart::Raw(part));
                        }
                    }
                    LanguageModelStreamPart::Error(part) => {
                        finish_reason = FinishReason::Error;
                        errors.push(part.error.clone());
                        parts.push(TextStreamPart::Error(part));
                    }
                    LanguageModelStreamPart::StreamStart(_) => unreachable!(),
                }
            }
        }
    }

    ensure_start_step(
        &mut parts,
        &mut start_step_index,
        request.clone(),
        warnings.clone(),
    );

    let performance = StreamTextStepPerformance {
        step_time_ms: u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let finish_step = TextStreamFinishStepPart::new(
        response.clone(),
        usage.clone(),
        performance,
        finish_reason.clone(),
        raw_finish_reason.clone(),
        provider_metadata.clone(),
    );
    parts.push(TextStreamPart::FinishStep(finish_step));
    parts.push(TextStreamPart::Finish(TextStreamFinishPart::new(
        finish_reason.clone(),
        raw_finish_reason.clone(),
        usage.clone(),
    )));

    let reasoning_text = has_reasoning_text.then_some(reasoning_text);
    let step = StreamTextStep {
        request: request.clone(),
        response: response.clone(),
        warnings: warnings.clone(),
        text: text.clone(),
        text_stream: text_stream.clone(),
        reasoning_text: reasoning_text.clone(),
        sources: sources.clone(),
        files: files.clone(),
        reasoning_files: reasoning_files.clone(),
        tool_calls: tool_calls.clone(),
        tool_results: tool_results.clone(),
        custom_parts: custom_parts.clone(),
        errors: errors.clone(),
        usage: usage.clone(),
        finish_reason: finish_reason.clone(),
        raw_finish_reason: raw_finish_reason.clone(),
        provider_metadata: provider_metadata.clone(),
        performance,
    };

    StreamTextResult {
        parts,
        text_stream,
        text,
        reasoning_text,
        sources,
        files,
        reasoning_files,
        tool_calls,
        tool_results,
        custom_parts,
        errors,
        warnings,
        usage: usage.clone(),
        total_usage: usage,
        finish_reason,
        raw_finish_reason,
        request,
        response,
        provider_metadata,
        steps: vec![step],
    }
}

fn ensure_start_step(
    parts: &mut Vec<TextStreamPart>,
    start_step_index: &mut Option<usize>,
    request: Option<LanguageModelRequest>,
    warnings: Vec<Warning>,
) {
    let start_step = TextStreamPart::StartStep(TextStreamStartStepPart::new(
        request.unwrap_or_default(),
        warnings,
    ));

    match start_step_index {
        Some(index) => parts[*index] = start_step,
        None => {
            *start_step_index = Some(parts.len());
            parts.push(start_step);
        }
    }
}

fn append_stream_text_user_agent(call_options: &mut LanguageModelCallOptions) {
    let headers = call_options.headers.take().map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    });

    call_options.headers = Some(with_user_agent_suffix(headers, [format!("ai/{VERSION}")]));
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use serde_json::{Map, json};

    use super::*;
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModelErrorStreamPart, LanguageModelFinishReason,
        LanguageModelMessage, LanguageModelRawStreamPart, LanguageModelStreamFinish,
        LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
        LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSystemMessage,
        LanguageModelTextDelta, LanguageModelTextPart, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::mock_models::MockLanguageModel;
    use crate::prompt::Prompt;

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("mock futures should be ready"),
        }
    }

    fn user_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(3),
                no_cache: Some(3),
                cache_read: Some(0),
                cache_write: Some(0),
            },
            output_tokens: OutputTokenUsage {
                total: Some(10),
                text: Some(10),
                reasoning: Some(0),
            },
            raw: None,
        }
    }

    fn finish_reason() -> LanguageModelFinishReason {
        LanguageModelFinishReason {
            unified: FinishReason::Stop,
            raw: Some("stop".to_string()),
        }
    }

    #[test]
    fn stream_text_calls_language_model_do_stream_with_standardized_prompt() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Hello").with_instructions("Use short answers"),
            )
            .expect("prompt should standardize")
            .with_max_output_tokens(20)
            .with_temperature(0.2)
            .with_header("x-trace", "trace_123"),
        ));

        assert_eq!(result.finish_reason, FinishReason::Stop);

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].prompt[0],
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Use short answers"))
        );
        assert_eq!(calls[0].prompt[1], user_message("Hello"));
        assert_eq!(calls[0].max_output_tokens, Some(20));
        assert_eq!(calls[0].temperature, Some(0.2));
        assert_eq!(
            calls[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-trace")),
            Some(&"trace_123".to_string())
        );
        assert!(
            calls[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent"))
                .is_some_and(|user_agent| user_agent.contains("ai/"))
        );
    }

    #[test]
    fn stream_text_collects_text_deltas_and_finish_metadata() {
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("testKey".to_string(), json!("testValue"))]),
        )]);
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("id-0")
            .with_model_id("mock-model-id");
        let model = MockLanguageModel::new().with_stream_result(
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ResponseMetadata(response_metadata),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(
                    LanguageModelStreamFinish::new(usage(), finish_reason())
                        .with_provider_metadata(provider_metadata.clone()),
                ),
            ])
            .with_response(
                LanguageModelStreamResultResponse::new().with_header("x-response-id", "resp_123"),
            ),
        );

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(result.text_stream, vec!["Hello", ", ", "world!"]);
        assert_eq!(result.usage, usage());
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.raw_finish_reason, Some("stop".to_string()));
        assert_eq!(result.provider_metadata, Some(provider_metadata));
        assert_eq!(result.response.id, Some("id-0".to_string()));
        assert_eq!(result.response.model_id, Some("mock-model-id".to_string()));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-response-id")),
            Some(&"resp_123".to_string())
        );
        assert_eq!(result.steps.len(), 1);
        assert!(matches!(
            result.parts.first(),
            Some(TextStreamPart::Start(_))
        ));
        assert!(matches!(
            result.parts.last(),
            Some(TextStreamPart::Finish(_))
        ));
    }

    #[test]
    fn stream_text_preserves_raw_chunks_when_requested() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Raw(LanguageModelRawStreamPart::new(
                    json!({"type": "raw-data", "content": "kept"}),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_include_raw_chunks(true),
        ));

        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Raw(_)))
        );
        assert_eq!(model.stream_calls()[0].include_raw_chunks, Some(true));
    }

    #[test]
    fn stream_text_omits_raw_chunks_by_default() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Raw(LanguageModelRawStreamPart::new(
                    json!({"type": "raw-data", "content": "hidden"}),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Raw(_)))
        );
        assert_eq!(model.stream_calls()[0].include_raw_chunks, None);
    }

    #[test]
    fn text_stream_parts_use_upstream_high_level_shapes() {
        let text_delta = TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("text-1", "Hello"));
        assert_eq!(
            serde_json::to_value(text_delta).expect("text delta should serialize"),
            json!({
                "type": "text-delta",
                "id": "text-1",
                "text": "Hello"
            })
        );

        let finish = TextStreamPart::Finish(TextStreamFinishPart::new(
            FinishReason::Stop,
            Some("stop".to_string()),
            usage(),
        ));
        let finish_value = serde_json::to_value(finish).expect("finish should serialize");
        assert_eq!(finish_value["type"], "finish");
        assert_eq!(finish_value["finishReason"], "stop");
        assert_eq!(finish_value["rawFinishReason"], "stop");
    }

    #[test]
    fn stream_text_retains_error_parts_and_marks_error_finish_without_finish_part() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({"message": "chunk failed"}),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        assert_eq!(result.text, "Hello");
        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.errors, vec![json!({"message": "chunk failed"})]);
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Error(_)))
        );
    }

    #[test]
    fn stream_text_maps_reasoning_sources_and_custom_parts() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                LanguageModelStreamPart::ReasoningDelta(
                    crate::language_model::LanguageModelReasoningDelta::new("r1", "Think"),
                ),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                LanguageModelStreamPart::Source(LanguageModelSource::url(
                    "source-1",
                    "https://example.com",
                )),
                LanguageModelStreamPart::Custom(LanguageModelCustomContent::new(
                    "mock-provider.custom",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Explain")],
        )));

        assert_eq!(result.reasoning_text, Some("Think".to_string()));
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.custom_parts.len(), 1);
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::ReasoningDelta(_)))
        );
    }
}
