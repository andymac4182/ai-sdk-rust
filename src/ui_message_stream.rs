use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::FinishReason;
use crate::provider::{ProviderMetadata, TypeValidationContext, TypeValidationError};
use crate::provider_utils::{FlexibleSchema, normalize_headers, validate_types};
use crate::util::{InvalidArgumentError, merge_objects};

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

        /// Whether the approval status was decided automatically.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_automatic: Option<bool>,

        /// Provider-specific metadata for the approval request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Tool approval response is available.
    ToolApprovalResponse {
        /// Approval request identifier.
        approval_id: String,

        /// Whether the approval was granted.
        approved: bool,

        /// Optional approval or denial reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,

        /// Whether the approval is for a provider-executed tool call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_executed: Option<bool>,
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

    /// Abort notification for a UI-message stream.
    Abort {
        /// Optional abort reason supplied by the caller/runtime.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<JsonValue>,
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

/// Returns the tool name after the static `tool-` UI part prefix.
pub fn get_static_tool_name(part: &JsonValue) -> Option<&str> {
    ui_message_part_type(part)?.strip_prefix("tool-")
}

/// Checks whether a UI message part is provider-specific custom content.
pub fn is_custom_content_ui_part(part: &JsonValue) -> bool {
    ui_message_part_type(part) == Some("custom")
}

/// Checks whether a UI message part is a data part.
pub fn is_data_ui_part(part: &JsonValue) -> bool {
    ui_message_part_type(part).is_some_and(|part_type| part_type.starts_with("data-"))
}

/// Tool schemas used by [`validate_ui_messages`] to validate UI tool input and output parts.
#[derive(Clone, Debug)]
pub struct UiMessageValidationTool {
    /// Schema used to validate available tool input values.
    pub input_schema: FlexibleSchema<JsonValue>,

    /// Optional schema used to validate output values in `output-available` parts.
    pub output_schema: Option<FlexibleSchema<JsonValue>>,
}

impl UiMessageValidationTool {
    /// Creates validation schemas for one UI tool.
    pub fn new(input_schema: impl Into<FlexibleSchema<JsonValue>>) -> Self {
        Self {
            input_schema: input_schema.into(),
            output_schema: None,
        }
    }

    /// Adds an output schema for `output-available` parts.
    pub fn with_output_schema(
        mut self,
        output_schema: impl Into<FlexibleSchema<JsonValue>>,
    ) -> Self {
        self.output_schema = Some(output_schema.into());
        self
    }
}

/// Options accepted by [`validate_ui_messages`] and [`safe_validate_ui_messages`].
#[derive(Clone, Debug, Default)]
pub struct UiMessageValidationOptions {
    /// UI messages to validate. `None` mirrors upstream's nullish parameter error.
    pub messages: Option<JsonValue>,

    /// Optional schema used to validate each message metadata value.
    pub metadata_schema: Option<FlexibleSchema<JsonValue>>,

    /// Optional schemas keyed by the suffix of `data-*` UI parts.
    pub data_schemas: BTreeMap<String, FlexibleSchema<JsonValue>>,

    /// Optional tool schemas keyed by static `tool-*` names.
    pub tools: BTreeMap<String, UiMessageValidationTool>,
}

/// Error returned by [`validate_ui_messages`].
#[derive(Clone, Debug, PartialEq)]
pub enum UiMessageValidationError {
    /// The top-level `messages` argument was missing.
    InvalidArgument(InvalidArgumentError),

    /// A message, part, metadata value, data value, tool input, or tool output failed validation.
    TypeValidation(TypeValidationError),
}

impl fmt::Display for UiMessageValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(error) => error.fmt(formatter),
            Self::TypeValidation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for UiMessageValidationError {}

impl From<InvalidArgumentError> for UiMessageValidationError {
    fn from(error: InvalidArgumentError) -> Self {
        Self::InvalidArgument(error)
    }
}

impl From<TypeValidationError> for UiMessageValidationError {
    fn from(error: TypeValidationError) -> Self {
        Self::TypeValidation(error)
    }
}

/// Result returned by [`safe_validate_ui_messages`].
#[derive(Clone, Debug, PartialEq)]
pub enum SafeValidateUiMessagesResult {
    /// Validation succeeded and returns the original normalized JSON messages.
    Success { data: Vec<JsonValue> },

    /// Validation failed and returns the upstream-style error.
    Failure { error: UiMessageValidationError },
}

impl SafeValidateUiMessagesResult {
    /// Returns whether validation succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns whether validation failed.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }
}

/// Validates UI messages using the portable runtime rules from upstream `validateUIMessages`.
pub fn validate_ui_messages(
    options: UiMessageValidationOptions,
) -> Result<Vec<JsonValue>, UiMessageValidationError> {
    match safe_validate_ui_messages(options) {
        SafeValidateUiMessagesResult::Success { data } => Ok(data),
        SafeValidateUiMessagesResult::Failure { error } => Err(error),
    }
}

/// Validates UI messages and returns an explicit success/failure result.
pub fn safe_validate_ui_messages(
    options: UiMessageValidationOptions,
) -> SafeValidateUiMessagesResult {
    match validate_ui_messages_inner(options) {
        Ok(data) => SafeValidateUiMessagesResult::Success { data },
        Err(error) => SafeValidateUiMessagesResult::Failure { error },
    }
}

fn validate_ui_messages_inner(
    options: UiMessageValidationOptions,
) -> Result<Vec<JsonValue>, UiMessageValidationError> {
    let Some(messages) = options.messages else {
        return Err(InvalidArgumentError::new(
            "messages",
            JsonValue::Null,
            "messages parameter must be provided",
        )
        .into());
    };

    let messages_array = messages.as_array().ok_or_else(|| {
        TypeValidationError::with_cause_message(messages.clone(), "messages must be an array", None)
    })?;

    if messages_array.is_empty() {
        return Err(TypeValidationError::with_cause_message(
            messages.clone(),
            "Messages array must not be empty",
            None,
        )
        .into());
    }

    for (message_index, message) in messages_array.iter().enumerate() {
        validate_ui_message_structure(message)?;

        let message_object = message.as_object().expect("message structure validates");
        let message_id = message_object
            .get("id")
            .and_then(JsonValue::as_str)
            .expect("message id validates");

        if let Some(metadata_schema) = &options.metadata_schema {
            let metadata = message_object
                .get("metadata")
                .cloned()
                .unwrap_or(JsonValue::Null);
            validate_types(
                metadata,
                metadata_schema.clone(),
                Some(
                    TypeValidationContext::new()
                        .with_field(format!("messages[{message_index}].metadata"))
                        .with_entity_id(message_id),
                ),
            )?;
        }

        let parts = message_object
            .get("parts")
            .and_then(JsonValue::as_array)
            .expect("message parts validate");

        for (part_index, part) in parts.iter().enumerate() {
            validate_ui_message_part_structure(part)?;
            validate_ui_message_part_schema(
                part,
                message_index,
                part_index,
                &options.data_schemas,
                &options.tools,
            )?;
        }
    }

    Ok(messages_array.clone())
}

fn validate_ui_message_structure(message: &JsonValue) -> Result<(), TypeValidationError> {
    let object = require_object(message, message, "message must be an object")?;
    require_string(object, "id", message)?;
    require_enum(object, "role", &["system", "user", "assistant"], message)?;

    let parts = object
        .get("parts")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            TypeValidationError::with_cause_message(message.clone(), "parts must be an array", None)
        })?;

    if parts.is_empty() {
        return Err(TypeValidationError::with_cause_message(
            message.clone(),
            "Message must contain at least one part",
            None,
        ));
    }

    Ok(())
}

