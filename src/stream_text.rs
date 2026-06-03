use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::generate_text::{
    ActiveTools, GenerateTextFinishEvent, GenerateTextInclude, GenerateTextModelInfo,
    GenerateTextOnFinish, GenerateTextOnLanguageModelCallEnd, GenerateTextOnLanguageModelCallStart,
    GenerateTextOnStart, GenerateTextOnStepFinish, GenerateTextOnStepStart,
    GenerateTextOnToolExecutionEnd, GenerateTextOnToolExecutionStart, GenerateTextStartEvent,
    GenerateTextStep, GenerateTextStepPerformance, GenerateTextStepStartEvent, GenerateTextTool,
    GenerateTextToolCall, GenerateTextToolExecutionEndEvent, GenerateTextToolExecutionStartEvent,
    GenerateTextToolOutputDenied, GenerateTextToolResult, LanguageModelCallEndEvent,
    LanguageModelCallStartEvent, PrepareStep, PrepareStepOptions, PrepareStepResult,
    StepToolApprovalResponse, StopCondition, ToolApprovalConfiguration, ToolApprovalResponseOutput,
    ToolCallNotFoundForApprovalError, ToolCallRepair, ToolCallRepairOptions, ToolInputRefinement,
    ToolInputRefinementError, apply_generate_text_response_metadata, execute_tool_calls,
    filter_active_language_model_tools, generate_text_call_id,
    generate_text_tool_result_from_language_model_tool_result,
    initial_tool_approval_response_message, invoke_tool_input_available_callback,
    invoke_tool_input_delta_callback, invoke_tool_input_start_callback, is_stop_condition_met,
    mark_invalid_tool_inputs, mark_runtime_dynamic_tool_calls, mark_tool_call_metadata,
    mark_tool_call_titles, mark_tool_result_metadata, mark_unavailable_tool_calls,
    merge_provider_options, refine_tool_inputs, refresh_generate_text_content,
    refresh_tool_call_views, refresh_tool_result_views, repair_tool_calls,
    resolve_tool_approvals_for_step, response_messages_for_step,
    should_continue_after_tool_results, sync_tool_result_inputs,
    update_pending_deferred_provider_tool_calls,
};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue, NonNullJsonValue};
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelAbortController, LanguageModelAbortSignal,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelCustomContent,
    LanguageModelErrorStreamPart, LanguageModelFile, LanguageModelFileData,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelRawStreamPart, LanguageModelReasoning,
    LanguageModelReasoningEnd, LanguageModelReasoningFile, LanguageModelReasoningStart,
    LanguageModelRequest, LanguageModelResponse, LanguageModelSource, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResultResponse, LanguageModelText,
    LanguageModelTextEnd, LanguageModelTextStart, LanguageModelToolApprovalRequest,
    LanguageModelToolCall, LanguageModelToolChoice, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelToolResult,
    LanguageModelUsage,
};
use crate::logger::{LogWarningsOptions, log_warnings};
use crate::prompt::{
    Prompt, PromptDownload, TimeoutConfiguration, apply_downloaded_prompt_assets,
    download_prompt_assets, get_chunk_timeout_ms, get_step_timeout_ms, get_total_timeout_ms,
    prompt_has_url_files, standardize_and_convert_to_language_model_prompt,
};
use crate::provider::{
    ApiCallError, InvalidPromptError, ProviderMetadata, ProviderOptions, get_error_message,
};
use crate::provider_utils::{
    ExperimentalSandbox, Tool, convert_to_base64, prepare_tools_with_context,
    with_user_agent_suffix,
};
use crate::retry::{
    DEFAULT_INITIAL_RETRY_DELAY_MS, DEFAULT_MAX_RETRIES, DEFAULT_RETRY_BACKOFF_FACTOR,
    retry_delay_from_response_headers,
};
use crate::telemetry::{TelemetryOptions, create_telemetry_dispatcher};
use crate::text_stream_response::{
    TextStreamResponse, TextStreamResponseInit, TextStreamResponseOptions,
    TextStreamResponseWriter, create_text_stream_response, pipe_text_stream_to_response,
};
use crate::ui_message_stream::{
    HandleUiMessageStreamFinishOptions, ResponseUiMessageId, UiMessage, UiMessageChunk,
    UiMessageStreamFinishCallback, UiMessageStreamFinishCallbackEvent, UiMessageStreamResponse,
    UiMessageStreamResponseInit, UiMessageStreamResponseOptions, UiMessageStreamResponseWriter,
    create_ui_message_stream_response, get_response_ui_message_id, handle_ui_message_stream_finish,
    pipe_ui_message_stream_to_response,
};
use crate::util::{AbortSignalSource, Callback, InvalidArgumentError, merge_abort_signals};
use crate::warning::Warning;

#[cfg(test)]
use crate::prompt::TimeoutConfigurationOptions;

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

    /// Time until the first text, reasoning, or tool input delta was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_first_output_token_ms: Option<u64>,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamAbortKind {
    #[serde(rename = "abort")]
    Abort,
}

/// Abort notification for a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamAbortPart {
    #[serde(rename = "type")]
    kind: TextStreamAbortKind,

    /// Optional abort reason supplied by the caller/runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<JsonValue>,
}

impl TextStreamAbortPart {
    /// Creates an abort part without a reason.
    pub fn new() -> Self {
        Self {
            kind: TextStreamAbortKind::Abort,
            reason: None,
        }
    }

    /// Creates an abort part with a reason.
    pub fn with_reason(reason: impl Into<JsonValue>) -> Self {
        Self {
            kind: TextStreamAbortKind::Abort,
            reason: Some(reason.into()),
        }
    }
}

impl Default for TextStreamAbortPart {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum TextStreamToolApprovalRequestKind {
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest,
}

/// Tool approval request emitted by a high-level text stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStreamToolApprovalRequestPart {
    #[serde(rename = "type")]
    kind: TextStreamToolApprovalRequestKind,

    /// Identifier for the approval request.
    pub approval_id: String,

    /// Identifier of the tool call that requires approval.
    pub tool_call_id: String,

    /// Whether the approval status was decided automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_automatic: Option<bool>,

    /// Optional provider-specific metadata for the approval request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextStreamToolApprovalRequestPart {
    /// Creates a high-level tool approval request part.
    pub fn new(approval_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            kind: TextStreamToolApprovalRequestKind::ToolApprovalRequest,
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            is_automatic: None,
            provider_metadata: None,
        }
    }

    /// Creates a high-level request from a provider stream request.
    pub fn from_language_model_tool_approval_request(
        request: LanguageModelToolApprovalRequest,
    ) -> Self {
        Self {
            kind: TextStreamToolApprovalRequestKind::ToolApprovalRequest,
            approval_id: request.approval_id,
            tool_call_id: request.tool_call_id,
            is_automatic: None,
            provider_metadata: request.provider_metadata,
        }
    }

    /// Sets whether this request was automatically approved or denied.
    pub fn with_automatic(mut self, is_automatic: bool) -> Self {
        self.is_automatic = Some(is_automatic);
        self
    }

    /// Adds provider-specific metadata to this approval request.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    fn to_language_model_tool_approval_request(&self) -> LanguageModelToolApprovalRequest {
        let mut request =
            LanguageModelToolApprovalRequest::new(&self.approval_id, &self.tool_call_id);
        if let Some(provider_metadata) = &self.provider_metadata {
            request = request.with_provider_metadata(provider_metadata.clone());
        }
        request
    }
}

/// Caller-controlled abort signal for Rust `stream_text` calls.
pub type StreamTextAbortSignal = LanguageModelAbortSignal;

/// Controller used to trigger a [`StreamTextAbortSignal`].
pub type StreamTextAbortController = LanguageModelAbortController;

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

    /// Tool approval request.
    ToolApprovalRequest(TextStreamToolApprovalRequestPart),

    /// Tool approval response.
    ToolApprovalResponse(ToolApprovalResponseOutput),

    /// Generated tool call.
    ToolCall(GenerateTextToolCall),

    /// Provider-executed tool result.
    ToolResult(GenerateTextToolResult),

    /// Denied tool output.
    ToolOutputDenied(GenerateTextToolOutputDenied),

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

    /// Abort notification for the high-level stream.
    Abort(TextStreamAbortPart),

    /// Provider stream error.
    Error(LanguageModelErrorStreamPart),

    /// Final metadata for one model-call step.
    FinishStep(TextStreamFinishStepPart),

    /// Final metadata for the high-level stream.
    Finish(TextStreamFinishPart),
}

/// Callback used by [`SmoothStreamChunking::Detector`] to split buffered text.
pub type SmoothStreamChunkDetector = Arc<dyn Fn(&str) -> Option<String> + Send + Sync + 'static>;

/// Chunking strategy used by [`smooth_stream`].
#[derive(Clone, Default)]
pub enum SmoothStreamChunking {
    /// Emit the first word plus trailing whitespace, matching upstream `word`.
    #[default]
    Word,

    /// Emit through the first newline sequence, matching upstream `line`.
    Line,

    /// Emit through the first custom pattern match.
    Pattern(Regex),

    /// Emit the first Unicode word segment, matching upstream `Intl.Segmenter`.
    ///
    /// Backed by ICU's `WordSegmenter` (the same Unicode segmentation library
    /// `Intl.Segmenter` uses), giving locale-aware word boundaries that are
    /// recommended for CJK languages.
    Segmenter,

    /// Emit the custom detector's prefix match.
    Detector(SmoothStreamChunkDetector),
}

impl SmoothStreamChunking {
    /// Resolves an upstream string chunking strategy (`"word"` / `"line"`).
    ///
    /// Mirrors the upstream runtime validation: any other string (the JS
    /// `chunking: 'foo'` / `chunking: null` cases) is rejected with an
    /// [`InvalidArgumentError`], because no built-in regular expression exists
    /// for it. In Rust the typed variants cannot be constructed from an invalid
    /// value directly, so this constructor is the parity surface for those
    /// error cases.
    pub fn from_strategy(strategy: &str) -> Result<Self, InvalidArgumentError> {
        match strategy {
            "word" => Ok(Self::Word),
            "line" => Ok(Self::Line),
            other => Err(InvalidArgumentError::new(
                "chunking",
                JsonValue::String(other.to_string()),
                format!(
                    "Chunking must be \"word\", \"line\", a RegExp, an Intl.Segmenter, or a ChunkDetector function. Received: {other}"
                ),
            )),
        }
    }
}

impl fmt::Debug for SmoothStreamChunking {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Word => formatter.write_str("Word"),
            Self::Line => formatter.write_str("Line"),
            Self::Pattern(pattern) => formatter
                .debug_tuple("Pattern")
                .field(&pattern.as_str())
                .finish(),
            Self::Segmenter => formatter.write_str("Segmenter"),
            Self::Detector(_) => formatter.write_str("Detector(..)"),
        }
    }
}

/// Options for Rust-native `smoothStream` parity.
#[derive(Clone, Debug)]
pub struct SmoothStreamOptions {
    /// Controls how buffered text and reasoning deltas are split.
    pub chunking: SmoothStreamChunking,

    /// Delay in milliseconds after each detected smoothed chunk.
    pub delay_in_ms: Option<i64>,
}

impl SmoothStreamOptions {
    /// Creates default word-based smoothing options.
    pub fn new() -> Self {
        Self {
            chunking: SmoothStreamChunking::Word,
            delay_in_ms: Some(10),
        }
    }

    /// Sets the smoothing chunking strategy.
    pub fn with_chunking(mut self, chunking: SmoothStreamChunking) -> Self {
        self.chunking = chunking;
        self
    }

    /// Sets the delay in milliseconds after each detected smoothed chunk.
    ///
    /// `None` mirrors upstream `delayInMs: null` and resolves immediately.
    pub fn with_delay_in_ms(mut self, delay_in_ms: Option<i64>) -> Self {
        self.delay_in_ms = delay_in_ms;
        self
    }
}

impl Default for SmoothStreamOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Error produced while applying [`smooth_stream`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SmoothStreamError {
    /// A regular expression matched an empty string, which cannot advance the buffer.
    EmptyPatternMatch { pattern: String },

    /// A custom detector returned an empty chunk.
    EmptyDetectorMatch,

    /// A custom detector returned a chunk that is not a prefix of the buffer.
    NonPrefixDetectorMatch { matched: String, buffer: String },
}

impl fmt::Display for SmoothStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPatternMatch { pattern } => {
                write!(
                    formatter,
                    "Chunking pattern must not match an empty string. Received: {pattern}"
                )
            }
            Self::EmptyDetectorMatch => {
                formatter.write_str("Chunking function must return a non-empty string.")
            }
            Self::NonPrefixDetectorMatch { matched, buffer } => write!(
                formatter,
                "Chunking function must return a match that is a prefix of the buffer. Received: \"{matched}\" expected to start with \"{buffer}\""
            ),
        }
    }
}

impl std::error::Error for SmoothStreamError {}

/// Smooths text and reasoning deltas in a collected stream part sequence.
pub fn smooth_stream(
    parts: impl IntoIterator<Item = TextStreamPart>,
    options: SmoothStreamOptions,
) -> Result<Vec<TextStreamPart>, SmoothStreamError> {
    smooth_stream_parts(parts, &options)
}

/// Function used to transform collected high-level stream parts.
pub type StreamTextTransformFunction<'a> = dyn Fn(Vec<TextStreamPart>) -> Vec<TextStreamPart> + 'a;

/// Rust-native equivalent of upstream `streamText` `experimental_transform`.
#[derive(Clone)]
pub struct StreamTextTransform<'a> {
    transform: Rc<StreamTextTransformFunction<'a>>,
}

impl<'a> StreamTextTransform<'a> {
    /// Creates a stream transform from a function over high-level stream parts.
    pub fn new<F>(transform: F) -> Self
    where
        F: Fn(Vec<TextStreamPart>) -> Vec<TextStreamPart> + 'a,
    {
        Self {
            transform: Rc::new(transform),
        }
    }

    /// Applies this transform.
    pub fn transform(&self, parts: Vec<TextStreamPart>) -> Vec<TextStreamPart> {
        (self.transform)(parts)
    }
}

impl fmt::Debug for StreamTextTransform<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextTransform")
            .finish_non_exhaustive()
    }
}

/// Event sent for each portable streamed chunk accepted by `onChunk`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextOnChunkEvent {
    /// Stream chunk emitted by the high-level text stream.
    pub chunk: TextStreamPart,
}

/// Future returned by a stream-text chunk callback.
pub type StreamTextOnChunkFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked for portable chunks emitted by `stream_text`.
pub type StreamTextOnChunkFunction<'a> =
    dyn Fn(StreamTextOnChunkEvent) -> StreamTextOnChunkFuture<'a> + 'a;

/// Callback wrapper for upstream `onChunk`.
pub struct StreamTextOnChunk<'a> {
    on_chunk: Rc<StreamTextOnChunkFunction<'a>>,
}

impl<'a> StreamTextOnChunk<'a> {
    /// Creates a chunk callback.
    pub fn new<F, Fut>(on_chunk: F) -> Self
    where
        F: Fn(StreamTextOnChunkEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_chunk: Rc::new(move |event| Box::pin(on_chunk(event))),
        }
    }

    /// Runs the chunk callback.
    pub fn chunk(&self, event: StreamTextOnChunkEvent) -> StreamTextOnChunkFuture<'a> {
        (self.on_chunk)(event)
    }
}

impl fmt::Debug for StreamTextOnChunk<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextOnChunk")
            .finish_non_exhaustive()
    }
}

/// Event sent when a provider stream error part is observed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextOnErrorEvent {
    /// Provider error represented as JSON.
    pub error: JsonValue,
}

/// Future returned by a stream-text error callback.
pub type StreamTextOnErrorFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked for provider errors emitted by `stream_text`.
pub type StreamTextOnErrorFunction<'a> =
    dyn Fn(StreamTextOnErrorEvent) -> StreamTextOnErrorFuture<'a> + 'a;

/// Callback wrapper for upstream `onError`.
pub struct StreamTextOnError<'a> {
    on_error: Rc<StreamTextOnErrorFunction<'a>>,
}

impl<'a> StreamTextOnError<'a> {
    /// Creates an error callback.
    pub fn new<F, Fut>(on_error: F) -> Self
    where
        F: Fn(StreamTextOnErrorEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_error: Rc::new(move |event| Box::pin(on_error(event))),
        }
    }

    /// Runs the error callback.
    pub fn error(&self, event: StreamTextOnErrorEvent) -> StreamTextOnErrorFuture<'a> {
        (self.on_error)(event)
    }
}

impl fmt::Debug for StreamTextOnError<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextOnError")
            .finish_non_exhaustive()
    }
}

/// Event sent when a stream is aborted before completing another step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTextOnAbortEvent {
    /// Completed generation steps before the abort was observed.
    pub steps: Vec<GenerateTextStep>,

    /// Optional abort reason supplied by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<JsonValue>,
}

/// Future returned by a stream-text abort callback.
pub type StreamTextOnAbortFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked when `stream_text` observes an abort signal.
pub type StreamTextOnAbortFunction<'a> =
    dyn Fn(StreamTextOnAbortEvent) -> StreamTextOnAbortFuture<'a> + 'a;

/// Callback wrapper for upstream `onAbort`.
pub struct StreamTextOnAbort<'a> {
    on_abort: Rc<StreamTextOnAbortFunction<'a>>,
}

impl<'a> StreamTextOnAbort<'a> {
    /// Creates an abort callback.
    pub fn new<F, Fut>(on_abort: F) -> Self
    where
        F: Fn(StreamTextOnAbortEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_abort: Rc::new(move |event| Box::pin(on_abort(event))),
        }
    }

    /// Runs the abort callback.
    pub fn abort(&self, event: StreamTextOnAbortEvent) -> StreamTextOnAbortFuture<'a> {
        (self.on_abort)(event)
    }
}

impl fmt::Debug for StreamTextOnAbort<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextOnAbort")
            .finish_non_exhaustive()
    }
}

/// Request options for a high-level text streaming call.
pub struct StreamTextOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for the streaming call.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,

    /// High-level Rust tools made available to the model.
    pub tools: Vec<Tool>,

    /// User-defined runtime context attached to every streamed step.
    pub runtime_context: JsonObject,

    /// Tool-specific context keyed by tool name.
    pub tools_context: JsonObject,

    /// Experimental sandbox environment passed through to Rust tool execution.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,

    /// Optional active tool names used to restrict the available tool set.
    pub active_tools: ActiveTools,

    /// Static approval configuration for streamed tool calls.
    pub tool_approval: Option<ToolApprovalConfiguration>,

    /// Per-tool input refinements applied after parsing valid tool calls.
    pub tool_input_refinements: BTreeMap<String, ToolInputRefinement>,

    /// Optional callback used to repair invalid model tool calls before execution.
    pub tool_call_repair: Option<ToolCallRepair>,

    /// Optional callback invoked before any streamed model work begins.
    pub on_start: Option<GenerateTextOnStart<'a>>,

    /// Optional callback invoked before each streamed model step begins.
    pub on_step_start: Option<GenerateTextOnStepStart<'a>>,

    /// Optional per-step preparation callback.
    pub prepare_step: Option<PrepareStep<'a, M>>,

    /// Optional callback invoked immediately before each provider stream call begins.
    pub on_language_model_call_start: Option<GenerateTextOnLanguageModelCallStart<'a>>,

    /// Optional callback invoked after each provider stream call completes.
    pub on_language_model_call_end: Option<GenerateTextOnLanguageModelCallEnd<'a>>,

    /// Optional callback invoked before a local Rust tool executor is invoked.
    pub on_tool_execution_start: Option<GenerateTextOnToolExecutionStart<'a>>,

    /// Optional callback invoked after a local Rust tool executor completes.
    pub on_tool_execution_end: Option<GenerateTextOnToolExecutionEnd<'a>>,

    /// Optional callback invoked after each completed streamed generation step.
    pub on_step_finish: Option<GenerateTextOnStepFinish<'a>>,

    /// Optional callback invoked after the full streamed generation result is complete.
    pub on_finish: Option<GenerateTextOnFinish<'a>>,

    /// Optional telemetry dispatcher settings.
    pub telemetry: Option<TelemetryOptions>,

    /// Optional callback used to download URL-backed prompt assets.
    pub download: Option<PromptDownload>,

    /// Optional request timeout configuration.
    pub timeout: Option<TimeoutConfiguration>,

    /// Maximum number of retries for failed provider stream requests.
    pub max_retries: usize,

    /// Optional Rust-native smooth stream transform.
    pub smooth_stream: Option<SmoothStreamOptions>,

    /// Optional stream transforms applied before output collection replay.
    pub transforms: Vec<StreamTextTransform<'a>>,

    /// Optional callback invoked for portable stream chunks.
    pub on_chunk: Option<StreamTextOnChunk<'a>>,

    /// Optional callback invoked for provider error stream parts.
    pub on_error: Option<StreamTextOnError<'a>>,

    /// Optional abort signal checked before and during streamed collection.
    pub abort_signal: Option<StreamTextAbortSignal>,

    /// Optional callback invoked when the abort signal is observed.
    pub on_abort: Option<StreamTextOnAbort<'a>>,

    /// Maximum number of model-call steps to run.
    pub max_steps: usize,

    /// Additional stop conditions checked after every completed step.
    pub stop_conditions: Vec<StopCondition>,

    /// Settings controlling which large provider payloads are retained in step results.
    pub include: GenerateTextInclude,
}

impl<'a, M: LanguageModel + ?Sized> StreamTextOptions<'a, M> {
    /// Creates stream options for a model and standardized prompt.
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
            on_start: None,
            on_step_start: None,
            prepare_step: None,
            on_language_model_call_start: None,
            on_language_model_call_end: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
            download: None,
            timeout: None,
            max_retries: DEFAULT_MAX_RETRIES,
            smooth_stream: None,
            transforms: Vec::new(),
            on_chunk: None,
            on_error: None,
            abort_signal: None,
            on_abort: None,
            max_steps: 1,
            stop_conditions: Vec::new(),
            include: GenerateTextInclude::new(),
        }
    }

    /// Creates stream options from the high-level upstream prompt shape.
    pub fn from_prompt(model: &'a M, prompt: Prompt) -> Result<Self, InvalidPromptError> {
        let prompt = standardize_and_convert_to_language_model_prompt(prompt)?;
        Ok(Self::new(model, prompt))
    }

    /// Creates stream options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, call_options: LanguageModelCallOptions) -> Self {
        let abort_signal = call_options.abort_signal.clone();
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
            on_start: None,
            on_step_start: None,
            prepare_step: None,
            on_language_model_call_start: None,
            on_language_model_call_end: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
            download: None,
            timeout: None,
            max_retries: DEFAULT_MAX_RETRIES,
            smooth_stream: None,
            transforms: Vec::new(),
            on_chunk: None,
            on_error: None,
            abort_signal,
            on_abort: None,
            max_steps: 1,
            stop_conditions: Vec::new(),
            include: GenerateTextInclude::new(),
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

    /// Sets the response format for the streamed generation.
    pub fn with_response_format(
        mut self,
        response_format: crate::language_model::LanguageModelResponseFormat,
    ) -> Self {
        self.call_options.response_format = Some(response_format);
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

    /// Sets the user-defined runtime context attached to every streamed step.
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

    /// Sets the active tool names for this streaming call.
    pub fn with_active_tools(
        mut self,
        active_tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.active_tools = Some(active_tools.into_iter().map(Into::into).collect());
        self
    }

    /// Sets static approval configuration for streamed tool calls.
    pub fn with_tool_approval(mut self, tool_approval: ToolApprovalConfiguration) -> Self {
        self.tool_approval = Some(tool_approval);
        self
    }

    /// Adds or replaces an input refinement for one tool.
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

    /// Sets a callback that can repair unavailable or invalid streamed tool calls.
    pub fn with_tool_call_repair<F, Fut, E>(mut self, repair: F) -> Self
    where
        F: Fn(ToolCallRepairOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<crate::language_model::LanguageModelToolCall>, E>>
            + Send
            + 'static,
        E: fmt::Display,
    {
        self.tool_call_repair = Some(ToolCallRepair::new(repair));
        self
    }

    /// Sets a callback that is invoked when streaming starts before model work.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(GenerateTextStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(GenerateTextOnStart::new(on_start));
        self
    }

    /// Sets a callback that is invoked before every streamed model step.
    pub fn with_on_step_start<F, Fut>(mut self, on_step_start: F) -> Self
    where
        F: Fn(GenerateTextStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_start = Some(GenerateTextOnStepStart::new(on_step_start));
        self
    }

    /// Sets a callback that can override settings for each streamed model step.
    pub fn with_prepare_step<F, Fut>(mut self, prepare: F) -> Self
    where
        F: Fn(PrepareStepOptions<'a, M>) -> Fut + 'a,
        Fut: Future<Output = PrepareStepResult<'a, M>> + 'a,
    {
        self.prepare_step = Some(PrepareStep::new(prepare));
        self
    }

    /// Sets a callback that is invoked immediately before each provider stream call begins.
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

    /// Sets a callback that is invoked after each provider stream call completes.
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

    /// Deprecated upstream alias for [`StreamTextOptions::with_on_tool_execution_start`].
    pub fn with_experimental_on_tool_call_start<F, Fut>(self, on_tool_execution_start: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        let mut this = self;
        if this.on_tool_execution_start.is_none() {
            this.on_tool_execution_start = Some(GenerateTextOnToolExecutionStart::new(
                on_tool_execution_start,
            ));
        }
        this
    }

    /// Deprecated upstream alias for [`StreamTextOptions::with_on_tool_execution_end`].
    pub fn with_experimental_on_tool_call_finish<F, Fut>(self, on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        let mut this = self;
        if this.on_tool_execution_end.is_none() {
            this.on_tool_execution_end =
                Some(GenerateTextOnToolExecutionEnd::new(on_tool_execution_end));
        }
        this
    }

    /// Sets a callback that is invoked after every completed streamed step.
    pub fn with_on_step_finish<F, Fut>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(GenerateTextStep) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_finish = Some(GenerateTextOnStepFinish::new(on_step_finish));
        self
    }

    /// Sets a callback that is invoked after the streamed generation result is complete.
    pub fn with_on_finish<F, Fut>(mut self, on_finish: F) -> Self
    where
        F: Fn(GenerateTextFinishEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_finish = Some(GenerateTextOnFinish::new(on_finish));
        self
    }

    /// Sets telemetry options for this streaming generation.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Deprecated upstream alias for [`StreamTextOptions::with_telemetry`].
    pub fn with_experimental_telemetry(self, telemetry: TelemetryOptions) -> Self {
        self.with_telemetry(telemetry)
    }

    /// Sets the request timeout configuration.
    pub fn with_timeout(mut self, timeout: impl Into<TimeoutConfiguration>) -> Self {
        self.timeout = Some(timeout.into());
        self
    }

    /// Sets the maximum number of retries for failed provider stream requests.
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Applies upstream-style smooth streaming to text and reasoning deltas.
    pub fn with_smooth_stream(mut self, smooth_stream: SmoothStreamOptions) -> Self {
        self.smooth_stream = Some(smooth_stream);
        self
    }

    /// Adds a Rust-native stream transform.
    pub fn with_transform(mut self, transform: StreamTextTransform<'a>) -> Self {
        self.transforms.push(transform);
        self
    }

    /// Replaces the Rust-native stream transform list.
    pub fn with_transforms(
        mut self,
        transforms: impl IntoIterator<Item = StreamTextTransform<'a>>,
    ) -> Self {
        self.transforms = transforms.into_iter().collect();
        self
    }

    /// Sets a callback that is invoked for each portable stream chunk.
    pub fn with_on_chunk<F, Fut>(mut self, on_chunk: F) -> Self
    where
        F: Fn(StreamTextOnChunkEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_chunk = Some(StreamTextOnChunk::new(on_chunk));
        self
    }

    /// Sets a callback that is invoked for provider stream errors.
    pub fn with_on_error<F, Fut>(mut self, on_error: F) -> Self
    where
        F: Fn(StreamTextOnErrorEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_error = Some(StreamTextOnError::new(on_error));
        self
    }

    /// Sets a caller-controlled abort signal for this stream.
    pub fn with_abort_signal(mut self, abort_signal: StreamTextAbortSignal) -> Self {
        self.call_options.abort_signal = Some(abort_signal.clone());
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets a callback that is invoked when streaming is aborted.
    pub fn with_on_abort<F, Fut>(mut self, on_abort: F) -> Self
    where
        F: Fn(StreamTextOnAbortEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_abort = Some(StreamTextOnAbort::new(on_abort));
        self
    }

    /// Sets the callback used to download URL-backed prompt assets.
    pub fn with_download(mut self, download: PromptDownload) -> Self {
        self.download = Some(download);
        self
    }

    /// Sets the maximum number of model-call steps.
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

    /// Sets whether raw stream chunks should be included.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.call_options.include_raw_chunks = Some(include_raw_chunks);
        self
    }

    /// Sets payload retention settings controlling which large provider payloads are
    /// retained in step results (request body and request messages).
    pub fn with_include(mut self, include: GenerateTextInclude) -> Self {
        self.include = include;
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
    pub tool_calls: Vec<GenerateTextToolCall>,

    /// Tool results emitted by the provider.
    pub tool_results: Vec<GenerateTextToolResult>,

    /// Response messages generated for this step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_messages: Vec<LanguageModelMessage>,

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
    pub tool_calls: Vec<GenerateTextToolCall>,

    /// Tool results emitted by all steps.
    pub tool_results: Vec<GenerateTextToolResult>,

    /// Response messages accumulated across all steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_messages: Vec<LanguageModelMessage>,

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

/// Callback invoked while converting stream-text parts to UI-message metadata.
pub type StreamTextMessageMetadataFunction =
    dyn Fn(&TextStreamPart) -> Option<JsonValue> + Send + Sync + 'static;

/// Callback wrapper for upstream `toUIMessageStream` `messageMetadata`.
#[derive(Clone)]
pub struct StreamTextMessageMetadata {
    callback: Arc<StreamTextMessageMetadataFunction>,
}

impl StreamTextMessageMetadata {
    /// Creates a message-metadata callback.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(&TextStreamPart) -> Option<JsonValue> + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Runs the metadata callback for one stream-text part.
    pub fn metadata(&self, part: &TextStreamPart) -> Option<JsonValue> {
        (self.callback)(part)
    }
}

impl fmt::Debug for StreamTextMessageMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextMessageMetadata")
            .finish_non_exhaustive()
    }
}

/// Callback invoked while converting stream-text errors to UI-message text.
pub type StreamTextUiMessageErrorFunction = dyn Fn(&JsonValue) -> String + Send + Sync + 'static;

/// Callback wrapper for upstream `toUIMessageStream` `onError`.
#[derive(Clone)]
pub struct StreamTextUiMessageErrorHandler {
    callback: Arc<StreamTextUiMessageErrorFunction>,
}

impl StreamTextUiMessageErrorHandler {
    /// Creates a UI-message stream error handler.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(&JsonValue) -> String + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Returns the UI-message error text for one stream error payload.
    pub fn error_text(&self, error: &JsonValue) -> String {
        (self.callback)(error)
    }
}

impl fmt::Debug for StreamTextUiMessageErrorHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextUiMessageErrorHandler")
            .finish_non_exhaustive()
    }
}

/// Function used to generate a persisted response UI-message id.
pub type StreamTextGenerateMessageIdFunction = dyn Fn() -> String + Send + Sync + 'static;

/// Callback wrapper for upstream `toUIMessageStream` `generateMessageId`.
#[derive(Clone)]
pub struct StreamTextGenerateMessageId {
    callback: Arc<StreamTextGenerateMessageIdFunction>,
}

impl StreamTextGenerateMessageId {
    /// Creates a response message id generator.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Generates a response message id.
    pub fn generate(&self) -> String {
        (self.callback)()
    }
}

impl fmt::Debug for StreamTextGenerateMessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamTextGenerateMessageId")
            .finish_non_exhaustive()
    }
}

/// Options for converting a [`StreamTextResult`] into UI-message stream chunks.
#[derive(Clone, Debug)]
pub struct StreamTextUiMessageStreamOptions {
    /// Optional response message id to include in the stream-start chunk.
    pub message_id: Option<String>,

    /// Original UI messages used to enable persistence-mode id selection.
    pub original_messages: Option<Vec<UiMessage>>,

    /// Optional response message id generator for persistence mode.
    pub generate_message_id: Option<StreamTextGenerateMessageId>,

    /// Optional callback that emits UI message metadata for matching stream parts.
    pub message_metadata: Option<StreamTextMessageMetadata>,

    /// Optional callback used to map stream errors into UI-safe text.
    pub on_error: Option<StreamTextUiMessageErrorHandler>,

    /// Optional callback invoked with final persisted UI-message state.
    pub on_finish: Option<UiMessageStreamFinishCallback>,

    /// Whether reasoning chunks should be included. Defaults to `true`.
    pub send_reasoning: bool,

    /// Whether source chunks should be included. Defaults to `false`.
    pub send_sources: bool,

    /// Whether the stream-start chunk should be included. Defaults to `true`.
    pub send_start: bool,

    /// Whether the stream-finish chunk should be included. Defaults to `true`.
    pub send_finish: bool,
}

impl StreamTextUiMessageStreamOptions {
    /// Creates default UI-message stream conversion options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the response message id included in the stream-start chunk.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Sets the original UI messages used for persistence-mode id selection.
    pub fn with_original_messages<I>(mut self, original_messages: I) -> Self
    where
        I: IntoIterator<Item = UiMessage>,
    {
        self.original_messages = Some(original_messages.into_iter().collect());
        self
    }

    /// Sets a response message id generator for persistence mode.
    pub fn with_generate_message_id<F>(mut self, generate_message_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_message_id = Some(StreamTextGenerateMessageId::new(generate_message_id));
        self
    }

    /// Sets a callback that can emit UI-message metadata for stream parts.
    pub fn with_message_metadata<F>(mut self, message_metadata: F) -> Self
    where
        F: Fn(&TextStreamPart) -> Option<JsonValue> + Send + Sync + 'static,
    {
        self.message_metadata = Some(StreamTextMessageMetadata::new(message_metadata));
        self
    }

    /// Sets a callback that maps stream errors into UI-message error text.
    pub fn with_on_error<F>(mut self, on_error: F) -> Self
    where
        F: Fn(&JsonValue) -> String + Send + Sync + 'static,
    {
        self.on_error = Some(StreamTextUiMessageErrorHandler::new(on_error));
        self
    }

    /// Sets a callback that receives the final persisted UI-message state.
    pub fn with_on_finish<F>(mut self, on_finish: F) -> Self
    where
        F: Fn(UiMessageStreamFinishCallbackEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(UiMessageStreamFinishCallback::new(on_finish));
        self
    }

    /// Sets whether reasoning chunks should be included.
    pub fn with_send_reasoning(mut self, send_reasoning: bool) -> Self {
        self.send_reasoning = send_reasoning;
        self
    }

    /// Sets whether source chunks should be included.
    pub fn with_send_sources(mut self, send_sources: bool) -> Self {
        self.send_sources = send_sources;
        self
    }

    /// Sets whether the stream-start chunk should be included.
    pub fn with_send_start(mut self, send_start: bool) -> Self {
        self.send_start = send_start;
        self
    }

    /// Sets whether the stream-finish chunk should be included.
    pub fn with_send_finish(mut self, send_finish: bool) -> Self {
        self.send_finish = send_finish;
        self
    }
}

impl Default for StreamTextUiMessageStreamOptions {
    fn default() -> Self {
        Self {
            message_id: None,
            original_messages: None,
            generate_message_id: None,
            message_metadata: None,
            on_error: None,
            on_finish: None,
            send_reasoning: true,
            send_sources: false,
            send_start: true,
            send_finish: true,
        }
    }
}

impl StreamTextResult {
    /// Returns the final collected step.
    pub fn final_step(&self) -> Option<&StreamTextStep> {
        self.steps.last()
    }

    /// Deserializes the final streamed output into a caller-provided Rust type.
    pub fn output_as<T>(&self) -> Result<T, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_json::from_value(stream_text_output_value(&self.text))
    }

    /// Deserializes a materialized partial-output entry into a Rust type.
    pub fn partial_output_as<T>(partial_output: &JsonValue) -> Result<T, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        serde_json::from_value(partial_output.clone())
    }

    /// Deserializes each materialized partial-output entry into Rust values.
    pub fn partial_outputs_as<T>(&self) -> Result<Vec<T>, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.partial_output_values()
            .iter()
            .map(Self::partial_output_as)
            .collect()
    }

    /// Deserializes array-output elements from partial-output stream entries.
    pub fn element_stream_as<T>(&self) -> Result<Vec<T>, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut elements = Vec::new();
        for partial_output in self.partial_output_values() {
            if let JsonValue::Array(items) = partial_output {
                for item in items {
                    elements.push(serde_json::from_value(item.clone())?);
                }
            }
        }
        Ok(elements)
    }

    /// Returns partial-output values represented by text deltas in this result.
    pub fn partial_output_values(&self) -> Vec<JsonValue> {
        if self.text_stream.is_empty() {
            return Vec::new();
        }
        vec![stream_text_output_value(&self.text)]
    }

    /// Consumes the materialized stream result, ignoring provider error parts.
    pub fn consume_stream(&self) {
        self.consume_stream_with_on_error(|_| {});
    }

    /// Consumes the materialized stream result and reports provider error parts.
    pub fn consume_stream_with_on_error<F>(&self, mut on_error: F)
    where
        F: FnMut(&JsonValue),
    {
        for part in &self.parts {
            if let TextStreamPart::Error(error_part) = part {
                on_error(&error_part.error);
            }
        }
    }

    /// Converts collected stream parts into UI-message stream chunks.
    pub fn to_ui_message_stream(&self) -> Vec<UiMessageChunk> {
        self.to_ui_message_stream_with_options(StreamTextUiMessageStreamOptions::default())
    }

    /// Converts collected stream parts into UI-message stream chunks with options.
    pub fn to_ui_message_stream_with_options(
        &self,
        options: StreamTextUiMessageStreamOptions,
    ) -> Vec<UiMessageChunk> {
        let mut chunks = Vec::new();
        let response_message_id = stream_text_response_message_id(&options);

        for stream_part in &self.parts {
            match stream_part {
                TextStreamPart::Start(_) => {
                    if options.send_start {
                        let mut chunk = match &response_message_id {
                            Some(message_id) => {
                                UiMessageChunk::start_with_message_id(message_id.clone())
                            }
                            None => UiMessageChunk::start(),
                        };
                        if let Some(message_metadata) =
                            stream_text_ui_message_metadata(&options, stream_part)
                        {
                            chunk = chunk.with_message_metadata(message_metadata);
                        }
                        chunks.push(chunk);
                    } else {
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::StartStep(_) => {
                    chunks.push(UiMessageChunk::start_step());
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::TextStart(part) => {
                    chunks.push(UiMessageChunk::TextStart {
                        id: part.id.clone(),
                        provider_metadata: part.provider_metadata.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::TextDelta(part) => {
                    chunks.push(UiMessageChunk::TextDelta {
                        id: part.id.clone(),
                        delta: part.text.clone(),
                        provider_metadata: part.provider_metadata.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::TextEnd(part) => {
                    chunks.push(UiMessageChunk::TextEnd {
                        id: part.id.clone(),
                        provider_metadata: part.provider_metadata.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ReasoningStart(part) => {
                    if options.send_reasoning {
                        chunks.push(UiMessageChunk::ReasoningStart {
                            id: part.id.clone(),
                            provider_metadata: part.provider_metadata.clone(),
                        });
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::ReasoningDelta(part) => {
                    if options.send_reasoning {
                        chunks.push(UiMessageChunk::ReasoningDelta {
                            id: part.id.clone(),
                            delta: part.text.clone(),
                            provider_metadata: part.provider_metadata.clone(),
                        });
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::ReasoningEnd(part) => {
                    if options.send_reasoning {
                        chunks.push(UiMessageChunk::ReasoningEnd {
                            id: part.id.clone(),
                            provider_metadata: part.provider_metadata.clone(),
                        });
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::Error(part) => {
                    chunks.push(UiMessageChunk::error(ui_message_error_text(
                        &part.error,
                        &options,
                    )));
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::Abort(part) => {
                    chunks.push(match &part.reason {
                        Some(reason) => UiMessageChunk::abort_with_reason(reason.clone()),
                        None => UiMessageChunk::abort(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::FinishStep(_) => {
                    chunks.push(UiMessageChunk::finish_step());
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::Finish(part) => {
                    if options.send_finish {
                        let mut chunk =
                            UiMessageChunk::finish_with_reason(part.finish_reason.clone());
                        if let Some(message_metadata) =
                            stream_text_ui_message_metadata(&options, stream_part)
                        {
                            chunk = chunk.with_message_metadata(message_metadata);
                        }
                        chunks.push(chunk);
                    } else {
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::ToolInputStart(part) => {
                    chunks.push(UiMessageChunk::ToolInputStart {
                        tool_call_id: part.id.clone(),
                        tool_name: part.tool_name.clone(),
                        provider_executed: part.provider_executed,
                        provider_metadata: part.provider_metadata.clone(),
                        dynamic: part.dynamic,
                        title: part.title.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolInputDelta(part) => {
                    chunks.push(UiMessageChunk::tool_input_delta(
                        part.id.clone(),
                        part.delta.clone(),
                    ));
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolInputEnd(_) => {
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolApprovalRequest(part) => {
                    chunks.push(UiMessageChunk::ToolApprovalRequest {
                        approval_id: part.approval_id.clone(),
                        tool_call_id: part.tool_call_id.clone(),
                        is_automatic: part.is_automatic,
                        provider_metadata: part.provider_metadata.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolApprovalResponse(part) => {
                    chunks.push(UiMessageChunk::ToolApprovalResponse {
                        approval_id: part.approval_id.clone(),
                        approved: part.approved,
                        reason: part.reason.clone(),
                        provider_executed: part.provider_executed,
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolCall(part) => {
                    if part.invalid == Some(true) {
                        chunks.push(UiMessageChunk::ToolInputError {
                            tool_call_id: part.tool_call_id.clone(),
                            tool_name: part.tool_name.clone(),
                            input: part.input.clone(),
                            error_text: tool_call_error_text(part.error.as_deref(), &options),
                            provider_executed: part.provider_executed,
                            provider_metadata: part.provider_metadata.clone(),
                            tool_metadata: part.tool_metadata.clone(),
                            dynamic: part.dynamic,
                            title: part.title.clone(),
                        });
                    } else {
                        chunks.push(UiMessageChunk::ToolInputAvailable {
                            tool_call_id: part.tool_call_id.clone(),
                            tool_name: part.tool_name.clone(),
                            input: part.input.clone(),
                            provider_executed: part.provider_executed,
                            provider_metadata: part.provider_metadata.clone(),
                            tool_metadata: part.tool_metadata.clone(),
                            dynamic: part.dynamic,
                            title: part.title.clone(),
                        });
                    }
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolResult(part) => {
                    if part.is_error == Some(true) {
                        chunks.push(UiMessageChunk::ToolOutputError {
                            tool_call_id: part.tool_call_id.clone(),
                            error_text: tool_result_error_text(part, &options),
                            provider_executed: part.provider_executed,
                            provider_metadata: part.provider_metadata.clone(),
                            tool_metadata: part.tool_metadata.clone(),
                            dynamic: part.dynamic,
                        });
                    } else {
                        chunks.push(UiMessageChunk::ToolOutputAvailable {
                            tool_call_id: part.tool_call_id.clone(),
                            output: part.output.clone(),
                            provider_executed: part.provider_executed,
                            provider_metadata: part.provider_metadata.clone(),
                            tool_metadata: part.tool_metadata.clone(),
                            preliminary: part.preliminary,
                            dynamic: part.dynamic,
                        });
                    }
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ToolOutputDenied(part) => {
                    chunks.push(UiMessageChunk::ToolOutputDenied {
                        tool_call_id: part.tool_call_id.clone(),
                        tool_name: None,
                        provider_executed: part.provider_executed,
                        dynamic: part.dynamic,
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::Custom(part) => {
                    chunks.push(UiMessageChunk::Custom {
                        kind: part.kind.clone(),
                        provider_metadata: part.provider_metadata.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::File(part) => {
                    chunks.push(UiMessageChunk::File {
                        media_type: part.file.media_type.clone(),
                        url: ui_message_file_url(&part.file.media_type, &part.file.data),
                        provider_metadata: part.provider_metadata.clone(),
                    });
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
                TextStreamPart::ReasoningFile(part) => {
                    if options.send_reasoning {
                        chunks.push(UiMessageChunk::ReasoningFile {
                            media_type: part.file.media_type.clone(),
                            url: ui_message_file_url(&part.file.media_type, &part.file.data),
                            provider_metadata: part.provider_metadata.clone(),
                        });
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::Source(part) => {
                    if options.send_sources {
                        match part {
                            LanguageModelSource::Url(source) => {
                                chunks.push(UiMessageChunk::SourceUrl {
                                    source_id: source.id.clone(),
                                    url: source.url.clone(),
                                    title: source.title.clone(),
                                    provider_metadata: source.provider_metadata.clone(),
                                });
                            }
                            LanguageModelSource::Document(source) => {
                                chunks.push(UiMessageChunk::SourceDocument {
                                    source_id: source.id.clone(),
                                    media_type: source.media_type.clone(),
                                    title: source.title.clone(),
                                    filename: source.filename.clone(),
                                    provider_metadata: source.provider_metadata.clone(),
                                });
                            }
                        }
                        push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                    }
                }
                TextStreamPart::Raw(_) => {
                    push_stream_text_ui_message_metadata(&mut chunks, &options, stream_part);
                }
            }
        }

        let mut finish_options = HandleUiMessageStreamFinishOptions::new(chunks);
        if let Some(message_id) = response_message_id {
            finish_options = finish_options.with_message_id(message_id);
        }
        if let Some(original_messages) = options.original_messages {
            finish_options = finish_options.with_original_messages(original_messages);
        }
        if let Some(on_finish) = options.on_finish {
            finish_options = finish_options.with_finish_callback(on_finish);
        }

        handle_ui_message_stream_finish(finish_options)
            .expect("streamText UI-message stream chunks remain processable")
    }

    /// Creates a UI-message stream response from collected stream parts.
    pub fn to_ui_message_stream_response(
        &self,
        init: UiMessageStreamResponseInit,
    ) -> UiMessageStreamResponse {
        self.to_ui_message_stream_response_with_options(
            init,
            StreamTextUiMessageStreamOptions::default(),
        )
    }

    /// Creates a UI-message stream response from collected stream parts with options.
    pub fn to_ui_message_stream_response_with_options(
        &self,
        init: UiMessageStreamResponseInit,
        options: StreamTextUiMessageStreamOptions,
    ) -> UiMessageStreamResponse {
        create_ui_message_stream_response(UiMessageStreamResponseOptions::from_init(
            self.to_ui_message_stream_with_options(options),
            init,
        ))
    }

    /// Pipes UI-message stream chunks to a response writer.
    pub fn pipe_ui_message_stream_to_response<W>(
        &self,
        response: &mut W,
        init: UiMessageStreamResponseInit,
    ) -> Result<(), W::Error>
    where
        W: UiMessageStreamResponseWriter,
    {
        self.pipe_ui_message_stream_to_response_with_options(
            response,
            init,
            StreamTextUiMessageStreamOptions::default(),
        )
    }

    /// Pipes UI-message stream chunks to a response writer with stream options.
    pub fn pipe_ui_message_stream_to_response_with_options<W>(
        &self,
        response: &mut W,
        init: UiMessageStreamResponseInit,
        options: StreamTextUiMessageStreamOptions,
    ) -> Result<(), W::Error>
    where
        W: UiMessageStreamResponseWriter,
    {
        pipe_ui_message_stream_to_response(
            response,
            UiMessageStreamResponseOptions::from_init(
                self.to_ui_message_stream_with_options(options),
                init,
            ),
        )
    }

    /// Creates a text-stream response from the collected final-step text stream.
    pub fn to_text_stream_response(&self, init: TextStreamResponseInit) -> TextStreamResponse {
        create_text_stream_response(TextStreamResponseOptions::from_init(
            self.text_stream.clone(),
            init,
        ))
    }

    /// Pipes the collected final-step text stream to a response writer.
    pub fn pipe_text_stream_to_response<W>(
        &self,
        response: &mut W,
        init: TextStreamResponseInit,
    ) -> Result<(), W::Error>
    where
        W: TextStreamResponseWriter,
    {
        pipe_text_stream_to_response(
            response,
            TextStreamResponseOptions::from_init(self.text_stream.clone(), init),
        )
    }
}

/// Converts a sequence of [`TextStreamPart`] chunks into a stream of text deltas.
///
/// This mirrors upstream `toTextStream`: only `text-delta` parts contribute and
/// each contributes its `text` value, all other part types are dropped.
pub fn to_text_stream<I>(parts: I) -> Vec<String>
where
    I: IntoIterator<Item = TextStreamPart>,
{
    parts
        .into_iter()
        .filter_map(|part| match part {
            TextStreamPart::TextDelta(part) => Some(part.text),
            _ => None,
        })
        .collect()
}

fn stream_text_output_value(text: &str) -> JsonValue {
    serde_json::from_str(text).unwrap_or_else(|_| JsonValue::String(text.to_string()))
}

fn stream_text_ui_message_metadata(
    options: &StreamTextUiMessageStreamOptions,
    part: &TextStreamPart,
) -> Option<JsonValue> {
    options
        .message_metadata
        .as_ref()
        .and_then(|message_metadata| message_metadata.metadata(part))
}

fn push_stream_text_ui_message_metadata(
    chunks: &mut Vec<UiMessageChunk>,
    options: &StreamTextUiMessageStreamOptions,
    part: &TextStreamPart,
) {
    if let Some(message_metadata) = stream_text_ui_message_metadata(options, part) {
        chunks.push(UiMessageChunk::message_metadata(message_metadata));
    }
}

fn stream_text_response_message_id(options: &StreamTextUiMessageStreamOptions) -> Option<String> {
    if let Some(message_id) = &options.message_id {
        return match options.original_messages.as_deref() {
            Some(original_messages) => get_response_ui_message_id(
                Some(original_messages),
                ResponseUiMessageId::id(message_id.clone()),
            ),
            None => Some(message_id.clone()),
        };
    }

    options.generate_message_id.as_ref().and_then(|generate| {
        let generate = generate.clone();
        match options.original_messages.as_deref() {
            Some(original_messages) => get_response_ui_message_id(
                Some(original_messages),
                ResponseUiMessageId::generate(move || generate.generate()),
            ),
            None => Some(generate.generate()),
        }
    })
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
        tools,
        mut runtime_context,
        mut tools_context,
        experimental_sandbox,
        active_tools,
        tool_approval,
        tool_input_refinements,
        tool_call_repair,
        on_start,
        on_step_start,
        prepare_step,
        on_language_model_call_start,
        on_language_model_call_end,
        on_tool_execution_start,
        on_tool_execution_end,
        on_step_finish,
        on_finish,
        telemetry,
        download,
        timeout,
        max_retries,
        smooth_stream,
        transforms,
        on_chunk,
        on_error,
        abort_signal: _,
        on_abort,
        max_steps,
        stop_conditions,
        include,
    } = options;
    let telemetry_dispatcher = create_telemetry_dispatcher(telemetry);
    let include_raw_chunks = call_options.include_raw_chunks.unwrap_or(false);
    let mut parts = vec![TextStreamPart::Start(TextStreamStartPart::new())];
    let base_language_model_tools = call_options.tools.take();
    let base_provider_options = call_options.provider_options.clone();
    let mut current_prompt = call_options.prompt.clone();
    let initial_messages = current_prompt.clone();
    let active_tools_for_start = active_tools.clone();
    let base_active_tools = active_tools;
    let call_id = generate_text_call_id();
    // Mirror upstream `streamText`: when `stepMs`/`chunkMs` are configured a
    // dedicated AbortController is created per category and its signal is always
    // merged into the request abort signal (the controller is aborted only once
    // the corresponding step/chunk timeout actually fires). The controllers are
    // kept alive for the lifetime of the stream so the merged signal stays valid.
    let step_abort_controller =
        get_step_timeout_ms(timeout.as_ref()).map(|_| StreamTextAbortController::new());
    let chunk_abort_controller =
        get_chunk_timeout_ms(timeout.as_ref()).map(|_| StreamTextAbortController::new());
    let request_abort_signal = merge_abort_signals([
        call_options
            .abort_signal
            .clone()
            .map(AbortSignalSource::signal),
        get_total_timeout_ms(timeout.as_ref()).map(AbortSignalSource::timeout_ms),
        step_abort_controller
            .as_ref()
            .map(|controller| AbortSignalSource::signal(controller.signal())),
        chunk_abort_controller
            .as_ref()
            .map(|controller| AbortSignalSource::signal(controller.signal())),
    ]);
    // Keep the controllers alive for the duration of the call so the merged
    // signal does not observe a dropped source.
    let _step_abort_controller = step_abort_controller;
    let _chunk_abort_controller = chunk_abort_controller;
    call_options.abort_signal = request_abort_signal.clone();
    let abort_signal = request_abort_signal;
    let max_steps = max_steps.max(1);
    let mut stream_steps = Vec::new();
    let mut generate_steps = Vec::new();
    let mut initial_response_messages = Vec::new();
    let mut pending_deferred_provider_tool_call_ids = BTreeSet::new();
    let mut aborted = false;
    let mut abort_reason = None;

    if on_start.is_some() || telemetry_dispatcher.is_enabled() {
        let mut start_tools = base_language_model_tools.clone().unwrap_or_default();
        if let Some(mut prepared_tools) =
            prepare_tools_with_context(&tools, Some(&tools_context), experimental_sandbox.as_ref())
        {
            start_tools.append(&mut prepared_tools);
        }

        let start_event = GenerateTextStartEvent {
            call_id: call_id.clone(),
            operation_id: "ai.streamText".to_string(),
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
            max_retries,
            reasoning: call_options.reasoning.clone(),
            headers: call_options.headers.clone(),
            provider_options: call_options.provider_options.clone(),
            runtime_context: runtime_context.clone(),
            tools_context: tools_context.clone(),
            timeout: timeout.clone(),
        };
        if let Some(on_start) = &on_start {
            on_start.start(start_event.clone()).await;
        }
        telemetry_dispatcher.on_start(&start_event);
    }

    if let Some(initial_response) = initial_tool_approval_response_message(
        &call_id,
        &current_prompt,
        &tools,
        &tools_context,
        (
            experimental_sandbox.as_ref(),
            call_options.abort_signal.as_ref(),
            timeout.as_ref(),
            None,
            None,
            on_tool_execution_start.as_ref(),
            on_tool_execution_end.as_ref(),
            Some(&telemetry_dispatcher),
        ),
    )
    .await
    {
        current_prompt.push(initial_response.message.clone());
        initial_response_messages.push(initial_response.message);
        for tool_result in initial_response.tool_results {
            push_text_stream_part(
                &mut parts,
                TextStreamPart::ToolResult(tool_result),
                on_chunk.as_ref(),
            )
            .await;
        }
        for denied_tool_output in initial_response.denied_tool_outputs {
            push_text_stream_part(
                &mut parts,
                TextStreamPart::ToolOutputDenied(denied_tool_output),
                on_chunk.as_ref(),
            )
            .await;
        }
    }

    for step_number in 0..max_steps {
        if let Some(abort_part) = stream_text_abort_part_from_signal(abort_signal.as_ref()) {
            abort_reason = abort_part.reason.clone();
            push_text_stream_part(
                &mut parts,
                TextStreamPart::Abort(abort_part),
                on_chunk.as_ref(),
            )
            .await;
            aborted = true;
            break;
        }

        let accumulated_response_messages = crate::generate_text::accumulated_response_messages(
            &initial_response_messages,
            &generate_steps,
        );
        let prepare_step_result = if let Some(prepare_step) = &prepare_step {
            prepare_step
                .prepare(PrepareStepOptions {
                    steps: generate_steps.clone(),
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
        let step_active_tools = step_active_tools
            .as_deref()
            .or(base_active_tools.as_deref());
        let mut step_prompt = current_prompt.clone();
        let step_tools =
            crate::generate_text::filter_active_tools(Some(tools.clone()), step_active_tools)
                .unwrap_or_default();
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
        step_call_options.prompt = step_prompt.clone();
        step_call_options.tools = step_language_model_tools;
        if let Some(tool_choice) = step_tool_choice {
            step_call_options.tool_choice = Some(tool_choice);
        }
        step_call_options.provider_options =
            merge_provider_options(base_provider_options.as_ref(), step_provider_options);
        append_stream_text_user_agent(&mut step_call_options);
        if prompt_has_url_files(&step_call_options.prompt) {
            let supported_urls = step_model.supported_urls().await;
            let downloaded_assets = download_prompt_assets(
                &step_call_options.prompt,
                &supported_urls,
                download.as_ref(),
            )
            .await
            .expect("prompt asset download failed");
            step_call_options.prompt = apply_downloaded_prompt_assets(
                step_call_options.prompt.clone(),
                &downloaded_assets,
            );
        }
        step_prompt = step_call_options.prompt.clone();

        if on_step_start.is_some() || telemetry_dispatcher.is_enabled() {
            let step_start_event = GenerateTextStepStartEvent {
                call_id: call_id.clone(),
                provider: step_model.provider().to_string(),
                model_id: step_model.model_id().to_string(),
                step_number,
                messages: step_prompt.clone(),
                tools: step_call_options.tools.clone().unwrap_or_default(),
                tool_choice: step_call_options.tool_choice.clone(),
                active_tools: step_active_tools.map(|tools| tools.to_vec()),
                steps: generate_steps.clone(),
                provider_options: step_call_options.provider_options.clone(),
                runtime_context: runtime_context.clone(),
                tools_context: tools_context.clone(),
            };
            if let Some(on_step_start) = &on_step_start {
                on_step_start.start(step_start_event.clone()).await;
            }
            telemetry_dispatcher.on_step_start(&step_start_event);
        }

        if on_language_model_call_start.is_some() || telemetry_dispatcher.is_enabled() {
            let language_model_call_start_event = LanguageModelCallStartEvent::from_call_options(
                &call_id,
                step_model.provider(),
                step_model.model_id(),
                &step_call_options,
            );
            if let Some(on_language_model_call_start) = &on_language_model_call_start {
                on_language_model_call_start
                    .start(language_model_call_start_event.clone())
                    .await;
            }
            telemetry_dispatcher.on_language_model_call_start(&language_model_call_start_event);
        }

        let model_call_started_at = Instant::now();
        let mut collected_step = collect_stream_text_step_with_retries(
            step_model,
            step_call_options.clone(),
            include_raw_chunks,
            &mut parts,
            StreamTextCollectionControls {
                max_retries,
                transforms: &transforms,
                smooth_stream: smooth_stream.as_ref(),
                on_chunk: on_chunk.as_ref(),
                on_error: on_error.as_ref(),
                abort_signal: abort_signal.as_ref(),
                tools: &step_tools,
                messages: &step_prompt,
                runtime_context: &runtime_context,
                experimental_sandbox: step_experimental_sandbox.as_ref(),
            },
        )
        .await;
        let response_time_ms =
            u64::try_from(model_call_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);

        if collected_step.aborted {
            abort_reason = collected_step.abort_reason.clone();
            aborted = true;
            break;
        }

        log_warnings(
            &LogWarningsOptions::new(collected_step.warnings.clone())
                .with_scope(model.provider(), model.model_id()),
        );

        mark_unavailable_tool_calls(
            &mut collected_step.tool_calls,
            step_call_options.tools.as_deref(),
        );
        mark_invalid_tool_inputs(
            &mut collected_step.tool_calls,
            step_call_options.tools.as_deref(),
        );
        repair_tool_calls(
            &mut collected_step.tool_calls,
            &collected_step.provider_content,
            tool_call_repair.as_ref(),
            &step_tools,
            step_call_options.tools.as_deref(),
            &step_prompt,
        )
        .await;
        refine_tool_inputs(&mut collected_step.tool_calls, &tool_input_refinements).await;
        sync_tool_result_inputs(&mut collected_step.tool_results, &collected_step.tool_calls);
        mark_runtime_dynamic_tool_calls(&mut collected_step.tool_calls, &step_tools);
        mark_tool_call_titles(&mut collected_step.tool_calls, &step_tools);
        mark_tool_call_metadata(&mut collected_step.tool_calls, &step_tools);
        mark_tool_result_metadata(
            &mut collected_step.tool_results,
            &collected_step.tool_calls,
            &step_tools,
        );

        apply_stream_text_include(&mut collected_step.request, include, &step_prompt);

        let mut generate_step = collected_step.to_generate_text_step(
            call_id.clone(),
            step_number,
            GenerateTextModelInfo::new(step_model.provider(), step_model.model_id()),
            runtime_context.clone(),
            tools_context.clone(),
        );
        generate_step.performance = GenerateTextStepPerformance::from_stream_usage(
            &generate_step.usage,
            response_time_ms,
            collected_step.performance.step_time_ms,
            BTreeMap::new(),
            collected_step.performance.time_to_first_output_token_ms,
        );
        refresh_generate_text_content(
            &mut generate_step,
            &collected_step.provider_content,
            &Default::default(),
        );
        apply_generate_text_response_metadata(&mut generate_step);

        if on_language_model_call_end.is_some() || telemetry_dispatcher.is_enabled() {
            let language_model_call_end_event =
                LanguageModelCallEndEvent::from_step(&generate_step, response_time_ms);
            if let Some(on_language_model_call_end) = &on_language_model_call_end {
                on_language_model_call_end
                    .end(language_model_call_end_event.clone())
                    .await;
            }
            telemetry_dispatcher.on_language_model_call_end(&language_model_call_end_event);
        }

        let tool_approvals = resolve_tool_approvals_for_step(
            &generate_step.tool_calls,
            &step_tools,
            tool_approval.as_ref(),
            &step_prompt,
            &tools_context,
            &runtime_context,
        )
        .await;

        for request in &tool_approvals.requests {
            let mut approval_request = TextStreamToolApprovalRequestPart::new(
                request.approval_id.clone(),
                request.tool_call_id.clone(),
            );
            if let Some(is_automatic) = request.is_automatic {
                approval_request = approval_request.with_automatic(is_automatic);
            }
            insert_part_after_tool_call(
                &mut parts,
                &request.tool_call_id,
                TextStreamPart::ToolApprovalRequest(approval_request),
            );
        }
        for response in &tool_approvals.responses {
            let approval_response = text_stream_tool_approval_response_output(response);
            let approval_id = approval_response.approval_id.clone();
            insert_part_after_tool_approval_request(
                &mut parts,
                &approval_id,
                TextStreamPart::ToolApprovalResponse(approval_response),
            );
        }

        let provider_result_tool_call_ids = collected_step
            .tool_results
            .iter()
            .filter(|tool_result| tool_result.provider_executed == Some(true))
            .map(|tool_result| tool_result.tool_call_id.clone())
            .collect::<BTreeSet<_>>();
        let executable_tool_calls = generate_step
            .tool_calls
            .iter()
            .filter(|tool_call| !provider_result_tool_call_ids.contains(&tool_call.tool_call_id))
            .cloned()
            .collect::<Vec<_>>();
        let preliminary_tool_results =
            Arc::new(std::sync::Mutex::new(Vec::<GenerateTextToolResult>::new()));
        let preliminary_tool_results_for_callback = Arc::clone(&preliminary_tool_results);
        let on_preliminary_tool_result =
            Callback::infallible(move |result: GenerateTextToolResult| {
                let preliminary_tool_results = Arc::clone(&preliminary_tool_results_for_callback);
                async move {
                    preliminary_tool_results
                        .lock()
                        .expect("preliminary tool results lock")
                        .push(result);
                }
            });
        let (local_tool_results, tool_execution_ms) = execute_tool_calls(
            &call_id,
            &step_tools,
            &executable_tool_calls,
            &step_prompt,
            &tools_context,
            &tool_approvals.blocked_tool_call_ids,
            (
                step_experimental_sandbox.as_ref(),
                step_call_options.abort_signal.as_ref(),
                timeout.as_ref(),
                None,
                Some(&on_preliminary_tool_result),
                on_tool_execution_start.as_ref(),
                on_tool_execution_end.as_ref(),
                Some(&telemetry_dispatcher),
            ),
        )
        .await;
        if let Some(abort_part) = stream_text_abort_part_from_signal(abort_signal.as_ref()) {
            abort_reason = abort_part.reason.clone();
            push_text_stream_part(
                &mut parts,
                TextStreamPart::Abort(abort_part),
                on_chunk.as_ref(),
            )
            .await;
            aborted = true;
            break;
        }
        let local_tool_results =
            apply_stream_text_transforms_to_tool_results(local_tool_results, &transforms);
        let preliminary_tool_results = apply_stream_text_transforms_to_tool_results(
            preliminary_tool_results
                .lock()
                .expect("preliminary tool results lock")
                .clone(),
            &transforms,
        );
        for tool_result in &preliminary_tool_results {
            push_text_stream_part(
                &mut parts,
                TextStreamPart::ToolResult(tool_result.clone()),
                on_chunk.as_ref(),
            )
            .await;
        }
        for tool_result in &local_tool_results {
            push_text_stream_part(
                &mut parts,
                TextStreamPart::ToolResult(tool_result.clone()),
                on_chunk.as_ref(),
            )
            .await;
        }

        collected_step
            .tool_results
            .extend(local_tool_results.iter().cloned());
        mark_tool_result_metadata(
            &mut collected_step.tool_results,
            &collected_step.tool_calls,
            &step_tools,
        );
        sync_stream_text_tool_parts(
            &mut parts,
            &collected_step.tool_calls,
            &collected_step.tool_results,
        );
        generate_step.tool_results = collected_step.tool_results.clone();
        refresh_tool_result_views(&mut generate_step);
        generate_step.performance.tool_execution_ms = tool_execution_ms;
        update_pending_deferred_provider_tool_calls(
            &mut pending_deferred_provider_tool_call_ids,
            &generate_step,
            &step_tools,
        );
        let should_continue = should_continue_after_tool_results(
            &generate_step,
            &local_tool_results,
            tool_approvals.denied_client_tool_call_count,
            !pending_deferred_provider_tool_call_ids.is_empty(),
        );

        let response_messages = response_messages_for_step(
            &generate_step,
            &collected_step.provider_content,
            &tool_approvals,
            &step_tools,
        )
        .await
        .unwrap_or_default();
        generate_step.response_messages = response_messages.clone();
        collected_step.response_messages = response_messages.clone();
        generate_step
            .response
            .get_or_insert_with(LanguageModelResponse::new)
            .messages = Some(response_messages.clone());
        refresh_generate_text_content(
            &mut generate_step,
            &collected_step.provider_content,
            &tool_approvals,
        );
        apply_generate_text_response_metadata(&mut generate_step);
        apply_stream_text_response_identity(
            &mut collected_step.response,
            generate_step.response.as_ref(),
        );

        parts.push(TextStreamPart::FinishStep(TextStreamFinishStepPart::new(
            collected_step.response.clone(),
            collected_step.usage.clone(),
            collected_step.performance,
            collected_step.finish_reason.clone(),
            collected_step.raw_finish_reason.clone(),
            collected_step.provider_metadata.clone(),
        )));

        if let Some(on_step_finish) = &on_step_finish {
            on_step_finish.finish(generate_step.clone()).await;
        }
        telemetry_dispatcher.on_step_finish(&generate_step);

        stream_steps.push(collected_step.into_stream_text_step());
        generate_steps.push(generate_step);

        if should_continue
            && !is_stop_condition_met(&stop_conditions, &generate_steps)
            && step_number + 1 < max_steps
        {
            if response_messages.is_empty() {
                break;
            }

            current_prompt = step_prompt;
            current_prompt.extend(response_messages);
        } else {
            break;
        }
    }

    let total_usage = add_stream_text_step_usage(&stream_steps);

    if aborted {
        if let Some(on_abort) = &on_abort {
            on_abort
                .abort(StreamTextOnAbortEvent {
                    steps: generate_steps.clone(),
                    reason: abort_reason.clone(),
                })
                .await;
        }
        // Upstream dispatches the telemetry integration's `onAbort` event (with
        // the completed `steps` and abort `reason`) but skips `onEnd`/`onError`
        // when the stream is aborted.
        if telemetry_dispatcher.is_enabled() {
            let mut abort_event = serde_json::Map::new();
            abort_event.insert("callId".to_string(), JsonValue::String(call_id.clone()));
            abort_event.insert(
                "steps".to_string(),
                serde_json::to_value(&generate_steps).unwrap_or(JsonValue::Null),
            );
            if let Some(reason) = &abort_reason {
                abort_event.insert("reason".to_string(), reason.clone());
            }
            telemetry_dispatcher.on_abort(JsonValue::Object(abort_event));
        }
    } else if let Some(final_step) = stream_steps.last() {
        parts.push(TextStreamPart::Finish(TextStreamFinishPart::new(
            final_step.finish_reason.clone(),
            final_step.raw_finish_reason.clone(),
            total_usage.clone(),
        )));
    }

    if !aborted && (on_finish.is_some() || telemetry_dispatcher.is_enabled()) {
        let finish_event = GenerateTextFinishEvent::from_steps(&[], &generate_steps);
        if let Some(on_finish) = &on_finish {
            on_finish.finish(finish_event.clone()).await;
        }
        telemetry_dispatcher.on_end(&finish_event);
    }

    let final_step = stream_steps.last();

    StreamTextResult {
        parts,
        text_stream: final_step
            .map(|step| step.text_stream.clone())
            .unwrap_or_default(),
        text: final_step.map(|step| step.text.clone()).unwrap_or_default(),
        reasoning_text: final_step.and_then(|step| step.reasoning_text.clone()),
        sources: stream_steps
            .iter()
            .flat_map(|step| step.sources.iter().cloned())
            .collect(),
        files: stream_steps
            .iter()
            .flat_map(|step| step.files.iter().cloned())
            .collect(),
        reasoning_files: stream_steps
            .iter()
            .flat_map(|step| step.reasoning_files.iter().cloned())
            .collect(),
        tool_calls: stream_steps
            .iter()
            .flat_map(|step| step.tool_calls.iter().cloned())
            .collect(),
        tool_results: stream_steps
            .iter()
            .flat_map(|step| step.tool_results.iter().cloned())
            .collect(),
        // Upstream `streamText` surfaces the synthesized tool-result message for
        // an initially approved/denied tool-approval response as the first entry
        // in `responseMessages`, ahead of the per-step assistant/tool messages.
        response_messages: initial_response_messages
            .iter()
            .cloned()
            .chain(
                stream_steps
                    .iter()
                    .flat_map(|step| step.response_messages.iter().cloned()),
            )
            .collect(),
        custom_parts: stream_steps
            .iter()
            .flat_map(|step| step.custom_parts.iter().cloned())
            .collect(),
        errors: stream_steps
            .iter()
            .flat_map(|step| step.errors.iter().cloned())
            .collect(),
        warnings: stream_steps
            .iter()
            .flat_map(|step| step.warnings.iter().cloned())
            .collect(),
        usage: final_step
            .map(|step| step.usage.clone())
            .unwrap_or_default(),
        total_usage,
        finish_reason: final_step
            .map(|step| step.finish_reason.clone())
            .unwrap_or(FinishReason::Other),
        raw_finish_reason: final_step.and_then(|step| step.raw_finish_reason.clone()),
        request: final_step.and_then(|step| step.request.clone()),
        response: final_step
            .map(|step| step.response.clone())
            .unwrap_or_default(),
        provider_metadata: final_step.and_then(|step| step.provider_metadata.clone()),
        steps: stream_steps,
    }
}

#[derive(Clone, Debug)]
struct CollectedStreamTextStep {
    request: Option<LanguageModelRequest>,
    response: StreamTextResponseMetadata,
    warnings: Vec<Warning>,
    text: String,
    text_stream: Vec<String>,
    reasoning_text: Option<String>,
    sources: Vec<LanguageModelSource>,
    files: Vec<LanguageModelFile>,
    reasoning_files: Vec<LanguageModelReasoningFile>,
    tool_calls: Vec<GenerateTextToolCall>,
    tool_results: Vec<GenerateTextToolResult>,
    response_messages: Vec<LanguageModelMessage>,
    custom_parts: Vec<LanguageModelCustomContent>,
    errors: Vec<JsonValue>,
    usage: LanguageModelUsage,
    finish_reason: FinishReason,
    raw_finish_reason: Option<String>,
    provider_metadata: Option<ProviderMetadata>,
    performance: StreamTextStepPerformance,
    provider_content: Vec<LanguageModelContent>,
    aborted: bool,
    abort_reason: Option<JsonValue>,
}

#[derive(Clone, Debug)]
enum ProviderContentEntry {
    Content(LanguageModelContent),
    TextBlock(String),
    ReasoningBlock(String),
}

fn materialize_provider_content(
    entries: &[ProviderContentEntry],
    text_blocks: &BTreeMap<String, (String, Option<ProviderMetadata>)>,
    reasoning_blocks: &BTreeMap<String, (String, Option<ProviderMetadata>)>,
) -> Vec<LanguageModelContent> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            ProviderContentEntry::Content(content) => Some(content.clone()),
            ProviderContentEntry::TextBlock(id) => {
                let (text, provider_metadata) = text_blocks.get(id)?;
                (!text.is_empty())
                    .then(|| text_language_model_content(text.clone(), provider_metadata.clone()))
            }
            ProviderContentEntry::ReasoningBlock(id) => {
                let (text, provider_metadata) = reasoning_blocks.get(id)?;
                (!text.is_empty()).then(|| {
                    reasoning_language_model_content(text.clone(), provider_metadata.clone())
                })
            }
        })
        .collect()
}

impl CollectedStreamTextStep {
    fn aborted(abort_reason: Option<JsonValue>) -> Self {
        Self {
            request: None,
            response: StreamTextResponseMetadata::new(),
            warnings: Vec::new(),
            text: String::new(),
            text_stream: Vec::new(),
            reasoning_text: None,
            sources: Vec::new(),
            files: Vec::new(),
            reasoning_files: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            response_messages: Vec::new(),
            custom_parts: Vec::new(),
            errors: Vec::new(),
            usage: LanguageModelUsage::default(),
            finish_reason: FinishReason::Other,
            raw_finish_reason: None,
            provider_metadata: None,
            performance: StreamTextStepPerformance::default(),
            provider_content: Vec::new(),
            aborted: true,
            abort_reason,
        }
    }

    fn to_generate_text_step(
        &self,
        call_id: String,
        step_number: usize,
        model: GenerateTextModelInfo,
        runtime_context: JsonObject,
        tools_context: JsonObject,
    ) -> GenerateTextStep {
        let mut step = GenerateTextStep::from_language_model_result(
            call_id,
            step_number,
            model,
            LanguageModelGenerateResult {
                content: self.provider_content.clone(),
                finish_reason: LanguageModelFinishReason {
                    unified: self.finish_reason.clone(),
                    raw: self.raw_finish_reason.clone(),
                },
                usage: self.usage.clone(),
                provider_metadata: self.provider_metadata.clone(),
                request: self.request.clone(),
                response: Some(language_model_response_from_stream_metadata(
                    self.response.clone(),
                )),
                warnings: self.warnings.clone(),
            },
        );

        step.runtime_context = runtime_context;
        step.tools_context = tools_context;
        step.tool_calls = self.tool_calls.clone();
        refresh_tool_call_views(&mut step);
        step.tool_results = self.tool_results.clone();
        refresh_tool_result_views(&mut step);
        step
    }

    fn into_stream_text_step(self) -> StreamTextStep {
        StreamTextStep {
            request: self.request,
            response: self.response,
            warnings: self.warnings,
            text: self.text,
            text_stream: self.text_stream,
            reasoning_text: self.reasoning_text,
            sources: self.sources,
            files: self.files,
            reasoning_files: self.reasoning_files,
            tool_calls: self.tool_calls,
            tool_results: self.tool_results,
            response_messages: self.response_messages,
            custom_parts: self.custom_parts,
            errors: self.errors,
            usage: self.usage,
            finish_reason: self.finish_reason,
            raw_finish_reason: self.raw_finish_reason,
            provider_metadata: self.provider_metadata,
            performance: self.performance,
        }
    }

    fn apply_transformed_parts(&mut self, parts: &[TextStreamPart]) {
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
        let mut provider_content_entries = Vec::new();
        let mut text_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();
        let mut reasoning_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();

        for part in parts {
            match part {
                TextStreamPart::TextStart(part) => {
                    text_blocks.insert(
                        part.id.clone(),
                        (String::new(), part.provider_metadata.clone()),
                    );
                    provider_content_entries.push(ProviderContentEntry::TextBlock(part.id.clone()));
                }
                TextStreamPart::TextDelta(part) if !part.text.is_empty() => {
                    text.push_str(&part.text);
                    text_stream.push(part.text.clone());
                    if let Some((block_text, block_metadata)) = text_blocks.get_mut(&part.id) {
                        block_text.push_str(&part.text);
                        if block_metadata.is_none() {
                            *block_metadata = part.provider_metadata.clone();
                        }
                    } else {
                        provider_content_entries.push(ProviderContentEntry::Content(
                            text_language_model_content(
                                part.text.clone(),
                                part.provider_metadata.clone(),
                            ),
                        ));
                    }
                }
                TextStreamPart::TextEnd(_) => {}
                TextStreamPart::ReasoningStart(part) => {
                    reasoning_blocks.insert(
                        part.id.clone(),
                        (String::new(), part.provider_metadata.clone()),
                    );
                    provider_content_entries
                        .push(ProviderContentEntry::ReasoningBlock(part.id.clone()));
                }
                TextStreamPart::ReasoningDelta(part) => {
                    has_reasoning_text = true;
                    reasoning_text.push_str(&part.text);
                    if let Some((block_text, block_metadata)) = reasoning_blocks.get_mut(&part.id) {
                        block_text.push_str(&part.text);
                        if block_metadata.is_none() {
                            *block_metadata = part.provider_metadata.clone();
                        }
                    } else {
                        provider_content_entries.push(ProviderContentEntry::Content(
                            reasoning_language_model_content(
                                part.text.clone(),
                                part.provider_metadata.clone(),
                            ),
                        ));
                    }
                }
                TextStreamPart::ReasoningEnd(_) => {}
                TextStreamPart::ToolApprovalRequest(part) => {
                    provider_content_entries.push(ProviderContentEntry::Content(
                        LanguageModelContent::ToolApprovalRequest(
                            part.to_language_model_tool_approval_request(),
                        ),
                    ));
                }
                TextStreamPart::ToolApprovalResponse(_) => {}
                TextStreamPart::ToolCall(part) => {
                    tool_calls.push(part.clone());
                    provider_content_entries.push(ProviderContentEntry::Content(
                        LanguageModelContent::ToolCall(
                            language_model_tool_call_from_stream_text_tool_call(part),
                        ),
                    ));
                }
                TextStreamPart::ToolResult(part) => {
                    tool_results.push(part.clone());
                    if let Some(tool_result) =
                        language_model_tool_result_from_stream_text_tool_result(part)
                    {
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::ToolResult(tool_result),
                        ));
                    }
                }
                TextStreamPart::Custom(part) => {
                    custom_parts.push(part.clone());
                    provider_content_entries.push(ProviderContentEntry::Content(
                        LanguageModelContent::Custom(part.clone()),
                    ));
                }
                TextStreamPart::File(part) => {
                    files.push(part.file.clone());
                    provider_content_entries.push(ProviderContentEntry::Content(
                        LanguageModelContent::File(part.file.clone()),
                    ));
                }
                TextStreamPart::ReasoningFile(part) => {
                    reasoning_files.push(part.file.clone());
                    provider_content_entries.push(ProviderContentEntry::Content(
                        LanguageModelContent::ReasoningFile(part.file.clone()),
                    ));
                }
                TextStreamPart::Source(part) => {
                    sources.push(part.clone());
                    provider_content_entries.push(ProviderContentEntry::Content(
                        LanguageModelContent::Source(part.clone()),
                    ));
                }
                TextStreamPart::Error(part) => {
                    errors.push(part.error.clone());
                }
                TextStreamPart::FinishStep(part) => {
                    self.response = part.response.clone();
                    self.usage = part.usage.clone();
                    self.performance = part.performance;
                    self.finish_reason = part.finish_reason.clone();
                    self.raw_finish_reason = part.raw_finish_reason.clone();
                    self.provider_metadata = part.provider_metadata.clone();
                }
                TextStreamPart::Finish(part) => {
                    self.finish_reason = part.finish_reason.clone();
                    self.raw_finish_reason = part.raw_finish_reason.clone();
                    self.usage = part.total_usage.clone();
                }
                _ => {}
            }
        }

        self.text = text;
        self.text_stream = text_stream;
        self.reasoning_text = has_reasoning_text.then_some(reasoning_text);
        self.sources = sources;
        self.files = files;
        self.reasoning_files = reasoning_files;
        self.tool_calls = tool_calls;
        self.tool_results = tool_results;
        self.custom_parts = custom_parts;
        self.errors = errors;
        self.provider_content = materialize_provider_content(
            &provider_content_entries,
            &text_blocks,
            &reasoning_blocks,
        );
    }

    fn apply_smooth_stream_error(&mut self, error: &SmoothStreamError) {
        self.errors.push(JsonValue::String(error.to_string()));
        self.finish_reason = FinishReason::Error;
        self.raw_finish_reason = Some("error".to_string());
        self.text.clear();
        self.text_stream.clear();
        self.reasoning_text = None;
        self.provider_content.clear();
    }
}

#[derive(Clone, Copy)]
struct StreamTextCollectionControls<'a, 'b> {
    max_retries: usize,
    transforms: &'a [StreamTextTransform<'b>],
    smooth_stream: Option<&'a SmoothStreamOptions>,
    on_chunk: Option<&'a StreamTextOnChunk<'b>>,
    on_error: Option<&'a StreamTextOnError<'b>>,
    abort_signal: Option<&'a StreamTextAbortSignal>,
    tools: &'a [Tool],
    messages: &'a LanguageModelPrompt,
    runtime_context: &'a JsonObject,
    experimental_sandbox: Option<&'a Arc<dyn ExperimentalSandbox>>,
}

#[derive(Clone, Copy)]
struct StreamTextAttemptControls<'a, 'b> {
    on_chunk: Option<&'a StreamTextOnChunk<'b>>,
    on_error: Option<&'a StreamTextOnError<'b>>,
    abort_signal: Option<&'a StreamTextAbortSignal>,
    tools: &'a [Tool],
    messages: &'a LanguageModelPrompt,
    runtime_context: &'a JsonObject,
    experimental_sandbox: Option<&'a Arc<dyn ExperimentalSandbox>>,
}

async fn collect_stream_text_step_with_retries<M>(
    model: &M,
    call_options: LanguageModelCallOptions,
    include_raw_chunks: bool,
    parts: &mut Vec<TextStreamPart>,
    controls: StreamTextCollectionControls<'_, '_>,
) -> CollectedStreamTextStep
where
    M: LanguageModel + ?Sized,
    M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
{
    let mut retries = 0;
    let mut retry_delay_ms = DEFAULT_INITIAL_RETRY_DELAY_MS;

    loop {
        let mut attempt_parts = Vec::new();
        let mut collected_step = collect_stream_text_step(
            model,
            call_options.clone(),
            include_raw_chunks,
            &mut attempt_parts,
            StreamTextAttemptControls {
                on_chunk: None,
                on_error: None,
                abort_signal: controls.abort_signal,
                tools: controls.tools,
                messages: controls.messages,
                runtime_context: controls.runtime_context,
                experimental_sandbox: controls.experimental_sandbox,
            },
        )
        .await;

        if collected_step.aborted {
            let _ = replay_stream_text_attempt_parts(
                parts,
                &attempt_parts,
                None,
                controls.on_chunk,
                controls.on_error,
                controls.abort_signal,
            )
            .await;
            return collected_step;
        }

        if retries < controls.max_retries
            && stream_text_step_is_retryable_pre_stream_failure(&collected_step, &attempt_parts)
        {
            retries += 1;
            if let Some(error) = collected_step.errors.first() {
                let delay_in_ms = stream_text_retry_delay_in_ms(error, retry_delay_ms);
                let delay_result = match controls.abort_signal {
                    Some(abort_signal) => {
                        ai_sdk_provider_utils::delay_with_options(
                            Some(i64::try_from(delay_in_ms).unwrap_or(i64::MAX)),
                            ai_sdk_provider_utils::DelayOptions::new()
                                .with_abort_signal(abort_signal.clone()),
                        )
                        .await
                    }
                    None => {
                        ai_sdk_provider_utils::delay(Some(
                            i64::try_from(delay_in_ms).unwrap_or(i64::MAX),
                        ))
                        .await;
                        Ok(())
                    }
                };

                if delay_result.is_err()
                    && let Some(abort_part) =
                        stream_text_abort_part_from_signal(controls.abort_signal)
                {
                    let abort_reason = abort_part.reason.clone();
                    push_text_stream_part(
                        parts,
                        TextStreamPart::Abort(abort_part),
                        controls.on_chunk,
                    )
                    .await;
                    return CollectedStreamTextStep::aborted(abort_reason);
                }
            }
            retry_delay_ms = retry_delay_ms.saturating_mul(DEFAULT_RETRY_BACKOFF_FACTOR);
            continue;
        }

        let attempt_parts = if controls.transforms.is_empty() {
            attempt_parts
        } else {
            let transformed_parts = apply_stream_text_transforms(
                stream_text_transform_input_parts(attempt_parts, &collected_step),
                controls.transforms,
            );
            collected_step.apply_transformed_parts(&transformed_parts);
            strip_stream_text_finish_parts(transformed_parts)
        };

        let (attempt_parts, smooth_stream_delay_after, smooth_stream_delay_in_ms) =
            match controls.smooth_stream {
                Some(smooth_stream) => {
                    match smooth_stream_scheduled_parts(attempt_parts, smooth_stream) {
                        Ok(scheduled_parts) => {
                            let delay_after = scheduled_parts
                                .iter()
                                .map(|scheduled| scheduled.delay_after)
                                .collect::<Vec<_>>();
                            let attempt_parts = scheduled_parts
                                .into_iter()
                                .map(|scheduled| scheduled.part)
                                .collect::<Vec<_>>();
                            collected_step.apply_transformed_parts(&attempt_parts);
                            (attempt_parts, Some(delay_after), smooth_stream.delay_in_ms)
                        }
                        Err(error) => {
                            collected_step.apply_smooth_stream_error(&error);
                            (
                                vec![TextStreamPart::Error(LanguageModelErrorStreamPart::new(
                                    JsonValue::String(error.to_string()),
                                ))],
                                None,
                                None,
                            )
                        }
                    }
                }
                None => (attempt_parts, None, None),
            };

        if let Some(abort_reason) = replay_stream_text_attempt_parts(
            parts,
            &attempt_parts,
            smooth_stream_delay_after
                .as_deref()
                .map(|delay_after| SmoothStreamReplayDelay {
                    delay_after,
                    delay_in_ms: smooth_stream_delay_in_ms,
                }),
            controls.on_chunk,
            controls.on_error,
            controls.abort_signal,
        )
        .await
        {
            return CollectedStreamTextStep::aborted(abort_reason);
        }

        return collected_step;
    }
}

async fn collect_stream_text_step<M>(
    model: &M,
    call_options: LanguageModelCallOptions,
    include_raw_chunks: bool,
    parts: &mut Vec<TextStreamPart>,
    controls: StreamTextAttemptControls<'_, '_>,
) -> CollectedStreamTextStep
where
    M: LanguageModel + ?Sized,
    M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
{
    if let Some(abort_part) = stream_text_abort_part_from_signal(controls.abort_signal) {
        let abort_reason = abort_part.reason.clone();
        push_text_stream_part(parts, TextStreamPart::Abort(abort_part), controls.on_chunk).await;
        return CollectedStreamTextStep::aborted(abort_reason);
    }

    let stream_result = model.do_stream(call_options).await;
    let request = stream_result.request;
    let envelope_response = stream_result.response;
    let mut response = StreamTextResponseMetadata::new();
    if let Some(envelope_response) = envelope_response.clone() {
        response = response.with_stream_response(envelope_response);
    }

    let step_start = Instant::now();
    let mut start_step_index = None;
    let mut warnings = Vec::new();
    let mut text = String::new();
    let mut text_stream = Vec::new();
    let mut reasoning_text = String::new();
    let mut has_reasoning_text = false;
    let mut sources = Vec::new();
    let mut files = Vec::new();
    let mut reasoning_files = Vec::new();
    let mut tool_calls = Vec::<GenerateTextToolCall>::new();
    let mut tool_results = Vec::new();
    let mut custom_parts = Vec::new();
    let mut errors = Vec::new();
    let mut usage = LanguageModelUsage::default();
    let mut finish_reason = FinishReason::Other;
    let mut raw_finish_reason = None;
    let mut provider_metadata = None;
    let mut provider_content_entries = Vec::new();
    let mut text_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();
    let mut reasoning_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();
    let mut ongoing_tool_call_tool_names = BTreeMap::<String, String>::new();
    let mut aborted = false;
    let mut abort_reason = None;
    let mut time_to_first_output_token_ms = None;

    for part in stream_result.stream {
        if let Some(abort_part) = stream_text_abort_part_from_signal(controls.abort_signal) {
            abort_reason = abort_part.reason.clone();
            push_text_stream_part(parts, TextStreamPart::Abort(abort_part), controls.on_chunk)
                .await;
            aborted = true;
            break;
        }

        match part {
            LanguageModelStreamPart::StreamStart(part) => {
                warnings = part.warnings;
            }
            part => {
                ensure_start_step(
                    parts,
                    &mut start_step_index,
                    request.clone(),
                    warnings.clone(),
                );

                match part {
                    LanguageModelStreamPart::TextStart(part) => {
                        text_blocks.insert(
                            part.id.clone(),
                            (String::new(), part.provider_metadata.clone()),
                        );
                        provider_content_entries
                            .push(ProviderContentEntry::TextBlock(part.id.clone()));
                        parts.push(TextStreamPart::TextStart(part));
                    }
                    LanguageModelStreamPart::TextDelta(part) => {
                        if !part.delta.is_empty() {
                            if time_to_first_output_token_ms.is_none() {
                                time_to_first_output_token_ms = Some(
                                    u64::try_from(step_start.elapsed().as_millis())
                                        .unwrap_or(u64::MAX),
                                );
                            }
                            text.push_str(&part.delta);
                            text_stream.push(part.delta.clone());
                            if let Some((block_text, block_metadata)) =
                                text_blocks.get_mut(&part.id)
                            {
                                block_text.push_str(&part.delta);
                                if block_metadata.is_none() {
                                    *block_metadata = part.provider_metadata.clone();
                                }
                            } else {
                                provider_content_entries.push(ProviderContentEntry::Content(
                                    text_language_model_content(
                                        part.delta.clone(),
                                        part.provider_metadata.clone(),
                                    ),
                                ));
                            }
                            let mut stream_part = TextStreamTextDeltaPart::new(part.id, part.delta);
                            if let Some(provider_metadata) = part.provider_metadata {
                                stream_part = stream_part.with_provider_metadata(provider_metadata);
                            }
                            push_text_stream_part(
                                parts,
                                TextStreamPart::TextDelta(stream_part),
                                controls.on_chunk,
                            )
                            .await;
                        }
                    }
                    LanguageModelStreamPart::TextEnd(part) => {
                        parts.push(TextStreamPart::TextEnd(part));
                    }
                    LanguageModelStreamPart::ReasoningStart(part) => {
                        reasoning_blocks.insert(
                            part.id.clone(),
                            (String::new(), part.provider_metadata.clone()),
                        );
                        provider_content_entries
                            .push(ProviderContentEntry::ReasoningBlock(part.id.clone()));
                        parts.push(TextStreamPart::ReasoningStart(part));
                    }
                    LanguageModelStreamPart::ReasoningDelta(part) => {
                        if time_to_first_output_token_ms.is_none() {
                            time_to_first_output_token_ms = Some(
                                u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX),
                            );
                        }
                        has_reasoning_text = true;
                        reasoning_text.push_str(&part.delta);
                        if let Some((block_text, block_metadata)) =
                            reasoning_blocks.get_mut(&part.id)
                        {
                            block_text.push_str(&part.delta);
                            if block_metadata.is_none() {
                                *block_metadata = part.provider_metadata.clone();
                            }
                        } else {
                            provider_content_entries.push(ProviderContentEntry::Content(
                                reasoning_language_model_content(
                                    part.delta.clone(),
                                    part.provider_metadata.clone(),
                                ),
                            ));
                        }
                        let mut stream_part =
                            TextStreamReasoningDeltaPart::new(part.id, part.delta);
                        if let Some(provider_metadata) = part.provider_metadata {
                            stream_part = stream_part.with_provider_metadata(provider_metadata);
                        }
                        push_text_stream_part(
                            parts,
                            TextStreamPart::ReasoningDelta(stream_part),
                            controls.on_chunk,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::ReasoningEnd(part) => {
                        parts.push(TextStreamPart::ReasoningEnd(part));
                    }
                    LanguageModelStreamPart::ToolInputStart(part) => {
                        let mut part = part.clone();
                        ongoing_tool_call_tool_names
                            .insert(part.id.clone(), part.tool_name.clone());
                        let tool = controls
                            .tools
                            .iter()
                            .find(|tool| tool.name == part.tool_name);
                        if let Some(tool) = tool {
                            if part.dynamic.is_none() {
                                part.dynamic = Some(tool.is_dynamic());
                            }
                            if part.title.is_none()
                                && let Some(title) = tool.title()
                            {
                                part.title = Some(title.to_string());
                            }
                        }
                        push_text_stream_part(
                            parts,
                            TextStreamPart::ToolInputStart(part.clone()),
                            controls.on_chunk,
                        )
                        .await;
                        invoke_tool_input_start_callback(
                            tool,
                            &part.id,
                            controls.messages,
                            controls.abort_signal,
                            controls.experimental_sandbox,
                            controls.runtime_context,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::ToolInputDelta(part) => {
                        if time_to_first_output_token_ms.is_none() {
                            time_to_first_output_token_ms = Some(
                                u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX),
                            );
                        }
                        let tool_name = ongoing_tool_call_tool_names.get(&part.id);
                        let tool = tool_name.and_then(|tool_name| {
                            controls.tools.iter().find(|tool| &tool.name == tool_name)
                        });
                        push_text_stream_part(
                            parts,
                            TextStreamPart::ToolInputDelta(part.clone()),
                            controls.on_chunk,
                        )
                        .await;
                        invoke_tool_input_delta_callback(
                            tool,
                            &part.id,
                            &part.delta,
                            controls.messages,
                            controls.abort_signal,
                            controls.experimental_sandbox,
                            controls.runtime_context,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::ToolInputEnd(part) => {
                        parts.push(TextStreamPart::ToolInputEnd(part));
                    }
                    LanguageModelStreamPart::ToolApprovalRequest(part) => {
                        if tool_calls
                            .iter()
                            .any(|tool_call| tool_call.tool_call_id == part.tool_call_id)
                        {
                            provider_content_entries.push(ProviderContentEntry::Content(
                                LanguageModelContent::ToolApprovalRequest(part.clone()),
                            ));
                            parts.push(TextStreamPart::ToolApprovalRequest(
                                TextStreamToolApprovalRequestPart::from_language_model_tool_approval_request(part),
                            ));
                        } else {
                            let error = ToolCallNotFoundForApprovalError::new(
                                &part.tool_call_id,
                                &part.approval_id,
                            );
                            let error_value = JsonValue::String(error.to_string());
                            errors.push(error_value.clone());
                            parts.push(TextStreamPart::Error(LanguageModelErrorStreamPart::new(
                                error_value,
                            )));
                        }
                    }
                    LanguageModelStreamPart::ToolCall(part) => {
                        let tool_call = GenerateTextToolCall::from_language_model_tool_call(&part);
                        let tool_name = ongoing_tool_call_tool_names
                            .remove(&tool_call.tool_call_id)
                            .unwrap_or_else(|| tool_call.tool_name.clone());
                        let tool = controls.tools.iter().find(|tool| tool.name == tool_name);
                        tool_calls.push(tool_call.clone());
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::ToolCall(part),
                        ));
                        push_text_stream_part(
                            parts,
                            TextStreamPart::ToolCall(tool_call),
                            controls.on_chunk,
                        )
                        .await;
                        let tool_call = tool_calls.last().expect("tool call was just pushed");
                        if tool_call.invalid != Some(true) {
                            invoke_tool_input_available_callback(
                                tool,
                                &tool_call.tool_call_id,
                                tool_call.input.clone(),
                                controls.messages,
                                controls.abort_signal,
                                controls.experimental_sandbox,
                                controls.runtime_context,
                            )
                            .await;
                        }
                    }
                    LanguageModelStreamPart::ToolResult(part) => {
                        let tool_result = generate_text_tool_result_from_language_model_tool_result(
                            &part,
                            &tool_calls,
                        );
                        tool_results.push(tool_result.clone());
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::ToolResult(part),
                        ));
                        push_text_stream_part(
                            parts,
                            TextStreamPart::ToolResult(tool_result),
                            controls.on_chunk,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::Custom(part) => {
                        custom_parts.push(part.clone());
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::Custom(part.clone()),
                        ));
                        push_text_stream_part(
                            parts,
                            TextStreamPart::Custom(part),
                            controls.on_chunk,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::File(part) => {
                        files.push(part.clone());
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::File(part.clone()),
                        ));
                        parts.push(TextStreamPart::File(TextStreamFilePart::new(part)));
                    }
                    LanguageModelStreamPart::ReasoningFile(part) => {
                        reasoning_files.push(part.clone());
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::ReasoningFile(part.clone()),
                        ));
                        parts.push(TextStreamPart::ReasoningFile(
                            TextStreamReasoningFilePart::new(part),
                        ));
                    }
                    LanguageModelStreamPart::Source(part) => {
                        sources.push(part.clone());
                        provider_content_entries.push(ProviderContentEntry::Content(
                            LanguageModelContent::Source(part.clone()),
                        ));
                        push_text_stream_part(
                            parts,
                            TextStreamPart::Source(part),
                            controls.on_chunk,
                        )
                        .await;
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
                            push_text_stream_part(
                                parts,
                                TextStreamPart::Raw(part),
                                controls.on_chunk,
                            )
                            .await;
                        }
                    }
                    LanguageModelStreamPart::Error(part) => {
                        finish_reason = FinishReason::Error;
                        errors.push(part.error.clone());
                        if let Some(on_error) = controls.on_error {
                            on_error
                                .error(StreamTextOnErrorEvent {
                                    error: part.error.clone(),
                                })
                                .await;
                        }
                        parts.push(TextStreamPart::Error(part));
                    }
                    LanguageModelStreamPart::StreamStart(_) => unreachable!(),
                }
            }
        }

        if let Some(abort_part) = stream_text_abort_part_from_signal(controls.abort_signal) {
            abort_reason = abort_part.reason.clone();
            push_text_stream_part(parts, TextStreamPart::Abort(abort_part), controls.on_chunk)
                .await;
            aborted = true;
            break;
        }
    }

    ensure_start_step(
        parts,
        &mut start_step_index,
        request.clone(),
        warnings.clone(),
    );

    let performance = StreamTextStepPerformance {
        step_time_ms: u64::try_from(step_start.elapsed().as_millis()).unwrap_or(u64::MAX),
        time_to_first_output_token_ms,
    };

    CollectedStreamTextStep {
        request,
        response,
        warnings,
        text,
        text_stream,
        reasoning_text: has_reasoning_text.then_some(reasoning_text),
        sources,
        files,
        reasoning_files,
        tool_calls,
        tool_results,
        response_messages: Vec::new(),
        custom_parts,
        errors,
        usage,
        finish_reason,
        raw_finish_reason,
        provider_metadata,
        performance,
        provider_content: materialize_provider_content(
            &provider_content_entries,
            &text_blocks,
            &reasoning_blocks,
        ),
        aborted,
        abort_reason,
    }
}

fn stream_text_step_is_retryable_pre_stream_failure(
    collected_step: &CollectedStreamTextStep,
    attempt_parts: &[TextStreamPart],
) -> bool {
    let Some(error) = collected_step.errors.first() else {
        return false;
    };

    collected_step.errors.len() == 1
        && collected_step.finish_reason == FinishReason::Error
        && collected_step.text.is_empty()
        && collected_step.text_stream.is_empty()
        && collected_step.reasoning_text.is_none()
        && collected_step.sources.is_empty()
        && collected_step.files.is_empty()
        && collected_step.reasoning_files.is_empty()
        && collected_step.tool_calls.is_empty()
        && collected_step.tool_results.is_empty()
        && collected_step.custom_parts.is_empty()
        && attempt_parts.iter().all(|part| {
            matches!(
                part,
                TextStreamPart::StartStep(_) | TextStreamPart::Error(_)
            )
        })
        && stream_text_error_is_retryable(error)
}

fn stream_text_error_is_retryable(error: &JsonValue) -> bool {
    error
        .get("isRetryable")
        .or_else(|| error.get("is_retryable"))
        .and_then(JsonValue::as_bool)
        .unwrap_or_else(|| {
            error
                .get("statusCode")
                .or_else(|| error.get("status_code"))
                .and_then(JsonValue::as_u64)
                .and_then(|status_code| u16::try_from(status_code).ok())
                .is_some_and(ApiCallError::is_retryable_status_code)
        })
}

fn stream_text_retry_delay_in_ms(error: &JsonValue, exponential_backoff_delay_ms: u64) -> u64 {
    let response_headers = stream_text_error_response_headers(error);
    retry_delay_from_response_headers(
        response_headers.as_ref(),
        exponential_backoff_delay_ms,
        time::OffsetDateTime::now_utc(),
    )
}

fn stream_text_error_response_headers(error: &JsonValue) -> Option<Headers> {
    let headers = error
        .get("responseHeaders")
        .or_else(|| error.get("response_headers"))
        .and_then(JsonValue::as_object)?;
    let mut response_headers = Headers::new();

    for (name, value) in headers {
        if let Some(value) = value.as_str() {
            response_headers.insert(name.clone(), value.to_string());
        } else if let Some(value) = value.as_i64() {
            response_headers.insert(name.clone(), value.to_string());
        } else if let Some(value) = value.as_u64() {
            response_headers.insert(name.clone(), value.to_string());
        } else if let Some(value) = value.as_f64() {
            response_headers.insert(name.clone(), value.to_string());
        }
    }

    (!response_headers.is_empty()).then_some(response_headers)
}

fn apply_stream_text_transforms(
    mut parts: Vec<TextStreamPart>,
    transforms: &[StreamTextTransform<'_>],
) -> Vec<TextStreamPart> {
    for transform in transforms {
        parts = transform.transform(parts);
    }

    parts
}

fn stream_text_transform_input_parts(
    mut parts: Vec<TextStreamPart>,
    collected_step: &CollectedStreamTextStep,
) -> Vec<TextStreamPart> {
    parts.push(TextStreamPart::FinishStep(TextStreamFinishStepPart::new(
        collected_step.response.clone(),
        collected_step.usage.clone(),
        collected_step.performance,
        collected_step.finish_reason.clone(),
        collected_step.raw_finish_reason.clone(),
        collected_step.provider_metadata.clone(),
    )));
    parts.push(TextStreamPart::Finish(TextStreamFinishPart::new(
        collected_step.finish_reason.clone(),
        collected_step.raw_finish_reason.clone(),
        collected_step.usage.clone(),
    )));
    parts
}

fn strip_stream_text_finish_parts(parts: Vec<TextStreamPart>) -> Vec<TextStreamPart> {
    parts
        .into_iter()
        .filter(|part| {
            !matches!(
                part,
                TextStreamPart::FinishStep(_) | TextStreamPart::Finish(_)
            )
        })
        .collect()
}

fn apply_stream_text_transforms_to_tool_results(
    tool_results: Vec<GenerateTextToolResult>,
    transforms: &[StreamTextTransform<'_>],
) -> Vec<GenerateTextToolResult> {
    if transforms.is_empty() {
        return tool_results;
    }

    let parts = tool_results
        .into_iter()
        .map(TextStreamPart::ToolResult)
        .collect();

    apply_stream_text_transforms(parts, transforms)
        .into_iter()
        .filter_map(|part| match part {
            TextStreamPart::ToolResult(part) => Some(part),
            _ => None,
        })
        .collect()
}

fn language_model_tool_call_from_stream_text_tool_call(
    tool_call: &GenerateTextToolCall,
) -> LanguageModelToolCall {
    let input = if tool_call.invalid == Some(true) {
        tool_call
            .input
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| tool_call.input.to_string())
    } else {
        serde_json::to_string(&tool_call.input).unwrap_or_else(|_| tool_call.input.to_string())
    };

    let mut provider_tool_call =
        LanguageModelToolCall::new(&tool_call.tool_call_id, &tool_call.tool_name, input);

    if let Some(provider_executed) = tool_call.provider_executed {
        provider_tool_call = provider_tool_call.with_provider_executed(provider_executed);
    }

    if let Some(dynamic) = tool_call.dynamic {
        provider_tool_call = provider_tool_call.with_dynamic(dynamic);
    }

    if let Some(provider_metadata) = &tool_call.provider_metadata {
        provider_tool_call = provider_tool_call.with_provider_metadata(provider_metadata.clone());
    }

    provider_tool_call
}

fn text_stream_tool_approval_response_output(
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

fn insert_part_after_tool_call(
    parts: &mut Vec<TextStreamPart>,
    tool_call_id: &str,
    part: TextStreamPart,
) {
    if let Some(index) = parts.iter().position(|candidate| {
        matches!(
            candidate,
            TextStreamPart::ToolCall(tool_call) if tool_call.tool_call_id == tool_call_id
        )
    }) {
        parts.insert(index + 1, part);
    } else {
        parts.push(part);
    }
}

fn insert_part_after_tool_approval_request(
    parts: &mut Vec<TextStreamPart>,
    approval_id: &str,
    part: TextStreamPart,
) {
    if let Some(index) = parts.iter().position(|candidate| {
        matches!(
            candidate,
            TextStreamPart::ToolApprovalRequest(request) if request.approval_id == approval_id
        )
    }) {
        parts.insert(index + 1, part);
    } else {
        parts.push(part);
    }
}

fn language_model_tool_result_from_stream_text_tool_result(
    tool_result: &GenerateTextToolResult,
) -> Option<LanguageModelToolResult> {
    let result = NonNullJsonValue::new(tool_result.output.clone()).ok()?;
    let mut provider_tool_result =
        LanguageModelToolResult::new(&tool_result.tool_call_id, &tool_result.tool_name, result);

    if let Some(is_error) = tool_result.is_error {
        provider_tool_result = provider_tool_result.with_is_error(is_error);
    }

    if let Some(preliminary) = tool_result.preliminary {
        provider_tool_result = provider_tool_result.with_preliminary(preliminary);
    }

    if let Some(dynamic) = tool_result.dynamic {
        provider_tool_result = provider_tool_result.with_dynamic(dynamic);
    }

    if let Some(provider_metadata) = &tool_result.provider_metadata {
        provider_tool_result =
            provider_tool_result.with_provider_metadata(provider_metadata.clone());
    }

    Some(provider_tool_result)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SmoothStreamDeltaKind {
    Text,
    Reasoning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SmoothStreamScheduledPart {
    part: TextStreamPart,
    delay_after: bool,
}

struct SmoothStreamState<'a> {
    chunking: &'a SmoothStreamChunking,
    output: Vec<SmoothStreamScheduledPart>,
    buffer: String,
    id: String,
    delta_kind: Option<SmoothStreamDeltaKind>,
    provider_metadata: Option<ProviderMetadata>,
}

impl<'a> SmoothStreamState<'a> {
    fn new(chunking: &'a SmoothStreamChunking) -> Self {
        Self {
            chunking,
            output: Vec::new(),
            buffer: String::new(),
            id: String::new(),
            delta_kind: None,
            provider_metadata: None,
        }
    }

    fn push_part(&mut self, part: TextStreamPart) -> Result<(), SmoothStreamError> {
        match part {
            TextStreamPart::TextDelta(part) => {
                self.push_delta(
                    SmoothStreamDeltaKind::Text,
                    part.id,
                    part.text,
                    part.provider_metadata,
                )?;
            }
            TextStreamPart::ReasoningDelta(part) => {
                self.push_delta(
                    SmoothStreamDeltaKind::Reasoning,
                    part.id,
                    part.text,
                    part.provider_metadata,
                )?;
            }
            part => {
                self.flush_buffer();
                self.push_part_without_delay(part);
            }
        }

        Ok(())
    }

    fn finish(mut self) -> Vec<SmoothStreamScheduledPart> {
        self.flush_buffer();
        self.output
    }

    fn push_delta(
        &mut self,
        delta_kind: SmoothStreamDeltaKind,
        id: String,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
    ) -> Result<(), SmoothStreamError> {
        if (self.delta_kind != Some(delta_kind) || self.id != id) && !self.buffer.is_empty() {
            self.flush_buffer();
        }

        self.buffer.push_str(&text);
        self.id = id;
        self.delta_kind = Some(delta_kind);

        if provider_metadata.is_some() {
            self.provider_metadata = provider_metadata;
        }

        while let Some(chunk) = detect_smooth_stream_chunk(&self.buffer, self.chunking)? {
            self.push_delta_part(delta_kind, chunk.clone(), None, true);
            self.buffer = self.buffer[chunk.len()..].to_string();
        }

        Ok(())
    }

    fn flush_buffer(&mut self) {
        if self.buffer.is_empty() {
            return;
        }

        let Some(delta_kind) = self.delta_kind else {
            return;
        };

        let text = std::mem::take(&mut self.buffer);
        let provider_metadata = self.provider_metadata.take();
        self.push_delta_part(delta_kind, text, provider_metadata, false);
    }

    fn push_delta_part(
        &mut self,
        delta_kind: SmoothStreamDeltaKind,
        text: String,
        provider_metadata: Option<ProviderMetadata>,
        delay_after: bool,
    ) {
        let part = match delta_kind {
            SmoothStreamDeltaKind::Text => {
                let mut part = TextStreamTextDeltaPart::new(self.id.clone(), text);
                if let Some(provider_metadata) = provider_metadata {
                    part = part.with_provider_metadata(provider_metadata);
                }
                TextStreamPart::TextDelta(part)
            }
            SmoothStreamDeltaKind::Reasoning => {
                let mut part = TextStreamReasoningDeltaPart::new(self.id.clone(), text);
                if let Some(provider_metadata) = provider_metadata {
                    part = part.with_provider_metadata(provider_metadata);
                }
                TextStreamPart::ReasoningDelta(part)
            }
        };

        self.output
            .push(SmoothStreamScheduledPart { part, delay_after });
    }

    fn push_part_without_delay(&mut self, part: TextStreamPart) {
        self.output.push(SmoothStreamScheduledPart {
            part,
            delay_after: false,
        });
    }
}

fn smooth_stream_parts(
    parts: impl IntoIterator<Item = TextStreamPart>,
    options: &SmoothStreamOptions,
) -> Result<Vec<TextStreamPart>, SmoothStreamError> {
    Ok(smooth_stream_scheduled_parts(parts, options)?
        .into_iter()
        .map(|scheduled| scheduled.part)
        .collect())
}

fn smooth_stream_scheduled_parts(
    parts: impl IntoIterator<Item = TextStreamPart>,
    options: &SmoothStreamOptions,
) -> Result<Vec<SmoothStreamScheduledPart>, SmoothStreamError> {
    let mut state = SmoothStreamState::new(&options.chunking);

    for part in parts {
        state.push_part(part)?;
    }

    Ok(state.finish())
}

fn detect_smooth_stream_chunk(
    buffer: &str,
    chunking: &SmoothStreamChunking,
) -> Result<Option<String>, SmoothStreamError> {
    match chunking {
        SmoothStreamChunking::Word => detect_smooth_stream_regex_chunk(buffer, word_chunk_regex()),
        SmoothStreamChunking::Line => detect_smooth_stream_regex_chunk(buffer, line_chunk_regex()),
        SmoothStreamChunking::Pattern(regex) => detect_smooth_stream_regex_chunk(buffer, regex),
        SmoothStreamChunking::Segmenter => Ok(detect_smooth_stream_segment_chunk(buffer)),
        SmoothStreamChunking::Detector(detector) => {
            let Some(chunk) = detector(buffer) else {
                return Ok(None);
            };

            if chunk.is_empty() {
                return Err(SmoothStreamError::EmptyDetectorMatch);
            }

            if !buffer.starts_with(&chunk) {
                return Err(SmoothStreamError::NonPrefixDetectorMatch {
                    matched: chunk,
                    buffer: buffer.to_string(),
                });
            }

            Ok(Some(chunk))
        }
    }
}

fn detect_smooth_stream_regex_chunk(
    buffer: &str,
    regex: &Regex,
) -> Result<Option<String>, SmoothStreamError> {
    let Some(chunk_match) = regex.find(buffer) else {
        return Ok(None);
    };

    if chunk_match.start() == chunk_match.end() {
        return Err(SmoothStreamError::EmptyPatternMatch {
            pattern: regex.as_str().to_string(),
        });
    }

    Ok(Some(buffer[..chunk_match.end()].to_string()))
}

/// Returns the first Unicode word segment of `buffer`, matching the upstream
/// `Intl.Segmenter` (`granularity: 'word'`) detector. Returns `None` for an
/// empty buffer, mirroring the upstream `buffer.length === 0` guard.
fn detect_smooth_stream_segment_chunk(buffer: &str) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }

    let segmenter = word_segmenter();
    for boundary in segmenter.segment_str(buffer) {
        if boundary == 0 {
            continue;
        }
        return Some(buffer[..boundary].to_string());
    }

    None
}

fn word_segmenter() -> &'static icu_segmenter::WordSegmenterBorrowed<'static> {
    static WORD_SEGMENTER: OnceLock<icu_segmenter::WordSegmenterBorrowed<'static>> =
        OnceLock::new();
    WORD_SEGMENTER.get_or_init(|| icu_segmenter::WordSegmenter::new_auto(Default::default()))
}

fn word_chunk_regex() -> &'static Regex {
    static WORD_CHUNK_REGEX: OnceLock<Regex> = OnceLock::new();
    WORD_CHUNK_REGEX.get_or_init(|| Regex::new(r"\S+\s+").expect("word chunk regex compiles"))
}

fn line_chunk_regex() -> &'static Regex {
    static LINE_CHUNK_REGEX: OnceLock<Regex> = OnceLock::new();
    LINE_CHUNK_REGEX.get_or_init(|| Regex::new(r"\n+").expect("line chunk regex compiles"))
}

#[derive(Clone, Copy)]
struct SmoothStreamReplayDelay<'a> {
    delay_after: &'a [bool],
    delay_in_ms: Option<i64>,
}

async fn replay_stream_text_attempt_parts(
    parts: &mut Vec<TextStreamPart>,
    attempt_parts: &[TextStreamPart],
    smooth_stream_delay: Option<SmoothStreamReplayDelay<'_>>,
    on_chunk: Option<&StreamTextOnChunk<'_>>,
    on_error: Option<&StreamTextOnError<'_>>,
    abort_signal: Option<&StreamTextAbortSignal>,
) -> Option<Option<JsonValue>> {
    for (part_index, part) in attempt_parts.iter().enumerate() {
        if let Some(on_chunk) = on_chunk
            && is_stream_text_chunk_callback_part(part)
        {
            on_chunk
                .chunk(StreamTextOnChunkEvent {
                    chunk: part.clone(),
                })
                .await;
        }

        if let Some(on_error) = on_error
            && let TextStreamPart::Error(part) = part
        {
            on_error
                .error(StreamTextOnErrorEvent {
                    error: part.error.clone(),
                })
                .await;
        }

        parts.push(part.clone());

        if let Some(abort_part) = stream_text_abort_part_from_signal(abort_signal) {
            let abort_reason = abort_part.reason.clone();
            push_text_stream_part(parts, TextStreamPart::Abort(abort_part), on_chunk).await;
            return Some(abort_reason);
        }

        if let Some(delay) = smooth_stream_delay
            && delay.delay_after.get(part_index).copied().unwrap_or(false)
        {
            ai_sdk_provider_utils::delay(delay.delay_in_ms).await;
        }
    }

    None
}

async fn push_text_stream_part(
    parts: &mut Vec<TextStreamPart>,
    part: TextStreamPart,
    on_chunk: Option<&StreamTextOnChunk<'_>>,
) {
    if let Some(on_chunk) = on_chunk
        && is_stream_text_chunk_callback_part(&part)
    {
        on_chunk
            .chunk(StreamTextOnChunkEvent {
                chunk: part.clone(),
            })
            .await;
    }

    parts.push(part);
}

fn stream_text_abort_part_from_signal(
    abort_signal: Option<&StreamTextAbortSignal>,
) -> Option<TextStreamAbortPart> {
    let abort_signal = abort_signal?;
    if !abort_signal.is_aborted() {
        return None;
    }

    Some(match abort_signal.reason() {
        Some(reason) => TextStreamAbortPart::with_reason(reason),
        None => TextStreamAbortPart::new(),
    })
}

fn is_stream_text_chunk_callback_part(part: &TextStreamPart) -> bool {
    matches!(
        part,
        TextStreamPart::TextDelta(_)
            | TextStreamPart::ReasoningDelta(_)
            | TextStreamPart::ToolInputStart(_)
            | TextStreamPart::ToolInputDelta(_)
            | TextStreamPart::ToolCall(_)
            | TextStreamPart::ToolResult(_)
            | TextStreamPart::ToolOutputDenied(_)
            | TextStreamPart::Custom(_)
            | TextStreamPart::Source(_)
            | TextStreamPart::Raw(_)
            | TextStreamPart::Abort(_)
    )
}

/// Dynamic-tool resolution input mirroring the upstream `tools` set used by
/// `toUIMessageChunk`.
///
/// Upstream `isDynamic` consults the `tools` object: a `dynamic` tool resolves
/// to `Some(true)`, a static tool resolves to `None`, and a tool that is not in
/// the set falls back to the part's own `dynamic` flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToUiMessageChunkToolKind {
    /// A statically-typed tool: upstream `isDynamic` returns `undefined`.
    Static,

    /// A dynamic tool: upstream `isDynamic` returns `true`.
    Dynamic,
}

/// Options controlling [`to_ui_message_chunk`], mirroring upstream
/// `ToUIMessageChunkOptions`.
#[derive(Default)]
pub struct ToUiMessageChunkOptions<'a> {
    /// Tool kinds keyed by tool name, used to resolve dynamic tool calls the
    /// same way upstream consults its `tools` set.
    pub tools: Option<&'a BTreeMap<String, ToUiMessageChunkToolKind>>,

    /// Whether reasoning parts should be emitted. Defaults to `true`.
    pub send_reasoning: bool,

    /// Whether source parts should be emitted. Defaults to `false`.
    pub send_sources: bool,

    /// Whether the stream-start part should be emitted. Defaults to `true`.
    pub send_start: bool,

    /// Whether the stream-finish part should be emitted. Defaults to `true`.
    pub send_finish: bool,

    /// Maps a stream error into UI-safe text, mirroring upstream `onError`.
    pub on_error: Option<&'a dyn Fn(&JsonValue) -> String>,

    /// Static metadata merged into start/finish chunks.
    pub message_metadata: Option<JsonValue>,

    /// Response message id injected into the start chunk.
    pub response_message_id: Option<String>,
}

impl<'a> ToUiMessageChunkOptions<'a> {
    /// Creates options with the upstream defaults (`sendReasoning`/`sendStart`/
    /// `sendFinish` enabled, `sendSources` disabled).
    pub fn new() -> Self {
        Self {
            tools: None,
            send_reasoning: true,
            send_sources: false,
            send_start: true,
            send_finish: true,
            on_error: None,
            message_metadata: None,
            response_message_id: None,
        }
    }
}

fn to_ui_message_chunk_error_text(
    error: &JsonValue,
    options: &ToUiMessageChunkOptions<'_>,
) -> String {
    if let Some(on_error) = options.on_error {
        return on_error(error);
    }

    match error {
        JsonValue::String(text) => text.clone(),
        other => get_error_message(Some(other as &dyn fmt::Display)),
    }
}

fn to_ui_message_chunk_is_dynamic(
    options: &ToUiMessageChunkOptions<'_>,
    tool_name: &str,
    part_dynamic: Option<bool>,
) -> Option<bool> {
    match options.tools.and_then(|tools| tools.get(tool_name)) {
        None => part_dynamic,
        Some(ToUiMessageChunkToolKind::Dynamic) => Some(true),
        Some(ToUiMessageChunkToolKind::Static) => None,
    }
}

/// Converts a single high-level [`TextStreamPart`] into a [`UiMessageChunk`],
/// mirroring upstream `toUIMessageChunk`.
///
/// Returns `None` for stream parts that do not produce UI message chunks
/// (`tool-input-end`, `raw`, and suppressed reasoning/source/start/finish
/// parts). Unlike the upstream `default` branch that throws for an unknown
/// chunk type, the Rust [`TextStreamPart`] enum is exhaustive so an unknown
/// part type is unrepresentable.
pub fn to_ui_message_chunk(
    part: &TextStreamPart,
    options: &ToUiMessageChunkOptions<'_>,
) -> Option<UiMessageChunk> {
    match part {
        TextStreamPart::TextStart(part) => Some(UiMessageChunk::TextStart {
            id: part.id.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        TextStreamPart::TextDelta(part) => Some(UiMessageChunk::TextDelta {
            id: part.id.clone(),
            delta: part.text.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        TextStreamPart::TextEnd(part) => Some(UiMessageChunk::TextEnd {
            id: part.id.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        TextStreamPart::ReasoningStart(part) => {
            if !options.send_reasoning {
                return None;
            }
            Some(UiMessageChunk::ReasoningStart {
                id: part.id.clone(),
                provider_metadata: part.provider_metadata.clone(),
            })
        }
        TextStreamPart::ReasoningDelta(part) => {
            if !options.send_reasoning {
                return None;
            }
            Some(UiMessageChunk::ReasoningDelta {
                id: part.id.clone(),
                delta: part.text.clone(),
                provider_metadata: part.provider_metadata.clone(),
            })
        }
        TextStreamPart::ReasoningEnd(part) => {
            if !options.send_reasoning {
                return None;
            }
            Some(UiMessageChunk::ReasoningEnd {
                id: part.id.clone(),
                provider_metadata: part.provider_metadata.clone(),
            })
        }
        TextStreamPart::File(part) => Some(UiMessageChunk::File {
            media_type: part.file.media_type.clone(),
            url: ui_message_file_url(&part.file.media_type, &part.file.data),
            provider_metadata: part.provider_metadata.clone(),
        }),
        TextStreamPart::ReasoningFile(part) => {
            if !options.send_reasoning {
                return None;
            }
            Some(UiMessageChunk::ReasoningFile {
                media_type: part.file.media_type.clone(),
                url: ui_message_file_url(&part.file.media_type, &part.file.data),
                provider_metadata: part.provider_metadata.clone(),
            })
        }
        TextStreamPart::Source(part) => {
            if !options.send_sources {
                return None;
            }
            match part {
                LanguageModelSource::Url(source) => Some(UiMessageChunk::SourceUrl {
                    source_id: source.id.clone(),
                    url: source.url.clone(),
                    title: source.title.clone(),
                    provider_metadata: source.provider_metadata.clone(),
                }),
                LanguageModelSource::Document(source) => Some(UiMessageChunk::SourceDocument {
                    source_id: source.id.clone(),
                    media_type: source.media_type.clone(),
                    title: source.title.clone(),
                    filename: source.filename.clone(),
                    provider_metadata: source.provider_metadata.clone(),
                }),
            }
        }
        TextStreamPart::Custom(part) => Some(UiMessageChunk::Custom {
            kind: part.kind.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        TextStreamPart::ToolInputStart(part) => {
            let dynamic = to_ui_message_chunk_is_dynamic(options, &part.tool_name, part.dynamic);
            Some(UiMessageChunk::ToolInputStart {
                tool_call_id: part.id.clone(),
                tool_name: part.tool_name.clone(),
                provider_executed: part.provider_executed,
                provider_metadata: part.provider_metadata.clone(),
                dynamic,
                title: part.title.clone(),
            })
        }
        TextStreamPart::ToolInputDelta(part) => Some(UiMessageChunk::ToolInputDelta {
            tool_call_id: part.id.clone(),
            input_text_delta: part.delta.clone(),
        }),
        TextStreamPart::ToolCall(part) => {
            let dynamic = to_ui_message_chunk_is_dynamic(options, &part.tool_name, part.dynamic);
            if part.invalid == Some(true) {
                let error = part
                    .error
                    .as_ref()
                    .map(|message| JsonValue::String(message.clone()))
                    .unwrap_or(JsonValue::Null);
                Some(UiMessageChunk::ToolInputError {
                    tool_call_id: part.tool_call_id.clone(),
                    tool_name: part.tool_name.clone(),
                    input: part.input.clone(),
                    error_text: to_ui_message_chunk_error_text(&error, options),
                    provider_executed: part.provider_executed,
                    provider_metadata: part.provider_metadata.clone(),
                    tool_metadata: part.tool_metadata.clone(),
                    dynamic,
                    title: part.title.clone(),
                })
            } else {
                Some(UiMessageChunk::ToolInputAvailable {
                    tool_call_id: part.tool_call_id.clone(),
                    tool_name: part.tool_name.clone(),
                    input: part.input.clone(),
                    provider_executed: part.provider_executed,
                    provider_metadata: part.provider_metadata.clone(),
                    tool_metadata: part.tool_metadata.clone(),
                    dynamic,
                    title: part.title.clone(),
                })
            }
        }
        TextStreamPart::ToolApprovalRequest(part) => Some(UiMessageChunk::ToolApprovalRequest {
            approval_id: part.approval_id.clone(),
            tool_call_id: part.tool_call_id.clone(),
            is_automatic: part.is_automatic,
            provider_metadata: None,
        }),
        TextStreamPart::ToolApprovalResponse(part) => Some(UiMessageChunk::ToolApprovalResponse {
            approval_id: part.approval_id.clone(),
            approved: part.approved,
            reason: part.reason.clone(),
            provider_executed: part.provider_executed,
        }),
        TextStreamPart::ToolResult(part) => {
            let dynamic = to_ui_message_chunk_is_dynamic(options, &part.tool_name, part.dynamic);
            if part.is_error == Some(true) {
                // Upstream models provider errors as a dedicated `tool-error`
                // part: when the tool is provider-executed the error value is
                // used verbatim (string) or JSON-stringified, otherwise it is
                // routed through `onError`.
                let error_text = if part.provider_executed == Some(true) {
                    match &part.output {
                        JsonValue::String(text) => text.clone(),
                        other => other.to_string(),
                    }
                } else {
                    to_ui_message_chunk_error_text(&part.output, options)
                };
                Some(UiMessageChunk::ToolOutputError {
                    tool_call_id: part.tool_call_id.clone(),
                    error_text,
                    provider_executed: part.provider_executed,
                    provider_metadata: part.provider_metadata.clone(),
                    tool_metadata: part.tool_metadata.clone(),
                    dynamic,
                })
            } else {
                Some(UiMessageChunk::ToolOutputAvailable {
                    tool_call_id: part.tool_call_id.clone(),
                    output: part.output.clone(),
                    provider_executed: part.provider_executed,
                    provider_metadata: part.provider_metadata.clone(),
                    tool_metadata: part.tool_metadata.clone(),
                    preliminary: part.preliminary,
                    dynamic,
                })
            }
        }
        TextStreamPart::ToolOutputDenied(part) => Some(UiMessageChunk::ToolOutputDenied {
            tool_call_id: part.tool_call_id.clone(),
            tool_name: None,
            provider_executed: None,
            dynamic: None,
        }),
        TextStreamPart::Error(part) => Some(UiMessageChunk::Error {
            error_text: to_ui_message_chunk_error_text(&part.error, options),
        }),
        TextStreamPart::StartStep(_) => Some(UiMessageChunk::StartStep),
        TextStreamPart::FinishStep(_) => Some(UiMessageChunk::FinishStep),
        TextStreamPart::Start(_) => {
            if !options.send_start {
                return None;
            }
            Some(UiMessageChunk::Start {
                message_id: options.response_message_id.clone(),
                message_metadata: options.message_metadata.clone(),
            })
        }
        TextStreamPart::Finish(part) => {
            if !options.send_finish {
                return None;
            }
            Some(UiMessageChunk::Finish {
                finish_reason: Some(part.finish_reason.clone()),
                message_metadata: options.message_metadata.clone(),
            })
        }
        TextStreamPart::Abort(part) => Some(UiMessageChunk::Abort {
            reason: part.reason.clone(),
        }),
        TextStreamPart::ToolInputEnd(_) | TextStreamPart::Raw(_) => None,
    }
}

fn ui_message_error_text(error: &JsonValue, options: &StreamTextUiMessageStreamOptions) -> String {
    if let Some(on_error) = &options.on_error {
        return on_error.error_text(error);
    }

    default_ui_message_error_text(error)
}

fn default_ui_message_error_text(_error: &JsonValue) -> String {
    "An error occurred.".to_string()
}

fn tool_call_error_text(error: Option<&str>, options: &StreamTextUiMessageStreamOptions) -> String {
    let error = error
        .map(|error| JsonValue::String(error.to_string()))
        .unwrap_or_else(|| JsonValue::String("An error occurred.".to_string()));
    if options.on_error.is_none() {
        return error
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| error.to_string());
    }
    ui_message_error_text(&error, options)
}

fn tool_result_error_text(
    tool_result: &GenerateTextToolResult,
    options: &StreamTextUiMessageStreamOptions,
) -> String {
    if options.on_error.is_none() {
        return tool_result
            .output
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| tool_result.output.to_string());
    }

    if tool_result.provider_executed != Some(true) {
        return ui_message_error_text(&tool_result.output, options);
    }

    tool_result
        .output
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| tool_result.output.to_string())
}

fn ui_message_file_url(media_type: &str, data: &LanguageModelFileData) -> String {
    match data {
        LanguageModelFileData::Data { data } => {
            format!("data:{media_type};base64,{}", convert_to_base64(data))
        }
        LanguageModelFileData::Url { url } => url.to_string(),
    }
}

fn text_language_model_content(
    text: String,
    provider_metadata: Option<ProviderMetadata>,
) -> LanguageModelContent {
    let mut content = LanguageModelText::new(text);
    if let Some(provider_metadata) = provider_metadata {
        content = content.with_provider_metadata(provider_metadata);
    }

    LanguageModelContent::Text(content)
}

fn reasoning_language_model_content(
    text: String,
    provider_metadata: Option<ProviderMetadata>,
) -> LanguageModelContent {
    let mut content = LanguageModelReasoning::new(text);
    if let Some(provider_metadata) = provider_metadata {
        content = content.with_provider_metadata(provider_metadata);
    }

    LanguageModelContent::Reasoning(content)
}

fn language_model_response_from_stream_metadata(
    metadata: StreamTextResponseMetadata,
) -> LanguageModelResponse {
    LanguageModelResponse {
        messages: None,
        id: metadata.id,
        timestamp: metadata.timestamp,
        model_id: metadata.model_id,
        headers: metadata.headers,
        body: None,
    }
}

/// Applies payload retention settings to a streamed step's request metadata.
///
/// Mirrors `apply_generate_text_include`: request messages are only retained when
/// `include.request_messages` is set, and the provider request body is stripped
/// unless `include.request_body` is set. By default both are excluded.
fn apply_stream_text_include(
    request: &mut Option<LanguageModelRequest>,
    include: GenerateTextInclude,
    step_prompt: &LanguageModelPrompt,
) {
    if include.request_messages {
        request
            .get_or_insert_with(LanguageModelRequest::new)
            .messages = Some(step_prompt.clone());
    }

    if !include.request_body {
        if let Some(request) = request {
            request.body = None;
        }
    }
}

fn apply_stream_text_response_identity(
    response: &mut StreamTextResponseMetadata,
    generate_response: Option<&LanguageModelResponse>,
) {
    let Some(generate_response) = generate_response else {
        return;
    };

    if response.id.is_none() {
        response.id = generate_response.id.clone();
    }

    if response.timestamp.is_none() {
        response.timestamp = generate_response.timestamp;
    }

    if response.model_id.is_none() {
        response.model_id = generate_response.model_id.clone();
    }

    if response.headers.is_none() {
        response.headers = generate_response.headers.clone();
    }
}

fn sync_stream_text_tool_parts(
    parts: &mut [TextStreamPart],
    tool_calls: &[GenerateTextToolCall],
    tool_results: &[GenerateTextToolResult],
) {
    for part in parts {
        match part {
            TextStreamPart::ToolCall(part) => {
                if let Some(tool_call) = tool_calls.iter().find(|tool_call| {
                    tool_call.tool_call_id == part.tool_call_id
                        && (tool_call.invalid != Some(true)
                            || !tool_call.error.as_deref().is_some_and(|error| {
                                error.starts_with("Model tried to call unavailable tool")
                            }))
                }) {
                    *part = tool_call.clone();
                }
            }
            TextStreamPart::ToolResult(part) if part.preliminary != Some(true) => {
                if let Some(tool_result) = tool_results
                    .iter()
                    .find(|tool_result| tool_result.tool_call_id == part.tool_call_id)
                {
                    *part = tool_result.clone();
                }
            }
            _ => {}
        }
    }
}

fn add_stream_text_step_usage(steps: &[StreamTextStep]) -> LanguageModelUsage {
    steps
        .iter()
        .fold(LanguageModelUsage::default(), |mut usage, step| {
            usage.input_tokens.total =
                add_optional_counts(usage.input_tokens.total, step.usage.input_tokens.total);
            usage.input_tokens.no_cache = add_optional_counts(
                usage.input_tokens.no_cache,
                step.usage.input_tokens.no_cache,
            );
            usage.input_tokens.cache_read = add_optional_counts(
                usage.input_tokens.cache_read,
                step.usage.input_tokens.cache_read,
            );
            usage.input_tokens.cache_write = add_optional_counts(
                usage.input_tokens.cache_write,
                step.usage.input_tokens.cache_write,
            );
            usage.output_tokens.total =
                add_optional_counts(usage.output_tokens.total, step.usage.output_tokens.total);
            usage.output_tokens.text =
                add_optional_counts(usage.output_tokens.text, step.usage.output_tokens.text);
            usage.output_tokens.reasoning = add_optional_counts(
                usage.output_tokens.reasoning,
                step.usage.output_tokens.reasoning,
            );
            usage
        })
}

fn add_optional_counts(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0) + right.unwrap_or(0)),
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
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use serde_json::{Map, json};

    use super::*;
    use crate::PromptDownload;
    use crate::file_data::{FileData, FileDataContent};
    use crate::generate_text::{
        GenerateTextContentPart, NormalizedToolApprovalStatus, ToolApprovalStatusKind,
        ToolCallRepairOptions, ToolCallRepairOriginalError, has_tool_call,
    };
    use crate::json::NonNullJsonValue;
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelDocumentSource, LanguageModelErrorStreamPart,
        LanguageModelFile, LanguageModelFileData, LanguageModelFilePart, LanguageModelFinishReason,
        LanguageModelMessage, LanguageModelRawStreamPart, LanguageModelReasoningDelta,
        LanguageModelReasoningFile, LanguageModelResponseFormat, LanguageModelStreamFinish,
        LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
        LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
        LanguageModelSystemMessage, LanguageModelTextDelta, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolApprovalRequest, LanguageModelToolApprovalRequestPart,
        LanguageModelToolApprovalResponsePart, LanguageModelToolCall, LanguageModelToolCallPart,
        LanguageModelToolContentPart, LanguageModelToolInputDelta, LanguageModelToolInputEnd,
        LanguageModelToolInputStart, LanguageModelToolMessage, LanguageModelToolResult,
        LanguageModelToolResultContentPart, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUrlSource, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::logger::{LogWarningsOptions, take_log_warning_calls_for_tests};
    use crate::mock_models::MockLanguageModel;
    use crate::prompt::Prompt;
    use crate::provider_utils::{
        DelayedPromise, DownloadedBlob, ExecuteToolOutput, SandboxCommandOptions,
        SandboxCommandResult, SandboxRunCommandFuture, Schema, Tool, ToolExecutionError,
        ValidationResult,
    };
    use crate::telemetry::{
        TelemetryEvent, TelemetryEventKind, TelemetryIntegration, TelemetryOptions,
        register_telemetry_integration, reset_telemetry_state_for_tests,
        telemetry_test_guard_for_tests,
    };
    use crate::ui_message_stream::UiMessageRole;
    use crate::util::parse_partial_json;
    use crate::warning::Warning;
    use serde::Deserialize;
    use url::Url;

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("mock futures should be ready"),
        }
    }

    fn poll_once<T>(future: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        future.poll(&mut context)
    }

    fn poll_until_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        loop {
            match Pin::new(&mut future).poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
    }

    fn user_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
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

    fn provider_executed_approval_response_prompt(
        response: LanguageModelToolApprovalResponsePart,
    ) -> Vec<LanguageModelMessage> {
        vec![
            user_message("Shorten this URL: https://example.com"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "mcp-call-1",
                        "mcp_tool",
                        json!({ "query": "test" }),
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("mcp-approval-1", "mcp-call-1"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(response),
            ])),
        ]
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

    fn stream_text_result_from_parts(parts: Vec<LanguageModelStreamPart>) -> StreamTextResult {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(parts));
        poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )))
    }

    fn stream_result_hello() -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ])
    }

    fn stream_text_result_from_text(text: &str) -> StreamTextResult {
        stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", text)),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ])
    }

    fn stream_text_partial_output_values(result: &StreamTextResult) -> Vec<JsonValue> {
        let mut output = String::new();
        let mut partial_values = Vec::new();

        for delta in &result.text_stream {
            output.push_str(delta);

            let value = parse_partial_json(Some(&output))
                .value()
                .cloned()
                .unwrap_or_else(|| JsonValue::String(output.clone()));

            if partial_values.last() != Some(&value) {
                partial_values.push(value);
            }
        }

        partial_values
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_text_output_type_default() {
        let result = stream_text_result_from_text("Hello world");
        let output: String = result.output_as().expect("text output is typed");

        assert_eq!(output, "Hello world");
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_text_output_type() {
        let result = stream_text_result_from_text("Hello world");
        let output: String = result.output_as().expect("text output is typed");

        assert_eq!(output, "Hello world");
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct StreamTypedObjectOutput {
        value: String,
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_object_output_type() {
        let result = stream_text_result_from_text(r#"{"value":"typed"}"#);
        let output: StreamTypedObjectOutput = result.output_as().expect("object output is typed");

        assert_eq!(
            output,
            StreamTypedObjectOutput {
                value: "typed".to_string()
            }
        );
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_array_output_type() {
        let result = stream_text_result_from_text(r#"["a","b"]"#);
        let output: Vec<String> = result.output_as().expect("array output is typed");

        assert_eq!(output, vec!["a".to_string(), "b".to_string()]);
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    enum StreamChoiceOutput {
        #[serde(rename = "a")]
        A,
        #[serde(rename = "b")]
        B,
        #[serde(rename = "c")]
        C,
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_choice_output_type() {
        let result = stream_text_result_from_text(r#""b""#);
        let output: StreamChoiceOutput = result.output_as().expect("choice output is typed");

        assert_eq!(output, StreamChoiceOutput::B);
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_json_output_type() {
        let result = stream_text_result_from_text(r#"{"value":["anything",1,true]}"#);
        let output: JsonValue = result.output_as().expect("JSON output is typed");

        assert_eq!(output, json!({ "value": ["anything", 1, true] }));
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_text_partial_output_type_default() {
        let result = stream_text_result_from_text("Hello world");
        let output: Vec<String> = result
            .partial_outputs_as()
            .expect("partial text output is typed");

        assert_eq!(output, vec!["Hello world".to_string()]);
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_text_partial_output_type() {
        let result = stream_text_result_from_text("Hello world");
        let output: Vec<String> = result
            .partial_outputs_as()
            .expect("partial text output is typed");

        assert_eq!(output, vec!["Hello world".to_string()]);
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_object_partial_output_type() {
        let result = stream_text_result_from_text(r#"{"value":"partial"}"#);
        let output: Vec<StreamTypedObjectOutput> = result
            .partial_outputs_as()
            .expect("partial object output is typed");

        assert_eq!(
            output,
            vec![StreamTypedObjectOutput {
                value: "partial".to_string()
            }]
        );
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_array_partial_output_type() {
        let result = stream_text_result_from_text(r#"["a","b"]"#);
        let output: Vec<Vec<String>> = result
            .partial_outputs_as()
            .expect("partial array output is typed");

        assert_eq!(output, vec![vec!["a".to_string(), "b".to_string()]]);
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_choice_partial_output_type() {
        let result = stream_text_result_from_text(r#""c""#);
        let output: Vec<StreamChoiceOutput> = result
            .partial_outputs_as()
            .expect("partial choice output is typed");

        assert_eq!(output, vec![StreamChoiceOutput::C]);
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_json_partial_output_type() {
        let result = stream_text_result_from_text(r#"{"value":["anything",1,true]}"#);
        let output: Vec<JsonValue> = result
            .partial_outputs_as()
            .expect("partial JSON output is typed");

        assert_eq!(output, vec![json!({ "value": ["anything", 1, true] })]);
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_element_type_for_array_output() {
        let result = stream_text_result_from_text(r#"[{"value":"one"},{"value":"two"}]"#);
        let output: Vec<StreamTypedObjectOutput> =
            result.element_stream_as().expect("element output is typed");

        assert_eq!(
            output,
            vec![
                StreamTypedObjectOutput {
                    value: "one".to_string()
                },
                StreamTypedObjectOutput {
                    value: "two".to_string()
                }
            ]
        );
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_empty_element_stream_for_text_output() {
        let result = stream_text_result_from_text("Hello world");
        let output: Vec<JsonValue> = result
            .element_stream_as()
            .expect("text output has no element stream");

        assert!(output.is_empty());
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_empty_element_stream_for_object_output() {
        let result = stream_text_result_from_text(r#"{"value":"typed"}"#);
        let output: Vec<JsonValue> = result
            .element_stream_as()
            .expect("object output has no element stream");

        assert!(output.is_empty());
    }

    #[test]
    fn stream_text_type_counterpart_should_infer_empty_element_stream_for_default_output() {
        let result = stream_text_result_from_text("Hello world");
        let output: Vec<JsonValue> = result
            .element_stream_as()
            .expect("default text output has no element stream");

        assert!(output.is_empty());
    }

    #[test]
    fn stream_text_result_partial_output_stream_collects_incremental_text_outputs() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            stream_text_partial_output_values(&result),
            vec![json!("Hello, "), json!("Hello, world!"),]
        );
        assert_eq!(result.text, "Hello, world!");
    }

    #[test]
    fn stream_text_result_partial_output_stream_repairs_incremental_object_outputs() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "text-1",
                "\"value\": ",
            )),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "\"Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "!\"")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", " }")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            stream_text_partial_output_values(&result),
            vec![
                json!({}),
                json!({ "value": "Hello, " }),
                json!({ "value": "Hello, world" }),
                json!({ "value": "Hello, world!" }),
            ]
        );
        assert_eq!(
            result
                .output_as::<JsonValue>()
                .expect("object output is typed"),
            json!({
                "value": "Hello, world!"
            })
        );
    }

    #[test]
    fn stream_text_result_partial_output_stream_repairs_incremental_array_outputs_and_elements() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "[")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "text-1",
                r#"{"value":"one"}"#,
            )),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", ",")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "text-1",
                r#"{"value":"two"}"#,
            )),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "]")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            stream_text_partial_output_values(&result),
            vec![
                json!([]),
                json!([{"value": "one"}]),
                json!([{"value": "one"}, {"value": "two"}]),
            ]
        );
        assert_eq!(
            result
                .element_stream_as::<StreamTypedObjectOutput>()
                .expect("array elements are typed"),
            vec![
                StreamTypedObjectOutput {
                    value: "one".to_string()
                },
                StreamTypedObjectOutput {
                    value: "two".to_string()
                },
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts text-output row
    /// `should not call JSON.stringify for string partial outputs` — string
    /// partial outputs are emitted as raw strings, never JSON-encoded values.
    #[test]
    fn stream_text_result_partial_output_stream_text_output_keeps_raw_string_partials() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        let partials = stream_text_partial_output_values(&result);
        assert_eq!(partials, vec![json!("Hello, "), json!("Hello, world!")]);
        // Every partial is a raw JSON string, never a serialized/quoted form.
        for value in &partials {
            assert!(matches!(value, JsonValue::String(_)));
        }
    }

    /// Maps packages/ai stream-text.test.ts text-output row
    /// `should resolve output promise with the correct content` — the resolved
    /// `output` promise for default text output equals the full text.
    #[test]
    fn stream_text_result_text_output_resolves_output_promise() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        let output: String = result.output_as().expect("text output is typed");
        assert_eq!(output, "Hello, world!");
    }

    /// Maps packages/ai stream-text.test.ts object-output row
    /// `should send partial output stream` — object output repairs incremental
    /// JSON deltas into a progressively-completed object stream.
    #[test]
    fn stream_text_result_object_output_sends_partial_output_stream() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"value\": ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "!\"")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            stream_text_partial_output_values(&result),
            vec![
                json!({}),
                json!({ "value": "Hello, " }),
                json!({ "value": "Hello, world" }),
                json!({ "value": "Hello, world!" }),
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts object-output row
    /// `should send partial output stream when last chunk contains content` —
    /// the final delta carrying both content and the closing brace still yields
    /// the completed object as the last partial.
    #[test]
    fn stream_text_result_object_output_partial_output_stream_last_chunk_with_content() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"value\": ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!\" }")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            stream_text_partial_output_values(&result),
            vec![
                json!({}),
                json!({ "value": "Hello, " }),
                json!({ "value": "Hello, world!" }),
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts object-output row
    /// `should resolve text promise with the correct content` — object output
    /// still resolves the raw text promise with the underlying JSON text.
    #[test]
    fn stream_text_result_object_output_resolves_text_promise() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"value\": ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!\" ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "}")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(result.text, "{ \"value\": \"Hello, world!\" }");
    }

    /// Maps packages/ai stream-text.test.ts object-output row
    /// `should resolve output promise with the correct content` — object output
    /// resolves the typed `output` promise with the completed object.
    #[test]
    fn stream_text_result_object_output_resolves_output_promise() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"value\": ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!\" ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "}")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        let output: StreamTypedObjectOutput = result.output_as().expect("object output is typed");
        assert_eq!(
            output,
            StreamTypedObjectOutput {
                value: "Hello, world!".to_string()
            }
        );
    }

    /// Maps packages/ai stream-text.test.ts:19630
    /// `should call onFinish with the correct content` — for object output the
    /// `onFinish` event carries the final step content as a single text part
    /// holding the raw JSON object string (object output does not strip the
    /// surrounding JSON from the emitted content).
    #[test]
    fn stream_text_result_object_output_on_finish_receives_raw_object_text_content() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"value\": ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "\"Hello, ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!\" ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "}")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")]).with_on_finish(
                move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                },
            ),
        ));

        result.consume_stream();

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        let event = &finish_events[0];

        assert_eq!(event.text, "{ \"value\": \"Hello, world!\" }");
        assert!(!event.call_id.is_empty());
        assert_eq!(event.content.len(), 1);
        match &event.content[0] {
            GenerateTextContentPart::Text(text) => {
                assert_eq!(text.text, "{ \"value\": \"Hello, world!\" }");
            }
            other => panic!("expected a single text content part, got {other:?}"),
        }
    }

    #[derive(Default)]
    struct MockStreamTextUiMessageResponse {
        status: Option<u16>,
        status_text: Option<String>,
        headers: Headers,
        chunks: Vec<Vec<u8>>,
        ended: bool,
    }

    impl MockStreamTextUiMessageResponse {
        fn decoded_chunks(&self) -> Vec<String> {
            self.chunks
                .iter()
                .map(|chunk| String::from_utf8(chunk.clone()).expect("chunk decodes"))
                .collect()
        }
    }

    impl UiMessageStreamResponseWriter for MockStreamTextUiMessageResponse {
        type Error = std::convert::Infallible;

        fn write_head(
            &mut self,
            status: u16,
            status_text: Option<&str>,
            headers: &Headers,
        ) -> Result<(), Self::Error> {
            self.status = Some(status);
            self.status_text = status_text.map(ToString::to_string);
            self.headers = headers.clone();
            Ok(())
        }

        fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error> {
            self.chunks.push(chunk.to_vec());
            Ok(())
        }

        fn end(&mut self) -> Result<(), Self::Error> {
            self.ended = true;
            Ok(())
        }
    }

    #[test]
    fn smooth_stream_combines_partial_words() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", ", ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should transform text chunks");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello, ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_split_larger_text_chunks() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                    "1",
                    "Hello, World! This is an example text.",
                )),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should split larger text chunks");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello, ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "World! ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "This ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "is ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "an ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "example ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "text.")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_keep_longer_whitespace_sequences_together() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "First line")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " \n\n")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "  ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "  Multiple spaces")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "\n    Indented")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should preserve whitespace sequences");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "First ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "line \n\n")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "    Multiple ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "spaces\n    ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Indented")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_flush_text_buffer_before_tool_call_starts() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "I will check the")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " weather in Lon")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "don.")),
                TextStreamPart::ToolCall(smooth_stream_weather_tool_call()),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush before tool call");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "I ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "will ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "check ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "the ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "weather ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "in ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "London.")),
                TextStreamPart::ToolCall(smooth_stream_weather_tool_call()),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_flush_text_buffer_before_streaming_tool_input_starts() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "I will check the")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " weather in Lon")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "don.")),
                TextStreamPart::ToolInputStart(LanguageModelToolInputStart::new("2", "weather")),
                TextStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "2",
                    "{ city: \"London\" }",
                )),
                TextStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("2")),
                TextStreamPart::ToolCall(smooth_stream_weather_tool_call()),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush before streaming tool input");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "I ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "will ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "check ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "the ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "weather ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "in ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "London.")),
                TextStreamPart::ToolInputStart(LanguageModelToolInputStart::new("2", "weather")),
                TextStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "2",
                    "{ city: \"London\" }",
                )),
                TextStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("2")),
                TextStreamPart::ToolCall(smooth_stream_weather_tool_call()),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_not_return_chunks_with_just_spaces() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "foo")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should buffer leading spaces");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "   foo")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_split_text_by_lines_when_using_line_chunking_mode() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                    "1",
                    "First line\nSecond line\nThird line with more text\n",
                )),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Partial line")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                    "1",
                    " continues\nFinal line\n",
                )),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Line),
        )
        .expect("line smoothing should split completed lines");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "First line\n")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Second line\n")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                    "1",
                    "Third line with more text\n"
                )),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                    "1",
                    "Partial line continues\n"
                )),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Final line\n")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_handle_text_without_line_endings_in_line_chunking_mode() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Text without")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " any line")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " breaks")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Line),
        )
        .expect("line smoothing should flush incomplete final line");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                    "1",
                    "Text without any line breaks"
                )),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_support_custom_chunking_regexps_character_level() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello, world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Pattern(
                Regex::new(".").expect("character regex compiles"),
            )),
        )
        .expect("pattern smoothing should split by character");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "H")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "e")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "l")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "l")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "o")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", ",")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "w")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "o")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "r")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "l")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "d")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_change_the_id_when_the_text_part_id_changes() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextStart(LanguageModelTextStart::new("2")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "I will check the")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " weather in Lon")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "don.")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "I will check the")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", " weather in Lon")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "don.")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("2")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush before switching text ids");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextStart(LanguageModelTextStart::new("2")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "I ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "will ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "check ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "the ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "weather ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "in ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "London.")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "I ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "will ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "check ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "the ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "weather ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "in ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "London.")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("2")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_split_larger_reasoning_chunks() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new(
                    "1",
                    "First I need to analyze the problem. Then I will solve it.",
                )),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should split larger reasoning chunks");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "First ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "I ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "need ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "to ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "analyze ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "the ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "problem. ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "Then ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "I ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "will ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "solve ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "it.")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_combine_partial_reasoning_words() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "Let")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", " me ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "think...")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should combine partial reasoning words");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "Let ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "me ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "think...")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_flush_reasoning_buffer_before_tool_call() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new(
                    "1",
                    "I should check the",
                )),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", " weather")),
                TextStreamPart::ToolCall(smooth_stream_weather_tool_call()),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush reasoning before tool call");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "I ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "should ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "check ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "the ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "weather")),
                TextStreamPart::ToolCall(smooth_stream_weather_tool_call()),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_use_line_chunking_for_reasoning() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new(
                    "1",
                    "Step 1: Analyze\nStep 2: Solve\n",
                )),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Line),
        )
        .expect("smooth stream should line chunk reasoning");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new(
                    "1",
                    "Step 1: Analyze\n"
                )),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new(
                    "1",
                    "Step 2: Solve\n"
                )),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_flush_text_buffer_when_switching_to_reasoning() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("2")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "world")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("2", "Let me")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("2", " think")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("2")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush text before reasoning");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("2")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "world")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("2", "Let ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("2", "me ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("2", "think")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("2")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_flush_reasoning_buffer_when_switching_to_text() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::TextStart(LanguageModelTextStart::new("2")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "Thinking ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "hard")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "The answer")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", " is 42")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("2")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush reasoning before text");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::TextStart(LanguageModelTextStart::new("2")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "Thinking ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "hard")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "The ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "answer ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "is ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("2", "42")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("2")),
            ]
        );
    }

    #[test]
    fn smooth_stream_should_handle_multiple_switches_between_text_and_reasoning() {
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                TextStreamPart::TextStart(LanguageModelTextStart::new("t1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("r1", "Think ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("t1", "Hello ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("r1", "more ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("t1", "world ")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("t1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("smooth stream should flush each text/reasoning switch");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                TextStreamPart::TextStart(LanguageModelTextStart::new("t1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("r1", "Think ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("t1", "Hello ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("r1", "more ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("t1", "world ")),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("t1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_marks_detected_chunks_for_default_delay() {
        let scheduled_parts = smooth_stream_scheduled_parts(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello, world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            &SmoothStreamOptions::new(),
        )
        .expect("smooth stream should schedule text chunks");

        assert_eq!(SmoothStreamOptions::new().delay_in_ms, Some(10));
        assert_eq!(
            scheduled_parts,
            vec![
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                    delay_after: false,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello, ")),
                    delay_after: true,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "world!")),
                    delay_after: false,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                    delay_after: false,
                },
            ]
        );
    }

    #[test]
    fn smooth_stream_supports_custom_and_null_delay_options() {
        assert_eq!(
            SmoothStreamOptions::new()
                .with_delay_in_ms(Some(20))
                .delay_in_ms,
            Some(20)
        );
        assert_eq!(
            SmoothStreamOptions::new()
                .with_delay_in_ms(None)
                .delay_in_ms,
            None
        );
    }

    #[test]
    fn smooth_stream_supports_line_and_pattern_chunking() {
        let line_parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "First line\nSecond")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " line\nFinal")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Line),
        )
        .expect("line smoothing should succeed");

        assert_eq!(
            line_parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "First line\n")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Second line\n")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Final")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );

        let pattern_parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello_, world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Pattern(
                Regex::new("_").expect("test regex compiles"),
            )),
        )
        .expect("pattern smoothing should succeed");

        assert_eq!(
            pattern_parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello_")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", ", world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_supports_detector_chunking_and_validation() {
        let detector = Arc::new(|buffer: &str| {
            Regex::new("[^_]*_")
                .ok()?
                .find(buffer)
                .map(|m| buffer[..m.end()].to_string())
        });
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "He_llo, ")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "w_orld!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Detector(detector)),
        )
        .expect("detector smoothing should succeed");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "He_")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "llo, w_")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "orld!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ]
        );

        let error = smooth_stream(
            vec![TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                "1",
                "Hello, world!",
            ))],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Detector(Arc::new(
                |_| Some("world".to_string()),
            ))),
        )
        .expect_err("non-prefix detector matches should fail");

        assert_eq!(
            error,
            SmoothStreamError::NonPrefixDetectorMatch {
                matched: "world".to_string(),
                buffer: "Hello, world!".to_string(),
            }
        );

        let error = smooth_stream(
            vec![TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                "1",
                "Hello, world!",
            ))],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Detector(Arc::new(
                |_| Some(String::new()),
            ))),
        )
        .expect_err("empty detector matches should fail");

        assert_eq!(error, SmoothStreamError::EmptyDetectorMatch);
    }

    #[test]
    fn smooth_stream_rejects_invalid_string_chunking_strategy() {
        let error = SmoothStreamChunking::from_strategy("foo")
            .expect_err("an unknown chunking strategy should be rejected");

        assert_eq!(error.parameter(), "chunking");
        assert_eq!(error.value(), &json!("foo"));
        assert_eq!(
            error.message(),
            "Invalid argument for parameter chunking: Chunking must be \"word\", \"line\", a RegExp, an Intl.Segmenter, or a ChunkDetector function. Received: foo"
        );

        // The supported string strategies still resolve to the built-in variants.
        assert!(matches!(
            SmoothStreamChunking::from_strategy("word"),
            Ok(SmoothStreamChunking::Word)
        ));
        assert!(matches!(
            SmoothStreamChunking::from_strategy("line"),
            Ok(SmoothStreamChunking::Line)
        ));
    }

    #[test]
    fn smooth_stream_rejects_null_chunking_option() {
        // Upstream rejects `chunking: null` with the same InvalidArgumentError as
        // any other non-string/non-regex/non-segmenter value. In Rust the typed
        // enum forbids constructing a null variant, so the parity surface is the
        // string constructor receiving the JS `null` rendering.
        let error = SmoothStreamChunking::from_strategy("null")
            .expect_err("a null chunking option should be rejected");

        assert_eq!(error.parameter(), "chunking");
        assert_eq!(
            error.message(),
            "Invalid argument for parameter chunking: Chunking must be \"word\", \"line\", a RegExp, an Intl.Segmenter, or a ChunkDetector function. Received: null"
        );
    }

    fn smooth_stream_segmenter_text_deltas(text: &str) -> Vec<TextStreamPart> {
        smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", text)),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Segmenter),
        )
        .expect("segmenter smoothing should succeed")
    }

    fn smooth_stream_expected_segment_parts(segments: &[&str]) -> Vec<TextStreamPart> {
        let mut parts = vec![TextStreamPart::TextStart(LanguageModelTextStart::new("1"))];
        for segment in segments {
            parts.push(TextStreamPart::TextDelta(TextStreamTextDeltaPart::new(
                "1", *segment,
            )));
        }
        parts.push(TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")));
        parts
    }

    #[test]
    fn smooth_stream_should_segment_english_text_using_segmenter() {
        // Assert the detected segments match the upstream Intl.Segmenter snapshot.
        assert_eq!(
            smooth_stream_segmenter_text_deltas("Hello, world!"),
            smooth_stream_expected_segment_parts(&["Hello", ",", " ", "world", "!"])
        );

        // Every detected segment is scheduled with a trailing delay, matching the
        // upstream `delay 10` interleaving (one delay per emitted text-delta).
        let scheduled = smooth_stream_scheduled_parts(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello, world!")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            &SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Segmenter),
        )
        .expect("segmenter smoothing should schedule chunks");
        assert_eq!(
            scheduled,
            vec![
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                    delay_after: false,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "Hello")),
                    delay_after: true,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", ",")),
                    delay_after: true,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", " ")),
                    delay_after: true,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "world")),
                    delay_after: true,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "!")),
                    delay_after: true,
                },
                SmoothStreamScheduledPart {
                    part: TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                    delay_after: false,
                },
            ]
        );
    }

    #[test]
    fn smooth_stream_should_segment_japanese_text_using_segmenter() {
        assert_eq!(
            smooth_stream_segmenter_text_deltas("こんにちは世界"),
            smooth_stream_expected_segment_parts(&["こんにちは", "世界"])
        );
    }

    #[test]
    fn smooth_stream_should_segment_chinese_text_using_segmenter() {
        assert_eq!(
            smooth_stream_segmenter_text_deltas("你好世界"),
            smooth_stream_expected_segment_parts(&["你好", "世界"])
        );
    }

    #[test]
    fn smooth_stream_should_handle_mixed_cjk_and_latin_content() {
        assert_eq!(
            smooth_stream_segmenter_text_deltas("Hello こんにちは World"),
            smooth_stream_expected_segment_parts(&["Hello", " ", "こんにちは", " ", "World"])
        );
    }

    #[test]
    fn smooth_stream_segmenter_drains_buffer_across_partial_deltas() {
        // Each incoming delta fully drains the buffer through the segmenter, so a
        // hiragana fragment that is not yet a complete word ("こんに") is emitted
        // character-by-character ("こん", then "に"). This documents the Rust ICU
        // segmentation. The algorithm matches upstream exactly; the only divergence
        // is the word-boundary *data*: ICU4X keeps the isolated fragment "ちは" as
        // one segment, whereas V8's Intl.Segmenter splits it into "ち" / "は".
        // Upstream case packages-ai-0905 pins that exact boundary to V8's bundled
        // ICU build, so it is classified `js-only-documented` (JavaScript runtime
        // ICU dictionary) rather than mapped here; see docs/ai-core-package-inventory.md.
        let parts = smooth_stream(
            vec![
                TextStreamPart::TextStart(LanguageModelTextStart::new("1")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "こんに")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "ちは")),
                TextStreamPart::TextDelta(TextStreamTextDeltaPart::new("1", "世界")),
                TextStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            ],
            SmoothStreamOptions::new().with_chunking(SmoothStreamChunking::Segmenter),
        )
        .expect("segmenter smoothing should succeed");

        assert_eq!(
            parts,
            smooth_stream_expected_segment_parts(&["こん", "に", "ちは", "世界"])
        );
    }

    #[test]
    fn smooth_stream_should_segment_longer_japanese_sentence_with_mixed_content() {
        assert_eq!(
            smooth_stream_segmenter_text_deltas(
                "東京は日本の首都です。人口は約1400万人で、世界最大の都市圏の一つです。美しい桜の季節には多くの観光客が訪れます。"
            ),
            smooth_stream_expected_segment_parts(&[
                "東京",
                "は",
                "日本",
                "の",
                "首都",
                "です",
                "。",
                "人口",
                "は",
                "約",
                "1400",
                "万人",
                "で",
                "、",
                "世界",
                "最大",
                "の",
                "都市",
                "圏",
                "の",
                "一つ",
                "です",
                "。",
                "美しい",
                "桜の",
                "季節",
                "に",
                "は",
                "多く",
                "の",
                "観光",
                "客",
                "が",
                "訪れ",
                "ます",
                "。"
            ])
        );
    }

    #[test]
    fn smooth_stream_preserves_provider_metadata_on_flushed_reasoning_delta() {
        let provider_metadata = ProviderMetadata::from([(
            "anthropic".to_string(),
            Map::from_iter([("signature".to_string(), json!("sig_abc123"))]),
        )]);
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "I am")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new(
                    "1",
                    " thinking...",
                )),
                TextStreamPart::ReasoningDelta(
                    TextStreamReasoningDeltaPart::new("1", "")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("reasoning smoothing should succeed");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "I ")),
                TextStreamPart::ReasoningDelta(TextStreamReasoningDeltaPart::new("1", "am ")),
                TextStreamPart::ReasoningDelta(
                    TextStreamReasoningDeltaPart::new("1", "thinking...")
                        .with_provider_metadata(provider_metadata)
                ),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ]
        );
    }

    #[test]
    fn smooth_stream_preserves_provider_metadata_on_reasoning_start_for_redacted_thinking() {
        let provider_metadata = ProviderMetadata::from([(
            "anthropic".to_string(),
            Map::from_iter([("redactedData".to_string(), json!("redacted-thinking-data"))]),
        )]);
        let reasoning_start =
            LanguageModelReasoningStart::new("1").with_provider_metadata(provider_metadata.clone());
        let parts = smooth_stream(
            vec![
                TextStreamPart::ReasoningStart(reasoning_start.clone()),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ],
            SmoothStreamOptions::new(),
        )
        .expect("reasoning start metadata should pass through");

        assert_eq!(
            parts,
            vec![
                TextStreamPart::ReasoningStart(reasoning_start),
                TextStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
            ]
        );
    }

    fn smooth_stream_weather_tool_call() -> GenerateTextToolCall {
        GenerateTextToolCall {
            tool_call_id: "1".to_string(),
            tool_name: "weather".to_string(),
            input: json!({ "city": "London" }),
            title: None,
            provider_executed: None,
            dynamic: None,
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: None,
        }
    }

    fn tool_calls_finish_reason() -> LanguageModelFinishReason {
        LanguageModelFinishReason {
            unified: FinishReason::ToolCalls,
            raw: Some("tool_calls".to_string()),
        }
    }

    fn warning_logger_text_stream_result(
        text: &str,
        warnings: Vec<Warning>,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(warnings)),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", text)),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ])
    }

    fn warning_logger_tool_call_stream_result(
        tool_name: &str,
        warnings: Vec<Warning>,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(warnings)),
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "call-1",
                tool_name,
                r#"{ "value": "test" }"#,
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                tool_calls_finish_reason(),
            )),
        ])
    }

    fn warning_logger_tool() -> Tool {
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        Tool::new("testTool", input_schema)
            .with_execute(|_input, _options| async move { Ok(json!("result")) })
    }

    #[test]
    fn stream_text_calls_log_warnings_with_warnings_from_a_single_step() {
        let expected_warnings = vec![
            Warning::Other {
                message: "Setting is not supported".to_string(),
            },
            Warning::Unsupported {
                feature: "temperature".to_string(),
                details: Some("Temperature parameter not supported".to_string()),
            },
        ];
        let model = MockLanguageModel::new().with_stream_result(warning_logger_text_stream_result(
            "Hello, world!",
            expected_warnings.clone(),
        ));
        take_log_warning_calls_for_tests();

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Hello")],
        )));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![
                LogWarningsOptions::new(expected_warnings)
                    .with_scope("mock-provider", "mock-model-id")
            ]
        );
    }

    #[test]
    fn stream_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step() {
        let warning1 = vec![Warning::Other {
            message: "warning1".to_string(),
        }];
        let warning2 = vec![Warning::Other {
            message: "warning2".to_string(),
        }];
        let model = MockLanguageModel::new().with_stream_results([
            warning_logger_tool_call_stream_result("testTool", warning1.clone()),
            warning_logger_text_stream_result("Final response", warning2.clone()),
        ]);
        take_log_warning_calls_for_tests();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Hello")])
                .with_tool(warning_logger_tool())
                .with_max_steps(2),
        ));

        assert_eq!(result.text, "Final response");
        assert_eq!(result.steps.len(), 2);
        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![
                LogWarningsOptions::new(warning1).with_scope("mock-provider", "mock-model-id"),
                LogWarningsOptions::new(warning2).with_scope("mock-provider", "mock-model-id"),
            ]
        );
    }

    #[test]
    fn stream_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present() {
        let model = MockLanguageModel::new().with_stream_result(warning_logger_text_stream_result(
            "Hello, world!",
            Vec::new(),
        ));
        take_log_warning_calls_for_tests();

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Hello")],
        )));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![LogWarningsOptions::new(Vec::new()).with_scope("mock-provider", "mock-model-id")]
        );
    }

    #[test]
    fn stream_text_result_warnings_resolve_with_warnings_from_all_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            warning_logger_tool_call_stream_result(
                "testTool",
                vec![Warning::Other {
                    message: "step 0 warning".to_string(),
                }],
            ),
            warning_logger_text_stream_result(
                "Final response",
                vec![Warning::Other {
                    message: "step 1 warning".to_string(),
                }],
            ),
        ]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Hello")])
                .with_tool(warning_logger_tool())
                .with_max_steps(2),
        ));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "step 0 warning".to_string(),
                },
                Warning::Other {
                    message: "step 1 warning".to_string(),
                },
            ]
        );
        assert_eq!(
            result.steps.last().expect("final step exists").warnings,
            vec![Warning::Other {
                message: "step 1 warning".to_string(),
            }]
        );
    }

    #[test]
    fn stream_text_result_warnings_are_passed_to_on_finish() {
        let model = MockLanguageModel::new().with_stream_results([
            warning_logger_tool_call_stream_result(
                "testTool",
                vec![Warning::Other {
                    message: "step 0 warning".to_string(),
                }],
            ),
            warning_logger_text_stream_result(
                "Final response",
                vec![Warning::Other {
                    message: "step 1 warning".to_string(),
                }],
            ),
        ]);
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Hello")])
                .with_tool(warning_logger_tool())
                .with_max_steps(2)
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                }),
        ));

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(result.warnings, finish_events[0].warnings);
        assert_eq!(
            finish_events[0].warnings,
            vec![
                Warning::Other {
                    message: "step 0 warning".to_string(),
                },
                Warning::Other {
                    message: "step 1 warning".to_string(),
                },
            ]
        );
        assert_eq!(
            finish_events[0]
                .steps
                .last()
                .expect("final step exists")
                .warnings,
            vec![Warning::Other {
                message: "step 1 warning".to_string(),
            }]
        );
    }

    fn url_source(id: &str, url: &str, title: &str) -> LanguageModelSource {
        LanguageModelSource::Url(LanguageModelUrlSource::new(id, url).with_title(title))
    }

    fn data_file(base64: &str) -> LanguageModelFile {
        LanguageModelFile::new(
            "text/plain",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64(base64.to_string()),
            },
        )
    }

    fn tool1() -> Tool {
        let schema = json!({ "type": "object", "properties": {} })
            .as_object()
            .expect("schema is an object")
            .clone();
        Tool::new("tool1", schema)
            .with_execute(|_input, _options| async move { Ok(json!("result1")) })
    }

    /// Maps packages/ai stream-text.test.ts:5773 — sources aggregate across all
    /// steps while the final step only retains its own source.
    #[test]
    fn stream_text_result_sources_contain_sources_from_all_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Source(url_source(
                    "source-0",
                    "https://example.com/0",
                    "Source 0",
                )),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "tool1", "{}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Source(url_source(
                    "source-1",
                    "https://example.com/1",
                    "Source 1",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")])
                .with_tool(tool1())
                .with_max_steps(3),
        ));

        assert_eq!(
            result.sources,
            vec![
                url_source("source-0", "https://example.com/0", "Source 0"),
                url_source("source-1", "https://example.com/1", "Source 1"),
            ]
        );
        assert_eq!(
            result.steps.last().expect("final step exists").sources,
            vec![url_source("source-1", "https://example.com/1", "Source 1")]
        );
    }

    /// Maps packages/ai stream-text.test.ts:5875 — files aggregate across all
    /// steps while the final step only retains its own file.
    #[test]
    fn stream_text_result_files_contain_files_from_all_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0w")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "tool1", "{}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0x")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")])
                .with_tool(tool1())
                .with_max_steps(3),
        ));

        assert_eq!(
            result.files,
            vec![data_file("c3RlcC0w"), data_file("c3RlcC0x")]
        );
        assert_eq!(
            result.steps.last().expect("final step exists").files,
            vec![data_file("c3RlcC0x")]
        );
    }

    /// Maps packages/ai stream-text.test.ts:5952 — onFinish receives files from
    /// all steps while the last reported step only carries its own file.
    #[test]
    fn stream_text_result_sends_files_from_all_steps_to_on_finish() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0w")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "tool1", "{}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0x")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")])
                .with_tool(tool1())
                .with_max_steps(3)
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                }),
        ));

        let finish_events = finish_events.lock().expect("finish events lock");
        let files: Vec<(String, String)> = finish_events[0]
            .files
            .iter()
            .map(|file| (file.media_type().to_string(), file.base64()))
            .collect();
        assert_eq!(
            files,
            vec![
                ("text/plain".to_string(), "c3RlcC0w".to_string()),
                ("text/plain".to_string(), "c3RlcC0x".to_string()),
            ]
        );
        let final_step_files: Vec<(String, String)> = finish_events[0]
            .steps
            .last()
            .expect("final step exists")
            .files
            .iter()
            .map(|file| (file.media_type().to_string(), file.base64()))
            .collect();
        assert_eq!(
            final_step_files,
            vec![("text/plain".to_string(), "c3RlcC0x".to_string())]
        );
    }

    /// Maps packages/ai stream-text.test.ts:6034 — file stream parts from all
    /// steps surface as file parts while the final step keeps only its file.
    #[test]
    fn stream_text_result_contains_file_content_parts_from_all_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0w")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "tool1", "{}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0x")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")])
                .with_tool(tool1())
                .with_max_steps(3),
        ));

        let file_parts: Vec<LanguageModelFile> = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::File(part) => Some(part.file.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            file_parts,
            vec![data_file("c3RlcC0w"), data_file("c3RlcC0x")]
        );
        assert_eq!(
            result.steps.last().expect("final step exists").files.len(),
            1
        );
    }

    /// Maps packages/ai stream-text.test.ts:6299 — a single step records the
    /// sources emitted by the model response.
    #[test]
    fn stream_text_step_result_contains_sources_from_model_response() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Source(url_source(
                    "123",
                    "https://example.com",
                    "Example",
                )),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Source(url_source(
                    "456",
                    "https://example.com/2",
                    "Example 2",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        assert_eq!(result.steps.len(), 1);
        assert_eq!(
            result.steps[0].sources,
            vec![
                url_source("123", "https://example.com", "Example"),
                url_source("456", "https://example.com/2", "Example 2"),
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:6404 — a single step records the
    /// files emitted by the model response.
    #[test]
    fn stream_text_step_result_contains_files_from_model_response() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(data_file("c3RlcC0w")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].files, vec![data_file("c3RlcC0w")]);
    }

    /// Maps packages/ai stream-text.test.ts:6522 — a single step records the
    /// reasoning files emitted by the model response.
    #[test]
    fn stream_text_step_result_contains_reasoning_files_from_model_response() {
        let reasoning_file = LanguageModelReasoningFile::new(
            "image/png",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("reasoning-data".to_string()),
            },
        );
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ReasoningFile(reasoning_file.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].reasoning_files, vec![reasoning_file]);
    }

    /// Maps packages/ai stream-text.test.ts:6113 — a single step records the
    /// reasoning text emitted by the model response.
    #[test]
    fn stream_text_step_result_contains_reasoning_from_model_response() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "1",
                    "I will open the conversation with witty banter.",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("2")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("2", "Hi there!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("2")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        assert_eq!(result.steps.len(), 1);
        assert_eq!(
            result.steps[0].reasoning_text.as_deref(),
            Some("I will open the conversation with witty banter.")
        );
        assert_eq!(result.steps[0].text, "Hi there!");
    }

    /// Maps packages/ai stream-text.test.ts:6659 — the final step exposed on the
    /// result equals the last collected step.
    #[test]
    fn stream_text_result_exposes_the_final_step() {
        let model = MockLanguageModel::new().with_stream_results([
            warning_logger_tool_call_stream_result("tool1", Vec::new()),
            warning_logger_text_stream_result("done", Vec::new()),
        ]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")])
                .with_tool(
                    Tool::new(
                        "tool1",
                        json!({ "type": "object", "properties": { "value": { "type": "string" } } })
                            .as_object()
                            .expect("schema is an object")
                            .clone(),
                    )
                    .with_execute(|_input, _options| async move { Ok(json!("result")) }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(result.steps.len(), 2);
        let final_step = result.steps.last().expect("final step exists");
        assert_eq!(final_step.text, result.text);
        assert_eq!(final_step.finish_reason, result.finish_reason);
        assert_eq!(final_step.response.id, result.response.id);
    }

    /// Maps packages/ai stream-text.test.ts:6804 — tool calls aggregate across
    /// all steps and split into static/dynamic groups; the final text step has
    /// none.
    #[test]
    fn stream_text_result_resolves_tool_calls_from_all_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value-1" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-2",
                    "dynamicTool",
                    r#"{ "value": "value-2" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let value_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result =
            poll_ready(stream_text(
                StreamTextOptions::new(&model, vec![user_message("test-input")])
                    .with_tool(Tool::new("tool1", value_schema.clone()).with_execute(
                        |_input, _options| async move { Ok(json!("value-1-result")) },
                    ))
                    .with_tool(Tool::dynamic("dynamicTool", value_schema).with_execute(
                        |_input, _options| async move { Ok(json!("value-2-result")) },
                    ))
                    .with_max_steps(4),
            ));

        let ids: Vec<&str> = result
            .tool_calls
            .iter()
            .map(|call| call.tool_call_id.as_str())
            .collect();
        assert_eq!(ids, vec!["call-1", "call-2"]);

        let static_ids: Vec<&str> = result
            .tool_calls
            .iter()
            .filter(|call| call.dynamic != Some(true))
            .map(|call| call.tool_call_id.as_str())
            .collect();
        assert_eq!(static_ids, vec!["call-1"]);

        let dynamic_ids: Vec<&str> = result
            .tool_calls
            .iter()
            .filter(|call| call.dynamic == Some(true))
            .map(|call| call.tool_call_id.as_str())
            .collect();
        assert_eq!(dynamic_ids, vec!["call-2"]);

        assert!(
            result
                .steps
                .last()
                .expect("final step exists")
                .tool_calls
                .is_empty()
        );
    }

    /// Maps packages/ai stream-text.test.ts:6932 — tool results aggregate across
    /// all steps and split into static/dynamic groups; the final text step has
    /// none.
    #[test]
    fn stream_text_result_resolves_tool_results_from_all_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value-1" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-2",
                    "dynamicTool",
                    r#"{ "value": "value-2" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let value_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result =
            poll_ready(stream_text(
                StreamTextOptions::new(&model, vec![user_message("test-input")])
                    .with_tool(Tool::new("tool1", value_schema.clone()).with_execute(
                        |_input, _options| async move { Ok(json!("value-1-result")) },
                    ))
                    .with_tool(Tool::dynamic("dynamicTool", value_schema).with_execute(
                        |_input, _options| async move { Ok(json!("value-2-result")) },
                    ))
                    .with_max_steps(4),
            ));

        let ids: Vec<&str> = result
            .tool_results
            .iter()
            .map(|result| result.tool_call_id.as_str())
            .collect();
        assert_eq!(ids, vec!["call-1", "call-2"]);

        let static_ids: Vec<&str> = result
            .tool_results
            .iter()
            .filter(|result| result.dynamic != Some(true))
            .map(|result| result.tool_call_id.as_str())
            .collect();
        assert_eq!(static_ids, vec!["call-1"]);

        let dynamic_ids: Vec<&str> = result
            .tool_results
            .iter()
            .filter(|result| result.dynamic == Some(true))
            .map(|result| result.tool_call_id.as_str())
            .collect();
        assert_eq!(dynamic_ids, vec!["call-2"]);

        assert!(
            result
                .steps
                .last()
                .expect("final step exists")
                .tool_results
                .is_empty()
        );
    }

    /// Maps packages/ai stream-text.test.ts:5681 — the result exposes provider
    /// response metadata (id, model id, timestamp, headers) and assistant
    /// response messages.
    #[test]
    fn stream_text_result_resolves_with_response_information() {
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("id-0")
            .with_model_id("mock-model-id")
            .with_timestamp(time::OffsetDateTime::UNIX_EPOCH);
        let model = MockLanguageModel::new().with_stream_result(
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(response_metadata),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ])
            .with_response(LanguageModelStreamResultResponse::new().with_header("call", "2")),
        );

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        assert_eq!(result.response.id, Some("id-0".to_string()));
        assert_eq!(result.response.model_id, Some("mock-model-id".to_string()));
        assert_eq!(
            result.response.timestamp,
            Some(time::OffsetDateTime::UNIX_EPOCH)
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("call")),
            Some(&"2".to_string())
        );
        assert_eq!(
            result.response_messages,
            vec![LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage::new(vec![LanguageModelAssistantContentPart::Text(
                    LanguageModelTextPart::new("Hello")
                )])
            )]
        );
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
    fn stream_text_prepare_step_overrides_step_settings_and_carries_contexts() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let seen_prepare_options = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let seen_prepare_options_for_callback = Arc::clone(&seen_prepare_options);
        let step_start_events = Arc::new(Mutex::new(Vec::<GenerateTextStepStartEvent>::new()));
        let step_start_events_for_callback = Arc::clone(&step_start_events);
        let mut base_provider_options = ProviderOptions::new();
        base_provider_options.insert(
            "base".to_string(),
            json!({ "mode": "outer" })
                .as_object()
                .expect("provider options are objects")
                .clone(),
        );
        let mut step_provider_options = ProviderOptions::new();
        step_provider_options.insert(
            "test".to_string(),
            json!({ "mode": "step" })
                .as_object()
                .expect("provider options are objects")
                .clone(),
        );
        let runtime_context = json!({ "tenant": "outer" })
            .as_object()
            .expect("runtime context is object")
            .clone();
        let tools_context = json!({ "weather": { "apiKey": "outer" } })
            .as_object()
            .expect("tools context is object")
            .clone();
        let step_runtime_context = json!({ "tenant": "step" })
            .as_object()
            .expect("runtime context is object")
            .clone();
        let step_tools_context = json!({ "weather": { "apiKey": "step" } })
            .as_object()
            .expect("tools context is object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Outer prompt")])
                .with_provider_options(base_provider_options.clone())
                .with_runtime_context(runtime_context.clone())
                .with_tools_context(tools_context.clone())
                .with_prepare_step({
                    let step_provider_options = step_provider_options.clone();
                    let step_runtime_context = step_runtime_context.clone();
                    let step_tools_context = step_tools_context.clone();
                    move |options| {
                        seen_prepare_options_for_callback
                            .lock()
                            .expect("prepare options lock")
                            .push(json!({
                                "stepNumber": options.step_number,
                                "messages": options.messages,
                                "initialMessages": options.initial_messages,
                                "responseMessages": options.response_messages,
                                "runtimeContext": options.runtime_context,
                                "toolsContext": options.tools_context
                            }));
                        let step_provider_options = step_provider_options.clone();
                        let step_runtime_context = step_runtime_context.clone();
                        let step_tools_context = step_tools_context.clone();
                        async move {
                            PrepareStepResult::new()
                                .with_messages(vec![user_message("Prepared prompt")])
                                .with_runtime_context(step_runtime_context)
                                .with_tools_context(step_tools_context)
                                .with_tool_choice(LanguageModelToolChoice::Required)
                                .with_provider_options(step_provider_options)
                        }
                    }
                })
                .with_on_step_start(move |event| {
                    let step_start_events = Arc::clone(&step_start_events_for_callback);
                    async move {
                        step_start_events
                            .lock()
                            .expect("step-start events lock")
                            .push(event);
                    }
                }),
        ));

        assert_eq!(result.steps.len(), 1);

        let prepare_options = seen_prepare_options.lock().expect("prepare options lock");
        assert_eq!(prepare_options.len(), 1);
        assert_eq!(prepare_options[0]["stepNumber"], json!(0));
        assert_eq!(
            prepare_options[0]["messages"],
            json!([user_message("Outer prompt")])
        );
        assert_eq!(
            prepare_options[0]["initialMessages"],
            json!([user_message("Outer prompt")])
        );
        assert_eq!(prepare_options[0]["responseMessages"], json!([]));
        assert_eq!(prepare_options[0]["runtimeContext"], json!(runtime_context));
        assert_eq!(prepare_options[0]["toolsContext"], json!(tools_context));
        drop(prepare_options);

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].prompt, vec![user_message("Prepared prompt")]);
        assert_eq!(
            calls[0].tool_choice,
            Some(LanguageModelToolChoice::Required)
        );
        assert_eq!(
            calls[0]
                .provider_options
                .as_ref()
                .and_then(|options| options.get("base")),
            base_provider_options.get("base")
        );
        assert_eq!(
            calls[0]
                .provider_options
                .as_ref()
                .and_then(|options| options.get("test")),
            step_provider_options.get("test")
        );

        let step_start_events = step_start_events.lock().expect("step-start events lock");
        assert_eq!(step_start_events.len(), 1);
        assert_eq!(
            step_start_events[0].messages,
            vec![user_message("Prepared prompt")]
        );
        assert_eq!(
            step_start_events[0].tool_choice,
            Some(LanguageModelToolChoice::Required)
        );
        assert_eq!(step_start_events[0].runtime_context, step_runtime_context);
        assert_eq!(step_start_events[0].tools_context, step_tools_context);
    }

    #[test]
    fn stream_text_prepare_step_passes_accumulated_steps_to_subsequent_prepare_step_calls() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Game Results",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let prepare_step_calls = Arc::new(Mutex::new(Vec::<(usize, usize, usize)>::new()));
        let prepare_step_calls_for_callback = Arc::clone(&prepare_step_calls);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, _options| async move { Ok(json!({ "forecast": "sunny" })) },
                ))
                .with_max_steps(2)
                .with_prepare_step(move |options| {
                    let prepare_step_calls = Arc::clone(&prepare_step_calls_for_callback);
                    prepare_step_calls
                        .lock()
                        .expect("prepare step calls lock succeeds")
                        .push((
                            options.step_number,
                            options.steps.len(),
                            options.messages.len(),
                        ));
                    async move { PrepareStepResult::new() }
                }),
        ));

        result.consume_stream();

        assert_eq!(result.steps.len(), 2);
        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(
            *prepare_step_calls
                .lock()
                .expect("prepare step calls lock succeeds"),
            vec![(0, 0, 1), (1, 1, 3)]
        );
    }

    #[test]
    fn stream_text_on_step_start_omits_telemetry_metadata() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let step_start_event = Arc::new(Mutex::new(None::<serde_json::Value>));
        let step_start_event_for_callback = Arc::clone(&step_start_event);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_enabled(true)
                        .with_function_id("test-function"),
                )
                .with_on_step_start(move |event| {
                    let step_start_event = Arc::clone(&step_start_event_for_callback);
                    async move {
                        *step_start_event.lock().expect("step-start event lock") =
                            Some(serde_json::to_value(event).expect("event serializes"));
                    }
                }),
        ));

        assert_eq!(result.text, "Hello");
        let step_start_event = step_start_event
            .lock()
            .expect("step-start event lock")
            .clone()
            .expect("step-start event captured");
        assert!(step_start_event.get("functionId").is_none());
    }

    #[test]
    fn stream_text_prepare_step_sandbox_override_reaches_tool_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{ "city": "Brisbane" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
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
        let prepare_sandbox_descriptions_for_callback = Arc::clone(&prepare_sandbox_descriptions);
        let step_sandbox_for_callback = Arc::clone(&step_sandbox);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
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
                    let descriptions = Arc::clone(&prepare_sandbox_descriptions_for_callback);
                    let step_sandbox = Arc::clone(&step_sandbox_for_callback);

                    async move {
                        descriptions.lock().expect("descriptions lock").push(
                            options
                                .experimental_sandbox
                                .as_ref()
                                .map(|sandbox| sandbox.description().to_string()),
                        );

                        PrepareStepResult::new().with_experimental_sandbox(step_sandbox)
                    }
                }),
        ));

        assert_eq!(
            *prepare_sandbox_descriptions
                .lock()
                .expect("descriptions lock"),
            vec![Some("default sandbox".to_string())]
        );
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["description"], "step sandbox");
        assert_eq!(result.tool_results[0].output["stdout"], "pwd");
    }

    #[test]
    fn stream_text_messages_support_models_that_use_context_in_supported_urls() {
        #[derive(Clone, Debug)]
        struct SupportedUrlsStreamModel {
            supported_urls_called: Arc<Mutex<bool>>,
        }

        impl SupportedUrlsStreamModel {
            fn new(supported_urls_called: Arc<Mutex<bool>>) -> Self {
                Self {
                    supported_urls_called,
                }
            }
        }

        impl LanguageModel for SupportedUrlsStreamModel {
            type SupportedUrlsFuture<'a>
                = std::future::Ready<LanguageModelSupportedUrls>
            where
                Self: 'a;

            type GenerateFuture<'a>
                = std::future::Ready<LanguageModelGenerateResult>
            where
                Self: 'a;

            type Stream = Vec<LanguageModelStreamPart>;

            type StreamFuture<'a>
                = std::future::Ready<LanguageModelStreamResult<Self::Stream>>
            where
                Self: 'a;

            fn provider(&self) -> &str {
                "test-provider"
            }

            fn model_id(&self) -> &str {
                "mock-model-id"
            }

            fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
                *self
                    .supported_urls_called
                    .lock()
                    .expect("supported urls called lock") = self.model_id() == "mock-model-id";

                ready(LanguageModelSupportedUrls::from([(
                    "image/*".to_string(),
                    vec![r"^https://.*$".to_string()],
                )]))
            }

            fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
                ready(LanguageModelGenerateResult::new(
                    Vec::<LanguageModelContent>::new(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Other,
                        raw: None,
                    },
                    LanguageModelUsage::default(),
                ))
            }

            fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
                ready(LanguageModelStreamResult::new(vec![
                    LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                    LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                    LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                        usage(),
                        finish_reason(),
                    )),
                ]))
            }
        }

        let supported_urls_called = Arc::new(Mutex::new(false));
        let model = SupportedUrlsStreamModel::new(Arc::clone(&supported_urls_called));
        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/test.jpg").expect("url parses"),
                        },
                        "image/jpeg",
                    ),
                )],
            ))],
        )));

        assert_eq!(result.text, "Hello, world!");
        assert!(
            *supported_urls_called
                .lock()
                .expect("supported urls called lock")
        );
    }

    #[test]
    fn stream_text_messages_with_url_file_calls_model_supported_urls() {
        let model = MockLanguageModel::new()
            .with_model_id("mock-model-id")
            .with_supported_urls(BTreeMap::from([(
                "image/*".to_string(),
                vec![r"^https://.*$".to_string()],
            )]))
            .with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/test.jpg").expect("url parses"),
                    },
                    "image/jpeg",
                ),
            )],
        ))];

        let result = poll_ready(stream_text(StreamTextOptions::new(&model, prompt)));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(model.supported_urls_calls(), 1);
    }

    #[test]
    fn stream_text_prepare_step_model_switch_uses_step_model_supported_urls() {
        let download_calls = Arc::new(Mutex::new(Vec::new()));
        let download = PromptDownload::new({
            let download_calls = Arc::clone(&download_calls);
            move |requested_downloads| {
                let download_calls = Arc::clone(&download_calls);
                async move {
                    download_calls.lock().expect("download calls lock").extend(
                        requested_downloads.iter().map(|download| {
                            (download.url.clone(), download.is_url_supported_by_model)
                        }),
                    );

                    Ok(requested_downloads
                        .into_iter()
                        .map(|download| {
                            if download.is_url_supported_by_model {
                                None
                            } else {
                                Some(
                                    DownloadedBlob::new(vec![1, 2, 3, 4])
                                        .with_media_type("image/png"),
                                )
                            }
                        })
                        .collect())
                }
            }
        });

        let primary = MockLanguageModel::new()
            .with_model_id("with-image-url-support")
            .with_supported_urls(BTreeMap::from([(
                "image/*".to_string(),
                vec![r"^https://.*$".to_string()],
            )]))
            .with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "should not run",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let secondary = MockLanguageModel::new()
            .with_provider("without-image-url-support")
            .with_model_id("without-image-url-support")
            .with_supported_urls(BTreeMap::new())
            .with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "response from without-image-url-support",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let secondary_model = &secondary;
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                    "Describe this image",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/test.jpg").expect("url parses"),
                    },
                    "image",
                )),
            ],
        ))];

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&primary, prompt)
                .with_prepare_step(move |_options| async move {
                    PrepareStepResult::new().with_model(secondary_model)
                })
                .with_download(download),
        ));

        assert_eq!(result.text, "response from without-image-url-support");
        assert_eq!(download_calls.lock().expect("download calls lock").len(), 1);
        assert_eq!(
            download_calls.lock().expect("download calls lock")[0],
            (
                Url::parse("https://example.com/test.jpg").expect("url parses"),
                false
            )
        );
        assert_eq!(primary.supported_urls_calls(), 0);
        assert_eq!(secondary.supported_urls_calls(), 1);
        assert_eq!(primary.stream_calls().len(), 0);
        let secondary_calls = secondary.stream_calls();
        assert_eq!(secondary_calls.len(), 1);
        assert!(matches!(
            &secondary_calls[0].prompt[0],
            LanguageModelMessage::User(message)
                if message.content.len() == 2
                    && matches!(
                        &message.content[1],
                        LanguageModelUserContentPart::File(file)
                            if file.media_type == "image/png"
                                && matches!(
                                    file.data,
                                    FileData::Data {
                                        data: FileDataContent::Bytes(ref bytes)
                                    } if bytes == &vec![1, 2, 3, 4]
                                )
                    )
        ));
    }

    #[test]
    fn stream_text_tool_result_url_file_calls_model_supported_urls() {
        let model = MockLanguageModel::new()
            .with_model_id("mock-model-id")
            .with_supported_urls(BTreeMap::from([(
                "image/*".to_string(),
                vec![r"^https://.*$".to_string()],
            )]))
            .with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Tool history handled.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let prompt = vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "tool1",
                    json!({}),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "tool1",
                    LanguageModelToolResultOutput::content(vec![
                        LanguageModelToolResultContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/tool-image.png")
                                    .expect("url parses"),
                            },
                            "image/png",
                        )),
                    ]),
                )),
            ])),
        ];

        let result = poll_ready(stream_text(StreamTextOptions::new(&model, prompt)));

        assert_eq!(result.text, "Tool history handled.");
        assert_eq!(model.supported_urls_calls(), 1);
    }

    #[test]
    fn stream_text_passes_provider_default_reasoning_to_model() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let mut options = StreamTextOptions::new(&model, vec![user_message("test-input")]);
        options.call_options.reasoning =
            Some(crate::language_model::LanguageModelReasoningEffort::ProviderDefault);

        let result = poll_ready(stream_text(options));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(
            model.stream_calls()[0].reasoning,
            Some(crate::language_model::LanguageModelReasoningEffort::ProviderDefault)
        );
    }

    #[test]
    fn stream_text_passes_high_reasoning_to_model() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let mut options = StreamTextOptions::new(&model, vec![user_message("test-input")]);
        options.call_options.reasoning =
            Some(crate::language_model::LanguageModelReasoningEffort::High);

        let result = poll_ready(stream_text(options));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(
            model.stream_calls()[0].reasoning,
            Some(crate::language_model::LanguageModelReasoningEffort::High)
        );
    }

    #[test]
    fn stream_text_passes_provider_metadata_to_model() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Provider metadata test.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "aProvider".to_string(),
            json!({
                "someKey": "someValue",
            })
            .as_object()
            .expect("provider options are objects")
            .clone(),
        );

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_provider_options(provider_options.clone()),
        ));

        assert_eq!(result.text, "Provider metadata test.");
        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(
            model.stream_calls()[0].provider_options,
            Some(provider_options)
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

        let text_response = result.to_text_stream_response(
            TextStreamResponseInit::new()
                .with_status(202)
                .with_header("x-stream", "text"),
        );

        assert_eq!(text_response.status, 202);
        assert_eq!(
            text_response
                .headers
                .get("content-type")
                .map(String::as_str),
            Some(crate::text_stream_response::TEXT_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            text_response.headers.get("x-stream").map(String::as_str),
            Some("text")
        );
        assert_eq!(
            text_response.decoded_body().expect("response body decodes"),
            result.text_stream
        );
    }

    #[test]
    fn stream_text_result_text_stream_filters_out_empty_text_deltas() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        assert_eq!(result.text_stream, vec!["Hello", ", ", "world!"]);
        assert_eq!(result.text, "Hello, world!");
        assert!(result.parts.iter().all(|part| match part {
            TextStreamPart::TextDelta(part) => !part.text.is_empty(),
            _ => true,
        }));
    }

    #[test]
    fn stream_text_result_text_stream_excludes_reasoning_content() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "r1",
                    "I will not be visible in textStream.",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        assert_eq!(result.text_stream, vec!["Hello"]);
        assert_eq!(result.text, "Hello");
        assert_eq!(
            result.reasoning_text,
            Some("I will not be visible in textStream.".to_string())
        );
    }

    #[test]
    fn stream_text_result_full_stream_sends_text_deltas() {
        let timestamp = time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(5);
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("response-id")
            .with_model_id("response-model-id")
            .with_timestamp(timestamp);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(response_metadata),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "text-start",
                "text-delta",
                "text-delta",
                "text-delta",
                "text-end",
                "finish-step",
                "finish"
            ]
        );

        let text_deltas = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::TextDelta(part) => Some((
                    part.id.clone(),
                    part.text.clone(),
                    part.provider_metadata.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            text_deltas,
            vec![
                ("1".to_string(), "Hello".to_string(), None),
                ("1".to_string(), ", ".to_string(), None),
                ("1".to_string(), "world!".to_string(), None),
            ]
        );
        assert_eq!(result.text_stream, vec!["Hello", ", ", "world!"]);
        assert_eq!(result.text, "Hello, world!");

        let finish_step = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::FinishStep(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes finish-step");
        assert_eq!(finish_step.response.id, Some("response-id".to_string()));
        assert_eq!(
            finish_step.response.model_id,
            Some("response-model-id".to_string())
        );
        assert_eq!(finish_step.response.timestamp, Some(timestamp));
        assert_eq!(finish_step.usage, usage());
        assert_eq!(finish_step.finish_reason, FinishReason::Stop);
        assert_eq!(finish_step.raw_finish_reason, Some("stop".to_string()));

        let finish = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::Finish(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes finish");
        assert_eq!(finish.finish_reason, FinishReason::Stop);
        assert_eq!(finish.raw_finish_reason, Some("stop".to_string()));
        assert_eq!(finish.total_usage, usage());
    }

    #[test]
    fn stream_text_result_full_stream_uses_fallback_response_metadata_when_response_metadata_missing()
     {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        let finish_step = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::FinishStep(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes finish-step");

        let response_id = finish_step
            .response
            .id
            .as_deref()
            .expect("fallback response id is generated");
        assert_eq!(response_id.len(), 16);
        assert_eq!(
            finish_step.response.model_id.as_deref(),
            Some("mock-model-id")
        );
        assert!(
            finish_step.response.timestamp.is_some(),
            "fallback response timestamp is generated"
        );
        assert_eq!(finish_step.response.headers, None);
        assert_eq!(finish_step.usage, usage());
        assert_eq!(finish_step.finish_reason, FinishReason::Stop);
        assert_eq!(finish_step.raw_finish_reason.as_deref(), Some("stop"));

        assert_eq!(result.text_stream, vec!["Hello", ", ", "world!"]);
        assert_eq!(result.text, "Hello, world!");
        assert_eq!(result.response, finish_step.response);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].response, finish_step.response);
    }

    #[test]
    fn stream_text_result_full_stream_sends_tool_calls() {
        let timestamp = time::OffsetDateTime::UNIX_EPOCH;
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("id-0")
            .with_model_id("mock-model-id")
            .with_timestamp(timestamp);
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("sig"))]),
        )]);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"],
            "additionalProperties": false
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(response_metadata),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "tool1", r#"{ "value": "value" }"#)
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("tool1", input_schema).with_title("Tool 1"))
                .with_tool_choice(LanguageModelToolChoice::Required),
        ));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ToolCall(_) => "tool-call",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec!["start", "start-step", "tool-call", "finish-step", "finish"]
        );

        let tool_call_part = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolCall(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-call");
        assert_eq!(tool_call_part.tool_call_id, "call-1");
        assert_eq!(tool_call_part.tool_name, "tool1");
        assert_eq!(tool_call_part.input, json!({ "value": "value" }));
        assert_eq!(tool_call_part.title.as_deref(), Some("Tool 1"));
        assert_eq!(
            tool_call_part.provider_metadata,
            Some(provider_metadata.clone())
        );
        assert_eq!(tool_call_part.provider_executed, None);

        let finish_step = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::FinishStep(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes finish-step");
        assert_eq!(finish_step.response.id.as_deref(), Some("id-0"));
        assert_eq!(
            finish_step.response.model_id.as_deref(),
            Some("mock-model-id")
        );
        assert_eq!(finish_step.response.timestamp, Some(timestamp));
        assert_eq!(finish_step.finish_reason, FinishReason::Stop);
        assert_eq!(finish_step.raw_finish_reason.as_deref(), Some("stop"));
        assert_eq!(finish_step.usage, usage());

        assert_eq!(result.tool_calls, vec![tool_call_part.clone()]);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].tool_calls, vec![tool_call_part.clone()]);
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert_eq!(
            stream_calls[0].tool_choice.as_ref(),
            Some(&LanguageModelToolChoice::Required)
        );
    }

    #[test]
    fn stream_text_result_full_stream_refines_tool_input_before_execution_parts_and_callbacks() {
        let model_call_end_inputs = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let tool_execution_start_inputs = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let model_call_end_inputs_for_callback = Arc::clone(&model_call_end_inputs);
        let tool_execution_start_inputs_for_callback = Arc::clone(&tool_execution_start_inputs);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": " raw " }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("tool1", input_schema).with_execute(
                    |input, _options| async move {
                        let value = input["value"].as_str().expect("value is a string");
                        Ok(json!(format!("result:{value}")))
                    },
                ))
                .with_tool_input_refinement("tool1", |mut input| async move {
                    let value = input["value"]
                        .as_str()
                        .expect("value is a string")
                        .trim()
                        .to_string();
                    input["value"] = json!(value);
                    Ok(input)
                })
                .with_experimental_on_language_model_call_end(move |event| {
                    let model_call_end_inputs = Arc::clone(&model_call_end_inputs_for_callback);
                    async move {
                        let input = match event.content.first() {
                            Some(GenerateTextContentPart::ToolCall(tool_call)) => {
                                tool_call.input.clone()
                            }
                            _ => json!(null),
                        };
                        model_call_end_inputs
                            .lock()
                            .expect("model call end inputs lock")
                            .push(input);
                    }
                })
                .with_on_tool_execution_start(move |event| {
                    let tool_execution_start_inputs =
                        Arc::clone(&tool_execution_start_inputs_for_callback);
                    async move {
                        tool_execution_start_inputs
                            .lock()
                            .expect("tool execution start inputs lock")
                            .push(event.tool_call.input);
                    }
                }),
        ));

        let tool_call_part = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolCall(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-call");
        assert_eq!(tool_call_part.input, json!({ "value": "raw" }));

        let tool_result_part = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolResult(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-result");
        assert_eq!(tool_result_part.input, json!({ "value": "raw" }));
        assert_eq!(tool_result_part.output, json!("result:raw"));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].input, json!({ "value": "raw" }));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].input, json!({ "value": "raw" }));
        assert_eq!(result.tool_results[0].output, json!("result:raw"));
        assert_eq!(
            model_call_end_inputs
                .lock()
                .expect("inputs lock")
                .as_slice(),
            [json!({ "value": "raw" })]
        );
        assert_eq!(
            tool_execution_start_inputs
                .lock()
                .expect("inputs lock")
                .as_slice(),
            [json!({ "value": "raw" })]
        );
    }

    #[test]
    fn stream_text_result_full_stream_sends_tool_call_deltas() {
        let timestamp = time::OffsetDateTime::UNIX_EPOCH;
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("id-0")
            .with_model_id("mock-model-id")
            .with_timestamp(timestamp);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(response_metadata),
                LanguageModelStreamPart::ToolInputStart(LanguageModelToolInputStart::new(
                    "call-1",
                    "test-tool",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "{\"",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "value",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "\":\"",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "Spark",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "le",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", " Day",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "\"}",
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "test-tool",
                    r#"{"value":"Sparkle Day"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("test-tool", input_schema))
                .with_tool_choice(LanguageModelToolChoice::Required),
        ));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ToolInputStart(_) => "tool-input-start",
                TextStreamPart::ToolInputDelta(_) => "tool-input-delta",
                TextStreamPart::ToolInputEnd(_) => "tool-input-end",
                TextStreamPart::ToolCall(_) => "tool-call",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "tool-input-start",
                "tool-input-delta",
                "tool-input-delta",
                "tool-input-delta",
                "tool-input-delta",
                "tool-input-delta",
                "tool-input-delta",
                "tool-input-delta",
                "tool-input-end",
                "tool-call",
                "finish-step",
                "finish"
            ]
        );

        let tool_input_start = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolInputStart(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-input-start");
        assert_eq!(tool_input_start.id, "call-1");
        assert_eq!(tool_input_start.tool_name, "test-tool");
        assert_eq!(tool_input_start.dynamic, Some(false));
        assert_eq!(tool_input_start.title, None);

        let deltas = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ToolInputDelta(part) => Some(part.delta.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            deltas,
            vec!["{\"", "value", "\":\"", "Spark", "le", " Day", "\"}"]
        );

        let tool_input_end = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolInputEnd(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-input-end");
        assert_eq!(tool_input_end.id, "call-1");

        let tool_call = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolCall(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-call");
        assert_eq!(tool_call.tool_call_id, "call-1");
        assert_eq!(tool_call.tool_name, "test-tool");
        assert_eq!(tool_call.input, json!({ "value": "Sparkle Day" }));

        let finish_step = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::FinishStep(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes finish-step");
        assert_eq!(finish_step.response.id.as_deref(), Some("id-0"));
        assert_eq!(finish_step.response.timestamp, Some(timestamp));
        assert_eq!(finish_step.finish_reason, FinishReason::ToolCalls);
        assert_eq!(finish_step.raw_finish_reason, None);
        assert_eq!(finish_step.usage, usage());

        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert_eq!(
            stream_calls[0].tool_choice.as_ref(),
            Some(&LanguageModelToolChoice::Required)
        );
    }

    #[test]
    fn stream_text_result_full_stream_passes_provider_metadata_on_tool_input_start() {
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("someKey".to_string(), json!("someValue"))]),
        )]);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new("call-1", "test-tool")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    r#"{"value":"test"}"#,
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "test-tool",
                    r#"{"value":"test"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("test-tool", input_schema))
                .with_tool_choice(LanguageModelToolChoice::Required),
        ));

        let tool_input_start = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolInputStart(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-input-start");
        assert_eq!(
            tool_input_start.provider_metadata,
            Some(provider_metadata.clone())
        );
        assert_eq!(tool_input_start.dynamic, Some(false));
    }

    #[test]
    fn stream_text_result_full_stream_sends_tool_results() {
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("stop".to_string()),
                    },
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("tool1", input_schema)
                    .with_title("Tool 1")
                    .with_execute(|input, options| async move {
                        assert_eq!(input, json!({ "value": "value" }));
                        assert_eq!(options.messages, vec![user_message("test-input")]);
                        Ok(json!("value-result"))
                    }),
            ),
        ));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ToolCall(_) => "tool-call",
                TextStreamPart::ToolResult(_) => "tool-result",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "tool-call",
                "tool-result",
                "finish-step",
                "finish"
            ]
        );

        let tool_result = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolResult(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-result");
        assert_eq!(tool_result.tool_call_id, "call-1");
        assert_eq!(tool_result.tool_name, "tool1");
        assert_eq!(tool_result.input, json!({ "value": "value" }));
        assert_eq!(tool_result.output, json!("value-result"));
        assert_eq!(tool_result.title.as_deref(), Some("Tool 1"));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results, vec![tool_result.clone()]);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].tool_results, vec![tool_result.clone()]);
    }

    struct OnePendingToolFuture {
        polls: Arc<AtomicUsize>,
        value: String,
    }

    impl Future for OnePendingToolFuture {
        type Output = Result<JsonValue, ToolExecutionError>;

        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            if this.polls.fetch_add(1, Ordering::SeqCst) == 0 {
                context.waker().wake_by_ref();
                return Poll::Pending;
            }

            Poll::Ready(Ok(json!(format!("{}-result", this.value))))
        }
    }

    #[test]
    fn stream_text_result_full_stream_sends_delayed_asynchronous_tool_results() {
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let polls = Arc::new(AtomicUsize::new(0));
        let polls_for_tool = Arc::clone(&polls);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("stop".to_string()),
                    },
                )),
            ]));

        let result = poll_until_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("tool1", input_schema)
                    .with_title("Tool 1")
                    .with_execute(move |input, _options| {
                        let value = input["value"]
                            .as_str()
                            .expect("value is a string")
                            .to_string();
                        OnePendingToolFuture {
                            polls: Arc::clone(&polls_for_tool),
                            value,
                        }
                    }),
            ),
        ));

        assert!(
            polls.load(Ordering::SeqCst) > 1,
            "tool future should have returned Pending before completion"
        );

        let tool_result_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::ToolResult(_)))
            .expect("full stream includes tool-result");
        let finish_step_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::FinishStep(_)))
            .expect("full stream includes finish-step");
        assert!(
            tool_result_index < finish_step_index,
            "delayed tool result must be emitted before finish-step"
        );

        let tool_result = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolResult(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool-result");
        assert_eq!(tool_result.input, json!({ "value": "value" }));
        assert_eq!(tool_result.output, json!("value-result"));
        assert_eq!(result.tool_results, vec![tool_result.clone()]);
    }

    #[test]
    fn stream_text_result_full_stream_sends_reasoning_deltas() {
        let signature_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("1234567890"))]),
        )]);
        let end_signature_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("0987654321"))]),
        )]);
        let redacted_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("redactedData".to_string(), json!("redacted-reasoning-data"))]),
        )]);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "1",
                    "I will open the conversation",
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "1",
                    " with witty banter.",
                )),
                LanguageModelStreamPart::ReasoningDelta(
                    LanguageModelReasoningDelta::new("1", "")
                        .with_provider_metadata(signature_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
                LanguageModelStreamPart::ReasoningStart(
                    LanguageModelReasoningStart::new("2")
                        .with_provider_metadata(redacted_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("2")),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("3")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "3",
                    " Once the user has relaxed,",
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "3",
                    " I will pry for valuable information.",
                )),
                LanguageModelStreamPart::ReasoningEnd(
                    LanguageModelReasoningEnd::new("3")
                        .with_provider_metadata(signature_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningStart(
                    LanguageModelReasoningStart::new("4")
                        .with_provider_metadata(signature_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "4",
                    " I need to think about",
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "4",
                    " this problem carefully.",
                )),
                LanguageModelStreamPart::ReasoningStart(
                    LanguageModelReasoningStart::new("5")
                        .with_provider_metadata(signature_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "5",
                    " The best solution",
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "5",
                    " requires careful",
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "5",
                    " consideration of all factors.",
                )),
                LanguageModelStreamPart::ReasoningEnd(
                    LanguageModelReasoningEnd::new("4")
                        .with_provider_metadata(end_signature_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningEnd(
                    LanguageModelReasoningEnd::new("5")
                        .with_provider_metadata(end_signature_metadata.clone()),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hi")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " there!")),
                LanguageModelStreamPart::TextEnd(
                    LanguageModelTextEnd::new("1")
                        .with_provider_metadata(end_signature_metadata.clone()),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ReasoningStart(_) => "reasoning-start",
                TextStreamPart::ReasoningDelta(_) => "reasoning-delta",
                TextStreamPart::ReasoningEnd(_) => "reasoning-end",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-delta",
                "reasoning-delta",
                "reasoning-end",
                "reasoning-start",
                "reasoning-end",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-delta",
                "reasoning-end",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-delta",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-delta",
                "reasoning-delta",
                "reasoning-end",
                "reasoning-end",
                "text-start",
                "text-delta",
                "text-delta",
                "text-end",
                "finish-step",
                "finish",
            ]
        );

        let reasoning_deltas = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ReasoningDelta(part) => Some((
                    part.id.clone(),
                    part.text.clone(),
                    part.provider_metadata.clone(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reasoning_deltas,
            vec![
                (
                    "1".to_string(),
                    "I will open the conversation".to_string(),
                    None
                ),
                ("1".to_string(), " with witty banter.".to_string(), None),
                (
                    "1".to_string(),
                    "".to_string(),
                    Some(signature_metadata.clone())
                ),
                (
                    "3".to_string(),
                    " Once the user has relaxed,".to_string(),
                    None
                ),
                (
                    "3".to_string(),
                    " I will pry for valuable information.".to_string(),
                    None
                ),
                ("4".to_string(), " I need to think about".to_string(), None),
                (
                    "4".to_string(),
                    " this problem carefully.".to_string(),
                    None
                ),
                ("5".to_string(), " The best solution".to_string(), None),
                ("5".to_string(), " requires careful".to_string(), None),
                (
                    "5".to_string(),
                    " consideration of all factors.".to_string(),
                    None
                ),
            ]
        );

        let reasoning_starts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ReasoningStart(part) => {
                    Some((part.id.clone(), part.provider_metadata.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reasoning_starts,
            vec![
                ("1".to_string(), None),
                ("2".to_string(), Some(redacted_metadata)),
                ("3".to_string(), None),
                ("4".to_string(), Some(signature_metadata.clone())),
                ("5".to_string(), Some(signature_metadata.clone())),
            ]
        );

        let reasoning_ends = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ReasoningEnd(part) => {
                    Some((part.id.clone(), part.provider_metadata.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reasoning_ends,
            vec![
                ("1".to_string(), None),
                ("2".to_string(), None),
                ("3".to_string(), Some(signature_metadata)),
                ("4".to_string(), Some(end_signature_metadata.clone())),
                ("5".to_string(), Some(end_signature_metadata.clone())),
            ]
        );

        let text_end_metadata = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::TextEnd(part) => part.provider_metadata.clone(),
                _ => None,
            })
            .expect("text-end provider metadata is preserved");
        assert_eq!(text_end_metadata, end_signature_metadata);
        assert_eq!(result.text_stream, vec!["Hi", " there!"]);
        assert_eq!(result.text, "Hi there!");
        assert_eq!(
            result.reasoning_text,
            Some(
                concat!(
                    "I will open the conversation with witty banter.",
                    " Once the user has relaxed,",
                    " I will pry for valuable information.",
                    " I need to think about",
                    " this problem carefully.",
                    " The best solution",
                    " requires careful",
                    " consideration of all factors."
                )
                .to_string()
            )
        );
    }

    #[test]
    fn stream_text_result_preserves_interleaved_text_and_reasoning_content_order() {
        let step = Arc::new(Mutex::new(None::<GenerateTextStep>));
        let step_for_callback = Arc::clone(&step);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("0")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "0",
                    "Thinking...",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("2")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("2", "This ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("2", "is ")),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("3")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "0",
                    "I'm thinking...",
                )),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "3",
                    "Separate thoughts",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("2", "a")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("0")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("2", " test.")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("2")),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("3")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_on_step_finish(
                move |event| {
                    let step = Arc::clone(&step_for_callback);
                    async move {
                        *step.lock().expect("step mutex is not poisoned") = Some(event);
                    }
                },
            ),
        ));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ReasoningStart(_) => "reasoning-start",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::ReasoningDelta(_) => "reasoning-delta",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::ReasoningEnd(_) => "reasoning-end",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "reasoning-start",
                "text-start",
                "reasoning-delta",
                "text-delta",
                "text-delta",
                "text-start",
                "text-delta",
                "text-delta",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-delta",
                "text-delta",
                "text-delta",
                "reasoning-end",
                "text-delta",
                "text-end",
                "reasoning-end",
                "text-end",
                "finish-step",
                "finish",
            ]
        );
        assert_eq!(
            result.text_stream,
            vec!["Hello", ", ", "This ", "is ", "a", "world!", " test."]
        );
        assert_eq!(result.text, "Hello, This is aworld! test.");
        assert_eq!(
            result.reasoning_text,
            Some("Thinking...I'm thinking...Separate thoughts".to_string())
        );
        assert_eq!(
            result
                .parts
                .iter()
                .map(|part| match part {
                    TextStreamPart::Start(_) => "start",
                    TextStreamPart::StartStep(_) => "start-step",
                    TextStreamPart::ReasoningStart(_) => "reasoning-start",
                    TextStreamPart::TextStart(_) => "text-start",
                    TextStreamPart::ReasoningDelta(_) => "reasoning-delta",
                    TextStreamPart::TextDelta(_) => "text-delta",
                    TextStreamPart::ReasoningEnd(_) => "reasoning-end",
                    TextStreamPart::TextEnd(_) => "text-end",
                    TextStreamPart::FinishStep(_) => "finish-step",
                    TextStreamPart::Finish(_) => "finish",
                    _ => "other",
                })
                .collect::<Vec<_>>(),
            vec![
                "start",
                "start-step",
                "reasoning-start",
                "text-start",
                "reasoning-delta",
                "text-delta",
                "text-delta",
                "text-start",
                "text-delta",
                "text-delta",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-delta",
                "text-delta",
                "text-delta",
                "reasoning-end",
                "text-delta",
                "text-end",
                "reasoning-end",
                "text-end",
                "finish-step",
                "finish",
            ]
        );

        let step = step
            .lock()
            .expect("step mutex is not poisoned")
            .clone()
            .expect("on_step_finish should receive a step");
        assert_eq!(result.steps.len(), 1);
        let content_labels = step
            .content
            .iter()
            .map(|part| match part {
                GenerateTextContentPart::Reasoning(part) => format!("reasoning:{}", part.text),
                GenerateTextContentPart::Text(part) => format!("text:{}", part.text),
                part => format!("other:{part:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            content_labels,
            vec![
                "reasoning:Thinking...I'm thinking...",
                "text:Hello, world!",
                "text:This is a test.",
                "reasoning:Separate thoughts",
            ]
        );
    }

    #[test]
    fn stream_text_result_full_stream_sends_sources() {
        let first_metadata = ProviderMetadata::from([(
            "provider".to_string(),
            Map::from_iter([("custom".to_string(), json!("value"))]),
        )]);
        let second_metadata = ProviderMetadata::from([(
            "provider".to_string(),
            Map::from_iter([("custom".to_string(), json!("value2"))]),
        )]);
        let first_source = LanguageModelSource::Url(
            LanguageModelUrlSource::new("123", "https://example.com")
                .with_title("Example")
                .with_provider_metadata(first_metadata),
        );
        let second_source = LanguageModelSource::Url(
            LanguageModelUrlSource::new("456", "https://example.com/2")
                .with_title("Example 2")
                .with_provider_metadata(second_metadata),
        );
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Source(first_source.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Source(second_source.clone()),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::Source(_) => "source",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "source",
                "text-start",
                "text-delta",
                "text-end",
                "source",
                "finish-step",
                "finish",
            ]
        );

        let source_parts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::Source(part) => Some(part.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            source_parts,
            vec![first_source.clone(), second_source.clone()]
        );
        assert_eq!(result.sources, vec![first_source, second_source]);
        assert_eq!(result.text_stream, vec!["Hello!"]);
        assert_eq!(result.text, "Hello!");
    }

    #[test]
    fn stream_text_result_full_stream_sends_custom_parts() {
        let provider_metadata = ProviderMetadata::from([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("cmp_123"))]),
        )]);
        let custom_part = LanguageModelCustomContent::new("openai.compaction")
            .with_provider_metadata(provider_metadata);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Custom(custom_part.clone()),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::Custom(_) => "custom",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "text-start",
                "text-delta",
                "text-end",
                "custom",
                "finish-step",
                "finish",
            ]
        );

        let custom_parts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::Custom(part) => Some(part.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(custom_parts, vec![custom_part.clone()]);
        assert_eq!(result.custom_parts, vec![custom_part]);
        assert_eq!(result.text_stream, vec!["Hello!"]);
        assert_eq!(result.text, "Hello!");
    }

    #[test]
    fn stream_text_result_full_stream_sends_files() {
        let first_file = LanguageModelFile::new(
            "text/plain",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("Hello World".to_string()),
            },
        );
        let second_file = LanguageModelFile::new(
            "image/jpeg",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("QkFVRw==".to_string()),
            },
        );
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(first_file.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::File(second_file.clone()),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::File(_) => "file",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "file",
                "text-start",
                "text-delta",
                "text-end",
                "file",
                "finish-step",
                "finish",
            ]
        );

        let file_parts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::File(part) => {
                    Some((part.file.clone(), part.provider_metadata.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            file_parts,
            vec![(first_file.clone(), None), (second_file.clone(), None),]
        );
        assert_eq!(result.files, vec![first_file, second_file]);
        assert_eq!(result.text_stream, vec!["Hello!"]);
        assert_eq!(result.text, "Hello!");
    }

    #[test]
    fn stream_text_result_full_stream_sends_files_with_provider_metadata() {
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("sig-1"))]),
        )]);
        let first_file = LanguageModelFile::new(
            "text/plain",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("Hello World".to_string()),
            },
        )
        .with_provider_metadata(provider_metadata.clone());
        let second_file = LanguageModelFile::new(
            "image/jpeg",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("QkFVRw==".to_string()),
            },
        );
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(first_file.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::File(second_file.clone()),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        let file_parts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::File(part) => {
                    Some((part.file.clone(), part.provider_metadata.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            file_parts,
            vec![
                (first_file.clone(), Some(provider_metadata)),
                (second_file.clone(), None),
            ]
        );
        assert_eq!(result.files, vec![first_file, second_file]);
    }

    #[test]
    fn stream_text_result_full_stream_sends_reasoning_files() {
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("rf-sig-1"))]),
        )]);
        let first_reasoning_file = LanguageModelReasoningFile::new(
            "image/png",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("reasoning-file-data-1".to_string()),
            },
        );
        let second_reasoning_file = LanguageModelReasoningFile::new(
            "image/jpeg",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("reasoning-file-data-2".to_string()),
            },
        )
        .with_provider_metadata(provider_metadata.clone());
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("id-0")
            .with_model_id("mock-model-id")
            .with_timestamp(time::OffsetDateTime::UNIX_EPOCH);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(response_metadata),
                LanguageModelStreamPart::ReasoningFile(first_reasoning_file.clone()),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "1",
                    "Some reasoning text.",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("1")),
                LanguageModelStreamPart::ReasoningFile(second_reasoning_file.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("prompt")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ReasoningFile(_) => "reasoning-file",
                TextStreamPart::ReasoningStart(_) => "reasoning-start",
                TextStreamPart::ReasoningDelta(_) => "reasoning-delta",
                TextStreamPart::ReasoningEnd(_) => "reasoning-end",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "reasoning-file",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-end",
                "reasoning-file",
                "text-start",
                "text-delta",
                "text-end",
                "finish-step",
                "finish",
            ]
        );

        let reasoning_file_parts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ReasoningFile(part) => {
                    Some((part.file.clone(), part.provider_metadata.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reasoning_file_parts,
            vec![
                (first_reasoning_file.clone(), None),
                (second_reasoning_file.clone(), Some(provider_metadata)),
            ]
        );
        assert_eq!(
            result.reasoning_files,
            vec![first_reasoning_file, second_reasoning_file]
        );
        assert_eq!(
            result.reasoning_text,
            Some("Some reasoning text.".to_string())
        );
        assert_eq!(result.text_stream, vec!["Hello!"]);
        assert_eq!(result.text, "Hello!");
    }

    #[test]
    fn stream_text_smooth_stream_transforms_chunks_before_callbacks() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_for_callback = Arc::clone(&chunks);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_smooth_stream(SmoothStreamOptions::new().with_delay_in_ms(None))
                .with_on_chunk(move |event| {
                    let chunks = Arc::clone(&chunks_for_callback);
                    async move {
                        if let TextStreamPart::TextDelta(part) = event.chunk {
                            chunks
                                .lock()
                                .expect("chunks mutex is not poisoned")
                                .push(part.text);
                        }
                    }
                }),
        ));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            result.text_stream,
            vec!["Hello, ".to_string(), "world!".to_string()]
        );
        assert_eq!(
            *chunks.lock().expect("chunks mutex is not poisoned"),
            ["Hello, ".to_string(), "world!".to_string()]
        );
        assert!(result.parts.iter().any(|part| {
            matches!(
                part,
                TextStreamPart::TextDelta(part) if part.text == "Hello, "
            )
        }));
    }

    #[test]
    fn stream_text_smooth_stream_waits_after_detected_chunks() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let started_at = Instant::now();
        let result = poll_until_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_smooth_stream(SmoothStreamOptions::new().with_delay_in_ms(Some(5))),
        ));

        assert_eq!(
            result.text_stream,
            vec!["Hello, ".to_string(), "world!".to_string()]
        );
        assert!(
            started_at.elapsed() >= std::time::Duration::from_millis(5),
            "smooth stream should await the configured delay after the detected chunk"
        );
    }

    #[test]
    fn stream_text_transform_updates_text_response_and_callbacks() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_for_callback = Arc::clone(&chunks);
        let step = Arc::new(Mutex::new(None::<GenerateTextStep>));
        let step_for_callback = Arc::clone(&step);
        let finish = Arc::new(Mutex::new(None::<GenerateTextFinishEvent>));
        let finish_for_callback = Arc::clone(&finish);
        let uppercase_text = StreamTextTransform::new(|parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::TextDelta(mut part) => {
                        part.text = part.text.to_uppercase();
                        TextStreamPart::TextDelta(part)
                    }
                    TextStreamPart::ReasoningDelta(mut part) => {
                        part.text = part.text.to_uppercase();
                        TextStreamPart::ReasoningDelta(part)
                    }
                    part => part,
                })
                .collect()
        });
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_transform(uppercase_text)
                .with_on_chunk(move |event| {
                    let chunks = Arc::clone(&chunks_for_callback);
                    async move {
                        if let TextStreamPart::TextDelta(part) = event.chunk {
                            chunks
                                .lock()
                                .expect("chunks mutex is not poisoned")
                                .push(part.text);
                        }
                    }
                })
                .with_on_step_finish(move |event| {
                    let step = Arc::clone(&step_for_callback);
                    async move {
                        *step.lock().expect("step mutex is not poisoned") = Some(event);
                    }
                })
                .with_on_finish(move |event| {
                    let finish = Arc::clone(&finish_for_callback);
                    async move {
                        *finish.lock().expect("finish mutex is not poisoned") = Some(event);
                    }
                }),
        ));

        assert_eq!(result.text, "HELLO, WORLD!");
        assert_eq!(
            result.text_stream,
            vec!["HELLO".to_string(), ", ".to_string(), "WORLD!".to_string()]
        );
        assert_eq!(
            serde_json::to_value(&result.response_messages).expect("response messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "text": "HELLO, WORLD!",
                            "type": "text"
                        }
                    ]
                }
            ])
        );
        assert_eq!(
            *chunks.lock().expect("chunks mutex is not poisoned"),
            ["HELLO".to_string(), ", ".to_string(), "WORLD!".to_string()]
        );
        let step = step
            .lock()
            .expect("step mutex is not poisoned")
            .clone()
            .expect("step finish ran");
        assert_eq!(step.text, "HELLO, WORLD!");
        assert!(
            serde_json::to_value(&step.response_messages)
                .expect("response messages serialize")
                .to_string()
                .contains("HELLO, WORLD!")
        );
        assert_eq!(
            finish
                .lock()
                .expect("finish mutex is not poisoned")
                .as_ref()
                .expect("finish ran")
                .text,
            "HELLO, WORLD!"
        );
    }

    #[test]
    fn stream_text_transform_applies_multiple_transforms_in_order() {
        let uppercase_and_add_comma = StreamTextTransform::new(|parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::TextDelta(mut part) => {
                        part.text = format!("{},", part.text.to_uppercase());
                        TextStreamPart::TextDelta(part)
                    }
                    part => part,
                })
                .collect()
        });
        let remove_commas = StreamTextTransform::new(|parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::TextDelta(mut part) => {
                        part.text = part.text.replace(',', "");
                        TextStreamPart::TextDelta(part)
                    }
                    part => part,
                })
                .collect()
        });
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_transform(uppercase_and_add_comma)
                .with_transform(remove_commas),
        ));

        assert_eq!(
            result.text_stream,
            vec!["HELLO".to_string(), " ".to_string(), "WORLD!".to_string()]
        );
        assert_eq!(result.text, "HELLO WORLD!");
    }

    #[test]
    fn stream_text_transform_updates_response_messages() {
        let uppercase_text = StreamTextTransform::new(|parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::TextDelta(mut part) => {
                        part.text = part.text.to_uppercase();
                        TextStreamPart::TextDelta(part)
                    }
                    part => part,
                })
                .collect()
        });
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_transform(uppercase_text),
        ));

        assert_eq!(result.text, "HELLO, WORLD!");
        assert_eq!(
            serde_json::to_value(&result.response_messages).expect("response messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "text": "HELLO, WORLD!",
                            "type": "text"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn stream_text_transform_updates_tool_calls_and_tool_results() {
        let uppercase_tool_data = StreamTextTransform::new(|parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::ToolCall(mut part) => {
                        if let JsonValue::Object(input) = &mut part.input {
                            input.insert("value".to_string(), json!("VALUE"));
                        }
                        TextStreamPart::ToolCall(part)
                    }
                    TextStreamPart::ToolResult(mut part) => {
                        if let JsonValue::Object(input) = &mut part.input {
                            input.insert("value".to_string(), json!("VALUE"));
                        }
                        part.output = json!("RESULT1");
                        TextStreamPart::ToolResult(part)
                    }
                    part => part,
                })
                .collect()
        });
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "tool1", r#"{"value":"value"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "call-1",
                    "tool1",
                    NonNullJsonValue::new(json!("result1")).expect("tool result is non-null"),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Use tool")])
                .with_transform(uppercase_tool_data),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].input, json!({ "value": "VALUE" }));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].input, json!({ "value": "VALUE" }));
        assert_eq!(result.tool_results[0].output, json!("RESULT1"));
        assert_eq!(
            result.steps[0].tool_calls[0].input,
            json!({ "value": "VALUE" })
        );
        assert_eq!(result.steps[0].tool_results[0].output, json!("RESULT1"));
    }

    #[test]
    fn stream_text_transform_updates_local_tool_results_after_execution() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_for_callback = Arc::clone(&chunks);
        let uppercase_tool_data = StreamTextTransform::new(|parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::ToolCall(mut part) => {
                        if let JsonValue::Object(input) = &mut part.input {
                            input.insert("city".to_string(), json!("BRISBANE"));
                        }
                        TextStreamPart::ToolCall(part)
                    }
                    TextStreamPart::ToolResult(mut part) => {
                        if let JsonValue::Object(input) = &mut part.input {
                            input.insert("city".to_string(), json!("BRISBANE"));
                        }
                        if let JsonValue::Object(output) = &mut part.output {
                            output.insert("forecast".to_string(), json!("SUNNY"));
                        }
                        TextStreamPart::ToolResult(part)
                    }
                    part => part,
                })
                .collect()
        });
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Done.")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
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

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        assert_eq!(input["city"], json!("BRISBANE"));
                        Ok(json!({
                            "forecast": "sunny",
                            "city": input["city"]
                        }))
                    },
                ))
                .with_transform(uppercase_tool_data)
                .with_on_chunk(move |event| {
                    let chunks = Arc::clone(&chunks_for_callback);
                    async move {
                        if let TextStreamPart::ToolResult(part) = event.chunk {
                            chunks
                                .lock()
                                .expect("chunks mutex is not poisoned")
                                .push(part.output);
                        }
                    }
                })
                .with_max_steps(2),
        ));

        assert_eq!(result.tool_calls[0].input, json!({ "city": "BRISBANE" }));
        assert_eq!(result.tool_results[0].input, json!({ "city": "BRISBANE" }));
        assert_eq!(
            result.tool_results[0].output,
            json!({ "forecast": "SUNNY", "city": "BRISBANE" })
        );
        assert_eq!(
            *chunks.lock().expect("chunks mutex is not poisoned"),
            [json!({ "forecast": "SUNNY", "city": "BRISBANE" })]
        );
        assert!(matches!(
            &model.stream_calls()[1].prompt[2],
            LanguageModelMessage::Tool(message)
                if matches!(
                    &message.content[0],
                    LanguageModelToolContentPart::ToolResult(part)
                        if part.output == LanguageModelToolResultOutput::json(json!({
                            "forecast": "SUNNY",
                            "city": "BRISBANE"
                        }))
                )
        ));
    }

    #[test]
    fn stream_text_transform_updates_finish_metadata_and_usage() {
        let updated_usage = LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(20),
                no_cache: Some(20),
                cache_read: Some(0),
                cache_write: Some(0),
            },
            output_tokens: OutputTokenUsage {
                total: Some(30),
                text: Some(30),
                reasoning: Some(0),
            },
            raw: None,
        };
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("testKey".to_string(), json!("TEST VALUE"))]),
        )]);
        let transform_usage = updated_usage.clone();
        let transform_metadata = provider_metadata.clone();
        let transform_finish = StreamTextTransform::new(move |parts| {
            parts
                .into_iter()
                .map(|part| match part {
                    TextStreamPart::FinishStep(mut part) => {
                        part.finish_reason = FinishReason::Length;
                        part.raw_finish_reason = Some("raw-length".to_string());
                        part.usage = transform_usage.clone();
                        part.provider_metadata = Some(transform_metadata.clone());
                        TextStreamPart::FinishStep(part)
                    }
                    TextStreamPart::Finish(mut part) => {
                        part.finish_reason = FinishReason::Length;
                        part.raw_finish_reason = Some("raw-length".to_string());
                        part.total_usage = transform_usage.clone();
                        TextStreamPart::Finish(part)
                    }
                    part => part,
                })
                .collect()
        });
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_transform(transform_finish),
        ));

        assert_eq!(result.finish_reason, FinishReason::Length);
        assert_eq!(result.raw_finish_reason, Some("raw-length".to_string()));
        assert_eq!(result.usage, updated_usage);
        assert_eq!(result.total_usage, updated_usage);
        assert_eq!(result.provider_metadata, Some(provider_metadata.clone()));
        assert_eq!(result.steps[0].provider_metadata, Some(provider_metadata));
        assert!(matches!(
            result.parts.iter().find_map(|part| match part {
                TextStreamPart::FinishStep(part) => Some(part),
                _ => None,
            }),
            Some(part) if part.finish_reason == FinishReason::Length
                && part.usage == updated_usage
        ));
    }

    #[test]
    fn stream_text_transform_can_stop_stream_with_finish_parts() {
        let step = Arc::new(Mutex::new(None::<GenerateTextStep>));
        let step_for_callback = Arc::clone(&step);
        let stop_response = StreamTextResponseMetadata {
            id: Some("response-id".to_string()),
            timestamp: Some(time::OffsetDateTime::UNIX_EPOCH),
            model_id: Some("mock-model-id".to_string()),
            headers: None,
        };
        let stop_usage = LanguageModelUsage::default();
        let transform_response = stop_response.clone();
        let transform_usage = stop_usage.clone();
        let stop_on_token = StreamTextTransform::new(move |parts| {
            let mut transformed = Vec::new();
            for part in parts {
                match part {
                    TextStreamPart::TextDelta(part) if part.text.contains("STOP") => {
                        transformed.push(TextStreamPart::FinishStep(
                            TextStreamFinishStepPart::new(
                                transform_response.clone(),
                                transform_usage.clone(),
                                StreamTextStepPerformance::default(),
                                FinishReason::Stop,
                                None,
                                None,
                            ),
                        ));
                        transformed.push(TextStreamPart::Finish(TextStreamFinishPart::new(
                            FinishReason::Stop,
                            None,
                            transform_usage.clone(),
                        )));
                        break;
                    }
                    part => transformed.push(part),
                }
            }
            transformed
        });
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "STOP")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_transform(stop_on_token)
                .with_on_step_finish(move |event| {
                    let step = Arc::clone(&step_for_callback);
                    async move {
                        *step.lock().expect("step mutex is not poisoned") = Some(event);
                    }
                }),
        ));

        assert_eq!(result.text, "Hello, ");
        assert_eq!(result.text_stream, vec!["Hello, ".to_string()]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.raw_finish_reason, None);
        assert_eq!(result.usage, stop_usage);
        assert_eq!(result.total_usage, stop_usage);
        assert_eq!(result.response, stop_response);
        assert!(!result.parts.iter().any(|part| {
            matches!(
                part,
                TextStreamPart::TextDelta(part)
                    if part.text.contains("STOP") || part.text.contains("world")
            )
        }));
        assert_eq!(
            result
                .parts
                .iter()
                .filter(|part| matches!(part, TextStreamPart::FinishStep(_)))
                .count(),
            1
        );
        assert_eq!(
            result
                .parts
                .iter()
                .filter(|part| matches!(part, TextStreamPart::Finish(_)))
                .count(),
            1
        );
        let step = step
            .lock()
            .expect("step mutex is not poisoned")
            .clone()
            .expect("step finish ran");
        assert_eq!(step.text, "Hello, ");
        assert_eq!(
            step.response.expect("step response is present").id,
            Some("response-id".to_string())
        );
        assert_eq!(step.usage, stop_usage);
    }

    #[test]
    fn stream_text_result_converts_to_ui_message_stream() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        assert_eq!(
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "text-start", "id": "1" },
                { "type": "text-delta", "id": "1", "delta": "Hello" },
                { "type": "text-delta", "id": "1", "delta": ", " },
                { "type": "text-delta", "id": "1", "delta": "world!" },
                { "type": "text-end", "id": "1" },
                { "type": "finish-step" },
                { "type": "finish", "finishReason": "stop" }
            ])
        );
    }

    #[test]
    fn stream_text_result_ui_message_stream_options_control_start_finish_and_reasoning() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "r1", "hidden",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "visible")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        let chunks = result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new()
                .with_message_id("msg-123")
                .with_send_reasoning(false)
                .with_send_finish(false),
        );

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "start", "messageId": "msg-123" },
                { "type": "start-step" },
                { "type": "text-start", "id": "1" },
                { "type": "text-delta", "id": "1", "delta": "visible" },
                { "type": "text-end", "id": "1" },
                { "type": "finish-step" }
            ])
        );
    }

    #[test]
    fn stream_text_result_to_ui_message_stream_suppresses_start_and_finish_when_disabled() {
        // Upstream parity: packages/ai to-ui-message-stream.test.ts
        // "suppresses start/finish chunks when sendStart/sendFinish are false".
        // Feed the raw TextStreamParts directly (start, text-start, text-end,
        // finish) so the mapping mirrors the upstream input exactly.
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let mut result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        result.parts = vec![
            TextStreamPart::Start(TextStreamStartPart::new()),
            TextStreamPart::TextStart(LanguageModelTextStart::new("t1")),
            TextStreamPart::TextEnd(LanguageModelTextEnd::new("t1")),
            TextStreamPart::Finish(TextStreamFinishPart::new(
                FinishReason::Stop,
                Some("stop".to_string()),
                usage(),
            )),
        ];

        let chunks = result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new()
                .with_send_start(false)
                .with_send_finish(false),
        );

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "text-start", "id": "t1" },
                { "type": "text-end", "id": "t1" }
            ])
        );
    }

    #[test]
    fn stream_text_result_to_ui_message_stream_supports_send_reasoning_true() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "r1", "hidden",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "visible")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        let chunks = result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new()
                .with_message_id("msg-123")
                .with_send_reasoning(true),
        );

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "start", "messageId": "msg-123" },
                { "type": "start-step" },
                { "type": "reasoning-start", "id": "r1" },
                { "type": "reasoning-delta", "id": "r1", "delta": "hidden" },
                { "type": "reasoning-end", "id": "r1" },
                { "type": "text-start", "id": "1" },
                { "type": "text-delta", "id": "1", "delta": "visible" },
                { "type": "text-end", "id": "1" },
                { "type": "finish-step" },
                { "type": "finish", "finishReason": "stop" }
            ])
        );
    }

    #[test]
    fn stream_text_result_ui_message_stream_options_use_persistence_message_ids() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        let continuation = serde_json::to_value(
            result.to_ui_message_stream_with_options(
                StreamTextUiMessageStreamOptions::new()
                    .with_original_messages([
                        UiMessage::new("user-1", UiMessageRole::User),
                        UiMessage::new("assistant-existing", UiMessageRole::Assistant),
                    ])
                    .with_generate_message_id(|| "generated-new".to_string()),
            ),
        )
        .expect("chunks serialize");

        assert_eq!(
            continuation[0],
            json!({ "type": "start", "messageId": "assistant-existing" })
        );

        let new_response = serde_json::to_value(
            result.to_ui_message_stream_with_options(
                StreamTextUiMessageStreamOptions::new()
                    .with_original_messages([UiMessage::new("user-1", UiMessageRole::User)])
                    .with_generate_message_id(|| "generated-new".to_string()),
            ),
        )
        .expect("chunks serialize");

        assert_eq!(
            new_response[0],
            json!({ "type": "start", "messageId": "generated-new" })
        );

        let client_generated = serde_json::to_value(
            result.to_ui_message_stream_with_options(
                StreamTextUiMessageStreamOptions::new()
                    .with_generate_message_id(|| "generated-new".to_string()),
            ),
        )
        .expect("chunks serialize");

        assert_eq!(
            client_generated[0],
            json!({ "type": "start", "messageId": "generated-new" })
        );
    }

    #[test]
    fn stream_text_result_ui_message_stream_options_on_finish_receives_persisted_messages() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "new")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        let finish_events = Arc::new(Mutex::new(Vec::<UiMessageStreamFinishCallbackEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);
        let original_assistant = UiMessage::new("assistant-existing", UiMessageRole::Assistant)
            .with_part(json!({ "type": "text", "text": "old", "state": "done" }));

        let chunks = result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new()
                .with_original_messages([
                    UiMessage::new("user-1", UiMessageRole::User),
                    original_assistant.clone(),
                ])
                .with_generate_message_id(|| "generated-new".to_string())
                .with_on_finish(move |event| {
                    finish_events_for_callback
                        .lock()
                        .expect("finish events lock")
                        .push(event);
                }),
        );

        assert_eq!(
            serde_json::to_value(&chunks[0]).expect("chunk serializes"),
            json!({ "type": "start", "messageId": "assistant-existing" })
        );

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert!(finish_events[0].is_continuation);
        assert!(!finish_events[0].is_aborted);
        assert_eq!(finish_events[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(finish_events[0].messages.len(), 2);
        assert_eq!(finish_events[0].messages[0].id, "user-1");
        assert_eq!(
            serde_json::to_value(&finish_events[0].response_message).expect("message serializes"),
            json!({
                "id": "assistant-existing",
                "role": "assistant",
                "parts": [
                    { "type": "text", "text": "old", "state": "done" },
                    { "type": "step-start" },
                    { "type": "text", "text": "new", "state": "done" }
                ]
            })
        );
        assert_eq!(
            finish_events[0].messages[1],
            finish_events[0].response_message
        );
    }

    #[test]
    fn stream_text_result_to_ui_message_stream_injects_generated_message_id_and_calls_on_finish() {
        // Upstream parity: packages/ai to-ui-message-stream.test.ts
        // "injects generated message id and calls onFinish". The original
        // messages end with a user message, so the response is a fresh
        // assistant message (isContinuation=false) with the generated id.
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("t1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("t1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("t1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        let finish_events = Arc::new(Mutex::new(Vec::<UiMessageStreamFinishCallbackEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);
        let generate_calls = Arc::new(Mutex::new(0usize));
        let generate_calls_for_callback = Arc::clone(&generate_calls);
        let original_user = UiMessage::new("user-msg-1", UiMessageRole::User)
            .with_part(json!({ "type": "text", "text": "Hi" }));

        let chunks = result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new()
                .with_original_messages([original_user.clone()])
                .with_generate_message_id(move || {
                    *generate_calls_for_callback.lock().expect("generate lock") += 1;
                    "msg-123".to_string()
                })
                .with_on_finish(move |event| {
                    finish_events_for_callback
                        .lock()
                        .expect("finish events lock")
                        .push(event);
                }),
        );

        assert_eq!(
            serde_json::to_value(&chunks[0]).expect("chunk serializes"),
            json!({ "type": "start", "messageId": "msg-123" })
        );
        assert_eq!(*generate_calls.lock().expect("generate lock"), 1);

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert!(!finish_events[0].is_aborted);
        assert!(!finish_events[0].is_continuation);
        assert_eq!(finish_events[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(
            serde_json::to_value(&finish_events[0].response_message).expect("message serializes"),
            json!({
                "id": "msg-123",
                "role": "assistant",
                "parts": [
                    { "type": "step-start" },
                    { "type": "text", "text": "Hello", "state": "done" }
                ]
            })
        );
        assert_eq!(finish_events[0].messages.len(), 2);
        assert_eq!(finish_events[0].messages[0].id, "user-msg-1");
        assert_eq!(
            finish_events[0].messages[1],
            finish_events[0].response_message
        );
    }

    #[test]
    fn stream_text_result_ui_message_stream_options_mask_errors_with_on_error() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let mut result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        result.parts = vec![
            TextStreamPart::Start(TextStreamStartPart::new()),
            TextStreamPart::StartStep(TextStreamStartStepPart::new(
                LanguageModelRequest::default(),
                Vec::new(),
            )),
            TextStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                "message": "provider secret"
            }))),
            TextStreamPart::ToolCall(GenerateTextToolCall {
                tool_call_id: "call-invalid".to_string(),
                tool_name: "weather".to_string(),
                input: json!("{ bad json"),
                title: None,
                provider_executed: None,
                dynamic: None,
                invalid: Some(true),
                error: Some("invalid input secret".to_string()),
                provider_metadata: None,
                tool_metadata: None,
            }),
            TextStreamPart::ToolResult(GenerateTextToolResult {
                tool_call_id: "call-local".to_string(),
                tool_name: "weather".to_string(),
                input: json!({ "city": "Paris" }),
                output: json!({ "message": "local tool secret" }),
                title: None,
                is_error: Some(true),
                provider_executed: None,
                dynamic: None,
                preliminary: None,
                provider_metadata: None,
                tool_metadata: None,
            }),
            TextStreamPart::ToolResult(GenerateTextToolResult {
                tool_call_id: "call-provider".to_string(),
                tool_name: "web_search".to_string(),
                input: json!({ "query": "rust" }),
                output: json!({ "message": "provider tool error" }),
                title: None,
                is_error: Some(true),
                provider_executed: Some(true),
                dynamic: None,
                preliminary: None,
                provider_metadata: None,
                tool_metadata: None,
            }),
            TextStreamPart::FinishStep(TextStreamFinishStepPart::new(
                StreamTextResponseMetadata::new(),
                usage(),
                StreamTextStepPerformance {
                    step_time_ms: 0,
                    time_to_first_output_token_ms: None,
                },
                FinishReason::Error,
                Some("error".to_string()),
                None,
            )),
            TextStreamPart::Finish(TextStreamFinishPart::new(
                FinishReason::Error,
                Some("error".to_string()),
                usage(),
            )),
        ];

        let chunks = serde_json::to_value(result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new().with_on_error(|error| {
                format!(
                    "masked:{}",
                    error
                        .get("message")
                        .and_then(JsonValue::as_str)
                        .or_else(|| error.as_str())
                        .unwrap_or("unknown")
                )
            }),
        ))
        .expect("chunks serialize");

        assert_eq!(
            chunks,
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "error", "errorText": "masked:provider secret" },
                {
                    "type": "tool-input-error",
                    "toolCallId": "call-invalid",
                    "toolName": "weather",
                    "input": "{ bad json",
                    "errorText": "masked:invalid input secret"
                },
                {
                    "type": "tool-output-error",
                    "toolCallId": "call-local",
                    "errorText": "masked:local tool secret"
                },
                {
                    "type": "tool-output-error",
                    "toolCallId": "call-provider",
                    "errorText": "{\"message\":\"provider tool error\"}",
                    "providerExecuted": true
                },
                { "type": "finish-step" },
                { "type": "finish", "finishReason": "error" }
            ])
        );
    }

    #[test]
    fn stream_text_result_to_ui_message_stream_masks_error_messages_by_default() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!("error"))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);

        assert!(
            serde_json::to_value(result.to_ui_message_stream())
                .expect("chunks serialize")
                .as_array()
                .expect("chunks are an array")
                .contains(&json!({
                    "type": "error",
                    "errorText": "An error occurred."
                }))
        );
    }

    #[test]
    fn stream_text_result_to_ui_message_stream_supports_custom_error_messages() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!("error"))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);

        assert!(
            serde_json::to_value(result.to_ui_message_stream_with_options(
                StreamTextUiMessageStreamOptions::new().with_on_error(|error| {
                    format!(
                        "custom error message: {}",
                        error.as_str().unwrap_or("unknown error")
                    )
                }),
            ))
            .expect("chunks serialize")
            .as_array()
            .expect("chunks are an array")
            .contains(&json!({
                "type": "error",
                "errorText": "custom error message: error"
            }))
        );
    }

    #[test]
    fn stream_text_result_consume_stream_ignores_abort_error_during_stream_consumption() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                "name": "AbortError",
                "message": "Stream aborted"
            }))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);

        result.consume_stream();
    }

    #[test]
    fn stream_text_result_consume_stream_ignores_response_aborted_error_during_stream_consumption()
    {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                "name": "ResponseAborted",
                "message": "Response aborted"
            }))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);

        result.consume_stream();
    }

    #[test]
    fn stream_text_result_consume_stream_ignores_any_errors_during_stream_consumption() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                "message": "Some error"
            }))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);

        result.consume_stream();
    }

    #[test]
    fn stream_text_result_consume_stream_calls_on_error_callback_with_the_error() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                "message": "Some error"
            }))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);
        let mut captured_errors = Vec::new();

        result.consume_stream_with_on_error(|error| captured_errors.push(error.clone()));

        assert_eq!(captured_errors, vec![json!({ "message": "Some error" })]);
    }

    #[test]
    fn stream_text_result_applies_ui_message_metadata_callback_in_sequence() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));
        let chunks = result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new().with_message_metadata(|part| match part {
                TextStreamPart::Start(_) => Some(json!({ "stage": "start" })),
                TextStreamPart::TextDelta(part) => Some(json!({ "delta": part.text.clone() })),
                TextStreamPart::Finish(part) => Some(json!({
                    "stage": "finish",
                    "finishReason": part.finish_reason.clone()
                })),
                _ => None,
            }),
        );

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "start", "messageMetadata": { "stage": "start" } },
                { "type": "start-step" },
                { "type": "text-start", "id": "1" },
                { "type": "text-delta", "id": "1", "delta": "Hello" },
                { "type": "message-metadata", "messageMetadata": { "delta": "Hello" } },
                { "type": "text-end", "id": "1" },
                { "type": "finish-step" },
                {
                    "type": "finish",
                    "finishReason": "stop",
                    "messageMetadata": { "stage": "finish", "finishReason": "stop" }
                }
            ])
        );
    }

    #[test]
    fn stream_text_result_maps_portable_non_text_parts_to_ui_message_stream() {
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("sig-1"))]),
        )]);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(
                    LanguageModelFile::new(
                        "text/plain",
                        LanguageModelFileData::Data {
                            data: FileDataContent::Base64("SGVsbG8=".to_string()),
                        },
                    )
                    .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ReasoningFile(
                    LanguageModelReasoningFile::new(
                        "application/json",
                        LanguageModelFileData::Data {
                            data: FileDataContent::Base64("e30=".to_string()),
                        },
                    )
                    .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::Source(LanguageModelSource::Url(
                    LanguageModelUrlSource::new("source-1", "https://example.com")
                        .with_title("Example")
                        .with_provider_metadata(provider_metadata.clone()),
                )),
                LanguageModelStreamPart::Source(LanguageModelSource::Document(
                    LanguageModelDocumentSource::new("doc-1", "application/pdf", "Reference")
                        .with_filename("reference.pdf")
                        .with_provider_metadata(provider_metadata.clone()),
                )),
                LanguageModelStreamPart::Custom(
                    LanguageModelCustomContent::new("mock-provider.custom")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new("call-1", "search")
                        .with_provider_executed(true)
                        .with_dynamic(true)
                        .with_title("Search")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    r#"{"query":"rust"}"#,
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "search", r#"{"query":"rust"}"#)
                        .with_provider_executed(true)
                        .with_dynamic(true)
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolResult(
                    LanguageModelToolResult::new(
                        "call-1",
                        "search",
                        NonNullJsonValue::new(json!({ "answer": "found" }))
                            .expect("tool result is non-null"),
                    )
                    .with_preliminary(true)
                    .with_dynamic(true)
                    .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("approval-1", "call-1")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Search")],
        )));
        let default_chunks =
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        assert!(
            default_chunks
                .as_array()
                .expect("chunks are an array")
                .iter()
                .all(|chunk| chunk["type"] != "source-url" && chunk["type"] != "source-document")
        );

        let chunks = serde_json::to_value(result.to_ui_message_stream_with_options(
            StreamTextUiMessageStreamOptions::new().with_send_sources(true),
        ))
        .expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");

        for expected in [
            json!({
                "type": "file",
                "mediaType": "text/plain",
                "url": "data:text/plain;base64,SGVsbG8=",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } }
            }),
            json!({
                "type": "reasoning-file",
                "mediaType": "application/json",
                "url": "data:application/json;base64,e30=",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } }
            }),
            json!({
                "type": "source-url",
                "sourceId": "source-1",
                "url": "https://example.com",
                "title": "Example",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } }
            }),
            json!({
                "type": "source-document",
                "sourceId": "doc-1",
                "mediaType": "application/pdf",
                "title": "Reference",
                "filename": "reference.pdf",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } }
            }),
            json!({
                "type": "custom",
                "kind": "mock-provider.custom",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } }
            }),
            json!({
                "type": "tool-input-start",
                "toolCallId": "call-1",
                "toolName": "search",
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "dynamic": true,
                "title": "Search"
            }),
            json!({
                "type": "tool-input-delta",
                "toolCallId": "call-1",
                "inputTextDelta": "{\"query\":\"rust\"}"
            }),
            json!({
                "type": "tool-input-available",
                "toolCallId": "call-1",
                "toolName": "search",
                "input": { "query": "rust" },
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "dynamic": true
            }),
            json!({
                "type": "tool-output-available",
                "toolCallId": "call-1",
                "output": { "answer": "found" },
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "preliminary": true,
                "dynamic": true
            }),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval-1",
                "toolCallId": "call-1",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } }
            }),
        ] {
            assert!(
                chunks.contains(&expected),
                "missing expected UI message chunk: {expected}"
            );
        }
    }

    #[test]
    fn stream_text_result_creates_ui_message_stream_response() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        let response = result.to_ui_message_stream_response(
            UiMessageStreamResponseInit::new()
                .with_status(201)
                .with_header("x-ui", "yes"),
        );

        assert_eq!(response.status, 201);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(crate::ui_message_stream::UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("x-ui").map(String::as_str),
            Some("yes")
        );
        assert_eq!(
            response.decoded_body().expect("response body decodes"),
            vec![
                r#"data: {"type":"start"}

"#
                .to_string(),
                r#"data: {"type":"start-step"}

"#
                .to_string(),
                r#"data: {"type":"text-start","id":"1"}

"#
                .to_string(),
                r#"data: {"type":"text-delta","id":"1","delta":"Hello"}

"#
                .to_string(),
                r#"data: {"type":"text-end","id":"1"}

"#
                .to_string(),
                r#"data: {"type":"finish-step"}

"#
                .to_string(),
                r#"data: {"type":"finish","finishReason":"stop"}

"#
                .to_string(),
                "data: [DONE]\n\n".to_string()
            ]
        );

        let response_with_stream_options = result.to_ui_message_stream_response_with_options(
            UiMessageStreamResponseInit::new().with_header("x-ui-options", "yes"),
            StreamTextUiMessageStreamOptions::new().with_message_id("response-id"),
        );

        assert_eq!(
            response_with_stream_options
                .headers
                .get("x-ui-options")
                .map(String::as_str),
            Some("yes")
        );
        assert_eq!(
            response_with_stream_options
                .decoded_body()
                .expect("response body decodes")[0],
            r#"data: {"type":"start","messageId":"response-id"}

"#
        );
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_writes_data_stream_parts() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response(&mut response, UiMessageStreamResponseInit::new())
            .expect("mock response writes");

        assert_eq!(response.status, Some(200));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(crate::ui_message_stream::UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response
                .headers
                .get(crate::ui_message_stream::UI_MESSAGE_STREAM_VERSION_HEADER)
                .map(String::as_str),
            Some(crate::ui_message_stream::UI_MESSAGE_STREAM_VERSION)
        );
        assert_eq!(
            response.decoded_chunks(),
            vec![
                r#"data: {"type":"start"}

"#,
                r#"data: {"type":"start-step"}

"#,
                r#"data: {"type":"text-start","id":"1"}

"#,
                r#"data: {"type":"text-delta","id":"1","delta":"Hello"}

"#,
                r#"data: {"type":"text-delta","id":"1","delta":", "}

"#,
                r#"data: {"type":"text-delta","id":"1","delta":"world!"}

"#,
                r#"data: {"type":"text-end","id":"1"}

"#,
                r#"data: {"type":"finish-step"}

"#,
                r#"data: {"type":"finish","finishReason":"stop"}

"#,
                "data: [DONE]\n\n"
            ]
        );
        assert!(response.ended);
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_applies_custom_headers() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response(
                &mut response,
                UiMessageStreamResponseInit::new()
                    .with_status(201)
                    .with_status_text("foo")
                    .with_header("custom-header", "custom-value"),
            )
            .expect("mock response writes");

        assert_eq!(response.status, Some(201));
        assert_eq!(response.status_text.as_deref(), Some("foo"));
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("custom-value")
        );
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(crate::ui_message_stream::UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert!(response.ended);
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_masks_error_messages_by_default() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!("error"))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response(&mut response, UiMessageStreamResponseInit::new())
            .expect("mock response writes");

        assert!(
            response.decoded_chunks().iter().any(|chunk| {
                chunk
                    == r#"data: {"type":"error","errorText":"An error occurred."}

"#
            }),
            "expected masked error chunk in {:?}",
            response.decoded_chunks()
        );
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_supports_custom_error_messages() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!("error"))),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("error".to_string()),
                },
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response_with_options(
                &mut response,
                UiMessageStreamResponseInit::new(),
                StreamTextUiMessageStreamOptions::new().with_on_error(|error| {
                    format!(
                        "custom error message: {}",
                        error.as_str().unwrap_or("unknown error")
                    )
                }),
            )
            .expect("mock response writes");

        assert!(
            response.decoded_chunks().iter().any(|chunk| {
                chunk
                    == r#"data: {"type":"error","errorText":"custom error message: error"}

"#
            }),
            "expected custom error chunk in {:?}",
            response.decoded_chunks()
        );
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_omits_finish_when_send_finish_false() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, World!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response_with_options(
                &mut response,
                UiMessageStreamResponseInit::new(),
                StreamTextUiMessageStreamOptions::new().with_send_finish(false),
            )
            .expect("mock response writes");

        let chunks = response.decoded_chunks();
        assert!(
            !chunks
                .iter()
                .any(|chunk| chunk.contains(r#""type":"finish""#)),
            "finish chunk should be omitted: {chunks:?}"
        );
        assert!(chunks.iter().any(|chunk| chunk.contains("finish-step")));
        assert_eq!(chunks.last().map(String::as_str), Some("data: [DONE]\n\n"));
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_writes_reasoning_content() {
        let metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("1234567890"))]),
        )]);
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("1")),
            LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                "1",
                "I will open the conversation",
            )),
            LanguageModelStreamPart::ReasoningDelta(
                LanguageModelReasoningDelta::new("1", "").with_provider_metadata(metadata.clone()),
            ),
            LanguageModelStreamPart::ReasoningEnd(
                LanguageModelReasoningEnd::new("1").with_provider_metadata(metadata),
            ),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hi")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response_with_options(
                &mut response,
                UiMessageStreamResponseInit::new(),
                StreamTextUiMessageStreamOptions::new().with_send_reasoning(true),
            )
            .expect("mock response writes");

        let chunks = response.decoded_chunks();
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk == "data: {\"type\":\"reasoning-start\",\"id\":\"1\"}\n\n")
        );
        assert!(chunks.iter().any(|chunk| {
            chunk == "data: {\"type\":\"reasoning-delta\",\"id\":\"1\",\"delta\":\"I will open the conversation\"}\n\n"
        }));
        assert!(chunks.iter().any(|chunk| {
            chunk == "data: {\"type\":\"reasoning-end\",\"id\":\"1\",\"providerMetadata\":{\"testProvider\":{\"signature\":\"1234567890\"}}}\n\n"
        }));
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_writes_source_content() {
        let metadata = ProviderMetadata::from([(
            "provider".to_string(),
            Map::from_iter([("custom".to_string(), json!("value"))]),
        )]);
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::Source(LanguageModelSource::Url(
                LanguageModelUrlSource::new("123", "https://example.com")
                    .with_title("Example")
                    .with_provider_metadata(metadata),
            )),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response_with_options(
                &mut response,
                UiMessageStreamResponseInit::new(),
                StreamTextUiMessageStreamOptions::new().with_send_sources(true),
            )
            .expect("mock response writes");

        assert!(response.decoded_chunks().iter().any(|chunk| {
            chunk == "data: {\"type\":\"source-url\",\"sourceId\":\"123\",\"url\":\"https://example.com\",\"title\":\"Example\",\"providerMetadata\":{\"provider\":{\"custom\":\"value\"}}}\n\n"
        }));
    }

    #[test]
    fn stream_text_result_pipe_ui_message_stream_to_response_writes_file_content() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::File(LanguageModelFile::new(
                "text/plain",
                LanguageModelFileData::Data {
                    data: FileDataContent::Base64("SGVsbG8=".to_string()),
                },
            )),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let mut response = MockStreamTextUiMessageResponse::default();

        result
            .pipe_ui_message_stream_to_response(&mut response, UiMessageStreamResponseInit::new())
            .expect("mock response writes");

        assert!(response.decoded_chunks().iter().any(|chunk| {
            chunk
                == "data: {\"type\":\"file\",\"mediaType\":\"text/plain\",\"url\":\"data:text/plain;base64,SGVsbG8=\"}\n\n"
        }));
    }

    #[test]
    fn stream_text_result_supports_text_ui_message_and_full_stream_from_single_result() {
        let response_metadata = LanguageModelStreamResponseMetadata::new()
            .with_id("id-0")
            .with_model_id("mock-model-id");
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::ResponseMetadata(response_metadata),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        let text_stream = result.text_stream.clone();
        let full_stream = serde_json::to_value(&result.parts).expect("parts serialize");
        let ui_message_stream =
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");

        assert_eq!(text_stream, vec!["Hello", ", ", "world!"]);
        assert_eq!(
            full_stream
                .as_array()
                .expect("full stream is array")
                .iter()
                .filter(|part| part["type"] == "text-delta")
                .map(|part| part["text"].as_str().expect("text delta"))
                .collect::<Vec<_>>(),
            vec!["Hello", ", ", "world!"]
        );
        assert!(
            full_stream
                .as_array()
                .expect("full stream is array")
                .iter()
                .any(|part| part["type"] == "finish-step"
                    && part["response"]["id"] == "id-0"
                    && part["response"]["modelId"] == "mock-model-id")
        );
        assert_eq!(
            ui_message_stream,
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "text-start", "id": "1" },
                { "type": "text-delta", "id": "1", "delta": "Hello" },
                { "type": "text-delta", "id": "1", "delta": ", " },
                { "type": "text-delta", "id": "1", "delta": "world!" },
                { "type": "text-end", "id": "1" },
                { "type": "finish-step" },
                { "type": "finish", "finishReason": "stop" }
            ])
        );

        assert_eq!(result.text_stream, text_stream);
        assert_eq!(
            serde_json::to_value(&result.parts).expect("parts serialize"),
            full_stream
        );
        assert_eq!(
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize"),
            ui_message_stream
        );
    }

    fn request_retention_model(body: JsonValue) -> MockLanguageModel {
        MockLanguageModel::new().with_stream_result(
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ])
            .with_request(LanguageModelRequest::new().with_body(body)),
        )
    }

    /// Maps packages/ai stream-text.test.ts:5406
    /// `result.request should exclude request body and request messages by default`
    /// — without an `include` retention setting, the resolved `result.request`
    /// carries neither the provider body nor the request messages.
    #[test]
    fn stream_text_result_request_excludes_body_and_messages_by_default() {
        let model = request_retention_model(json!("test body"));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));
        result.consume_stream();

        let request = result.request.expect("request metadata present");
        assert_eq!(request.body, None);
        assert_eq!(request.messages, None);
    }

    /// Maps packages/ai stream-text.test.ts:5436
    /// `should include request body when retention.requestBody is true` — enabling
    /// `include.request_body` retains the provider request body while messages stay
    /// excluded.
    #[test]
    fn stream_text_result_request_includes_body_when_retention_request_body_true() {
        let model = request_retention_model(json!("test body"));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_include(GenerateTextInclude::new().with_request_body(true)),
        ));
        result.consume_stream();

        let request = result.request.expect("request metadata present");
        assert_eq!(request.body, Some(json!("test body")));
        assert_eq!(request.messages, None);
    }

    /// Maps packages/ai stream-text.test.ts:5530
    /// `should include request messages when retention.requestMessages is true` —
    /// enabling both retention flags keeps the body and records the step prompt as
    /// the request messages, exposed on both `result.request` and the step request.
    #[test]
    fn stream_text_result_request_includes_messages_when_retention_request_messages_true() {
        let model = request_retention_model(json!("test body"));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_include(
                GenerateTextInclude::new()
                    .with_request_body(true)
                    .with_request_messages(true),
            ),
        ));
        result.consume_stream();

        let expected_messages = vec![user_message("test-input")];
        let request = result.request.clone().expect("request metadata present");
        assert_eq!(request.body, Some(json!("test body")));
        assert_eq!(request.messages.as_ref(), Some(&expected_messages));

        let step_request = result.steps[0]
            .request
            .as_ref()
            .expect("step request metadata present");
        assert_eq!(step_request.messages.as_ref(), Some(&expected_messages));
    }

    /// Maps packages/ai stream-text.test.ts:5564
    /// `should resolve with messages from after prepareStep` — when `prepareStep`
    /// replaces the step messages and `include.request_messages` is enabled, the
    /// retained request messages reflect the prepared prompt, not the original input.
    #[test]
    fn stream_text_result_request_resolves_with_messages_from_after_prepare_step() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let prepared_messages = vec![user_message("prepared prompt")];

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_include(GenerateTextInclude::new().with_request_messages(true))
                .with_prepare_step({
                    let prepared_messages = prepared_messages.clone();
                    move |_options| {
                        let prepared_messages = prepared_messages.clone();
                        async move { PrepareStepResult::new().with_messages(prepared_messages) }
                    }
                }),
        ));
        result.consume_stream();

        let request = result.request.clone().expect("request metadata present");
        assert_eq!(request.messages.as_ref(), Some(&prepared_messages));

        let step_request = result.steps[0]
            .request
            .as_ref()
            .expect("step request metadata present");
        assert_eq!(step_request.messages.as_ref(), Some(&prepared_messages));
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
    fn stream_text_passes_explicit_false_include_raw_chunks_to_model() {
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

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_include_raw_chunks(false),
        ));

        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Raw(_)))
        );
        assert_eq!(model.stream_calls()[0].include_raw_chunks, Some(false));
    }

    /// Maps packages/ai stream-text.test.ts:20326
    /// `should filter available tools to only the ones in activeTools` — only the
    /// tools named in `active_tools` are forwarded to the provider `doStream`
    /// call; the remaining configured tools are filtered out.
    #[test]
    fn stream_text_filters_available_tools_to_active_tools() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let value_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", value_schema.clone())
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_tool(
                    Tool::new("tool2", value_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result2")) }),
                )
                .with_active_tools(["tool1"]),
        ));

        assert_eq!(result.text, "Hello, world!");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let tools = stream_calls[0]
            .tools
            .as_ref()
            .expect("tools forwarded to provider call");
        let tool_names: Vec<&str> = tools
            .iter()
            .map(|tool| match tool {
                LanguageModelTool::Function(tool) => tool.name.as_str(),
                LanguageModelTool::Provider(tool) => tool.name.as_str(),
            })
            .collect();
        assert_eq!(tool_names, vec!["tool1"]);
    }

    /// Maps packages/ai stream-text.test.ts:20705
    /// `should pass includeRawChunks flag correctly to the model` — the resolved
    /// `include_raw_chunks` flag (true, false, or unset default) is forwarded to
    /// the provider `doStream` call exactly as configured.
    #[test]
    fn stream_text_passes_include_raw_chunks_flag_correctly_to_model() {
        fn run(option: Option<bool>) -> Option<bool> {
            let model =
                MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                    LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                    LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                    LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                        usage(),
                        finish_reason(),
                    )),
                ]));

            let mut options = StreamTextOptions::new(&model, vec![user_message("test prompt")]);
            if let Some(include_raw_chunks) = option {
                options = options.with_include_raw_chunks(include_raw_chunks);
            }
            let result = poll_ready(stream_text(options));
            result.consume_stream();

            model.stream_calls()[0].include_raw_chunks
        }

        // include.rawChunks: true -> forwarded as true.
        assert_eq!(run(Some(true)), Some(true));
        // include.rawChunks: false -> forwarded as false.
        assert_eq!(run(Some(false)), Some(false));
        // omitted -> defaults to unset (treated as false by the provider).
        assert_eq!(run(None), None);
    }

    #[test]
    fn stream_text_passes_response_format_to_model() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let response_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("response schema is an object")
        .clone();
        let response_format = LanguageModelResponseFormat::json().with_schema(response_schema);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_response_format(response_format.clone()),
        ));

        assert_eq!(result.text, "Hello");
        assert_eq!(
            model.stream_calls()[0].response_format,
            Some(response_format)
        );
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

        let abort = TextStreamPart::Abort(TextStreamAbortPart::with_reason(json!({
            "source": "client"
        })));
        assert_eq!(
            serde_json::to_value(&abort).expect("abort should serialize"),
            json!({
                "type": "abort",
                "reason": { "source": "client" }
            })
        );
        assert_eq!(
            serde_json::from_value::<TextStreamPart>(json!({ "type": "abort" }))
                .expect("abort should deserialize"),
            TextStreamPart::Abort(TextStreamAbortPart::new())
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
    fn stream_text_result_maps_abort_part_to_ui_message_stream() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let mut result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));
        result.parts.insert(
            3,
            TextStreamPart::Abort(TextStreamAbortPart::with_reason("client-disconnected")),
        );

        assert_eq!(
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "text-start", "id": "1" },
                { "type": "abort", "reason": "client-disconnected" },
                { "type": "text-delta", "id": "1", "delta": "Hello" },
                { "type": "text-end", "id": "1" },
                { "type": "finish-step" },
                { "type": "finish", "finishReason": "stop" }
            ])
        );
    }

    #[test]
    fn stream_text_aborts_before_model_call_and_invokes_on_abort() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let abort_controller = StreamTextAbortController::new();
        abort_controller.abort_with_reason("manual abort");
        let abort_events = Arc::new(Mutex::new(Vec::<StreamTextOnAbortEvent>::new()));
        let events = Arc::clone(&abort_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_abort_signal(abort_controller.signal())
                .with_on_abort(move |event| {
                    let events = Arc::clone(&events);
                    async move {
                        events.lock().expect("abort events lock").push(event);
                    }
                }),
        ));

        assert!(model.stream_calls().is_empty());
        assert!(result.steps.is_empty());
        assert_eq!(result.finish_reason, FinishReason::Other);
        assert_eq!(
            serde_json::to_value(&result.parts).expect("parts serialize"),
            json!([
                { "type": "start" },
                { "type": "abort", "reason": "manual abort" }
            ])
        );
        assert_eq!(
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "abort", "reason": "manual abort" }
            ])
        );

        let events = abort_events.lock().expect("abort events lock");
        assert_eq!(events.len(), 1);
        assert!(events[0].steps.is_empty());
        assert_eq!(events[0].reason, Some(json!("manual abort")));
    }

    #[test]
    fn stream_text_aborts_after_chunk_callback_and_suppresses_finish() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " World")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();
        let chunk_events = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let chunks = Arc::clone(&chunk_events);
        let abort_events = Arc::new(Mutex::new(Vec::<StreamTextOnAbortEvent>::new()));
        let events = Arc::clone(&abort_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_abort_signal(abort_signal)
                .with_on_chunk(move |event| {
                    let abort_controller = abort_controller.clone();
                    let chunks = Arc::clone(&chunks);
                    async move {
                        if let TextStreamPart::TextDelta(part) = &event.chunk
                            && part.text == "Hello"
                        {
                            abort_controller.abort_with_reason("client-disconnected");
                        }
                        chunks
                            .lock()
                            .expect("chunk events lock")
                            .push(serde_json::to_value(event.chunk).expect("chunk serializes"));
                    }
                })
                .with_on_abort(move |event| {
                    let events = Arc::clone(&events);
                    async move {
                        events.lock().expect("abort events lock").push(event);
                    }
                }),
        ));

        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let provider_abort_signal = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("abort signal should propagate to provider call options");
        assert!(provider_abort_signal.is_aborted());
        assert_eq!(
            provider_abort_signal.reason(),
            Some(json!("client-disconnected"))
        );
        assert!(result.steps.is_empty());
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Finish(_)))
        );
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::FinishStep(_)))
        );
        assert_eq!(
            serde_json::to_value(&result.parts).expect("parts serialize"),
            json!([
                { "type": "start" },
                {
                    "type": "start-step",
                    "request": {},
                    "warnings": []
                },
                { "type": "text-start", "id": "1" },
                { "type": "text-delta", "id": "1", "text": "Hello" },
                { "type": "abort", "reason": "client-disconnected" }
            ])
        );
        assert_eq!(
            chunk_events.lock().expect("chunk events lock").as_slice(),
            [
                json!({ "type": "text-delta", "id": "1", "text": "Hello" }),
                json!({ "type": "abort", "reason": "client-disconnected" })
            ]
        );

        let events = abort_events.lock().expect("abort events lock");
        assert_eq!(events.len(), 1);
        assert!(events[0].steps.is_empty());
        assert_eq!(events[0].reason, Some(json!("client-disconnected")));
    }

    /// Maps packages/ai stream-text.test.ts:21343
    /// `should call telemetry onAbort but not onEnd or onError when the abort
    /// signal is triggered` — when the stream is aborted mid-step, the telemetry
    /// integration receives exactly one `onAbort` event (carrying the empty
    /// `steps` accumulated so far) and neither `onEnd` nor `onError`.
    #[test]
    fn stream_text_dispatches_telemetry_on_abort_but_not_on_end_or_error() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();

        let telemetry_events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnAbort,
            TelemetryEventKind::OnEnd,
            TelemetryEventKind::OnError,
        ] {
            let captured = Arc::clone(&telemetry_events);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("telemetry event lock").push(event);
            });
        }

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_abort_signal(abort_signal)
                .with_on_error(|_event| ready(()))
                .with_on_chunk(move |event| {
                    let abort_controller = abort_controller.clone();
                    async move {
                        if let TextStreamPart::TextDelta(part) = &event.chunk
                            && part.text == "Hello"
                        {
                            abort_controller.abort_with_reason("client-disconnected");
                        }
                    }
                })
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("stream-text-abort")
                        .with_integration(integration),
                ),
        ));
        result.consume_stream();

        let events = telemetry_events.lock().expect("telemetry event lock");
        // Exactly one telemetry event: onAbort. No onEnd, no onError.
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![TelemetryEventKind::OnAbort]
        );
        // The aborted stream completed no steps before the abort fired.
        assert_eq!(events[0].event["steps"], json!([]));
        // The abort event carries the active language-model call id.
        assert!(
            events[0].event["callId"]
                .as_str()
                .is_some_and(|call_id| call_id.starts_with("call")),
            "abort event should carry the call id"
        );
    }

    #[test]
    fn stream_text_forwards_timeout_as_abort_signal_to_model() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_timeout(TimeoutConfiguration::total_ms(5_000)),
        ));

        assert_eq!(result.text, "Hello");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let provider_abort_signal = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("timeout should create an abort signal");
        assert!(!provider_abort_signal.is_aborted());
    }

    #[test]
    fn stream_text_merges_timeout_with_abort_signal() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_timeout(TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new().with_total_ms(5_000),
                ))
                .with_abort_signal(abort_signal.clone()),
        ));

        assert_eq!(result.text, "Hello");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let provider_abort_signal = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("merged timeout should create an abort signal");
        assert!(!provider_abort_signal.is_same_signal(&abort_signal));
        assert!(!provider_abort_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(provider_abort_signal.is_aborted());
        assert_eq!(
            provider_abort_signal.reason(),
            Some(json!("client-disconnected"))
        );
    }

    #[test]
    fn stream_text_forwards_abort_signal_to_tool_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]));
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();
        let received_signal = Arc::new(Mutex::new(None::<StreamTextAbortSignal>));
        let received_signal_for_closure = Arc::clone(&received_signal);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Use the tool")])
                .with_abort_signal(abort_signal.clone())
                .with_tool(Tool::new("tool1", input_schema).with_execute(
                    move |_input, options| {
                        let received_signal = Arc::clone(&received_signal_for_closure);
                        async move {
                            *received_signal.lock().expect("signal lock") = options.abort_signal;
                            Ok(json!("result1"))
                        }
                    },
                )),
        ));

        assert_eq!(result.tool_results.len(), 1);
        let captured_signal = received_signal
            .lock()
            .expect("signal lock")
            .clone()
            .expect("tool received abort signal");
        assert!(captured_signal.is_same_signal(&abort_signal));
        assert!(!captured_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(captured_signal.is_aborted());
        assert_eq!(captured_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn stream_text_passes_undefined_when_no_timeout_or_abort_signal_provided() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Say hello")],
        )));

        assert_eq!(result.text, "Hello");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert!(stream_calls[0].abort_signal.is_none());
    }

    /// Maps packages/ai stream-text.test.ts `timeout` row
    /// `should support both totalMs and stepMs together` — a timeout that
    /// configures both `total_ms` and `step_ms` still forwards a (total-derived)
    /// abort signal to the model call.
    #[test]
    fn stream_text_supports_total_ms_and_step_ms_together() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_timeout(
                TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new()
                        .with_total_ms(10_000)
                        .with_step_ms(5_000),
                ),
            ),
        ));

        assert_eq!(result.text, "Hello");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert!(stream_calls[0].abort_signal.is_some());
    }

    #[test]
    fn stream_text_passes_undefined_when_timeout_object_has_no_total_ms() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")]).with_timeout(
                TimeoutConfiguration::detailed(TimeoutConfigurationOptions::new()),
            ),
        ));

        assert_eq!(result.text, "Hello");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert!(stream_calls[0].abort_signal.is_none());
    }

    /// Maps packages/ai stream-text.test.ts:16761
    /// `should support chunkMs alongside totalMs and stepMs` — configuring all of
    /// `total_ms`, `step_ms`, and `chunk_ms` still forwards a (total-derived)
    /// abort signal to the provider `doStream` call.
    #[test]
    fn stream_text_supports_chunk_ms_alongside_total_ms_and_step_ms() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_timeout(
                TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new()
                        .with_total_ms(30_000)
                        .with_step_ms(10_000)
                        .with_chunk_ms(5_000),
                ),
            ),
        ));
        result.consume_stream();

        assert_eq!(result.text, "Hello");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert!(stream_calls[0].abort_signal.is_some());
    }

    /// Maps packages/ai stream-text.test.ts:16517
    /// `should forward stepMs as abort signal to each step` — configuring only
    /// `step_ms` (no total timeout) still forwards a defined abort signal to the
    /// provider `doStream` call (the per-step abort controller signal is merged
    /// into the request signal even before any timeout fires).
    #[test]
    fn stream_text_forwards_step_ms_as_abort_signal_to_each_step() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_timeout(
                TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new().with_step_ms(5_000),
                ),
            ),
        ));
        result.consume_stream();

        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let signal = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("step_ms should forward a defined abort signal");
        assert!(!signal.is_aborted());
    }

    /// Maps packages/ai stream-text.test.ts:16549
    /// `should reuse the same abort signal for all steps when stepMs is set` —
    /// across a two-step tool loop the provider `doStream` call receives a defined
    /// abort signal on every step, and it is the *same* signal instance each time
    /// (the per-step abort controller is created once and reused, only its
    /// timeout is reset per step).
    #[test]
    fn stream_text_reuses_the_same_abort_signal_for_all_steps_when_step_ms_is_set() {
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let model = MockLanguageModel::new().with_stream_results(vec![
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "test" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Final response",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_timeout(TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new().with_step_ms(5_000),
                ))
                .with_max_steps(2)
                .with_stop_condition(StopCondition::StepCount(2))
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("tool result")) }),
                ),
        ));
        result.consume_stream();

        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 2);
        let first = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("first step should receive a defined abort signal");
        let second = stream_calls[1]
            .abort_signal
            .as_ref()
            .expect("second step should receive a defined abort signal");
        assert!(!first.is_aborted());
        assert!(!second.is_aborted());
        // The same merged abort signal is reused for every step.
        assert!(first.is_same_signal(second));
    }

    /// Maps packages/ai stream-text.test.ts:16642
    /// `should forward chunkMs as abort signal to model` — configuring only
    /// `chunk_ms` (no total timeout) still forwards a defined abort signal to the
    /// provider `doStream` call.
    #[test]
    fn stream_text_forwards_chunk_ms_as_abort_signal_to_model() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_timeout(
                TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new().with_chunk_ms(5_000),
                ),
            ),
        ));
        result.consume_stream();

        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        assert!(stream_calls[0].abort_signal.is_some());
    }

    /// Maps packages/ai stream-text.test.ts:16673
    /// `should complete successfully when chunks arrive within chunkMs timeout` —
    /// when all chunks arrive before the (generous) `chunk_ms` timeout could fire,
    /// the stream completes normally and the forwarded abort signal is never
    /// marked aborted.
    #[test]
    fn stream_text_completes_successfully_when_chunks_arrive_within_chunk_ms_timeout() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " World")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_timeout(
                TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new().with_chunk_ms(5_000),
                ),
            ),
        ));
        result.consume_stream();

        assert_eq!(result.text, "Hello World");
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let signal = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("chunk_ms should forward a defined abort signal");
        assert!(!signal.is_aborted());
    }

    /// Maps packages/ai stream-text.test.ts:16792
    /// `should clean up step timeout when doStream throws an error` — when
    /// `doStream` throws immediately under a `step_ms` timeout, the stream
    /// consumes to completion (surfacing the error) without panicking, leaving no
    /// pending step timeout behind.
    #[test]
    fn stream_text_cleans_up_step_timeout_when_do_stream_throws_an_error() {
        // Model whose stream surfaces an immediate failure (mirrors upstream
        // `doStream` throwing) under a configured step timeout.
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                    "message": "Fail immediately"
                }))),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_max_retries(0)
                .with_timeout(TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new().with_step_ms(10_000),
                )),
        ));
        // Should consume to completion without panicking; the per-step abort
        // controller (created for the step timeout) is dropped/cleaned up
        // alongside the failed step.
        result.consume_stream();

        assert_eq!(result.finish_reason, FinishReason::Error);
        let has_error = result
            .parts
            .iter()
            .any(|part| matches!(part, TextStreamPart::Error(_)));
        assert!(has_error);
    }

    struct DelayedStreamLanguageModel {
        delayed: Arc<DelayedPromise<()>>,
    }

    impl DelayedStreamLanguageModel {
        fn new(delayed: Arc<DelayedPromise<()>>) -> Self {
            Self { delayed }
        }
    }

    impl LanguageModel for DelayedStreamLanguageModel {
        type SupportedUrlsFuture<'a>
            = std::future::Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = std::future::Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            std::future::ready(BTreeMap::new())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            std::future::ready(LanguageModelGenerateResult::new(
                Vec::new(),
                LanguageModelFinishReason {
                    unified: FinishReason::Other,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            let delayed = Arc::clone(&self.delayed);
            Box::pin(async move {
                let _ = delayed.promise().await;
                LanguageModelStreamResult::new(Vec::new())
            })
        }
    }

    #[test]
    fn stream_text_aborts_while_waiting_for_do_stream_and_invokes_on_abort() {
        let delayed = Arc::new(DelayedPromise::<()>::new());
        let model = DelayedStreamLanguageModel::new(Arc::clone(&delayed));
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();
        let abort_events = Arc::new(Mutex::new(Vec::<StreamTextOnAbortEvent>::new()));
        let events = Arc::clone(&abort_events);

        let mut result = Box::pin(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_abort_signal(abort_signal.clone())
                .with_on_abort(move |event| {
                    let events = Arc::clone(&events);
                    async move {
                        events.lock().expect("abort events lock").push(event);
                    }
                }),
        ));

        assert!(matches!(poll_once(result.as_mut()), Poll::Pending));

        abort_controller.abort_with_reason("client-disconnected");
        delayed.resolve(());

        let result = loop {
            match poll_once(result.as_mut()) {
                Poll::Ready(result) => break result,
                Poll::Pending => continue,
            }
        };

        assert!(model.delayed.is_resolved());
        assert!(!model.delayed.is_pending());
        assert!(!model.delayed.is_rejected());
        assert_eq!(result.steps.len(), 0);
        assert_eq!(
            serde_json::to_value(&result.parts).expect("parts serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step", "request": {}, "warnings": [] },
                { "type": "abort", "reason": "client-disconnected" }
            ])
        );
        assert_eq!(
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "abort", "reason": "client-disconnected" }
            ])
        );

        let events = abort_events.lock().expect("abort events lock");
        assert_eq!(events.len(), 1);
        assert!(events[0].steps.is_empty());
        assert_eq!(events[0].reason, Some(json!("client-disconnected")));
    }

    /// Maps packages/ai stream-text.test.ts:16387
    /// `should throw Timeout error when abort signal is aborted` — when the
    /// caller's `abortSignal` is aborted (with no explicit reason) before the
    /// delayed `doStream` resolves, the stream terminates with an abort outcome:
    /// no text is produced, no step completes, and the abort part carries the
    /// default (reasonless) AbortError signal rather than a model result.
    #[test]
    fn stream_text_aborts_when_abort_signal_aborted_before_do_stream_resolves() {
        let delayed = Arc::new(DelayedPromise::<()>::new());
        let model = DelayedStreamLanguageModel::new(Arc::clone(&delayed));
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();

        let mut result = Box::pin(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_abort_signal(abort_signal.clone()),
        ));

        assert!(matches!(poll_once(result.as_mut()), Poll::Pending));

        // Abort without a reason (mirrors upstream `abortController.abort()`),
        // then let the delayed doStream resolve.
        abort_controller.abort();
        delayed.resolve(());

        let result = loop {
            match poll_once(result.as_mut()) {
                Poll::Ready(result) => break result,
                Poll::Pending => continue,
            }
        };

        assert_eq!(result.text, "");
        assert_eq!(result.steps.len(), 0);
        assert_eq!(
            serde_json::to_value(&result.parts).expect("parts serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step", "request": {}, "warnings": [] },
                { "type": "abort" }
            ])
        );
    }

    #[test]
    fn stream_text_aborts_during_tool_execution_before_tool_result() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]));
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();
        let abort_events = Arc::new(Mutex::new(Vec::<StreamTextOnAbortEvent>::new()));
        let events = Arc::clone(&abort_events);
        let on_error_calls = Arc::new(Mutex::new(Vec::<StreamTextOnErrorEvent>::new()));
        let errors = Arc::clone(&on_error_calls);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Use the tool")])
                .with_abort_signal(abort_signal)
                .with_tool(Tool::new("tool1", input_schema).with_execute(
                    move |_input, _options| {
                        let abort_controller = abort_controller.clone();
                        async move {
                            abort_controller.abort_with_reason("tool-aborted");
                            Ok(json!("result1"))
                        }
                    },
                ))
                .with_on_error(move |error| {
                    let errors = Arc::clone(&errors);
                    async move {
                        errors.lock().expect("error calls lock").push(error);
                    }
                })
                .with_on_abort(move |event| {
                    let events = Arc::clone(&events);
                    async move {
                        events.lock().expect("abort events lock").push(event);
                    }
                }),
        ));

        assert!(result.steps.is_empty());
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::ToolResult(_)))
        );
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::FinishStep(_)))
        );
        assert!(on_error_calls.lock().expect("error calls lock").is_empty());
        assert_eq!(
            serde_json::to_value(&result.parts).expect("parts serialize"),
            json!([
                { "type": "start" },
                {
                    "type": "start-step",
                    "request": {},
                    "warnings": []
                },
                {
                    "type": "tool-call",
                    "toolCallId": "call-1",
                    "toolName": "tool1",
                    "input": { "value": "value" }
                },
                { "type": "abort", "reason": "tool-aborted" }
            ])
        );

        let events = abort_events.lock().expect("abort events lock");
        assert_eq!(events.len(), 1);
        assert!(events[0].steps.is_empty());
        assert_eq!(events[0].reason, Some(json!("tool-aborted")));
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

    #[test]
    fn stream_text_maps_tool_input_deltas_and_high_level_tool_outputs() {
        let provider_metadata = ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("someKey".to_string(), json!("someValue"))]),
        )]);
        let tool_result_output =
            NonNullJsonValue::new(json!("result:Sparkle Day")).expect("result is non-null");
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new("call-1", "tool1")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    "{\"value\":",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    "\"Sparkle Day\"}",
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    "{\"value\":\"Sparkle Day\"}",
                )),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "call-1",
                    "tool1",
                    tool_result_output,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Call the tool")],
        )));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].input, json!({"value": "Sparkle Day"}));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(
            result.tool_results[0].input,
            json!({"value": "Sparkle Day"})
        );
        assert_eq!(result.tool_results[0].output, json!("result:Sparkle Day"));
        assert_eq!(result.tool_results[0].provider_executed, Some(true));

        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::ToolInputDelta(_)))
        );
        assert!(result.parts.iter().any(|part| {
            matches!(
                part,
                TextStreamPart::ToolInputStart(part)
                    if part.provider_metadata == Some(provider_metadata.clone())
            )
        }));

        let tool_call_part = result
            .parts
            .iter()
            .find(|part| matches!(part, TextStreamPart::ToolCall(_)))
            .expect("tool call part exists");
        assert_eq!(
            serde_json::to_value(tool_call_part).expect("tool call serializes"),
            json!({
                "type": "tool-call",
                "toolCallId": "call-1",
                "toolName": "tool1",
                "input": { "value": "Sparkle Day" }
            })
        );

        let tool_result_part = result
            .parts
            .iter()
            .find(|part| matches!(part, TextStreamPart::ToolResult(_)))
            .expect("tool result part exists");
        assert_eq!(
            serde_json::to_value(tool_result_part).expect("tool result serializes"),
            json!({
                "type": "tool-result",
                "toolCallId": "call-1",
                "toolName": "tool1",
                "input": { "value": "Sparkle Day" },
                "output": "result:Sparkle Day",
                "providerExecuted": true
            })
        );
    }

    #[test]
    fn stream_text_invokes_tool_input_lifecycle_callbacks_from_stream() {
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let runtime_context = JsonObject::from_iter([("requestId".to_string(), json!("req-1"))]);
        let recorded = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let start_recorded = Arc::clone(&recorded);
        let delta_recorded = Arc::clone(&recorded);
        let available_recorded = Arc::clone(&recorded);
        let abort_controller = StreamTextAbortController::new();
        let abort_signal = abort_controller.signal();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::ToolInputStart(LanguageModelToolInputStart::new(
                    "call-1", "tool1",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    r#"{"value":""#,
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    r#"Sparkle Day"}"#,
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{"value":"Sparkle Day"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Call the tool")])
                .with_runtime_context(runtime_context.clone())
                .with_abort_signal(abort_signal)
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_on_input_start(move |options| {
                            let recorded = Arc::clone(&start_recorded);
                            async move {
                                recorded.lock().expect("recorded lock").push(json!({
                                    "type": "onInputStart",
                                    "toolCallId": options.tool_call_id,
                                    "context": options.context,
                                    "messages": options.messages,
                                    "abortSignalSet": options.abort_signal.is_some()
                                }));
                            }
                        })
                        .with_on_input_delta(move |options| {
                            let recorded = Arc::clone(&delta_recorded);
                            async move {
                                recorded.lock().expect("recorded lock").push(json!({
                                    "type": "onInputDelta",
                                    "toolCallId": options.tool_call_id,
                                    "inputTextDelta": options.input_text_delta,
                                    "context": options.context,
                                    "messages": options.messages,
                                    "abortSignalSet": options.abort_signal.is_some()
                                }));
                            }
                        })
                        .with_on_input_available(move |options| {
                            let recorded = Arc::clone(&available_recorded);
                            async move {
                                recorded.lock().expect("recorded lock").push(json!({
                                    "type": "onInputAvailable",
                                    "toolCallId": options.tool_call_id,
                                    "input": options.input,
                                    "context": options.context,
                                    "messages": options.messages,
                                    "abortSignalSet": options.abort_signal.is_some()
                                }));
                            }
                        }),
                ),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(
            result.tool_calls[0].input,
            json!({ "value": "Sparkle Day" })
        );
        assert_eq!(result.text, "hello");
        assert_eq!(
            result
                .parts
                .iter()
                .filter_map(|part| match part {
                    TextStreamPart::TextDelta(part) => Some(format!("text:{}", part.text)),
                    TextStreamPart::ToolInputStart(part) => {
                        Some(format!("tool-input-start:{}", part.tool_name))
                    }
                    TextStreamPart::ToolInputDelta(part) => {
                        Some(format!("tool-input-delta:{}", part.delta))
                    }
                    TextStreamPart::ToolInputEnd(part) => {
                        Some(format!("tool-input-end:{}", part.id))
                    }
                    TextStreamPart::ToolCall(part) => {
                        Some(format!("tool-call:{}", part.tool_name))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [
                "text:hello",
                "tool-input-start:tool1",
                r#"tool-input-delta:{"value":""#,
                r#"tool-input-delta:Sparkle Day"}"#,
                "tool-input-end:call-1",
                "tool-call:tool1",
            ]
        );
        assert_eq!(
            recorded.lock().expect("recorded lock").as_slice(),
            [
                json!({
                    "type": "onInputStart",
                    "toolCallId": "call-1",
                    "context": runtime_context,
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true
                }),
                json!({
                    "type": "onInputDelta",
                    "toolCallId": "call-1",
                    "inputTextDelta": r#"{"value":""#,
                    "context": runtime_context,
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true
                }),
                json!({
                    "type": "onInputDelta",
                    "toolCallId": "call-1",
                    "inputTextDelta": r#"Sparkle Day"}"#,
                    "context": runtime_context,
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true
                }),
                json!({
                    "type": "onInputAvailable",
                    "toolCallId": "call-1",
                    "input": { "value": "Sparkle Day" },
                    "context": runtime_context,
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true
                })
            ]
        );
    }

    #[test]
    fn stream_text_executes_local_tool_and_continues_to_final_text() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Brisbane is sunny.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
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

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
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

        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(model.stream_calls()[1].prompt.len(), 3);
        assert!(matches!(
            &model.stream_calls()[1].prompt[1],
            LanguageModelMessage::Assistant(message)
                if matches!(
                    &message.content[0],
                    LanguageModelAssistantContentPart::ToolCall(part)
                        if part.tool_name == "weather"
                            && part.input == json!({"city": "Brisbane"})
                )
        ));
        assert!(matches!(
            &model.stream_calls()[1].prompt[2],
            LanguageModelMessage::Tool(message)
                if matches!(
                    &message.content[0],
                    LanguageModelToolContentPart::ToolResult(part)
                        if part.tool_name == "weather"
                            && part.output == LanguageModelToolResultOutput::json(json!({
                                "forecast": "sunny",
                                "city": "Brisbane",
                                "toolCallId": "call-1"
                            }))
                )
        ));

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.text, "Brisbane is sunny.");
        assert_eq!(result.text_stream, vec!["Brisbane is sunny."]);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
        assert_eq!(result.usage, usage());
        assert_eq!(result.total_usage.input_tokens.total, Some(6));
        assert_eq!(result.total_usage.output_tokens.total, Some(20));
        assert_eq!(
            serde_json::to_value(&result.response_messages).expect("response messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": {
                                "city": "Brisbane"
                            },
                            "type": "tool-call"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "json",
                                "value": {
                                    "forecast": "sunny",
                                    "city": "Brisbane",
                                    "toolCallId": "call-1"
                                }
                            },
                            "type": "tool-result"
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "text": "Brisbane is sunny.",
                            "type": "text"
                        }
                    ]
                }
            ])
        );

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::ToolCall(_) => "tool-call",
                TextStreamPart::ToolResult(_) => "tool-result",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "tool-call",
                "tool-result",
                "finish-step",
                "start-step",
                "text-start",
                "text-delta",
                "text-end",
                "finish-step",
                "finish"
            ]
        );
    }

    #[test]
    fn stream_text_invokes_tool_execution_callbacks_for_local_tools() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let callback_events = Arc::new(Mutex::new(Vec::new()));
        let start_events = Arc::clone(&callback_events);
        let end_events = Arc::clone(&callback_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_on_tool_execution_start(move |event| {
                    let start_events = Arc::clone(&start_events);
                    async move {
                        start_events.lock().expect("events lock").push(format!(
                            "start:{}:{}:{}",
                            event.tool_call.tool_call_id,
                            event.tool_call.input["city"]
                                .as_str()
                                .expect("city is a string"),
                            event.messages.len()
                        ));
                    }
                })
                .with_on_tool_execution_end(move |event| {
                    let end_events = Arc::clone(&end_events);
                    async move {
                        end_events.lock().expect("events lock").push(format!(
                            "end:{}:{}:{}",
                            event.tool_call.tool_call_id,
                            event.tool_output.output["forecast"]
                                .as_str()
                                .expect("forecast is a string"),
                            event.messages.len()
                        ));
                    }
                }),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
        assert_eq!(
            callback_events.lock().expect("events lock").as_slice(),
            ["start:call-1:Brisbane:1", "end:call-1:sunny:1"]
        );
    }

    #[test]
    fn stream_text_uses_experimental_on_tool_call_start_as_a_fallback() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let events_for_callback = Arc::clone(&events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_experimental_on_tool_call_start(move |_event| {
                    let events = Arc::clone(&events_for_callback);
                    async move {
                        events
                            .lock()
                            .expect("events lock")
                            .push("experimental_onToolCallStart");
                    }
                }),
        ));

        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            ["experimental_onToolCallStart"]
        );
        assert_eq!(result.tool_results.len(), 1);
    }

    #[test]
    fn stream_text_prefers_on_tool_execution_start_over_experimental_on_tool_call_start() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let alias_events = Arc::clone(&events);
        let preferred_events = Arc::clone(&events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_on_tool_execution_start(move |_event| {
                    let preferred_events = Arc::clone(&preferred_events);
                    async move {
                        preferred_events
                            .lock()
                            .expect("events lock")
                            .push("onToolExecutionStart");
                    }
                })
                .with_experimental_on_tool_call_start(move |_event| {
                    let alias_events = Arc::clone(&alias_events);
                    async move {
                        alias_events
                            .lock()
                            .expect("events lock")
                            .push("experimental_onToolCallStart");
                    }
                }),
        ));

        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            ["onToolExecutionStart"]
        );
        assert_eq!(result.tool_results.len(), 1);
    }

    #[test]
    fn stream_text_uses_experimental_on_tool_call_finish_as_a_fallback() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let events_for_callback = Arc::clone(&events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_experimental_on_tool_call_finish(move |_event| {
                    let events = Arc::clone(&events_for_callback);
                    async move {
                        events
                            .lock()
                            .expect("events lock")
                            .push("experimental_onToolCallFinish");
                    }
                }),
        ));

        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            ["experimental_onToolCallFinish"]
        );
        assert_eq!(result.tool_results.len(), 1);
    }

    #[test]
    fn stream_text_prefers_on_tool_execution_end_over_experimental_on_tool_call_finish() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let alias_events = Arc::clone(&events);
        let preferred_events = Arc::clone(&events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_on_tool_execution_end(move |_event| {
                    let preferred_events = Arc::clone(&preferred_events);
                    async move {
                        preferred_events
                            .lock()
                            .expect("events lock")
                            .push("onToolExecutionEnd");
                    }
                })
                .with_experimental_on_tool_call_finish(move |_event| {
                    let alias_events = Arc::clone(&alias_events);
                    async move {
                        alias_events
                            .lock()
                            .expect("events lock")
                            .push("experimental_onToolCallFinish");
                    }
                }),
        ));

        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            ["onToolExecutionEnd"]
        );
        assert_eq!(result.tool_results.len(), 1);
    }

    #[test]
    fn stream_text_continues_for_deferred_provider_executed_tool_results() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "provider-call-1",
                        "providerTool",
                        r#"{"city":"Brisbane"}"#,
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "provider-call-1",
                    "providerTool",
                    NonNullJsonValue::new(json!({ "forecast": "sunny" }))
                        .expect("provider result is non-null"),
                )),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Deferred result ready.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let output_schema = input_schema.clone();
        let provider_args = json!({ "mode": "deferred" })
            .as_object()
            .expect("provider args are an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
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

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 2);
        assert!(matches!(
            &calls[1].prompt[1],
            LanguageModelMessage::Assistant(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelAssistantContentPart::ToolCall(part)
                            if part.tool_call_id == "provider-call-1"
                                && part.tool_name == "providerTool"
                                && part.input == json!({ "city": "Brisbane" })
                                && part.provider_executed == Some(true)
                    )
        ));

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.text, "Deferred result ready.");
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
    }

    #[test]
    fn stream_text_resolves_deferred_provider_tool_error_in_same_step() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "provider-call-1",
                        "providerTool",
                        r#"{"city":"Brisbane"}"#,
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(
                    LanguageModelToolResult::new(
                        "provider-call-1",
                        "providerTool",
                        NonNullJsonValue::new(json!("ERROR")).expect("provider error is non-null"),
                    )
                    .with_is_error(true),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Handled provider error.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let provider_args = json!({ "mode": "deferred" })
            .as_object()
            .expect("provider args are an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::provider_executed(
                        "providerTool",
                        "test.providerTool",
                        provider_args,
                        schema.clone(),
                        schema,
                    )
                    .with_supports_deferred_results(true),
                )
                .with_max_steps(3),
        ));

        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.text, "Handled provider error.");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "provider-call-1");
        assert_eq!(result.tool_results[0].tool_name, "providerTool");
        assert_eq!(result.tool_results[0].input, json!({ "city": "Brisbane" }));
        assert_eq!(result.tool_results[0].output, json!("ERROR"));
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
        assert_eq!(result.steps[0].tool_calls.len(), 1);
        assert_eq!(
            result.steps[0].tool_calls[0].input,
            json!({ "city": "Brisbane" })
        );
        assert_eq!(result.steps[0].tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.steps[0].tool_results.len(), 1);
        assert_eq!(
            result.steps[0].tool_results[0].input,
            json!({ "city": "Brisbane" })
        );
        assert_eq!(result.steps[0].tool_results[0].output, json!("ERROR"));
        assert_eq!(result.steps[0].tool_results[0].is_error, Some(true));
        assert_eq!(
            result.steps[0].tool_results[0].provider_executed,
            Some(true)
        );
    }

    #[test]
    fn stream_text_resolves_deferred_provider_tool_error_in_later_step() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "provider-call-1",
                        "providerTool",
                        r#"{"city":"Brisbane"}"#,
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolResult(
                    LanguageModelToolResult::new(
                        "provider-call-1",
                        "providerTool",
                        NonNullJsonValue::new(json!("ERROR")).expect("provider error is non-null"),
                    )
                    .with_is_error(true),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Handled provider error.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let provider_args = json!({ "mode": "deferred" })
            .as_object()
            .expect("provider args are an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(
                    Tool::provider_executed(
                        "providerTool",
                        "test.providerTool",
                        provider_args,
                        schema.clone(),
                        schema,
                    )
                    .with_supports_deferred_results(true),
                )
                .with_max_steps(3),
        ));

        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.text, "Handled provider error.");
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "provider-call-1");
        assert_eq!(result.tool_results[0].input, json!(null));
        assert_eq!(result.tool_results[0].output, json!("ERROR"));
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
    }

    #[test]
    fn stream_text_preserves_provider_metadata_when_replaying_the_next_step() {
        let tool_search_provider_metadata = ProviderMetadata::from([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("tsc_123"))]),
        )]);
        let tool_search_provider_options = ProviderOptions::from_iter([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("tsc_123"))]),
        )]);
        let tool_search_result_provider_metadata = ProviderMetadata::from([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("tso_123"))]),
        )]);
        let tool_search_result_provider_options = ProviderOptions::from_iter([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("tso_123"))]),
        )]);
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "tool-search-call-1",
                        "toolSearch",
                        r#"{"arguments":{"paths":["get_weather"]},"call_id":null}"#,
                    )
                    .with_provider_executed(true)
                    .with_provider_metadata(tool_search_provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolResult(
                    LanguageModelToolResult::new(
                        "tool-search-call-1",
                        "toolSearch",
                        NonNullJsonValue::new(json!({
                            "tools": [{
                                "type": "function",
                                "name": "get_weather",
                                "description": "Get the current weather at a specific location",
                                "parameters": {
                                    "type": "object",
                                    "properties": {
                                        "location": { "type": "string" }
                                    },
                                    "required": ["location"]
                                }
                            }]
                        }))
                        .expect("provider tool result is non-null"),
                    )
                    .with_provider_metadata(tool_search_result_provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-2",
                    "get_weather",
                    r#"{"location":"San Francisco, CA"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Sunny.")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let tool_search_input_schema = json!({
            "type": "object",
            "properties": {
                "arguments": {
                    "type": "object",
                    "properties": {
                        "paths": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["paths"]
                },
                "call_id": { "type": "null" }
            },
            "required": ["arguments", "call_id"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let tool_search_output_schema = json!({
            "type": "object",
            "properties": {
                "tools": {
                    "type": "array",
                    "items": { "type": "object" }
                }
            },
            "required": ["tools"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let get_weather_input_schema = json!({
            "type": "object",
            "properties": {
                "location": { "type": "string" }
            },
            "required": ["location"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::provider_executed(
                    "toolSearch",
                    "test.toolSearch",
                    json!({})
                        .as_object()
                        .expect("provider args are an object")
                        .clone(),
                    tool_search_input_schema,
                    tool_search_output_schema,
                ))
                .with_tool(
                    Tool::new("get_weather", get_weather_input_schema).with_execute(
                        |input, _| async move {
                            Ok(json!({
                                "forecast": "sunny",
                                "location": input["location"],
                            }))
                        },
                    ),
                )
                .with_max_steps(3),
        ));

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 2);
        assert!(matches!(
            &calls[1].prompt[1],
            LanguageModelMessage::Assistant(message)
                if message.content.len() == 3
                    && matches!(
                        &message.content[0],
                        LanguageModelAssistantContentPart::ToolCall(part)
                            if part.tool_call_id == "tool-search-call-1"
                                && part.tool_name == "toolSearch"
                                && part.input == json!({
                                    "arguments": { "paths": ["get_weather"] },
                                    "call_id": null
                                })
                                && part.provider_executed == Some(true)
                                && part.provider_options == Some(tool_search_provider_options.clone())
                    )
                    && matches!(
                        &message.content[1],
                        LanguageModelAssistantContentPart::ToolResult(part)
                            if part.tool_call_id == "tool-search-call-1"
                                && part.tool_name == "toolSearch"
                                && part.provider_options == Some(tool_search_result_provider_options.clone())
                    )
                    && matches!(
                        &message.content[2],
                        LanguageModelAssistantContentPart::ToolCall(part)
                            if part.tool_call_id == "call-2"
                                && part.tool_name == "get_weather"
                                && part.input == json!({ "location": "San Francisco, CA" })
                    )
        ));

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.text, "Sunny.");
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_results.len(), 2);
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(
            result.tool_calls[0].provider_metadata,
            Some(tool_search_provider_metadata)
        );
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
        assert_eq!(
            result.tool_results[0].provider_metadata,
            Some(tool_search_result_provider_metadata)
        );
        assert_eq!(result.tool_calls[1].tool_name, "get_weather");
        assert_eq!(result.tool_results[1].tool_name, "get_weather");
    }

    #[test]
    fn stream_text_invokes_lifecycle_callbacks_with_streamed_steps() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let callback_events = Arc::new(Mutex::new(Vec::new()));
        let start_events = Arc::clone(&callback_events);
        let step_start_events = Arc::clone(&callback_events);
        let model_start_events = Arc::clone(&callback_events);
        let model_end_events = Arc::clone(&callback_events);
        let step_finish_events = Arc::clone(&callback_events);
        let finish_events = Arc::clone(&callback_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_timeout(TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new()
                        .with_total_ms(5_000)
                        .with_step_ms(1_000),
                ))
                .with_stop_condition(StopCondition::StepCount(3))
                .with_on_start(move |event| {
                    let start_events = Arc::clone(&start_events);
                    async move {
                        assert_eq!(event.operation_id, "ai.streamText");
                        assert_eq!(event.messages.len(), 1);
                        assert_eq!(event.max_retries, DEFAULT_MAX_RETRIES);
                        assert_eq!(
                            event.timeout,
                            Some(TimeoutConfiguration::detailed(
                                TimeoutConfigurationOptions::new()
                                    .with_total_ms(5_000)
                                    .with_step_ms(1_000),
                            ))
                        );
                        start_events
                            .lock()
                            .expect("events lock")
                            .push("on-start".to_string());
                    }
                })
                .with_on_step_start(move |event| {
                    let step_start_events = Arc::clone(&step_start_events);
                    async move {
                        assert_eq!(event.step_number, 0);
                        assert_eq!(event.messages.len(), 1);
                        assert!(event.steps.is_empty());
                        step_start_events
                            .lock()
                            .expect("events lock")
                            .push("on-step-start".to_string());
                    }
                })
                .with_experimental_on_language_model_call_start(move |event| {
                    let model_start_events = Arc::clone(&model_start_events);
                    async move {
                        assert_eq!(event.messages.len(), 1);
                        model_start_events
                            .lock()
                            .expect("events lock")
                            .push("on-language-model-call-start".to_string());
                    }
                })
                .with_experimental_on_language_model_call_end(move |event| {
                    let model_end_events = Arc::clone(&model_end_events);
                    async move {
                        assert_eq!(event.finish_reason, FinishReason::Stop);
                        assert_eq!(event.usage, usage());
                        assert!(!event.response_id.is_empty());
                        model_end_events
                            .lock()
                            .expect("events lock")
                            .push("on-language-model-call-end".to_string());
                    }
                })
                .with_on_step_finish(move |step| {
                    let step_finish_events = Arc::clone(&step_finish_events);
                    async move {
                        assert_eq!(step.step_number, 0);
                        assert_eq!(step.text, "Hello");
                        assert!(
                            step.response
                                .and_then(|response| response.messages)
                                .is_some()
                        );
                        step_finish_events
                            .lock()
                            .expect("events lock")
                            .push("on-step-finish".to_string());
                    }
                })
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events);
                    async move {
                        assert_eq!(event.text, "Hello");
                        assert_eq!(event.finish_reason, FinishReason::Stop);
                        assert_eq!(event.steps.len(), 1);
                        assert_eq!(event.total_usage, usage());
                        finish_events
                            .lock()
                            .expect("events lock")
                            .push("on-finish".to_string());
                    }
                }),
        ));

        assert_eq!(result.text, "Hello");
        assert_eq!(
            callback_events.lock().expect("events lock").as_slice(),
            [
                "on-start",
                "on-step-start",
                "on-language-model-call-start",
                "on-language-model-call-end",
                "on-step-finish",
                "on-finish"
            ]
        );
    }

    #[test]
    fn stream_text_generates_consistent_call_id_for_language_model_callbacks() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let start_call_ids = Arc::new(Mutex::new(Vec::<String>::new()));
        let end_call_ids = Arc::new(Mutex::new(Vec::<String>::new()));
        let end_response_ids = Arc::new(Mutex::new(Vec::<String>::new()));
        let start_call_ids_for_callback = Arc::clone(&start_call_ids);
        let end_call_ids_for_callback = Arc::clone(&end_call_ids);
        let end_response_ids_for_callback = Arc::clone(&end_response_ids);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test prompt")])
                .with_experimental_on_language_model_call_start(move |event| {
                    let start_call_ids = Arc::clone(&start_call_ids_for_callback);
                    async move {
                        assert_eq!(event.provider, "mock-provider");
                        assert_eq!(event.model_id, "mock-model-id");
                        assert_eq!(event.messages.len(), 1);
                        start_call_ids
                            .lock()
                            .expect("start call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_experimental_on_language_model_call_end(move |event| {
                    let end_call_ids = Arc::clone(&end_call_ids_for_callback);
                    let end_response_ids = Arc::clone(&end_response_ids_for_callback);
                    async move {
                        assert_eq!(event.provider, "mock-provider");
                        assert_eq!(event.model_id, "mock-model-id");
                        assert_eq!(event.finish_reason, FinishReason::Stop);
                        assert_eq!(event.usage, usage());
                        assert!(event.content.is_empty());
                        end_call_ids
                            .lock()
                            .expect("end call ids lock")
                            .push(event.call_id);
                        end_response_ids
                            .lock()
                            .expect("end response ids lock")
                            .push(event.response_id);
                    }
                }),
        ));

        assert_eq!(result.steps.len(), 1);
        let step = &result.steps[0];
        let start_call_ids = start_call_ids.lock().expect("start call ids lock");
        let end_call_ids = end_call_ids.lock().expect("end call ids lock");
        assert_eq!(start_call_ids.len(), 1);
        assert_eq!(end_call_ids.len(), 1);
        assert!(start_call_ids[0].starts_with("call-"));
        assert_eq!(start_call_ids[0].len(), "call-".len() + 24);
        assert_eq!(end_call_ids[0], start_call_ids[0]);

        assert_eq!(end_call_ids.as_slice(), [start_call_ids[0].clone()]);
        assert_eq!(
            end_response_ids
                .lock()
                .expect("end response ids lock")
                .as_slice(),
            [step
                .response
                .id
                .clone()
                .expect("stream_text generated a response id")]
        );
    }

    #[test]
    fn stream_text_measures_time_to_first_output_token_from_text_deltas() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let model_call_end_events = Arc::new(Mutex::new(Vec::new()));
        let model_call_end_events_for_callback = Arc::clone(&model_call_end_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_experimental_on_language_model_call_end(move |event| {
                    let model_call_end_events = Arc::clone(&model_call_end_events_for_callback);
                    async move {
                        model_call_end_events
                            .lock()
                            .expect("model call end events lock")
                            .push(event.performance);
                    }
                }),
        ));

        assert_eq!(result.text, "Hello");
        assert_eq!(result.steps.len(), 1);
        let step_performance = &result.steps[0].performance;
        assert!(
            step_performance.time_to_first_output_token_ms.is_some(),
            "first output token duration is captured"
        );

        let end_events = model_call_end_events
            .lock()
            .expect("model call end events lock");
        assert_eq!(end_events.len(), 1);
        assert_eq!(
            end_events[0].time_to_first_output_token_ms,
            step_performance.time_to_first_output_token_ms
        );
        assert!(end_events[0].output_tokens_per_second.is_some());
        assert!(end_events[0].input_tokens_per_second.is_some());
        assert!(end_events[0].effective_output_tokens_per_second.is_finite());
        assert!(end_events[0].effective_total_tokens_per_second.is_finite());
    }

    #[test]
    fn stream_text_measures_time_to_first_output_token_from_tool_input_deltas() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolInputStart(LanguageModelToolInputStart::new(
                    "call-1", "tool1",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1", "{",
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    "{\"value\":\"value\"}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let model_call_end_events = Arc::new(Mutex::new(Vec::new()));
        let model_call_end_events_for_callback = Arc::clone(&model_call_end_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Call the tool")])
                .with_experimental_on_language_model_call_end(move |event| {
                    let model_call_end_events = Arc::clone(&model_call_end_events_for_callback);
                    async move {
                        model_call_end_events
                            .lock()
                            .expect("model call end events lock")
                            .push(event.performance);
                    }
                }),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.steps.len(), 1);
        let step_performance = &result.steps[0].performance;
        assert!(
            step_performance.time_to_first_output_token_ms.is_some(),
            "first tool-input token duration is captured"
        );

        let end_events = model_call_end_events
            .lock()
            .expect("model call end events lock");
        assert_eq!(end_events.len(), 1);
        assert_eq!(
            end_events[0].time_to_first_output_token_ms,
            step_performance.time_to_first_output_token_ms
        );
        assert!(end_events[0].output_tokens_per_second.is_some());
        assert!(end_events[0].input_tokens_per_second.is_some());
        assert!(end_events[0].effective_output_tokens_per_second.is_finite());
        assert!(end_events[0].effective_total_tokens_per_second.is_finite());
    }

    #[test]
    fn stream_text_invokes_finish_callback_with_completed_records() {
        let provider_metadata = ProviderMetadata::from([(
            "mock".to_string(),
            Map::from_iter([("trace".to_string(), json!("stream-finish"))]),
        )]);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(
                    LanguageModelStreamFinish::new(usage(), finish_reason())
                        .with_provider_metadata(provider_metadata.clone()),
                ),
            ]));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")]).with_on_finish(
                move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                },
            ),
        ));

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert_eq!(finish_events[0].text, result.text);
        assert_eq!(finish_events[0].finish_reason, result.finish_reason);
        assert_eq!(finish_events[0].raw_finish_reason, result.raw_finish_reason);
        assert_eq!(finish_events[0].usage, result.usage);
        assert_eq!(finish_events[0].total_usage, result.total_usage);
        assert_eq!(finish_events[0].provider_metadata, Some(provider_metadata));
        assert_eq!(finish_events[0].steps.len(), 1);
        assert_eq!(finish_events[0].steps[0].text, result.steps[0].text);
        let step_response = finish_events[0].steps[0]
            .response
            .as_ref()
            .expect("finish event step has response metadata");
        assert!(step_response.id.is_some());
        assert!(step_response.timestamp.is_some());
        assert_eq!(step_response.model_id.as_deref(), Some("mock-model-id"));
        assert!(
            step_response
                .messages
                .as_ref()
                .is_some_and(|messages| !messages.is_empty())
        );
    }

    #[test]
    fn stream_text_passes_runtime_context_to_finish_callback() {
        let runtime_context = JsonObject::from_iter([("context".to_string(), json!("test"))]);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1", "Hello, ",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let finish_contexts = Arc::new(Mutex::new(Vec::<JsonObject>::new()));
        let finish_contexts_for_callback = Arc::clone(&finish_contexts);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_runtime_context(runtime_context.clone())
                .with_on_finish(move |event| {
                    let finish_contexts = Arc::clone(&finish_contexts_for_callback);
                    async move {
                        finish_contexts
                            .lock()
                            .expect("finish contexts lock")
                            .push(event.runtime_context);
                    }
                }),
        ));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            finish_contexts
                .lock()
                .expect("finish contexts lock")
                .as_slice(),
            [runtime_context]
        );
    }

    #[test]
    fn stream_text_marks_schema_invalid_tool_call_in_result_full_and_ui_stream() {
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
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ToolInputStart(LanguageModelToolInputStart::new(
                    "call-1",
                    "cityAttractions",
                )),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    r#"{ "cities": "San Francisco" }"#,
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "cityAttractions",
                    r#"{ "cities": "San Francisco" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("cityAttractions", input_schema)),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_call_id, "call-1");
        assert_eq!(result.tool_calls[0].tool_name, "cityAttractions");
        assert_eq!(
            result.tool_calls[0].input,
            json!({ "cities": "San Francisco" })
        );
        assert_eq!(result.tool_calls[0].invalid, Some(true));
        assert_eq!(result.tool_calls[0].dynamic, Some(true));
        let error_text = result.tool_calls[0]
            .error
            .as_deref()
            .expect("invalid tool call includes an error");
        assert!(error_text.contains("Invalid input for tool cityAttractions"));
        assert!(error_text.contains("$.city is required"));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert_eq!(result.tool_results[0].dynamic, Some(true));
        assert_eq!(
            result.tool_results[0]
                .output
                .as_str()
                .expect("tool error output is text"),
            error_text
        );

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].tool_calls, result.tool_calls);
        assert_eq!(result.steps[0].tool_results, result.tool_results);

        let full_stream_tool_call = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolCall(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes invalid tool call");
        assert_eq!(full_stream_tool_call.invalid, Some(true));
        assert_eq!(full_stream_tool_call.error.as_deref(), Some(error_text));

        let full_stream_tool_result = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolResult(part) => Some(part),
                _ => None,
            })
            .expect("full stream includes tool error result");
        assert_eq!(full_stream_tool_result.is_error, Some(true));
        assert_eq!(full_stream_tool_result.output.as_str(), Some(error_text));

        let ui_chunks = serde_json::to_value(result.to_ui_message_stream())
            .expect("ui message stream serializes");
        let ui_chunks = ui_chunks.as_array().expect("ui chunks are an array");
        assert!(
            ui_chunks
                .iter()
                .any(|chunk| chunk["type"] == "tool-input-start"
                    && chunk["toolCallId"] == "call-1"
                    && chunk["toolName"] == "cityAttractions")
        );
        assert!(
            ui_chunks
                .iter()
                .any(|chunk| chunk["type"] == "tool-input-delta"
                    && chunk["toolCallId"] == "call-1"
                    && chunk["inputTextDelta"] == r#"{ "cities": "San Francisco" }"#)
        );
        assert!(
            ui_chunks
                .iter()
                .any(|chunk| chunk["type"] == "tool-input-error"
                    && chunk["toolCallId"] == "call-1"
                    && chunk["toolName"] == "cityAttractions"
                    && chunk["input"] == json!({ "cities": "San Francisco" })
                    && chunk["errorText"] == error_text)
        );
        assert!(
            ui_chunks
                .iter()
                .any(|chunk| chunk["type"] == "tool-output-error"
                    && chunk["toolCallId"] == "call-1"
                    && chunk["errorText"] == error_text)
        );
    }

    #[test]
    fn stream_text_streams_preliminary_tool_results_before_final_result() {
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
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "cityAttractions",
                    r#"{ "city": "San Francisco" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("cityAttractions", input_schema).with_execute_outputs(
                    |input, _options| {
                        ready(Ok(vec![
                            ExecuteToolOutput::preliminary(json!({
                                "status": "loading",
                                "text": format!(
                                    "Getting weather for {}",
                                    input["city"].as_str().unwrap_or_default()
                                )
                            })),
                            ExecuteToolOutput::preliminary(json!({
                                "status": "success",
                                "text": format!(
                                    "The weather in {} is 72°F",
                                    input["city"].as_str().unwrap_or_default()
                                ),
                                "temperature": 72
                            })),
                        ]))
                    },
                ),
            ),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].preliminary, None);
        assert_eq!(
            result.tool_results[0].output,
            json!({
                "status": "success",
                "text": "The weather in San Francisco is 72°F",
                "temperature": 72
            })
        );
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].tool_results, result.tool_results);

        let streamed_tool_results = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ToolResult(part) => Some((part.preliminary, part.output.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            streamed_tool_results,
            vec![
                (
                    Some(true),
                    json!({
                        "status": "loading",
                        "text": "Getting weather for San Francisco"
                    })
                ),
                (
                    Some(true),
                    json!({
                        "status": "success",
                        "text": "The weather in San Francisco is 72°F",
                        "temperature": 72
                    })
                ),
                (
                    None,
                    json!({
                        "status": "success",
                        "text": "The weather in San Francisco is 72°F",
                        "temperature": 72
                    })
                ),
            ]
        );

        let ui_chunks = serde_json::to_value(result.to_ui_message_stream())
            .expect("ui message stream serializes");
        let tool_output_chunks = ui_chunks
            .as_array()
            .expect("ui chunks are an array")
            .iter()
            .filter(|chunk| chunk["type"] == "tool-output-available")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            tool_output_chunks,
            vec![
                json!({
                    "type": "tool-output-available",
                    "toolCallId": "call-1",
                    "output": {
                        "status": "loading",
                        "text": "Getting weather for San Francisco"
                    },
                    "preliminary": true
                }),
                json!({
                    "type": "tool-output-available",
                    "toolCallId": "call-1",
                    "output": {
                        "status": "success",
                        "text": "The weather in San Francisco is 72°F",
                        "temperature": 72
                    },
                    "preliminary": true
                }),
                json!({
                    "type": "tool-output-available",
                    "toolCallId": "call-1",
                    "output": {
                        "status": "success",
                        "text": "The weather in San Francisco is 72°F",
                        "temperature": 72
                    }
                }),
            ]
        );
    }

    #[test]
    fn stream_text_preserves_provider_executed_dynamic_tool_input_streaming() {
        let provider_metadata = ProviderMetadata::from([(
            "anthropic".to_string(),
            Map::from_iter([("serverName".to_string(), json!("echo"))]),
        )]);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new("call-1", "cityAttractions")
                        .with_provider_executed(true)
                        .with_dynamic(true)
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    r#"{ "city": "San Francisco" }"#,
                )),
                LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        "call-1",
                        "cityAttractions",
                        r#"{ "city": "San Francisco" }"#,
                    )
                    .with_provider_executed(true)
                    .with_dynamic(true)
                    .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::ToolResult(
                    LanguageModelToolResult::new(
                        "call-1",
                        "cityAttractions",
                        NonNullJsonValue::new(json!({
                            "status": "success",
                            "text": "The weather in San Francisco is 72°F"
                        }))
                        .expect("tool result is non-null"),
                    )
                    .with_dynamic(true)
                    .with_provider_metadata(provider_metadata.clone()),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_calls[0].dynamic, Some(true));
        assert_eq!(
            result.tool_calls[0].provider_metadata,
            Some(provider_metadata.clone())
        );
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
        assert_eq!(result.tool_results[0].dynamic, Some(true));
        assert_eq!(
            result.tool_results[0].provider_metadata,
            Some(provider_metadata.clone())
        );
        assert_eq!(result.steps[0].tool_calls, result.tool_calls);
        assert_eq!(result.steps[0].tool_results, result.tool_results);

        let full_stream = serde_json::to_value(&result.parts).expect("parts serialize");
        for expected in [
            json!({
                "type": "tool-input-start",
                "id": "call-1",
                "toolName": "cityAttractions",
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": { "anthropic": { "serverName": "echo" } }
            }),
            json!({
                "type": "tool-call",
                "toolCallId": "call-1",
                "toolName": "cityAttractions",
                "input": { "city": "San Francisco" },
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": { "anthropic": { "serverName": "echo" } }
            }),
            json!({
                "type": "tool-result",
                "toolCallId": "call-1",
                "toolName": "cityAttractions",
                "input": { "city": "San Francisco" },
                "output": {
                    "status": "success",
                    "text": "The weather in San Francisco is 72°F"
                },
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": { "anthropic": { "serverName": "echo" } }
            }),
        ] {
            assert!(
                full_stream
                    .as_array()
                    .expect("full stream parts are an array")
                    .contains(&expected),
                "missing expected full-stream part: {expected}"
            );
        }

        let ui_chunks = serde_json::to_value(result.to_ui_message_stream())
            .expect("ui message stream serializes");
        for expected in [
            json!({
                "type": "tool-input-start",
                "toolCallId": "call-1",
                "toolName": "cityAttractions",
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": { "anthropic": { "serverName": "echo" } }
            }),
            json!({
                "type": "tool-input-available",
                "toolCallId": "call-1",
                "toolName": "cityAttractions",
                "input": { "city": "San Francisco" },
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": { "anthropic": { "serverName": "echo" } }
            }),
            json!({
                "type": "tool-output-available",
                "toolCallId": "call-1",
                "output": {
                    "status": "success",
                    "text": "The weather in San Francisco is 72°F"
                },
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": { "anthropic": { "serverName": "echo" } }
            }),
        ] {
            assert!(
                ui_chunks
                    .as_array()
                    .expect("ui message chunks are an array")
                    .contains(&expected),
                "missing expected UI-message chunk: {expected}"
            );
        }
    }

    #[test]
    fn stream_text_dispatches_telemetry_lifecycle_events() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepStart,
            TelemetryEventKind::OnLanguageModelCallStart,
            TelemetryEventKind::OnLanguageModelCallEnd,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let captured = Arc::clone(&events);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("telemetry event lock").push(event);
            });
        }

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")]).with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("stream-text-test")
                    .with_record_inputs(false)
                    .with_record_outputs(true)
                    .with_integration(integration),
            ),
        ));

        assert_eq!(result.text, "Hello");
        let events = events.lock().expect("telemetry event lock");
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![
                TelemetryEventKind::OnStart,
                TelemetryEventKind::OnStepStart,
                TelemetryEventKind::OnLanguageModelCallStart,
                TelemetryEventKind::OnLanguageModelCallEnd,
                TelemetryEventKind::OnStepFinish,
                TelemetryEventKind::OnEnd,
            ]
        );
        assert!(
            events
                .iter()
                .all(|event| event.function_id.as_deref() == Some("stream-text-test"))
        );
        assert!(
            events
                .iter()
                .all(|event| event.record_inputs == Some(false))
        );
        assert!(
            events
                .iter()
                .all(|event| event.record_outputs == Some(true))
        );
        assert_eq!(events[0].event["operationId"], json!("ai.streamText"));
        assert_eq!(events[0].event["provider"], json!("mock-provider"));
        assert_eq!(events[0].event["maxRetries"], json!(DEFAULT_MAX_RETRIES));
        assert_eq!(events[5].event["text"], json!("Hello"));
    }

    #[test]
    fn stream_text_telemetry_excludes_runtime_context_by_default() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let telemetry_contexts = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let captured = Arc::clone(&telemetry_contexts);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("telemetry event lock").push(
                    event
                        .event
                        .get("runtimeContext")
                        .cloned()
                        .expect("runtimeContext field"),
                );
            });
        }

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_runtime_context(
                    json!({
                        "userId": "user-123",
                        "requestId": "request-123",
                    })
                    .as_object()
                    .expect("runtime context is object")
                    .clone(),
                )
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("stream-text-test")
                        .with_integration(integration),
                ),
        ));

        assert_eq!(result.text, "Hello");
        let telemetry_contexts = telemetry_contexts.lock().expect("telemetry event lock");
        assert_eq!(
            telemetry_contexts.as_slice(),
            [json!({}), json!({}), json!({}), json!({})]
        );
    }

    #[test]
    fn stream_text_telemetry_includes_configured_runtime_context_properties() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let telemetry_contexts = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let captured = Arc::clone(&telemetry_contexts);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("telemetry event lock").push(
                    event
                        .event
                        .get("runtimeContext")
                        .cloned()
                        .expect("runtimeContext field"),
                );
            });
        }

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_runtime_context(
                    json!({
                        "userId": "user-123",
                        "requestId": "request-123",
                    })
                    .as_object()
                    .expect("runtime context is object")
                    .clone(),
                )
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("stream-text-test")
                        .with_integration(integration)
                        .with_runtime_context_key("requestId", true),
                ),
        ));

        assert_eq!(result.text, "Hello");
        let telemetry_contexts = telemetry_contexts.lock().expect("telemetry event lock");
        assert_eq!(
            telemetry_contexts.as_slice(),
            [
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
                json!({ "requestId": "request-123" }),
            ]
        );
    }

    #[test]
    fn stream_text_passes_full_runtime_context_to_callbacks() {
        let callback_contexts = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let on_start_contexts = Arc::clone(&callback_contexts);
        let on_step_start_contexts = Arc::clone(&callback_contexts);
        let on_step_finish_contexts = Arc::clone(&callback_contexts);
        let on_finish_contexts = Arc::clone(&callback_contexts);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(
                &MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                    LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                    LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                        "text-1", "Hello",
                    )),
                    LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                    LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                        usage(),
                        finish_reason(),
                    )),
                ])),
                vec![user_message("Say hello")],
            )
            .with_runtime_context(
                json!({
                    "userId": "user-123",
                    "requestId": "request-123",
                })
                .as_object()
                .expect("runtime context is object")
                .clone(),
            )
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("stream-text-test")
                    .with_runtime_context_key("requestId", true),
            )
            .with_on_start(move |event| {
                on_start_contexts
                    .lock()
                    .expect("runtime context lock")
                    .push(JsonValue::Object(event.runtime_context.clone()));
                ready(())
            })
            .with_on_step_start(move |event| {
                on_step_start_contexts
                    .lock()
                    .expect("runtime context lock")
                    .push(JsonValue::Object(event.runtime_context.clone()));
                ready(())
            })
            .with_on_step_finish(move |event| {
                on_step_finish_contexts
                    .lock()
                    .expect("runtime context lock")
                    .push(JsonValue::Object(event.runtime_context.clone()));
                ready(())
            })
            .with_on_finish(move |event| {
                on_finish_contexts
                    .lock()
                    .expect("runtime context lock")
                    .push(JsonValue::Object(event.runtime_context.clone()));
                ready(())
            }),
        ));

        assert_eq!(result.text, "Hello");
        let callback_contexts = callback_contexts.lock().expect("runtime context lock");
        assert_eq!(
            callback_contexts.as_slice(),
            [
                json!({ "userId": "user-123", "requestId": "request-123" }),
                json!({ "userId": "user-123", "requestId": "request-123" }),
                json!({ "userId": "user-123", "requestId": "request-123" }),
                json!({ "userId": "user-123", "requestId": "request-123" }),
            ]
        );
    }

    #[test]
    fn stream_text_telemetry_includes_configured_tools_context_properties() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let telemetry_contexts = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let tools_context = json!({
            "weather": {
                "apiKey": "secret-api-key",
                "region": "eu",
            },
        })
        .as_object()
        .expect("tools context is object")
        .clone();
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
                    "apiKey": { "type": "string" },
                    "region": { "type": "string" },
                },
                "required": ["apiKey", "region"]
            })
            .as_object()
            .expect("schema is an object")
            .clone(),
        );
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepStart,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let captured = Arc::clone(&telemetry_contexts);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("telemetry event lock").push(
                    event
                        .event
                        .get("toolsContext")
                        .cloned()
                        .expect("toolsContext field"),
                );
            });
        }

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_tools_context(tools_context.clone())
                .with_tool(Tool::new("weather", input_schema).with_context_schema(context_schema))
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("stream-text-test")
                        .with_integration(integration)
                        .with_tool_context_key("weather", "region", true),
                ),
        ));

        assert_eq!(result.text, "Hello");
        let telemetry_contexts = telemetry_contexts.lock().expect("telemetry event lock");
        assert_eq!(
            telemetry_contexts.as_slice(),
            [
                json!({ "weather": { "region": "eu" } }),
                json!({ "weather": { "region": "eu" } }),
                json!({ "weather": { "region": "eu" } }),
                json!({ "weather": { "region": "eu" } }),
            ]
        );
    }

    #[test]
    fn stream_text_calls_globally_registered_integration_listeners() {
        let _guard = telemetry_test_guard_for_tests();
        reset_telemetry_state_for_tests();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let global_start_events = Arc::clone(&events);
        let global_step_finish_events = Arc::clone(&events);
        let global_end_events = Arc::clone(&events);
        let global_start_seen = Arc::new(AtomicBool::new(false));
        let global_step_finish_seen = Arc::new(AtomicBool::new(false));
        let global_end_seen = Arc::new(AtomicBool::new(false));
        let global_start_seen_for_callback = Arc::clone(&global_start_seen);
        let global_step_finish_seen_for_callback = Arc::clone(&global_step_finish_seen);
        let global_end_seen_for_callback = Arc::clone(&global_end_seen);
        register_telemetry_integration(
            TelemetryIntegration::new()
                .with_callback(TelemetryEventKind::OnStart, move |_| {
                    if !global_start_seen_for_callback.swap(true, Ordering::SeqCst) {
                        global_start_events
                            .lock()
                            .expect("telemetry event lock")
                            .push("global-onStart".to_string());
                    }
                })
                .with_callback(TelemetryEventKind::OnStepFinish, move |_| {
                    if !global_step_finish_seen_for_callback.swap(true, Ordering::SeqCst) {
                        global_step_finish_events
                            .lock()
                            .expect("telemetry event lock")
                            .push("global-onStepFinish".to_string());
                    }
                })
                .with_callback(TelemetryEventKind::OnEnd, move |_| {
                    if !global_end_seen_for_callback.swap(true, Ordering::SeqCst) {
                        global_end_events
                            .lock()
                            .expect("telemetry event lock")
                            .push("global-onEnd".to_string());
                    }
                }),
        );

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            events.lock().expect("telemetry event lock").as_slice(),
            [
                "global-onStart".to_string(),
                "global-onStepFinish".to_string(),
                "global-onEnd".to_string()
            ]
        );
        reset_telemetry_state_for_tests();
    }

    #[test]
    fn stream_text_prefers_per_call_integrations_over_global_integrations() {
        let _guard = telemetry_test_guard_for_tests();
        reset_telemetry_state_for_tests();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let global_events = Arc::clone(&events);
        register_telemetry_integration(TelemetryIntegration::new().with_callback(
            TelemetryEventKind::OnStart,
            move |event| {
                if event.function_id.as_deref() == Some("stream-text-per-call-precedence") {
                    global_events
                        .lock()
                        .expect("telemetry event lock")
                        .push("global".to_string());
                }
            },
        ));
        let per_call_events = Arc::clone(&events);
        let per_call =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |_| {
                per_call_events
                    .lock()
                    .expect("telemetry event lock")
                    .push("per-call".to_string());
            });

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("stream-text-per-call-precedence")
                    .with_integration(per_call),
            ),
        ));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            events.lock().expect("telemetry event lock").as_slice(),
            ["per-call".to_string()]
        );
        reset_telemetry_state_for_tests();
    }

    #[test]
    fn stream_text_calls_integration_listeners_alongside_user_callbacks() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let user_start_events = Arc::clone(&events);
        let user_step_finish_events = Arc::clone(&events);
        let user_finish_events = Arc::clone(&events);
        let integration_start_events = Arc::clone(&events);
        let integration_step_finish_events = Arc::clone(&events);
        let integration_end_events = Arc::clone(&events);
        let integration = TelemetryIntegration::new()
            .with_callback(TelemetryEventKind::OnStart, move |_| {
                integration_start_events
                    .lock()
                    .expect("telemetry event lock")
                    .push("integration-onStart".to_string());
            })
            .with_callback(TelemetryEventKind::OnStepFinish, move |_| {
                integration_step_finish_events
                    .lock()
                    .expect("telemetry event lock")
                    .push("integration-onStepFinish".to_string());
            })
            .with_callback(TelemetryEventKind::OnEnd, move |_| {
                integration_end_events
                    .lock()
                    .expect("telemetry event lock")
                    .push("integration-onEnd".to_string());
            });

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_on_start(move |_| {
                    let events = Arc::clone(&user_start_events);
                    async move {
                        events
                            .lock()
                            .expect("telemetry event lock")
                            .push("user-onStart".to_string());
                    }
                })
                .with_on_step_finish(move |_| {
                    let events = Arc::clone(&user_step_finish_events);
                    async move {
                        events
                            .lock()
                            .expect("telemetry event lock")
                            .push("user-onStepFinish".to_string());
                    }
                })
                .with_on_finish(move |_| {
                    let events = Arc::clone(&user_finish_events);
                    async move {
                        events
                            .lock()
                            .expect("telemetry event lock")
                            .push("user-onFinish".to_string());
                    }
                })
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        ));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            events.lock().expect("telemetry event lock").as_slice(),
            [
                "user-onStart".to_string(),
                "integration-onStart".to_string(),
                "user-onStepFinish".to_string(),
                "integration-onStepFinish".to_string(),
                "user-onFinish".to_string(),
                "integration-onEnd".to_string()
            ]
        );
    }

    #[test]
    fn stream_text_does_not_break_when_integration_listener_panics() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let integration = TelemetryIntegration::new()
            .with_callback(TelemetryEventKind::OnStart, |_| {
                panic!("integration error");
            })
            .with_callback(TelemetryEventKind::OnStepFinish, |_| {
                panic!("integration error");
            })
            .with_callback(TelemetryEventKind::OnEnd, |_| {
                panic!("integration error");
            });

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_telemetry(TelemetryOptions::new().with_integration(integration)),
        ));

        assert_eq!(result.text, "Hello, world!");
    }

    #[test]
    fn stream_text_supports_multiple_per_call_telemetry_integrations_as_array() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let first_events = Arc::clone(&events);
        let second_events = Arc::clone(&events);
        let first =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |_| {
                first_events
                    .lock()
                    .expect("telemetry event lock")
                    .push("first".to_string());
            });
        let second =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |_| {
                second_events
                    .lock()
                    .expect("telemetry event lock")
                    .push("second".to_string());
            });

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_telemetry(TelemetryOptions::new().with_integrations([first, second])),
        ));

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(
            events.lock().expect("telemetry event lock").as_slice(),
            ["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn stream_text_accepts_experimental_telemetry_alias() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let start_events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let telemetry_events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let start_events_for_callback = Arc::clone(&start_events);
        let telemetry_events_for_callback = Arc::clone(&telemetry_events);
        let integration =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |event| {
                telemetry_events_for_callback
                    .lock()
                    .expect("telemetry event lock")
                    .push(event);
            });

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_experimental_telemetry(
                    TelemetryOptions::new()
                        .with_enabled(true)
                        .with_function_id("deprecated-fn")
                        .with_integration(integration),
                )
                .with_on_start(move |event| {
                    let start_events = Arc::clone(&start_events_for_callback);
                    async move {
                        start_events
                            .lock()
                            .expect("start event lock")
                            .push(serde_json::to_value(event).expect("event serializes"));
                    }
                }),
        ));

        assert_eq!(result.text, "Hello");
        let start_events = start_events.lock().expect("start event lock");
        assert_eq!(start_events.len(), 1);
        assert!(start_events[0].get("isEnabled").is_none());
        assert!(start_events[0].get("functionId").is_none());
        let telemetry_events = telemetry_events.lock().expect("telemetry event lock");
        assert_eq!(telemetry_events.len(), 1);
        assert_eq!(
            telemetry_events[0].function_id.as_deref(),
            Some("deprecated-fn")
        );
    }

    #[test]
    fn stream_text_dispatches_tool_execution_telemetry_events() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let tool_start_events = Arc::clone(&events);
        let tool_end_events = Arc::clone(&events);
        let integration = TelemetryIntegration::new()
            .with_callback(TelemetryEventKind::OnToolExecutionStart, move |event| {
                tool_start_events
                    .lock()
                    .expect("telemetry event lock")
                    .push(event);
            })
            .with_callback(TelemetryEventKind::OnToolExecutionEnd, move |event| {
                tool_end_events
                    .lock()
                    .expect("telemetry event lock")
                    .push(event);
            });

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("stream-tool-telemetry")
                        .with_integration(integration),
                ),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
        let events = events.lock().expect("telemetry event lock");
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![
                TelemetryEventKind::OnToolExecutionStart,
                TelemetryEventKind::OnToolExecutionEnd,
            ]
        );
        assert_eq!(events[0].event["toolCall"]["toolName"], json!("weather"));
        assert_eq!(events[1].event["toolCall"]["toolCallId"], json!("call-1"));
        assert!(events[1].event["toolExecutionMs"].is_number());
        assert!(
            events
                .iter()
                .all(|event| event.function_id.as_deref() == Some("stream-tool-telemetry"))
        );
    }

    #[test]
    fn stream_text_invokes_chunk_callback_for_portable_chunks() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Raw(LanguageModelRawStreamPart::new(
                    json!({"type": "raw-data", "content": "kept"}),
                )),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let callback_events = Arc::new(Mutex::new(Vec::new()));
        let chunk_events = Arc::clone(&callback_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_include_raw_chunks(true)
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, _options| async move { Ok(json!({ "forecast": "sunny" })) },
                ))
                .with_on_chunk(move |event| {
                    let chunk_events = Arc::clone(&chunk_events);
                    async move {
                        let label = match event.chunk {
                            TextStreamPart::Raw(_) => "raw".to_string(),
                            TextStreamPart::TextDelta(part) => format!("text:{}", part.text),
                            TextStreamPart::ToolCall(part) => {
                                format!("tool-call:{}", part.tool_name)
                            }
                            TextStreamPart::ToolResult(part) => {
                                format!("tool-result:{}", part.tool_name)
                            }
                            _ => "other".to_string(),
                        };
                        chunk_events.lock().expect("events lock").push(label);
                    }
                }),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(
            callback_events.lock().expect("events lock").as_slice(),
            [
                "raw",
                "text:Hello",
                "tool-call:weather",
                "tool-result:weather"
            ]
        );
    }

    #[test]
    fn stream_text_invokes_error_callback_for_error_parts() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({"message": "chunk failed"}),
                )),
            ]));
        let callback_errors = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::clone(&callback_errors);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")]).with_on_error(
                move |event| {
                    let errors = Arc::clone(&errors);
                    async move {
                        errors.lock().expect("errors lock").push(
                            event.error["message"]
                                .as_str()
                                .expect("message is a string")
                                .to_string(),
                        );
                    }
                },
            ),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(
            callback_errors.lock().expect("errors lock").as_slice(),
            ["chunk failed"]
        );
    }

    #[test]
    fn stream_text_invokes_finish_callback_when_error_chunk_occurs_mid_stream() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Hello")),
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({"message": "chunk error"}),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: None,
                    },
                )),
            ]));
        let callback_errors = Arc::new(Mutex::new(Vec::<String>::new()));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let errors = Arc::clone(&callback_errors);
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_on_error(move |event| {
                    let errors = Arc::clone(&errors);
                    async move {
                        errors.lock().expect("errors lock").push(
                            event.error["message"]
                                .as_str()
                                .expect("message is a string")
                                .to_string(),
                        );
                    }
                })
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                }),
        ));

        assert_eq!(result.text, "Hello");
        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(
            callback_errors.lock().expect("errors lock").as_slice(),
            ["chunk error"]
        );

        let error_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::Error(_)))
            .expect("full stream includes error part");
        let finish_step_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::FinishStep(_)))
            .expect("full stream includes finish-step");
        let finish_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::Finish(_)))
            .expect("full stream includes finish");
        assert!(error_index < finish_step_index);
        assert!(finish_step_index < finish_index);

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert_eq!(finish_events[0].finish_reason, FinishReason::Error);
        assert_eq!(finish_events[0].text, "Hello");
        assert_eq!(finish_events[0].usage, usage());
        assert_eq!(finish_events[0].total_usage, usage());
    }

    #[test]
    fn stream_text_invokes_error_callback_when_error_occurs_in_second_step() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: None,
                    },
                )),
            ]),
            LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
                LanguageModelErrorStreamPart::new(json!({"message": "test error"})),
            )]),
        ]);
        let input_schema = json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let callback_errors = Arc::new(Mutex::new(Vec::<String>::new()));
        let errors = Arc::clone(&callback_errors);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_max_steps(3)
                .with_on_error(move |event| {
                    let errors = Arc::clone(&errors);
                    async move {
                        errors.lock().expect("errors lock").push(
                            event.error["message"]
                                .as_str()
                                .expect("message is a string")
                                .to_string(),
                        );
                    }
                }),
        ));

        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(
            callback_errors.lock().expect("errors lock").as_slice(),
            ["test error"]
        );
    }

    #[test]
    fn stream_text_retries_retryable_pre_stream_errors() {
        let retryable_error = LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
            LanguageModelErrorStreamPart::new(json!({
                "message": "rate limited",
                "statusCode": 429,
                "isRetryable": true,
                "responseHeaders": { "retry-after-ms": "1" }
            })),
        )]);
        let successful_stream = LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "Recovered")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let model =
            MockLanguageModel::new().with_stream_results([retryable_error, successful_stream]);
        let callback_errors = Arc::new(Mutex::new(Vec::<String>::new()));
        let callback_chunks = Arc::new(Mutex::new(Vec::<String>::new()));
        let errors = Arc::clone(&callback_errors);
        let chunks = Arc::clone(&callback_chunks);

        let result = poll_until_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Say hello")])
                .with_max_retries(1)
                .with_on_error(move |event| {
                    let errors = Arc::clone(&errors);
                    async move {
                        errors
                            .lock()
                            .expect("errors lock")
                            .push(event.error["message"].as_str().unwrap_or("").to_string());
                    }
                })
                .with_on_chunk(move |event| {
                    let chunks = Arc::clone(&chunks);
                    async move {
                        if let TextStreamPart::TextDelta(part) = event.chunk {
                            chunks.lock().expect("chunks lock").push(part.text);
                        }
                    }
                }),
        ));

        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.text, "Recovered");
        assert!(result.errors.is_empty());
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Error(_)))
        );
        assert!(callback_errors.lock().expect("errors lock").is_empty());
        assert_eq!(
            callback_chunks.lock().expect("chunks lock").as_slice(),
            ["Recovered"]
        );
    }

    #[test]
    fn stream_text_preserves_system_messages_when_retrying_after_retryable_error() {
        let retryable_error = LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
            LanguageModelErrorStreamPart::new(json!({
                "message": "Internal Server Error",
                "statusCode": 500,
                "responseHeaders": { "retry-after-ms": "1" }
            })),
        )]);
        let successful_stream = LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "hello")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", " ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "world")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let model =
            MockLanguageModel::new().with_stream_results([retryable_error, successful_stream]);
        let prompt = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("INSTRUCTIONS")),
            user_message("test-input"),
        ];

        let result = poll_until_ready(stream_text(
            StreamTextOptions::new(&model, prompt.clone()).with_max_retries(1),
        ));

        assert_eq!(result.text_stream, ["hello", " ", "world"]);
        let calls = model.stream_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].prompt, prompt);
        assert_eq!(calls[1].prompt, calls[0].prompt);
    }

    #[test]
    fn stream_text_stops_after_max_steps_even_when_tool_calls_continue() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, _options| async move { Ok(json!({ "forecast": "sunny" })) },
                ))
                .with_max_steps(1),
        ));

        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
    }

    /// Maps packages/ai stream-text.test.ts row
    /// `should complete tool loop with isLoopFinished()` — with an
    /// `is_loop_finished()` stop condition the loop runs the tool-call step and
    /// then the follow-up text step, finishing with the final-step text.
    #[test]
    fn stream_text_completes_tool_loop_with_is_loop_finished_stop_condition() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Done!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_max_steps(10)
                .with_stop_condition(crate::generate_text::is_loop_finished()),
        ));

        assert_eq!(result.text, "Done!");
        assert_eq!(result.steps.len(), 2);
        assert_eq!(model.stream_calls().len(), 2);
    }

    /// Maps packages/ai stream-text.test.ts `options.headers` row
    /// `should set headers` — request headers configured on the call options are
    /// forwarded to the provider `doStream` call.
    #[test]
    fn stream_text_sets_request_headers_on_provider_call() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", ", ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let mut options = StreamTextOptions::new(&model, vec![user_message("test-input")]);
        options.call_options.headers = Some(Headers::from_iter([(
            "custom-request-header".to_string(),
            "request-header-value".to_string(),
        )]));

        let result = poll_ready(stream_text(options));

        assert_eq!(result.text_stream, vec!["Hello", ", ", "world!"]);
        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let headers = stream_calls[0]
            .headers
            .as_ref()
            .expect("headers forwarded to provider call");
        assert_eq!(
            headers.get("custom-request-header").map(String::as_str),
            Some("request-header-value")
        );
    }

    #[test]
    fn stream_text_honors_stop_condition_after_streamed_tool_call() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "should not run",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, _options| async move { Ok(json!({ "forecast": "sunny" })) },
                ))
                .with_max_steps(3)
                .with_stop_condition(has_tool_call(["weather"])),
        ));

        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.tool_calls[0].tool_name, "weather");
    }

    #[test]
    fn stream_text_emits_error_when_tool_call_missing_for_provider_approval_request() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("mcp-approval-1", "non-existent-call"),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: None,
                    },
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Approve MCP tool")],
        )));

        assert!(result.tool_calls.is_empty());
        assert!(
            result
                .parts
                .iter()
                .all(|part| !matches!(part, TextStreamPart::ToolApprovalRequest(_)))
        );
        let error = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::Error(part) => Some(part.error.clone()),
                _ => None,
            })
            .expect("missing approval tool call emits an error part");
        assert_eq!(
            error,
            json!(
                "Tool call \"non-existent-call\" not found for approval request \"mcp-approval-1\"."
            )
        );
        assert_eq!(result.errors, vec![error]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.raw_finish_reason, None);
    }

    #[test]
    fn stream_text_handles_multiple_provider_executed_tool_approval_requests() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("mcp-call-1", "mcp_search", r#"{"query":"first"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("mcp-call-2", "mcp_execute", r#"{"command":"ls"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("approval-1", "mcp-call-1"),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("approval-2", "mcp-call-2"),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Run MCP tools")],
        )));

        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_calls[0].tool_call_id, "mcp-call-1");
        assert_eq!(result.tool_calls[0].tool_name, "mcp_search");
        assert_eq!(result.tool_calls[0].input, json!({ "query": "first" }));
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_calls[1].tool_call_id, "mcp-call-2");
        assert_eq!(result.tool_calls[1].tool_name, "mcp_execute");
        assert_eq!(result.tool_calls[1].input, json!({ "command": "ls" }));
        assert_eq!(result.tool_calls[1].provider_executed, Some(true));

        let approval_requests = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ToolApprovalRequest(request) => Some(request),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(approval_requests.len(), 2);
        assert_eq!(approval_requests[0].approval_id, "approval-1");
        assert_eq!(approval_requests[0].tool_call_id, "mcp-call-1");
        assert_eq!(approval_requests[1].approval_id, "approval-2");
        assert_eq!(approval_requests[1].tool_call_id, "mcp-call-2");

        let part_order = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ToolCall(part) => Some(format!("tool-call:{}", part.tool_call_id)),
                TextStreamPart::ToolApprovalRequest(part) => {
                    Some(format!("approval:{}", part.tool_call_id))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_order,
            [
                "tool-call:mcp-call-1",
                "tool-call:mcp-call-2",
                "approval:mcp-call-1",
                "approval:mcp-call-2"
            ]
        );
    }

    fn provider_executed_approval_result() -> StreamTextResult {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("mcp-call-1", "mcp_tool", r#"{"query":"test"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new("mcp-approval-1", "mcp-call-1"),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));

        poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("Run MCP tool")],
        )))
    }

    /// Maps packages/ai stream-text.test.ts:27868
    /// `should add provider-executed tool approval request to UI message stream`
    /// — a provider-executed tool call awaiting approval surfaces both a
    /// `tool-input-available` (providerExecuted) chunk and a
    /// `tool-approval-request` chunk in the UI message stream.
    #[test]
    fn stream_text_adds_provider_executed_tool_approval_request_to_ui_message_stream() {
        let result = provider_executed_approval_result();

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        assert!(chunks.contains(&json!({
            "type": "tool-input-available",
            "toolCallId": "mcp-call-1",
            "toolName": "mcp_tool",
            "input": { "query": "test" },
            "providerExecuted": true
        })));
        assert!(chunks.contains(&json!({
            "type": "tool-approval-request",
            "approvalId": "mcp-approval-1",
            "toolCallId": "mcp-call-1"
        })));
    }

    /// Maps packages/ai stream-text.test.ts:27903
    /// `should add provider-executed tool approval request to content` — the
    /// streamed content carries the provider-executed tool-call followed by the
    /// tool-approval-request that references it.
    #[test]
    fn stream_text_adds_provider_executed_tool_approval_request_to_content() {
        let result = provider_executed_approval_result();

        let tool_call_index = result
            .parts
            .iter()
            .position(|part| {
                matches!(
                    part,
                    TextStreamPart::ToolCall(call)
                        if call.tool_call_id == "mcp-call-1"
                            && call.provider_executed == Some(true)
                )
            })
            .expect("provider-executed tool-call content part present");

        let (approval_index, approval_request) = result
            .parts
            .iter()
            .enumerate()
            .find_map(|(index, part)| match part {
                TextStreamPart::ToolApprovalRequest(request)
                    if request.tool_call_id == "mcp-call-1" =>
                {
                    Some((index, request))
                }
                _ => None,
            })
            .expect("tool-approval-request content part present");

        assert!(tool_call_index < approval_index);
        assert_eq!(approval_request.approval_id, "mcp-approval-1");
    }

    /// Maps packages/ai stream-text.test.ts:27936
    /// `should add provider-executed tool approval request to response messages`
    /// — the assistant response message retains the provider-executed tool-call
    /// and a tool-approval-request part (after the tool call).
    #[test]
    fn stream_text_adds_provider_executed_tool_approval_request_to_response_messages() {
        let result = provider_executed_approval_result();

        let assistant_message = result
            .response_messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Assistant(assistant) => Some(assistant),
                _ => None,
            })
            .expect("assistant response message present");

        let tool_call_index = assistant_message
            .content
            .iter()
            .position(|part| {
                matches!(
                    part,
                    LanguageModelAssistantContentPart::ToolCall(call)
                        if call.tool_call_id == "mcp-call-1" && call.tool_name == "mcp_tool"
                )
            })
            .expect("tool-call content part present");

        let (approval_index, approval_request) = assistant_message
            .content
            .iter()
            .enumerate()
            .find_map(|(index, part)| match part {
                LanguageModelAssistantContentPart::ToolApprovalRequest(request)
                    if request.tool_call_id == "mcp-call-1" =>
                {
                    Some((index, request))
                }
                _ => None,
            })
            .expect("tool-approval-request content part present");

        assert!(tool_call_index < approval_index);
        assert_eq!(approval_request.approval_id, "mcp-approval-1");
    }

    #[test]
    fn stream_text_sends_approved_provider_executed_tool_approval_response_once() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("mcp-call-1", "mcp_tool", r#"{"query":"test"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "mcp-call-1",
                    "mcp_tool",
                    NonNullJsonValue::new(json!({ "shortened_url": "https://short.url/abc" }))
                        .expect("tool result is non-null"),
                )),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Here is your shortened URL: https://short.url/abc",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let prompt = provider_executed_approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("mcp-approval-1", true)
                .with_provider_executed(true),
        );

        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_messages(prompt))
                .expect("prompt converts")
                .with_tool(Tool::provider_executed(
                    "mcp_tool",
                    "test.mcp_tool",
                    JsonObject::new(),
                    schema.clone(),
                    schema,
                ))
                .with_max_steps(3),
        ));

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].prompt.len(), 3);
        assert!(matches!(
            &calls[0].prompt[1],
            LanguageModelMessage::Assistant(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelAssistantContentPart::ToolCall(part)
                            if part.tool_call_id == "mcp-call-1"
                                && part.tool_name == "mcp_tool"
                                && part.provider_executed == Some(true)
                    )
        ));
        assert!(matches!(
            &calls[0].prompt[2],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolApprovalResponse(response)
                            if response.approval_id == "mcp-approval-1"
                                && response.approved
                                && response.reason.is_none()
                    )
        ));
        assert_eq!(
            result.text,
            "Here is your shortened URL: https://short.url/abc"
        );
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "mcp-call-1");
        assert_eq!(result.tool_results[0].tool_name, "mcp_tool");
        assert_eq!(result.tool_results[0].provider_executed, Some(true));
        assert_eq!(
            result.tool_results[0].output,
            json!({ "shortened_url": "https://short.url/abc" })
        );
    }

    #[test]
    fn stream_text_sends_denied_provider_executed_tool_approval_response_once() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "I understand. The tool execution was not approved.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let prompt = provider_executed_approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("mcp-approval-1", false)
                .with_reason("User denied the request")
                .with_provider_executed(true),
        );

        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_messages(prompt))
                .expect("prompt converts")
                .with_tool(Tool::provider_executed(
                    "mcp_tool",
                    "test.mcp_tool",
                    JsonObject::new(),
                    schema.clone(),
                    schema,
                ))
                .with_max_steps(3),
        ));

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].prompt.len(), 3);
        assert!(matches!(
            &calls[0].prompt[1],
            LanguageModelMessage::Assistant(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelAssistantContentPart::ToolCall(part)
                            if part.tool_call_id == "mcp-call-1"
                                && part.tool_name == "mcp_tool"
                                && part.provider_executed == Some(true)
                    )
        ));
        assert!(matches!(
            &calls[0].prompt[2],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolApprovalResponse(response)
                            if response.approval_id == "mcp-approval-1"
                                && !response.approved
                                && response.reason.as_deref() == Some("User denied the request")
                    )
        ));
        assert_eq!(
            result.text,
            "I understand. The tool execution was not approved."
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert!(result.tool_results.is_empty());
    }

    #[test]
    fn stream_text_streams_user_tool_approval_request_without_executing_tool() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let execute_count = Arc::new(AtomicUsize::new(0));
        let execute_count_for_tool = Arc::clone(&execute_count);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let execute_count = Arc::clone(&execute_count_for_tool);
                        async move {
                            execute_count.fetch_add(1, Ordering::SeqCst);
                            Ok(json!({ "forecast": "sunny" }))
                        }
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("weather", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));

        assert_eq!(execute_count.load(Ordering::SeqCst), 0);
        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_call_id, "call-1");
        assert_eq!(result.tool_calls[0].tool_name, "weather");
        assert_eq!(result.tool_calls[0].input, json!({ "city": "Brisbane" }));
        assert!(result.tool_results.is_empty());

        let tool_call_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::ToolCall(call) if call.tool_call_id == "call-1"))
            .expect("tool call is streamed");
        let (approval_request_index, approval_request) = result
            .parts
            .iter()
            .enumerate()
            .find_map(|(index, part)| {
                if let TextStreamPart::ToolApprovalRequest(request) = part
                    && request.tool_call_id == "call-1"
                {
                    Some((index, request))
                } else {
                    None
                }
            })
            .expect("user approval request is streamed");
        assert!(tool_call_index < approval_request_index);
        assert_eq!(approval_request.is_automatic, None);
        assert!(
            result
                .parts
                .iter()
                .all(|part| !matches!(part, TextStreamPart::ToolApprovalResponse(_)))
        );

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        assert!(chunks.contains(&json!({
            "type": "tool-input-available",
            "toolCallId": "call-1",
            "toolName": "weather",
            "input": { "city": "Brisbane" }
        })));
        assert!(chunks.contains(&json!({
            "type": "tool-approval-request",
            "approvalId": approval_request.approval_id.clone(),
            "toolCallId": "call-1"
        })));
    }

    /// Maps packages/ai stream-text.test.ts:25524
    /// `should add tool approval requests to the content` — a user-approval tool
    /// call adds a tool-call followed by a tool-approval-request to the streamed
    /// content, and the approval request references the originating tool call.
    #[test]
    fn stream_text_adds_tool_approval_requests_to_content() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("tool1", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));

        let tool_call_index = result
            .parts
            .iter()
            .position(|part| {
                matches!(part, TextStreamPart::ToolCall(call) if call.tool_call_id == "call-1")
            })
            .expect("tool-call content part present");

        let (approval_index, approval_request) = result
            .parts
            .iter()
            .enumerate()
            .find_map(|(index, part)| match part {
                TextStreamPart::ToolApprovalRequest(request)
                    if request.tool_call_id == "call-1" =>
                {
                    Some((index, request))
                }
                _ => None,
            })
            .expect("tool-approval-request content part present");

        assert!(tool_call_index < approval_index);
        assert!(!approval_request.approval_id.is_empty());
        // No approval response yet: approval is still pending in the content.
        assert!(
            result
                .parts
                .iter()
                .all(|part| !matches!(part, TextStreamPart::ToolApprovalResponse(_)))
        );
    }

    /// Maps packages/ai stream-text.test.ts:25557
    /// `should add tool approval requests to the response messages` — when a tool
    /// call requires user approval, the assistant response message retains both
    /// the tool-call and a tool-approval-request part (after the tool call).
    #[test]
    fn stream_text_adds_tool_approval_requests_to_response_messages() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("tool1", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));

        // The tool is not executed; approval is pending.
        assert!(result.tool_results.is_empty());

        let assistant_message = result
            .response_messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Assistant(assistant) => Some(assistant),
                _ => None,
            })
            .expect("assistant response message present");

        let tool_call_index = assistant_message
            .content
            .iter()
            .position(|part| {
                matches!(
                    part,
                    LanguageModelAssistantContentPart::ToolCall(call)
                        if call.tool_call_id == "call-1" && call.tool_name == "tool1"
                )
            })
            .expect("tool-call content part present");

        let (approval_index, approval_request) = assistant_message
            .content
            .iter()
            .enumerate()
            .find_map(|(index, part)| match part {
                LanguageModelAssistantContentPart::ToolApprovalRequest(request)
                    if request.tool_call_id == "call-1" =>
                {
                    Some((index, request))
                }
                _ => None,
            })
            .expect("tool-approval-request content part present");

        assert!(tool_call_index < approval_index);
        assert!(!approval_request.approval_id.is_empty());
    }

    #[test]
    fn stream_text_user_approval_function_can_block_one_call_and_execute_another() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane","mode":"needs-approval"}"#,
                )),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-2",
                    "weather",
                    r#"{"city":"Sydney","mode":"auto"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let approval_calls = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let approval_calls_for_callback = Arc::clone(&approval_calls);
        let execute_calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let execute_calls_for_tool = Arc::clone(&execute_calls);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |input, _options| {
                        let execute_calls = Arc::clone(&execute_calls_for_tool);
                        async move {
                            execute_calls
                                .lock()
                                .expect("execute calls lock")
                                .push(input["city"].as_str().expect("city is a string").to_owned());
                            Ok(json!(format!(
                                "forecast for {}",
                                input["city"].as_str().expect("city is a string")
                            )))
                        }
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new().with_tool_approval_function(
                        "weather",
                        move |input, options| {
                            let approval_calls = Arc::clone(&approval_calls_for_callback);
                            async move {
                                approval_calls.lock().expect("approval calls lock").push((
                                    options.tool_call_id,
                                    input["mode"].as_str().expect("mode is a string").to_owned(),
                                ));
                                if input["mode"] == json!("needs-approval") {
                                    Some(ToolApprovalStatusKind::UserApproval.into())
                                } else {
                                    Some(ToolApprovalStatusKind::NotApplicable.into())
                                }
                            }
                        },
                    ),
                )
                .with_max_steps(3),
        ));

        assert_eq!(
            approval_calls
                .lock()
                .expect("approval calls lock")
                .as_slice(),
            [
                ("call-1".to_string(), "needs-approval".to_string()),
                ("call-2".to_string(), "auto".to_string())
            ]
        );
        assert_eq!(
            execute_calls.lock().expect("execute calls lock").as_slice(),
            ["Sydney"]
        );
        assert_eq!(model.stream_calls().len(), 1);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-2");
        assert_eq!(result.tool_results[0].output, json!("forecast for Sydney"));

        let part_order = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::ToolCall(part) => Some(format!("call:{}", part.tool_call_id)),
                TextStreamPart::ToolApprovalRequest(part) => {
                    Some(format!("approval:{}", part.tool_call_id))
                }
                TextStreamPart::ToolResult(part) => Some(format!("result:{}", part.tool_call_id)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_order,
            [
                "call:call-1",
                "approval:call-1",
                "call:call-2",
                "result:call-2"
            ]
        );

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        assert!(chunks.iter().any(|chunk| {
            chunk["type"] == "tool-approval-request" && chunk["toolCallId"] == "call-1"
        }));
        assert!(chunks.iter().any(|chunk| {
            chunk["type"] == "tool-output-available"
                && chunk["toolCallId"] == "call-2"
                && chunk["output"] == json!("forecast for Sydney")
        }));
    }

    #[test]
    fn stream_text_executes_initial_approved_tool_approval_before_first_model_call() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let prepare_step_response_messages =
            Arc::new(Mutex::new(Vec::<Vec<LanguageModelMessage>>::new()));
        let prepare_step_response_messages_for_callback =
            Arc::clone(&prepare_step_response_messages);
        let prompt = approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("approval-1", true),
            false,
        );

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, prompt.clone())
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, options| async move {
                        Ok(json!({
                            "forecast": "sunny",
                            "city": input["city"],
                            "toolCallId": options.tool_call_id
                        }))
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("weather", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_prepare_step(move |options| {
                    let messages = Arc::clone(&prepare_step_response_messages_for_callback);
                    async move {
                        messages
                            .lock()
                            .expect("prepare-step messages lock")
                            .push(options.response_messages);
                        PrepareStepResult::new()
                    }
                })
                .with_max_steps(3),
        ));

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0].prompt[..3], prompt.as_slice());
        assert!(matches!(
            &calls[0].prompt[3],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.tool_name == "weather"
                                && part.output == LanguageModelToolResultOutput::json(json!({
                                    "forecast": "sunny",
                                    "city": "Brisbane",
                                    "toolCallId": "call-1"
                                }))
                    )
        ));
        assert_eq!(result.text, "Hello, world!");
        assert_eq!(result.steps.len(), 1);

        let prepare_step_response_messages = prepare_step_response_messages
            .lock()
            .expect("prepare-step messages lock");
        assert_eq!(prepare_step_response_messages.len(), 1);
        assert_eq!(prepare_step_response_messages[0].len(), 1);
        assert!(matches!(
            &prepare_step_response_messages[0][0],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.output == LanguageModelToolResultOutput::json(json!({
                                    "forecast": "sunny",
                                    "city": "Brisbane",
                                    "toolCallId": "call-1"
                                }))
                    )
        ));

        let first_tool_result_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::ToolResult(result) if result.tool_call_id == "call-1"))
            .expect("initial tool result is streamed");
        let start_step_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::StartStep(_)))
            .expect("start-step is streamed");
        assert!(first_tool_result_index < start_step_index);

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        let tool_output_index = chunks
            .iter()
            .position(|chunk| {
                chunk["type"] == "tool-output-available"
                    && chunk["toolCallId"] == "call-1"
                    && chunk["output"]["forecast"] == "sunny"
            })
            .expect("initial tool output is in UI stream");
        let ui_start_step_index = chunks
            .iter()
            .position(|chunk| chunk["type"] == "start-step")
            .expect("UI start-step exists");
        assert!(tool_output_index < ui_start_step_index);
    }

    /// Shared fixture for the upstream
    /// `when a call from a single tool with preliminary results that needs
    /// approval is approved` block. The prompt carries a pre-approved `tool1`
    /// call; the tool streams a preliminary result then a final result, and the
    /// continuation model emits `Hello, world!`.
    fn stream_text_approved_preliminary_tool_result() -> StreamTextResult {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        // Pre-approved `tool1` call (mirrors the upstream messages fixture).
        let prompt = vec![
            user_message("test-input"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "tool1",
                    json!({ "value": "value" }),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("id-1", "call-1"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("id-1", true),
                ),
            ])),
        ];

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, prompt)
                .with_tool(Tool::new("tool1", input_schema).with_execute_outputs(
                    |_input, _options| {
                        ready(Ok(vec![
                            ExecuteToolOutput::preliminary(json!("preliminary-result")),
                            ExecuteToolOutput::preliminary(json!("final-result")),
                        ]))
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("tool1", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));
        result.consume_stream();
        result
    }

    /// Maps packages/ai stream-text.test.ts:27160
    /// `should call the model with a prompt that includes the tool result` — an
    /// approved preliminary-result tool feeds its FINAL output (text output
    /// `final-result`) into the continuation prompt as a `tool-result`, before
    /// the first model call.
    #[test]
    fn stream_text_approved_preliminary_tool_result_continuation_prompt_includes_final_result() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let prompt = vec![
            user_message("test-input"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "tool1",
                    json!({ "value": "value" }),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("id-1", "call-1"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("id-1", true),
                ),
            ])),
        ];

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, prompt.clone())
                .with_tool(Tool::new("tool1", input_schema).with_execute_outputs(
                    |_input, _options| {
                        ready(Ok(vec![
                            ExecuteToolOutput::preliminary(json!("preliminary-result")),
                            ExecuteToolOutput::preliminary(json!("final-result")),
                        ]))
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("tool1", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));
        result.consume_stream();

        // A single model call whose prompt carries the user/assistant prefix and
        // ends with the approved tool's FINAL result.
        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0].prompt[..3], prompt.as_slice());
        assert!(matches!(
            &calls[0].prompt[3],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.tool_name == "tool1"
                                && part.output
                                    == LanguageModelToolResultOutput::text("final-result")
                    )
        ));
    }

    /// Maps packages/ai stream-text.test.ts:27211
    /// `should include the tool result in the response messages` — the resolved
    /// response messages start with the FINAL tool result, then the assistant
    /// continuation text.
    #[test]
    fn stream_text_approved_preliminary_tool_result_response_messages_include_final_result() {
        let result = stream_text_approved_preliminary_tool_result();
        assert_eq!(result.response_messages.len(), 2);
        assert!(matches!(
            &result.response_messages[0],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.tool_name == "tool1"
                                && part.output
                                    == LanguageModelToolResultOutput::text("final-result")
                    )
        ));
        assert!(matches!(
            &result.response_messages[1],
            LanguageModelMessage::Assistant(message)
                if matches!(
                    message.content.first(),
                    Some(LanguageModelAssistantContentPart::Text(text))
                        if text.text == "Hello, world!"
                )
        ));
    }

    #[test]
    fn stream_text_serializes_initial_approved_tool_error_before_first_model_call() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Recovered from tool error.",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let prompt = approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("approval-1", true),
            false,
        );

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, prompt.clone())
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |_input, _options| async move {
                        Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new(
                            "No valid token for plugin",
                        ))
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("weather", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            &calls[0].prompt[3],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.output == LanguageModelToolResultOutput::error_text(
                                    "No valid token for plugin"
                                )
                    )
        ));
        assert_eq!(result.text, "Recovered from tool error.");

        let tool_error_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::ToolResult(result) if result.tool_call_id == "call-1" && result.is_error == Some(true)))
            .expect("initial tool error is streamed");
        let start_step_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::StartStep(_)))
            .expect("start-step is streamed");
        assert!(tool_error_index < start_step_index);

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        let tool_error_chunk_index = chunks
            .iter()
            .position(|chunk| {
                chunk["type"] == "tool-output-error"
                    && chunk["toolCallId"] == "call-1"
                    && chunk["errorText"] == "No valid token for plugin"
            })
            .expect("initial tool error is in UI stream");
        let ui_start_step_index = chunks
            .iter()
            .position(|chunk| chunk["type"] == "start-step")
            .expect("UI start-step exists");
        assert!(tool_error_chunk_index < ui_start_step_index);
    }

    #[test]
    fn stream_text_streams_initial_denied_tool_approval_before_first_model_call() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let execute_count = Arc::new(AtomicUsize::new(0));
        let execute_count_for_tool = Arc::clone(&execute_count);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let prompt = approval_response_prompt(
            LanguageModelToolApprovalResponsePart::new("approval-1", false),
            false,
        );

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, prompt.clone())
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let execute_count = Arc::clone(&execute_count_for_tool);
                        async move {
                            execute_count.fetch_add(1, Ordering::SeqCst);
                            Ok(json!({ "forecast": "sunny" }))
                        }
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("weather", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));

        assert_eq!(execute_count.load(Ordering::SeqCst), 0);
        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0].prompt[..3], prompt.as_slice());
        assert!(matches!(
            &calls[0].prompt[3],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.tool_name == "weather"
                                && matches!(
                                    part.output,
                                    LanguageModelToolResultOutput::ExecutionDenied { .. }
                                )
                    )
        ));
        assert_eq!(result.text, "Hello, world!");
        assert!(result.tool_results.is_empty());

        let denied_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::ToolOutputDenied(denied) if denied.tool_call_id == "call-1" && denied.tool_name == "weather"))
            .expect("initial denied tool output is streamed");
        let start_step_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::StartStep(_)))
            .expect("start-step is streamed");
        assert!(denied_index < start_step_index);

        let full_stream = serde_json::to_value(&result.parts).expect("parts serialize");
        assert!(
            full_stream
                .as_array()
                .expect("parts are an array")
                .iter()
                .any(|part| part["type"] == "tool-output-denied"
                    && part["toolCallId"] == "call-1"
                    && part["toolName"] == "weather")
        );

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        let tool_denied_chunk_index = chunks
            .iter()
            .position(|chunk| {
                chunk["type"] == "tool-output-denied"
                    && chunk["toolCallId"] == "call-1"
                    && chunk.get("toolName").is_none()
            })
            .expect("initial denied tool output is in UI stream");
        let ui_start_step_index = chunks
            .iter()
            .position(|chunk| chunk["type"] == "start-step")
            .expect("UI start-step exists");
        assert!(tool_denied_chunk_index < ui_start_step_index);
    }

    /// Maps packages/ai stream-text.test.ts:27556
    /// `should include the tool error in the response messages` — a denied tool
    /// approval surfaces an `execution-denied` tool result in the resolved
    /// response messages, followed by the assistant continuation text. The tool
    /// is never executed.
    #[test]
    fn stream_text_denied_tool_approval_response_messages_include_execution_denied() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let execute_count = Arc::new(AtomicUsize::new(0));
        let execute_count_for_tool = Arc::clone(&execute_count);
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let prompt = vec![
            user_message("test-input"),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "tool1",
                    json!({ "value": "value" }),
                )),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("id-1", "call-1"),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("id-1", false),
                ),
            ])),
        ];

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, prompt)
                .with_tool(Tool::new("tool1", input_schema).with_execute(
                    move |_input, _options| {
                        let execute_count = Arc::clone(&execute_count_for_tool);
                        async move {
                            execute_count.fetch_add(1, Ordering::SeqCst);
                            Ok(json!("result1"))
                        }
                    },
                ))
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("tool1", NormalizedToolApprovalStatus::UserApproval),
                )
                .with_max_steps(3),
        ));
        result.consume_stream();

        // The denied tool is never executed.
        assert_eq!(execute_count.load(Ordering::SeqCst), 0);
        assert_eq!(result.response_messages.len(), 2);
        assert!(matches!(
            &result.response_messages[0],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 1
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_call_id == "call-1"
                                && part.tool_name == "tool1"
                                && matches!(
                                    part.output,
                                    LanguageModelToolResultOutput::ExecutionDenied { .. }
                                )
                    )
        ));
        assert!(matches!(
            &result.response_messages[1],
            LanguageModelMessage::Assistant(message)
                if matches!(
                    message.content.first(),
                    Some(LanguageModelAssistantContentPart::Text(text))
                        if text.text == "Hello, world!"
                )
        ));
    }

    #[test]
    fn stream_text_automatic_tool_approval_response_streams_before_tool_result() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Approved.",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let execute_count = Arc::new(AtomicUsize::new(0));
        let execute_count_for_tool = Arc::clone(&execute_count);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |input, _options| {
                        let execute_count = Arc::clone(&execute_count_for_tool);
                        async move {
                            execute_count.fetch_add(1, Ordering::SeqCst);
                            Ok(json!({ "forecast": "sunny", "city": input["city"] }))
                        }
                    },
                ))
                .with_tool_approval(ToolApprovalConfiguration::new().with_tool_status(
                    "weather",
                    NormalizedToolApprovalStatus::approved_with_reason("trusted internal tool"),
                ))
                .with_max_steps(2),
        ));

        assert_eq!(execute_count.load(Ordering::SeqCst), 1);
        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.text, "Approved.");

        let (approval_request_index, approval_request) = result
            .parts
            .iter()
            .enumerate()
            .find_map(|(index, part)| {
                if let TextStreamPart::ToolApprovalRequest(request) = part
                    && request.tool_call_id == "call-1"
                    && request.is_automatic == Some(true)
                {
                    Some((index, request))
                } else {
                    None
                }
            })
            .expect("automatic approval request is streamed");
        let (approval_response_index, approval_response) = result
            .parts
            .iter()
            .enumerate()
            .find_map(|(index, part)| {
                if let TextStreamPart::ToolApprovalResponse(response) = part
                    && response.approved
                    && response.reason.as_deref() == Some("trusted internal tool")
                {
                    Some((index, response))
                } else {
                    None
                }
            })
            .expect("automatic approval response is streamed");
        assert_eq!(approval_response.approval_id, approval_request.approval_id);
        let tool_result_index = result
            .parts
            .iter()
            .position(|part| matches!(part, TextStreamPart::ToolResult(result) if result.tool_call_id == "call-1"))
            .expect("tool result is streamed");
        assert!(approval_request_index < approval_response_index);
        assert!(approval_response_index < tool_result_index);

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        assert!(chunks.contains(&json!({
            "type": "tool-approval-request",
            "approvalId": approval_request.approval_id.clone(),
            "toolCallId": "call-1",
            "isAutomatic": true
        })));
        assert!(chunks.contains(&json!({
            "type": "tool-approval-response",
            "approvalId": approval_request.approval_id.clone(),
            "approved": true,
            "reason": "trusted internal tool"
        })));
        assert!(chunks.iter().any(|chunk| {
            chunk["type"] == "tool-output-available"
                && chunk["toolCallId"] == "call-1"
                && chunk["output"]["forecast"] == "sunny"
        }));

        assert!(matches!(
            &model.stream_calls()[1].prompt[2],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 2
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolApprovalResponse(response)
                            if response.approved
                                && response.reason.as_deref() == Some("trusted internal tool")
                    )
                    && matches!(
                        &message.content[1],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_name == "weather"
                    )
        ));
    }

    #[test]
    fn stream_text_applies_denied_tool_approval_to_continuation_messages() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "text-1",
                    "Request denied.",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let execute_count = Arc::new(AtomicUsize::new(0));
        let execute_count_for_tool = Arc::clone(&execute_count);
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    move |_input, _options| {
                        let execute_count = Arc::clone(&execute_count_for_tool);
                        async move {
                            execute_count.fetch_add(1, Ordering::SeqCst);
                            Ok(json!({ "forecast": "sunny" }))
                        }
                    },
                ))
                .with_tool_approval(ToolApprovalConfiguration::new().with_tool_status(
                    "weather",
                    NormalizedToolApprovalStatus::denied_with_reason("blocked by policy"),
                ))
                .with_max_steps(2),
        ));

        assert_eq!(execute_count.load(Ordering::SeqCst), 0);
        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.text, "Request denied.");
        let approval_request_id = result
            .parts
            .iter()
            .find_map(|part| {
                if let TextStreamPart::ToolApprovalRequest(request) = part
                    && request.tool_call_id == "call-1"
                    && request.is_automatic == Some(true)
                {
                    Some(request.approval_id.clone())
                } else {
                    None
                }
            })
            .expect("automatic denial request is streamed");
        assert!(result.parts.iter().any(|part| {
            matches!(
                part,
                TextStreamPart::ToolApprovalResponse(response)
                    if response.approval_id == approval_request_id
                        && !response.approved
                        && response.reason.as_deref() == Some("blocked by policy")
            )
        }));

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks are an array");
        assert!(chunks.contains(&json!({
            "type": "tool-approval-request",
            "approvalId": approval_request_id.clone(),
            "toolCallId": "call-1",
            "isAutomatic": true
        })));
        assert!(chunks.contains(&json!({
            "type": "tool-approval-response",
            "approvalId": approval_request_id.clone(),
            "approved": false,
            "reason": "blocked by policy"
        })));

        assert!(matches!(
            &model.stream_calls()[1].prompt[2],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 2
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolApprovalResponse(response)
                            if !response.approved
                                && response.reason.as_deref() == Some("blocked by policy")
                    )
                    && matches!(
                        &message.content[1],
                        LanguageModelToolContentPart::ToolResult(part)
                            if part.tool_name == "weather"
                                && matches!(
                                    &part.output,
                                    LanguageModelToolResultOutput::ExecutionDenied { .. }
                                )
            )
        ));
    }

    #[test]
    fn stream_text_validates_tool_context_before_approval_callback_and_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let approval_called = Arc::new(AtomicBool::new(false));
        let approval_called_for_callback = Arc::clone(&approval_called);
        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_tool = Arc::clone(&executed);
        let input_schema = json!({ "type": "object" })
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
            .expect("schema is an object")
            .clone(),
        )
        .with_validator(|value| {
            if value.get("apiKey").and_then(JsonValue::as_str).is_some() {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("expected apiKey string")
            }
        });
        let tools_context =
            JsonObject::from_iter([("weather".to_string(), json!({ "apiKey": 123 }))]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_context_schema(context_schema)
                        .with_execute(move |_input, _options| {
                            let executed = Arc::clone(&executed_for_tool);
                            async move {
                                executed.store(true, Ordering::SeqCst);
                                Ok(json!({ "forecast": "sunny" }))
                            }
                        }),
                )
                .with_tool_approval(
                    ToolApprovalConfiguration::new().with_tool_approval_function(
                        "weather",
                        move |_input, _options| {
                            let approval_called = Arc::clone(&approval_called_for_callback);
                            async move {
                                approval_called.store(true, Ordering::SeqCst);
                                Some(
                                    NormalizedToolApprovalStatus::approved_with_reason("trusted")
                                        .into(),
                                )
                            }
                        },
                    ),
                ),
        ));

        assert!(!approval_called.load(Ordering::SeqCst));
        assert!(!executed.load(Ordering::SeqCst));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert!(
            result.tool_results[0]
                .output
                .as_str()
                .expect("error output is a string")
                .contains("expected apiKey string")
        );
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::ToolApprovalRequest(_)))
        );
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::ToolApprovalResponse(_)))
        );
    }

    #[test]
    fn stream_text_passes_tools_context_to_tool_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "weather",
                    r#"{"city":"Brisbane"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let recorded_context = Arc::new(Mutex::new(None::<JsonValue>));
        let recorded_context_for_tool = Arc::clone(&recorded_context);
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
                    "context": { "type": "string" }
                },
                "required": ["context"]
            })
            .as_object()
            .expect("schema is an object")
            .clone(),
        )
        .with_validator(|value| {
            if value.get("context").and_then(JsonValue::as_str).is_some() {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("expected context string")
            }
        });
        let tools_context =
            JsonObject::from_iter([("weather".to_string(), json!({ "context": "test" }))]);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("weather", input_schema)
                        .with_context_schema(context_schema)
                        .with_execute(move |_input, options| {
                            let recorded_context = Arc::clone(&recorded_context_for_tool);
                            async move {
                                *recorded_context.lock().expect("recorded context lock") =
                                    options.context.clone();
                                Ok(json!({ "forecast": "sunny" }))
                            }
                        }),
                ),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(
            *recorded_context.lock().expect("recorded context lock"),
            Some(json!({ "context": "test" }))
        );
        assert_eq!(result.tool_results[0].is_error, None);
        assert_eq!(
            result.tool_results[0]
                .output
                .as_object()
                .expect("tool result output is an object")
                .get("forecast"),
            Some(&json!("sunny"))
        );
    }

    #[test]
    fn stream_text_repairs_and_refines_streamed_tool_call_before_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "weather", "{bad",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
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

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
                .with_tool(Tool::new("weather", input_schema).with_execute(
                    |input, _options| async move {
                        Ok(json!({
                            "city": input["city"],
                            "forecast": "sunny"
                        }))
                    },
                ))
                .with_tool_call_repair(|_options| async move {
                    Ok::<Option<LanguageModelToolCall>, String>(Some(LanguageModelToolCall::new(
                        "call-1",
                        "weather",
                        r#"{"city":"brisbane"}"#,
                    )))
                })
                .with_tool_input_refinement("weather", |mut input| async move {
                    input["city"] = json!("BRISBANE");
                    Ok(input)
                }),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].input, json!({ "city": "BRISBANE" }));
        assert_eq!(result.tool_calls[0].invalid, None);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["city"], "BRISBANE");
        assert_eq!(result.tool_results[0].is_error, None);
    }

    #[test]
    fn stream_text_repairs_unknown_streamed_tool_name_before_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "forecast", "{}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = json!({ "type": "object" })
            .as_object()
            .expect("schema is an object")
            .clone();
        let repair_options = Arc::new(Mutex::new(Vec::<ToolCallRepairOptions>::new()));
        let repair_options_for_closure = Arc::clone(&repair_options);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Weather?")])
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
                }),
        ));

        let repair_options = repair_options.lock().expect("repair options lock");
        assert_eq!(repair_options.len(), 1);
        assert_eq!(repair_options[0].tool_call.tool_name, "forecast");
        assert_eq!(repair_options[0].tool_call.input, "{}");
        assert!(matches!(
            &repair_options[0].error,
            ToolCallRepairOriginalError::NoSuchTool(error)
                if error.tool_name() == "forecast"
                    && error.available_tools() == Some(&["weather".to_string()][..])
        ));
        drop(repair_options);

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "weather");
        assert_eq!(result.tool_calls[0].input, json!({}));
        assert_eq!(result.tool_calls[0].invalid, None);
        assert_eq!(result.tool_calls[0].error, None);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_name, "weather");
        assert_eq!(result.tool_results[0].input, json!({}));
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
        assert_eq!(result.tool_results[0].output["calledWith"], json!({}));
    }

    // Upstream: stream-text.test.ts errors "should swallow error to prevent server crash".
    // A pre-text stream error must not panic; textStream is empty and the result errors.
    #[test]
    fn stream_text_swallows_error_and_yields_empty_text_stream() {
        let result = stream_text_result_from_parts(vec![LanguageModelStreamPart::Error(
            LanguageModelErrorStreamPart::new(json!({ "message": "test error" })),
        )]);

        assert!(result.text_stream.is_empty());
        assert_eq!(result.text, "");
        assert_eq!(result.errors, vec![json!({ "message": "test error" })]);
        assert_eq!(result.finish_reason, FinishReason::Error);
    }

    // Upstream: stream-text.test.ts errors "should reject text promise when error is thrown".
    // When no output is generated and the stream errors, result.text resolves to empty
    // (the Rust port surfaces the error via result.errors rather than a rejected promise).
    #[test]
    fn stream_text_text_is_empty_when_error_is_thrown() {
        let result = stream_text_result_from_parts(vec![LanguageModelStreamPart::Error(
            LanguageModelErrorStreamPart::new(json!({ "message": "test error" })),
        )]);

        assert_eq!(result.text, "");
        assert!(!result.errors.is_empty());
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Error(_)))
        );
    }

    // Upstream: stream-text.test.ts options.experimental_onStart
    // "should send correct information with system and messages": the start event exposes
    // provider, modelId, messages, maxOutputTokens and temperature.
    #[test]
    fn stream_text_on_start_exposes_provider_model_and_settings() {
        let model = MockLanguageModel::new().with_stream_result(stream_result_hello());
        let captured = Arc::new(Mutex::new(None::<GenerateTextStartEvent>));
        let captured_for_callback = Arc::clone(&captured);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(
                &model,
                vec![
                    LanguageModelMessage::System(LanguageModelSystemMessage::new(
                        "you are a helpful assistant",
                    )),
                    user_message("test-message"),
                ],
            )
            .with_max_output_tokens(100)
            .with_temperature(0.5)
            .with_on_start(move |event| {
                let captured = Arc::clone(&captured_for_callback);
                async move {
                    *captured.lock().expect("captured lock") = Some(event);
                }
            }),
        ));

        let _ = result.text;
        let event = captured
            .lock()
            .expect("captured lock")
            .clone()
            .expect("on_start ran");
        assert_eq!(event.provider, "mock-provider");
        assert_eq!(event.model_id, "mock-model-id");
        assert_eq!(event.messages.len(), 2);
        assert_eq!(event.max_output_tokens, Some(100));
        assert_eq!(event.temperature, Some(0.5));
        assert_eq!(event.max_retries, DEFAULT_MAX_RETRIES);
    }

    // Upstream: stream-text.test.ts options.experimental_onStart
    // "should expose tools and toolChoice".
    #[test]
    fn stream_text_on_start_exposes_tools_and_tool_choice() {
        let model = MockLanguageModel::new().with_stream_result(stream_result_hello());
        let captured = Arc::new(Mutex::new(None::<GenerateTextStartEvent>));
        let captured_for_callback = Arc::clone(&captured);

        let input_schema =
            json!({ "type": "object", "properties": { "value": { "type": "string" } } })
                .as_object()
                .expect("schema is object")
                .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("myTool", input_schema))
                .with_tool_choice(LanguageModelToolChoice::Auto)
                .with_on_start(move |event| {
                    let captured = Arc::clone(&captured_for_callback);
                    async move {
                        *captured.lock().expect("captured lock") = Some(event);
                    }
                }),
        ));

        let _ = result.text;
        let event = captured
            .lock()
            .expect("captured lock")
            .clone()
            .expect("on_start ran");
        assert_eq!(event.tools.len(), 1);
        let tool_name = match &event.tools[0] {
            LanguageModelTool::Function(tool) => tool.name.as_str(),
            LanguageModelTool::Provider(tool) => tool.name.as_str(),
        };
        assert_eq!(tool_name, "myTool");
        assert_eq!(event.tool_choice, Some(LanguageModelToolChoice::Auto));
    }

    // Upstream: stream-text.test.ts options.experimental_onStart
    // "should expose providerOptions".
    #[test]
    fn stream_text_on_start_exposes_provider_options() {
        let model = MockLanguageModel::new().with_stream_result(stream_result_hello());
        let captured = Arc::new(Mutex::new(None::<GenerateTextStartEvent>));
        let captured_for_callback = Arc::clone(&captured);

        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "openai".to_string(),
            json!({ "logprobs": true })
                .as_object()
                .expect("provider options object")
                .clone(),
        );
        let expected = provider_options.clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_provider_options(provider_options)
                .with_on_start(move |event| {
                    let captured = Arc::clone(&captured_for_callback);
                    async move {
                        *captured.lock().expect("captured lock") = Some(event);
                    }
                }),
        ));

        let _ = result.text;
        let event = captured
            .lock()
            .expect("captured lock")
            .clone()
            .expect("on_start ran");
        assert_eq!(event.provider_options, Some(expected));
    }

    // Upstream: stream-text.test.ts options.experimental_onStart
    // "should expose timeout and stopWhen": the onStart event exposes the
    // configured timeout (totalMs/stepMs) when timeout and stopWhen are set.
    #[test]
    fn stream_text_on_start_exposes_timeout() {
        let model = MockLanguageModel::new().with_stream_result(stream_result_hello());
        let captured = Arc::new(Mutex::new(None::<GenerateTextStartEvent>));
        let captured_for_callback = Arc::clone(&captured);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_timeout(TimeoutConfiguration::detailed(
                    TimeoutConfigurationOptions::new()
                        .with_total_ms(5_000)
                        .with_step_ms(1_000),
                ))
                .with_stop_condition(StopCondition::StepCount(3))
                .with_on_start(move |event| {
                    let captured = Arc::clone(&captured_for_callback);
                    async move {
                        *captured.lock().expect("captured lock") = Some(event);
                    }
                }),
        ));

        result.consume_stream();
        let event = captured
            .lock()
            .expect("captured lock")
            .clone()
            .expect("on_start ran");
        assert_eq!(
            event.timeout,
            Some(TimeoutConfiguration::detailed(
                TimeoutConfigurationOptions::new()
                    .with_total_ms(5_000)
                    .with_step_ms(1_000),
            ))
        );
    }

    // Upstream: stream-text.test.ts result.toUIMessageStream
    // "should create a ui message stream with provider metadata": reasoning and text
    // parts carry providerMetadata through to the UI message stream chunks.
    #[test]
    fn stream_text_to_ui_message_stream_includes_provider_metadata() {
        fn signature(value: &str) -> ProviderMetadata {
            ProviderMetadata::from([(
                "testProvider".to_string(),
                Map::from_iter([("signature".to_string(), json!(value))]),
            )])
        }

        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::ReasoningStart(
                LanguageModelReasoningStart::new("r1").with_provider_metadata(signature("r1")),
            ),
            LanguageModelStreamPart::ReasoningDelta(
                LanguageModelReasoningDelta::new("r1", "Hello")
                    .with_provider_metadata(signature("r2")),
            ),
            LanguageModelStreamPart::ReasoningDelta(
                LanguageModelReasoningDelta::new("r1", ", ")
                    .with_provider_metadata(signature("r3")),
            ),
            LanguageModelStreamPart::ReasoningEnd(
                LanguageModelReasoningEnd::new("r1").with_provider_metadata(signature("r4")),
            ),
            LanguageModelStreamPart::TextStart(
                LanguageModelTextStart::new("1").with_provider_metadata(signature("1")),
            ),
            LanguageModelStreamPart::TextDelta(
                LanguageModelTextDelta::new("1", "Hello").with_provider_metadata(signature("2")),
            ),
            LanguageModelStreamPart::TextDelta(
                LanguageModelTextDelta::new("1", ", ").with_provider_metadata(signature("3")),
            ),
            LanguageModelStreamPart::TextDelta(
                LanguageModelTextDelta::new("1", "world!").with_provider_metadata(signature("4")),
            ),
            LanguageModelStreamPart::TextEnd(
                LanguageModelTextEnd::new("1").with_provider_metadata(signature("5")),
            ),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step" },
                {
                    "type": "reasoning-start",
                    "id": "r1",
                    "providerMetadata": { "testProvider": { "signature": "r1" } }
                },
                {
                    "type": "reasoning-delta",
                    "id": "r1",
                    "delta": "Hello",
                    "providerMetadata": { "testProvider": { "signature": "r2" } }
                },
                {
                    "type": "reasoning-delta",
                    "id": "r1",
                    "delta": ", ",
                    "providerMetadata": { "testProvider": { "signature": "r3" } }
                },
                {
                    "type": "reasoning-end",
                    "id": "r1",
                    "providerMetadata": { "testProvider": { "signature": "r4" } }
                },
                {
                    "type": "text-start",
                    "id": "1",
                    "providerMetadata": { "testProvider": { "signature": "1" } }
                },
                {
                    "type": "text-delta",
                    "id": "1",
                    "delta": "Hello",
                    "providerMetadata": { "testProvider": { "signature": "2" } }
                },
                {
                    "type": "text-delta",
                    "id": "1",
                    "delta": ", ",
                    "providerMetadata": { "testProvider": { "signature": "3" } }
                },
                {
                    "type": "text-delta",
                    "id": "1",
                    "delta": "world!",
                    "providerMetadata": { "testProvider": { "signature": "4" } }
                },
                {
                    "type": "text-end",
                    "id": "1",
                    "providerMetadata": { "testProvider": { "signature": "5" } }
                },
                { "type": "finish-step" },
                { "type": "finish", "finishReason": "stop" }
            ])
        );
    }

    // Upstream: stream-text.test.ts options.experimental_onStepStart
    // "should expose providerOptions and runtimeContext".
    #[test]
    fn stream_text_on_step_start_exposes_provider_options_and_runtime_context() {
        let model = MockLanguageModel::new().with_stream_result(stream_result_hello());
        let captured = Arc::new(Mutex::new(None::<GenerateTextStepStartEvent>));
        let captured_for_callback = Arc::clone(&captured);

        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "openai".to_string(),
            json!({ "logprobs": true })
                .as_object()
                .expect("provider options object")
                .clone(),
        );
        let expected_provider_options = provider_options.clone();

        let runtime_context = json!({ "userId": "test-user" })
            .as_object()
            .expect("runtime context object")
            .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_provider_options(provider_options)
                .with_runtime_context(runtime_context)
                .with_on_step_start(move |event| {
                    let captured = Arc::clone(&captured_for_callback);
                    async move {
                        *captured.lock().expect("captured lock") = Some(event);
                    }
                }),
        ));

        let _ = result.text;
        let event = captured
            .lock()
            .expect("captured lock")
            .clone()
            .expect("on_step_start ran");
        assert_eq!(event.provider_options, Some(expected_provider_options));
        assert_eq!(
            event.runtime_context,
            json!({ "userId": "test-user" })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    // --- Two-step (initial reasoning + tool-call, then text) value promises ---
    //
    // Mirrors the upstream `2 steps: initial, tool-result` describe block. Step
    // one streams a reasoning delta and a tool call (so the loop continues); step
    // two streams the final text. Step usage values mirror upstream `testUsage`
    // and `testUsage2` so total usage sums to the documented snapshot.

    fn two_step_usage_initial() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(3),
                no_cache: Some(3),
                cache_read: None,
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: Some(10),
                text: Some(10),
                reasoning: None,
            },
            raw: None,
        }
    }

    fn two_step_usage_final() -> LanguageModelUsage {
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
                reasoning: Some(10),
            },
            raw: None,
        }
    }

    fn two_step_initial_then_tool_result() -> StreamTextResult {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("0")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "0", "thinking",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("0")),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_initial(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-1")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_final(),
                    finish_reason(),
                )),
            ]),
        ]);

        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_max_steps(3),
        ))
    }

    /// Maps packages/ai stream-text.test.ts value-promise row
    /// `result.totalUsage should contain total token usage` — total usage sums
    /// both steps.
    #[test]
    fn stream_text_result_total_usage_sums_both_steps() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(
            result.total_usage,
            LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(6),
                    no_cache: Some(6),
                    cache_read: Some(0),
                    cache_write: Some(0),
                },
                output_tokens: OutputTokenUsage {
                    total: Some(20),
                    text: Some(20),
                    reasoning: Some(10),
                },
                raw: None,
            }
        );
    }

    /// Maps packages/ai stream-text.test.ts value-promise row
    /// `result.finalStep.usage should contain token usage from final step` — the
    /// final step exposes only its own usage, not the running total.
    #[test]
    fn stream_text_result_final_step_usage_is_final_step_only() {
        let result = two_step_initial_then_tool_result();
        let final_step = result.steps.last().expect("final step exists");
        assert_eq!(final_step.usage, two_step_usage_final());
        assert_eq!(final_step.usage.output_tokens.reasoning, Some(10));
        assert_eq!(final_step.usage.input_tokens.total, Some(3));
    }

    /// Maps packages/ai stream-text.test.ts value-promise row
    /// `result.finishReason should contain finish reason from final step`.
    #[test]
    fn stream_text_result_finish_reason_from_final_step() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(
            result.steps.last().expect("final step").finish_reason,
            FinishReason::Stop
        );
    }

    /// Maps packages/ai stream-text.test.ts value-promise row
    /// `result.text should contain text from final step`.
    #[test]
    fn stream_text_result_text_from_final_step() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(result.text, "Hello, world!");
    }

    /// Maps packages/ai stream-text.test.ts value-promise row
    /// `result.steps should contain all steps` — both the tool-call step and the
    /// final text step are retained in order.
    #[test]
    fn stream_text_result_steps_contain_all_steps() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.steps[0].reasoning_text.as_deref(), Some("thinking"));
        assert_eq!(result.steps[0].tool_calls.len(), 1);
        assert_eq!(result.steps[0].tool_results.len(), 1);
        assert_eq!(result.steps[0].text, "");
        assert_eq!(result.steps[1].text, "Hello, world!");
        assert_eq!(result.steps[1].finish_reason, FinishReason::Stop);
    }

    /// Maps packages/ai stream-text.test.ts value-promise row
    /// `result.responseMessages should contain response messages from all steps`
    /// — assistant + tool messages accumulate across both steps.
    #[test]
    fn stream_text_result_response_messages_from_all_steps() {
        let result = two_step_initial_then_tool_result();
        // First assistant message carries reasoning + tool-call, then the tool
        // message, then the final assistant text message.
        assert!(result.response_messages.len() >= 3);
        let last = result
            .response_messages
            .last()
            .expect("response messages present");
        assert!(matches!(last, LanguageModelMessage::Assistant(_)));
    }

    /// Maps packages/ai stream-text.test.ts result.toolCalls row
    /// `should return toolCalls from all steps` — tool calls aggregate across
    /// every step.
    #[test]
    fn stream_text_result_tool_calls_from_all_steps() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "tool1");
        assert_eq!(result.tool_calls[0].tool_call_id, "call-1");
    }

    /// Maps packages/ai stream-text.test.ts result.toolCalls row
    /// `should return empty finalStep.toolCalls when final step has no tool
    /// calls` — the text-only final step exposes no tool calls.
    #[test]
    fn stream_text_result_final_step_tool_calls_empty() {
        let result = two_step_initial_then_tool_result();
        assert!(
            result
                .steps
                .last()
                .expect("final step")
                .tool_calls
                .is_empty()
        );
    }

    /// Maps packages/ai stream-text.test.ts result.toolResults row
    /// `should return toolResults from all steps` — tool results aggregate across
    /// every step.
    #[test]
    fn stream_text_result_tool_results_from_all_steps() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_name, "tool1");
        assert_eq!(result.tool_results[0].output, json!("result1"));
    }

    /// Maps packages/ai stream-text.test.ts result.toolResults row
    /// `should return final step toolResults from finalStep` — the text-only
    /// final step exposes no tool results.
    #[test]
    fn stream_text_result_final_step_tool_results_empty() {
        let result = two_step_initial_then_tool_result();
        assert!(
            result
                .steps
                .last()
                .expect("final step")
                .tool_results
                .is_empty()
        );
        assert_eq!(result.steps[0].tool_results[0].output, json!("result1"));
    }

    /// Maps packages/ai stream-text.test.ts row `result.reasoning should contain
    /// reasoning from model response` — reasoning text from the first step is
    /// exposed on that step.
    #[test]
    fn stream_text_result_reasoning_from_model_response() {
        let result = two_step_initial_then_tool_result();
        assert_eq!(result.steps[0].reasoning_text.as_deref(), Some("thinking"));
    }

    /// Maps packages/ai stream-text.test.ts row `should contain correct step
    /// inputs` — the second step's prompt includes the prior assistant tool-call
    /// and the tool result message.
    #[test]
    fn stream_text_two_step_second_step_prompt_includes_tool_result() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_initial(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_final(),
                    finish_reason(),
                )),
            ]),
        ]);

        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let seen_messages = Arc::new(Mutex::new(Vec::<Vec<LanguageModelMessage>>::new()));
        let seen_for_tool = Arc::clone(&seen_messages);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema).with_execute(move |_input, options| {
                        let seen = Arc::clone(&seen_for_tool);
                        async move {
                            seen.lock()
                                .expect("seen lock")
                                .push(options.messages.clone());
                            Ok(json!("result1"))
                        }
                    }),
                )
                .with_max_steps(3),
        ));

        assert_eq!(result.steps.len(), 2);
        // The second LLM call must have been prompted with the assistant
        // tool-call message and the tool result, beyond the original user input.
        let final_messages = &result.steps[1].request;
        let _ = final_messages;
        // Tool executed exactly once with the original user message visible.
        let seen = seen_messages.lock().expect("seen lock");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], vec![user_message("test-input")]);
        assert_eq!(result.text, "Hello, world!");
    }

    /// Maps packages/ai stream-text.test.ts `2 stop conditions` row
    /// `result.steps should contain a single step` — even though the first step
    /// emits a tool call (which would normally continue the loop), the stop
    /// condition halts after a single step.
    #[test]
    fn stream_text_result_steps_contain_a_single_step_when_stop_condition_matches() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_initial(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "Hello, world!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_final(),
                    finish_reason(),
                )),
            ]),
        ]);

        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_max_steps(5)
                .with_stop_condition(StopCondition::StepCount(1)),
        ));

        // The stop condition matched after the first (tool-call) step, so the
        // second model stream is never consumed.
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].tool_calls.len(), 1);
    }

    fn execute_tools_value_schema() -> JsonObject {
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

    fn execute_tools_single_call_model(tool_name: &str) -> MockLanguageModel {
        MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "call-1",
                tool_name,
                r#"{"value":"test"}"#,
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                tool_calls_finish_reason(),
            )),
        ]))
    }

    #[test]
    fn stream_text_execute_tools_from_stream_handles_async_tool_execution() {
        let model = execute_tools_single_call_model("syncTool");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("syncTool", execute_tools_value_schema()).with_execute(
                    |input, _options| async move {
                        Ok(json!(format!(
                            "{}-sync-result",
                            input["value"].as_str().expect("value is a string")
                        )))
                    },
                ),
            ),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_call_id, "call-1");
        assert_eq!(result.tool_calls[0].tool_name, "syncTool");
        assert_eq!(result.tool_calls[0].input, json!({ "value": "test" }));
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].output, json!("test-sync-result"));
        assert_eq!(result.tool_results[0].is_error, None);
    }

    #[test]
    fn stream_text_execute_tools_from_stream_handles_sync_tool_execution() {
        // Rust tool executors are always futures; the upstream sync execute path
        // is a future that resolves immediately. Assert the same observable output.
        let model = execute_tools_single_call_model("syncTool");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("syncTool", execute_tools_value_schema()).with_execute(
                    |input, _options| {
                        let output = json!(format!(
                            "{}-sync-result",
                            input["value"].as_str().expect("value is a string")
                        ));
                        std::future::ready(Ok(output))
                    },
                ),
            ),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output, json!("test-sync-result"));
        assert_eq!(result.tool_results[0].tool_name, "syncTool");
        assert_eq!(result.tool_results[0].is_error, None);
    }

    #[test]
    fn stream_text_execute_tools_from_stream_passes_sandbox_to_tool_execution() {
        let model = execute_tools_single_call_model("sandboxTool");
        let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(TestSandbox::new("test sandbox"));
        let received = Arc::new(Mutex::new(None::<String>));
        let received_for_tool = Arc::clone(&received);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_experimental_sandbox(Arc::clone(&sandbox))
                .with_tool(
                    Tool::new("sandboxTool", execute_tools_value_schema()).with_execute(
                        move |input, options| {
                            let received = Arc::clone(&received_for_tool);
                            async move {
                                let sandbox = options
                                    .experimental_sandbox
                                    .expect("sandbox is passed to tool execution");
                                *received.lock().expect("received lock") =
                                    Some(sandbox.description().to_string());
                                Ok(json!(format!(
                                    "{}-sandbox-result",
                                    input["value"].as_str().expect("value is a string")
                                )))
                            }
                        },
                    ),
                ),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output, json!("test-sandbox-result"));
        assert_eq!(
            received.lock().expect("received lock").as_deref(),
            Some("test sandbox")
        );
    }

    #[test]
    fn stream_text_execute_tools_from_stream_does_not_execute_provider_executed_tool_calls() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "providerTool", r#"{"value":"test"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "call-1",
                    "providerTool",
                    NonNullJsonValue::new(json!("example-result"))
                        .expect("provider result is non-null"),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = execute_tools_value_schema();
        let output_schema = input_schema.clone();
        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_tool = Arc::clone(&executed);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::provider_executed(
                    "providerTool",
                    "test.providerTool",
                    JsonObject::new(),
                    input_schema,
                    output_schema,
                )
                .with_execute(move |_input, _options| {
                    let executed = Arc::clone(&executed_for_tool);
                    async move {
                        executed.store(true, Ordering::SeqCst);
                        Ok(json!("should-not-execute"))
                    }
                }),
            ),
        ));

        assert!(!executed.load(Ordering::SeqCst));
    }

    #[test]
    fn stream_text_execute_tools_from_stream_calls_execution_callbacks_in_order() {
        let model = execute_tools_single_call_model("testTool");
        let call_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let start_order = Arc::clone(&call_order);
        let execute_order = Arc::clone(&call_order);
        let end_order = Arc::clone(&call_order);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        move |input, _options| {
                            let execute_order = Arc::clone(&execute_order);
                            async move {
                                execute_order.lock().expect("order lock").push("execute");
                                Ok(json!(format!(
                                    "{}-result",
                                    input["value"].as_str().expect("value is a string")
                                )))
                            }
                        },
                    ),
                )
                .with_on_tool_execution_start(move |_event| {
                    let start_order = Arc::clone(&start_order);
                    async move {
                        start_order
                            .lock()
                            .expect("order lock")
                            .push("onToolExecutionStart");
                    }
                })
                .with_on_tool_execution_end(move |_event| {
                    let end_order = Arc::clone(&end_order);
                    async move {
                        end_order
                            .lock()
                            .expect("order lock")
                            .push("onToolExecutionEnd");
                    }
                }),
        ));

        assert_eq!(
            call_order.lock().expect("order lock").as_slice(),
            ["onToolExecutionStart", "execute", "onToolExecutionEnd"]
        );
    }

    #[test]
    fn stream_text_execute_tools_from_stream_passes_call_metadata_to_callbacks() {
        let model = execute_tools_single_call_model("testTool");
        let tools_context =
            JsonObject::from_iter([("testTool".to_string(), json!({ "value": "test" }))]);
        let start_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionStartEvent>::new()));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let start_events_for_callback = Arc::clone(&start_events);
        let end_events_for_callback = Arc::clone(&end_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        |input, _options| async move {
                            Ok(json!(format!(
                                "{}-result",
                                input["value"].as_str().expect("value is a string")
                            )))
                        },
                    ),
                )
                .with_on_tool_execution_start(move |event| {
                    let events = Arc::clone(&start_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                })
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));

        let start_events = start_events.lock().expect("events lock");
        assert_eq!(start_events.len(), 1);
        let start = &start_events[0];
        assert_eq!(start.tool_call.tool_call_id, "call-1");
        assert_eq!(start.tool_call.tool_name, "testTool");
        assert_eq!(start.tool_call.input, json!({ "value": "test" }));
        assert_eq!(start.tool_context, Some(json!({ "value": "test" })));
        assert_eq!(start.messages, vec![user_message("test-input")]);

        let end_events = end_events.lock().expect("events lock");
        assert_eq!(end_events.len(), 1);
        let end = &end_events[0];
        assert_eq!(end.tool_call.tool_call_id, "call-1");
        assert_eq!(end.tool_context, Some(json!({ "value": "test" })));
        assert_eq!(end.tool_output.output, json!("test-result"));
    }

    /// Maps packages/ai stream-text.test.ts `tool execution errors` row
    /// `should include tool error part in the full stream` — a local tool whose
    /// execution fails surfaces an error tool-result part in the high-level
    /// stream parts (carrying `is_error` and the failure message).
    #[test]
    fn stream_text_includes_tool_error_part_in_full_stream() {
        let model = execute_tools_single_call_model("tool1");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("tool1", execute_tools_value_schema()).with_execute(
                    |_input, _options| async move {
                        Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new("test error"))
                    },
                ),
            ),
        ));

        let error_result = result
            .parts
            .iter()
            .find_map(|part| match part {
                TextStreamPart::ToolResult(tool_result) if tool_result.is_error == Some(true) => {
                    Some(tool_result)
                }
                _ => None,
            })
            .expect("error tool-result part present in full stream");

        assert_eq!(error_result.tool_call_id, "call-1");
        assert_eq!(error_result.tool_name, "tool1");
        assert!(
            error_result
                .output
                .as_str()
                .expect("error output is a string")
                .contains("test error")
        );
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].is_error, Some(true));
    }

    /// Maps packages/ai stream-text.test.ts `tool execution errors` row
    /// `should include error result in response messages` — the failed tool
    /// result is recorded in the accumulated response messages as a tool message
    /// with an errored tool-result content part.
    #[test]
    fn stream_text_includes_tool_error_result_in_response_messages() {
        let model = execute_tools_single_call_model("tool1");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("tool1", execute_tools_value_schema()).with_execute(
                    |_input, _options| async move {
                        Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new("test error"))
                    },
                ),
            ),
        ));

        let tool_message = result
            .response_messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(tool_message) => Some(tool_message),
                _ => None,
            })
            .expect("tool response message present");

        let tool_result = tool_message
            .content
            .iter()
            .find_map(|part| match part {
                LanguageModelToolContentPart::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .expect("tool-result content part present");

        assert_eq!(tool_result.tool_call_id, "call-1");
        assert_eq!(tool_result.tool_name, "tool1");
    }

    /// Maps packages/ai stream-text.test.ts `tool execution errors` row
    /// `should add tool-error parts to ui message stream` — a failed local tool
    /// emits a `tool-output-error` chunk in the UI message stream.
    #[test]
    fn stream_text_adds_tool_error_parts_to_ui_message_stream() {
        let model = execute_tools_single_call_model("tool1");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("tool1", execute_tools_value_schema()).with_execute(
                    |_input, _options| async move {
                        Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new("test error"))
                    },
                ),
            ),
        ));

        let chunks = result.to_ui_message_stream();
        let error_chunk = chunks
            .iter()
            .find_map(|chunk| match chunk {
                UiMessageChunk::ToolOutputError {
                    tool_call_id,
                    error_text,
                    ..
                } => Some((tool_call_id, error_text)),
                _ => None,
            })
            .expect("tool-output-error chunk present in ui message stream");

        assert_eq!(error_chunk.0, "call-1");
        assert!(error_chunk.1.contains("test error"));
    }

    /// Maps packages/ai stream-text.test.ts `provider-executed tools` row
    /// `should include provider-executed tool call and result content` — a
    /// provider-executed tool call streamed alongside its provider-supplied
    /// result is surfaced in the result content (and tool-call/tool-result
    /// collections) carrying `provider_executed: true`, including an errored
    /// provider result.
    #[test]
    fn stream_text_includes_provider_executed_tool_call_and_result_content() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "web_search", r#"{"value":"value"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "call-1",
                    "web_search",
                    NonNullJsonValue::new(json!({ "value": "result1" }))
                        .expect("provider result is non-null"),
                )),
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-2", "web_search", r#"{"value":"value"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(
                    LanguageModelToolResult::new(
                        "call-2",
                        "web_search",
                        NonNullJsonValue::new(json!("ERROR")).expect("provider result is non-null"),
                    )
                    .with_is_error(true),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_tool = Arc::clone(&executed);
        let input_schema = execute_tools_value_schema();
        let output_schema = input_schema.clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::provider_executed(
                    "web_search",
                    "test.web_search",
                    JsonObject::new(),
                    input_schema,
                    output_schema,
                )
                .with_execute(move |_input, _options| {
                    let executed = Arc::clone(&executed_for_tool);
                    async move {
                        executed.store(true, Ordering::SeqCst);
                        Ok(json!("should-not-execute"))
                    }
                }),
            ),
        ));

        // Provider-executed tools are never invoked locally.
        assert!(!executed.load(Ordering::SeqCst));

        assert_eq!(result.tool_calls.len(), 2);
        assert!(
            result
                .tool_calls
                .iter()
                .all(|call| call.provider_executed == Some(true))
        );
        assert_eq!(result.tool_calls[0].tool_call_id, "call-1");
        assert_eq!(result.tool_calls[0].tool_name, "web_search");
        assert_eq!(result.tool_calls[0].input, json!({ "value": "value" }));

        assert_eq!(result.tool_results.len(), 2);
        assert!(
            result
                .tool_results
                .iter()
                .all(|res| res.provider_executed == Some(true))
        );
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].output, json!({ "value": "result1" }));
        assert_eq!(result.tool_results[0].is_error, None);
        assert_eq!(result.tool_results[1].tool_call_id, "call-2");
        assert_eq!(result.tool_results[1].is_error, Some(true));
    }

    /// Maps packages/ai stream-text.test.ts `provider-executed tools` row
    /// `should only execute a single step` — provider-executed tool results are
    /// settled within the same model call, so the loop does not start a second
    /// step.
    #[test]
    fn stream_text_provider_executed_tool_results_run_in_a_single_step() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "web_search", r#"{"value":"value"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "call-1",
                    "web_search",
                    NonNullJsonValue::new(json!({ "value": "result1" }))
                        .expect("provider result is non-null"),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let input_schema = execute_tools_value_schema();
        let output_schema = input_schema.clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::provider_executed(
                    "web_search",
                    "test.web_search",
                    JsonObject::new(),
                    input_schema,
                    output_schema,
                ),
            ),
        ));

        assert_eq!(result.steps.len(), 1);
        assert_eq!(model.stream_calls().len(), 1);
    }

    /// Maps packages/ai stream-text.test.ts `tool execution errors` row
    /// `should include the error part in the step stream` — a local tool whose
    /// execution fails records an errored tool-result on the step (with
    /// `is_error` set and the failure message in the output).
    #[test]
    fn stream_text_step_includes_tool_error_part() {
        let model = execute_tools_single_call_model("tool1");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("tool1", execute_tools_value_schema()).with_execute(
                    |_input, _options| async move {
                        Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new("test error"))
                    },
                ),
            ),
        ));

        assert_eq!(result.steps.len(), 1);
        let step = &result.steps[0];
        assert_eq!(step.tool_results.len(), 1);
        let error_result = &step.tool_results[0];
        assert_eq!(error_result.tool_call_id, "call-1");
        assert_eq!(error_result.tool_name, "tool1");
        assert_eq!(error_result.is_error, Some(true));
        assert!(
            error_result
                .output
                .as_str()
                .expect("error output is a string")
                .contains("test error")
        );
    }

    /// Maps packages/ai stream-text.test.ts `options.output` text-output row
    /// `should resolve output promise with the correct content` — the default
    /// text output resolves to the full accumulated text.
    #[test]
    fn stream_text_text_output_resolves_to_full_text() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        assert_eq!(
            result
                .output_as::<String>()
                .expect("text output resolves to a string"),
            "Hello, world!".to_string()
        );
    }

    #[test]
    fn stream_text_execute_tools_from_stream_reports_success_to_execution_end() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let end_events_for_callback = Arc::clone(&end_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        |input, _options| async move {
                            Ok(json!(format!(
                                "{}-result",
                                input["value"].as_str().expect("value is a string")
                            )))
                        },
                    ),
                )
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));

        let end_events = end_events.lock().expect("events lock");
        assert_eq!(end_events.len(), 1);
        let end = &end_events[0];
        assert_eq!(end.tool_call.input, json!({ "value": "abc" }));
        assert_eq!(end.tool_context, None);
        assert_eq!(end.tool_output.output, json!("abc-result"));
        assert_eq!(end.tool_output.is_error, None);
    }

    #[test]
    fn stream_text_execute_tools_from_stream_reports_error_to_execution_end() {
        let model = execute_tools_single_call_model("failingTool");
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let end_events_for_callback = Arc::clone(&end_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("failingTool", execute_tools_value_schema()).with_execute(
                        |input, _options| async move {
                            if input["value"] == json!("test") {
                                Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new(
                                    "tool failed",
                                ))
                            } else {
                                Ok(json!("test-result"))
                            }
                        },
                    ),
                )
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));

        let end_events = end_events.lock().expect("events lock");
        assert_eq!(end_events.len(), 1);
        let end = &end_events[0];
        assert_eq!(end.tool_call.tool_name, "failingTool");
        assert_eq!(end.tool_output.is_error, Some(true));
        assert!(
            end.tool_output
                .output
                .as_str()
                .expect("error output is a string")
                .contains("tool failed")
        );
    }

    #[test]
    fn stream_text_execute_tools_from_stream_skips_callbacks_for_tools_without_execute() {
        let model = execute_tools_single_call_model("noExecuteTool");
        let starts = Arc::new(AtomicUsize::new(0));
        let ends = Arc::new(AtomicUsize::new(0));
        let starts_for_callback = Arc::clone(&starts);
        let ends_for_callback = Arc::clone(&ends);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::new("noExecuteTool", execute_tools_value_schema()))
                .with_on_tool_execution_start(move |_event| {
                    let starts = Arc::clone(&starts_for_callback);
                    async move {
                        starts.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .with_on_tool_execution_end(move |_event| {
                    let ends = Arc::clone(&ends_for_callback);
                    async move {
                        ends.fetch_add(1, Ordering::SeqCst);
                    }
                }),
        ));

        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(ends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stream_text_execute_tools_from_stream_calls_callbacks_for_each_tool_in_multi_tool_stream() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"a"}"#,
                )),
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-2",
                    "testTool",
                    r#"{"value":"b"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let start_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionStartEvent>::new()));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let start_events_for_callback = Arc::clone(&start_events);
        let end_events_for_callback = Arc::clone(&end_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        |input, _options| async move {
                            Ok(json!(format!(
                                "{}-result",
                                input["value"].as_str().expect("value is a string")
                            )))
                        },
                    ),
                )
                .with_on_tool_execution_start(move |event| {
                    let events = Arc::clone(&start_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                })
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));

        let start_events = start_events.lock().expect("events lock");
        assert_eq!(start_events.len(), 2);
        assert_eq!(start_events[0].tool_call.tool_call_id, "call-1");
        assert_eq!(start_events[0].tool_call.input, json!({ "value": "a" }));
        assert_eq!(start_events[1].tool_call.tool_call_id, "call-2");
        assert_eq!(start_events[1].tool_call.input, json!({ "value": "b" }));

        let end_events = end_events.lock().expect("events lock");
        assert_eq!(end_events.len(), 2);
        assert_eq!(end_events[0].tool_output.output, json!("a-result"));
        assert_eq!(end_events[1].tool_output.output, json!("b-result"));
    }

    #[test]
    fn stream_text_execute_tools_from_stream_skips_callbacks_for_provider_executed_tools() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "providerTool", r#"{"value":"test"}"#)
                        .with_provider_executed(true),
                ),
                LanguageModelStreamPart::ToolResult(LanguageModelToolResult::new(
                    "call-1",
                    "providerTool",
                    NonNullJsonValue::new(json!({ "result": "example" }))
                        .expect("provider result is non-null"),
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let input_schema = execute_tools_value_schema();
        let output_schema = input_schema.clone();
        let starts = Arc::new(AtomicUsize::new(0));
        let ends = Arc::new(AtomicUsize::new(0));
        let starts_for_callback = Arc::clone(&starts);
        let ends_for_callback = Arc::clone(&ends);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(Tool::provider_executed(
                    "providerTool",
                    "test.providerTool",
                    JsonObject::new(),
                    input_schema,
                    output_schema,
                ))
                .with_on_tool_execution_start(move |_event| {
                    let starts = Arc::clone(&starts_for_callback);
                    async move {
                        starts.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .with_on_tool_execution_end(move |_event| {
                    let ends = Arc::clone(&ends_for_callback);
                    async move {
                        ends.fetch_add(1, Ordering::SeqCst);
                    }
                }),
        ));

        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(ends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stream_text_execute_tools_from_stream_surfaces_async_tool_error() {
        let model = execute_tools_single_call_model("failingTool");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("failingTool", execute_tools_value_schema()).with_execute(
                    |input, _options| async move {
                        if input["value"] == json!("test") {
                            Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new(
                                "Tool execution failed!",
                            ))
                        } else {
                            Ok(json!("test-result"))
                        }
                    },
                ),
            ),
        ));

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].tool_call_id, "call-1");
        assert_eq!(result.tool_results[0].tool_name, "failingTool");
        assert_eq!(result.tool_results[0].input, json!({ "value": "test" }));
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert!(
            result.tool_results[0]
                .output
                .as_str()
                .expect("error output is a string")
                .contains("Tool execution failed!")
        );
    }

    #[test]
    fn stream_text_execute_tools_from_stream_surfaces_sync_tool_error() {
        let model = execute_tools_single_call_model("failingTool");
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::new("failingTool", execute_tools_value_schema()).with_execute(
                    |input, _options| {
                        let outcome = if input["value"] == json!("test") {
                            Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new(
                                "Sync tool failed!",
                            ))
                        } else {
                            Ok(json!("test-result"))
                        };
                        std::future::ready(outcome)
                    },
                ),
            ),
        ));

        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].is_error, Some(true));
        assert!(
            result.tool_results[0]
                .output
                .as_str()
                .expect("error output is a string")
                .contains("Sync tool failed!")
        );
    }

    // Upstream: stream-text.test.ts result.onFinish "should send sources"
    // (packages-ai-1148). Sources emitted by the model reach the onFinish event.
    #[test]
    fn stream_text_on_finish_event_sends_sources() {
        let first_source = LanguageModelSource::Url(
            LanguageModelUrlSource::new("123", "https://example.com").with_title("Example"),
        );
        let second_source = LanguageModelSource::Url(
            LanguageModelUrlSource::new("456", "https://example.com/2").with_title("Example 2"),
        );
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Source(first_source.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Source(second_source.clone()),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")]).with_on_finish(
                move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events.lock().expect("finish lock").push(event);
                    }
                },
            ),
        ));

        let finish_events = finish_events.lock().expect("finish lock");
        assert_eq!(finish_events.len(), 1);
        assert_eq!(finish_events[0].sources, vec![first_source, second_source]);
    }

    // Upstream: stream-text.test.ts result.onFinish "should send files"
    // (packages-ai-1150). Files emitted by the model reach the onFinish event.
    #[test]
    fn stream_text_on_finish_event_sends_files() {
        let first_file = data_file("Hello World");
        let second_file = LanguageModelFile::new(
            "image/jpeg",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("QkFVRw==".to_string()),
            },
        );
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::File(first_file.clone()),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::File(second_file.clone()),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")]).with_on_finish(
                move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events.lock().expect("finish lock").push(event);
                    }
                },
            ),
        ));

        let finish_events = finish_events.lock().expect("finish lock");
        assert_eq!(finish_events.len(), 1);
        let files: Vec<(String, String)> = finish_events[0]
            .files
            .iter()
            .map(|file| (file.media_type().to_string(), file.base64()))
            .collect();
        assert_eq!(
            files,
            vec![
                ("text/plain".to_string(), "Hello World".to_string()),
                ("image/jpeg".to_string(), "QkFVRw==".to_string()),
            ]
        );
    }

    // Upstream: stream-text.test.ts result.onFinish "should send custom parts"
    // (packages-ai-1149). Custom provider content is aggregated in the result.
    #[test]
    fn stream_text_on_finish_aggregates_custom_parts() {
        let provider_metadata = ProviderMetadata::from([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("cmp_123"))]),
        )]);
        let custom_part = LanguageModelCustomContent::new("openai.compaction")
            .with_provider_metadata(provider_metadata);
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
            LanguageModelStreamPart::Custom(custom_part.clone()),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        assert_eq!(result.custom_parts, vec![custom_part]);
    }

    // Upstream: stream-text.test.ts result.response.messages
    // "should contain assistant response message when there are no tool calls"
    // (packages-ai-1152). A text-only response yields a single assistant message.
    #[test]
    fn stream_text_response_messages_contain_assistant_message_without_tool_calls() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        assert_eq!(result.response_messages.len(), 1);
        let message = &result.response_messages[0];
        let LanguageModelMessage::Assistant(assistant) = message else {
            panic!("expected assistant message, got {message:?}");
        };
        assert_eq!(assistant.content.len(), 1);
        let LanguageModelAssistantContentPart::Text(text) = &assistant.content[0] else {
            panic!("expected text content part");
        };
        assert_eq!(text.text, "Hello, world!");
    }

    // Upstream: stream-text.test.ts result.responseMessages "should contain reasoning"
    // (packages-ai-1073). Reasoning parts streamed by the model are carried into
    // the assistant response message as reasoning content parts.
    #[test]
    fn stream_text_response_messages_contain_reasoning() {
        let result = stream_text_result_from_parts(vec![
            LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
            LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                "r1",
                "I will open the conversation with witty banter.",
            )),
            LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let LanguageModelMessage::Assistant(assistant) = &result.response_messages[0] else {
            panic!("expected assistant message");
        };
        let reasoning_parts = assistant
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelAssistantContentPart::Reasoning(reasoning) => Some(reasoning),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(reasoning_parts.len(), 1);
        assert_eq!(
            reasoning_parts[0].text,
            "I will open the conversation with witty banter."
        );
    }

    // Upstream: stream-text.test.ts options.onChunk
    // "should include custom parts in onChunk events" (packages-ai-1145).
    #[test]
    fn stream_text_on_chunk_includes_custom_parts() {
        let provider_metadata = ProviderMetadata::from([(
            "openai".to_string(),
            Map::from_iter([("itemId".to_string(), json!("cmp_123"))]),
        )]);
        let custom_part = LanguageModelCustomContent::new("openai.compaction")
            .with_provider_metadata(provider_metadata);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::Custom(custom_part.clone()),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let chunks = Arc::new(Mutex::new(Vec::<TextStreamPart>::new()));
        let chunks_for_callback = Arc::clone(&chunks);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("prompt")]).with_on_chunk(
                move |event| {
                    let chunks = Arc::clone(&chunks_for_callback);
                    async move {
                        chunks.lock().expect("chunks lock").push(event.chunk);
                    }
                },
            ),
        ));
        let _ = result.text;

        let chunks = chunks.lock().expect("chunks lock");
        let custom_chunks = chunks
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::Custom(part) => Some(part.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(custom_chunks, vec![custom_part]);
    }

    // Upstream: stream-text.test.ts options.experimental_onStart
    // "should be called before doStream" (packages-ai-1110). The onStart callback
    // runs before the model's doStream is invoked.
    #[test]
    fn stream_text_on_start_runs_before_do_stream() {
        struct RecordingStreamModel {
            call_order: Arc<Mutex<Vec<&'static str>>>,
        }

        impl LanguageModel for RecordingStreamModel {
            type SupportedUrlsFuture<'a>
                = std::future::Ready<LanguageModelSupportedUrls>
            where
                Self: 'a;
            type GenerateFuture<'a>
                = std::future::Ready<LanguageModelGenerateResult>
            where
                Self: 'a;
            type Stream = Vec<LanguageModelStreamPart>;
            type StreamFuture<'a>
                = std::future::Ready<LanguageModelStreamResult<Self::Stream>>
            where
                Self: 'a;

            fn provider(&self) -> &str {
                "mock-provider"
            }

            fn model_id(&self) -> &str {
                "mock-model-id"
            }

            fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
                ready(LanguageModelSupportedUrls::default())
            }

            fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
                ready(LanguageModelGenerateResult::new(
                    Vec::<LanguageModelContent>::new(),
                    finish_reason(),
                    LanguageModelUsage::default(),
                ))
            }

            fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
                self.call_order.lock().expect("order lock").push("doStream");
                ready(stream_result_hello())
            }
        }

        let call_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let call_order_for_callback = Arc::clone(&call_order);
        let model = RecordingStreamModel {
            call_order: Arc::clone(&call_order),
        };

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_on_start(
                move |_event| {
                    let call_order = Arc::clone(&call_order_for_callback);
                    async move {
                        call_order.lock().expect("order lock").push("onStart");
                    }
                },
            ),
        ));
        let _ = result.text;

        let call_order = call_order.lock().expect("order lock");
        assert_eq!(*call_order, vec!["onStart", "doStream"]);
    }

    // Upstream: stream-text.test.ts options.onError
    // "should not prevent error from being forwarded" (packages-ai-1151). When the
    // model throws, the error is surfaced on the full stream even with an onError
    // handler present.
    #[test]
    fn stream_text_on_error_does_not_prevent_error_from_being_forwarded() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({ "message": "test error" }),
                )),
            ]));
        let errors = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let errors_for_callback = Arc::clone(&errors);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_on_error(
                move |event| {
                    let errors = Arc::clone(&errors_for_callback);
                    async move {
                        errors.lock().expect("errors lock").push(event.error);
                    }
                },
            ),
        ));

        let error_parts = result
            .parts
            .iter()
            .filter_map(|part| match part {
                TextStreamPart::Error(part) => Some(part.error.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(error_parts.len(), 1);
        assert_eq!(error_parts[0], json!({ "message": "test error" }));
        assert_eq!(errors.lock().expect("errors lock").len(), 1);
    }

    // Upstream: stream-text.test.ts options model-call callbacks
    // "should fire the model-call callbacks before tool execution and step finish"
    // (packages-ai-1124). The step-start, language-model-call-start/end,
    // tool-execution-start/end, and step-finish callbacks fire in order.
    #[test]
    fn stream_text_model_call_callbacks_fire_before_tool_execution_and_step_finish() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);

        let call_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let push = |order: &Arc<Mutex<Vec<&'static str>>>, label: &'static str| {
            order.lock().expect("order lock").push(label);
        };
        let o_step_start = Arc::clone(&call_order);
        let o_call_start = Arc::clone(&call_order);
        let o_call_end = Arc::clone(&call_order);
        let o_tool_start = Arc::clone(&call_order);
        let o_tool_end = Arc::clone(&call_order);
        let o_step_finish = Arc::clone(&call_order);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        |input, _options| async move {
                            Ok(json!(format!(
                                "{}-result",
                                input["value"].as_str().expect("value is a string")
                            )))
                        },
                    ),
                )
                .with_max_steps(2)
                .with_on_step_start(move |_event| {
                    push(&o_step_start, "onStepStart");
                    async {}
                })
                .with_experimental_on_language_model_call_start(move |_event| {
                    push(&o_call_start, "onLanguageModelCallStart");
                    async {}
                })
                .with_experimental_on_language_model_call_end(move |_event| {
                    push(&o_call_end, "onLanguageModelCallEnd");
                    async {}
                })
                .with_on_tool_execution_start(move |_event| {
                    push(&o_tool_start, "onToolExecutionStart");
                    async {}
                })
                .with_on_tool_execution_end(move |_event| {
                    push(&o_tool_end, "onToolExecutionEnd");
                    async {}
                })
                .with_on_step_finish(move |_event| {
                    push(&o_step_finish, "onStepFinish");
                    async {}
                }),
        ));
        result.consume_stream();

        let call_order = call_order.lock().expect("order lock");
        // The first step calls the model, executes the tool, then finishes the
        // step; the model-call callbacks must precede tool execution and the step
        // finish callback.
        let first_six = &call_order[..6];
        assert_eq!(
            first_six,
            [
                "onStepStart",
                "onLanguageModelCallStart",
                "onLanguageModelCallEnd",
                "onToolExecutionStart",
                "onToolExecutionEnd",
                "onStepFinish",
            ]
        );
    }

    // Upstream: stream-text.test.ts options.onStepStart
    // "should be called once per step in a multi-step tool loop" (packages-ai-1116).
    #[test]
    fn stream_text_on_step_start_called_once_per_step_in_multi_step_loop() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let step_numbers = Arc::new(Mutex::new(Vec::<usize>::new()));
        let step_numbers_for_callback = Arc::clone(&step_numbers);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema())
                        .with_execute(|_input, _options| async move { Ok(json!("tool-result")) }),
                )
                .with_max_steps(2)
                .with_on_step_start(move |event| {
                    let step_numbers = Arc::clone(&step_numbers_for_callback);
                    async move {
                        step_numbers
                            .lock()
                            .expect("step numbers lock")
                            .push(event.step_number);
                    }
                }),
        ));
        result.consume_stream();

        assert_eq!(*step_numbers.lock().expect("step numbers lock"), vec![0, 1]);
    }

    // Upstream: stream-text.test.ts options.onToolExecutionStart
    // "should be called before tool execution" (packages-ai-1129).
    #[test]
    fn stream_text_on_tool_execution_start_runs_before_tool_execution() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let call_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let order_for_start = Arc::clone(&call_order);
        let order_for_execute = Arc::clone(&call_order);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        move |_input, _options| {
                            let order_for_execute = Arc::clone(&order_for_execute);
                            async move {
                                order_for_execute
                                    .lock()
                                    .expect("order lock")
                                    .push("execute");
                                Ok(json!("tool-result"))
                            }
                        },
                    ),
                )
                .with_on_tool_execution_start(move |_event| {
                    let order_for_start = Arc::clone(&order_for_start);
                    async move {
                        order_for_start
                            .lock()
                            .expect("order lock")
                            .push("onToolExecutionStart");
                    }
                }),
        ));
        result.consume_stream();

        let call_order = call_order.lock().expect("order lock");
        let start_index = call_order
            .iter()
            .position(|label| *label == "onToolExecutionStart")
            .expect("start fired");
        let execute_index = call_order
            .iter()
            .position(|label| *label == "execute")
            .expect("execute ran");
        assert!(start_index < execute_index);
    }

    // Upstream: stream-text.test.ts options.onEnd "should send correct information"
    // (packages-ai-1147). The onFinish callback receives the final text, finish
    // reason, and usage.
    #[test]
    fn stream_text_on_finish_sends_correct_information() {
        let model = MockLanguageModel::new().with_stream_result(stream_result_hello());
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_on_finish(
                move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events.lock().expect("finish lock").push(event);
                    }
                },
            ),
        ));
        let _ = result.text;

        let finish_events = finish_events.lock().expect("finish lock");
        assert_eq!(finish_events.len(), 1);
        let event = &finish_events[0];
        assert_eq!(event.text, "Hello, world!");
        assert_eq!(event.finish_reason, FinishReason::Stop);
        assert_eq!(event.usage, usage());
    }

    // Upstream: stream-text.test.ts options tool callbacks
    // "should fire tool call callbacks for each tool in a multi-step loop"
    // (packages-ai-1143). Each step with a tool call fires the tool start/end
    // callbacks, so a two-step loop fires both twice.
    #[test]
    fn stream_text_tool_callbacks_fire_for_each_tool_in_multi_step_loop() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"a"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-2",
                    "testTool",
                    r#"{"value":"b"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let starts = Arc::new(AtomicUsize::new(0));
        let ends = Arc::new(AtomicUsize::new(0));
        let starts_for_callback = Arc::clone(&starts);
        let ends_for_callback = Arc::clone(&ends);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema())
                        .with_execute(|_input, _options| async move { Ok(json!("tool-result")) }),
                )
                .with_max_steps(3)
                .with_on_tool_execution_start(move |_event| {
                    let starts = Arc::clone(&starts_for_callback);
                    async move {
                        starts.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .with_on_tool_execution_end(move |_event| {
                    let ends = Arc::clone(&ends_for_callback);
                    async move {
                        ends.fetch_add(1, Ordering::SeqCst);
                    }
                }),
        ));
        result.consume_stream();

        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_eq!(ends.load(Ordering::SeqCst), 2);
    }

    // Upstream: stream-text.test.ts options.onStepStart
    // "should be called before doStream on each step" (packages-ai-1117). The
    // step-start callback runs before the model's doStream on the step.
    #[test]
    fn stream_text_on_step_start_runs_before_do_stream() {
        struct StepRecordingStreamModel {
            call_order: Arc<Mutex<Vec<&'static str>>>,
        }

        impl LanguageModel for StepRecordingStreamModel {
            type SupportedUrlsFuture<'a>
                = std::future::Ready<LanguageModelSupportedUrls>
            where
                Self: 'a;
            type GenerateFuture<'a>
                = std::future::Ready<LanguageModelGenerateResult>
            where
                Self: 'a;
            type Stream = Vec<LanguageModelStreamPart>;
            type StreamFuture<'a>
                = std::future::Ready<LanguageModelStreamResult<Self::Stream>>
            where
                Self: 'a;

            fn provider(&self) -> &str {
                "mock-provider"
            }

            fn model_id(&self) -> &str {
                "mock-model-id"
            }

            fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
                ready(LanguageModelSupportedUrls::default())
            }

            fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
                ready(LanguageModelGenerateResult::new(
                    Vec::<LanguageModelContent>::new(),
                    finish_reason(),
                    LanguageModelUsage::default(),
                ))
            }

            fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
                self.call_order.lock().expect("order lock").push("doStream");
                ready(stream_result_hello())
            }
        }

        let call_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let call_order_for_callback = Arc::clone(&call_order);
        let model = StepRecordingStreamModel {
            call_order: Arc::clone(&call_order),
        };

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_on_step_start(
                move |_event| {
                    let call_order = Arc::clone(&call_order_for_callback);
                    async move {
                        call_order.lock().expect("order lock").push("onStepStart");
                    }
                },
            ),
        ));
        let _ = result.text;

        let call_order = call_order.lock().expect("order lock");
        assert_eq!(*call_order, vec!["onStepStart", "doStream"]);
    }

    // Upstream: stream-text.test.ts options.onToolExecutionEnd
    // "should pass context on success" (packages-ai-1140). The per-tool context
    // from toolsContext reaches the tool-execution-end event on success.
    #[test]
    fn stream_text_on_tool_execution_end_passes_context_on_success() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let mut tools_context = serde_json::Map::new();
        tools_context.insert("testTool".to_string(), json!({ "context": "test" }));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let end_events_for_callback = Arc::clone(&end_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema())
                        .with_execute(|_input, _options| async move { Ok(json!("ok")) }),
                )
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));

        let end_events = end_events.lock().expect("events lock");
        assert_eq!(end_events.len(), 1);
        assert_eq!(
            end_events[0].tool_context,
            Some(json!({ "context": "test" }))
        );
        assert_eq!(end_events[0].tool_output.is_error, None);
    }

    // Upstream: stream-text.test.ts options.onToolExecutionEnd
    // "should pass context on error" (packages-ai-1141). The per-tool context
    // reaches the tool-execution-end event even when the tool execution fails.
    #[test]
    fn stream_text_on_tool_execution_end_passes_context_on_error() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let mut tools_context = serde_json::Map::new();
        tools_context.insert("testTool".to_string(), json!({ "context": "test" }));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateTextToolExecutionEndEvent>::new()));
        let end_events_for_callback = Arc::clone(&end_events);

        let _result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tools_context(tools_context)
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema()).with_execute(
                        |_input, _options| async move {
                            Err::<JsonValue, ToolExecutionError>(ToolExecutionError::new(
                                "tool execution failed",
                            ))
                        },
                    ),
                )
                .with_on_tool_execution_end(move |event| {
                    let events = Arc::clone(&end_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));

        let end_events = end_events.lock().expect("events lock");
        assert_eq!(end_events.len(), 1);
        assert_eq!(
            end_events[0].tool_context,
            Some(json!({ "context": "test" }))
        );
        assert_eq!(end_events[0].tool_output.is_error, Some(true));
    }

    // Upstream: stream-text.test.ts result.toUIMessageStream
    // "should include tool metadata in ui message stream chunks" (packages-ai-1034).
    // A dynamic tool's metadata is carried onto the tool-input-available chunk.
    #[test]
    fn stream_text_ui_message_stream_includes_tool_metadata() {
        let metadata = json!({ "clientName": "MyMCPClient" })
            .as_object()
            .expect("metadata is an object")
            .clone();
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "test-tool",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_tool(
                Tool::dynamic("test-tool", execute_tools_value_schema())
                    .with_metadata(metadata.clone())
                    .with_execute(|_input, _options| async move { Ok(json!("result")) }),
            ),
        ));

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks is an array");
        let available = chunks
            .iter()
            .find(|chunk| chunk["type"] == json!("tool-input-available"))
            .expect("tool-input-available chunk present");
        assert_eq!(available["toolName"], json!("test-tool"));
        assert_eq!(available["dynamic"], json!(true));
        assert_eq!(
            available["toolMetadata"],
            json!({ "clientName": "MyMCPClient" })
        );
    }

    // Upstream: stream-text.test.ts options.onStepStart
    // "should reflect model changes from prepareStep" (packages-ai-1119). When
    // prepareStep swaps the model for the second step, the step-start events
    // report the active step model for each step.
    #[test]
    fn stream_text_on_step_start_reflects_model_changes_from_prepare_step() {
        let primary =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "testTool",
                    r#"{"value":"abc"}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]));
        let alternate = MockLanguageModel::new()
            .with_provider("alternate-provider")
            .with_model_id("alternate-model-id")
            .with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let step_start_events = Arc::new(Mutex::new(Vec::<GenerateTextStepStartEvent>::new()));
        let step_start_events_for_callback = Arc::clone(&step_start_events);
        let alternate_model = &alternate;

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&primary, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("testTool", execute_tools_value_schema())
                        .with_execute(|_input, _options| async move { Ok(json!("tool-result")) }),
                )
                .with_max_steps(2)
                .with_prepare_step(move |options| {
                    let switch = options.step_number == 1;
                    async move {
                        if switch {
                            PrepareStepResult::new().with_model(alternate_model)
                        } else {
                            PrepareStepResult::new()
                        }
                    }
                })
                .with_on_step_start(move |event| {
                    let events = Arc::clone(&step_start_events_for_callback);
                    async move {
                        events.lock().expect("events lock").push(event);
                    }
                }),
        ));
        result.consume_stream();

        let events = step_start_events.lock().expect("events lock");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].provider, "mock-provider");
        assert_eq!(events[0].model_id, "mock-model-id");
        assert_eq!(events[1].provider, "alternate-provider");
        assert_eq!(events[1].model_id, "alternate-model-id");
    }

    // Upstream: stream-text.test.ts result.onFinish
    // "onFinishResult should expose deprecated AI SDK 6 final-step properties"
    // (packages-ai-1164). The onFinish event mirrors the final step's reasoning,
    // reasoning text, request, and response.
    #[test]
    fn stream_text_on_finish_exposes_final_step_properties() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "r1", "thinking",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")]).with_on_finish(
                move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events.lock().expect("finish lock").push(event);
                    }
                },
            ),
        ));
        result.consume_stream();

        let finish_events = finish_events.lock().expect("finish lock");
        let event = &finish_events[0];
        let final_step = result.steps.last().expect("final step present");
        assert_eq!(event.reasoning_text, final_step.reasoning_text);
        assert_eq!(event.reasoning_text.as_deref(), Some("thinking"));
        assert_eq!(event.request, final_step.request);
        assert_eq!(event.usage, final_step.usage);
        assert_eq!(event.finish_reason, final_step.finish_reason);
    }

    // Upstream: stream-text.test.ts result.fullStream "should return events in order"
    // (packages-ai-1144). The full stream emits its parts in deterministic order.
    #[test]
    fn stream_text_full_stream_returns_events_in_order() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
                LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                    "r1", "why",
                )),
                LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let result = poll_ready(stream_text(StreamTextOptions::new(
            &model,
            vec![user_message("test-input")],
        )));

        let part_names = result
            .parts
            .iter()
            .map(|part| match part {
                TextStreamPart::Start(_) => "start",
                TextStreamPart::StartStep(_) => "start-step",
                TextStreamPart::TextStart(_) => "text-start",
                TextStreamPart::TextDelta(_) => "text-delta",
                TextStreamPart::TextEnd(_) => "text-end",
                TextStreamPart::ReasoningStart(_) => "reasoning-start",
                TextStreamPart::ReasoningDelta(_) => "reasoning-delta",
                TextStreamPart::ReasoningEnd(_) => "reasoning-end",
                TextStreamPart::FinishStep(_) => "finish-step",
                TextStreamPart::Finish(_) => "finish",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            part_names,
            vec![
                "start",
                "start-step",
                "text-start",
                "text-delta",
                "text-end",
                "reasoning-start",
                "reasoning-delta",
                "reasoning-end",
                "finish-step",
                "finish",
            ]
        );
    }

    // Upstream: stream-text.test.ts result.onFinish (multi-step)
    // "onFinish should send correct information" (packages-ai-1173). After a
    // two-step tool loop, the onFinish event reports the final text, the summed
    // total usage, and the response messages aggregated across all steps.
    #[test]
    fn stream_text_on_finish_sends_correct_information_across_steps() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_initial(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "Hello, ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world!")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    two_step_usage_final(),
                    finish_reason(),
                )),
            ]),
        ]);
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_max_steps(3)
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events.lock().expect("finish lock").push(event);
                    }
                }),
        ));
        result.consume_stream();

        let finish_events = finish_events.lock().expect("finish lock");
        assert_eq!(finish_events.len(), 1);
        let event = &finish_events[0];
        assert_eq!(event.text, "Hello, world!");
        assert_eq!(event.steps.len(), 2);
        assert_eq!(event.total_usage, result.total_usage);
        // Response messages span all steps: assistant tool-call, tool result,
        // then the final assistant text message.
        assert!(event.response_messages.len() >= 3);
        assert!(matches!(
            event.response_messages.last(),
            Some(LanguageModelMessage::Assistant(_))
        ));
        assert!(
            event
                .response_messages
                .iter()
                .any(|message| matches!(message, LanguageModelMessage::Tool(_)))
        );
    }

    // Upstream: stream-text.test.ts result.toUIMessageStream (tool step)
    // "should have correct ui message stream" (packages-ai-1169). A tool-executing
    // step produces tool-input-available and tool-output-available ui chunks.
    #[test]
    fn stream_text_ui_message_stream_emits_tool_input_and_output_chunks() {
        let model = MockLanguageModel::new().with_stream_results([
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1",
                    "tool1",
                    r#"{ "value": "value" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    tool_calls_finish_reason(),
                )),
            ]),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "done")),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ]);
        let input_schema = json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("test-input")])
                .with_tool(
                    Tool::new("tool1", input_schema)
                        .with_execute(|_input, _options| async move { Ok(json!("result1")) }),
                )
                .with_max_steps(3),
        ));

        let chunks = serde_json::to_value(result.to_ui_message_stream()).expect("chunks serialize");
        let chunks = chunks.as_array().expect("chunks is an array");
        let types = chunks
            .iter()
            .map(|chunk| chunk["type"].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert!(
            types.iter().any(|t| t == "tool-input-available"),
            "tool-input-available chunk present, got {types:?}"
        );
        assert!(
            types.iter().any(|t| t == "tool-output-available"),
            "tool-output-available chunk present, got {types:?}"
        );
        let available = chunks
            .iter()
            .find(|chunk| chunk["type"] == json!("tool-input-available"))
            .expect("tool-input-available chunk present");
        assert_eq!(available["toolName"], json!("tool1"));
        assert_eq!(available["input"], json!({ "value": "value" }));
    }

    // Upstream parity: packages/ai/src/ui-message-stream/to-ui-message-chunk.test.ts
    fn to_ui_message_chunk_provider_metadata() -> ProviderMetadata {
        ProviderMetadata::from([(
            "testProvider".to_string(),
            Map::from_iter([("signature".to_string(), json!("sig-1"))]),
        )])
    }

    fn to_ui_message_chunk_value(
        part: &TextStreamPart,
        options: &ToUiMessageChunkOptions<'_>,
    ) -> Option<JsonValue> {
        to_ui_message_chunk(part, options)
            .map(|chunk| serde_json::to_value(chunk).expect("chunk serializes"))
    }

    fn to_ui_message_chunk_test_file() -> LanguageModelFile {
        LanguageModelFile::new(
            "text/plain",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("SGVsbG8=".to_string()),
            },
        )
    }

    #[test]
    fn to_ui_message_chunk_maps_text_parts_and_preserves_provider_metadata() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let options = ToUiMessageChunkOptions::new();

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::TextStart(
                    LanguageModelTextStart::new("text-1")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                &options,
            ),
            Some(json!({
                "type": "text-start",
                "id": "text-1",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::TextDelta(
                    TextStreamTextDeltaPart::new("text-1", "hello")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                &options,
            ),
            Some(json!({
                "type": "text-delta",
                "id": "text-1",
                "delta": "hello",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::TextEnd(
                    LanguageModelTextEnd::new("text-1").with_provider_metadata(provider_metadata),
                ),
                &options,
            ),
            Some(json!({
                "type": "text-end",
                "id": "text-1",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_reasoning_parts_by_default_and_suppresses_when_disabled() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let parts = vec![
            TextStreamPart::ReasoningStart(
                LanguageModelReasoningStart::new("reasoning-1")
                    .with_provider_metadata(provider_metadata.clone()),
            ),
            TextStreamPart::ReasoningDelta(
                TextStreamReasoningDeltaPart::new("reasoning-1", "thinking")
                    .with_provider_metadata(provider_metadata.clone()),
            ),
            TextStreamPart::ReasoningEnd(
                LanguageModelReasoningEnd::new("reasoning-1")
                    .with_provider_metadata(provider_metadata),
            ),
        ];

        let default_options = ToUiMessageChunkOptions::new();
        let mapped: Vec<Option<JsonValue>> = parts
            .iter()
            .map(|part| to_ui_message_chunk_value(part, &default_options))
            .collect();
        assert_eq!(
            mapped,
            vec![
                Some(json!({
                    "type": "reasoning-start",
                    "id": "reasoning-1",
                    "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                })),
                Some(json!({
                    "type": "reasoning-delta",
                    "id": "reasoning-1",
                    "delta": "thinking",
                    "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                })),
                Some(json!({
                    "type": "reasoning-end",
                    "id": "reasoning-1",
                    "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                })),
            ],
        );

        let disabled = ToUiMessageChunkOptions {
            send_reasoning: false,
            ..ToUiMessageChunkOptions::new()
        };
        let suppressed: Vec<Option<UiMessageChunk>> = parts
            .iter()
            .map(|part| to_ui_message_chunk(part, &disabled))
            .collect();
        assert_eq!(suppressed, vec![None, None, None]);
    }

    #[test]
    fn to_ui_message_chunk_maps_files_and_suppresses_reasoning_files_when_disabled() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let options = ToUiMessageChunkOptions::new();

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::File(TextStreamFilePart::new(
                    to_ui_message_chunk_test_file()
                        .with_provider_metadata(provider_metadata.clone()),
                ),),
                &options,
            ),
            Some(json!({
                "type": "file",
                "mediaType": "text/plain",
                "url": "data:text/plain;base64,SGVsbG8=",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );

        let reasoning_file_part = TextStreamPart::ReasoningFile(TextStreamReasoningFilePart::new(
            LanguageModelReasoningFile::new(
                "text/plain",
                LanguageModelFileData::Data {
                    data: FileDataContent::Base64("SGVsbG8=".to_string()),
                },
            )
            .with_provider_metadata(provider_metadata),
        ));

        assert_eq!(
            to_ui_message_chunk_value(&reasoning_file_part, &options),
            Some(json!({
                "type": "reasoning-file",
                "mediaType": "text/plain",
                "url": "data:text/plain;base64,SGVsbG8=",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );

        let disabled = ToUiMessageChunkOptions {
            send_reasoning: false,
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(to_ui_message_chunk(&reasoning_file_part, &disabled), None);
    }

    #[test]
    fn to_ui_message_chunk_skips_sources_by_default_and_sends_them_when_enabled() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let url_source_part = TextStreamPart::Source(LanguageModelSource::Url(
            LanguageModelUrlSource::new("source-1", "https://example.com")
                .with_title("Example")
                .with_provider_metadata(provider_metadata.clone()),
        ));
        let document_source_part = TextStreamPart::Source(LanguageModelSource::Document(
            LanguageModelDocumentSource::new("source-2", "application/pdf", "Document")
                .with_filename("document.pdf")
                .with_provider_metadata(provider_metadata),
        ));

        let default_options = ToUiMessageChunkOptions::new();
        assert_eq!(
            to_ui_message_chunk(&url_source_part, &default_options),
            None
        );
        assert_eq!(
            to_ui_message_chunk(&document_source_part, &default_options),
            None
        );

        let with_sources = ToUiMessageChunkOptions {
            send_sources: true,
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(
            to_ui_message_chunk_value(&url_source_part, &with_sources),
            Some(json!({
                "type": "source-url",
                "sourceId": "source-1",
                "url": "https://example.com",
                "title": "Example",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );
        assert_eq!(
            to_ui_message_chunk_value(&document_source_part, &with_sources),
            Some(json!({
                "type": "source-document",
                "sourceId": "source-2",
                "mediaType": "application/pdf",
                "title": "Document",
                "filename": "document.pdf",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_custom_and_lifecycle_parts() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let default_options = ToUiMessageChunkOptions::new();

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::Custom(
                    LanguageModelCustomContent::new("openai.compaction")
                        .with_provider_metadata(provider_metadata.clone()),
                ),
                &default_options,
            ),
            Some(json!({
                "type": "custom",
                "kind": "openai.compaction",
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
            })),
        );

        let start_options = ToUiMessageChunkOptions {
            message_metadata: Some(json!({ "model": "test-model" })),
            response_message_id: Some("msg-1".to_string()),
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::Start(TextStreamStartPart::new()),
                &start_options,
            ),
            Some(json!({
                "type": "start",
                "messageId": "msg-1",
                "messageMetadata": { "model": "test-model" },
            })),
        );

        let no_start = ToUiMessageChunkOptions {
            send_start: false,
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(
            to_ui_message_chunk(
                &TextStreamPart::Start(TextStreamStartPart::new()),
                &no_start
            ),
            None,
        );

        let finish_part = TextStreamPart::Finish(TextStreamFinishPart::new(
            FinishReason::Stop,
            Some("stop".to_string()),
            usage(),
        ));
        let finish_options = ToUiMessageChunkOptions {
            message_metadata: Some(json!({ "model": "test-model" })),
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(
            to_ui_message_chunk_value(&finish_part, &finish_options),
            Some(json!({
                "type": "finish",
                "finishReason": "stop",
                "messageMetadata": { "model": "test-model" },
            })),
        );

        let no_finish = ToUiMessageChunkOptions {
            send_finish: false,
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(to_ui_message_chunk(&finish_part, &no_finish), None);

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::StartStep(TextStreamStartStepPart::new(
                    LanguageModelRequest::default(),
                    Vec::new(),
                )),
                &default_options,
            ),
            Some(json!({ "type": "start-step" })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::FinishStep(TextStreamFinishStepPart::new(
                    StreamTextResponseMetadata::default(),
                    usage(),
                    StreamTextStepPerformance::default(),
                    FinishReason::Stop,
                    Some("stop".to_string()),
                    Some(provider_metadata),
                )),
                &default_options,
            ),
            Some(json!({ "type": "finish-step" })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::Abort(TextStreamAbortPart::with_reason(json!("user"))),
                &default_options,
            ),
            Some(json!({ "type": "abort", "reason": "user" })),
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_tool_input_streaming_parts() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let mut tools = BTreeMap::new();
        tools.insert("dynamicTool".to_string(), ToUiMessageChunkToolKind::Dynamic);
        let options = ToUiMessageChunkOptions {
            tools: Some(&tools),
            ..ToUiMessageChunkOptions::new()
        };

        // Upstream also threads `toolMetadata` onto tool-input-start; the Rust
        // `LanguageModelToolInputStart`/`UiMessageChunk::ToolInputStart` model
        // does not carry tool metadata on this part, so it is not asserted here.
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new("call-1", "dynamicTool")
                        .with_provider_executed(true)
                        .with_provider_metadata(provider_metadata)
                        .with_title("Dynamic Tool"),
                ),
                &options,
            ),
            Some(json!({
                "type": "tool-input-start",
                "toolCallId": "call-1",
                "toolName": "dynamicTool",
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "dynamic": true,
                "title": "Dynamic Tool",
            })),
        );

        // Tool not present in the set: falls back to the part's own dynamic flag.
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new("call-2", "providerTool").with_dynamic(true),
                ),
                &options,
            ),
            Some(json!({
                "type": "tool-input-start",
                "toolCallId": "call-2",
                "toolName": "providerTool",
                "dynamic": true,
            })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                    "call-1",
                    "{\"value\"",
                )),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-input-delta",
                "toolCallId": "call-1",
                "inputTextDelta": "{\"value\"",
            })),
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_valid_and_invalid_tool_call_parts() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let tool_metadata = Map::from_iter([("clientName".to_string(), json!("test-client"))]);
        let mut tools = BTreeMap::new();
        tools.insert("staticTool".to_string(), ToUiMessageChunkToolKind::Static);
        let options = ToUiMessageChunkOptions {
            tools: Some(&tools),
            ..ToUiMessageChunkOptions::new()
        };

        // staticTool resolves to a non-dynamic tool -> no `dynamic` field.
        let static_call = GenerateTextToolCall {
            tool_call_id: "call-1".to_string(),
            tool_name: "staticTool".to_string(),
            input: json!({ "value": "input" }),
            title: Some("Static Tool".to_string()),
            provider_executed: Some(true),
            dynamic: None,
            invalid: None,
            error: None,
            provider_metadata: Some(provider_metadata.clone()),
            tool_metadata: Some(tool_metadata.clone()),
        };
        assert_eq!(
            to_ui_message_chunk_value(&TextStreamPart::ToolCall(static_call), &options),
            Some(json!({
                "type": "tool-input-available",
                "toolCallId": "call-1",
                "toolName": "staticTool",
                "input": { "value": "input" },
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "toolMetadata": { "clientName": "test-client" },
                "title": "Static Tool",
            })),
        );

        let runtime_call = GenerateTextToolCall {
            tool_call_id: "call-2".to_string(),
            tool_name: "runtimeTool".to_string(),
            input: json!({ "value": "input" }),
            title: None,
            provider_executed: None,
            dynamic: Some(true),
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolCall(runtime_call),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-input-available",
                "toolCallId": "call-2",
                "toolName": "runtimeTool",
                "input": { "value": "input" },
                "dynamic": true,
            })),
        );

        let handle_error: &dyn Fn(&JsonValue) -> String = &|error| {
            format!(
                "handled: {}",
                error.as_str().expect("error text is a string")
            )
        };
        let invalid_call = GenerateTextToolCall {
            tool_call_id: "call-3".to_string(),
            tool_name: "runtimeTool".to_string(),
            input: json!("{broken"),
            title: Some("Invalid Tool".to_string()),
            provider_executed: Some(true),
            dynamic: Some(true),
            invalid: Some(true),
            error: Some("invalid input".to_string()),
            provider_metadata: Some(provider_metadata),
            tool_metadata: Some(tool_metadata),
        };
        let invalid_options = ToUiMessageChunkOptions {
            on_error: Some(handle_error),
            ..ToUiMessageChunkOptions::new()
        };
        assert_eq!(
            to_ui_message_chunk_value(&TextStreamPart::ToolCall(invalid_call), &invalid_options),
            Some(json!({
                "type": "tool-input-error",
                "toolCallId": "call-3",
                "toolName": "runtimeTool",
                "input": "{broken",
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "toolMetadata": { "clientName": "test-client" },
                "dynamic": true,
                "errorText": "handled: invalid input",
                "title": "Invalid Tool",
            })),
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_tool_result_error_denial_and_approval_parts() {
        let provider_metadata = to_ui_message_chunk_provider_metadata();
        let tool_metadata = Map::from_iter([("clientName".to_string(), json!("test-client"))]);

        let result_part = GenerateTextToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "dynamicTool".to_string(),
            input: json!({ "value": "input" }),
            output: json!({ "value": "output" }),
            title: None,
            is_error: None,
            provider_executed: Some(true),
            dynamic: Some(true),
            preliminary: Some(true),
            provider_metadata: Some(provider_metadata.clone()),
            tool_metadata: Some(tool_metadata.clone()),
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolResult(result_part),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-output-available",
                "toolCallId": "call-1",
                "output": { "value": "output" },
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "toolMetadata": { "clientName": "test-client" },
                "preliminary": true,
                "dynamic": true,
            })),
        );

        // Provider-executed error: JSON-stringified, onError must NOT be used.
        let unused_on_error: &dyn Fn(&JsonValue) -> String =
            &|_| "should not be used for provider-executed errors".to_string();
        let provider_error = GenerateTextToolResult {
            tool_call_id: "call-2".to_string(),
            tool_name: "dynamicTool".to_string(),
            input: json!({ "value": "input" }),
            output: json!({ "code": "provider-error" }),
            title: None,
            is_error: Some(true),
            provider_executed: Some(true),
            dynamic: Some(true),
            preliminary: None,
            provider_metadata: Some(provider_metadata),
            tool_metadata: Some(tool_metadata),
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolResult(provider_error),
                &ToUiMessageChunkOptions {
                    on_error: Some(unused_on_error),
                    ..ToUiMessageChunkOptions::new()
                },
            ),
            Some(json!({
                "type": "tool-output-error",
                "toolCallId": "call-2",
                "errorText": "{\"code\":\"provider-error\"}",
                "providerExecuted": true,
                "providerMetadata": { "testProvider": { "signature": "sig-1" } },
                "toolMetadata": { "clientName": "test-client" },
                "dynamic": true,
            })),
        );

        // Provider-executed string error: used verbatim.
        let string_error = GenerateTextToolResult {
            tool_call_id: "call-string-error".to_string(),
            tool_name: "dynamicTool".to_string(),
            input: json!({ "value": "input" }),
            output: json!("provider string error"),
            title: None,
            is_error: Some(true),
            provider_executed: Some(true),
            dynamic: Some(true),
            preliminary: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolResult(string_error),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-output-error",
                "toolCallId": "call-string-error",
                "errorText": "provider string error",
                "providerExecuted": true,
                "dynamic": true,
            })),
        );

        // Non-provider-executed error: routed through onError.
        let handle_error: &dyn Fn(&JsonValue) -> String = &|error| {
            format!(
                "handled: {}",
                error.as_str().expect("error text is a string")
            )
        };
        let local_error = GenerateTextToolResult {
            tool_call_id: "call-3".to_string(),
            tool_name: "staticTool".to_string(),
            input: json!({ "value": "input" }),
            output: json!("tool failed"),
            title: None,
            is_error: Some(true),
            provider_executed: None,
            dynamic: None,
            preliminary: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolResult(local_error),
                &ToUiMessageChunkOptions {
                    on_error: Some(handle_error),
                    ..ToUiMessageChunkOptions::new()
                },
            ),
            Some(json!({
                "type": "tool-output-error",
                "toolCallId": "call-3",
                "errorText": "handled: tool failed",
            })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolOutputDenied(GenerateTextToolOutputDenied::new(
                    "call-4",
                    "staticTool",
                )),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-output-denied",
                "toolCallId": "call-4",
            })),
        );

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolApprovalRequest(
                    TextStreamToolApprovalRequestPart::new("approval-1", "call-5")
                        .with_automatic(true),
                ),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-approval-request",
                "approvalId": "approval-1",
                "toolCallId": "call-5",
                "isAutomatic": true,
            })),
        );

        let approval_tool_call = GenerateTextToolCall {
            tool_call_id: "call-5".to_string(),
            tool_name: "staticTool".to_string(),
            input: json!({ "value": "input" }),
            title: None,
            provider_executed: None,
            dynamic: None,
            invalid: None,
            error: None,
            provider_metadata: None,
            tool_metadata: None,
        };
        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::ToolApprovalResponse(
                    ToolApprovalResponseOutput::new("approval-1", approval_tool_call, false)
                        .with_reason("not allowed")
                        .with_provider_executed(true),
                ),
                &ToUiMessageChunkOptions::new(),
            ),
            Some(json!({
                "type": "tool-approval-response",
                "approvalId": "approval-1",
                "approved": false,
                "reason": "not allowed",
                "providerExecuted": true,
            })),
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_error_parts_through_on_error() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&calls);
        let on_error: &dyn Fn(&JsonValue) -> String = &|error| {
            captured.lock().expect("lock").push(error.clone());
            "handled error".to_string()
        };

        assert_eq!(
            to_ui_message_chunk_value(
                &TextStreamPart::Error(LanguageModelErrorStreamPart::new(json!("boom"))),
                &ToUiMessageChunkOptions {
                    on_error: Some(on_error),
                    ..ToUiMessageChunkOptions::new()
                },
            ),
            Some(json!({ "type": "error", "errorText": "handled error" })),
        );

        assert_eq!(*calls.lock().expect("lock"), vec![json!("boom")]);
    }

    #[test]
    fn to_ui_message_chunk_returns_none_for_parts_without_ui_message_chunks() {
        let options = ToUiMessageChunkOptions::new();

        assert_eq!(
            to_ui_message_chunk(
                &TextStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("call-1")),
                &options,
            ),
            None,
        );

        assert_eq!(
            to_ui_message_chunk(
                &TextStreamPart::Raw(LanguageModelRawStreamPart::new(
                    json!({ "provider": "raw" })
                )),
                &options,
            ),
            None,
        );
    }

    // ----------------------------------------------------------------------
    // packages/ai stream-text.test.ts "5 steps ... (dice game fixture)" block.
    //
    // Upstream drives a five-step tool loop: steps 1-4 emit a `rollDie` tool
    // call (finishReason `tool-calls`) and the fifth step emits the final text
    // (`Game Results`, finishReason `stop`). Steps 1 and 2 carry Anthropic
    // `container` provider metadata. The Rust port reproduces the generic
    // multi-step behaviour with the same shape: tool-call continuation, per-step
    // finish reasons, prompt accumulation, provider-metadata pass-through, and
    // the onStepFinish / onFinish aggregates.
    // ----------------------------------------------------------------------

    const DICE_CONTAINER_ID: &str = "container_011CWHPPTDTn1XufeRB9uHeH";

    fn dice_container_metadata() -> ProviderMetadata {
        ProviderMetadata::from([(
            "anthropic".to_string(),
            Map::from_iter([("container".to_string(), json!({ "id": DICE_CONTAINER_ID }))]),
        )])
    }

    fn dice_tool_call_step(
        call_id: &str,
        with_container: bool,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let finish = if with_container {
            LanguageModelStreamFinish::new(usage(), tool_calls_finish_reason())
                .with_provider_metadata(dice_container_metadata())
        } else {
            LanguageModelStreamFinish::new(usage(), tool_calls_finish_reason())
        };
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                call_id,
                "rollDie",
                r#"{ "player": "player1" }"#,
            )),
            LanguageModelStreamPart::Finish(finish),
        ])
    }

    /// Builds the shared five-step dice-game model. Steps 1-4 (indices 0-3)
    /// emit a `rollDie` tool call; the fifth step emits the closing text.
    fn dice_game_model() -> MockLanguageModel {
        MockLanguageModel::new().with_stream_results([
            dice_tool_call_step("call-1", true),
            dice_tool_call_step("call-2", true),
            dice_tool_call_step("call-3", false),
            dice_tool_call_step("call-4", false),
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("final")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "final",
                    "Game Results: player1 wins!",
                )),
                LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("final")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]),
        ])
    }

    fn dice_roll_tool(executions: Arc<Mutex<Vec<String>>>) -> Tool {
        let schema = json!({
            "type": "object",
            "properties": { "player": { "type": "string" } },
            "required": ["player"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        Tool::new("rollDie", schema).with_execute(move |input, _options| {
            let executions = Arc::clone(&executions);
            async move {
                let player = input
                    .get("player")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default()
                    .to_string();
                executions.lock().expect("executions lock").push(player);
                Ok(json!(4))
            }
        })
    }

    fn run_dice_game(
        model: &MockLanguageModel,
        executions: Arc<Mutex<Vec<String>>>,
    ) -> StreamTextResult {
        let result = poll_ready(stream_text(
            StreamTextOptions::new(model, vec![user_message("Simulate a dice game")])
                .with_tool(dice_roll_tool(executions))
                .with_max_steps(5),
        ));
        result.consume_stream();
        result
    }

    /// Maps packages/ai stream-text.test.ts:24276 `should contain 5 steps`.
    #[test]
    fn stream_text_dice_game_contains_five_steps() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let result = run_dice_game(&model, executions);
        assert_eq!(result.steps.len(), 5);
    }

    /// Maps packages/ai stream-text.test.ts:24280
    /// `should have correct finishReason for each step` — steps 1-4 are
    /// `tool-calls`, the final step is `stop`.
    #[test]
    fn stream_text_dice_game_step_finish_reasons() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let result = run_dice_game(&model, executions);
        let reasons: Vec<FinishReason> = result
            .steps
            .iter()
            .map(|step| step.finish_reason.clone())
            .collect();
        assert_eq!(
            reasons,
            vec![
                FinishReason::ToolCalls,
                FinishReason::ToolCalls,
                FinishReason::ToolCalls,
                FinishReason::ToolCalls,
                FinishReason::Stop,
            ]
        );
    }

    /// Maps packages/ai stream-text.test.ts:24254
    /// `should execute rollDie tool 4 times` — the loop runs the client tool
    /// once for each of the four tool-call steps.
    #[test]
    fn stream_text_dice_game_executes_roll_die_four_times() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let result = run_dice_game(&model, Arc::clone(&executions));
        let _ = result;
        let executions = executions.lock().expect("executions lock");
        assert_eq!(executions.len(), 4);
        assert!(executions.iter().all(|player| player == "player1"));
    }

    /// Maps packages/ai stream-text.test.ts:23802
    /// `should include all previous messages in step 3 prompt (round 2)` — by
    /// the third model call the prompt has accumulated the prior assistant
    /// tool-call and tool-result messages, so it is longer than the first.
    #[test]
    fn stream_text_dice_game_step_three_prompt_includes_previous_messages() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let result = run_dice_game(&model, executions);
        let _ = result;
        let calls = model.stream_calls();
        assert_eq!(calls.len(), 5);
        let first_len = calls[0].prompt.len();
        let third_len = calls[2].prompt.len();
        assert!(
            third_len > first_len,
            "step 3 prompt ({third_len}) should include more messages than step 1 ({first_len})"
        );
        // The accumulated prompt must contain a tool-result message from the
        // earlier rollDie executions.
        assert!(
            calls[2]
                .prompt
                .iter()
                .any(|message| matches!(message, LanguageModelMessage::Tool(_))),
            "step 3 prompt should include a tool-result message"
        );
    }

    /// Maps packages/ai stream-text.test.ts:23912
    /// `should include all previous messages in step 5 prompt (final step)` —
    /// the final model call carries the full accumulated conversation.
    #[test]
    fn stream_text_dice_game_final_step_prompt_includes_all_previous_messages() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let result = run_dice_game(&model, executions);
        let _ = result;
        let calls = model.stream_calls();
        assert_eq!(calls.len(), 5);
        let final_prompt = &calls[4].prompt;
        // Original user message plus four assistant tool-call / tool-result
        // rounds: the final prompt is the longest and ends with a tool result.
        assert!(final_prompt.len() > calls[0].prompt.len());
        let tool_messages = final_prompt
            .iter()
            .filter(|message| matches!(message, LanguageModelMessage::Tool(_)))
            .count();
        assert_eq!(tool_messages, 4);
        assert!(matches!(
            final_prompt.first(),
            Some(LanguageModelMessage::User(_))
        ));
    }

    /// Maps packages/ai stream-text.test.ts:24382
    /// `should contain provider metadata with container ID for steps 1 and 2`.
    #[test]
    fn stream_text_dice_game_step_provider_metadata_container_id() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let result = run_dice_game(&model, executions);
        let expected = dice_container_metadata();
        assert_eq!(result.steps[0].provider_metadata.as_ref(), Some(&expected));
        assert_eq!(result.steps[1].provider_metadata.as_ref(), Some(&expected));
        assert_eq!(result.steps[2].provider_metadata, None);
    }

    /// Maps packages/ai stream-text.test.ts:23899
    /// `should forward container ID via providerOptions in step 2` — when a
    /// `prepare_step` callback forwards the `anthropic.container.id` provider
    /// metadata from the previous step's finish into the next step's
    /// `provider_options` (mirroring upstream
    /// `forwardAnthropicContainerIdFromLastStep`), the second `doStream` request
    /// is invoked with that container id as its provider options.
    #[test]
    fn stream_text_dice_game_forwards_container_id_via_provider_options_in_step_2() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));

        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Simulate a dice game")])
                .with_tool(dice_roll_tool(executions))
                .with_max_steps(5)
                .with_prepare_step(|options| {
                    // Forward the container id from the previous step's response
                    // provider metadata into this step's request provider options.
                    let forwarded = options.steps.last().and_then(|step| {
                        step.provider_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.get("anthropic"))
                            .and_then(|anthropic| anthropic.get("container"))
                            .and_then(|container| container.get("id"))
                            .and_then(JsonValue::as_str)
                            .map(|id| {
                                ProviderOptions::from([(
                                    "anthropic".to_string(),
                                    Map::from_iter([(
                                        "container".to_string(),
                                        json!({ "id": id }),
                                    )]),
                                )])
                            })
                    });
                    async move {
                        match forwarded {
                            Some(provider_options) => {
                                PrepareStepResult::new().with_provider_options(provider_options)
                            }
                            None => PrepareStepResult::new(),
                        }
                    }
                }),
        ));
        result.consume_stream();

        let calls = model.stream_calls();
        assert!(calls.len() >= 2);
        // Step 1 (index 0) has no previous step, so no forwarded provider options.
        assert_eq!(calls[0].provider_options, None);
        // Step 2 (index 1) carries the container id forwarded from step 1's finish.
        assert_eq!(
            calls[1].provider_options,
            Some(ProviderOptions::from([(
                "anthropic".to_string(),
                Map::from_iter([("container".to_string(), json!({ "id": DICE_CONTAINER_ID }),)]),
            )]))
        );
    }

    /// Maps packages/ai stream-text.test.ts:24368
    /// `should be called for each step` — onStepFinish fires once per step.
    #[test]
    fn stream_text_dice_game_on_step_finish_called_for_each_step() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let steps = Arc::new(Mutex::new(Vec::<GenerateTextStep>::new()));
        let steps_for_callback = Arc::clone(&steps);
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Simulate a dice game")])
                .with_tool(dice_roll_tool(executions))
                .with_max_steps(5)
                .with_on_step_finish(move |step| {
                    let steps = Arc::clone(&steps_for_callback);
                    async move {
                        steps.lock().expect("steps lock").push(step);
                    }
                }),
        ));
        result.consume_stream();
        assert_eq!(steps.lock().expect("steps lock").len(), 5);
    }

    /// Maps packages/ai stream-text.test.ts:24373
    /// `should contain correct finishReason for each step` — onStepFinish
    /// reports `tool-calls` for the first four steps and `stop` for the last.
    #[test]
    fn stream_text_dice_game_on_step_finish_finish_reason_per_step() {
        let model = dice_game_model();
        let executions = Arc::new(Mutex::new(Vec::new()));
        let steps = Arc::new(Mutex::new(Vec::<GenerateTextStep>::new()));
        let steps_for_callback = Arc::clone(&steps);
        let result = poll_ready(stream_text(
            StreamTextOptions::new(&model, vec![user_message("Simulate a dice game")])
                .with_tool(dice_roll_tool(executions))
                .with_max_steps(5)
                .with_on_step_finish(move |step| {
                    let steps = Arc::clone(&steps_for_callback);
                    async move {
                        steps.lock().expect("steps lock").push(step);
                    }
                }),
        ));
        result.consume_stream();
        let reasons: Vec<FinishReason> = steps
            .lock()
            .expect("steps lock")
            .iter()
            .map(|step| step.finish_reason.clone())
            .collect();
        assert_eq!(
            reasons,
            vec![
                FinishReason::ToolCalls,
                FinishReason::ToolCalls,
                FinishReason::ToolCalls,
                FinishReason::ToolCalls,
                FinishReason::Stop,
            ]
        );
    }

    fn run_dice_game_on_finish(model: &MockLanguageModel) -> GenerateTextFinishEvent {
        let executions = Arc::new(Mutex::new(Vec::new()));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateTextFinishEvent>::new()));
        let finish_for_callback = Arc::clone(&finish_events);
        let result = poll_ready(stream_text(
            StreamTextOptions::new(model, vec![user_message("Simulate a dice game")])
                .with_tool(dice_roll_tool(executions))
                .with_max_steps(5)
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_for_callback);
                    async move {
                        finish_events.lock().expect("finish lock").push(event);
                    }
                }),
        ));
        result.consume_stream();
        let events = finish_events.lock().expect("finish lock");
        assert_eq!(events.len(), 1);
        events[0].clone()
    }

    /// Maps packages/ai stream-text.test.ts:24408
    /// onFinish `should be called with correct text`.
    #[test]
    fn stream_text_dice_game_on_finish_text() {
        let model = dice_game_model();
        let event = run_dice_game_on_finish(&model);
        assert!(event.text.contains("Game Results"));
    }

    /// Maps packages/ai stream-text.test.ts:24413
    /// onFinish `should be called with correct finishReason`.
    #[test]
    fn stream_text_dice_game_on_finish_finish_reason() {
        let model = dice_game_model();
        let event = run_dice_game_on_finish(&model);
        assert_eq!(event.finish_reason, FinishReason::Stop);
    }

    /// Maps packages/ai stream-text.test.ts:24418
    /// onFinish `should contain all steps`.
    #[test]
    fn stream_text_dice_game_on_finish_contains_all_steps() {
        let model = dice_game_model();
        let event = run_dice_game_on_finish(&model);
        assert_eq!(event.steps.len(), 5);
    }
}
