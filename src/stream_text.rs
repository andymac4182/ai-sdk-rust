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
    ActiveTools, GenerateTextFinishEvent, GenerateTextModelInfo, GenerateTextOnFinish,
    GenerateTextOnLanguageModelCallEnd, GenerateTextOnLanguageModelCallStart, GenerateTextOnStart,
    GenerateTextOnStepFinish, GenerateTextOnStepStart, GenerateTextOnToolExecutionEnd,
    GenerateTextOnToolExecutionStart, GenerateTextStartEvent, GenerateTextStep,
    GenerateTextStepStartEvent, GenerateTextTool, GenerateTextToolCall,
    GenerateTextToolExecutionEndEvent, GenerateTextToolExecutionStartEvent, GenerateTextToolResult,
    LanguageModelCallEndEvent, LanguageModelCallStartEvent, StopCondition,
    ToolApprovalConfiguration, ToolCallRepair, ToolCallRepairOptions, ToolInputRefinement,
    ToolInputRefinementError, apply_generate_text_response_metadata, execute_tool_calls,
    filter_active_language_model_tools, generate_text_call_id,
    generate_text_tool_result_from_language_model_tool_result,
    invoke_tool_input_available_callback, invoke_tool_input_delta_callback,
    invoke_tool_input_start_callback, is_stop_condition_met, mark_runtime_dynamic_tool_calls,
    mark_tool_call_metadata, mark_tool_call_titles, mark_tool_result_metadata,
    mark_unavailable_tool_calls, refine_tool_inputs, refresh_generate_text_content,
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
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelPrompt,
    LanguageModelRawStreamPart, LanguageModelReasoning, LanguageModelReasoningEnd,
    LanguageModelReasoningFile, LanguageModelReasoningStart, LanguageModelRequest,
    LanguageModelResponse, LanguageModelSource, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResultResponse, LanguageModelText,
    LanguageModelTextEnd, LanguageModelTextStart, LanguageModelToolApprovalRequest,
    LanguageModelToolCall, LanguageModelToolChoice, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelToolResult,
    LanguageModelUsage,
};
use crate::prompt::{Prompt, prompt_has_url_files, standardize_prompt};
use crate::provider::{ApiCallError, InvalidPromptError, ProviderMetadata, ProviderOptions};
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

    /// Provider-executed tool approval request.
    ToolApprovalRequest(LanguageModelToolApprovalRequest),

    /// Generated tool call.
    ToolCall(GenerateTextToolCall),

    /// Provider-executed tool result.
    ToolResult(GenerateTextToolResult),

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

    /// Emit the custom detector's prefix match.
    Detector(SmoothStreamChunkDetector),
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
            on_language_model_call_start: None,
            on_language_model_call_end: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
            max_retries: DEFAULT_MAX_RETRIES,
            smooth_stream: None,
            transforms: Vec::new(),
            on_chunk: None,
            on_error: None,
            abort_signal: None,
            on_abort: None,
            max_steps: 1,
            stop_conditions: Vec::new(),
        }
    }

    /// Creates stream options from the high-level upstream prompt shape.
    pub fn from_prompt(model: &'a M, prompt: Prompt) -> Result<Self, InvalidPromptError> {
        let prompt = standardize_prompt(prompt)?.into_language_model_prompt();
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
            on_language_model_call_start: None,
            on_language_model_call_end: None,
            on_tool_execution_start: None,
            on_tool_execution_end: None,
            on_step_finish: None,
            on_finish: None,
            telemetry: None,
            max_retries: DEFAULT_MAX_RETRIES,
            smooth_stream: None,
            transforms: Vec::new(),
            on_chunk: None,
            on_error: None,
            abort_signal,
            on_abort: None,
            max_steps: 1,
            stop_conditions: Vec::new(),
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
        self.with_on_tool_execution_start(on_tool_execution_start)
    }

    /// Deprecated upstream alias for [`StreamTextOptions::with_on_tool_execution_end`].
    pub fn with_experimental_on_tool_call_finish<F, Fut>(self, on_tool_execution_end: F) -> Self
    where
        F: Fn(GenerateTextToolExecutionEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_tool_execution_end(on_tool_execution_end)
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
                        provider_metadata: part.provider_metadata.clone(),
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
        runtime_context,
        tools_context,
        experimental_sandbox,
        active_tools,
        tool_approval,
        tool_input_refinements,
        tool_call_repair,
        on_start,
        on_step_start,
        on_language_model_call_start,
        on_language_model_call_end,
        on_tool_execution_start,
        on_tool_execution_end,
        on_step_finish,
        on_finish,
        telemetry,
        max_retries,
        smooth_stream,
        transforms,
        on_chunk,
        on_error,
        abort_signal,
        on_abort,
        max_steps,
        stop_conditions,
    } = options;
    let telemetry_dispatcher = create_telemetry_dispatcher(telemetry);
    let include_raw_chunks = call_options.include_raw_chunks.unwrap_or(false);
    let mut parts = vec![TextStreamPart::Start(TextStreamStartPart::new())];
    let base_language_model_tools = call_options.tools.take();
    let mut current_prompt = call_options.prompt.clone();
    let initial_messages = current_prompt.clone();
    let active_tools_for_start = active_tools.clone();
    let active_tools = active_tools.as_deref();
    let call_id = generate_text_call_id();
    let max_steps = max_steps.max(1);
    let mut stream_steps = Vec::new();
    let mut generate_steps = Vec::new();
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
        };
        if let Some(on_start) = &on_start {
            on_start.start(start_event.clone()).await;
        }
        telemetry_dispatcher.on_start(&start_event);
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

        let step_prompt = current_prompt.clone();
        let step_tools =
            crate::generate_text::filter_active_tools(Some(tools.clone()), active_tools)
                .unwrap_or_default();
        let mut step_language_model_tools =
            filter_active_language_model_tools(base_language_model_tools.clone(), active_tools);

        if let Some(mut prepared_tools) = prepare_tools_with_context(
            &step_tools,
            Some(&tools_context),
            experimental_sandbox.as_ref(),
        ) {
            step_language_model_tools
                .get_or_insert_with(Vec::new)
                .append(&mut prepared_tools);
        }

        let mut step_call_options = call_options.clone();
        step_call_options.prompt = step_prompt.clone();
        step_call_options.tools = step_language_model_tools;
        append_stream_text_user_agent(&mut step_call_options);
        if prompt_has_url_files(&step_call_options.prompt) {
            let _ = model.supported_urls().await;
        }

        if on_step_start.is_some() || telemetry_dispatcher.is_enabled() {
            let step_start_event = GenerateTextStepStartEvent {
                call_id: call_id.clone(),
                provider: model.provider().to_string(),
                model_id: model.model_id().to_string(),
                step_number,
                messages: step_prompt.clone(),
                tools: step_call_options.tools.clone().unwrap_or_default(),
                tool_choice: step_call_options.tool_choice.clone(),
                active_tools: active_tools.map(|tools| tools.to_vec()),
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
                model.provider(),
                model.model_id(),
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
            model,
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
                experimental_sandbox: experimental_sandbox.as_ref(),
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

        mark_unavailable_tool_calls(
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

        let mut generate_step = collected_step.to_generate_text_step(
            call_id.clone(),
            step_number,
            GenerateTextModelInfo::new(model.provider(), model.model_id()),
            runtime_context.clone(),
            tools_context.clone(),
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
            parts.push(TextStreamPart::ToolApprovalRequest(
                LanguageModelToolApprovalRequest::new(
                    request.approval_id.clone(),
                    request.tool_call_id.clone(),
                ),
            ));
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
        let (local_tool_results, tool_execution_ms) = execute_tool_calls(
            &call_id,
            &step_tools,
            &executable_tool_calls,
            &step_prompt,
            &tools_context,
            &tool_approvals.blocked_tool_call_ids,
            (
                experimental_sandbox.as_ref(),
                on_tool_execution_start.as_ref(),
                on_tool_execution_end.as_ref(),
                Some(&telemetry_dispatcher),
            ),
        )
        .await;
        let local_tool_results =
            apply_stream_text_transforms_to_tool_results(local_tool_results, &transforms);
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
        let mut provider_content = Vec::new();
        let mut text_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();
        let mut reasoning_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();

        for part in parts {
            match part {
                TextStreamPart::TextStart(part) => {
                    text_blocks.insert(
                        part.id.clone(),
                        (String::new(), part.provider_metadata.clone()),
                    );
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
                        provider_content.push(text_language_model_content(
                            part.text.clone(),
                            part.provider_metadata.clone(),
                        ));
                    }
                }
                TextStreamPart::TextEnd(part) => {
                    if let Some((block_text, provider_metadata)) = text_blocks.remove(&part.id)
                        && !block_text.is_empty()
                    {
                        provider_content
                            .push(text_language_model_content(block_text, provider_metadata));
                    }
                }
                TextStreamPart::ReasoningStart(part) => {
                    reasoning_blocks.insert(
                        part.id.clone(),
                        (String::new(), part.provider_metadata.clone()),
                    );
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
                        provider_content.push(reasoning_language_model_content(
                            part.text.clone(),
                            part.provider_metadata.clone(),
                        ));
                    }
                }
                TextStreamPart::ReasoningEnd(part) => {
                    if let Some((block_text, provider_metadata)) = reasoning_blocks.remove(&part.id)
                        && !block_text.is_empty()
                    {
                        provider_content.push(reasoning_language_model_content(
                            block_text,
                            provider_metadata,
                        ));
                    }
                }
                TextStreamPart::ToolApprovalRequest(part) => {
                    provider_content.push(LanguageModelContent::ToolApprovalRequest(part.clone()));
                }
                TextStreamPart::ToolCall(part) => {
                    tool_calls.push(part.clone());
                    provider_content.push(LanguageModelContent::ToolCall(
                        language_model_tool_call_from_stream_text_tool_call(part),
                    ));
                }
                TextStreamPart::ToolResult(part) => {
                    tool_results.push(part.clone());
                    if let Some(tool_result) =
                        language_model_tool_result_from_stream_text_tool_result(part)
                    {
                        provider_content.push(LanguageModelContent::ToolResult(tool_result));
                    }
                }
                TextStreamPart::Custom(part) => {
                    custom_parts.push(part.clone());
                    provider_content.push(LanguageModelContent::Custom(part.clone()));
                }
                TextStreamPart::File(part) => {
                    files.push(part.file.clone());
                    provider_content.push(LanguageModelContent::File(part.file.clone()));
                }
                TextStreamPart::ReasoningFile(part) => {
                    reasoning_files.push(part.file.clone());
                    provider_content.push(LanguageModelContent::ReasoningFile(part.file.clone()));
                }
                TextStreamPart::Source(part) => {
                    sources.push(part.clone());
                    provider_content.push(LanguageModelContent::Source(part.clone()));
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

        for (_, (block_text, provider_metadata)) in text_blocks {
            if !block_text.is_empty() {
                provider_content.push(text_language_model_content(block_text, provider_metadata));
            }
        }

        for (_, (block_text, provider_metadata)) in reasoning_blocks {
            if !block_text.is_empty() {
                provider_content.push(reasoning_language_model_content(
                    block_text,
                    provider_metadata,
                ));
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
        self.provider_content = provider_content;
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
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut custom_parts = Vec::new();
    let mut errors = Vec::new();
    let mut usage = LanguageModelUsage::default();
    let mut finish_reason = FinishReason::Other;
    let mut raw_finish_reason = None;
    let mut provider_metadata = None;
    let mut provider_content = Vec::new();
    let mut text_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();
    let mut reasoning_blocks = BTreeMap::<String, (String, Option<ProviderMetadata>)>::new();
    let mut ongoing_tool_call_tool_names = BTreeMap::<String, String>::new();
    let mut aborted = false;
    let mut abort_reason = None;

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
                        parts.push(TextStreamPart::TextStart(part));
                    }
                    LanguageModelStreamPart::TextDelta(part) => {
                        if !part.delta.is_empty() {
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
                                provider_content.push(text_language_model_content(
                                    part.delta.clone(),
                                    part.provider_metadata.clone(),
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
                        if let Some((block_text, provider_metadata)) = text_blocks.remove(&part.id)
                            && !block_text.is_empty()
                        {
                            provider_content
                                .push(text_language_model_content(block_text, provider_metadata));
                        }
                        parts.push(TextStreamPart::TextEnd(part));
                    }
                    LanguageModelStreamPart::ReasoningStart(part) => {
                        reasoning_blocks.insert(
                            part.id.clone(),
                            (String::new(), part.provider_metadata.clone()),
                        );
                        parts.push(TextStreamPart::ReasoningStart(part));
                    }
                    LanguageModelStreamPart::ReasoningDelta(part) => {
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
                            provider_content.push(reasoning_language_model_content(
                                part.delta.clone(),
                                part.provider_metadata.clone(),
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
                        if let Some((block_text, provider_metadata)) =
                            reasoning_blocks.remove(&part.id)
                            && !block_text.is_empty()
                        {
                            provider_content.push(reasoning_language_model_content(
                                block_text,
                                provider_metadata,
                            ));
                        }
                        parts.push(TextStreamPart::ReasoningEnd(part));
                    }
                    LanguageModelStreamPart::ToolInputStart(part) => {
                        ongoing_tool_call_tool_names
                            .insert(part.id.clone(), part.tool_name.clone());
                        let tool = controls
                            .tools
                            .iter()
                            .find(|tool| tool.name == part.tool_name);
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
                        provider_content
                            .push(LanguageModelContent::ToolApprovalRequest(part.clone()));
                        parts.push(TextStreamPart::ToolApprovalRequest(part));
                    }
                    LanguageModelStreamPart::ToolCall(part) => {
                        let tool_call = GenerateTextToolCall::from_language_model_tool_call(&part);
                        let tool_name = ongoing_tool_call_tool_names
                            .remove(&tool_call.tool_call_id)
                            .unwrap_or_else(|| tool_call.tool_name.clone());
                        let tool = controls.tools.iter().find(|tool| tool.name == tool_name);
                        tool_calls.push(tool_call.clone());
                        provider_content.push(LanguageModelContent::ToolCall(part));
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
                        provider_content.push(LanguageModelContent::ToolResult(part));
                        push_text_stream_part(
                            parts,
                            TextStreamPart::ToolResult(tool_result),
                            controls.on_chunk,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::Custom(part) => {
                        custom_parts.push(part.clone());
                        provider_content.push(LanguageModelContent::Custom(part.clone()));
                        push_text_stream_part(
                            parts,
                            TextStreamPart::Custom(part),
                            controls.on_chunk,
                        )
                        .await;
                    }
                    LanguageModelStreamPart::File(part) => {
                        files.push(part.clone());
                        provider_content.push(LanguageModelContent::File(part.clone()));
                        parts.push(TextStreamPart::File(TextStreamFilePart::new(part)));
                    }
                    LanguageModelStreamPart::ReasoningFile(part) => {
                        reasoning_files.push(part.clone());
                        provider_content.push(LanguageModelContent::ReasoningFile(part.clone()));
                        parts.push(TextStreamPart::ReasoningFile(
                            TextStreamReasoningFilePart::new(part),
                        ));
                    }
                    LanguageModelStreamPart::Source(part) => {
                        sources.push(part.clone());
                        provider_content.push(LanguageModelContent::Source(part.clone()));
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

    for (_, (block_text, provider_metadata)) in text_blocks {
        if !block_text.is_empty() {
            provider_content.push(text_language_model_content(block_text, provider_metadata));
        }
    }

    for (_, (block_text, provider_metadata)) in reasoning_blocks {
        if !block_text.is_empty() {
            provider_content.push(reasoning_language_model_content(
                block_text,
                provider_metadata,
            ));
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
        custom_parts,
        errors,
        usage,
        finish_reason,
        raw_finish_reason,
        provider_metadata,
        performance,
        provider_content,
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
            | TextStreamPart::Custom(_)
            | TextStreamPart::Source(_)
            | TextStreamPart::Raw(_)
            | TextStreamPart::Abort(_)
    )
}

fn ui_message_error_text(error: &JsonValue, options: &StreamTextUiMessageStreamOptions) -> String {
    if let Some(on_error) = &options.on_error {
        return on_error.error_text(error);
    }

    default_ui_message_error_text(error)
}

fn default_ui_message_error_text(error: &JsonValue) -> String {
    error
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| "An error occurred.".to_string())
}

fn tool_call_error_text(error: Option<&str>, options: &StreamTextUiMessageStreamOptions) -> String {
    let error = error
        .map(|error| JsonValue::String(error.to_string()))
        .unwrap_or_else(|| JsonValue::String("An error occurred.".to_string()));
    ui_message_error_text(&error, options)
}

fn tool_result_error_text(
    tool_result: &GenerateTextToolResult,
    options: &StreamTextUiMessageStreamOptions,
) -> String {
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
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use serde_json::{Map, json};

    use super::*;
    use crate::file_data::{FileData, FileDataContent};
    use crate::generate_text::{ToolApprovalStatusKind, has_tool_call};
    use crate::json::NonNullJsonValue;
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModelAssistantContentPart,
        LanguageModelDocumentSource, LanguageModelErrorStreamPart, LanguageModelFile,
        LanguageModelFileData, LanguageModelFilePart, LanguageModelFinishReason,
        LanguageModelMessage, LanguageModelRawStreamPart, LanguageModelReasoningDelta,
        LanguageModelReasoningFile, LanguageModelStreamFinish, LanguageModelStreamResponseMetadata,
        LanguageModelStreamResult, LanguageModelStreamResultResponse, LanguageModelStreamStart,
        LanguageModelSystemMessage, LanguageModelTextDelta, LanguageModelTextPart,
        LanguageModelToolApprovalRequest, LanguageModelToolCall, LanguageModelToolContentPart,
        LanguageModelToolInputDelta, LanguageModelToolInputEnd, LanguageModelToolInputStart,
        LanguageModelToolResult, LanguageModelToolResultOutput, LanguageModelUrlSource,
        LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::mock_models::MockLanguageModel;
    use crate::prompt::Prompt;
    use crate::provider_utils::Tool;
    use crate::telemetry::{
        TelemetryEvent, TelemetryEventKind, TelemetryIntegration, TelemetryOptions,
    };
    use crate::ui_message_stream::UiMessageRole;
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

    fn tool_calls_finish_reason() -> LanguageModelFinishReason {
        LanguageModelFinishReason {
            unified: FinishReason::ToolCalls,
            raw: Some("tool_calls".to_string()),
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
                StreamTextStepPerformance { step_time_ms: 0 },
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
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
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
                    "abortSignalSet": false
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
                    "abortSignalSet": false
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
                    "abortSignalSet": false
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
                    "abortSignalSet": false
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
    fn stream_text_resolves_deferred_provider_tool_errors() {
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
                .with_on_start(move |event| {
                    let start_events = Arc::clone(&start_events);
                    async move {
                        assert_eq!(event.operation_id, "ai.streamText");
                        assert_eq!(event.messages.len(), 1);
                        assert_eq!(event.max_retries, DEFAULT_MAX_RETRIES);
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
                .with_tool_approval(
                    ToolApprovalConfiguration::new()
                        .with_tool_status("weather", ToolApprovalStatusKind::Denied),
                )
                .with_max_steps(2),
        ));

        assert_eq!(execute_count.load(Ordering::SeqCst), 0);
        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.text, "Request denied.");
        assert!(result.parts.iter().any(|part| {
            matches!(
                part,
                TextStreamPart::ToolApprovalRequest(request)
                    if request.tool_call_id == "call-1"
            )
        }));

        assert!(matches!(
            &model.stream_calls()[1].prompt[2],
            LanguageModelMessage::Tool(message)
                if message.content.len() == 2
                    && matches!(
                        &message.content[0],
                        LanguageModelToolContentPart::ToolApprovalResponse(response)
                            if !response.approved
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
}