fn validate_ui_message_part_structure(part: &JsonValue) -> Result<(), TypeValidationError> {
    let object = require_object(part, part, "message part must be an object")?;
    let part_type = require_string(object, "type", part)?;

    match part_type {
        "text" | "reasoning" => {
            require_string(object, "text", part)?;
            optional_enum(object, "state", &["streaming", "done"], part)?;
            optional_object(object, "providerMetadata", part)?;
        }
        "custom" => {
            require_string(object, "kind", part)?;
            optional_object(object, "providerMetadata", part)?;
        }
        "source-url" => {
            require_string(object, "sourceId", part)?;
            require_string(object, "url", part)?;
            optional_string(object, "title", part)?;
            optional_object(object, "providerMetadata", part)?;
        }
        "source-document" => {
            require_string(object, "sourceId", part)?;
            require_string(object, "mediaType", part)?;
            require_string(object, "title", part)?;
            optional_string(object, "filename", part)?;
            optional_object(object, "providerMetadata", part)?;
        }
        "file" => {
            require_string(object, "mediaType", part)?;
            optional_string(object, "filename", part)?;
            require_string(object, "url", part)?;
            optional_object(object, "providerMetadata", part)?;
        }
        "reasoning-file" => {
            require_string(object, "mediaType", part)?;
            require_string(object, "url", part)?;
            optional_object(object, "providerMetadata", part)?;
        }
        "step-start" => {}
        "dynamic-tool" => validate_ui_tool_part_structure(object, part, true)?,
        part_type if part_type.starts_with("data-") => {
            require_key(object, "data", part)?;
        }
        part_type if part_type.starts_with("tool-") => {
            validate_ui_tool_part_structure(object, part, false)?
        }
        _ => {
            return Err(TypeValidationError::with_cause_message(
                part.clone(),
                format!("Unsupported UI message part type {part_type}"),
                None,
            ));
        }
    }

    Ok(())
}

fn validate_ui_tool_part_structure(
    object: &JsonObject,
    part: &JsonValue,
    dynamic: bool,
) -> Result<(), TypeValidationError> {
    if dynamic {
        require_string(object, "toolName", part)?;
    }
    require_string(object, "toolCallId", part)?;
    optional_object(object, "toolMetadata", part)?;
    optional_bool(object, "providerExecuted", part)?;
    optional_object(object, "callProviderMetadata", part)?;
    let state = require_string(object, "state", part)?;

    match state {
        "input-streaming" => {
            reject_keys(object, &["output", "errorText", "approval"], part)?;
        }
        "input-available" => {
            require_key(object, "input", part)?;
            reject_keys(object, &["output", "errorText", "approval"], part)?;
        }
        "approval-requested" => {
            require_key(object, "input", part)?;
            reject_keys(object, &["output", "errorText"], part)?;
            validate_approval(object.get("approval"), part, ApprovalKind::Requested)?;
        }
        "approval-responded" => {
            require_key(object, "input", part)?;
            reject_keys(object, &["output", "errorText"], part)?;
            validate_approval(object.get("approval"), part, ApprovalKind::Responded)?;
        }
        "output-available" => {
            require_key(object, "input", part)?;
            require_key(object, "output", part)?;
            reject_keys(object, &["errorText"], part)?;
            optional_object(object, "resultProviderMetadata", part)?;
            optional_bool(object, "preliminary", part)?;
            if object.contains_key("approval") {
                validate_approval(object.get("approval"), part, ApprovalKind::ApprovedOutput)?;
            }
        }
        "output-error" => {
            optional_object(object, "resultProviderMetadata", part)?;
            optional_string(object, "errorText", part)?;
            require_string(object, "errorText", part)?;
            reject_keys(object, &["output"], part)?;
            if object.contains_key("approval") {
                validate_approval(object.get("approval"), part, ApprovalKind::ApprovedOutput)?;
            }
        }
        "output-denied" => {
            require_key(object, "input", part)?;
            reject_keys(object, &["output", "errorText"], part)?;
            validate_approval(object.get("approval"), part, ApprovalKind::DeniedOutput)?;
        }
        _ => {
            return Err(TypeValidationError::with_cause_message(
                part.clone(),
                format!("Unsupported tool part state {state}"),
                None,
            ));
        }
    }

    Ok(())
}

fn validate_ui_message_part_schema(
    part: &JsonValue,
    message_index: usize,
    part_index: usize,
    data_schemas: &BTreeMap<String, FlexibleSchema<JsonValue>>,
    tools: &BTreeMap<String, UiMessageValidationTool>,
) -> Result<(), TypeValidationError> {
    let object = part.as_object().expect("part structure validates");
    let part_type = object
        .get("type")
        .and_then(JsonValue::as_str)
        .expect("part type validates");

    if part_type.starts_with("data-") && !data_schemas.is_empty() {
        let data_name = part_type.trim_start_matches("data-");
        let data = object.get("data").cloned().unwrap_or(JsonValue::Null);
        let data_id = object.get("id").and_then(JsonValue::as_str);
        let context = TypeValidationContext::new()
            .with_field(format!(
                "messages[{message_index}].parts[{part_index}].data"
            ))
            .with_entity_name(data_name);
        let context = match data_id {
            Some(id) => context.with_entity_id(id),
            None => context,
        };

        let Some(data_schema) = data_schemas.get(data_name) else {
            return Err(TypeValidationError::new(
                data,
                format!("No data schema found for data part {data_name}"),
                Some(context),
            ));
        };

        validate_types(data, data_schema.clone(), Some(context))?;
    }

    if part_type.starts_with("tool-") && !tools.is_empty() {
        let tool_name = part_type.trim_start_matches("tool-");
        let state = object
            .get("state")
            .and_then(JsonValue::as_str)
            .expect("tool state validates");
        let tool_call_id = object
            .get("toolCallId")
            .and_then(JsonValue::as_str)
            .expect("tool call id validates");

        let Some(tool) = tools.get(tool_name) else {
            if matches!(state, "output-available" | "output-error" | "output-denied") {
                return Ok(());
            }

            return Err(TypeValidationError::new(
                object.get("input").cloned().unwrap_or(JsonValue::Null),
                format!("No tool schema found for tool part {tool_name}"),
                Some(
                    TypeValidationContext::new()
                        .with_field(format!(
                            "messages[{message_index}].parts[{part_index}].input"
                        ))
                        .with_entity_name(tool_name)
                        .with_entity_id(tool_call_id),
                ),
            ));
        };

        if matches!(state, "input-available" | "output-available")
            || (state == "output-error" && object.contains_key("input"))
        {
            validate_types(
                object.get("input").cloned().unwrap_or(JsonValue::Null),
                tool.input_schema.clone(),
                Some(
                    TypeValidationContext::new()
                        .with_field(format!(
                            "messages[{message_index}].parts[{part_index}].input"
                        ))
                        .with_entity_name(tool_name)
                        .with_entity_id(tool_call_id),
                ),
            )?;
        }

        if state == "output-available" {
            if let Some(output_schema) = &tool.output_schema {
                validate_types(
                    object.get("output").cloned().unwrap_or(JsonValue::Null),
                    output_schema.clone(),
                    Some(
                        TypeValidationContext::new()
                            .with_field(format!(
                                "messages[{message_index}].parts[{part_index}].output"
                            ))
                            .with_entity_name(tool_name)
                            .with_entity_id(tool_call_id),
                    ),
                )?;
            }
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ApprovalKind {
    Requested,
    Responded,
    ApprovedOutput,
    DeniedOutput,
}

fn validate_approval(
    approval: Option<&JsonValue>,
    part: &JsonValue,
    kind: ApprovalKind,
) -> Result<(), TypeValidationError> {
    let approval = approval.ok_or_else(|| {
        TypeValidationError::with_cause_message(part.clone(), "approval must be provided", None)
    })?;
    let approval_object = require_object(approval, part, "approval must be an object")?;
    require_string(approval_object, "id", part)?;
    optional_bool(approval_object, "isAutomatic", part)?;
    optional_string(approval_object, "reason", part)?;

    match kind {
        ApprovalKind::Requested => {
            reject_keys(approval_object, &["approved", "reason"], part)?;
        }
        ApprovalKind::Responded => {
            require_bool(approval_object, "approved", part)?;
        }
        ApprovalKind::ApprovedOutput => {
            require_bool_value(approval_object, "approved", true, part)?;
        }
        ApprovalKind::DeniedOutput => {
            require_bool_value(approval_object, "approved", false, part)?;
        }
    }

    Ok(())
}

fn require_object<'a>(
    value: &'a JsonValue,
    whole_value: &JsonValue,
    message: impl Into<String>,
) -> Result<&'a JsonObject, TypeValidationError> {
    value
        .as_object()
        .ok_or_else(|| TypeValidationError::with_cause_message(whole_value.clone(), message, None))
}

fn require_key<'a>(
    object: &'a JsonObject,
    key: &str,
    whole_value: &JsonValue,
) -> Result<&'a JsonValue, TypeValidationError> {
    object.get(key).ok_or_else(|| {
        TypeValidationError::with_cause_message(
            whole_value.clone(),
            format!("{key} must be provided"),
            None,
        )
    })
}

