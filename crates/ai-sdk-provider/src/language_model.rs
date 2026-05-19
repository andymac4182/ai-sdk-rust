use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::file_data::{FileData, FileDataContent};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonSchema, JsonValue, NonNullJsonValue};
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

#[derive(Debug, Default)]
struct LanguageModelAbortState {
    aborted: AtomicBool,
    reason: Mutex<Option<JsonValue>>,
    wakers: Mutex<Vec<Waker>>,
}

/// Caller-controlled abort signal passed to provider language-model calls.
#[derive(Clone, Debug, Default)]
pub struct LanguageModelAbortSignal {
    state: Arc<LanguageModelAbortState>,
}

impl LanguageModelAbortSignal {
    /// Returns whether the model call has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.state.aborted.load(Ordering::SeqCst)
    }

    /// Returns the abort reason when one was supplied.
    pub fn reason(&self) -> Option<JsonValue> {
        self.state
            .reason
            .lock()
            .expect("language model abort reason lock is not poisoned")
            .clone()
    }

    /// Polls until the signal is aborted, registering the current task for wake-up.
    pub fn poll_aborted(&self, context: &Context<'_>) -> Poll<()> {
        if self.is_aborted() {
            return Poll::Ready(());
        }

        let mut wakers = self
            .state
            .wakers
            .lock()
            .expect("language model abort waker lock is not poisoned");

        if self.is_aborted() {
            return Poll::Ready(());
        }

        if !wakers.iter().any(|waker| waker.will_wake(context.waker())) {
            wakers.push(context.waker().clone());
        }

        Poll::Pending
    }
}

impl PartialEq for LanguageModelAbortSignal {
    fn eq(&self, other: &Self) -> bool {
        self.is_aborted() == other.is_aborted() && self.reason() == other.reason()
    }
}

/// Controller used to trigger a [`LanguageModelAbortSignal`].
#[derive(Clone, Debug, Default)]
pub struct LanguageModelAbortController {
    signal: LanguageModelAbortSignal,
}

impl LanguageModelAbortController {
    /// Creates a new abort controller.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a cloneable signal that can be passed to model calls.
    pub fn signal(&self) -> LanguageModelAbortSignal {
        self.signal.clone()
    }

    /// Aborts without a reason.
    pub fn abort(&self) {
        self.abort_inner(None);
    }

    /// Aborts with a reason.
    pub fn abort_with_reason(&self, reason: impl Into<JsonValue>) {
        self.abort_inner(Some(reason.into()));
    }

    fn abort_inner(&self, reason: Option<JsonValue>) {
        let already_aborted = self.signal.state.aborted.swap(true, Ordering::SeqCst);
        if !already_aborted {
            *self
                .signal
                .state
                .reason
                .lock()
                .expect("language model abort reason lock is not poisoned") = reason;
            let mut wakers = self
                .signal
                .state
                .wakers
                .lock()
                .expect("language model abort waker lock is not poisoned");
            for waker in wakers.drain(..) {
                waker.wake();
            }
        }
    }
}

/// Supported URL regular-expression patterns by media type for a language model.
///
/// Upstream uses JavaScript `RegExp` values. The Rust boundary stores their
/// regular-expression source strings and compiles them only where matching is
/// needed.
pub type LanguageModelSupportedUrls = BTreeMap<String, Vec<String>>;

/// A provider-v4 language model.
///
/// The upstream TypeScript contract exposes a `supportedUrls` property that may
/// be `PromiseLike`, plus `doGenerate` and `doStream` methods that return
/// `PromiseLike` values. This Rust trait maps those boundaries to associated
/// [`Future`] types without introducing an async-trait or async-stream
/// dependency.
pub trait LanguageModel {
    /// Future returned by [`LanguageModel::supported_urls`].
    type SupportedUrlsFuture<'a>: Future<Output = LanguageModelSupportedUrls> + Send + 'a
    where
        Self: 'a;

    /// Future returned by [`LanguageModel::do_generate`].
    type GenerateFuture<'a>: Future<Output = LanguageModelGenerateResult> + Send + 'a
    where
        Self: 'a;

    /// Stream abstraction returned inside [`LanguageModelStreamResult`].
    type Stream;

    /// Future returned by [`LanguageModel::do_stream`].
    type StreamFuture<'a>: Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a
    where
        Self: 'a;

    /// Returns the provider/model interface version implemented by this model.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Returns the provider identifier.
    fn provider(&self) -> &str;

    /// Returns the provider-specific model id.
    fn model_id(&self) -> &str;

    /// Returns supported URL regular-expression patterns grouped by media type.
    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_>;

    /// Generates a language model output without streaming.
    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_>;

    /// Generates a language model output stream.
    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_>;
}

/// Unified reason why a language model finished generating a response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    /// The model generated a stop sequence or otherwise finished normally.
    Stop,
    /// The model reached its maximum output length.
    Length,
    /// A content filter stopped generation.
    ContentFilter,
    /// The model emitted one or more tool calls.
    ToolCalls,
    /// The model stopped because of an error.
    Error,
    /// The provider reported another finish reason.
    Other,
}

/// Finish reason reported for a language model response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageModelFinishReason {
    /// Provider-independent finish reason.
    pub unified: FinishReason,

    /// Provider-specific raw finish reason, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// Usage information for input tokens in a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputTokenUsage {
    /// Total input tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    /// Non-cached input tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<u64>,

    /// Cached input tokens read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,

    /// Cached input tokens written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
}

/// Usage information for output tokens in a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTokenUsage {
    /// Total output tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    /// Text output tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<u64>,

    /// Reasoning output tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
}

/// Usage information for a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelUsage {
    /// Information about input tokens.
    pub input_tokens: InputTokenUsage,

    /// Information about output tokens.
    pub output_tokens: OutputTokenUsage,

    /// Raw provider usage information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<JsonObject>,
}

/// Provider response metadata for a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelResponseMetadata {
    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Start timestamp for the generated response, when one is available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub timestamp: Option<OffsetDateTime>,

    /// Provider model identifier used for the response, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// Optional request information for telemetry and debugging language model calls.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelRequest {
    /// Input messages sent to the model for a high-level generation step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<LanguageModelMessage>>,

    /// Request HTTP body that was sent to the provider API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl LanguageModelRequest {
    /// Creates empty request metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the raw provider request body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }

    /// Sets the input messages sent to the model for this request.
    pub fn with_messages(mut self, messages: Vec<LanguageModelMessage>) -> Self {
        self.messages = Some(messages);
        self
    }
}

/// Optional response information for telemetry and debugging language model calls.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelResponse {
    /// Response messages generated during a high-level generation step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<Vec<LanguageModelMessage>>,

    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Start timestamp for the generated response, when one is available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub timestamp: Option<OffsetDateTime>,

    /// Provider model identifier used for the response, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl LanguageModelResponse {
    /// Creates empty response metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the response messages generated during this step.
    pub fn with_messages(mut self, messages: Vec<LanguageModelMessage>) -> Self {
        self.messages = Some(messages);
        self
    }

    /// Sets the provider response identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the response start timestamp.
    pub fn with_timestamp(mut self, timestamp: OffsetDateTime) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the provider model identifier used for the response.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the raw provider response body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelTextKind {
    #[serde(rename = "text")]
    Text,
}

/// Text that the model has generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelText {
    #[serde(rename = "type")]
    kind: LanguageModelTextKind,

    /// The text content.
    pub text: String,

    /// Optional provider-specific metadata for the text part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelText {
    /// Creates a generated text part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelTextKind::Text,
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this generated text part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningKind {
    #[serde(rename = "reasoning")]
    Reasoning,
}

/// Reasoning that the model has generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoning {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningKind,

    /// The reasoning text content.
    pub text: String,

    /// Optional provider-specific metadata for the reasoning part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelReasoning {
    /// Creates a generated reasoning part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningKind::Reasoning,
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this generated reasoning part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelCustomContentType {
    #[serde(rename = "custom")]
    Custom,
}

/// Provider-specific generated content that does not map to a standardized part.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCustomContent {
    #[serde(rename = "type")]
    content_type: LanguageModelCustomContentType,

    /// Provider-specific kind in the `{provider}.{provider-type}` format.
    pub kind: String,

    /// Optional provider-specific metadata for the custom content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelCustomContent {
    /// Creates a provider-specific generated content part.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            content_type: LanguageModelCustomContentType::Custom,
            kind: kind.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this custom content part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// Generated file data returned by a language model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LanguageModelFileData {
    /// Raw bytes or base64-encoded generated file content.
    Data { data: FileDataContent },

    /// A URL pointing to the generated file.
    Url { url: Url },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelFileKind {
    #[serde(rename = "file")]
    File,
}

