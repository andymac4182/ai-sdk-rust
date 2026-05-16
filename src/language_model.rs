use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::file_data::{FileData, FileDataContent};
use crate::json::{JsonObject, JsonSchema, JsonValue, NonNullJsonValue};
use crate::provider::{ProviderMetadata, ProviderOptions};

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
        FinishReason, InputTokenUsage, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelContent, LanguageModelCustomContent,
        LanguageModelCustomPart, LanguageModelFile, LanguageModelFileData, LanguageModelFilePart,
        LanguageModelFinishReason, LanguageModelFunctionTool, LanguageModelMessage,
        LanguageModelPrompt, LanguageModelProviderTool, LanguageModelReasoning,
        LanguageModelReasoningFile, LanguageModelReasoningPart, LanguageModelResponseMetadata,
        LanguageModelSource, LanguageModelSystemMessage, LanguageModelText, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolApprovalRequest, LanguageModelToolApprovalResponsePart,
        LanguageModelToolCall, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResult,
        LanguageModelToolResultContentPart, LanguageModelToolResultCustomContent,
        LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUrlSource,
        LanguageModelUsage, LanguageModelUserContentPart, LanguageModelUserMessage,
        OutputTokenUsage,
    };
    use crate::file_data::{FileData, FileDataContent};
    use crate::json::NonNullJsonValue;
    use crate::provider::ProviderOptions;
    use serde_json::json;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    use url::Url;

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
    fn assistant_message_deserializes_reasoning_custom_and_tool_call_parts() {
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
            ]))
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