fn require_string<'a>(
    object: &'a JsonObject,
    key: &str,
    whole_value: &JsonValue,
) -> Result<&'a str, TypeValidationError> {
    require_key(object, key, whole_value)?
        .as_str()
        .ok_or_else(|| {
            TypeValidationError::with_cause_message(
                whole_value.clone(),
                format!("{key} must be a string"),
                None,
            )
        })
}

fn optional_string(
    object: &JsonObject,
    key: &str,
    whole_value: &JsonValue,
) -> Result<(), TypeValidationError> {
    if object.get(key).is_some_and(|value| !value.is_string()) {
        return Err(TypeValidationError::with_cause_message(
            whole_value.clone(),
            format!("{key} must be a string"),
            None,
        ));
    }

    Ok(())
}

fn require_bool(
    object: &JsonObject,
    key: &str,
    whole_value: &JsonValue,
) -> Result<bool, TypeValidationError> {
    require_key(object, key, whole_value)?
        .as_bool()
        .ok_or_else(|| {
            TypeValidationError::with_cause_message(
                whole_value.clone(),
                format!("{key} must be a boolean"),
                None,
            )
        })
}

fn require_bool_value(
    object: &JsonObject,
    key: &str,
    expected: bool,
    whole_value: &JsonValue,
) -> Result<(), TypeValidationError> {
    let value = require_bool(object, key, whole_value)?;
    if value != expected {
        return Err(TypeValidationError::with_cause_message(
            whole_value.clone(),
            format!("{key} must be {expected}"),
            None,
        ));
    }

    Ok(())
}

fn optional_bool(
    object: &JsonObject,
    key: &str,
    whole_value: &JsonValue,
) -> Result<(), TypeValidationError> {
    if object.get(key).is_some_and(|value| !value.is_boolean()) {
        return Err(TypeValidationError::with_cause_message(
            whole_value.clone(),
            format!("{key} must be a boolean"),
            None,
        ));
    }

    Ok(())
}

fn optional_object(
    object: &JsonObject,
    key: &str,
    whole_value: &JsonValue,
) -> Result<(), TypeValidationError> {
    if object.get(key).is_some_and(|value| !value.is_object()) {
        return Err(TypeValidationError::with_cause_message(
            whole_value.clone(),
            format!("{key} must be an object"),
            None,
        ));
    }

    Ok(())
}

fn require_enum<'a>(
    object: &'a JsonObject,
    key: &str,
    variants: &[&str],
    whole_value: &JsonValue,
) -> Result<&'a str, TypeValidationError> {
    let value = require_string(object, key, whole_value)?;
    if variants.contains(&value) {
        Ok(value)
    } else {
        Err(TypeValidationError::with_cause_message(
            whole_value.clone(),
            format!("{key} must be one of {}", variants.join(", ")),
            None,
        ))
    }
}

fn optional_enum(
    object: &JsonObject,
    key: &str,
    variants: &[&str],
    whole_value: &JsonValue,
) -> Result<(), TypeValidationError> {
    if object.contains_key(key) {
        require_enum(object, key, variants, whole_value)?;
    }

    Ok(())
}

fn reject_keys(
    object: &JsonObject,
    keys: &[&str],
    whole_value: &JsonValue,
) -> Result<(), TypeValidationError> {
    for key in keys {
        if object.contains_key(*key) {
            return Err(TypeValidationError::with_cause_message(
                whole_value.clone(),
                format!("{key} must not be present"),
                None,
            ));
        }
    }

    Ok(())
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
            is_automatic: None,
            provider_metadata: None,
        }
    }

    /// Creates a tool-approval-response UI-message chunk.
    pub fn tool_approval_response(approval_id: impl Into<String>, approved: bool) -> Self {
        Self::ToolApprovalResponse {
            approval_id: approval_id.into(),
            approved,
            reason: None,
            provider_executed: None,
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

    /// Creates a stream-abort UI-message chunk without a reason.
    pub fn abort() -> Self {
        Self::Abort { reason: None }
    }

    /// Creates a stream-abort UI-message chunk with a reason.
    pub fn abort_with_reason(reason: impl Into<JsonValue>) -> Self {
        Self::Abort {
            reason: Some(reason.into()),
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

    /// Whether an abort chunk has been observed.
    pub aborted: bool,

    /// Abort reason from the most recent abort chunk, when supplied.
    pub abort_reason: Option<JsonValue>,

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
            aborted: false,
            abort_reason: None,
            active_text_parts: BTreeMap::new(),
            active_reasoning_parts: BTreeMap::new(),
        }
    }
}

/// Event passed to a UI-message stream finish callback.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessageStreamFinishEvent {
    /// Final accumulated UI message.
    pub message: UiMessage,

    /// Message snapshots emitted while applying stream chunks.
    pub message_states: Vec<UiMessage>,

    /// Last finish reason observed in the stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,

    /// Whether an abort chunk was observed.
    pub is_aborted: bool,

    /// Abort reason from the last abort chunk, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<JsonValue>,
}

/// Event passed to the upstream-style UI-message stream finish callback.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessageStreamFinishCallbackEvent {
    /// The updated conversation messages after applying the response stream.
    pub messages: Vec<UiMessage>,

    /// Whether the response continued the last original assistant message.
    pub is_continuation: bool,

    /// Whether an abort chunk was observed.
    pub is_aborted: bool,

    /// The response message sent to the client.
    pub response_message: UiMessage,

    /// Last finish reason observed in the stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
}

/// Callback invoked after a UI-message stream has produced its final message.
pub type UiMessageStreamFinishCallbackFunction =
    dyn Fn(UiMessageStreamFinishCallbackEvent) + Send + Sync + 'static;

/// Callback wrapper for upstream `UIMessageStreamOnFinishCallback`.
#[derive(Clone)]
pub struct UiMessageStreamFinishCallback {
    callback: Arc<UiMessageStreamFinishCallbackFunction>,
}

impl UiMessageStreamFinishCallback {
    /// Creates a UI-message stream finish callback.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(UiMessageStreamFinishCallbackEvent) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Runs the finish callback.
    pub fn finish(&self, event: UiMessageStreamFinishCallbackEvent) {
        (self.callback)(event);
    }
}

impl fmt::Debug for UiMessageStreamFinishCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiMessageStreamFinishCallback")
            .finish_non_exhaustive()
    }
}

/// Event passed to the upstream-style UI-message stream step-finish callback.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessageStreamStepFinishCallbackEvent {
    /// The updated conversation messages after applying the current step.
    pub messages: Vec<UiMessage>,

    /// Whether the response continued the last original assistant message.
    pub is_continuation: bool,

    /// The response message sent to the client.
    pub response_message: UiMessage,
}

/// Callback invoked after a UI-message stream step has finished.
pub type UiMessageStreamStepFinishCallbackFunction =
    dyn Fn(UiMessageStreamStepFinishCallbackEvent) + Send + Sync + 'static;

/// Callback wrapper for upstream `UIMessageStreamOnStepFinishCallback`.
#[derive(Clone)]
pub struct UiMessageStreamStepFinishCallback {
    callback: Arc<UiMessageStreamStepFinishCallbackFunction>,
}

impl UiMessageStreamStepFinishCallback {
    /// Creates a UI-message stream step-finish callback.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(UiMessageStreamStepFinishCallbackEvent) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Runs the step-finish callback.
    pub fn finish_step(&self, event: UiMessageStreamStepFinishCallbackEvent) {
        (self.callback)(event);
    }
}

impl fmt::Debug for UiMessageStreamStepFinishCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiMessageStreamStepFinishCallback")
            .finish_non_exhaustive()
    }
}

/// Callback invoked after a UI-message stream has been read.
pub type UiMessageStreamOnFinishFunction =
    dyn Fn(UiMessageStreamFinishEvent) + Send + Sync + 'static;

/// Callback wrapper for upstream-style UI-message stream `onFinish`.
#[derive(Clone)]
pub struct UiMessageStreamOnFinish {
    callback: Arc<UiMessageStreamOnFinishFunction>,
}

impl UiMessageStreamOnFinish {
    /// Creates a UI-message stream finish callback.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(UiMessageStreamFinishEvent) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Runs the finish callback.
    pub fn finish(&self, event: UiMessageStreamFinishEvent) {
        (self.callback)(event);
    }
}