/// A file that the model has generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelFile {
    #[serde(rename = "type")]
    kind: LanguageModelFileKind,

    /// The IANA media type of the generated file.
    pub media_type: String,

    /// Generated file data.
    pub data: LanguageModelFileData,

    /// Optional provider-specific metadata for the file part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelFile {
    /// Creates a generated file part.
    pub fn new(media_type: impl Into<String>, data: LanguageModelFileData) -> Self {
        Self {
            kind: LanguageModelFileKind::File,
            media_type: media_type.into(),
            data,
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this generated file part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningFileKind {
    #[serde(rename = "reasoning-file")]
    ReasoningFile,
}

/// A file that the model has generated as part of reasoning.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningFile {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningFileKind,

    /// The IANA media type of the generated reasoning file.
    pub media_type: String,

    /// Generated file data.
    pub data: LanguageModelFileData,

    /// Optional provider-specific metadata for the reasoning file part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelReasoningFile {
    /// Creates a generated reasoning file part.
    pub fn new(media_type: impl Into<String>, data: LanguageModelFileData) -> Self {
        Self {
            kind: LanguageModelReasoningFileKind::ReasoningFile,
            media_type: media_type.into(),
            data,
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this reasoning file part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolApprovalRequestKind {
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest,
}

/// Tool approval request emitted for a provider-executed tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolApprovalRequest {
    #[serde(rename = "type")]
    kind: LanguageModelToolApprovalRequestKind,

    /// Identifier for the approval request.
    pub approval_id: String,

    /// Identifier of the tool call that requires approval.
    pub tool_call_id: String,

    /// Optional provider-specific metadata for the approval request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelToolApprovalRequest {
    /// Creates a provider-executed tool approval request.
    pub fn new(approval_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelToolApprovalRequestKind::ToolApprovalRequest,
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this tool approval request.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelSourceKind {
    #[serde(rename = "source")]
    Source,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelUrlSourceType {
    #[serde(rename = "url")]
    Url,
}

/// A URL source used as input to generate a language model response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelUrlSource {
    #[serde(rename = "type")]
    kind: LanguageModelSourceKind,

    source_type: LanguageModelUrlSourceType,

    /// Identifier for the source.
    pub id: String,

    /// URL string for the source.
    pub url: String,

    /// Optional title for the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Optional provider-specific metadata for the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelUrlSource {
    /// Creates a URL source.
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelSourceKind::Source,
            source_type: LanguageModelUrlSourceType::Url,
            id: id.into(),
            url: url.into(),
            title: None,
            provider_metadata: None,
        }
    }

    /// Sets the source title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds provider-specific metadata to this source.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelDocumentSourceType {
    #[serde(rename = "document")]
    Document,
}

/// A document source used as input to generate a language model response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelDocumentSource {
    #[serde(rename = "type")]
    kind: LanguageModelSourceKind,

    source_type: LanguageModelDocumentSourceType,

    /// Identifier for the source.
    pub id: String,

    /// The IANA media type of the source document.
    pub media_type: String,

    /// Title of the source document.
    pub title: String,

    /// Optional filename of the source document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Optional provider-specific metadata for the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelDocumentSource {
    /// Creates a document source.
    pub fn new(
        id: impl Into<String>,
        media_type: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            kind: LanguageModelSourceKind::Source,
            source_type: LanguageModelDocumentSourceType::Document,
            id: id.into(),
            media_type: media_type.into(),
            title: title.into(),
            filename: None,
            provider_metadata: None,
        }
    }

    /// Sets the source document filename.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Adds provider-specific metadata to this source.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// A source that was used as input to generate a language model response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelSource {
    /// Source that references web content.
    Url(LanguageModelUrlSource),

    /// Source that references a file or document.
    Document(LanguageModelDocumentSource),
}

impl LanguageModelSource {
    /// Creates a URL source.
    pub fn url(id: impl Into<String>, url: impl Into<String>) -> Self {
        Self::Url(LanguageModelUrlSource::new(id, url))
    }

    /// Creates a document source.
    pub fn document(
        id: impl Into<String>,
        media_type: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self::Document(LanguageModelDocumentSource::new(id, media_type, title))
    }

    /// Adds provider-specific metadata to this source.
    pub fn with_provider_metadata(self, provider_metadata: ProviderMetadata) -> Self {
        match self {
            Self::Url(source) => Self::Url(source.with_provider_metadata(provider_metadata)),
            Self::Document(source) => {
                Self::Document(source.with_provider_metadata(provider_metadata))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolCallKind {
    #[serde(rename = "tool-call")]
    ToolCall,
}

/// Tool call generated by a language model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolCall {
    #[serde(rename = "type")]
    kind: LanguageModelToolCallKind,

    /// Unique identifier for the tool call.
    pub tool_call_id: String,

    /// Name of the tool that should be called.
    pub tool_name: String,

    /// Stringified JSON object containing the tool call arguments.
    pub input: String,

    /// Whether the tool call will be executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool is dynamic and defined at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Optional provider-specific metadata for the tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelToolCall {
    /// Creates a generated tool call.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        Self {
            kind: LanguageModelToolCallKind::ToolCall,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input: input.into(),
            provider_executed: None,
            dynamic: None,
            provider_metadata: None,
        }
    }

    /// Sets whether the provider will execute this tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Sets whether this tool call is for a dynamic runtime-defined tool.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }

    /// Adds provider-specific metadata to this tool call.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolResultKind {
    #[serde(rename = "tool-result")]
    ToolResult,
}

/// Result of a provider-executed tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolResult {
    #[serde(rename = "type")]
    kind: LanguageModelToolResultKind,

    /// Identifier of the tool call this result is associated with.
    pub tool_call_id: String,

    /// Name of the tool that generated this result.
    pub tool_name: String,

    /// JSON-serializable, non-null result of the tool call.
    pub result: NonNullJsonValue,

    /// Whether the result is an error or an error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,

    /// Whether the tool result is preliminary and may be replaced by a later result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preliminary: Option<bool>,

    /// Whether the tool is dynamic and defined at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Optional provider-specific metadata for the tool result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelToolResult {
    /// Creates a provider-executed tool result.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        result: NonNullJsonValue,
    ) -> Self {
        Self {
            kind: LanguageModelToolResultKind::ToolResult,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            result,
            is_error: None,
            preliminary: None,
            dynamic: None,
            provider_metadata: None,
        }
    }

    /// Sets whether this tool result represents an error.
    pub fn with_is_error(mut self, is_error: bool) -> Self {
        self.is_error = Some(is_error);
        self
    }

    /// Sets whether this tool result is preliminary.
    pub fn with_preliminary(mut self, preliminary: bool) -> Self {
        self.preliminary = Some(preliminary);
        self
    }

    /// Sets whether this tool result came from a dynamic runtime-defined tool.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }

    /// Adds provider-specific metadata to this tool result.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// A generated content part returned by a language model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelContent {
    /// Generated text content.
    Text(LanguageModelText),

    /// Generated reasoning content.
    Reasoning(LanguageModelReasoning),

    /// Provider-specific generated content.
    Custom(LanguageModelCustomContent),

    /// Generated reasoning file content.
    ReasoningFile(LanguageModelReasoningFile),

    /// Generated file content.
    File(LanguageModelFile),

    /// Tool approval request content.
    ToolApprovalRequest(LanguageModelToolApprovalRequest),

    /// Source content used to generate the response.
    Source(LanguageModelSource),

    /// Generated tool call content.
    ToolCall(LanguageModelToolCall),

    /// Provider-executed tool result content.
    ToolResult(LanguageModelToolResult),
}

/// Result of a non-streaming language model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelGenerateResult {
    /// Ordered content that the model has generated.
    pub content: Vec<LanguageModelContent>,

    /// Reason why the model finished generating.
    pub finish_reason: LanguageModelFinishReason,

    /// Usage information for the model call.
    pub usage: LanguageModelUsage,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional request information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelResponse>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,
}

impl LanguageModelGenerateResult {
    /// Creates a language model generation result with no warnings.
    pub fn new(
        content: Vec<LanguageModelContent>,
        finish_reason: LanguageModelFinishReason,
        usage: LanguageModelUsage,
    ) -> Self {
        Self {
            content,
            finish_reason,
            usage,
            provider_metadata: None,
            request: None,
            response: None,
            warnings: Vec::new(),
        }
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets optional request information.
    pub fn with_request(mut self, request: LanguageModelRequest) -> Self {
        self.request = Some(request);
        self
    }

    /// Sets optional response information.
    pub fn with_response(mut self, response: LanguageModelResponse) -> Self {
        self.response = Some(response);
        self
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }
}

/// Optional response information for a streaming language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelStreamResultResponse {
    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl LanguageModelStreamResultResponse {
    /// Creates empty stream result response metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Result of a streaming language model provider call.
///
/// The upstream TypeScript contract uses a `ReadableStream` for `stream`. This
/// Rust wrapper keeps that field generic so callers can use their own stream
/// abstraction while preserving the provider-v4 metadata envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelStreamResult<S> {
    /// Stream of language model output parts.
    pub stream: S,

    /// Optional request information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelStreamResultResponse>,
}

impl<S> LanguageModelStreamResult<S> {
    /// Creates a language model stream result.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            request: None,
            response: None,
        }
    }

    /// Sets optional request information.
    pub fn with_request(mut self, request: LanguageModelRequest) -> Self {
        self.request = Some(request);
        self
    }

    /// Sets optional response information.
    pub fn with_response(mut self, response: LanguageModelStreamResultResponse) -> Self {
        self.response = Some(response);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelTextStartKind {
    #[serde(rename = "text-start")]
    TextStart,
}

/// Start of a streamed text block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelTextStart {
    #[serde(rename = "type")]
    kind: LanguageModelTextStartKind,

    /// Optional provider-specific metadata for the text block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Identifier for the streamed text block.
    pub id: String,
}

impl LanguageModelTextStart {
    /// Creates a streamed text block start part.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelTextStartKind::TextStart,
            provider_metadata: None,
            id: id.into(),
        }
    }

    /// Adds provider-specific metadata to this streamed text block.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelTextDeltaKind {
    #[serde(rename = "text-delta")]
    TextDelta,
}

/// Delta for a streamed text block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelTextDelta {
    #[serde(rename = "type")]
    kind: LanguageModelTextDeltaKind,

    /// Identifier for the streamed text block.
    pub id: String,

    /// Optional provider-specific metadata for the text delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Text delta emitted by the provider.
    pub delta: String,
}

impl LanguageModelTextDelta {
    /// Creates a streamed text delta part.
    pub fn new(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelTextDeltaKind::TextDelta,
            id: id.into(),
            provider_metadata: None,
            delta: delta.into(),
        }
    }

    /// Adds provider-specific metadata to this streamed text delta.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelTextEndKind {
    #[serde(rename = "text-end")]
    TextEnd,
}

/// End of a streamed text block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelTextEnd {
    #[serde(rename = "type")]
    kind: LanguageModelTextEndKind,

    /// Optional provider-specific metadata for the text block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Identifier for the streamed text block.
    pub id: String,
}

impl LanguageModelTextEnd {
    /// Creates a streamed text block end part.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelTextEndKind::TextEnd,
            provider_metadata: None,
            id: id.into(),
        }
    }

    /// Adds provider-specific metadata to this streamed text block.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningStartKind {
    #[serde(rename = "reasoning-start")]
    ReasoningStart,
}

/// Start of a streamed reasoning block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningStart {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningStartKind,

    /// Optional provider-specific metadata for the reasoning block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Identifier for the streamed reasoning block.
    pub id: String,
}

impl LanguageModelReasoningStart {
    /// Creates a streamed reasoning block start part.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningStartKind::ReasoningStart,
            provider_metadata: None,
            id: id.into(),
        }
    }

    /// Adds provider-specific metadata to this streamed reasoning block.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningDeltaKind {
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta,
}

/// Delta for a streamed reasoning block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningDelta {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningDeltaKind,

    /// Identifier for the streamed reasoning block.
    pub id: String,

    /// Optional provider-specific metadata for the reasoning delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Reasoning delta emitted by the provider.
    pub delta: String,
}

impl LanguageModelReasoningDelta {
    /// Creates a streamed reasoning delta part.
    pub fn new(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningDeltaKind::ReasoningDelta,
            id: id.into(),
            provider_metadata: None,
            delta: delta.into(),
        }
    }

    /// Adds provider-specific metadata to this streamed reasoning delta.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningEndKind {
    #[serde(rename = "reasoning-end")]
    ReasoningEnd,
}

/// End of a streamed reasoning block.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningEnd {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningEndKind,

    /// Identifier for the streamed reasoning block.
    pub id: String,

    /// Optional provider-specific metadata for the reasoning block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelReasoningEnd {
    /// Creates a streamed reasoning block end part.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningEndKind::ReasoningEnd,
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this streamed reasoning block.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolInputStartKind {
    #[serde(rename = "tool-input-start")]
    ToolInputStart,
}

/// Start of streamed input for a tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolInputStart {
    #[serde(rename = "type")]
    kind: LanguageModelToolInputStartKind,

    /// Identifier for the streamed tool input.
    pub id: String,

    /// Name of the tool being called.
    pub tool_name: String,

    /// Optional provider-specific metadata for the tool input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Whether the tool call will be executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool is dynamic and defined at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,

    /// Optional provider-supplied display title for the tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl LanguageModelToolInputStart {
    /// Creates a streamed tool input start part.
    pub fn new(id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelToolInputStartKind::ToolInputStart,
            id: id.into(),
            tool_name: tool_name.into(),
            provider_metadata: None,
            provider_executed: None,
            dynamic: None,
            title: None,
        }
    }

    /// Adds provider-specific metadata to this streamed tool input.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets whether the provider will execute this tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Sets whether this tool call is for a dynamic runtime-defined tool.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }

    /// Sets the provider-supplied display title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolInputDeltaKind {
    #[serde(rename = "tool-input-delta")]
    ToolInputDelta,
}

/// Delta for streamed input to a tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolInputDelta {
    #[serde(rename = "type")]
    kind: LanguageModelToolInputDeltaKind,

    /// Identifier for the streamed tool input.
    pub id: String,

    /// Tool input delta emitted by the provider.
    pub delta: String,

    /// Optional provider-specific metadata for the tool input delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelToolInputDelta {
    /// Creates a streamed tool input delta part.
    pub fn new(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelToolInputDeltaKind::ToolInputDelta,
            id: id.into(),
            delta: delta.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this streamed tool input delta.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolInputEndKind {
    #[serde(rename = "tool-input-end")]
    ToolInputEnd,
}

/// End of streamed input for a tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolInputEnd {
    #[serde(rename = "type")]
    kind: LanguageModelToolInputEndKind,

    /// Identifier for the streamed tool input.
    pub id: String,

    /// Optional provider-specific metadata for the tool input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelToolInputEnd {
    /// Creates a streamed tool input end part.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelToolInputEndKind::ToolInputEnd,
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this streamed tool input.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelStreamStartKind {
    #[serde(rename = "stream-start")]
    StreamStart,
}

/// Start of a language model stream, including call-level warnings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelStreamStart {
    #[serde(rename = "type")]
    kind: LanguageModelStreamStartKind,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,
}

