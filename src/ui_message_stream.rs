use std::collections::BTreeMap;
use std::fmt;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::FinishReason;
use crate::provider::ProviderMetadata;
use crate::provider_utils::normalize_headers;
use crate::util::merge_objects;

/// Default content type used by upstream UI-message stream response helpers.
pub const UI_MESSAGE_STREAM_CONTENT_TYPE: &str = "text/event-stream";

/// Header that marks the upstream UI-message stream protocol version.
pub const UI_MESSAGE_STREAM_VERSION_HEADER: &str = "x-vercel-ai-ui-message-stream";

/// Current upstream UI-message stream protocol version.
pub const UI_MESSAGE_STREAM_VERSION: &str = "v1";

/// A subset of upstream UI-message stream chunks.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum UiMessageChunk {
    /// Start of a UI-message stream.
    Start {
        /// Optional message identifier to assign to the response.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,

        /// Optional metadata to merge into the UI message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_metadata: Option<JsonValue>,
    },

    /// Start of a model-call step inside the UI-message stream.
    StartStep,

    /// Start of a text part.
    TextStart {
        /// Text part identifier.
        id: String,

        /// Provider-specific metadata for the text part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Delta for a text part.
    TextDelta {
        /// Text part identifier.
        id: String,

        /// Text delta.
        delta: String,

        /// Provider-specific metadata for the text delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// End of a text part.
    TextEnd {
        /// Text part identifier.
        id: String,

        /// Provider-specific metadata for the text part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Start of a reasoning part.
    ReasoningStart {
        /// Reasoning part identifier.
        id: String,

        /// Provider-specific metadata for the reasoning part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Delta for a reasoning part.
    ReasoningDelta {
        /// Reasoning part identifier.
        id: String,

        /// Reasoning delta.
        delta: String,

        /// Provider-specific metadata for the reasoning delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// End of a reasoning part.
    ReasoningEnd {
        /// Reasoning part identifier.
        id: String,

        /// Provider-specific metadata for the reasoning part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Generated file part.
    File {
        /// The IANA media type of the generated file.
        media_type: String,

        /// URL or data URI for the generated file.
        url: String,

        /// Provider-specific metadata for the file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Generated reasoning file part.
    ReasoningFile {
        /// The IANA media type of the generated reasoning file.
        media_type: String,

        /// URL or data URI for the generated reasoning file.
        url: String,

        /// Provider-specific metadata for the reasoning file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// URL source part.
    SourceUrl {
        /// Source identifier.
        source_id: String,

        /// Source URL.
        url: String,

        /// Optional source title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,

        /// Provider-specific metadata for the source.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Document source part.
    SourceDocument {
        /// Source identifier.
        source_id: String,

        /// Source document media type.
        media_type: String,

        /// Source document title.
        title: String,

        /// Optional source document filename.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,

        /// Provider-specific metadata for the source.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Start of streamed input for a tool call.
    ToolInputStart {
        /// Tool call identifier.
        tool_call_id: String,

        /// Tool name.
        tool_name: String,

        /// Whether the provider executes the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,

        /// Provider-specific metadata for the tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,

        /// Whether the tool was dynamically defined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<bool>,

        /// Optional display title for the tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },

    /// Delta for streamed input to a tool call.
    ToolInputDelta {
        /// Tool call identifier.
        tool_call_id: String,

        /// Tool input delta.
        input_text_delta: String,
    },

    /// Parsed tool input is available.
    ToolInputAvailable {
        /// Tool call identifier.
        tool_call_id: String,

        /// Tool name.
        tool_name: String,

        /// Parsed tool input.
        input: JsonValue,

        /// Whether the provider executes the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,

        /// Provider-specific metadata for the tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,

        /// High-level metadata from the matched tool definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_metadata: Option<JsonObject>,

        /// Whether the tool was dynamically defined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<bool>,

        /// Optional display title for the tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },

    /// Tool input could not be parsed or validated.
    ToolInputError {
        /// Tool call identifier.
        tool_call_id: String,

        /// Tool name.
        tool_name: String,

        /// Raw or partially parsed tool input.
        input: JsonValue,

        /// Human-readable error text.
        error_text: String,

        /// Whether the provider executes the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,

        /// Provider-specific metadata for the tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,

        /// High-level metadata from the matched tool definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_metadata: Option<JsonObject>,

        /// Whether the tool was dynamically defined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<bool>,

        /// Optional display title for the tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },

    /// Tool output is available.
    ToolOutputAvailable {
        /// Tool call identifier.
        tool_call_id: String,

        /// Tool output.
        output: JsonValue,

        /// Whether the provider executed the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,

        /// Provider-specific metadata for the tool result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,

        /// High-level metadata from the matched tool definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_metadata: Option<JsonObject>,

        /// Whether the tool output is preliminary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preliminary: Option<bool>,

        /// Whether the tool was dynamically defined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<bool>,
    },

    /// Tool execution failed.
    ToolOutputError {
        /// Tool call identifier.
        tool_call_id: String,

        /// Human-readable error text.
        error_text: String,

        /// Whether the provider executed the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,

        /// Provider-specific metadata for the tool result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,

        /// High-level metadata from the matched tool definition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_metadata: Option<JsonObject>,

        /// Whether the tool was dynamically defined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<bool>,
    },

    /// Provider-executed tool approval is requested.
    ToolApprovalRequest {
        /// Approval request identifier.
        approval_id: String,

        /// Tool call identifier.
        tool_call_id: String,

        /// Provider-specific metadata for the approval request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Tool output was denied.
    ToolOutputDenied {
        /// Tool call identifier.
        tool_call_id: String,

        /// Tool name.
        tool_name: String,

        /// Whether the provider would execute the tool.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,

        /// Whether the tool was dynamically defined.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dynamic: Option<bool>,
    },

    /// Provider-specific custom chunk.
    Custom {
        /// Provider-specific custom kind.
        kind: String,

        /// Provider-specific metadata for the custom chunk.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Error chunk sent to UI-message stream consumers.
    Error {
        /// Error text visible to the client.
        error_text: String,
    },

    /// End of a model-call step inside the UI-message stream.
    FinishStep,

    /// End of a UI-message stream.
    Finish {
        /// Optional finish reason reported by the model.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<FinishReason>,

        /// Optional metadata to merge into the UI message.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_metadata: Option<JsonValue>,
    },

    /// Metadata update for the streamed UI message.
    MessageMetadata {
        /// Metadata to merge into the UI message.
        message_metadata: JsonValue,
    },
}

/// Role of an upstream UI message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UiMessageRole {
    /// System message.
    System,

    /// User message.
    User,

    /// Assistant response message.
    Assistant,
}

/// Minimal portable shape of an upstream UI message.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessage {
    /// Message identifier.
    pub id: String,

    /// Message role.
    pub role: UiMessageRole,

    /// Optional message metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,

    /// UI message parts.
    #[serde(default)]
    pub parts: Vec<JsonValue>,
}

impl UiMessage {
    /// Creates a UI message with no metadata or parts.
    pub fn new(id: impl Into<String>, role: UiMessageRole) -> Self {
        Self {
            id: id.into(),
            role,
            metadata: None,
            parts: Vec::new(),
        }
    }

    /// Adds metadata to this UI message.
    pub fn with_metadata(mut self, metadata: impl Into<JsonValue>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Adds one UI message part.
    pub fn with_part(mut self, part: impl Into<JsonValue>) -> Self {
        self.parts.push(part.into());
        self
    }
}

/// Response message id source accepted by [`get_response_ui_message_id`].
pub enum ResponseUiMessageId {
    /// Use this fixed response id.
    Id(String),

    /// Generate a new response id when the response is not a continuation.
    Generate(Box<dyn FnOnce() -> String>),
}

impl ResponseUiMessageId {
    /// Creates a fixed response message id.
    pub fn id(id: impl Into<String>) -> Self {
        Self::Id(id.into())
    }

    /// Creates a response message id generator.
    pub fn generate<F>(generate: F) -> Self
    where
        F: FnOnce() -> String + 'static,
    {
        Self::Generate(Box::new(generate))
    }
}

/// Determines the response UI message id for persistence-aware streams.
///
/// This mirrors upstream `getResponseUIMessageId`: if there are no original
/// messages, id generation is left to the client and `None` is returned. If the
/// last original message is an assistant message, its id is reused for
/// continuation. Otherwise the supplied id or id generator is used.
pub fn get_response_ui_message_id(
    original_messages: Option<&[UiMessage]>,
    response_message_id: ResponseUiMessageId,
) -> Option<String> {
    let original_messages = original_messages?;

    if let Some(last_message) = original_messages.last() {
        if last_message.role == UiMessageRole::Assistant {
            return Some(last_message.id.clone());
        }
    }

    match response_message_id {
        ResponseUiMessageId::Id(id) => Some(id),
        ResponseUiMessageId::Generate(generate) => Some(generate()),
    }
}

/// Checks whether a UI message part is a static tool part.
///
/// This mirrors upstream `isStaticToolUIPart`: static tool parts use a
/// `tool-<name>` part type.
pub fn is_static_tool_ui_part(part: &JsonValue) -> bool {
    ui_message_part_type(part).is_some_and(|part_type| part_type.starts_with("tool-"))
}

/// Checks whether a UI message part is a dynamic tool part.
pub fn is_dynamic_tool_ui_part(part: &JsonValue) -> bool {
    ui_message_part_type(part) == Some("dynamic-tool")
}

/// Checks whether a UI message part is a static or dynamic tool part.
pub fn is_tool_ui_part(part: &JsonValue) -> bool {
    is_static_tool_ui_part(part) || is_dynamic_tool_ui_part(part)
}

/// Checks whether the last assistant message has complete non-provider tool calls.
///
/// Mirrors upstream `lastAssistantMessageIsCompleteWithToolCalls`: only the
/// last step is considered, provider-executed tools are ignored, at least one
/// non-provider tool must be present, and every considered tool must have an
/// `output-available` or `output-error` state.
pub fn last_assistant_message_is_complete_with_tool_calls(messages: &[UiMessage]) -> bool {
    let Some(message) = messages.last() else {
        return false;
    };

    if message.role != UiMessageRole::Assistant {
        return false;
    }

    let last_step_start = last_step_part_index(&message.parts).map_or(0, |index| index + 1);
    let mut has_tool = false;
    for part in message.parts[last_step_start..]
        .iter()
        .filter(|part| is_tool_ui_part(part))
        .filter(|part| !ui_message_part_provider_executed(part))
    {
        has_tool = true;
        if !ui_message_part_state(part).is_some_and(is_terminal_tool_output_state) {
            return false;
        }
    }

    has_tool
}

/// Checks whether the last assistant message has complete tool approval responses.
///
/// Mirrors upstream `lastAssistantMessageIsCompleteWithApprovalResponses`: only
/// the last step is considered, at least one tool must have
/// `approval-responded`, and every tool in the step must be terminal.
pub fn last_assistant_message_is_complete_with_approval_responses(messages: &[UiMessage]) -> bool {
    let Some(message) = messages.last() else {
        return false;
    };

    if message.role != UiMessageRole::Assistant {
        return false;
    }

    let last_step_start = last_step_part_index(&message.parts).map_or(0, |index| index + 1);
    let mut has_approval_response = false;
    for part in message.parts[last_step_start..]
        .iter()
        .filter(|part| is_tool_ui_part(part))
    {
        let state = ui_message_part_state(part);
        has_approval_response |= state == Some("approval-responded");
        if !state.is_some_and(is_terminal_approval_state) {
            return false;
        }
    }

    has_approval_response
}

impl UiMessageChunk {
    /// Creates a stream-start UI-message chunk.
    pub fn start() -> Self {
        Self::Start {
            message_id: None,
            message_metadata: None,
        }
    }

    /// Creates a stream-start UI-message chunk with a message id.
    pub fn start_with_message_id(message_id: impl Into<String>) -> Self {
        Self::Start {
            message_id: Some(message_id.into()),
            message_metadata: None,
        }
    }

    /// Adds message metadata to a stream-start or stream-finish chunk.
    pub fn with_message_metadata(mut self, message_metadata: impl Into<JsonValue>) -> Self {
        let message_metadata = Some(message_metadata.into());

        match &mut self {
            Self::Start {
                message_metadata: target,
                ..
            }
            | Self::Finish {
                message_metadata: target,
                ..
            } => {
                *target = message_metadata;
            }
            _ => {}
        }

        self
    }

    /// Creates a step-start UI-message chunk.
    pub fn start_step() -> Self {
        Self::StartStep
    }

    /// Creates a text-start UI-message chunk.
    pub fn text_start(id: impl Into<String>) -> Self {
        Self::TextStart {
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Creates a text-delta UI-message chunk.
    pub fn text_delta(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::TextDelta {
            id: id.into(),
            delta: delta.into(),
            provider_metadata: None,
        }
    }

    /// Creates a text-end UI-message chunk.
    pub fn text_end(id: impl Into<String>) -> Self {
        Self::TextEnd {
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Creates a reasoning-start UI-message chunk.
    pub fn reasoning_start(id: impl Into<String>) -> Self {
        Self::ReasoningStart {
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Creates a reasoning-delta UI-message chunk.
    pub fn reasoning_delta(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::ReasoningDelta {
            id: id.into(),
            delta: delta.into(),
            provider_metadata: None,
        }
    }

    /// Creates a reasoning-end UI-message chunk.
    pub fn reasoning_end(id: impl Into<String>) -> Self {
        Self::ReasoningEnd {
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Creates a file UI-message chunk.
    pub fn file(media_type: impl Into<String>, url: impl Into<String>) -> Self {
        Self::File {
            media_type: media_type.into(),
            url: url.into(),
            provider_metadata: None,
        }
    }

    /// Creates a reasoning-file UI-message chunk.
    pub fn reasoning_file(media_type: impl Into<String>, url: impl Into<String>) -> Self {
        Self::ReasoningFile {
            media_type: media_type.into(),
            url: url.into(),
            provider_metadata: None,
        }
    }

    /// Creates a URL source UI-message chunk.
    pub fn source_url(source_id: impl Into<String>, url: impl Into<String>) -> Self {
        Self::SourceUrl {
            source_id: source_id.into(),
            url: url.into(),
            title: None,
            provider_metadata: None,
        }
    }

    /// Creates a document source UI-message chunk.
    pub fn source_document(
        source_id: impl Into<String>,
        media_type: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self::SourceDocument {
            source_id: source_id.into(),
            media_type: media_type.into(),
            title: title.into(),
            filename: None,
            provider_metadata: None,
        }
    }

    /// Creates a tool-input-start UI-message chunk.
    pub fn tool_input_start(tool_call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self::ToolInputStart {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            provider_executed: None,
            provider_metadata: None,
            dynamic: None,
            title: None,
        }
    }

    /// Creates a tool-input-delta UI-message chunk.
    pub fn tool_input_delta(
        tool_call_id: impl Into<String>,
        input_text_delta: impl Into<String>,
    ) -> Self {
        Self::ToolInputDelta {
            tool_call_id: tool_call_id.into(),
            input_text_delta: input_text_delta.into(),
        }
    }

    /// Creates a tool-input-available UI-message chunk.
    pub fn tool_input_available(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: impl Into<JsonValue>,
    ) -> Self {
        Self::ToolInputAvailable {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input: input.into(),
            provider_executed: None,
            provider_metadata: None,
            tool_metadata: None,
            dynamic: None,
            title: None,
        }
    }

    /// Creates a tool-input-error UI-message chunk.
    pub fn tool_input_error(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: impl Into<JsonValue>,
        error_text: impl Into<String>,
    ) -> Self {
        Self::ToolInputError {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input: input.into(),
            error_text: error_text.into(),
            provider_executed: None,
            provider_metadata: None,
            tool_metadata: None,
            dynamic: None,
            title: None,
        }
    }

    /// Creates a tool-output-available UI-message chunk.
    pub fn tool_output_available(
        tool_call_id: impl Into<String>,
        output: impl Into<JsonValue>,
    ) -> Self {
        Self::ToolOutputAvailable {
            tool_call_id: tool_call_id.into(),
            output: output.into(),
            provider_executed: None,
            provider_metadata: None,
            tool_metadata: None,
            preliminary: None,
            dynamic: None,
        }
    }

    /// Creates a tool-output-error UI-message chunk.
    pub fn tool_output_error(
        tool_call_id: impl Into<String>,
        error_text: impl Into<String>,
    ) -> Self {
        Self::ToolOutputError {
            tool_call_id: tool_call_id.into(),
            error_text: error_text.into(),
            provider_executed: None,
            provider_metadata: None,
            tool_metadata: None,
            dynamic: None,
        }
    }

    /// Creates a tool-approval-request UI-message chunk.
    pub fn tool_approval_request(
        approval_id: impl Into<String>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        Self::ToolApprovalRequest {
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            provider_metadata: None,
        }
    }

    /// Creates a tool-output-denied UI-message chunk.
    pub fn tool_output_denied(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
    ) -> Self {
        Self::ToolOutputDenied {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            provider_executed: None,
            dynamic: None,
        }
    }

    /// Creates a custom UI-message chunk.
    pub fn custom(kind: impl Into<String>) -> Self {
        Self::Custom {
            kind: kind.into(),
            provider_metadata: None,
        }
    }

    /// Creates an error UI-message chunk.
    pub fn error(error_text: impl Into<String>) -> Self {
        Self::Error {
            error_text: error_text.into(),
        }
    }

    /// Creates a step-finish UI-message chunk.
    pub fn finish_step() -> Self {
        Self::FinishStep
    }

    /// Creates a stream-finish UI-message chunk.
    pub fn finish() -> Self {
        Self::Finish {
            finish_reason: None,
            message_metadata: None,
        }
    }

    /// Creates a stream-finish UI-message chunk with a finish reason.
    pub fn finish_with_reason(finish_reason: FinishReason) -> Self {
        Self::Finish {
            finish_reason: Some(finish_reason),
            message_metadata: None,
        }
    }

    /// Creates a message-metadata UI-message chunk.
    pub fn message_metadata(message_metadata: impl Into<JsonValue>) -> Self {
        Self::MessageMetadata {
            message_metadata: message_metadata.into(),
        }
    }
}

/// Error returned when a UI-message stream cannot be applied to message state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiMessageStreamProcessError {
    chunk_type: String,
    chunk_id: String,
    message: String,
}

impl UiMessageStreamProcessError {
    /// Creates a UI-message stream error.
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

    /// Returns the stream chunk type associated with this error.
    pub fn chunk_type(&self) -> &str {
        &self.chunk_type
    }

    /// Returns the stream chunk id associated with this error.
    pub fn chunk_id(&self) -> &str {
        &self.chunk_id
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for UiMessageStreamProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for UiMessageStreamProcessError {}

/// Mutable UI-message state used while applying UI-message stream chunks.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamingUiMessageState {
    /// Current accumulated UI message.
    pub message: UiMessage,

    /// Last finish reason observed for the stream.
    pub finish_reason: Option<FinishReason>,

    active_text_parts: BTreeMap<String, usize>,
    active_reasoning_parts: BTreeMap<String, usize>,
}

impl StreamingUiMessageState {
    /// Creates streaming UI-message state from an optional previous message.
    pub fn new(message_id: impl Into<String>, last_message: Option<UiMessage>) -> Self {
        let message = match last_message {
            Some(message) if message.role == UiMessageRole::Assistant => message,
            _ => UiMessage::new(message_id, UiMessageRole::Assistant),
        };

        Self {
            message,
            finish_reason: None,
            active_text_parts: BTreeMap::new(),
            active_reasoning_parts: BTreeMap::new(),
        }
    }
}

/// Options for [`read_ui_message_stream`].
#[derive(Clone, Debug, PartialEq)]
pub struct ReadUiMessageStreamOptions {
    /// Previous assistant message to resume from.
    pub message: Option<UiMessage>,

    /// UI-message stream chunks to apply.
    pub stream: Vec<UiMessageChunk>,

    /// Whether an `error` chunk terminates processing.
    pub terminate_on_error: bool,
}

impl ReadUiMessageStreamOptions {
    /// Creates read options for a UI-message stream.
    pub fn new<I>(stream: I) -> Self
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        Self {
            message: None,
            stream: stream.into_iter().collect(),
            terminate_on_error: false,
        }
    }

    /// Sets the previous assistant message to resume from.
    pub fn with_message(mut self, message: UiMessage) -> Self {
        self.message = Some(message);
        self
    }

    /// Sets whether an `error` chunk terminates processing.
    pub fn with_terminate_on_error(mut self, terminate_on_error: bool) -> Self {
        self.terminate_on_error = terminate_on_error;
        self
    }
}

/// Transforms plain text stream chunks into the upstream UI-message chunk sequence.
pub fn transform_text_to_ui_message_stream<I, S>(stream: I) -> Vec<UiMessageChunk>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut chunks = vec![
        UiMessageChunk::start(),
        UiMessageChunk::start_step(),
        UiMessageChunk::text_start("text-1"),
    ];

    chunks.extend(
        stream
            .into_iter()
            .map(|part| UiMessageChunk::text_delta("text-1", part.into())),
    );

    chunks.push(UiMessageChunk::text_end("text-1"));
    chunks.push(UiMessageChunk::finish_step());
    chunks.push(UiMessageChunk::finish());

    chunks
}

/// Applies UI-message stream chunks and returns cloned message states after writes.
pub fn process_ui_message_stream<I>(
    state: &mut StreamingUiMessageState,
    stream: I,
    terminate_on_error: bool,
) -> Result<Vec<UiMessage>, UiMessageStreamProcessError>
where
    I: IntoIterator<Item = UiMessageChunk>,
{
    let mut updates = Vec::new();

    for chunk in stream {
        match chunk {
            UiMessageChunk::Start {
                message_id,
                message_metadata,
            } => {
                let should_write = message_id.is_some() || message_metadata.is_some();

                if let Some(message_id) = message_id {
                    state.message.id = message_id;
                }

                update_ui_message_metadata(&mut state.message, message_metadata);

                if should_write {
                    updates.push(state.message.clone());
                }
            }
            UiMessageChunk::StartStep => {
                state.message.parts.push(step_start_part());
            }
            UiMessageChunk::TextStart {
                id,
                provider_metadata,
            } => {
                let index = state.message.parts.len();
                state
                    .message
                    .parts
                    .push(streaming_text_part("", provider_metadata));
                state.active_text_parts.insert(id, index);
                updates.push(state.message.clone());
            }
            UiMessageChunk::TextDelta {
                id,
                delta,
                provider_metadata,
            } => {
                let index = *state.active_text_parts.get(&id).ok_or_else(|| {
                    UiMessageStreamProcessError::new(
                        "text-delta",
                        id.clone(),
                        format!(
                            "Received text-delta for missing text part with ID \"{id}\". \
Ensure a \"text-start\" chunk is sent before any \"text-delta\" chunks."
                        ),
                    )
                })?;

                append_ui_message_part_text(&mut state.message.parts[index], &delta);
                merge_ui_message_part_provider_metadata(
                    &mut state.message.parts[index],
                    provider_metadata,
                );
                updates.push(state.message.clone());
            }
            UiMessageChunk::TextEnd {
                id,
                provider_metadata,
            } => {
                let index = *state.active_text_parts.get(&id).ok_or_else(|| {
                    UiMessageStreamProcessError::new(
                        "text-end",
                        id.clone(),
                        format!(
                            "Received text-end for missing text part with ID \"{id}\". \
Ensure a \"text-start\" chunk is sent before any \"text-end\" chunks."
                        ),
                    )
                })?;

                set_ui_message_part_state(&mut state.message.parts[index], "done");
                merge_ui_message_part_provider_metadata(
                    &mut state.message.parts[index],
                    provider_metadata,
                );
                state.active_text_parts.remove(&id);
                updates.push(state.message.clone());
            }
            UiMessageChunk::ReasoningStart {
                id,
                provider_metadata,
            } => {
                let index = state.message.parts.len();
                state
                    .message
                    .parts
                    .push(streaming_reasoning_part("", provider_metadata));
                state.active_reasoning_parts.insert(id, index);
                updates.push(state.message.clone());
            }
            UiMessageChunk::ReasoningDelta {
                id,
                delta,
                provider_metadata,
            } => {
                let index = *state.active_reasoning_parts.get(&id).ok_or_else(|| {
                    UiMessageStreamProcessError::new(
                        "reasoning-delta",
                        id.clone(),
                        format!(
                            "Received reasoning-delta for missing reasoning part with ID \"{id}\". \
Ensure a \"reasoning-start\" chunk is sent before any \"reasoning-delta\" chunks."
                        ),
                    )
                })?;

                append_ui_message_part_text(&mut state.message.parts[index], &delta);
                merge_ui_message_part_provider_metadata(
                    &mut state.message.parts[index],
                    provider_metadata,
                );
                updates.push(state.message.clone());
            }
            UiMessageChunk::ReasoningEnd {
                id,
                provider_metadata,
            } => {
                let index = *state.active_reasoning_parts.get(&id).ok_or_else(|| {
                    UiMessageStreamProcessError::new(
                        "reasoning-end",
                        id.clone(),
                        format!(
                            "Received reasoning-end for missing reasoning part with ID \"{id}\". \
Ensure a \"reasoning-start\" chunk is sent before any \"reasoning-end\" chunks."
                        ),
                    )
                })?;

                set_ui_message_part_state(&mut state.message.parts[index], "done");
                merge_ui_message_part_provider_metadata(
                    &mut state.message.parts[index],
                    provider_metadata,
                );
                state.active_reasoning_parts.remove(&id);
                updates.push(state.message.clone());
            }
            chunk @ (UiMessageChunk::File { .. }
            | UiMessageChunk::ReasoningFile { .. }
            | UiMessageChunk::SourceUrl { .. }
            | UiMessageChunk::SourceDocument { .. }
            | UiMessageChunk::ToolInputStart { .. }
            | UiMessageChunk::ToolInputDelta { .. }
            | UiMessageChunk::ToolInputAvailable { .. }
            | UiMessageChunk::ToolInputError { .. }
            | UiMessageChunk::ToolOutputAvailable { .. }
            | UiMessageChunk::ToolOutputError { .. }
            | UiMessageChunk::ToolApprovalRequest { .. }
            | UiMessageChunk::ToolOutputDenied { .. }
            | UiMessageChunk::Custom { .. }) => {
                state.message.parts.push(
                    serde_json::to_value(&chunk)
                        .expect("ui-message stream chunk serializes to a JSON part"),
                );
                updates.push(state.message.clone());
            }
            UiMessageChunk::Error { error_text } => {
                if terminate_on_error {
                    return Err(UiMessageStreamProcessError::new("error", "", error_text));
                }
            }
            UiMessageChunk::FinishStep => {
                state.active_text_parts.clear();
                state.active_reasoning_parts.clear();
            }
            UiMessageChunk::Finish {
                finish_reason,
                message_metadata,
            } => {
                let should_write = message_metadata.is_some();
                state.finish_reason = finish_reason;
                update_ui_message_metadata(&mut state.message, message_metadata);

                if should_write {
                    updates.push(state.message.clone());
                }
            }
            UiMessageChunk::MessageMetadata { message_metadata } => {
                update_ui_message_metadata(&mut state.message, Some(message_metadata));
                updates.push(state.message.clone());
            }
        }
    }

    Ok(updates)
}

/// Reads UI-message stream chunks into cloned message states.
pub fn read_ui_message_stream(
    options: ReadUiMessageStreamOptions,
) -> Result<Vec<UiMessage>, UiMessageStreamProcessError> {
    let message_id = options
        .message
        .as_ref()
        .map(|message| message.id.clone())
        .unwrap_or_default();
    let mut state = StreamingUiMessageState::new(message_id, options.message);

    process_ui_message_stream(&mut state, options.stream, options.terminate_on_error)
}

fn update_ui_message_metadata(message: &mut UiMessage, message_metadata: Option<JsonValue>) {
    if let Some(message_metadata) = message_metadata {
        message.metadata = merge_objects(message.metadata.as_ref(), Some(&message_metadata));
    }
}

fn step_start_part() -> JsonValue {
    let mut object = JsonObject::new();
    object.insert(
        "type".to_string(),
        JsonValue::String("step-start".to_string()),
    );
    JsonValue::Object(object)
}

fn streaming_text_part(
    text: impl Into<String>,
    provider_metadata: Option<ProviderMetadata>,
) -> JsonValue {
    streaming_text_like_part("text", text, provider_metadata)
}

fn streaming_reasoning_part(
    text: impl Into<String>,
    provider_metadata: Option<ProviderMetadata>,
) -> JsonValue {
    streaming_text_like_part("reasoning", text, provider_metadata)
}

fn streaming_text_like_part(
    part_type: &str,
    text: impl Into<String>,
    provider_metadata: Option<ProviderMetadata>,
) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert("type".to_string(), JsonValue::String(part_type.to_string()));
    object.insert("text".to_string(), JsonValue::String(text.into()));
    object.insert(
        "state".to_string(),
        JsonValue::String("streaming".to_string()),
    );

    if let Some(provider_metadata) = provider_metadata {
        object.insert(
            "providerMetadata".to_string(),
            provider_metadata_to_json(provider_metadata),
        );
    }

    JsonValue::Object(object)
}

fn append_ui_message_part_text(part: &mut JsonValue, delta: &str) {
    if let Some(object) = ui_message_part_object_mut(part) {
        match object.get_mut("text") {
            Some(JsonValue::String(text)) => text.push_str(delta),
            _ => {
                object.insert("text".to_string(), JsonValue::String(delta.to_string()));
            }
        }
    }
}

fn set_ui_message_part_state(part: &mut JsonValue, state: &str) {
    if let Some(object) = ui_message_part_object_mut(part) {
        object.insert("state".to_string(), JsonValue::String(state.to_string()));
    }
}

fn merge_ui_message_part_provider_metadata(
    part: &mut JsonValue,
    provider_metadata: Option<ProviderMetadata>,
) {
    if let (Some(object), Some(provider_metadata)) =
        (ui_message_part_object_mut(part), provider_metadata)
    {
        object.insert(
            "providerMetadata".to_string(),
            provider_metadata_to_json(provider_metadata),
        );
    }
}

fn ui_message_part_object_mut(part: &mut JsonValue) -> Option<&mut JsonObject> {
    match part {
        JsonValue::Object(object) => Some(object),
        _ => None,
    }
}

fn provider_metadata_to_json(provider_metadata: ProviderMetadata) -> JsonValue {
    serde_json::to_value(provider_metadata).expect("provider metadata serializes")
}

fn ui_message_part_type(part: &JsonValue) -> Option<&str> {
    part.get("type").and_then(JsonValue::as_str)
}

fn ui_message_part_state(part: &JsonValue) -> Option<&str> {
    part.get("state").and_then(JsonValue::as_str)
}

fn ui_message_part_provider_executed(part: &JsonValue) -> bool {
    part.get("providerExecuted")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
}

fn last_step_part_index(parts: &[JsonValue]) -> Option<usize> {
    parts
        .iter()
        .rposition(|part| ui_message_part_type(part) == Some("step-start"))
}

fn is_terminal_tool_output_state(state: &str) -> bool {
    matches!(state, "output-available" | "output-error")
}

fn is_terminal_approval_state(state: &str) -> bool {
    matches!(
        state,
        "output-available" | "output-error" | "approval-responded"
    )
}

/// Response metadata supplied to UI-message stream response helpers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UiMessageStreamResponseInit {
    /// HTTP status code. Defaults to `200`.
    pub status: Option<u16>,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Optional response headers.
    pub headers: Option<Headers>,
}

impl UiMessageStreamResponseInit {
    /// Creates empty response initialization options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the response status code.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the response status text.
    pub fn with_status_text(mut self, status_text: impl Into<String>) -> Self {
        self.status_text = Some(status_text.into());
        self
    }

    /// Replaces response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Options shared by UI-message stream response helpers.
#[derive(Clone, Debug, PartialEq)]
pub struct UiMessageStreamResponseOptions {
    /// UI-message chunks to encode as server-sent events.
    pub stream: Vec<UiMessageChunk>,

    /// HTTP status code. Defaults to `200`.
    pub status: Option<u16>,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Optional response headers.
    pub headers: Option<Headers>,
}

impl UiMessageStreamResponseOptions {
    /// Creates options for a UI-message stream.
    pub fn new<I>(stream: I) -> Self
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        Self::from_init(stream, UiMessageStreamResponseInit::default())
    }

    /// Creates options for a UI-message stream and response initialization values.
    pub fn from_init<I>(stream: I, init: UiMessageStreamResponseInit) -> Self
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        Self {
            stream: stream.into_iter().collect(),
            status: init.status,
            status_text: init.status_text,
            headers: init.headers,
        }
    }

    /// Sets the response status code.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the response status text.
    pub fn with_status_text(mut self, status_text: impl Into<String>) -> Self {
        self.status_text = Some(status_text.into());
        self
    }

    /// Replaces response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Collected response returned by [`create_ui_message_stream_response`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiMessageStreamResponse {
    /// HTTP status code.
    pub status: u16,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Response headers.
    pub headers: Headers,

    /// UTF-8 encoded SSE chunks.
    pub body: Vec<Vec<u8>>,
}

impl UiMessageStreamResponse {
    /// Decodes the UTF-8 body chunks back into strings.
    pub fn decoded_body(&self) -> Result<Vec<String>, std::string::FromUtf8Error> {
        self.body.iter().cloned().map(String::from_utf8).collect()
    }
}

/// Creates a collected response from UI-message chunks.
///
/// This mirrors upstream `createUIMessageStreamResponse`: missing status
/// defaults to `200`, UI-message SSE headers are applied unless already
/// supplied, and each JSON chunk is encoded as an SSE `data:` event followed by
/// a `[DONE]` sentinel.
pub fn create_ui_message_stream_response(
    options: UiMessageStreamResponseOptions,
) -> UiMessageStreamResponse {
    let UiMessageStreamResponseOptions {
        stream,
        status,
        status_text,
        headers,
    } = options;

    UiMessageStreamResponse {
        status: status.unwrap_or(200),
        status_text,
        headers: prepare_ui_message_stream_headers(headers),
        body: encode_ui_message_sse_stream(stream),
    }
}

/// Minimal sink trait used by [`pipe_ui_message_stream_to_response`].
pub trait UiMessageStreamResponseWriter {
    /// Error type returned by the response writer.
    type Error;

    /// Writes response status and headers before body chunks.
    fn write_head(
        &mut self,
        status: u16,
        status_text: Option<&str>,
        headers: &Headers,
    ) -> Result<(), Self::Error>;

    /// Writes one UTF-8 encoded SSE chunk.
    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error>;

    /// Finalizes the response.
    fn end(&mut self) -> Result<(), Self::Error>;
}

/// Pipes UI-message chunks to a server-response writer.
///
/// This mirrors upstream `pipeUIMessageStreamToResponse` without binding this
/// crate to a concrete HTTP framework.
pub fn pipe_ui_message_stream_to_response<W>(
    response: &mut W,
    options: UiMessageStreamResponseOptions,
) -> Result<(), W::Error>
where
    W: UiMessageStreamResponseWriter,
{
    let UiMessageStreamResponseOptions {
        stream,
        status,
        status_text,
        headers,
    } = options;

    let status = status.unwrap_or(200);
    let headers = prepare_ui_message_stream_headers(headers);

    response.write_head(status, status_text.as_deref(), &headers)?;

    for chunk in encode_ui_message_sse_stream(stream) {
        response.write_chunk(&chunk)?;
    }

    response.end()
}

fn prepare_ui_message_stream_headers(headers: Option<Headers>) -> Headers {
    let mut headers = normalize_headers(headers.map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    }));

    headers
        .entry("content-type".to_string())
        .or_insert_with(|| UI_MESSAGE_STREAM_CONTENT_TYPE.to_string());
    headers
        .entry("cache-control".to_string())
        .or_insert_with(|| "no-cache".to_string());
    headers
        .entry("connection".to_string())
        .or_insert_with(|| "keep-alive".to_string());
    headers
        .entry(UI_MESSAGE_STREAM_VERSION_HEADER.to_string())
        .or_insert_with(|| UI_MESSAGE_STREAM_VERSION.to_string());
    headers
        .entry("x-accel-buffering".to_string())
        .or_insert_with(|| "no".to_string());

    headers
}

fn encode_ui_message_sse_stream(stream: Vec<UiMessageChunk>) -> Vec<Vec<u8>> {
    let mut chunks = stream
        .into_iter()
        .map(|chunk| {
            format!(
                "data: {}\n\n",
                serde_json::to_string(&chunk).expect("UI-message chunk serializes")
            )
            .into_bytes()
        })
        .collect::<Vec<_>>();

    chunks.push(b"data: [DONE]\n\n".to_vec());
    chunks
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::convert::Infallible;
    use std::rc::Rc;

    use super::*;
    use serde_json::json;

    #[test]
    fn create_ui_message_stream_response_sets_sse_headers_and_encoded_chunks() {
        let response = create_ui_message_stream_response(
            UiMessageStreamResponseOptions::new([UiMessageChunk::text_delta("1", "test-data")])
                .with_status(201)
                .with_status_text("Created")
                .with_header("Custom-Header", "test"),
        );

        assert_eq!(response.status, 201);
        assert_eq!(response.status_text.as_deref(), Some("Created"));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("cache-control").map(String::as_str),
            Some("no-cache")
        );
        assert_eq!(
            response.headers.get("connection").map(String::as_str),
            Some("keep-alive")
        );
        assert_eq!(
            response
                .headers
                .get(UI_MESSAGE_STREAM_VERSION_HEADER)
                .map(String::as_str),
            Some(UI_MESSAGE_STREAM_VERSION)
        );
        assert_eq!(
            response
                .headers
                .get("x-accel-buffering")
                .map(String::as_str),
            Some("no")
        );
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("test")
        );
        assert_eq!(
            response.decoded_body().expect("body chunks decode"),
            vec![
                r#"data: {"type":"text-delta","id":"1","delta":"test-data"}

"#
                .to_string(),
                "data: [DONE]\n\n".to_string()
            ]
        );
    }

    #[test]
    fn create_ui_message_stream_response_preserves_existing_headers_and_encodes_errors() {
        let response = create_ui_message_stream_response(
            UiMessageStreamResponseOptions::new([UiMessageChunk::error("Custom error message")])
                .with_header("Content-Type", "application/x-custom")
                .with_header(UI_MESSAGE_STREAM_VERSION_HEADER, "custom-version"),
        );

        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("application/x-custom")
        );
        assert_eq!(
            response
                .headers
                .get(UI_MESSAGE_STREAM_VERSION_HEADER)
                .map(String::as_str),
            Some("custom-version")
        );
        assert_eq!(
            response.decoded_body().expect("body chunks decode"),
            vec![
                r#"data: {"type":"error","errorText":"Custom error message"}

"#
                .to_string(),
                "data: [DONE]\n\n".to_string()
            ]
        );
    }

    #[test]
    fn ui_message_chunk_serializes_portable_tool_source_and_file_chunks() {
        let chunks = vec![
            UiMessageChunk::file("text/plain", "data:text/plain;base64,aGk="),
            UiMessageChunk::SourceUrl {
                source_id: "source-1".to_string(),
                url: "https://example.com".to_string(),
                title: Some("Example".to_string()),
                provider_metadata: None,
            },
            UiMessageChunk::tool_input_available(
                "call-1",
                "getWeather",
                json!({ "city": "Brisbane" }),
            ),
            UiMessageChunk::tool_output_available("call-1", json!({ "temperature": 22 })),
            UiMessageChunk::tool_approval_request("approval-1", "call-1"),
        ];

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                {
                    "type": "file",
                    "mediaType": "text/plain",
                    "url": "data:text/plain;base64,aGk="
                },
                {
                    "type": "source-url",
                    "sourceId": "source-1",
                    "url": "https://example.com",
                    "title": "Example"
                },
                {
                    "type": "tool-input-available",
                    "toolCallId": "call-1",
                    "toolName": "getWeather",
                    "input": { "city": "Brisbane" }
                },
                {
                    "type": "tool-output-available",
                    "toolCallId": "call-1",
                    "output": { "temperature": 22 }
                },
                {
                    "type": "tool-approval-request",
                    "approvalId": "approval-1",
                    "toolCallId": "call-1"
                }
            ])
        );
    }

    #[test]
    fn process_ui_message_stream_preserves_portable_non_text_chunks_as_parts() {
        let messages = read_ui_message_stream(ReadUiMessageStreamOptions::new([
            UiMessageChunk::start_with_message_id("msg-123"),
            UiMessageChunk::start_step(),
            UiMessageChunk::tool_input_available(
                "call-1",
                "getWeather",
                json!({ "city": "Brisbane" }),
            ),
            UiMessageChunk::tool_output_available("call-1", json!({ "temperature": 22 })),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]))
        .expect("ui stream reads");

        assert_eq!(
            messages.last().map(|message| message.parts.clone()),
            Some(vec![
                json!({ "type": "step-start" }),
                json!({
                    "type": "tool-input-available",
                    "toolCallId": "call-1",
                    "toolName": "getWeather",
                    "input": { "city": "Brisbane" }
                }),
                json!({
                    "type": "tool-output-available",
                    "toolCallId": "call-1",
                    "output": { "temperature": 22 }
                }),
            ])
        );
    }

    #[test]
    fn pipe_ui_message_stream_to_response_writes_headers_chunks_and_end() {
        let mut response = MockUiMessageStreamResponse::default();

        pipe_ui_message_stream_to_response(
            &mut response,
            UiMessageStreamResponseOptions::new([
                UiMessageChunk::text_start("1"),
                UiMessageChunk::text_delta("1", "test-data"),
                UiMessageChunk::text_end("1"),
            ])
            .with_status(202)
            .with_status_text("Accepted")
            .with_header("Custom-Header", "test"),
        )
        .expect("mock response writes");

        assert_eq!(response.status, Some(202));
        assert_eq!(response.status_text.as_deref(), Some("Accepted"));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("test")
        );
        assert_eq!(
            response.decoded_chunks(),
            vec![
                r#"data: {"type":"text-start","id":"1"}

"#,
                r#"data: {"type":"text-delta","id":"1","delta":"test-data"}

"#,
                r#"data: {"type":"text-end","id":"1"}

"#,
                "data: [DONE]\n\n"
            ]
        );
        assert!(response.ended);
    }

    #[test]
    fn transform_text_to_ui_message_stream_emits_upstream_sequence() {
        let chunks = transform_text_to_ui_message_stream(["Hello", " ", "World"]);

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "text-start", "id": "text-1" },
                { "type": "text-delta", "id": "text-1", "delta": "Hello" },
                { "type": "text-delta", "id": "text-1", "delta": " " },
                { "type": "text-delta", "id": "text-1", "delta": "World" },
                { "type": "text-end", "id": "text-1" },
                { "type": "finish-step" },
                { "type": "finish" }
            ])
        );
    }

    #[test]
    fn transform_text_to_ui_message_stream_handles_empty_streams() {
        let chunks = transform_text_to_ui_message_stream(Vec::<String>::new());

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "start" },
                { "type": "start-step" },
                { "type": "text-start", "id": "text-1" },
                { "type": "text-end", "id": "text-1" },
                { "type": "finish-step" },
                { "type": "finish" }
            ])
        );
    }

    #[test]
    fn read_ui_message_stream_returns_message_states_for_basic_text_stream() {
        let messages = read_ui_message_stream(ReadUiMessageStreamOptions::new([
            UiMessageChunk::start_with_message_id("msg-123"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello, "),
            UiMessageChunk::text_delta("text-1", "world!"),
            UiMessageChunk::text_end("text-1"),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]))
        .expect("stream reads");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "id": "msg-123",
                    "role": "assistant",
                    "parts": []
                },
                {
                    "id": "msg-123",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "text",
                            "text": "",
                            "state": "streaming"
                        }
                    ]
                },
                {
                    "id": "msg-123",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "text",
                            "text": "Hello, ",
                            "state": "streaming"
                        }
                    ]
                },
                {
                    "id": "msg-123",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "text",
                            "text": "Hello, world!",
                            "state": "streaming"
                        }
                    ]
                },
                {
                    "id": "msg-123",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "text",
                            "text": "Hello, world!",
                            "state": "done"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn process_ui_message_stream_accumulates_reasoning_parts() {
        let mut state = StreamingUiMessageState::new("msg-123", None);

        let messages = process_ui_message_stream(
            &mut state,
            [
                UiMessageChunk::reasoning_start("reasoning-1"),
                UiMessageChunk::reasoning_delta("reasoning-1", "Thinking"),
                UiMessageChunk::reasoning_delta("reasoning-1", "..."),
                UiMessageChunk::reasoning_end("reasoning-1"),
            ],
            false,
        )
        .expect("stream processes");

        assert_eq!(messages.len(), 4);
        assert_eq!(
            serde_json::to_value(state.message).expect("message serializes"),
            json!({
                "id": "msg-123",
                "role": "assistant",
                "parts": [
                    {
                        "type": "reasoning",
                        "text": "Thinking...",
                        "state": "done"
                    }
                ]
            })
        );
    }

    #[test]
    fn process_ui_message_stream_merges_message_metadata() {
        let mut state = StreamingUiMessageState::new("msg-123", None);

        let messages = process_ui_message_stream(
            &mut state,
            [
                UiMessageChunk::start().with_message_metadata(json!({
                    "trace": { "id": "trace-1", "attempt": 1 },
                    "keep": true
                })),
                UiMessageChunk::message_metadata(json!({
                    "trace": { "attempt": 2 },
                    "finish": true
                })),
            ],
            false,
        )
        .expect("stream processes");

        assert_eq!(messages.len(), 2);
        assert_eq!(
            state.message.metadata,
            Some(json!({
                "trace": { "id": "trace-1", "attempt": 2 },
                "keep": true,
                "finish": true
            }))
        );
    }

    #[test]
    fn process_ui_message_stream_reports_missing_text_delta() {
        let mut state = StreamingUiMessageState::new("msg-123", None);

        let error = process_ui_message_stream(
            &mut state,
            [UiMessageChunk::text_delta("missing-id", "Hello")],
            false,
        )
        .expect_err("missing text start fails");

        assert_eq!(error.chunk_type(), "text-delta");
        assert_eq!(error.chunk_id(), "missing-id");
        assert_eq!(
            error.message(),
            "Received text-delta for missing text part with ID \"missing-id\". \
Ensure a \"text-start\" chunk is sent before any \"text-delta\" chunks."
        );
    }

    #[test]
    fn read_ui_message_stream_terminates_on_error_chunk_when_requested() {
        let error = read_ui_message_stream(
            ReadUiMessageStreamOptions::new([
                UiMessageChunk::start_with_message_id("msg-123"),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "Hello"),
                UiMessageChunk::error("Test error message"),
            ])
            .with_terminate_on_error(true),
        )
        .expect_err("error chunk terminates");

        assert_eq!(error.message(), "Test error message");
        assert_eq!(error.chunk_type(), "error");
    }

    #[test]
    fn get_response_ui_message_id_returns_none_without_original_messages() {
        let called = Rc::new(Cell::new(false));
        let called_in_generator = Rc::clone(&called);

        let result = get_response_ui_message_id(
            None,
            ResponseUiMessageId::generate(move || {
                called_in_generator.set(true);
                "new-id".to_string()
            }),
        );

        assert_eq!(result, None);
        assert!(!called.get());
    }

    #[test]
    fn get_response_ui_message_id_reuses_last_assistant_message_id() {
        let called = Rc::new(Cell::new(false));
        let called_in_generator = Rc::clone(&called);
        let original_messages = vec![
            UiMessage::new("user-id", UiMessageRole::User),
            UiMessage::new("assistant-id", UiMessageRole::Assistant),
        ];

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::generate(move || {
                called_in_generator.set(true);
                "new-id".to_string()
            }),
        );

        assert_eq!(result.as_deref(), Some("assistant-id"));
        assert!(!called.get());
    }

    #[test]
    fn get_response_ui_message_id_generates_when_last_message_is_not_assistant() {
        let original_messages = vec![
            UiMessage::new("assistant-id", UiMessageRole::Assistant),
            UiMessage::new("user-id", UiMessageRole::User),
        ];

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::generate(|| "new-id".to_string()),
        );

        assert_eq!(result.as_deref(), Some("new-id"));
    }

    #[test]
    fn get_response_ui_message_id_uses_generator_for_empty_messages() {
        let original_messages = Vec::new();

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::generate(|| "new-id".to_string()),
        );

        assert_eq!(result.as_deref(), Some("new-id"));
    }

    #[test]
    fn get_response_ui_message_id_uses_fixed_response_id() {
        let original_messages = vec![UiMessage::new("user-id", UiMessageRole::User)];

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::id("fixed-id"),
        );

        assert_eq!(result.as_deref(), Some("fixed-id"));
    }

    #[test]
    fn ui_message_serializes_upstream_minimal_shape() {
        let message = UiMessage::new("message-id", UiMessageRole::Assistant)
            .with_metadata(json!({ "traceId": "trace-1" }))
            .with_part(json!({ "type": "text", "text": "hello" }));

        assert_eq!(
            serde_json::to_value(message).expect("message serializes"),
            json!({
                "id": "message-id",
                "role": "assistant",
                "metadata": { "traceId": "trace-1" },
                "parts": [{ "type": "text", "text": "hello" }]
            })
        );
    }

    #[test]
    fn tool_ui_part_predicates_match_upstream_runtime_shape() {
        assert!(is_static_tool_ui_part(&json!({
            "type": "tool-getWeather",
            "state": "input-available"
        })));
        assert!(is_dynamic_tool_ui_part(&json!({
            "type": "dynamic-tool",
            "state": "input-available"
        })));
        assert!(is_tool_ui_part(&json!({
            "type": "tool-getWeather",
            "state": "input-available"
        })));
        assert!(is_tool_ui_part(&json!({
            "type": "dynamic-tool",
            "state": "input-available"
        })));
        assert!(!is_tool_ui_part(&json!({
            "type": "text",
            "state": "done"
        })));
    }

    #[test]
    fn last_assistant_message_is_complete_with_tool_calls_matches_upstream_cases() {
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            UiMessage::new("user-id", UiMessageRole::User)
        ]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getLocation",
                    "call-location",
                    "output-available",
                    json!({}),
                    Some(json!("New York")),
                ),
                step_start_json(),
                json!({ "type": "text", "text": "The current weather is windy.", "state": "done" }),
            ])
        ]));
        assert!(last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getWeatherInformation",
                    "call-weather",
                    "output-available",
                    json!({ "city": "New York" }),
                    Some(json!("windy")),
                ),
                json!({ "type": "text", "text": "The current weather is windy.", "state": "done" }),
            ])
        ]));
        assert!(last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getWeatherInformation",
                    "call-weather",
                    "output-error",
                    json!({ "city": "New York" }),
                    None,
                ),
            ])
        ]));
        assert!(last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "output-available",
                    json!({ "location": "San Francisco" }),
                    Some(json!("sunny")),
                ),
            ])
        ]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "input-streaming",
                    json!({ "location": "San Francisco" }),
                    None,
                ),
            ])
        ]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "input-available",
                    json!({ "location": "San Francisco" }),
                    None,
                ),
            ])
        ]));
        assert!(last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getWeatherInformation",
                    "call-regular",
                    "output-available",
                    json!({ "city": "New York" }),
                    Some(json!("windy")),
                ),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "output-available",
                    json!({ "location": "San Francisco" }),
                    Some(json!("sunny")),
                ),
            ])
        ]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getWeatherInformation",
                    "call-regular",
                    "output-available",
                    json!({ "city": "New York" }),
                    Some(json!("windy")),
                ),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "input-available",
                    json!({ "location": "San Francisco" }),
                    None,
                ),
            ])
        ]));
        assert!(last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getLocation",
                    "call-location",
                    "output-available",
                    json!({}),
                    Some(json!("New York")),
                ),
                step_start_json(),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "output-available",
                    json!({ "location": "New York" }),
                    Some(json!("cloudy")),
                ),
                json!({ "type": "text", "text": "The current weather is cloudy.", "state": "done" }),
            ])
        ]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                tool_part_json(
                    "tool-getLocation",
                    "call-location",
                    "output-available",
                    json!({}),
                    Some(json!("New York")),
                ),
                step_start_json(),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "input-streaming",
                    json!({ "location": "New York" }),
                    None,
                ),
            ])
        ]));
        assert!(!last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                {
                    let mut part = tool_part_json(
                        "tool-web_search",
                        "srvtoolu-1",
                        "output-available",
                        json!({ "query": "New York weather" }),
                        Some(json!([])),
                    );
                    part.as_object_mut()
                        .expect("tool part is object")
                        .insert("providerExecuted".to_string(), json!(true));
                    part
                },
                json!({ "type": "text", "text": "The current weather is windy.", "state": "done" }),
            ])
        ]));
    }

    #[test]
    fn last_assistant_message_is_complete_with_approval_responses_matches_upstream_cases() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[]
        ));
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[UiMessage::new("user-id", UiMessageRole::User,)]
        ));
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                json!({ "type": "text", "text": "Hello", "state": "done" })
            ])]
        ));
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-requested", false),
            ])]
        ));
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-responded", false),
                approval_tool_part_json("tool-getWeather", "call-2", "approval-requested", false),
            ])]
        ));
        assert!(last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-responded", false),
            ])]
        ));
        assert!(last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("dynamic-tool", "call-1", "approval-responded", true),
            ])]
        ));
        assert!(last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-responded", false),
                tool_part_json(
                    "tool-getWeather",
                    "call-2",
                    "output-available",
                    json!({ "city": "Paris" }),
                    Some(json!({ "temperature": 20, "weather": "cloudy" })),
                ),
            ])]
        ));
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("dynamic-tool", "call-1", "approval-responded", true),
                approval_tool_part_json("tool-getWeather", "call-2", "approval-requested", false),
            ])]
        ));
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-responded", false),
                step_start_json(),
                json!({ "type": "text", "text": "Done.", "state": "done" }),
            ])]
        ));
    }

    fn assistant_message(parts: Vec<JsonValue>) -> UiMessage {
        let mut message = UiMessage::new("message-id", UiMessageRole::Assistant);
        message.parts = parts;
        message
    }

    fn step_start_json() -> JsonValue {
        json!({ "type": "step-start" })
    }

    fn tool_part_json(
        part_type: &str,
        tool_call_id: &str,
        state: &str,
        input: JsonValue,
        output: Option<JsonValue>,
    ) -> JsonValue {
        let mut part = json!({
            "type": part_type,
            "toolCallId": tool_call_id,
            "state": state,
            "input": input,
        });
        if let Some(output) = output {
            part.as_object_mut()
                .expect("tool part is object")
                .insert("output".to_string(), output);
        }
        part
    }

    fn dynamic_tool_part_json(
        tool_name: &str,
        tool_call_id: &str,
        state: &str,
        input: JsonValue,
        output: Option<JsonValue>,
    ) -> JsonValue {
        let mut part = tool_part_json("dynamic-tool", tool_call_id, state, input, output);
        part.as_object_mut()
            .expect("dynamic tool part is object")
            .insert("toolName".to_string(), json!(tool_name));
        part
    }

    fn approval_tool_part_json(
        part_type: &str,
        tool_call_id: &str,
        state: &str,
        provider_executed: bool,
    ) -> JsonValue {
        let mut part = json!({
            "type": part_type,
            "toolCallId": tool_call_id,
            "state": state,
            "input": { "city": "Tokyo" },
            "approval": { "id": format!("approval-{tool_call_id}") },
        });
        if part_type == "dynamic-tool" {
            part.as_object_mut()
                .expect("approval tool part is object")
                .insert("toolName".to_string(), json!("mcp.shorten_url"));
        }
        if state == "approval-responded" {
            part.as_object_mut()
                .expect("approval tool part is object")
                .insert(
                    "approval".to_string(),
                    json!({ "id": format!("approval-{tool_call_id}"), "approved": true }),
                );
        }
        if provider_executed {
            part.as_object_mut()
                .expect("approval tool part is object")
                .insert("providerExecuted".to_string(), json!(true));
        }
        part
    }

    #[derive(Default)]
    struct MockUiMessageStreamResponse {
        status: Option<u16>,
        status_text: Option<String>,
        headers: Headers,
        chunks: Vec<Vec<u8>>,
        ended: bool,
    }

    impl MockUiMessageStreamResponse {
        fn decoded_chunks(&self) -> Vec<String> {
            self.chunks
                .iter()
                .map(|chunk| String::from_utf8(chunk.clone()).expect("chunk decodes"))
                .collect()
        }
    }

    impl UiMessageStreamResponseWriter for MockUiMessageStreamResponse {
        type Error = Infallible;

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
}