impl fmt::Debug for UiMessageStreamOnFinish {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiMessageStreamOnFinish")
            .finish_non_exhaustive()
    }
}

/// Options for [`read_ui_message_stream`].
#[derive(Clone, Debug)]
pub struct ReadUiMessageStreamOptions {
    /// Previous assistant message to resume from.
    pub message: Option<UiMessage>,

    /// UI-message stream chunks to apply.
    pub stream: Vec<UiMessageChunk>,

    /// Whether an `error` chunk terminates processing.
    pub terminate_on_error: bool,

    /// Optional callback invoked after stream chunks have been applied.
    pub on_finish: Option<UiMessageStreamOnFinish>,
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
            on_finish: None,
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

    /// Sets a callback that receives the final UI-message stream state.
    pub fn with_on_finish<F>(mut self, on_finish: F) -> Self
    where
        F: Fn(UiMessageStreamFinishEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(UiMessageStreamOnFinish::new(on_finish));
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

/// Decodes byte text-stream chunks and calls the provided text-part callback.
pub fn process_text_stream<I, B, F>(stream: I, mut on_text_part: F)
where
    I: IntoIterator<Item = B>,
    B: AsRef<[u8]>,
    F: FnMut(String),
{
    for chunk in stream {
        on_text_part(String::from_utf8_lossy(chunk.as_ref()).into_owned());
    }
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
            | UiMessageChunk::ToolApprovalResponse { .. }
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
            UiMessageChunk::Abort { reason } => {
                let active_indices = state
                    .active_text_parts
                    .values()
                    .chain(state.active_reasoning_parts.values())
                    .copied()
                    .collect::<Vec<_>>();

                for index in &active_indices {
                    if let Some(part) = state.message.parts.get_mut(*index) {
                        set_ui_message_part_state(part, "done");
                    }
                }

                state.active_text_parts.clear();
                state.active_reasoning_parts.clear();
                state.aborted = true;
                state.abort_reason = reason;

                if !active_indices.is_empty() {
                    updates.push(state.message.clone());
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

    let message_states =
        process_ui_message_stream(&mut state, options.stream, options.terminate_on_error)?;

    if let Some(on_finish) = options.on_finish {
        on_finish.finish(UiMessageStreamFinishEvent {
            message: state.message.clone(),
            message_states: message_states.clone(),
            finish_reason: state.finish_reason,
            is_aborted: state.aborted,
            abort_reason: state.abort_reason,
        });
    }

    Ok(message_states)
}

/// Options for [`handle_ui_message_stream_finish`].
#[derive(Clone, Debug)]
pub struct HandleUiMessageStreamFinishOptions {
    /// UI-message stream chunks to forward.
    pub stream: Vec<UiMessageChunk>,

    /// Response message id to inject into a start chunk that does not include one.
    pub message_id: Option<String>,

    /// Original UI messages used to compute continuation state.
    pub original_messages: Vec<UiMessage>,

    /// Optional callback invoked with the final persisted-message state.
    pub on_finish: Option<UiMessageStreamFinishCallback>,

    /// Optional callback invoked after each step finishes.
    pub on_step_finish: Option<UiMessageStreamStepFinishCallback>,
}

impl HandleUiMessageStreamFinishOptions {
    /// Creates finish-handler options for a UI-message stream.
    pub fn new<I>(stream: I) -> Self
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        Self {
            stream: stream.into_iter().collect(),
            message_id: None,
            original_messages: Vec::new(),
            on_finish: None,
            on_step_finish: None,
        }
    }

    /// Sets the response message id used when the stream start chunk has no id.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Sets the original UI messages for persistence/continuation mode.
    pub fn with_original_messages<I>(mut self, original_messages: I) -> Self
    where
        I: IntoIterator<Item = UiMessage>,
    {
        self.original_messages = original_messages.into_iter().collect();
        self
    }

    /// Sets the upstream-style finish callback.
    pub fn with_on_finish<F>(mut self, on_finish: F) -> Self
    where
        F: Fn(UiMessageStreamFinishCallbackEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(UiMessageStreamFinishCallback::new(on_finish));
        self
    }

    /// Sets a pre-built upstream-style finish callback.
    pub fn with_finish_callback(mut self, on_finish: UiMessageStreamFinishCallback) -> Self {
        self.on_finish = Some(on_finish);
        self
    }

    /// Sets the upstream-style step-finish callback.
    pub fn with_on_step_finish<F>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(UiMessageStreamStepFinishCallbackEvent) + Send + Sync + 'static,
    {
        self.on_step_finish = Some(UiMessageStreamStepFinishCallback::new(on_step_finish));
        self
    }

    /// Sets a pre-built upstream-style step-finish callback.
    pub fn with_step_finish_callback(
        mut self,
        on_step_finish: UiMessageStreamStepFinishCallback,
    ) -> Self {
        self.on_step_finish = Some(on_step_finish);
        self
    }
}

/// Options for [`create_ui_message_stream`].
#[derive(Clone, Debug, Default)]
pub struct CreateUiMessageStreamOptions {
    /// Response message id generated for the created stream.
    pub message_id: Option<String>,

    /// Original UI messages used to compute continuation state.
    pub original_messages: Vec<UiMessage>,

    /// Optional callback invoked with the final persisted-message state.
    pub on_finish: Option<UiMessageStreamFinishCallback>,

    /// Optional callback invoked after each step finishes.
    pub on_step_finish: Option<UiMessageStreamStepFinishCallback>,

    /// Optional callback used to map execution errors into UI-safe text.
    pub on_error: Option<UiMessageStreamCreateErrorHandler>,
}

impl CreateUiMessageStreamOptions {
    /// Creates default create-stream options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the generated response message id.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Sets the original UI messages for persistence/continuation mode.
    pub fn with_original_messages<I>(mut self, original_messages: I) -> Self
    where
        I: IntoIterator<Item = UiMessage>,
    {
        self.original_messages = original_messages.into_iter().collect();
        self
    }

    /// Sets the upstream-style finish callback.
    pub fn with_on_finish<F>(mut self, on_finish: F) -> Self
    where
        F: Fn(UiMessageStreamFinishCallbackEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(UiMessageStreamFinishCallback::new(on_finish));
        self
    }

    /// Sets the upstream-style step-finish callback.
    pub fn with_on_step_finish<F>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(UiMessageStreamStepFinishCallbackEvent) + Send + Sync + 'static,
    {
        self.on_step_finish = Some(UiMessageStreamStepFinishCallback::new(on_step_finish));
        self
    }

    /// Sets a callback that maps create-stream execution errors to UI-message error text.
    pub fn with_on_error<F>(mut self, on_error: F) -> Self
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        self.on_error = Some(UiMessageStreamCreateErrorHandler::new(on_error));
        self
    }
}

/// Callback invoked to mask create-stream execution errors.
pub type UiMessageStreamCreateErrorFunction = dyn Fn(&str) -> String + Send + Sync + 'static;

/// Callback wrapper for `create_ui_message_stream` error handling.
#[derive(Clone)]
pub struct UiMessageStreamCreateErrorHandler {
    callback: Arc<UiMessageStreamCreateErrorFunction>,
}

impl UiMessageStreamCreateErrorHandler {
    /// Creates a create-stream error handler.
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Maps an execution error into UI-safe error text.
    pub fn error_text(&self, error: &str) -> String {
        (self.callback)(error)
    }
}

impl fmt::Debug for UiMessageStreamCreateErrorHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiMessageStreamCreateErrorHandler")
            .finish_non_exhaustive()
    }
}

/// Writer used by [`create_ui_message_stream`].
#[derive(Clone, Debug, Default)]
pub struct UiMessageStreamWriter {
    chunks: Vec<UiMessageChunk>,
    merge_errors: Vec<String>,
}

impl UiMessageStreamWriter {
    /// Appends one UI-message stream chunk.
    pub fn write(&mut self, chunk: UiMessageChunk) {
        self.chunks.push(chunk);
    }