impl LanguageModelStreamStart {
    /// Creates a stream start part.
    pub fn new(warnings: Vec<Warning>) -> Self {
        Self {
            kind: LanguageModelStreamStartKind::StreamStart,
            warnings,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelStreamResponseMetadataKind {
    #[serde(rename = "response-metadata")]
    #[default]
    ResponseMetadata,
}

/// Response metadata emitted after it becomes available during streaming.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelStreamResponseMetadata {
    #[serde(rename = "type")]
    kind: LanguageModelStreamResponseMetadataKind,

    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Start timestamp for the generated response, when one is available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub timestamp: Option<OffsetDateTime>,

    /// Provider model identifier used for the response, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

impl LanguageModelStreamResponseMetadata {
    /// Creates empty response metadata.
    pub fn new() -> Self {
        Self {
            kind: LanguageModelStreamResponseMetadataKind::ResponseMetadata,
            ..Self::default()
        }
    }

    /// Sets the provider response identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the response start timestamp.
    pub fn with_timestamp(mut self, timestamp: OffsetDateTime) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the provider model identifier used for the response.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelStreamFinishKind {
    #[serde(rename = "finish")]
    Finish,
}

/// Final metadata emitted after a language model stream finishes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelStreamFinish {
    #[serde(rename = "type")]
    kind: LanguageModelStreamFinishKind,

    /// Usage information for the model call.
    pub usage: LanguageModelUsage,

    /// Reason why the model finished generating.
    pub finish_reason: LanguageModelFinishReason,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelStreamFinish {
    /// Creates a stream finish part.
    pub fn new(usage: LanguageModelUsage, finish_reason: LanguageModelFinishReason) -> Self {
        Self {
            kind: LanguageModelStreamFinishKind::Finish,
            usage,
            finish_reason,
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelRawStreamPartKind {
    #[serde(rename = "raw")]
    Raw,
}

/// Raw provider chunk emitted when raw chunks are enabled.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelRawStreamPart {
    #[serde(rename = "type")]
    kind: LanguageModelRawStreamPartKind,

    /// Raw provider chunk represented as JSON.
    pub raw_value: JsonValue,
}

impl LanguageModelRawStreamPart {
    /// Creates a raw stream part.
    pub fn new(raw_value: JsonValue) -> Self {
        Self {
            kind: LanguageModelRawStreamPartKind::Raw,
            raw_value,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelErrorStreamPartKind {
    #[serde(rename = "error")]
    Error,
}

/// Error emitted during a language model stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelErrorStreamPart {
    #[serde(rename = "type")]
    kind: LanguageModelErrorStreamPartKind,

    /// Provider error represented as JSON.
    pub error: JsonValue,
}

impl LanguageModelErrorStreamPart {
    /// Creates an error stream part.
    pub fn new(error: JsonValue) -> Self {
        Self {
            kind: LanguageModelErrorStreamPartKind::Error,
            error,
        }
    }
}

/// A provider stream part emitted by a language model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelStreamPart {
    /// Start of a streamed text block.
    TextStart(LanguageModelTextStart),

    /// Delta for a streamed text block.
    TextDelta(LanguageModelTextDelta),

    /// End of a streamed text block.
    TextEnd(LanguageModelTextEnd),

    /// Start of a streamed reasoning block.
    ReasoningStart(LanguageModelReasoningStart),

    /// Delta for a streamed reasoning block.
    ReasoningDelta(LanguageModelReasoningDelta),

    /// End of a streamed reasoning block.
    ReasoningEnd(LanguageModelReasoningEnd),

    /// Start of streamed tool input.
    ToolInputStart(LanguageModelToolInputStart),

    /// Delta for streamed tool input.
    ToolInputDelta(LanguageModelToolInputDelta),

    /// End of streamed tool input.
    ToolInputEnd(LanguageModelToolInputEnd),

    /// Provider-executed tool approval request content.
    ToolApprovalRequest(LanguageModelToolApprovalRequest),

    /// Generated tool call content.
    ToolCall(LanguageModelToolCall),

    /// Provider-executed tool result content.
    ToolResult(LanguageModelToolResult),

    /// Provider-specific generated content.
    Custom(LanguageModelCustomContent),

    /// Generated file content.
    File(LanguageModelFile),

    /// Generated reasoning file content.
    ReasoningFile(LanguageModelReasoningFile),

    /// Source content used to generate the response.
    Source(LanguageModelSource),

    /// Stream start with call-level warnings.
    StreamStart(LanguageModelStreamStart),

    /// Response metadata emitted during streaming.
    ResponseMetadata(LanguageModelStreamResponseMetadata),

    /// Final usage and finish metadata.
    Finish(LanguageModelStreamFinish),

    /// Raw provider chunk.
    Raw(LanguageModelRawStreamPart),

    /// Provider stream error.
    Error(LanguageModelErrorStreamPart),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelFunctionToolKind {
    #[serde(rename = "function")]
    Function,
}

/// Example input for a function tool.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageModelToolInputExample {
    /// Example input object for the tool.
    pub input: JsonObject,
}

impl LanguageModelToolInputExample {
    /// Creates a function tool input example.
    pub fn new(input: JsonObject) -> Self {
        Self { input }
    }
}

/// Function tool definition made available to a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelFunctionTool {
    #[serde(rename = "type")]
    kind: LanguageModelFunctionToolKind,

    /// Name of the tool, unique within this model call.
    pub name: String,

    /// Description of the tool's purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema 7 object describing the tool input.
    pub input_schema: JsonSchema,

    /// Optional examples that show the model what inputs should look like.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_examples: Option<Vec<LanguageModelToolInputExample>>,

    /// Strict mode setting for providers that support it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,

    /// Provider-specific options for this tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelFunctionTool {
    /// Creates a function tool definition.
    pub fn new(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            kind: LanguageModelFunctionToolKind::Function,
            name: name.into(),
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
        }
    }

    /// Sets the tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a tool input example.
    pub fn with_input_example(mut self, input: JsonObject) -> Self {
        self.input_examples
            .get_or_insert_with(Vec::new)
            .push(LanguageModelToolInputExample::new(input));
        self
    }

    /// Sets strict mode for providers that support it.
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }

    /// Adds provider-specific options to this tool.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelProviderToolKind {
    #[serde(rename = "provider")]
    Provider,
}

/// Provider-specific tool definition made available to a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelProviderTool {
    #[serde(rename = "type")]
    kind: LanguageModelProviderToolKind,

    /// Provider tool identifier, typically `<provider-id>.<unique-tool-name>`.
    pub id: String,

    /// Name of the tool, unique within this model call.
    pub name: String,

    /// Provider-specific arguments for configuring the tool.
    pub args: JsonObject,
}

impl LanguageModelProviderTool {
    /// Creates a provider-specific tool definition.
    pub fn new(id: impl Into<String>, name: impl Into<String>, args: JsonObject) -> Self {
        Self {
            kind: LanguageModelProviderToolKind::Provider,
            id: id.into(),
            name: name.into(),
            args,
        }
    }
}

/// Tool definition made available to a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelTool {
    /// Function tool with a caller-defined JSON schema.
    Function(LanguageModelFunctionTool),

    /// Provider-defined tool with provider-specific arguments.
    Provider(LanguageModelProviderTool),
}

/// Strategy for selecting a tool during a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LanguageModelToolChoice {
    /// The model may choose whether to call a tool.
    Auto,

    /// The model must not call a tool.
    None,

    /// The model must call one of the available tools.
    Required,

    /// The model must call a specific tool.
    Tool {
        /// Name of the tool that must be selected.
        #[serde(rename = "toolName")]
        tool_name: String,
    },
}

/// Requested output format for a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LanguageModelResponseFormat {
    /// Plain text output.
    Text,

    /// JSON output, optionally constrained by a schema.
    Json {
        /// JSON schema that the generated output should conform to.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema: Option<JsonSchema>,

        /// Name of the output used by providers for additional guidance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,

        /// Description of the output used by providers for additional guidance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl LanguageModelResponseFormat {
    /// Creates a plain text response format.
    pub fn text() -> Self {
        Self::Text
    }

    /// Creates a JSON response format.
    pub fn json() -> Self {
        Self::Json {
            schema: None,
            name: None,
            description: None,
        }
    }

    /// Sets the JSON schema for a JSON response format.
    pub fn with_schema(self, schema: JsonSchema) -> Self {
        match self {
            Self::Json {
                name, description, ..
            } => Self::Json {
                schema: Some(schema),
                name,
                description,
            },
            other => other,
        }
    }

    /// Sets the JSON response name for a JSON response format.
    pub fn with_name(self, name: impl Into<String>) -> Self {
        match self {
            Self::Json {
                schema,
                description,
                ..
            } => Self::Json {
                schema,
                name: Some(name.into()),
                description,
            },
            other => other,
        }
    }

    /// Sets the JSON response description for a JSON response format.
    pub fn with_description(self, description: impl Into<String>) -> Self {
        match self {
            Self::Json { schema, name, .. } => Self::Json {
                schema,
                name,
                description: Some(description.into()),
            },
            other => other,
        }
    }
}

/// Reasoning effort requested for a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageModelReasoningEffort {
    /// Use the provider's default reasoning effort.
    ProviderDefault,
    /// Disable reasoning when supported.
    None,
    /// Use minimal reasoning effort.
    Minimal,
    /// Use low reasoning effort.
    Low,
    /// Use medium reasoning effort.
    Medium,
    /// Use high reasoning effort.
    High,
    /// Use extra-high reasoning effort.
    Xhigh,
}

/// Options passed to a language model provider call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCallOptions {
    /// Standardized prompt sent to the provider.
    pub prompt: LanguageModelPrompt,

    /// Maximum number of tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Temperature setting. The range depends on the provider and model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Stop sequences that stop generation when emitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Nucleus sampling setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Top-k sampling setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,

    /// Presence penalty setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Frequency penalty setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Requested response format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<LanguageModelResponseFormat>,

    /// Seed used for deterministic sampling when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Tools available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<LanguageModelTool>>,

    /// Tool selection strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<LanguageModelToolChoice>,

    /// Whether raw chunks should be included in streamed responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_raw_chunks: Option<bool>,

    /// Abort signal for cancelling the operation.
    #[serde(default, skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Reasoning effort requested for the model call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<LanguageModelReasoningEffort>,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelCallOptions {
    /// Creates language model call options with the required standardized prompt.
    pub fn new(prompt: LanguageModelPrompt) -> Self {
        Self {
            prompt,
            max_output_tokens: None,
            temperature: None,
            stop_sequences: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            response_format: None,
            seed: None,
            tools: None,
            tool_choice: None,
            include_raw_chunks: None,
            abort_signal: None,
            headers: None,
            reasoning: None,
            provider_options: None,
        }
    }

    /// Sets the maximum number of output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the sampling temperature.
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

    /// Sets nucleus sampling.
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Sets top-k sampling.
    pub fn with_top_k(mut self, top_k: u64) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets the presence penalty.
    pub fn with_presence_penalty(mut self, presence_penalty: f64) -> Self {
        self.presence_penalty = Some(presence_penalty);
        self
    }

    /// Sets the frequency penalty.
    pub fn with_frequency_penalty(mut self, frequency_penalty: f64) -> Self {
        self.frequency_penalty = Some(frequency_penalty);
        self
    }

    /// Sets the response format.
    pub fn with_response_format(mut self, response_format: LanguageModelResponseFormat) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Sets the deterministic sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Adds a tool that is available to the model.
    pub fn with_tool(mut self, tool: LanguageModelTool) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    /// Sets the tool selection strategy.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Sets whether raw stream chunks should be included.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.include_raw_chunks = Some(include_raw_chunks);
        self
    }

    /// Sets the abort signal for the model call.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the reasoning effort.
    pub fn with_reasoning(mut self, reasoning: LanguageModelReasoningEffort) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// A standardized prompt passed to a language model provider.
pub type LanguageModelPrompt = Vec<LanguageModelMessage>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelSystemMessageRole {
    #[serde(rename = "system")]
    System,
}