    /// Merges another UI-message stream into this writer.
    pub fn merge<I>(&mut self, stream: I)
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        self.chunks.extend(stream);
    }

    /// Merges a fallible UI-message stream into this writer.
    pub fn merge_result<I, E>(&mut self, stream: I)
    where
        I: IntoIterator<Item = Result<UiMessageChunk, E>>,
        E: fmt::Display,
    {
        for chunk in stream {
            match chunk {
                Ok(chunk) => self.chunks.push(chunk),
                Err(error) => {
                    self.merge_errors.push(error.to_string());
                    break;
                }
            }
        }
    }

    /// Returns the written chunks.
    pub fn into_chunks(self) -> Vec<UiMessageChunk> {
        self.into_chunks_with_error_handler(None)
    }

    fn into_chunks_with_error_handler(
        self,
        on_error: Option<&UiMessageStreamCreateErrorHandler>,
    ) -> Vec<UiMessageChunk> {
        let Self {
            mut chunks,
            merge_errors,
        } = self;
        chunks.extend(merge_errors.into_iter().map(|error| {
            UiMessageChunk::error(
                on_error
                    .map(|on_error| on_error.error_text(&error))
                    .unwrap_or(error),
            )
        }));
        chunks
    }
}

/// Creates a UI-message stream from a Rust writer callback.
pub fn create_ui_message_stream<F>(
    options: CreateUiMessageStreamOptions,
    execute: F,
) -> Result<Vec<UiMessageChunk>, UiMessageStreamProcessError>
where
    F: FnOnce(&mut UiMessageStreamWriter),
{
    create_ui_message_stream_with_result(options, |writer| {
        execute(writer);
        Ok::<(), std::convert::Infallible>(())
    })
}

/// Creates a UI-message stream from a fallible Rust writer callback.
pub fn create_ui_message_stream_with_result<F, E>(
    options: CreateUiMessageStreamOptions,
    execute: F,
) -> Result<Vec<UiMessageChunk>, UiMessageStreamProcessError>
where
    F: FnOnce(&mut UiMessageStreamWriter) -> Result<(), E>,
    E: fmt::Display,
{
    let CreateUiMessageStreamOptions {
        message_id,
        original_messages,
        on_finish,
        on_step_finish,
        on_error,
    } = options;

    let mut writer = UiMessageStreamWriter::default();
    if let Err(error) = execute(&mut writer) {
        let error = error.to_string();
        let error_text = on_error
            .as_ref()
            .map(|on_error| on_error.error_text(&error))
            .unwrap_or(error);
        writer.write(UiMessageChunk::error(error_text));
    }

    let mut finish_options = HandleUiMessageStreamFinishOptions::new(
        writer.into_chunks_with_error_handler(on_error.as_ref()),
    );
    if let Some(message_id) = message_id {
        finish_options = finish_options.with_message_id(message_id);
    }
    finish_options = finish_options.with_original_messages(original_messages);
    if let Some(on_finish) = on_finish {
        finish_options = finish_options.with_finish_callback(on_finish);
    }
    if let Some(on_step_finish) = on_step_finish {
        finish_options = finish_options.with_step_finish_callback(on_step_finish);
    }

    handle_ui_message_stream_finish(finish_options)
}

/// Applies upstream-style finish handling to a UI-message stream.
pub fn handle_ui_message_stream_finish(
    options: HandleUiMessageStreamFinishOptions,
) -> Result<Vec<UiMessageChunk>, UiMessageStreamProcessError> {
    let HandleUiMessageStreamFinishOptions {
        mut stream,
        mut message_id,
        original_messages,
        on_finish,
        on_step_finish,
    } = options;

    let last_message = original_messages
        .last()
        .filter(|message| message.role == UiMessageRole::Assistant)
        .cloned();

    if let Some(last_message) = &last_message {
        message_id = Some(last_message.id.clone());
    }

    for chunk in &mut stream {
        if let UiMessageChunk::Start {
            message_id: chunk_message_id,
            ..
        } = chunk
        {
            if chunk_message_id.is_none() {
                *chunk_message_id = message_id.clone();
            }
        }
    }

    if on_finish.is_some() || on_step_finish.is_some() {
        let mut state = StreamingUiMessageState::new(
            message_id.clone().unwrap_or_default(),
            last_message.clone(),
        );

        for chunk in stream.iter().cloned() {
            let is_finish_step = matches!(chunk, UiMessageChunk::FinishStep);
            process_ui_message_stream(&mut state, [chunk], false)?;

            if is_finish_step {
                if let Some(on_step_finish) = &on_step_finish {
                    let is_continuation =
                        ui_message_stream_is_continuation(&state.message, last_message.as_ref());
                    on_step_finish.finish_step(UiMessageStreamStepFinishCallbackEvent {
                        messages: ui_message_stream_persisted_messages(
                            &original_messages,
                            state.message.clone(),
                            is_continuation,
                        ),
                        is_continuation,
                        response_message: state.message.clone(),
                    });
                }
            }
        }

        let is_continuation =
            ui_message_stream_is_continuation(&state.message, last_message.as_ref());

        if let Some(on_finish) = on_finish {
            on_finish.finish(UiMessageStreamFinishCallbackEvent {
                messages: ui_message_stream_persisted_messages(
                    &original_messages,
                    state.message.clone(),
                    is_continuation,
                ),
                is_continuation,
                is_aborted: state.aborted,
                response_message: state.message,
                finish_reason: state.finish_reason,
            });
        }
    }

    Ok(stream)
}

fn ui_message_stream_is_continuation(
    response_message: &UiMessage,
    last_message: Option<&UiMessage>,
) -> bool {
    last_message.is_some_and(|last_message| response_message.id == last_message.id)
}