/// System message in a standardized language model prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelSystemMessage {
    role: LanguageModelSystemMessageRole,

    /// System instruction text.
    pub content: String,

    /// Provider-specific options for this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelSystemMessage {
    /// Creates a system prompt message.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            role: LanguageModelSystemMessageRole::System,
            content: content.into(),
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this message.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelUserMessageRole {
    #[serde(rename = "user")]
    User,
}

/// User message in a standardized language model prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelUserMessage {
    role: LanguageModelUserMessageRole,

    /// User-provided content parts.
    pub content: Vec<LanguageModelUserContentPart>,

    /// Provider-specific options for this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelUserMessage {
    /// Creates a user prompt message.
    pub fn new(content: Vec<LanguageModelUserContentPart>) -> Self {
        Self {
            role: LanguageModelUserMessageRole::User,
            content,
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this message.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelAssistantMessageRole {
    #[serde(rename = "assistant")]
    Assistant,
}

/// Assistant message in a standardized language model prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelAssistantMessage {
    role: LanguageModelAssistantMessageRole,

    /// Assistant-produced content parts.
    pub content: Vec<LanguageModelAssistantContentPart>,

    /// Provider-specific options for this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelAssistantMessage {
    /// Creates an assistant prompt message.
    pub fn new(content: Vec<LanguageModelAssistantContentPart>) -> Self {
        Self {
            role: LanguageModelAssistantMessageRole::Assistant,
            content,
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this message.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolMessageRole {
    #[serde(rename = "tool")]
    Tool,
}

/// Tool message in a standardized language model prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolMessage {
    role: LanguageModelToolMessageRole,

    /// Tool result or approval response content parts.
    pub content: Vec<LanguageModelToolContentPart>,

    /// Provider-specific options for this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelToolMessage {
    /// Creates a tool prompt message.
    pub fn new(content: Vec<LanguageModelToolContentPart>) -> Self {
        Self {
            role: LanguageModelToolMessageRole::Tool,
            content,
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this message.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// A message in a standardized language model prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelMessage {
    /// System instruction message.
    System(LanguageModelSystemMessage),

    /// User message with text or file input parts.
    User(LanguageModelUserMessage),

    /// Assistant message with generated prompt-history parts.
    Assistant(LanguageModelAssistantMessage),

    /// Tool message with tool results or approval responses.
    Tool(LanguageModelToolMessage),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelTextPartKind {
    #[serde(rename = "text")]
    Text,
}

/// Text content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelTextPart {
    #[serde(rename = "type")]
    kind: LanguageModelTextPartKind,

    /// The text content.
    pub text: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelTextPart {
    /// Creates a text prompt part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelTextPartKind::Text,
            text: text.into(),
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningPartKind {
    #[serde(rename = "reasoning")]
    Reasoning,
}

/// Reasoning content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningPart {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningPartKind,

    /// The reasoning text.
    pub text: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelReasoningPart {
    /// Creates a reasoning prompt part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningPartKind::Reasoning,
            text: text.into(),
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningFilePartKind {
    #[serde(rename = "reasoning-file")]
    ReasoningFile,
}

/// Reasoning file content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningFilePart {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningFilePartKind,

    /// Reasoning file data.
    pub data: LanguageModelFileData,

    /// The IANA media type of the reasoning file.
    pub media_type: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelReasoningFilePart {
    /// Creates a reasoning file prompt part.
    pub fn new(data: LanguageModelFileData, media_type: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningFilePartKind::ReasoningFile,
            data,
            media_type: media_type.into(),
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelCustomPartKind {
    #[serde(rename = "custom")]
    Custom,
}

/// Provider-specific prompt content part.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCustomPart {
    #[serde(rename = "type")]
    part_type: LanguageModelCustomPartKind,

    /// Provider-specific kind in the `{provider}.{provider-type}` format.
    pub kind: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelCustomPart {
    /// Creates a provider-specific prompt part.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            part_type: LanguageModelCustomPartKind::Custom,
            kind: kind.into(),
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelFilePartKind {
    #[serde(rename = "file")]
    File,
}

/// File content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelFilePart {
    #[serde(rename = "type")]
    kind: LanguageModelFilePartKind,

    /// Optional filename of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Prompt file data.
    pub data: FileData,

    /// The IANA media type or top-level media segment of the file.
    pub media_type: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelFilePart {
    /// Creates a file prompt part.
    pub fn new(data: FileData, media_type: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelFilePartKind::File,
            filename: None,
            data,
            media_type: media_type.into(),
            provider_options: None,
        }
    }

    /// Sets the file name.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolCallPartKind {
    #[serde(rename = "tool-call")]
    ToolCall,
}

/// Tool call content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolCallPart {
    #[serde(rename = "type")]
    kind: LanguageModelToolCallPartKind,

    /// ID of the tool call.
    pub tool_call_id: String,

    /// Name of the tool being called.
    pub tool_name: String,

    /// JSON-serializable tool call input.
    pub input: JsonValue,

    /// Whether the provider will execute this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelToolCallPart {
    /// Creates a tool call prompt part.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JsonValue,
    ) -> Self {
        Self {
            kind: LanguageModelToolCallPartKind::ToolCall,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            provider_executed: None,
            provider_options: None,
        }
    }

    /// Sets whether the provider will execute this tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolResultPartKind {
    #[serde(rename = "tool-result")]
    ToolResult,
}

/// Tool result content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolResultPart {
    #[serde(rename = "type")]
    kind: LanguageModelToolResultPartKind,

    /// ID of the matching tool call.
    pub tool_call_id: String,

    /// Name of the tool that generated the result.
    pub tool_name: String,

    /// Output of the tool call.
    pub output: LanguageModelToolResultOutput,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelToolResultPart {
    /// Creates a tool result prompt part.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        output: LanguageModelToolResultOutput,
    ) -> Self {
        Self {
            kind: LanguageModelToolResultPartKind::ToolResult,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            output,
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolApprovalResponsePartKind {
    #[serde(rename = "tool-approval-response")]
    ToolApprovalResponse,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolApprovalRequestPartKind {
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest,
}

/// Tool approval request content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolApprovalRequestPart {
    #[serde(rename = "type")]
    kind: LanguageModelToolApprovalRequestPartKind,

    /// ID of the approval request.
    pub approval_id: String,

    /// ID of the tool call that the approval request is for.
    pub tool_call_id: String,

    /// Whether the approval status was decided automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_automatic: Option<bool>,
}

impl LanguageModelToolApprovalRequestPart {
    /// Creates a tool approval request prompt part.
    pub fn new(approval_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelToolApprovalRequestPartKind::ToolApprovalRequest,
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            is_automatic: None,
        }
    }

    /// Sets whether the approval status was decided automatically.
    pub fn with_automatic(mut self, is_automatic: bool) -> Self {
        self.is_automatic = Some(is_automatic);
        self
    }
}

/// Tool approval response content part in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolApprovalResponsePart {
    #[serde(rename = "type")]
    kind: LanguageModelToolApprovalResponsePartKind,

    /// ID of the approval request.
    pub approval_id: String,

    /// Whether the approval was granted.
    pub approved: bool,

    /// Optional approval or denial reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelToolApprovalResponsePart {
    /// Creates a tool approval response prompt part.
    pub fn new(approval_id: impl Into<String>, approved: bool) -> Self {
        Self {
            kind: LanguageModelToolApprovalResponsePartKind::ToolApprovalResponse,
            approval_id: approval_id.into(),
            approved,
            reason: None,
            provider_options: None,
        }
    }

    /// Sets the approval or denial reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Content part allowed in user prompt messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelUserContentPart {
    /// Text content.
    Text(LanguageModelTextPart),

    /// File content.
    File(LanguageModelFilePart),
}

/// Content part allowed in assistant prompt messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelAssistantContentPart {
    /// Text content.
    Text(LanguageModelTextPart),

    /// File content.
    File(LanguageModelFilePart),

    /// Provider-specific custom content.
    Custom(LanguageModelCustomPart),

    /// Reasoning content.
    Reasoning(LanguageModelReasoningPart),

    /// Reasoning file content.
    ReasoningFile(LanguageModelReasoningFilePart),

    /// Tool call content.
    ToolCall(LanguageModelToolCallPart),

    /// Tool result content.
    ToolResult(LanguageModelToolResultPart),

    /// Tool approval request content.
    ToolApprovalRequest(LanguageModelToolApprovalRequestPart),
}

/// Content part allowed in tool prompt messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelToolContentPart {
    /// Tool result content.
    ToolResult(LanguageModelToolResultPart),

    /// Tool approval response content.
    ToolApprovalResponse(LanguageModelToolApprovalResponsePart),
}

/// Result of a tool call in a standardized prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LanguageModelToolResultOutput {
    /// Text tool output.
    Text {
        /// Text output value.
        value: String,

        /// Provider-specific options for this output.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// JSON tool output.
    Json {
        /// JSON output value.
        value: JsonValue,

        /// Provider-specific options for this output.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Execution denied by the user.
    ExecutionDenied {
        /// Optional denial reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,

        /// Provider-specific options for this output.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Text error output.
    ErrorText {
        /// Text error value.
        value: String,

        /// Provider-specific options for this output.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// JSON error output.
    ErrorJson {
        /// JSON error value.
        value: JsonValue,

        /// Provider-specific options for this output.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Multi-part tool output.
    Content {
        /// Content output parts.
        value: Vec<LanguageModelToolResultContentPart>,
    },
}

impl LanguageModelToolResultOutput {
    /// Creates a text tool output.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
            provider_options: None,
        }
    }

    /// Creates a JSON tool output.
    pub fn json(value: JsonValue) -> Self {
        Self::Json {
            value,
            provider_options: None,
        }
    }

    /// Creates an execution-denied tool output.
    pub fn execution_denied() -> Self {
        Self::ExecutionDenied {
            reason: None,
            provider_options: None,
        }
    }

    /// Creates a text error tool output.
    pub fn error_text(value: impl Into<String>) -> Self {
        Self::ErrorText {
            value: value.into(),
            provider_options: None,
        }
    }

    /// Creates a JSON error tool output.
    pub fn error_json(value: JsonValue) -> Self {
        Self::ErrorJson {
            value,
            provider_options: None,
        }
    }

    /// Creates a multi-part tool output.
    pub fn content(value: Vec<LanguageModelToolResultContentPart>) -> Self {
        Self::Content { value }
    }

    /// Adds provider-specific options to output variants that support them.
    pub fn with_provider_options(self, provider_options: ProviderOptions) -> Self {
        match self {
            Self::Text { value, .. } => Self::Text {
                value,
                provider_options: Some(provider_options),
            },
            Self::Json { value, .. } => Self::Json {
                value,
                provider_options: Some(provider_options),
            },
            Self::ExecutionDenied { reason, .. } => Self::ExecutionDenied {
                reason,
                provider_options: Some(provider_options),
            },
            Self::ErrorText { value, .. } => Self::ErrorText {
                value,
                provider_options: Some(provider_options),
            },
            Self::ErrorJson { value, .. } => Self::ErrorJson {
                value,
                provider_options: Some(provider_options),
            },
            Self::Content { value } => Self::Content { value },
        }
    }

    /// Sets the denial reason for an execution-denied output.
    pub fn with_reason(self, reason: impl Into<String>) -> Self {
        match self {
            Self::ExecutionDenied {
                provider_options, ..
            } => Self::ExecutionDenied {
                reason: Some(reason.into()),
                provider_options,
            },
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolResultCustomContentKind {
    #[serde(rename = "custom")]
    Custom,
}

/// Provider-specific custom content inside a multi-part tool result output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolResultCustomContent {
    #[serde(rename = "type")]
    kind: LanguageModelToolResultCustomContentKind,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl LanguageModelToolResultCustomContent {
    /// Creates a custom tool-result content part.
    pub fn new() -> Self {
        Self {
            kind: LanguageModelToolResultCustomContentKind::Custom,
            provider_options: None,
        }
    }

    /// Adds provider-specific options to this content part.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

impl Default for LanguageModelToolResultCustomContent {
    fn default() -> Self {
        Self::new()
    }
}

/// Content part inside a multi-part tool result output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LanguageModelToolResultContentPart {
    /// Text content.
    Text(LanguageModelTextPart),

    /// File content.
    File(LanguageModelFilePart),

    /// Provider-specific custom content.
    Custom(LanguageModelToolResultCustomContent),
}

#[cfg(test)]
mod tests {
    use super::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelAbortController,
        LanguageModelAssistantContentPart, LanguageModelAssistantMessage, LanguageModelCallOptions,
        LanguageModelContent, LanguageModelCustomContent, LanguageModelCustomPart,
        LanguageModelErrorStreamPart, LanguageModelFile, LanguageModelFileData,
        LanguageModelFilePart, LanguageModelFinishReason, LanguageModelFunctionTool,
        LanguageModelGenerateResult, LanguageModelMessage, LanguageModelPrompt,
        LanguageModelProviderTool, LanguageModelRawStreamPart, LanguageModelReasoning,
        LanguageModelReasoningDelta, LanguageModelReasoningEffort, LanguageModelReasoningEnd,
        LanguageModelReasoningFile, LanguageModelReasoningPart, LanguageModelReasoningStart,
        LanguageModelRequest, LanguageModelResponse, LanguageModelResponseFormat,
        LanguageModelResponseMetadata, LanguageModelSource, LanguageModelStreamFinish,
        LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
        LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
        LanguageModelSystemMessage, LanguageModelText, LanguageModelTextDelta,
        LanguageModelTextEnd, LanguageModelTextPart, LanguageModelTextStart, LanguageModelTool,
        LanguageModelToolApprovalRequest, LanguageModelToolApprovalRequestPart,
        LanguageModelToolApprovalResponsePart, LanguageModelToolCall, LanguageModelToolCallPart,
        LanguageModelToolChoice, LanguageModelToolContentPart, LanguageModelToolInputDelta,
        LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelToolMessage,
        LanguageModelToolResult, LanguageModelToolResultContentPart,
        LanguageModelToolResultCustomContent, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUrlSource, LanguageModelUsage,
        LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::file_data::{FileData, FileDataContent};
    use crate::json::NonNullJsonValue;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    use url::Url;

    struct StaticLanguageModel;

    impl LanguageModel for StaticLanguageModel {
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
            "language-test"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::from([(
                "image/*".to_string(),
                vec!["^https://cdn\\.example\\.com/images/".to_string()],
            )]))
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Text(LanguageModelText::new(
                    "generated text",
                ))],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            ]))
        }
    }

    fn poll_ready<T>(mut future: Ready<T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("std::future::Ready never returns pending"),
        }
    }

    #[test]
    fn language_model_trait_exposes_upstream_v4_boundaries() {
        let model = StaticLanguageModel;

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "language-test");

        assert_eq!(
            poll_ready(model.supported_urls()),
            BTreeMap::from([(
                "image/*".to_string(),
                vec!["^https://cdn\\.example\\.com/images/".to_string()],
            )])
        );

        let generate_result =
            poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        assert_eq!(generate_result.content.len(), 1);
        assert_eq!(generate_result.finish_reason.unified, FinishReason::Stop);

        let stream_result = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));
        assert_eq!(stream_result.stream.len(), 1);
    }

    #[test]
    fn supported_urls_serializes_as_media_type_pattern_map() {
        let supported_urls: LanguageModelSupportedUrls = BTreeMap::from([
            (
                "application/pdf".to_string(),
                vec![
                    "\\.pdf$".to_string(),
                    "^https://docs\\.example/".to_string(),
                ],
            ),
            (
                "image/*".to_string(),
                vec!["^https://cdn\\.example/images/".to_string()],
            ),
        ]);

        let value = serde_json::to_value(&supported_urls).expect("supported urls serialize");
        assert_eq!(
            value,
            json!({
                "application/pdf": ["\\.pdf$", "^https://docs\\.example/"],
                "image/*": ["^https://cdn\\.example/images/"]
            })
        );

        let round_tripped: LanguageModelSupportedUrls =
            serde_json::from_value(value).expect("supported urls deserialize");
        assert_eq!(round_tripped, supported_urls);
    }

    #[test]
    fn finish_reason_uses_upstream_kebab_case_names() {
        let reason = LanguageModelFinishReason {
            unified: FinishReason::ToolCalls,
            raw: Some("tool_calls".to_string()),
        };

        assert_eq!(
            serde_json::to_value(reason).expect("finish reason serializes"),
            json!({
                "unified": "tool-calls",
                "raw": "tool_calls"
            })
        );
    }

    #[test]
    fn usage_uses_upstream_camel_case_token_fields() {
        let usage = LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(120),
                cache_read: Some(40),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(32),
                reasoning: Some(8),
                ..OutputTokenUsage::default()
            },
            raw: Some(
                serde_json::from_value(json!({
                    "providerTotal": 152
                }))
                .expect("raw usage is a JSON object"),
            ),
        };

        assert_eq!(
            serde_json::to_value(usage).expect("usage serializes"),
            json!({
                "inputTokens": {
                    "total": 120,
                    "cacheRead": 40
                },
                "outputTokens": {
                    "total": 32,
                    "reasoning": 8
                },
                "raw": {
                    "providerTotal": 152
                }
            })
        );
    }

    #[test]
    fn usage_deserializes_when_optional_counts_are_missing() {
        let usage: LanguageModelUsage = serde_json::from_value(json!({
            "inputTokens": {},
            "outputTokens": {
                "text": 10
            }
        }))
        .expect("usage deserializes");

        assert_eq!(
            usage,
            LanguageModelUsage {
                input_tokens: InputTokenUsage::default(),
                output_tokens: OutputTokenUsage {
                    text: Some(10),
                    ..OutputTokenUsage::default()
                },
                raw: None,
            }
        );
    }

    #[test]
    fn response_metadata_uses_upstream_camel_case_and_rfc3339_timestamp() {
        let metadata = LanguageModelResponseMetadata {
            id: Some("resp_123".to_string()),
            timestamp: Some(
                OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses"),
            ),
            model_id: Some("openai/gpt-5".to_string()),
        };

        assert_eq!(
            serde_json::to_value(metadata).expect("response metadata serializes"),
            json!({
                "id": "resp_123",
                "timestamp": "2026-05-16T09:30:00Z",
                "modelId": "openai/gpt-5"
            })
        );
    }

    #[test]
    fn response_metadata_deserializes_when_optional_fields_are_missing() {
        let metadata: LanguageModelResponseMetadata = serde_json::from_value(json!({
            "modelId": "provider/model"
        }))
        .expect("response metadata deserializes");

        assert_eq!(
            metadata,
            LanguageModelResponseMetadata {
                model_id: Some("provider/model".to_string()),
                ..LanguageModelResponseMetadata::default()
            }
        );
    }

    #[test]
    fn text_part_serializes_upstream_shape_with_provider_metadata() {
        let text = LanguageModelText::new("Hello").with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "logprobs": true
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(text).expect("text part serializes"),
            json!({
                "type": "text",
                "text": "Hello",
                "providerMetadata": {
                    "openai": {
                        "logprobs": true
                    }
                }
            })
        );
    }

    #[test]
    fn text_part_deserializes_and_omits_missing_provider_metadata() {
        let text: LanguageModelText = serde_json::from_value(json!({
            "type": "text",
            "text": "Hello"
        }))
        .expect("text part deserializes");

        assert_eq!(text, LanguageModelText::new("Hello"));
        assert_eq!(
            serde_json::to_value(text).expect("text part serializes"),
            json!({
                "type": "text",
                "text": "Hello"
            })
        );
    }

    #[test]
    fn reasoning_part_serializes_upstream_shape_with_provider_metadata() {
        let reasoning = LanguageModelReasoning::new("I should check the source.")
            .with_provider_metadata(
                serde_json::from_value(json!({
                    "anthropic": {
                        "signature": "sig_123"
                    }
                }))
                .expect("provider metadata deserializes"),
            );

        assert_eq!(
            serde_json::to_value(reasoning).expect("reasoning part serializes"),
            json!({
                "type": "reasoning",
                "text": "I should check the source.",
                "providerMetadata": {
                    "anthropic": {
                        "signature": "sig_123"
                    }
                }
            })
        );
    }

    #[test]
    fn reasoning_part_rejects_other_content_types() {
        let error = serde_json::from_value::<LanguageModelReasoning>(json!({
            "type": "text",
            "text": "Not reasoning"
        }))
        .expect_err("wrong discriminator is rejected");

        assert!(error.to_string().contains("unknown variant `text`"));
    }

    #[test]
    fn custom_content_serializes_upstream_shape_with_provider_metadata() {
        let custom = LanguageModelCustomContent::new("openai.audio").with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "format": "wav"
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(custom).expect("custom content serializes"),
            json!({
                "type": "custom",
                "kind": "openai.audio",
                "providerMetadata": {
                    "openai": {
                        "format": "wav"
                    }
                }
            })
        );
    }

    #[test]
    fn custom_content_deserializes_and_omits_missing_provider_metadata() {
        let custom: LanguageModelCustomContent = serde_json::from_value(json!({
            "type": "custom",
            "kind": "provider.block"
        }))
        .expect("custom content deserializes");

        assert_eq!(custom, LanguageModelCustomContent::new("provider.block"));
        assert_eq!(
            serde_json::to_value(custom).expect("custom content serializes"),
            json!({
                "type": "custom",
                "kind": "provider.block"
            })
        );
    }

    #[test]
    fn file_part_serializes_upstream_data_shape_with_provider_metadata() {
        let file = LanguageModelFile::new(
            "image/png",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("iVBORw0KGgo=".to_string()),
            },
        )
        .with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "fileId": "file_123"
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(file).expect("file part serializes"),
            json!({
                "type": "file",
                "mediaType": "image/png",
                "data": {
                    "type": "data",
                    "data": "iVBORw0KGgo="
                },
                "providerMetadata": {
                    "openai": {
                        "fileId": "file_123"
                    }
                }
            })
        );
    }

    #[test]
    fn reasoning_file_part_deserializes_url_data_and_omits_missing_provider_metadata() {
        let reasoning_file: LanguageModelReasoningFile = serde_json::from_value(json!({
            "type": "reasoning-file",
            "mediaType": "application/pdf",
            "data": {
                "type": "url",
                "url": "https://example.com/reasoning.pdf"
            }
        }))
        .expect("reasoning file part deserializes");

        assert_eq!(
            reasoning_file,
            LanguageModelReasoningFile::new(
                "application/pdf",
                LanguageModelFileData::Url {
                    url: Url::parse("https://example.com/reasoning.pdf").expect("valid URL"),
                },
            )
        );
        assert_eq!(
            serde_json::to_value(reasoning_file).expect("reasoning file part serializes"),
            json!({
                "type": "reasoning-file",
                "mediaType": "application/pdf",
                "data": {
                    "type": "url",
                    "url": "https://example.com/reasoning.pdf"
                }
            })
        );
    }

    #[test]
    fn language_model_file_data_rejects_prompt_only_file_variants() {
        let error = serde_json::from_value::<LanguageModelFileData>(json!({
            "type": "reference",
            "reference": {
                "openai": "file_123"
            }
        }))
        .expect_err("reference data is rejected for generated file data");

        assert!(error.to_string().contains("unknown variant `reference`"));
    }

    #[test]
    fn tool_approval_request_serializes_upstream_shape_with_provider_metadata() {
        let request = LanguageModelToolApprovalRequest::new("approval_123", "tool_call_456")
            .with_provider_metadata(
                serde_json::from_value(json!({
                    "openai": {
                        "serverLabel": "mcp-server"
                    }
                }))
                .expect("provider metadata deserializes"),
            );

        assert_eq!(
            serde_json::to_value(request).expect("tool approval request serializes"),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval_123",
                "toolCallId": "tool_call_456",
                "providerMetadata": {
                    "openai": {
                        "serverLabel": "mcp-server"
                    }
                }
            })
        );
    }

    #[test]
    fn tool_approval_request_deserializes_and_omits_missing_provider_metadata() {
        let request: LanguageModelToolApprovalRequest = serde_json::from_value(json!({
            "type": "tool-approval-request",
            "approvalId": "approval_123",
            "toolCallId": "tool_call_456"
        }))
        .expect("tool approval request deserializes");

        assert_eq!(
            request,
            LanguageModelToolApprovalRequest::new("approval_123", "tool_call_456")
        );
        assert_eq!(
            serde_json::to_value(request).expect("tool approval request serializes"),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval_123",
                "toolCallId": "tool_call_456"
            })
        );
    }

    #[test]
    fn tool_approval_request_rejects_other_content_types() {
        let error = serde_json::from_value::<LanguageModelToolApprovalRequest>(json!({
            "type": "tool-call",
            "approvalId": "approval_123",
            "toolCallId": "tool_call_456"
        }))
        .expect_err("wrong discriminator is rejected");

        assert!(error.to_string().contains("unknown variant `tool-call`"));
    }

    #[test]
    fn url_source_serializes_upstream_shape_with_optional_title_and_metadata() {
        let source = LanguageModelUrlSource::new("source_123", "https://example.com/article")
            .with_title("Research article")
            .with_provider_metadata(
                serde_json::from_value(json!({
                    "google": {
                        "groundingChunk": 0
                    }
                }))
                .expect("provider metadata deserializes"),
            );

        assert_eq!(
            serde_json::to_value(LanguageModelSource::Url(source)).expect("source serializes"),
            json!({
                "type": "source",
                "sourceType": "url",
                "id": "source_123",
                "url": "https://example.com/article",
                "title": "Research article",
                "providerMetadata": {
                    "google": {
                        "groundingChunk": 0
                    }
                }
            })
        );
    }

    #[test]
    fn document_source_deserializes_and_omits_missing_optional_fields() {
        let source: LanguageModelSource = serde_json::from_value(json!({
            "type": "source",
            "sourceType": "document",
            "id": "doc_123",
            "mediaType": "application/pdf",
            "title": "Model card"
        }))
        .expect("document source deserializes");

        assert_eq!(
            source,
            LanguageModelSource::document("doc_123", "application/pdf", "Model card")
        );
        assert_eq!(
            serde_json::to_value(source).expect("document source serializes"),
            json!({
                "type": "source",
                "sourceType": "document",
                "id": "doc_123",
                "mediaType": "application/pdf",
                "title": "Model card"
            })
        );
    }

    #[test]
    fn source_rejects_other_content_types() {
        serde_json::from_value::<LanguageModelSource>(json!({
            "type": "tool-call",
            "sourceType": "url",
            "id": "source_123",
            "url": "https://example.com"
        }))
        .expect_err("wrong discriminator is rejected");
    }

    #[test]
    fn tool_call_serializes_upstream_shape_with_optional_flags_and_metadata() {
        let tool_call =
            LanguageModelToolCall::new("tool_call_123", "weather", r#"{"city":"Brisbane"}"#)
                .with_provider_executed(true)
                .with_dynamic(true)
                .with_provider_metadata(
                    serde_json::from_value(json!({
                        "openai": {
                            "itemId": "item_123"
                        }
                    }))
                    .expect("provider metadata deserializes"),
                );

        assert_eq!(
            serde_json::to_value(tool_call).expect("tool call serializes"),
            json!({
                "type": "tool-call",
                "toolCallId": "tool_call_123",
                "toolName": "weather",
                "input": "{\"city\":\"Brisbane\"}",
                "providerExecuted": true,
                "dynamic": true,
                "providerMetadata": {
                    "openai": {
                        "itemId": "item_123"
                    }
                }
            })
        );
    }

    #[test]
    fn tool_call_deserializes_and_omits_missing_optional_fields() {
        let tool_call: LanguageModelToolCall = serde_json::from_value(json!({
            "type": "tool-call",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "input": "{\"city\":\"Brisbane\"}"
        }))
        .expect("tool call deserializes");

        assert_eq!(
            tool_call,
            LanguageModelToolCall::new("tool_call_123", "weather", r#"{"city":"Brisbane"}"#)
        );
        assert_eq!(
            serde_json::to_value(tool_call).expect("tool call serializes"),
            json!({
                "type": "tool-call",
                "toolCallId": "tool_call_123",
                "toolName": "weather",
                "input": "{\"city\":\"Brisbane\"}"
            })
        );
    }

    #[test]
    fn tool_call_rejects_other_content_types() {
        let error = serde_json::from_value::<LanguageModelToolCall>(json!({
            "type": "tool-result",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "input": "{}"
        }))
        .expect_err("wrong discriminator is rejected");

        assert!(error.to_string().contains("unknown variant `tool-result`"));
    }

    #[test]
    fn tool_result_serializes_upstream_shape_with_optional_flags_and_metadata() {
        let tool_result = LanguageModelToolResult::new(
            "tool_call_123",
            "weather",
            NonNullJsonValue::new(json!({
                "temperatureCelsius": 24
            }))
            .expect("tool result is non-null"),
        )
        .with_is_error(false)
        .with_preliminary(true)
        .with_dynamic(true)
        .with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "itemId": "item_456"
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(tool_result).expect("tool result serializes"),
            json!({
                "type": "tool-result",
                "toolCallId": "tool_call_123",
                "toolName": "weather",
                "result": {
                    "temperatureCelsius": 24
                },
                "isError": false,
                "preliminary": true,
                "dynamic": true,
                "providerMetadata": {
                    "openai": {
                        "itemId": "item_456"
                    }
                }
            })
        );
    }

    #[test]
    fn tool_result_deserializes_and_omits_missing_optional_fields() {
        let tool_result: LanguageModelToolResult = serde_json::from_value(json!({
            "type": "tool-result",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "result": "sunny"
        }))
        .expect("tool result deserializes");

        assert_eq!(
            tool_result,
            LanguageModelToolResult::new(
                "tool_call_123",
                "weather",
                NonNullJsonValue::new(json!("sunny")).expect("tool result is non-null"),
            )
        );
        assert_eq!(
            serde_json::to_value(tool_result).expect("tool result serializes"),
            json!({
                "type": "tool-result",
                "toolCallId": "tool_call_123",
                "toolName": "weather",
                "result": "sunny"
            })
        );
    }

    #[test]
    fn tool_result_rejects_null_results() {
        let error = serde_json::from_value::<LanguageModelToolResult>(json!({
            "type": "tool-result",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "result": null
        }))
        .expect_err("null tool results are rejected");

        assert!(
            error
                .to_string()
                .contains("JSON values cannot be null in this position")
        );
    }

    #[test]
    fn tool_result_rejects_other_content_types() {
        let error = serde_json::from_value::<LanguageModelToolResult>(json!({
            "type": "tool-call",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "result": {}
        }))
        .expect_err("wrong discriminator is rejected");

        assert!(error.to_string().contains("unknown variant `tool-call`"));
    }

    #[test]
    fn content_union_serializes_upstream_generated_content_shapes() {
        assert_eq!(
            serde_json::to_value(LanguageModelContent::Text(LanguageModelText::new("Hello")))
                .expect("content serializes"),
            json!({
                "type": "text",
                "text": "Hello"
            })
        );

        assert_eq!(
            serde_json::to_value(LanguageModelContent::Source(LanguageModelSource::url(
                "source_123",
                "https://example.com"
            )))
            .expect("content serializes"),
            json!({
                "type": "source",
                "sourceType": "url",
                "id": "source_123",
                "url": "https://example.com"
            })
        );
    }

    #[test]
    fn content_union_deserializes_tool_result_variant() {
        let content: LanguageModelContent = serde_json::from_value(json!({
            "type": "tool-result",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "result": {
                "temperatureCelsius": 24
            }
        }))
        .expect("content deserializes");

        assert_eq!(
            content,
            LanguageModelContent::ToolResult(LanguageModelToolResult::new(
                "tool_call_123",
                "weather",
                NonNullJsonValue::new(json!({
                    "temperatureCelsius": 24
                }))
                .expect("tool result is non-null"),
            ))
        );
    }

    #[test]
    fn content_union_rejects_unknown_content_types() {
        serde_json::from_value::<LanguageModelContent>(json!({
            "type": "unsupported",
            "text": "No matching content variant"
        }))
        .expect_err("unsupported content variant is rejected");
    }

    #[test]
    fn request_metadata_serializes_high_level_messages_and_body() {
        let request = LanguageModelRequest::new()
            .with_messages(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Hello"),
                )]),
            )])
            .with_body(json!({
                "raw": true
            }));

        assert_eq!(
            serde_json::to_value(request).expect("request metadata serializes"),
            json!({
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Hello"
                            }
                        ]
                    }
                ],
                "body": {
                    "raw": true
                }
            })
        );

        let request: LanguageModelRequest =
            serde_json::from_value(json!({})).expect("minimal request metadata deserializes");
        assert_eq!(request.messages, None);
        assert_eq!(request.body, None);
    }

    #[test]
    fn response_metadata_serializes_high_level_messages_and_provider_data() {
        let timestamp =
            OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses");
        let response = LanguageModelResponse::new()
            .with_messages(vec![LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage::new(vec![LanguageModelAssistantContentPart::Text(
                    LanguageModelTextPart::new("Hello"),
                )]),
            )])
            .with_id("resp_123")
            .with_timestamp(timestamp)
            .with_model_id("openai/gpt-5")
            .with_header("x-request-id", "req_123")
            .with_body(json!({
                "id": "resp_123"
            }));

        assert_eq!(
            serde_json::to_value(response).expect("response metadata serializes"),
            json!({
                "messages": [
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
                "id": "resp_123",
                "timestamp": "2026-05-16T09:30:00Z",
                "modelId": "openai/gpt-5",
                "headers": {
                    "x-request-id": "req_123"
                },
                "body": {
                    "id": "resp_123"
                }
            })
        );

        let response: LanguageModelResponse =
            serde_json::from_value(json!({})).expect("minimal response metadata deserializes");
        assert_eq!(response.messages, None);
        assert_eq!(response.id, None);
        assert_eq!(response.timestamp, None);
        assert_eq!(response.model_id, None);
        assert_eq!(response.headers, None);
        assert_eq!(response.body, None);
    }

    #[test]
    fn generate_result_serializes_upstream_shape_with_request_response_and_warnings() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "cachedPromptTokens": 8
            }
        }))
        .expect("provider metadata deserializes");
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses");

        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new("Hello"))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(12),
                    cache_read: Some(8),
                    ..InputTokenUsage::default()
                },
                output_tokens: OutputTokenUsage {
                    total: Some(4),
                    text: Some(4),
                    ..OutputTokenUsage::default()
                },
                raw: Some(
                    serde_json::from_value(json!({
                        "totalTokens": 16
                    }))
                    .expect("raw usage is a JSON object"),
                ),
            },
        )
        .with_provider_metadata(provider_metadata)
        .with_request(LanguageModelRequest::new().with_body(json!({
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ]
        })))
        .with_response(
            LanguageModelResponse::new()
                .with_id("resp_123")
                .with_timestamp(response_timestamp)
                .with_model_id("openai/gpt-5")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "id": "resp_123"
                })),
        )
        .with_warning(Warning::Compatibility {
            feature: "json-mode".to_string(),
            details: None,
        });

        assert_eq!(
            serde_json::to_value(result).expect("generate result serializes"),
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": "Hello"
                    }
                ],
                "finishReason": {
                    "unified": "stop",
                    "raw": "stop"
                },
                "usage": {
                    "inputTokens": {
                        "total": 12,
                        "cacheRead": 8
                    },
                    "outputTokens": {
                        "total": 4,
                        "text": 4
                    },
                    "raw": {
                        "totalTokens": 16
                    }
                },
                "providerMetadata": {
                    "openai": {
                        "cachedPromptTokens": 8
                    }
                },
                "request": {
                    "body": {
                        "messages": [
                            {
                                "role": "user",
                                "content": "Hello"
                            }
                        ]
                    }
                },
                "response": {
                    "id": "resp_123",
                    "timestamp": "2026-05-16T09:30:00Z",
                    "modelId": "openai/gpt-5",
                    "headers": {
                        "x-request-id": "req_123"
                    },
                    "body": {
                        "id": "resp_123"
                    }
                },
                "warnings": [
                    {
                        "type": "compatibility",
                        "feature": "json-mode"
                    }
                ]
            })
        );
    }

    #[test]
    fn generate_result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: LanguageModelGenerateResult = serde_json::from_value(json!({
            "content": [
                {
                    "type": "text",
                    "text": "Hello"
                }
            ],
            "finishReason": {
                "unified": "stop"
            },
            "usage": {
                "inputTokens": {},
                "outputTokens": {}
            },
            "warnings": []
        }))
        .expect("generate result deserializes");

        assert_eq!(
            result,
            LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Text(LanguageModelText::new("Hello"))],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("generate result serializes"),
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": "Hello"
                    }
                ],
                "finishReason": {
                    "unified": "stop"
                },
                "usage": {
                    "inputTokens": {},
                    "outputTokens": {}
                },
                "warnings": []
            })
        );
    }

    #[test]
    fn stream_result_serializes_upstream_metadata_envelope() {
        let result = LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text_1", "Hello")),
        ])
        .with_request(LanguageModelRequest::new().with_body(json!({
            "messages": [
                {
                    "role": "user",
                    "content": "Hello"
                }
            ]
        })))
        .with_response(
            LanguageModelStreamResultResponse::new().with_header("x-request-id", "req_123"),
        );

        assert_eq!(
            serde_json::to_value(result).expect("stream result serializes"),
            json!({
                "stream": [
                    {
                        "type": "stream-start",
                        "warnings": []
                    },
                    {
                        "type": "text-delta",
                        "id": "text_1",
                        "delta": "Hello"
                    }
                ],
                "request": {
                    "body": {
                        "messages": [
                            {
                                "role": "user",
                                "content": "Hello"
                            }
                        ]
                    }
                },
                "response": {
                    "headers": {
                        "x-request-id": "req_123"
                    }
                }
            })
        );
    }

    #[test]
    fn stream_result_deserializes_minimal_stream_and_omits_metadata() {
        let result: LanguageModelStreamResult<Vec<LanguageModelStreamPart>> =
            serde_json::from_value(json!({
                "stream": [
                    {
                        "type": "text-start",
                        "id": "text_1"
                    }
                ]
            }))
            .expect("stream result deserializes");

        assert_eq!(
            result,
            LanguageModelStreamResult::new(vec![LanguageModelStreamPart::TextStart(
                LanguageModelTextStart::new("text_1"),
            )])
        );
        assert_eq!(
            serde_json::to_value(result).expect("stream result serializes"),
            json!({
                "stream": [
                    {
                        "type": "text-start",
                        "id": "text_1"
                    }
                ]
            })
        );
    }

    #[test]
    fn stream_block_parts_serialize_upstream_shapes() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "itemId": "item_123"
            }
        }))
        .expect("provider metadata deserializes");

        let parts = vec![
            LanguageModelStreamPart::TextStart(
                LanguageModelTextStart::new("text_1")
                    .with_provider_metadata(provider_metadata.clone()),
            ),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text_1", "Hel")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text_1")),
            LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("reason_1")),
            LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                "reason_1",
                "Check the source.",
            )),
            LanguageModelStreamPart::ReasoningEnd(
                LanguageModelReasoningEnd::new("reason_1")
                    .with_provider_metadata(provider_metadata.clone()),
            ),
            LanguageModelStreamPart::ToolInputStart(
                LanguageModelToolInputStart::new("tool_1", "weather")
                    .with_provider_executed(true)
                    .with_dynamic(true)
                    .with_title("Weather lookup")
                    .with_provider_metadata(provider_metadata),
            ),
            LanguageModelStreamPart::ToolInputDelta(LanguageModelToolInputDelta::new(
                "tool_1",
                r#"{"city""#,
            )),
            LanguageModelStreamPart::ToolInputEnd(LanguageModelToolInputEnd::new("tool_1")),
        ];

        assert_eq!(
            serde_json::to_value(parts).expect("stream parts serialize"),
            json!([
                {
                    "type": "text-start",
                    "providerMetadata": {
                        "openai": {
                            "itemId": "item_123"
                        }
                    },
                    "id": "text_1"
                },
                {
                    "type": "text-delta",
                    "id": "text_1",
                    "delta": "Hel"
                },
                {
                    "type": "text-end",
                    "id": "text_1"
                },
                {
                    "type": "reasoning-start",
                    "id": "reason_1"
                },
                {
                    "type": "reasoning-delta",
                    "id": "reason_1",
                    "delta": "Check the source."
                },
                {
                    "type": "reasoning-end",
                    "id": "reason_1",
                    "providerMetadata": {
                        "openai": {
                            "itemId": "item_123"
                        }
                    }
                },
                {
                    "type": "tool-input-start",
                    "id": "tool_1",
                    "toolName": "weather",
                    "providerMetadata": {
                        "openai": {
                            "itemId": "item_123"
                        }
                    },
                    "providerExecuted": true,
                    "dynamic": true,
                    "title": "Weather lookup"
                },
                {
                    "type": "tool-input-delta",
                    "id": "tool_1",
                    "delta": "{\"city\""
                },
                {
                    "type": "tool-input-end",
                    "id": "tool_1"
                }
            ])
        );
    }

    #[test]
    fn stream_lifecycle_parts_serialize_upstream_shapes() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "anthropic": {
                "stopSequence": "\n\nHuman:"
            }
        }))
        .expect("provider metadata deserializes");
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses");

        let parts = vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(vec![
                Warning::Unsupported {
                    feature: "topK".to_string(),
                    details: Some("The selected model ignores topK.".to_string()),
                },
            ])),
            LanguageModelStreamPart::ResponseMetadata(
                LanguageModelStreamResponseMetadata::new()
                    .with_id("resp_123")
                    .with_timestamp(response_timestamp)
                    .with_model_id("anthropic/claude-sonnet-4"),
            ),
            LanguageModelStreamPart::Finish(
                LanguageModelStreamFinish::new(
                    LanguageModelUsage {
                        input_tokens: InputTokenUsage {
                            total: Some(120),
                            ..InputTokenUsage::default()
                        },
                        output_tokens: OutputTokenUsage {
                            total: Some(32),
                            text: Some(24),
                            reasoning: Some(8),
                        },
                        raw: None,
                    },
                    LanguageModelFinishReason {
                        unified: FinishReason::Stop,
                        raw: Some("end_turn".to_string()),
                    },
                )
                .with_provider_metadata(provider_metadata),
            ),
            LanguageModelStreamPart::Raw(LanguageModelRawStreamPart::new(json!({
                "providerEvent": "chunk"
            }))),
            LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
                "message": "transient provider error"
            }))),
        ];

        assert_eq!(
            serde_json::to_value(parts).expect("stream lifecycle parts serialize"),
            json!([
                {
                    "type": "stream-start",
                    "warnings": [
                        {
                            "type": "unsupported",
                            "feature": "topK",
                            "details": "The selected model ignores topK."
                        }
                    ]
                },
                {
                    "type": "response-metadata",
                    "id": "resp_123",
                    "timestamp": "2026-05-16T09:30:00Z",
                    "modelId": "anthropic/claude-sonnet-4"
                },
                {
                    "type": "finish",
                    "usage": {
                        "inputTokens": {
                            "total": 120
                        },
                        "outputTokens": {
                            "total": 32,
                            "text": 24,
                            "reasoning": 8
                        }
                    },
                    "finishReason": {
                        "unified": "stop",
                        "raw": "end_turn"
                    },
                    "providerMetadata": {
                        "anthropic": {
                            "stopSequence": "\n\nHuman:"
                        }
                    }
                },
                {
                    "type": "raw",
                    "rawValue": {
                        "providerEvent": "chunk"
                    }
                },
                {
                    "type": "error",
                    "error": {
                        "message": "transient provider error"
                    }
                }
            ])
        );
    }

    #[test]
    fn stream_part_union_deserializes_generated_content_and_finish_variants() {
        let tool_call: LanguageModelStreamPart = serde_json::from_value(json!({
            "type": "tool-call",
            "toolCallId": "tool_call_123",
            "toolName": "weather",
            "input": "{\"city\":\"Brisbane\"}"
        }))
        .expect("tool call stream part deserializes");

        assert_eq!(
            tool_call,
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "tool_call_123",
                "weather",
                r#"{"city":"Brisbane"}"#,
            ))
        );

        let source: LanguageModelStreamPart = serde_json::from_value(json!({
            "type": "source",
            "sourceType": "url",
            "id": "source_123",
            "url": "https://example.com"
        }))
        .expect("source stream part deserializes");

        assert_eq!(
            source,
            LanguageModelStreamPart::Source(LanguageModelSource::url(
                "source_123",
                "https://example.com",
            ))
        );

        let finish: LanguageModelStreamPart = serde_json::from_value(json!({
            "type": "finish",
            "usage": {
                "inputTokens": {},
                "outputTokens": {}
            },
            "finishReason": {
                "unified": "stop"
            }
        }))
        .expect("finish stream part deserializes");

        assert_eq!(
            finish,
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                LanguageModelUsage::default(),
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
            ))
        );
    }

    #[test]
    fn stream_start_requires_warnings_array() {
        let error = serde_json::from_value::<LanguageModelStreamStart>(json!({
            "type": "stream-start"
        }))
        .expect_err("stream-start warnings are required");

        assert!(error.to_string().contains("missing field `warnings`"));
    }

    #[test]
    fn function_tool_serializes_upstream_shape_with_schema_examples_and_options() {
        let input_schema = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("input schema is a JSON object");

        let example = serde_json::from_value(json!({
            "city": "Brisbane"
        }))
        .expect("example input is a JSON object");

        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "strictJsonSchema": true
            }
        }))
        .expect("provider options deserialize");

        let tool = LanguageModelFunctionTool::new("weather", input_schema)
            .with_description("Get the current weather.")
            .with_input_example(example)
            .with_strict(true)
            .with_provider_options(provider_options);

        assert_eq!(
            serde_json::to_value(tool).expect("function tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "description": "Get the current weather.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string"
                        }
                    },
                    "required": ["city"]
                },
                "inputExamples": [
                    {
                        "input": {
                            "city": "Brisbane"
                        }
                    }
                ],
                "strict": true,
                "providerOptions": {
                    "openai": {
                        "strictJsonSchema": true
                    }
                }
            })
        );
    }

    #[test]
    fn function_tool_deserializes_and_omits_missing_optional_fields() {
        let tool: LanguageModelFunctionTool = serde_json::from_value(json!({
            "type": "function",
            "name": "lookup",
            "inputSchema": {
                "type": "object"
            }
        }))
        .expect("function tool deserializes");

        let input_schema =
            serde_json::from_value(json!({ "type": "object" })).expect("schema is object");

        assert_eq!(tool, LanguageModelFunctionTool::new("lookup", input_schema));
        assert_eq!(
            serde_json::to_value(tool).expect("function tool serializes"),
            json!({
                "type": "function",
                "name": "lookup",
                "inputSchema": {
                    "type": "object"
                }
            })
        );
    }

    #[test]
    fn provider_tool_deserializes_record_args() {
        let tool: LanguageModelProviderTool = serde_json::from_value(json!({
            "type": "provider",
            "id": "openai.web_search",
            "name": "web_search",
            "args": {
                "searchContextSize": "low"
            }
        }))
        .expect("provider tool deserializes");

        let args = serde_json::from_value(json!({
            "searchContextSize": "low"
        }))
        .expect("provider tool args are a JSON object");

        assert_eq!(
            tool,
            LanguageModelProviderTool::new("openai.web_search", "web_search", args)
        );
        assert_eq!(
            serde_json::to_value(tool).expect("provider tool serializes"),
            json!({
                "type": "provider",
                "id": "openai.web_search",
                "name": "web_search",
                "args": {
                    "searchContextSize": "low"
                }
            })
        );
    }

    #[test]
    fn tool_union_deserializes_provider_tool_variant() {
        let tool: LanguageModelTool = serde_json::from_value(json!({
            "type": "provider",
            "id": "openai.web_search",
            "name": "web_search",
            "args": {}
        }))
        .expect("tool union deserializes");

        assert_eq!(
            tool,
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "openai.web_search",
                "web_search",
                serde_json::from_value(json!({})).expect("args are a JSON object"),
            ))
        );
    }

    #[test]
    fn tool_union_rejects_unknown_tool_types() {
        serde_json::from_value::<LanguageModelTool>(json!({
            "type": "unsupported",
            "name": "unknown"
        }))
        .expect_err("unsupported tool variant is rejected");
    }

    #[test]
    fn tool_choice_serializes_upstream_tagged_shapes() {
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::Auto).expect("tool choice serializes"),
            json!({ "type": "auto" })
        );
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::None).expect("tool choice serializes"),
            json!({ "type": "none" })
        );
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::Required)
                .expect("tool choice serializes"),
            json!({ "type": "required" })
        );
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::Tool {
                tool_name: "search".to_string(),
            })
            .expect("tool choice serializes"),
            json!({
                "type": "tool",
                "toolName": "search"
            })
        );
    }

    #[test]
    fn tool_choice_deserializes_specific_tool_selection() {
        let tool_choice: LanguageModelToolChoice = serde_json::from_value(json!({
            "type": "tool",
            "toolName": "weather"
        }))
        .expect("tool choice deserializes");

        assert_eq!(
            tool_choice,
            LanguageModelToolChoice::Tool {
                tool_name: "weather".to_string()
            }
        );
    }

    #[test]
    fn call_options_serializes_upstream_shape_with_generation_controls() {
        let input_schema =
            serde_json::from_value(json!({ "type": "object" })).expect("schema is object");
        let response_schema = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string"
                }
            }
        }))
        .expect("response schema is object");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "anthropic": {
                "cacheControl": {
                    "type": "ephemeral"
                }
            }
        }))
        .expect("provider options deserialize");

        let options = LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
            LanguageModelSystemMessage::new("Return compact JSON."),
        )])
        .with_max_output_tokens(256)
        .with_temperature(0.2)
        .with_stop_sequence("</json>")
        .with_top_p(0.95)
        .with_top_k(40)
        .with_presence_penalty(0.1)
        .with_frequency_penalty(0.2)
        .with_response_format(
            LanguageModelResponseFormat::json()
                .with_schema(response_schema)
                .with_name("summary")
                .with_description("A short summary object."),
        )
        .with_seed(1234)
        .with_tool(LanguageModelTool::Function(LanguageModelFunctionTool::new(
            "weather",
            input_schema,
        )))
        .with_tool_choice(LanguageModelToolChoice::Tool {
            tool_name: "weather".to_string(),
        })
        .with_include_raw_chunks(true)
        .with_header("x-request-id", "req_123")
        .with_reasoning(LanguageModelReasoningEffort::High)
        .with_provider_options(provider_options);

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "prompt": [
                    {
                        "role": "system",
                        "content": "Return compact JSON."
                    }
                ],
                "maxOutputTokens": 256,
                "temperature": 0.2,
                "stopSequences": ["</json>"],
                "topP": 0.95,
                "topK": 40,
                "presencePenalty": 0.1,
                "frequencyPenalty": 0.2,
                "responseFormat": {
                    "type": "json",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "summary": {
                                "type": "string"
                            }
                        }
                    },
                    "name": "summary",
                    "description": "A short summary object."
                },
                "seed": 1234,
                "tools": [
                    {
                        "type": "function",
                        "name": "weather",
                        "inputSchema": {
                            "type": "object"
                        }
                    }
                ],
                "toolChoice": {
                    "type": "tool",
                    "toolName": "weather"
                },
                "includeRawChunks": true,
                "headers": {
                    "x-request-id": "req_123"
                },
                "reasoning": "high",
                "providerOptions": {
                    "anthropic": {
                        "cacheControl": {
                            "type": "ephemeral"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn call_options_deserializes_minimal_prompt_and_omits_missing_options() {
        let options: LanguageModelCallOptions = serde_json::from_value(json!({
            "prompt": [
                {
                    "role": "system",
                    "content": "Be concise."
                }
            ]
        }))
        .expect("call options deserialize");

        assert_eq!(
            options,
            LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Be concise."),
            )])
        );
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "prompt": [
                    {
                        "role": "system",
                        "content": "Be concise."
                    }
                ]
            })
        );
    }

    #[test]
    fn call_options_carries_abort_signal_without_serializing_it() {
        let abort_controller = LanguageModelAbortController::new();
        let options = LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
            LanguageModelSystemMessage::new("Be concise."),
        )])
        .with_abort_signal(abort_controller.signal());

        assert!(
            options
                .abort_signal
                .as_ref()
                .is_some_and(|signal| !signal.is_aborted())
        );
        assert_eq!(
            serde_json::to_value(&options).expect("call options serialize"),
            json!({
                "prompt": [
                    {
                        "role": "system",
                        "content": "Be concise."
                    }
                ]
            })
        );

        let cloned_signal = options.abort_signal.clone().expect("abort signal set");
        abort_controller.abort_with_reason("manual abort");
        assert!(cloned_signal.is_aborted());
        assert_eq!(cloned_signal.reason(), Some(json!("manual abort")));
    }

    #[test]
    fn call_options_deserializes_text_format_and_provider_default_reasoning() {
        let options: LanguageModelCallOptions = serde_json::from_value(json!({
            "prompt": [
                {
                    "role": "system",
                    "content": "Answer plainly."
                }
            ],
            "responseFormat": {
                "type": "text"
            },
            "reasoning": "provider-default"
        }))
        .expect("call options deserialize");

        assert_eq!(
            options.response_format,
            Some(LanguageModelResponseFormat::Text)
        );
        assert_eq!(
            options.reasoning,
            Some(LanguageModelReasoningEffort::ProviderDefault)
        );
    }

    #[test]
    fn prompt_serializes_system_and_user_messages_with_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "anthropic": {
                "cacheControl": {
                    "type": "ephemeral"
                }
            }
        }))
        .expect("provider options deserialize");

        let prompt: LanguageModelPrompt = vec![
            LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Be concise.")
                    .with_provider_options(provider_options.clone()),
            ),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Summarize this document.")
                        .with_provider_options(provider_options.clone()),
                ),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Text {
                            text: "Quarterly results".to_string(),
                        },
                        "text/plain",
                    )
                    .with_filename("results.txt"),
                ),
            ])),
        ];

        assert_eq!(
            serde_json::to_value(prompt).expect("prompt serializes"),
            json!([
                {
                    "role": "system",
                    "content": "Be concise.",
                    "providerOptions": {
                        "anthropic": {
                            "cacheControl": {
                                "type": "ephemeral"
                            }
                        }
                    }
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Summarize this document.",
                            "providerOptions": {
                                "anthropic": {
                                    "cacheControl": {
                                        "type": "ephemeral"
                                    }
                                }
                            }
                        },
                        {
                            "type": "file",
                            "filename": "results.txt",
                            "data": {
                                "type": "text",
                                "text": "Quarterly results"
                            },
                            "mediaType": "text/plain"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn assistant_message_deserializes_reasoning_custom_tool_call_and_approval_request_parts() {
        let message: LanguageModelMessage = serde_json::from_value(json!({
            "role": "assistant",
            "content": [
                {
                    "type": "reasoning",
                    "text": "I should call the weather tool."
                },
                {
                    "type": "custom",
                    "kind": "openai.audio"
                },
                {
                    "type": "tool-call",
                    "toolCallId": "tool_call_123",
                    "toolName": "weather",
                    "input": {
                        "city": "Brisbane"
                    },
                    "providerExecuted": true
                },
                {
                    "type": "tool-approval-request",
                    "approvalId": "approval_123",
                    "toolCallId": "tool_call_123",
                    "isAutomatic": true
                }
            ]
        }))
        .expect("assistant message deserializes");

        assert_eq!(
            message,
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Reasoning(LanguageModelReasoningPart::new(
                    "I should call the weather tool.",
                )),
                LanguageModelAssistantContentPart::Custom(LanguageModelCustomPart::new(
                    "openai.audio",
                )),
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "tool_call_123",
                        "weather",
                        json!({
                            "city": "Brisbane"
                        }),
                    )
                    .with_provider_executed(true),
                ),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new("approval_123", "tool_call_123")
                        .with_automatic(true),
                ),
            ]))
        );
    }

    #[test]
    fn assistant_message_serializes_tool_approval_request_part() {
        let message = LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
            LanguageModelAssistantContentPart::ToolApprovalRequest(
                LanguageModelToolApprovalRequestPart::new("approval_123", "tool_call_456"),
            ),
        ]));

        assert_eq!(
            serde_json::to_value(message).expect("assistant message serializes"),
            json!({
                "role": "assistant",
                "content": [
                    {
                        "type": "tool-approval-request",
                        "approvalId": "approval_123",
                        "toolCallId": "tool_call_456"
                    }
                ]
            })
        );
    }

    #[test]
    fn tool_message_serializes_tool_result_and_approval_response_parts() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "format": "compact"
            }
        }))
        .expect("provider options deserialize");

        let message = LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
            LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                "tool_call_123",
                "weather",
                LanguageModelToolResultOutput::content(vec![
                    LanguageModelToolResultContentPart::Text(LanguageModelTextPart::new("Sunny")),
                    LanguageModelToolResultContentPart::Custom(
                        LanguageModelToolResultCustomContent::new()
                            .with_provider_options(provider_options),
                    ),
                ]),
            )),
            LanguageModelToolContentPart::ToolApprovalResponse(
                LanguageModelToolApprovalResponsePart::new("approval_123", false)
                    .with_reason("User declined external access."),
            ),
        ]));

        assert_eq!(
            serde_json::to_value(message).expect("tool message serializes"),
            json!({
                "role": "tool",
                "content": [
                    {
                        "type": "tool-result",
                        "toolCallId": "tool_call_123",
                        "toolName": "weather",
                        "output": {
                            "type": "content",
                            "value": [
                                {
                                    "type": "text",
                                    "text": "Sunny"
                                },
                                {
                                    "type": "custom",
                                    "providerOptions": {
                                        "openai": {
                                            "format": "compact"
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    {
                        "type": "tool-approval-response",
                        "approvalId": "approval_123",
                        "approved": false,
                        "reason": "User declined external access."
                    }
                ]
            })
        );
    }

    #[test]
    fn tool_result_output_serializes_tagged_output_variants_with_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "provider": {
                "traceId": "trace_123"
            }
        }))
        .expect("provider options deserialize");

        assert_eq!(
            serde_json::to_value(
                LanguageModelToolResultOutput::json(json!({
                    "ok": true
                }))
                .with_provider_options(provider_options.clone()),
            )
            .expect("json output serializes"),
            json!({
                "type": "json",
                "value": {
                    "ok": true
                },
                "providerOptions": {
                    "provider": {
                        "traceId": "trace_123"
                    }
                }
            })
        );

        assert_eq!(
            serde_json::to_value(
                LanguageModelToolResultOutput::execution_denied()
                    .with_reason("Not approved.")
                    .with_provider_options(provider_options),
            )
            .expect("execution denied output serializes"),
            json!({
                "type": "execution-denied",
                "reason": "Not approved.",
                "providerOptions": {
                    "provider": {
                        "traceId": "trace_123"
                    }
                }
            })
        );

        assert_eq!(
            serde_json::to_value(LanguageModelToolResultOutput::error_text("Timed out."))
                .expect("error text output serializes"),
            json!({
                "type": "error-text",
                "value": "Timed out."
            })
        );
    }

    #[test]
    fn prompt_message_rejects_unknown_roles() {
        serde_json::from_value::<LanguageModelMessage>(json!({
            "role": "developer",
            "content": "Unsupported role"
        }))
        .expect_err("unsupported prompt role is rejected");
    }
}