fn ui_message_stream_persisted_messages(
    original_messages: &[UiMessage],
    response_message: UiMessage,
    is_continuation: bool,
) -> Vec<UiMessage> {
    let mut messages = if is_continuation {
        original_messages
            .iter()
            .take(original_messages.len().saturating_sub(1))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        original_messages.to_vec()
    };
    messages.push(response_message);
    messages
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
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::provider_utils::{Schema, ValidationResult};
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
    fn process_text_stream_should_process_stream_chunks_correctly() {
        let mut chunks = Vec::new();

        process_text_stream(
            [b"Hello".as_slice(), b" ".as_slice(), b"World".as_slice()],
            |chunk| {
                chunks.push(chunk);
            },
        );

        assert_eq!(chunks, vec!["Hello", " ", "World"]);
    }

    #[test]
    fn process_text_stream_should_handle_empty_streams() {
        let calls = Rc::new(Cell::new(0usize));
        let callback_calls = Rc::clone(&calls);

        process_text_stream(Vec::<Vec<u8>>::new(), move |_| {
            callback_calls.set(callback_calls.get() + 1);
        });

        assert_eq!(calls.get(), 0);
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
    fn read_ui_message_stream_invokes_finish_callback_with_final_state() {
        let finish_events = Arc::new(Mutex::new(Vec::<UiMessageStreamFinishEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let messages = read_ui_message_stream(
            ReadUiMessageStreamOptions::new([
                UiMessageChunk::start_with_message_id("msg-123"),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "Hello"),
                UiMessageChunk::text_end("text-1"),
                UiMessageChunk::finish_with_reason(FinishReason::Stop),
            ])
            .with_on_finish(move |event| {
                finish_events_for_callback
                    .lock()
                    .expect("finish events lock")
                    .push(event);
            }),
        )
        .expect("stream reads");

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert_eq!(
            finish_events[0].message,
            messages.last().expect("final message state").clone()
        );
        assert_eq!(finish_events[0].message_states, messages);
        assert_eq!(finish_events[0].finish_reason, Some(FinishReason::Stop));
        assert!(!finish_events[0].is_aborted);
        assert_eq!(finish_events[0].abort_reason, None);
    }

    #[test]
    fn handle_ui_message_stream_finish_injects_id_and_calls_on_finish() {
        let finish_events = Arc::new(Mutex::new(Vec::<UiMessageStreamFinishCallbackEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let chunks = handle_ui_message_stream_finish(
            HandleUiMessageStreamFinishOptions::new([
                UiMessageChunk::start(),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "Hello"),
                UiMessageChunk::text_end("text-1"),
                UiMessageChunk::finish_with_reason(FinishReason::Stop),
            ])
            .with_message_id("response-message-id")
            .with_original_messages([UiMessage::new("user-1", UiMessageRole::User)])
            .with_on_finish(move |event| {
                finish_events_for_callback
                    .lock()
                    .expect("finish events lock")
                    .push(event);
            }),
        )
        .expect("stream finish is handled");

        assert_eq!(
            serde_json::to_value(&chunks[0]).expect("chunk serializes"),
            json!({ "type": "start", "messageId": "response-message-id" })
        );

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert!(!finish_events[0].is_continuation);
        assert!(!finish_events[0].is_aborted);
        assert_eq!(finish_events[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(finish_events[0].messages.len(), 2);
        assert_eq!(finish_events[0].messages[0].id, "user-1");
        assert_eq!(finish_events[0].response_message.id, "response-message-id");
        assert_eq!(
            finish_events[0].response_message.parts,
            vec![json!({ "type": "text", "text": "Hello", "state": "done" })]
        );
        assert_eq!(
            finish_events[0].messages[1],
            finish_events[0].response_message
        );
    }

    #[test]
    fn create_ui_message_stream_invokes_step_and_finish_callbacks() {
        let step_events = Arc::new(Mutex::new(
            Vec::<UiMessageStreamStepFinishCallbackEvent>::new(),
        ));
        let step_events_for_callback = Arc::clone(&step_events);
        let finish_events = Arc::new(Mutex::new(Vec::<UiMessageStreamFinishCallbackEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let chunks = create_ui_message_stream(
            CreateUiMessageStreamOptions::new()
                .with_message_id("response-message-id")
                .with_original_messages([UiMessage::new("user-1", UiMessageRole::User)])
                .with_on_step_finish(move |event| {
                    step_events_for_callback
                        .lock()
                        .expect("step events lock")
                        .push(event);
                })
                .with_on_finish(move |event| {
                    finish_events_for_callback
                        .lock()
                        .expect("finish events lock")
                        .push(event);
                }),
            |writer| {
                writer.write(UiMessageChunk::start());
                writer.write(UiMessageChunk::start_step());
                writer.write(UiMessageChunk::text_start("text-1"));
                writer.write(UiMessageChunk::text_delta("text-1", "one"));
                writer.write(UiMessageChunk::text_end("text-1"));
                writer.write(UiMessageChunk::finish_step());
                writer.merge([
                    UiMessageChunk::start_step(),
                    UiMessageChunk::text_start("text-2"),
                    UiMessageChunk::text_delta("text-2", "two"),
                    UiMessageChunk::text_end("text-2"),
                    UiMessageChunk::finish_step(),
                    UiMessageChunk::finish_with_reason(FinishReason::Stop),
                ]);
            },
        )
        .expect("stream is created");

        assert_eq!(
            serde_json::to_value(&chunks[0]).expect("chunk serializes"),
            json!({ "type": "start", "messageId": "response-message-id" })
        );

        let step_events = step_events.lock().expect("step events lock");
        assert_eq!(step_events.len(), 2);
        assert!(!step_events[0].is_continuation);
        assert_eq!(step_events[0].messages.len(), 2);
        assert_eq!(
            step_events[0].response_message.parts,
            vec![
                json!({ "type": "step-start" }),
                json!({ "type": "text", "text": "one", "state": "done" })
            ]
        );
        assert_eq!(
            step_events[1].response_message.parts,
            vec![
                json!({ "type": "step-start" }),
                json!({ "type": "text", "text": "one", "state": "done" }),
                json!({ "type": "step-start" }),
                json!({ "type": "text", "text": "two", "state": "done" })
            ]
        );
        assert_eq!(step_events[1].messages[1], step_events[1].response_message);

        let finish_events = finish_events.lock().expect("finish events lock");
        assert_eq!(finish_events.len(), 1);
        assert!(!finish_events[0].is_continuation);
        assert!(!finish_events[0].is_aborted);
        assert_eq!(finish_events[0].finish_reason, Some(FinishReason::Stop));
        assert_eq!(finish_events[0].messages[0].id, "user-1");
        assert_eq!(
            finish_events[0].response_message,
            step_events[1].response_message
        );
    }

    #[test]
    fn create_ui_message_stream_adds_error_chunk_when_execute_returns_error() {
        let chunks = create_ui_message_stream_with_result(
            CreateUiMessageStreamOptions::new().with_on_error(|error| format!("masked {error}")),
            |writer| {
                writer.write(UiMessageChunk::text_delta("text-1", "before-error"));
                Err("execute-error")
            },
        )
        .expect("stream is created");

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "text-delta", "id": "text-1", "delta": "before-error" },
                { "type": "error", "errorText": "masked execute-error" }
            ])
        );
    }

    #[test]
    fn create_ui_message_stream_adds_error_chunk_when_merged_stream_errors() {
        let chunks = create_ui_message_stream(
            CreateUiMessageStreamOptions::new().with_on_error(|error| format!("masked {error}")),
            |writer| {
                writer.merge_result([
                    Ok(UiMessageChunk::text_delta("text-1", "1a")),
                    Err("stream-1-error"),
                ]);
                writer.merge_result([
                    Ok::<_, &str>(UiMessageChunk::text_delta("text-2", "2a")),
                    Ok(UiMessageChunk::text_delta("text-2", "2b")),
                ]);
            },
        )
        .expect("stream is created");

        assert_eq!(
            serde_json::to_value(chunks).expect("chunks serialize"),
            json!([
                { "type": "text-delta", "id": "text-1", "delta": "1a" },
                { "type": "text-delta", "id": "text-2", "delta": "2a" },
                { "type": "text-delta", "id": "text-2", "delta": "2b" },
                { "type": "error", "errorText": "masked stream-1-error" }
            ])
        );
    }

    #[test]
    fn process_ui_message_stream_accepts_abort_chunks() {
        let abort = UiMessageChunk::abort_with_reason(json!({ "source": "client" }));
        assert_eq!(
            serde_json::to_value(&abort).expect("abort chunk serializes"),
            json!({
                "type": "abort",
                "reason": { "source": "client" }
            })
        );
        assert_eq!(
            serde_json::from_value::<UiMessageChunk>(json!({ "type": "abort" }))
                .expect("abort chunk deserializes"),
            UiMessageChunk::abort()
        );

        let mut state = StreamingUiMessageState::new("msg-123", None);
        let messages = process_ui_message_stream(
            &mut state,
            [
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "partial"),
                abort,
            ],
            false,
        )
        .expect("abort chunks process");

        assert!(state.aborted);
        assert_eq!(state.abort_reason, Some(json!({ "source": "client" })));
        assert_eq!(
            serde_json::to_value(messages.last().expect("abort writes final state"))
                .expect("message serializes"),
            json!({
                "id": "msg-123",
                "role": "assistant",
                "parts": [
                    {
                        "type": "text",
                        "text": "partial",
                        "state": "done"
                    }
                ]
            })
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
    fn get_static_tool_name_should_return_the_tool_name_after_the_tool_prefix() {
        assert_eq!(
            get_static_tool_name(&json!({
                "type": "tool-getLocation",
                "toolCallId": "tool1",
                "state": "output-available",
                "input": {},
                "output": "some result"
            })),
            Some("getLocation")
        );
    }

    #[test]
    fn get_static_tool_name_should_return_the_tool_name_for_tools_that_contains_a_dash() {
        assert_eq!(
            get_static_tool_name(&json!({
                "type": "tool-get-location",
                "toolCallId": "tool1",
                "state": "output-available",
                "input": {},
                "output": "some result"
            })),
            Some("get-location")
        );
    }

    #[test]
    fn is_custom_content_ui_part_should_return_true_for_a_custom_part() {
        assert!(is_custom_content_ui_part(&json!({
            "type": "custom",
            "kind": "test-provider.compaction",
            "providerMetadata": {
                "openai": { "itemId": "cmp_123" }
            }
        })));
    }

    #[test]
    fn is_custom_content_ui_part_should_return_true_for_a_custom_part_without_provider_metadata() {
        assert!(is_custom_content_ui_part(&json!({
            "type": "custom",
            "kind": "openai.compaction"
        })));
    }

    #[test]
    fn is_custom_content_ui_part_should_return_false_for_a_text_part() {
        assert!(!is_custom_content_ui_part(&json!({
            "type": "text",
            "text": "some text"
        })));
    }

    #[test]
    fn is_data_ui_part_should_return_true_if_the_part_is_a_data_part() {
        assert!(is_data_ui_part(&json!({
            "type": "data-someDataPart",
            "data": "some data"
        })));
    }

    #[test]
    fn is_data_ui_part_should_return_false_if_the_part_is_not_a_data_part() {
        assert!(!is_data_ui_part(&json!({
            "type": "text",
            "text": "some text"
        })));
    }

    fn schema_named(name: &'static str) -> FlexibleSchema<JsonValue> {
        Schema::new(JsonObject::new())
            .with_validator(move |value| {
                let valid = match name {
                    "metadata" => value.get("foo").and_then(JsonValue::as_str).is_some(),
                    "string" => value.is_string(),
                    "number" => value.is_number(),
                    "input-location" => value.get("location").and_then(JsonValue::as_str).is_some(),
                    "output-weather" => value.get("weather").and_then(JsonValue::as_str).is_some(),
                    _ => true,
                };

                if valid {
                    ValidationResult::success(value.clone())
                } else {
                    ValidationResult::failure(format!("{name} schema mismatch"))
                }
            })
            .into()
    }

    fn validate_messages(messages: JsonValue) -> Result<Vec<JsonValue>, UiMessageValidationError> {
        validate_ui_messages(UiMessageValidationOptions {
            messages: Some(messages),
            ..Default::default()
        })
    }

    #[test]
    fn validate_ui_messages_should_throw_invalid_argument_error_when_messages_parameter_is_null() {
        let error = validate_ui_messages(UiMessageValidationOptions::default())
            .expect_err("missing messages should fail");

        assert!(matches!(
            error,
            UiMessageValidationError::InvalidArgument(_)
        ));
        assert_eq!(
            error.to_string(),
            "Invalid argument for parameter messages: messages parameter must be provided"
        );
    }

    #[test]
    fn validate_ui_messages_should_throw_type_validation_error_when_messages_array_is_empty() {
        let error = validate_messages(json!([])).expect_err("empty array should fail");

        assert!(matches!(error, UiMessageValidationError::TypeValidation(_)));
        assert!(
            error
                .to_string()
                .contains("Messages array must not be empty")
        );
    }

    #[test]
    fn validate_ui_messages_should_throw_type_validation_error_when_message_has_empty_parts_array()
    {
        let error = validate_messages(json!([
            {
                "id": "1",
                "role": "user",
                "parts": []
            }
        ]))
        .expect_err("empty parts should fail");

        assert!(
            error
                .to_string()
                .contains("Message must contain at least one part")
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_a_user_message_with_metadata_when_no_metadata_schema_is_provided()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "user",
                "metadata": { "foo": "bar" },
                "parts": [{ "type": "text", "text": "Hello, world!" }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_a_user_message_with_metadata() {
        let messages = json!([
            {
                "id": "1",
                "role": "user",
                "metadata": { "foo": "bar" },
                "parts": [{ "type": "text", "text": "Hello, world!" }]
            }
        ]);

        let result = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(messages.clone()),
            metadata_schema: Some(schema_named("metadata")),
            ..Default::default()
        })
        .unwrap();

        assert_eq!(result, messages.as_array().unwrap().clone());
    }

    #[test]
    fn validate_ui_messages_should_throw_type_validation_error_when_metadata_is_invalid() {
        let error = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "user",
                    "metadata": { "foo": 123 },
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                }
            ])),
            metadata_schema: Some(schema_named("metadata")),
            ..Default::default()
        })
        .expect_err("invalid metadata should fail");

        assert!(error.to_string().contains("messages[0].metadata"));
        assert!(error.to_string().contains("id: \"1\""));
    }

    #[test]
    fn validate_ui_messages_should_validate_text_part_with_provider_metadata() {
        let messages = json!([
            {
                "id": "1",
                "role": "user",
                "parts": [{
                    "type": "text",
                    "text": "Hello, world!",
                    "providerMetadata": {
                        "someProvider": { "custom": "metadata" }
                    }
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_a_user_message_with_a_text_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "user",
                "parts": [{ "type": "text", "text": "Hello, world!" }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_custom_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "custom",
                    "kind": "test-provider.compaction",
                    "providerMetadata": {
                        "openai": { "itemId": "cmp_123" }
                    }
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_custom_part_without_provider_metadata()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "custom",
                    "kind": "openai.compaction"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_reasoning_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "reasoning",
                    "text": "The answer needs weather data.",
                    "state": "done"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_source_url_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "source-url",
                    "sourceId": "source-1",
                    "url": "https://example.com",
                    "title": "Example"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_source_document_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "source-document",
                    "sourceId": "source-1",
                    "mediaType": "text/plain",
                    "title": "Example",
                    "filename": "example.txt"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_file_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "file",
                    "mediaType": "image/png",
                    "filename": "image.png",
                    "url": "data:image/png;base64,AA=="
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_step_start_part() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{ "type": "step-start" }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_two_data_parts() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [
                    { "type": "data-city", "id": "city-1", "data": "Brisbane" },
                    { "type": "data-temperature", "id": "temp-1", "data": 27 }
                ]
            }
        ]);
        let mut data_schemas = BTreeMap::new();
        data_schemas.insert("city".to_string(), schema_named("string"));
        data_schemas.insert("temperature".to_string(), schema_named("number"));

        let result = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(messages.clone()),
            data_schemas,
            ..Default::default()
        })
        .unwrap();

        assert_eq!(result, messages.as_array().unwrap().clone());
    }

    #[test]
    fn validate_ui_messages_should_throw_type_validation_error_when_data_is_invalid() {
        let mut data_schemas = BTreeMap::new();
        data_schemas.insert("city".to_string(), schema_named("string"));

        let error = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "assistant",
                    "parts": [{ "type": "data-city", "id": "city-1", "data": 123 }]
                }
            ])),
            data_schemas,
            ..Default::default()
        })
        .expect_err("invalid data should fail");

        assert!(error.to_string().contains("messages[0].parts[0].data"));
        assert!(error.to_string().contains("city"));
        assert!(error.to_string().contains("id: \"city-1\""));
    }

    #[test]
    fn validate_ui_messages_should_throw_type_validation_error_when_there_is_no_data_schema_for_a_data_part()
     {
        let mut data_schemas = BTreeMap::new();
        data_schemas.insert("other".to_string(), schema_named("string"));

        let error = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "assistant",
                    "parts": [{ "type": "data-city", "id": "city-1", "data": "Brisbane" }]
                }
            ])),
            data_schemas,
            ..Default::default()
        })
        .expect_err("missing data schema should fail");

        assert!(
            error
                .to_string()
                .contains("No data schema found for data part city")
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_dynamic_tool_part_in_output_error_state()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "toolCallId": "tool-1",
                    "state": "output-error",
                    "errorText": "failed"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_dynamic_tool_part_in_input_streaming_state()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "toolCallId": "tool-1",
                    "state": "input-streaming"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_dynamic_tool_part_in_input_available_state()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "toolCallId": "tool-1",
                    "state": "input-available",
                    "input": { "location": "Brisbane" }
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_an_assistant_message_with_a_dynamic_tool_part_in_output_available_state()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "toolCallId": "tool-1",
                    "state": "output-available",
                    "input": { "location": "Brisbane" },
                    "output": { "weather": "sunny" }
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_a_dynamic_tool_part_in_output_error_state_when_input_key_is_absent()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "toolCallId": "tool-1",
                    "state": "output-error",
                    "errorText": "failed"
                }]
            }
        ]);

        assert_eq!(
            validate_messages(messages.clone()).unwrap(),
            messages.as_array().unwrap().clone()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_tool_input_when_state_is_input_available() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "tool-weather",
                    "toolCallId": "tool-1",
                    "state": "input-available",
                    "input": { "location": "Brisbane" }
                }]
            }
        ]);
        let mut tools = BTreeMap::new();
        tools.insert(
            "weather".to_string(),
            UiMessageValidationTool::new(schema_named("input-location")),
        );

        let result = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(messages.clone()),
            tools,
            ..Default::default()
        })
        .unwrap();

        assert_eq!(result, messages.as_array().unwrap().clone());
    }

    #[test]
    fn validate_ui_messages_should_validate_tool_input_and_output_when_state_is_output_available() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "tool-weather",
                    "toolCallId": "tool-1",
                    "state": "output-available",
                    "input": { "location": "Brisbane" },
                    "output": { "weather": "sunny" }
                }]
            }
        ]);
        let mut tools = BTreeMap::new();
        tools.insert(
            "weather".to_string(),
            UiMessageValidationTool::new(schema_named("input-location"))
                .with_output_schema(schema_named("output-weather")),
        );

        let result = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(messages.clone()),
            tools,
            ..Default::default()
        })
        .unwrap();

        assert_eq!(result, messages.as_array().unwrap().clone());
    }

    #[test]
    fn validate_ui_messages_should_skip_tool_input_validation_when_state_is_output_error_and_there_is_no_input()
     {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "tool-weather",
                    "toolCallId": "tool-1",
                    "state": "output-error",
                    "errorText": "failed"
                }]
            }
        ]);
        let mut tools = BTreeMap::new();
        tools.insert(
            "weather".to_string(),
            UiMessageValidationTool::new(schema_named("input-location")),
        );

        assert!(
            validate_ui_messages(UiMessageValidationOptions {
                messages: Some(messages),
                tools,
                ..Default::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn validate_ui_messages_should_throw_error_when_no_tool_schema_is_found() {
        let mut tools = BTreeMap::new();
        tools.insert(
            "other".to_string(),
            UiMessageValidationTool::new(schema_named("input-location")),
        );

        let error = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "assistant",
                    "parts": [{
                        "type": "tool-weather",
                        "toolCallId": "tool-1",
                        "state": "input-available",
                        "input": { "location": "Brisbane" }
                    }]
                }
            ])),
            tools,
            ..Default::default()
        })
        .expect_err("missing tool schema should fail");

        assert!(
            error
                .to_string()
                .contains("No tool schema found for tool part weather")
        );
    }

    #[test]
    fn validate_ui_messages_should_skip_validation_for_tool_part_in_output_available_state_when_tool_schema_is_missing()
     {
        let mut tools = BTreeMap::new();
        tools.insert(
            "other".to_string(),
            UiMessageValidationTool::new(schema_named("input-location")),
        );

        assert!(
            validate_ui_messages(UiMessageValidationOptions {
                messages: Some(json!([
                    {
                        "id": "1",
                        "role": "assistant",
                        "parts": [{
                            "type": "tool-weather",
                            "toolCallId": "tool-1",
                            "state": "output-available",
                            "input": { "unexpected": true },
                            "output": { "anything": true }
                        }]
                    }
                ])),
                tools,
                ..Default::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn validate_ui_messages_should_validate_automatic_approval_reasons_on_output_parts() {
        let messages = json!([
            {
                "id": "1",
                "role": "assistant",
                "parts": [{
                    "type": "tool-weather",
                    "toolCallId": "tool-1",
                    "state": "output-available",
                    "input": { "location": "Brisbane" },
                    "output": { "weather": "sunny" },
                    "approval": {
                        "id": "approval-1",
                        "approved": true,
                        "reason": "automatic",
                        "isAutomatic": true
                    }
                }]
            }
        ]);

        assert!(validate_messages(messages).is_ok());
    }

    #[test]
    fn validate_ui_messages_should_throw_error_when_tool_input_validation_fails() {
        let mut tools = BTreeMap::new();
        tools.insert(
            "weather".to_string(),
            UiMessageValidationTool::new(schema_named("input-location")),
        );

        let error = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "assistant",
                    "parts": [{
                        "type": "tool-weather",
                        "toolCallId": "tool-1",
                        "state": "input-available",
                        "input": { "city": "Brisbane" }
                    }]
                }
            ])),
            tools,
            ..Default::default()
        })
        .expect_err("invalid tool input should fail");

        assert!(error.to_string().contains("messages[0].parts[0].input"));
        assert!(error.to_string().contains("weather"));
        assert!(error.to_string().contains("id: \"tool-1\""));
    }

    #[test]
    fn validate_ui_messages_should_throw_error_when_tool_output_validation_fails() {
        let mut tools = BTreeMap::new();
        tools.insert(
            "weather".to_string(),
            UiMessageValidationTool::new(schema_named("input-location"))
                .with_output_schema(schema_named("output-weather")),
        );

        let error = validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "assistant",
                    "parts": [{
                        "type": "tool-weather",
                        "toolCallId": "tool-1",
                        "state": "output-available",
                        "input": { "location": "Brisbane" },
                        "output": { "temperature": 27 }
                    }]
                }
            ])),
            tools,
            ..Default::default()
        })
        .expect_err("invalid tool output should fail");

        assert!(error.to_string().contains("messages[0].parts[0].output"));
        assert!(error.to_string().contains("weather"));
        assert!(error.to_string().contains("id: \"tool-1\""));
    }

    #[test]
    fn safe_validate_ui_messages_should_return_success_result_for_valid_messages() {
        let result = safe_validate_ui_messages(UiMessageValidationOptions {
            messages: Some(json!([
                {
                    "id": "1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                }
            ])),
            ..Default::default()
        });

        assert!(result.is_success());
    }

    #[test]
    fn safe_validate_ui_messages_should_return_failure_result_when_messages_parameter_is_null() {
        let result = safe_validate_ui_messages(UiMessageValidationOptions::default());

        assert!(matches!(
            result,
            SafeValidateUiMessagesResult::Failure {
                error: UiMessageValidationError::InvalidArgument(_)
            }
        ));
    }

    #[test]
    fn last_assistant_tool_calls_false_when_last_step_only_has_text() {
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
    }

    #[test]
    fn last_assistant_tool_calls_true_when_text_follows_last_tool_result() {
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
    }

    #[test]
    fn last_assistant_tool_calls_true_when_tool_has_output_error() {
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
                json!({ "type": "text", "text": "The current weather is windy.", "state": "done" }),
            ])
        ]));
    }

    #[test]
    fn last_assistant_tool_calls_true_when_dynamic_tool_is_complete() {
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
    }

    #[test]
    fn last_assistant_tool_calls_false_when_dynamic_tool_input_streaming() {
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
    }

    #[test]
    fn last_assistant_tool_calls_false_when_dynamic_tool_has_input_only() {
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
    }

    #[test]
    fn last_assistant_tool_calls_true_when_dynamic_tool_has_output_error() {
        assert!(last_assistant_message_is_complete_with_tool_calls(&[
            assistant_message(vec![
                step_start_json(),
                dynamic_tool_part_json(
                    "getDynamicWeather",
                    "call-dynamic",
                    "output-error",
                    json!({ "location": "San Francisco" }),
                    None,
                ),
            ])
        ]));
    }

    #[test]
    fn last_assistant_tool_calls_true_when_regular_and_dynamic_tools_complete() {
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
    }

    #[test]
    fn last_assistant_tool_calls_false_when_mixed_tools_include_incomplete() {
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
    }

    #[test]
    fn last_assistant_tool_calls_true_when_last_step_dynamic_tool_complete() {
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
    }

    #[test]
    fn last_assistant_tool_calls_false_when_last_step_dynamic_tool_incomplete() {
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
    }

    #[test]
    fn last_assistant_tool_calls_false_for_provider_executed_tool_only() {
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
    fn last_assistant_approval_responses_false_when_messages_empty() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_false_when_last_message_is_user() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[UiMessage::new("user-id", UiMessageRole::User)]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_false_when_last_step_has_no_tools() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                json!({ "type": "text", "text": "Hello", "state": "done" })
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_false_when_no_tool_approval_responded() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-requested", false),
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_false_when_any_tool_approval_requested() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-responded", false),
                approval_tool_part_json("tool-getWeather", "call-2", "approval-requested", false),
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_true_when_non_provider_tool_approval_responded() {
        assert!(last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("tool-getWeather", "call-1", "approval-responded", false),
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_true_when_provider_tool_approval_responded() {
        assert!(last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("dynamic-tool", "call-1", "approval-responded", true),
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_true_when_terminal_tools_include_approval_response() {
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
    }

    #[test]
    fn last_assistant_approval_responses_true_when_provider_approval_and_regular_output() {
        assert!(last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("dynamic-tool", "call-1", "approval-responded", true),
                tool_part_json(
                    "tool-getWeather",
                    "call-2",
                    "output-available",
                    json!({ "city": "Tokyo" }),
                    Some(json!({ "temperature": 25, "weather": "sunny" })),
                ),
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_false_when_regular_tool_still_needs_approval() {
        assert!(!last_assistant_message_is_complete_with_approval_responses(
            &[assistant_message(vec![
                step_start_json(),
                approval_tool_part_json("dynamic-tool", "call-1", "approval-responded", true),
                approval_tool_part_json("tool-getWeather", "call-2", "approval-requested", false),
            ])]
        ));
    }

    #[test]
    fn last_assistant_approval_responses_false_when_only_prior_step_has_approval() {
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
