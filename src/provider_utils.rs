use std::collections::BTreeMap;
use std::env::{self, VarError};
use std::fmt;
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
};
use crate::headers::Headers;
use crate::image_model::ImageModelFile;
use crate::json::{JsonObject, JsonSchema, JsonValue};
use crate::language_model::{
    LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage, LanguageModelPrompt,
    LanguageModelReasoningEffort, LanguageModelSupportedUrls, LanguageModelSystemMessage,
    LanguageModelTool, LanguageModelToolInputExample,
};
use crate::provider::{
    ApiCallError, InvalidArgumentError, JsonParseError, LoadApiKeyError, LoadSettingError,
    ProviderOptions, TypeValidationContext, TypeValidationError, UnsupportedFunctionalityError,
};
use crate::warning::Warning;

const DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX: &str = "JSON schema:";
const DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX: &str =
    "You MUST answer with a JSON object that matches the JSON schema above.";
const DEFAULT_JSON_INSTRUCTION_SUFFIX: &str = "You MUST answer with JSON.";

/// Default maximum response download size used by upstream provider-utils: 2 GiB.
pub const DEFAULT_MAX_DOWNLOAD_SIZE: usize = 2 * 1024 * 1024 * 1024;

/// Error returned when inline file data cannot be converted to raw bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineFileDataBytesError {
    /// The supplied file data is a URL or provider reference rather than inline content.
    NonInlineFileData,

    /// The supplied inline data is not valid base64.
    InvalidBase64Data,
}

impl fmt::Display for InlineFileDataBytesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonInlineFileData => formatter.write_str("file data must be inline data or text"),
            Self::InvalidBase64Data => formatter.write_str("invalid base64 inline file data"),
        }
    }
}

impl std::error::Error for InlineFileDataBytesError {}

/// Error returned when base64 data cannot be decoded into bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Base64DecodeError;

impl fmt::Display for Base64DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid base64 data")
    }
}

impl std::error::Error for Base64DecodeError {}

/// Error returned when a URL is unsafe or failed to download.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadError {
    url: String,
    status_code: Option<u16>,
    status_text: Option<String>,
    message: String,
}

impl DownloadError {
    /// Creates a download error with a caller-supplied message.
    pub fn new(url: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            status_code: None,
            status_text: None,
            message: message.into(),
        }
    }

    /// Creates a download error from an HTTP response status.
    pub fn with_status(
        url: impl Into<String>,
        status_code: u16,
        status_text: impl Into<String>,
    ) -> Self {
        let url = url.into();
        let status_text = status_text.into();
        Self {
            message: format!("Failed to download {url}: {status_code} {status_text}"),
            url,
            status_code: Some(status_code),
            status_text: Some(status_text),
        }
    }

    /// Creates a download error from a lower-level failure message.
    pub fn with_cause_message(url: impl Into<String>, cause_message: impl fmt::Display) -> Self {
        let url = url.into();
        Self {
            message: format!("Failed to download {url}: {cause_message}"),
            url,
            status_code: None,
            status_text: None,
        }
    }

    /// Returns the URL that failed validation or download.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the response status code when one was available.
    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    /// Returns the response status text when one was available.
    pub fn status_text(&self) -> Option<&str> {
        self.status_text.as_deref()
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its URL.
    pub fn into_url(self) -> String {
        self.url
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DownloadError {}

/// Result returned by safe type validation.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidateTypesResult<T = JsonValue> {
    /// Type validation succeeded.
    Success {
        /// Validated or transformed value.
        value: T,

        /// Raw JSON value before validation.
        raw_value: JsonValue,
    },

    /// Type validation failed without panicking.
    Failure {
        /// Wrapped type-validation error.
        error: TypeValidationError,

        /// Raw JSON value that failed validation.
        raw_value: JsonValue,
    },
}

impl<T> ValidateTypesResult<T> {
    /// Creates a successful type-validation result.
    pub fn success(value: T, raw_value: JsonValue) -> Self {
        Self::Success { value, raw_value }
    }

    /// Creates a failed type-validation result.
    pub fn failure(error: TypeValidationError, raw_value: JsonValue) -> Self {
        Self::Failure { error, raw_value }
    }

    /// Returns whether type validation succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns whether type validation failed.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }

    /// Returns the validated or transformed value on success.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Success { value, .. } => Some(value),
            Self::Failure { .. } => None,
        }
    }

    /// Returns the raw JSON value before validation.
    pub fn raw_value(&self) -> &JsonValue {
        match self {
            Self::Success { raw_value, .. } | Self::Failure { raw_value, .. } => raw_value,
        }
    }

    /// Returns the type-validation error on failure.
    pub fn error(&self) -> Option<&TypeValidationError> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error, .. } => Some(error),
        }
    }
}

/// Error returned by safe JSON parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum ParseJsonError {
    /// JSON text could not be parsed or failed secure JSON parsing.
    JsonParse(JsonParseError),

    /// Parsed JSON failed schema/type validation.
    TypeValidation(TypeValidationError),
}

impl ParseJsonError {
    /// Returns the JSON parse error when this is a parse failure.
    pub fn as_json_parse_error(&self) -> Option<&JsonParseError> {
        match self {
            Self::JsonParse(error) => Some(error),
            Self::TypeValidation(_) => None,
        }
    }

    /// Returns the type validation error when this is a validation failure.
    pub fn as_type_validation_error(&self) -> Option<&TypeValidationError> {
        match self {
            Self::JsonParse(_) => None,
            Self::TypeValidation(error) => Some(error),
        }
    }
}

impl From<JsonParseError> for ParseJsonError {
    fn from(error: JsonParseError) -> Self {
        Self::JsonParse(error)
    }
}

impl From<TypeValidationError> for ParseJsonError {
    fn from(error: TypeValidationError) -> Self {
        Self::TypeValidation(error)
    }
}

impl fmt::Display for ParseJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JsonParse(error) => error.fmt(formatter),
            Self::TypeValidation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParseJsonError {}

/// Result returned by safe JSON parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum ParseJsonResult<T = JsonValue> {
    /// Parsing and optional validation succeeded.
    Success {
        /// Parsed or validated value.
        value: T,

        /// Raw JSON value before optional schema/type validation.
        raw_value: JsonValue,
    },

    /// Parsing or optional validation failed without panicking.
    Failure {
        /// Parse or validation error.
        error: ParseJsonError,

        /// Raw JSON value before validation, when parsing succeeded.
        raw_value: Option<JsonValue>,
    },
}

impl<T> ParseJsonResult<T> {
    /// Creates a successful parse result.
    pub fn success(value: T, raw_value: JsonValue) -> Self {
        Self::Success { value, raw_value }
    }

    /// Creates a failed parse result.
    pub fn failure(error: impl Into<ParseJsonError>, raw_value: Option<JsonValue>) -> Self {
        Self::Failure {
            error: error.into(),
            raw_value,
        }
    }

    /// Returns whether parsing and optional validation succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns whether parsing or optional validation failed.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }

    /// Returns the parsed or validated value on success.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Success { value, .. } => Some(value),
            Self::Failure { .. } => None,
        }
    }

    /// Returns the raw parsed JSON value when one is available.
    pub fn raw_value(&self) -> Option<&JsonValue> {
        match self {
            Self::Success { raw_value, .. } => Some(raw_value),
            Self::Failure { raw_value, .. } => raw_value.as_ref(),
        }
    }

    /// Returns the parse or validation error on failure.
    pub fn error(&self) -> Option<&ParseJsonError> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error, .. } => Some(error),
        }
    }
}

/// Result returned by provider response handlers.
///
/// This mirrors upstream `@ai-sdk/provider-utils` response handlers: every
/// handler returns a parsed value and may include raw JSON data plus extracted
/// response headers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHandlerResult<T = JsonValue> {
    /// Parsed or constructed response value.
    pub value: T,

    /// Raw JSON value before optional validation, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_value: Option<JsonValue>,

    /// Headers extracted from the HTTP response, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<Headers>,
}

impl<T> ResponseHandlerResult<T> {
    /// Creates a response-handler result with a parsed value.
    pub fn new(value: T) -> Self {
        Self {
            value,
            raw_value: None,
            response_headers: None,
        }
    }

    /// Adds the raw JSON value before validation.
    pub fn with_raw_value(mut self, raw_value: impl Into<JsonValue>) -> Self {
        self.raw_value = Some(raw_value.into());
        self
    }

    /// Adds headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = Some(response_headers);
        self
    }

    /// Returns the parsed or constructed response value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Returns the raw JSON value before validation, when available.
    pub fn raw_value(&self) -> Option<&JsonValue> {
        self.raw_value.as_ref()
    }

    /// Returns the extracted response headers, when available.
    pub fn response_headers(&self) -> Option<&Headers> {
        self.response_headers.as_ref()
    }

    /// Converts this result into the parsed or constructed response value.
    pub fn into_value(self) -> T {
        self.value
    }
}

/// Inputs for the status-code error response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createStatusCodeErrorResponseHandler`, avoiding a concrete HTTP client
/// dependency while preserving the API-call error shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCodeErrorResponseHandlerOptions {
    /// URL that produced the status-code error response.
    pub url: String,

    /// Request body values associated with the failed provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// HTTP status text from the response.
    pub status_text: String,

    /// Headers extracted from the response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw response body text.
    pub response_body: String,
}

impl StatusCodeErrorResponseHandlerOptions {
    /// Creates status-code error response handler options.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        status_text: impl Into<String>,
        response_body: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            status_text: status_text.into(),
            response_headers: Headers::new(),
            response_body: response_body.into(),
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the JSON response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createJsonResponseHandler`, keeping response parsing independent from any
/// concrete HTTP client while preserving API-call error context.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonResponseHandlerOptions {
    /// URL that produced the response.
    pub url: String,

    /// Request body values associated with the provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw response body text.
    pub response_body: String,
}

impl JsonResponseHandlerOptions {
    /// Creates JSON response handler options.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        response_body: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            response_headers: Headers::new(),
            response_body: response_body.into(),
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the JSON error response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createJsonErrorResponseHandler`, preserving resilient JSON error parsing
/// without introducing a concrete HTTP client dependency.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonErrorResponseHandlerOptions {
    /// URL that produced the error response.
    pub url: String,

    /// Request body values associated with the failed provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// HTTP status text from the response.
    pub status_text: String,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw response body text.
    pub response_body: String,
}

impl JsonErrorResponseHandlerOptions {
    /// Creates JSON error response handler options.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        status_text: impl Into<String>,
        response_body: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            status_text: status_text.into(),
            response_headers: Headers::new(),
            response_body: response_body.into(),
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the binary response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createBinaryResponseHandler`, keeping response body reading independent
/// from any concrete HTTP client while preserving API-call error context.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryResponseHandlerOptions {
    /// URL that produced the response.
    pub url: String,

    /// Request body values associated with the provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw binary response body bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<Vec<u8>>,
}

impl BinaryResponseHandlerOptions {
    /// Creates binary response handler options with a readable response body.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        response_body: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            response_headers: Headers::new(),
            response_body: Some(response_body.into()),
        }
    }

    /// Creates binary response handler options without a response body.
    pub fn empty(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            response_headers: Headers::new(),
            response_body: None,
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

struct MediaTypeSignature {
    media_type: &'static str,
    bytes_prefix: &'static [Option<u8>],
}

const IMAGE_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "image/gif",
        bytes_prefix: &[Some(0x47), Some(0x49), Some(0x46)],
    },
    MediaTypeSignature {
        media_type: "image/png",
        bytes_prefix: &[Some(0x89), Some(0x50), Some(0x4e), Some(0x47)],
    },
    MediaTypeSignature {
        media_type: "image/jpeg",
        bytes_prefix: &[Some(0xff), Some(0xd8)],
    },
    MediaTypeSignature {
        media_type: "image/webp",
        bytes_prefix: &[
            Some(0x52),
            Some(0x49),
            Some(0x46),
            Some(0x46),
            None,
            None,
            None,
            None,
            Some(0x57),
            Some(0x45),
            Some(0x42),
            Some(0x50),
        ],
    },
    MediaTypeSignature {
        media_type: "image/bmp",
        bytes_prefix: &[Some(0x42), Some(0x4d)],
    },
    MediaTypeSignature {
        media_type: "image/tiff",
        bytes_prefix: &[Some(0x49), Some(0x49), Some(0x2a), Some(0x00)],
    },
    MediaTypeSignature {
        media_type: "image/tiff",
        bytes_prefix: &[Some(0x4d), Some(0x4d), Some(0x00), Some(0x2a)],
    },
    MediaTypeSignature {
        media_type: "image/avif",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x20),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x61),
            Some(0x76),
            Some(0x69),
            Some(0x66),
        ],
    },
    MediaTypeSignature {
        media_type: "image/heic",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x20),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x68),
            Some(0x65),
            Some(0x69),
            Some(0x63),
        ],
    },
];

const DOCUMENT_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[MediaTypeSignature {
    media_type: "application/pdf",
    bytes_prefix: &[Some(0x25), Some(0x50), Some(0x44), Some(0x46)],
}];

const AUDIO_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xfb)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xfa)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xf3)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xf2)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xe3)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xe2)],
    },
    MediaTypeSignature {
        media_type: "audio/wav",
        bytes_prefix: &[
            Some(0x52),
            Some(0x49),
            Some(0x46),
            Some(0x46),
            None,
            None,
            None,
            None,
            Some(0x57),
            Some(0x41),
            Some(0x56),
            Some(0x45),
        ],
    },
    MediaTypeSignature {
        media_type: "audio/ogg",
        bytes_prefix: &[Some(0x4f), Some(0x67), Some(0x67), Some(0x53)],
    },
    MediaTypeSignature {
        media_type: "audio/flac",
        bytes_prefix: &[Some(0x66), Some(0x4c), Some(0x61), Some(0x43)],
    },
    MediaTypeSignature {
        media_type: "audio/aac",
        bytes_prefix: &[Some(0x40), Some(0x15), Some(0x00), Some(0x00)],
    },
    MediaTypeSignature {
        media_type: "audio/mp4",
        bytes_prefix: &[Some(0x66), Some(0x74), Some(0x79), Some(0x70)],
    },
    MediaTypeSignature {
        media_type: "audio/webm",
        bytes_prefix: &[Some(0x1a), Some(0x45), Some(0xdf), Some(0xa3)],
    },
];

const VIDEO_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "video/mp4",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            None,
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
        ],
    },
    MediaTypeSignature {
        media_type: "video/webm",
        bytes_prefix: &[Some(0x1a), Some(0x45), Some(0xdf), Some(0xa3)],
    },
    MediaTypeSignature {
        media_type: "video/quicktime",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x14),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x71),
            Some(0x74),
        ],
    },
    MediaTypeSignature {
        media_type: "video/x-msvideo",
        bytes_prefix: &[Some(0x52), Some(0x49), Some(0x46), Some(0x46)],
    },
];

/// Future returned by a Rust tool execution function.
pub type ToolExecuteFuture =
    Pin<Box<dyn Future<Output = Result<JsonValue, ToolExecutionError>> + Send>>;

/// Function used to execute a Rust tool call.
pub type ToolExecuteFunction =
    dyn Fn(JsonValue, ToolExecutionOptions) -> ToolExecuteFuture + Send + Sync + 'static;

/// Options passed to a tool execution function.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionOptions {
    /// Identifier of the model tool call being executed.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,
}

impl ToolExecutionOptions {
    /// Creates tool execution options.
    pub fn new(tool_call_id: impl Into<String>, messages: LanguageModelPrompt) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
        }
    }
}

/// Error returned by a Rust tool execution function.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionError {
    /// Human-readable execution failure message.
    pub message: String,
}

impl ToolExecutionError {
    /// Creates a tool execution error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the execution failure message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for ToolExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolExecutionError {}

impl From<String> for ToolExecutionError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for ToolExecutionError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

/// User-defined Rust function tool made available to a language model call.
///
/// This mirrors the function-tool branch of upstream `@ai-sdk/provider-utils`
/// `Tool`: it carries model-facing schema/description metadata and may include
/// an executor for later client-side tool handling.
#[derive(Clone)]
pub struct Tool {
    /// Name of the tool, unique within a model call.
    pub name: String,

    /// Optional description of what the tool does.
    pub description: Option<String>,

    /// JSON Schema 7 object describing the tool input.
    pub input_schema: JsonSchema,

    /// Optional examples that show the model what inputs should look like.
    pub input_examples: Option<Vec<LanguageModelToolInputExample>>,

    /// Strict mode setting for providers that support it.
    pub strict: Option<bool>,

    /// Provider-specific options sent with the tool definition.
    pub provider_options: Option<ProviderOptions>,

    execute: Option<Arc<ToolExecuteFunction>>,
}

impl Tool {
    /// Creates a function tool definition.
    pub fn new(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            name: name.into(),
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
            execute: None,
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

    /// Sets the Rust executor for this tool.
    pub fn with_execute<F, Fut>(mut self, execute: F) -> Self
    where
        F: Fn(JsonValue, ToolExecutionOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, ToolExecutionError>> + Send + 'static,
    {
        self.execute = Some(Arc::new(move |input, options| {
            Box::pin(execute(input, options))
        }));
        self
    }

    /// Returns whether this tool has an executor.
    pub fn is_executable(&self) -> bool {
        self.execute.is_some()
    }

    /// Executes this tool when an executor is present.
    pub fn execute(
        &self,
        input: JsonValue,
        options: ToolExecutionOptions,
    ) -> Option<ToolExecuteFuture> {
        self.execute.as_ref().map(|execute| execute(input, options))
    }

    /// Converts this high-level tool into the provider-facing language-model tool shape.
    pub fn to_language_model_tool(&self) -> LanguageModelTool {
        let mut tool = LanguageModelFunctionTool::new(self.name.clone(), self.input_schema.clone());

        if let Some(description) = &self.description {
            tool = tool.with_description(description.clone());
        }

        if let Some(input_examples) = &self.input_examples {
            for input_example in input_examples {
                tool = tool.with_input_example(input_example.input.clone());
            }
        }

        if let Some(strict) = self.strict {
            tool = tool.with_strict(strict);
        }

        if let Some(provider_options) = &self.provider_options {
            tool = tool.with_provider_options(provider_options.clone());
        }

        LanguageModelTool::Function(tool)
    }
}

impl fmt::Debug for Tool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .field("input_examples", &self.input_examples)
            .field("strict", &self.strict)
            .field("provider_options", &self.provider_options)
            .field("is_executable", &self.is_executable())
            .finish()
    }
}

/// Bidirectional mapping between caller-facing and provider-facing tool names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolNameMapping {
    custom_tool_name_to_provider_tool_name: BTreeMap<String, String>,
    provider_tool_name_to_custom_tool_name: BTreeMap<String, String>,
}

impl ToolNameMapping {
    /// Maps a caller-facing tool name to the provider-facing name.
    ///
    /// Names without a mapping are returned unchanged.
    pub fn to_provider_tool_name(&self, custom_tool_name: &str) -> String {
        self.custom_tool_name_to_provider_tool_name
            .get(custom_tool_name)
            .cloned()
            .unwrap_or_else(|| custom_tool_name.to_string())
    }

    /// Maps a provider-facing tool name to the caller-facing name.
    ///
    /// Names without a mapping are returned unchanged.
    pub fn to_custom_tool_name(&self, provider_tool_name: &str) -> String {
        self.provider_tool_name_to_custom_tool_name
            .get(provider_tool_name)
            .cloned()
            .unwrap_or_else(|| provider_tool_name.to_string())
    }
}

/// Creates provider-defined tool name mappings from model tools.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `createToolNameMapping`:
/// only provider-defined tools whose ids are present in `provider_tool_names`
/// produce mappings; function tools and unknown provider tool ids pass through
/// unchanged.
pub fn create_tool_name_mapping<'a>(
    tools: impl IntoIterator<Item = &'a LanguageModelTool>,
    provider_tool_names: &BTreeMap<String, String>,
) -> ToolNameMapping {
    let mut mapping = ToolNameMapping::default();

    for tool in tools {
        let LanguageModelTool::Provider(tool) = tool else {
            continue;
        };

        if let Some(provider_tool_name) = provider_tool_names.get(&tool.id) {
            mapping
                .custom_tool_name_to_provider_tool_name
                .insert(tool.name.clone(), provider_tool_name.clone());
            mapping
                .provider_tool_name_to_custom_tool_name
                .insert(provider_tool_name.clone(), tool.name.clone());
        }
    }

    mapping
}

/// Converts high-level Rust tools into provider-facing language-model tools.
pub fn prepare_tools<'a>(
    tools: impl IntoIterator<Item = &'a Tool>,
) -> Option<Vec<LanguageModelTool>> {
    let tools = tools
        .into_iter()
        .map(Tool::to_language_model_tool)
        .collect::<Vec<_>>();

    if tools.is_empty() { None } else { Some(tools) }
}

/// Options for injecting JSON response instructions into a standardized prompt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InjectJsonInstructionIntoMessagesOptions {
    /// Standardized prompt messages to update.
    pub messages: LanguageModelPrompt,

    /// JSON schema to include in the system instruction.
    pub schema: Option<JsonSchema>,

    /// Custom prefix to place before the serialized JSON schema.
    pub schema_prefix: Option<String>,

    /// Custom suffix to place after the serialized JSON schema or generic JSON instruction.
    pub schema_suffix: Option<String>,
}

impl InjectJsonInstructionIntoMessagesOptions {
    /// Creates JSON instruction injection options for a standardized prompt.
    pub fn new(messages: LanguageModelPrompt) -> Self {
        Self {
            messages,
            schema: None,
            schema_prefix: None,
            schema_suffix: None,
        }
    }

    /// Sets the JSON schema included in the system instruction.
    pub fn with_schema(mut self, schema: JsonSchema) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Sets the prefix placed before the serialized JSON schema.
    pub fn with_schema_prefix(mut self, schema_prefix: impl Into<String>) -> Self {
        self.schema_prefix = Some(schema_prefix.into());
        self
    }

    /// Sets the suffix placed after the schema or generic JSON instruction.
    pub fn with_schema_suffix(mut self, schema_suffix: impl Into<String>) -> Self {
        self.schema_suffix = Some(schema_suffix.into());
        self
    }
}

/// Injects JSON response instructions into the leading system prompt message.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `injectJsonInstructionIntoMessages`: the first system message is updated
/// when present, otherwise a new system message is inserted before the original
/// prompt, and all non-system messages are preserved in order.
pub fn inject_json_instruction_into_messages(
    options: InjectJsonInstructionIntoMessagesOptions,
) -> LanguageModelPrompt {
    let InjectJsonInstructionIntoMessagesOptions {
        messages,
        schema,
        schema_prefix,
        schema_suffix,
    } = options;

    let mut messages = messages.into_iter();
    let first_message = messages.next();
    let mut remaining_messages = Vec::new();

    let mut system_message = match first_message {
        Some(LanguageModelMessage::System(system_message)) => system_message,
        Some(message) => {
            remaining_messages.push(message);
            LanguageModelSystemMessage::new("")
        }
        None => LanguageModelSystemMessage::new(""),
    };

    remaining_messages.extend(messages);
    system_message.content = inject_json_instruction(
        Some(&system_message.content),
        schema.as_ref(),
        schema_prefix.as_deref(),
        schema_suffix.as_deref(),
    );

    let mut updated_messages = Vec::with_capacity(remaining_messages.len() + 1);
    updated_messages.push(LanguageModelMessage::System(system_message));
    updated_messages.extend(remaining_messages);
    updated_messages
}

fn inject_json_instruction(
    prompt: Option<&str>,
    schema: Option<&JsonSchema>,
    schema_prefix: Option<&str>,
    schema_suffix: Option<&str>,
) -> String {
    let mut lines = Vec::new();

    if let Some(prompt) = prompt.filter(|prompt| !prompt.is_empty()) {
        lines.push(prompt.to_string());
        lines.push(String::new());
    }

    let schema_prefix = schema_prefix.or(schema.map(|_| DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX));
    if let Some(schema_prefix) = schema_prefix {
        lines.push(schema_prefix.to_string());
    }

    if let Some(schema) = schema {
        lines.push(serde_json::to_string(schema).expect("JSON schemas serialize"));
    }

    let schema_suffix = schema_suffix.or_else(|| {
        Some(if schema.is_some() {
            DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX
        } else {
            DEFAULT_JSON_INSTRUCTION_SUFFIX
        })
    });
    if let Some(schema_suffix) = schema_suffix {
        lines.push(schema_suffix.to_string());
    }

    lines.join("\n")
}

/// Adds `additionalProperties: false` to object JSON schemas recursively.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `addAdditionalPropertiesToJsonSchema`: object schemas, including union
/// schemas whose `type` includes `"object"`, are made closed recursively across
/// properties, items, composition lists, and definitions.
pub fn add_additional_properties_to_json_schema(mut json_schema: JsonSchema) -> JsonSchema {
    add_additional_properties_to_json_schema_object(&mut json_schema);
    json_schema
}

fn add_additional_properties_to_json_schema_object(json_schema: &mut JsonSchema) {
    if is_object_json_schema(json_schema) {
        json_schema.insert("additionalProperties".to_string(), JsonValue::Bool(false));

        if let Some(JsonValue::Object(properties)) = json_schema.get_mut("properties") {
            for property in properties.values_mut() {
                visit_json_schema_definition(property);
            }
        }
    }

    if let Some(items) = json_schema.get_mut("items") {
        visit_json_schema_definition_or_array(items);
    }

    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(JsonValue::Array(definitions)) = json_schema.get_mut(key) {
            for definition in definitions {
                visit_json_schema_definition(definition);
            }
        }
    }

    if let Some(JsonValue::Object(definitions)) = json_schema.get_mut("definitions") {
        for definition in definitions.values_mut() {
            visit_json_schema_definition(definition);
        }
    }
}

fn visit_json_schema_definition_or_array(definition: &mut JsonValue) {
    match definition {
        JsonValue::Array(definitions) => {
            for definition in definitions {
                visit_json_schema_definition(definition);
            }
        }
        _ => visit_json_schema_definition(definition),
    }
}

fn visit_json_schema_definition(definition: &mut JsonValue) {
    if let JsonValue::Object(json_schema) = definition {
        add_additional_properties_to_json_schema_object(json_schema);
    }
}

fn is_object_json_schema(json_schema: &JsonSchema) -> bool {
    match json_schema.get("type") {
        Some(JsonValue::String(schema_type)) => schema_type == "object",
        Some(JsonValue::Array(schema_types)) => schema_types
            .iter()
            .any(|schema_type| schema_type.as_str() == Some("object")),
        _ => false,
    }
}

/// Top-level reasoning effort levels that can be mapped to provider-specific settings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningLevel {
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

impl ReasoningLevel {
    /// Returns the upstream provider-v4 string for this reasoning level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

impl TryFrom<LanguageModelReasoningEffort> for ReasoningLevel {
    type Error = LanguageModelReasoningEffort;

    fn try_from(value: LanguageModelReasoningEffort) -> Result<Self, Self::Error> {
        match value {
            LanguageModelReasoningEffort::Minimal => Ok(Self::Minimal),
            LanguageModelReasoningEffort::Low => Ok(Self::Low),
            LanguageModelReasoningEffort::Medium => Ok(Self::Medium),
            LanguageModelReasoningEffort::High => Ok(Self::High),
            LanguageModelReasoningEffort::Xhigh => Ok(Self::Xhigh),
            value => Err(value),
        }
    }
}

/// Returns whether a reasoning request should override the provider default.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isCustomReasoning`: missing
/// reasoning and `provider-default` are not custom, while `none` and all effort
/// levels are custom reasoning settings.
pub fn is_custom_reasoning(reasoning: Option<&LanguageModelReasoningEffort>) -> bool {
    !matches!(
        reasoning,
        None | Some(LanguageModelReasoningEffort::ProviderDefault)
    )
}

/// Maps a top-level reasoning effort level to a provider-specific effort value.
///
/// This mirrors upstream `mapReasoningToProviderEffort`: unsupported levels add
/// an unsupported warning, and renamed levels add a compatibility warning.
pub fn map_reasoning_to_provider_effort<T>(
    reasoning: ReasoningLevel,
    effort_map: &BTreeMap<ReasoningLevel, T>,
    warnings: &mut Vec<Warning>,
) -> Option<T>
where
    T: AsRef<str> + Clone,
{
    let Some(mapped) = effort_map.get(&reasoning) else {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            )),
        });
        return None;
    };

    if mapped.as_ref() != reasoning.as_str() {
        warnings.push(Warning::Compatibility {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not directly supported by this model. mapped to effort \"{}\".",
                reasoning.as_str(),
                mapped.as_ref()
            )),
        });
    }

    Some(mapped.clone())
}

/// Maps a top-level reasoning effort level to a provider-specific token budget.
///
/// The budget is the rounded product of max output tokens and the configured
/// percentage, clamped between the minimum and maximum reasoning budgets.
pub fn map_reasoning_to_provider_budget(
    reasoning: ReasoningLevel,
    max_output_tokens: u64,
    max_reasoning_budget: u64,
    min_reasoning_budget: Option<u64>,
    budget_percentages: Option<&BTreeMap<ReasoningLevel, f64>>,
    warnings: &mut Vec<Warning>,
) -> Option<u64> {
    let percentage = match budget_percentages {
        Some(percentages) => percentages.get(&reasoning).copied(),
        None => Some(default_reasoning_budget_percentage(reasoning)),
    };

    let Some(percentage) = percentage else {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            )),
        });
        return None;
    };

    let requested_budget = ((max_output_tokens as f64) * percentage).round() as u64;

    Some(
        requested_budget
            .max(min_reasoning_budget.unwrap_or(1024))
            .min(max_reasoning_budget),
    )
}

fn default_reasoning_budget_percentage(reasoning: ReasoningLevel) -> f64 {
    match reasoning {
        ReasoningLevel::Minimal => 0.02,
        ReasoningLevel::Low => 0.1,
        ReasoningLevel::Medium => 0.3,
        ReasoningLevel::High => 0.6,
        ReasoningLevel::Xhigh => 0.9,
    }
}

/// A value that can be supplied as either one item or an array of items.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Arrayable<T> {
    /// A single item.
    Single(T),

    /// Multiple items.
    Array(Vec<T>),
}

impl<T> Arrayable<T> {
    /// Creates an arrayable single value.
    pub fn single(value: T) -> Self {
        Self::Single(value)
    }

    /// Creates an arrayable array value.
    pub fn array(values: Vec<T>) -> Self {
        Self::Array(values)
    }

    /// Converts the value into an array.
    pub fn into_vec(self) -> Vec<T> {
        match self {
            Self::Single(value) => vec![value],
            Self::Array(values) => values,
        }
    }
}

/// Normalizes a missing, single, or array value into an array.
pub fn as_array<T>(value: Option<Arrayable<T>>) -> Vec<T> {
    value.map_or_else(Vec::new, Arrayable::into_vec)
}

/// Checks whether an optional value is present.
pub fn is_non_nullable<T>(value: &Option<T>) -> bool {
    value.is_some()
}

/// Filters missing values out of a list of optional values.
pub fn filter_nullable<T>(values: impl IntoIterator<Item = Option<T>>) -> Vec<T> {
    values.into_iter().flatten().collect()
}

/// Removes entries whose values are missing.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `removeUndefinedEntries`:
/// values that are nullish in JavaScript are omitted from the returned record,
/// while present falsy-equivalent values are preserved.
pub fn remove_undefined_entries<K, T, I>(record: I) -> BTreeMap<String, T>
where
    I: IntoIterator<Item = (K, Option<T>)>,
    K: Into<String>,
{
    record
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.into(), value)))
        .collect()
}

/// Checks whether a JSON value has the provider-reference record shape.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isProviderReference` at the
/// JSON boundary: plain objects without a `type` discriminator are treated as
/// provider references, while tagged file-data objects and non-objects are not.
pub fn is_provider_reference(data: &JsonValue) -> bool {
    data.as_object()
        .is_some_and(|object| !object.contains_key("type"))
}

/// Validates a JSON value with a caller-supplied type validator.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `validateTypes`: validation
/// failures are wrapped in the provider-level [`TypeValidationError`] with the
/// original JSON value and optional validation context.
pub fn validate_types<T, F, E>(
    value: JsonValue,
    validate: F,
    context: Option<TypeValidationContext>,
) -> Result<T, TypeValidationError>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    match safe_validate_types(value, validate, context) {
        ValidateTypesResult::Success { value, .. } => Ok(value),
        ValidateTypesResult::Failure { error, .. } => Err(error),
    }
}

/// Safely validates a JSON value with a caller-supplied type validator.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `safeValidateTypes`: success
/// returns both the validated value and the original raw value, while
/// validation failures return a [`TypeValidationError`] and preserve the raw
/// value.
pub fn safe_validate_types<T, F, E>(
    value: JsonValue,
    validate: F,
    context: Option<TypeValidationContext>,
) -> ValidateTypesResult<T>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    match validate(&value) {
        Ok(validated_value) => ValidateTypesResult::success(validated_value, value),
        Err(error) => {
            let validation_error = TypeValidationError::new(value.clone(), error, context);
            ValidateTypesResult::failure(validation_error, value)
        }
    }
}

/// Parses and validates options for a single provider.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `parseProviderOptions`:
/// missing provider options are ignored, present provider-specific options are
/// validated, and validation failures become an [`InvalidArgumentError`] for
/// the `providerOptions` argument.
pub fn parse_provider_options<T, F, E>(
    provider: &str,
    provider_options: Option<&ProviderOptions>,
    validate: F,
) -> Result<Option<T>, InvalidArgumentError>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let Some(provider_options) = provider_options.and_then(|options| options.get(provider)) else {
        return Ok(None);
    };

    match safe_validate_types(JsonValue::Object(provider_options.clone()), validate, None) {
        ValidateTypesResult::Success { value, .. } => Ok(Some(value)),
        ValidateTypesResult::Failure { .. } => Err(InvalidArgumentError::new(
            "providerOptions",
            format!("invalid {provider} provider options"),
        )),
    }
}

/// Parses a JSON string into a JSON value.
///
/// This mirrors the no-schema overload of upstream `@ai-sdk/provider-utils`
/// `parseJSON`, using Rust's JSON representation and wrapping parse failures
/// in the provider-level [`JsonParseError`].
pub fn parse_json(text: &str) -> Result<JsonValue, JsonParseError> {
    secure_json_parse(text).map_err(|cause| JsonParseError::new(text, cause))
}

/// Safely parses a JSON string into a JSON value.
///
/// This mirrors the no-schema overload of upstream `@ai-sdk/provider-utils`
/// `safeParseJSON`: successful parses include both `value` and `rawValue`, and
/// parse failures are returned as [`JsonParseError`] values without a raw JSON
/// value.
pub fn safe_parse_json(text: &str) -> ParseJsonResult {
    match parse_json(text) {
        Ok(value) => ParseJsonResult::success(value.clone(), value),
        Err(error) => ParseJsonResult::failure(error, None),
    }
}

/// Returns whether the input can be parsed as JSON.
pub fn is_parsable_json(input: &str) -> bool {
    secure_json_parse(input).is_ok()
}

fn secure_json_parse(text: &str) -> Result<JsonValue, SecureJsonParseError> {
    let value = serde_json::from_str::<JsonValue>(text).map_err(SecureJsonParseError::Parse)?;
    reject_forbidden_json_keys(&value)?;
    Ok(value)
}

#[derive(Debug)]
enum SecureJsonParseError {
    Parse(serde_json::Error),
    ForbiddenPrototypeProperty,
}

impl fmt::Display for SecureJsonParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::ForbiddenPrototypeProperty => {
                formatter.write_str("Object contains forbidden prototype property")
            }
        }
    }
}

fn reject_forbidden_json_keys(value: &JsonValue) -> Result<(), SecureJsonParseError> {
    match value {
        JsonValue::Array(values) => {
            for value in values {
                reject_forbidden_json_keys(value)?;
            }
        }
        JsonValue::Object(object) => {
            if object.contains_key("__proto__") {
                return Err(SecureJsonParseError::ForbiddenPrototypeProperty);
            }

            if object
                .get("constructor")
                .and_then(JsonValue::as_object)
                .is_some_and(|constructor| constructor.contains_key("prototype"))
            {
                return Err(SecureJsonParseError::ForbiddenPrototypeProperty);
            }

            for value in object.values() {
                reject_forbidden_json_keys(value)?;
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {}
    }

    Ok(())
}

/// Converts inline file data into raw bytes.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertInlineFileDataToUint8Array`: text file data is UTF-8 encoded, raw
/// byte data is returned unchanged, and string data is decoded from base64.
/// URL and provider-reference variants are rejected because the upstream helper
/// only accepts tagged inline data/text file data.
pub fn convert_inline_file_data_to_bytes(
    data: &FileData,
) -> Result<Vec<u8>, InlineFileDataBytesError> {
    match data {
        FileData::Text { text } => Ok(text.as_bytes().to_vec()),
        FileData::Data { data } => match data {
            FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
            FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
                .map_err(|_| InlineFileDataBytesError::InvalidBase64Data),
        },
        FileData::Url { .. } | FileData::Reference { .. } => {
            Err(InlineFileDataBytesError::NonInlineFileData)
        }
    }
}

/// Converts a base64 or base64url string into raw bytes.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertBase64ToUint8Array`: URL-safe `-` and `_` alphabet characters are
/// accepted in addition to ordinary base64 data.
pub fn convert_base64_to_bytes(base64: &str) -> Result<Vec<u8>, Base64DecodeError> {
    decode_base64(base64).ok_or(Base64DecodeError)
}

/// Converts raw bytes into a base64 string.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertUint8ArrayToBase64`.
pub fn convert_bytes_to_base64(bytes: &[u8]) -> String {
    encode_base64(bytes)
}

/// Converts file data content into a base64 string.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `convertToBase64`: base64
/// strings pass through unchanged, while raw bytes are encoded.
pub fn convert_to_base64(value: &FileDataContent) -> String {
    match value {
        FileDataContent::Bytes(bytes) => convert_bytes_to_base64(bytes),
        FileDataContent::Base64(base64) => base64.clone(),
    }
}

/// Detects the IANA media type of raw bytes or base64-encoded file content.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `detectMediaType`: when a
/// top-level media type is supplied, only that signature table is checked;
/// otherwise image, application document, audio, and video signatures are
/// considered in upstream order.
pub fn detect_media_type(
    data: &FileDataContent,
    top_level_type: Option<&str>,
) -> Option<&'static str> {
    if let Some(top_level_type) = top_level_type {
        return match top_level_type {
            "image" => detect_media_type_by_signatures(data, IMAGE_MEDIA_TYPE_SIGNATURES),
            "audio" => detect_media_type_by_signatures(data, AUDIO_MEDIA_TYPE_SIGNATURES),
            "video" => detect_media_type_by_signatures(data, VIDEO_MEDIA_TYPE_SIGNATURES),
            "application" => detect_media_type_by_signatures(data, DOCUMENT_MEDIA_TYPE_SIGNATURES),
            _ => None,
        };
    }

    for signatures in [
        IMAGE_MEDIA_TYPE_SIGNATURES,
        DOCUMENT_MEDIA_TYPE_SIGNATURES,
        AUDIO_MEDIA_TYPE_SIGNATURES,
        VIDEO_MEDIA_TYPE_SIGNATURES,
    ] {
        if let Some(media_type) = detect_media_type_by_signatures(data, signatures) {
            return Some(media_type);
        }
    }

    None
}

fn detect_media_type_by_signatures(
    data: &FileDataContent,
    signatures: &[MediaTypeSignature],
) -> Option<&'static str> {
    let bytes = bytes_for_media_type_detection(data)?;

    signatures
        .iter()
        .find(|signature| bytes_match_signature(&bytes, signature.bytes_prefix))
        .map(|signature| signature.media_type)
}

fn bytes_match_signature(bytes: &[u8], bytes_prefix: &[Option<u8>]) -> bool {
    bytes.len() >= bytes_prefix.len()
        && bytes_prefix
            .iter()
            .enumerate()
            .all(|(index, byte)| byte.is_none_or(|byte| bytes[index] == byte))
}

fn bytes_for_media_type_detection(data: &FileDataContent) -> Option<Vec<u8>> {
    match data {
        FileDataContent::Bytes(bytes) => Some(strip_id3_tags_if_present(bytes).to_vec()),
        FileDataContent::Base64(base64) if base64.starts_with("SUQz") => {
            decode_base64(base64).map(|bytes| strip_id3_tags_if_present(&bytes).to_vec())
        }
        FileDataContent::Base64(base64) => {
            let prefix_length = base64
                .char_indices()
                .nth(24)
                .map_or(base64.len(), |(index, _)| index);
            decode_base64(&base64[..prefix_length])
        }
    }
}

fn strip_id3_tags_if_present(bytes: &[u8]) -> &[u8] {
    if bytes.len() <= 10 || !bytes.starts_with(&[0x49, 0x44, 0x33]) {
        return bytes;
    }

    let id3_size = ((usize::from(bytes[6] & 0x7f)) << 21)
        | ((usize::from(bytes[7] & 0x7f)) << 14)
        | ((usize::from(bytes[8] & 0x7f)) << 7)
        | usize::from(bytes[9] & 0x7f);

    bytes.get(id3_size + 10..).unwrap_or_default()
}

fn decode_base64(base64: &str) -> Option<Vec<u8>> {
    let mut sextets = Vec::new();

    for byte in base64.bytes() {
        match byte {
            b'=' => break,
            b'\t' | b'\n' | b'\r' | b' ' => continue,
            _ => sextets.push(base64_value(byte)?),
        }
    }

    if sextets.len() % 4 == 1 {
        return None;
    }

    let mut bytes = Vec::with_capacity((sextets.len() * 3) / 4);
    let mut chunks = sextets.chunks_exact(4);

    for chunk in &mut chunks {
        let buffer = (u32::from(chunk[0]) << 18)
            | (u32::from(chunk[1]) << 12)
            | (u32::from(chunk[2]) << 6)
            | u32::from(chunk[3]);
        bytes.push((buffer >> 16) as u8);
        bytes.push((buffer >> 8) as u8);
        bytes.push(buffer as u8);
    }

    match chunks.remainder() {
        [] => {}
        [first, second] => {
            bytes.push((*first << 2) | (*second >> 4));
        }
        [first, second, third] => {
            bytes.push((*first << 2) | (*second >> 4));
            bytes.push(((*second & 0x0f) << 4) | (*third >> 2));
        }
        _ => return None,
    }

    Some(bytes)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or_default();
        let third = chunk.get(2).copied().unwrap_or_default();
        let bits = (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third);

        encoded.push(ALPHABET[((bits >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((bits >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(ALPHABET[((bits >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(ALPHABET[(bits & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

/// Returns the top-level segment of a media type.
pub fn get_top_level_media_type(media_type: &str) -> &str {
    media_type
        .find('/')
        .map_or(media_type, |slash_index| &media_type[..slash_index])
}

/// Returns whether a media type has a non-empty, non-wildcard subtype.
pub fn is_full_media_type(media_type: &str) -> bool {
    media_type
        .split_once('/')
        .is_some_and(|(_, subtype)| !subtype.is_empty() && subtype != "*")
}

/// Resolves a prompt file part media type to a full `type/subtype` value.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `resolveFullMediaType`:
/// full media types are returned unchanged, top-level or wildcard media types
/// are detected from inline byte data when possible, and other unresolved cases
/// report an [`UnsupportedFunctionalityError`].
pub fn resolve_full_media_type(
    part: &LanguageModelFilePart,
) -> Result<String, UnsupportedFunctionalityError> {
    if is_full_media_type(&part.media_type) {
        return Ok(part.media_type.clone());
    }

    let FileData::Data { data } = &part.data else {
        return Err(UnsupportedFunctionalityError::new(format!(
            "file of media type \"{}\" must specify subtype since it is not passed as inline bytes",
            part.media_type
        )));
    };

    detect_media_type(data, Some(get_top_level_media_type(&part.media_type)))
        .map(str::to_string)
        .ok_or_else(|| {
            UnsupportedFunctionalityError::new(format!(
                "file of media type \"{}\" must specify subtype since it could not be auto-detected",
                part.media_type
            ))
        })
}

/// Returns whether a URL is natively supported by a model for a media type.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isUrlSupported`: media type
/// keys and the checked URL are matched case-insensitively by lowercasing before
/// regex evaluation, `*` and `*/*` match all media types, and top-level-only
/// media types such as `image` only match the corresponding `image/*` key.
pub fn is_url_supported(
    media_type: &str,
    url: &str,
    supported_urls: &LanguageModelSupportedUrls,
) -> bool {
    let media_type = media_type.to_lowercase();
    let url = url.to_lowercase();
    let is_top_level_only = !media_type.contains('/');

    supported_urls
        .iter()
        .flat_map(|(supported_media_type, patterns)| {
            let supported_media_type = supported_media_type.to_lowercase();
            let media_type_prefix = if supported_media_type == "*" || supported_media_type == "*/*"
            {
                String::new()
            } else {
                supported_media_type.replacen('*', "", 1)
            };

            let media_type_matches = if media_type_prefix.is_empty() {
                true
            } else if is_top_level_only {
                format!("{media_type}/") == media_type_prefix
            } else {
                media_type.starts_with(&media_type_prefix)
            };

            media_type_matches.then_some(patterns).into_iter().flatten()
        })
        .any(|pattern| {
            regex::Regex::new(pattern)
                .map(|regex| regex.is_match(&url))
                .unwrap_or(false)
        })
}

/// Reads response body chunks with a maximum size limit.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `readResponseWithSizeLimit`:
/// a parseable `Content-Length` header is checked before reading chunks, streamed
/// bytes are checked as they are accumulated, and limit violations return a
/// [`DownloadError`] with the upstream message shape.
pub fn read_response_with_size_limit<I, C>(
    url: &str,
    chunks: I,
    content_length: Option<&str>,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, DownloadError>
where
    I: IntoIterator<Item = C>,
    C: AsRef<[u8]>,
{
    let max_bytes = max_bytes.unwrap_or(DEFAULT_MAX_DOWNLOAD_SIZE);

    if let Some(content_length) = content_length.and_then(parse_content_length_header)
        && content_length > max_bytes as u128
    {
        return Err(DownloadError::new(
            url,
            format!(
                "Download of {url} exceeded maximum size of {max_bytes} bytes (Content-Length: {content_length})."
            ),
        ));
    }

    let mut response_body = Vec::new();
    let mut total_bytes = 0usize;

    for chunk in chunks {
        let chunk = chunk.as_ref();
        total_bytes = total_bytes.checked_add(chunk.len()).ok_or_else(|| {
            DownloadError::new(
                url,
                format!("Download of {url} exceeded maximum size of {max_bytes} bytes."),
            )
        })?;

        if total_bytes > max_bytes {
            return Err(DownloadError::new(
                url,
                format!("Download of {url} exceeded maximum size of {max_bytes} bytes."),
            ));
        }

        response_body.extend_from_slice(chunk);
    }

    Ok(response_body)
}

fn parse_content_length_header(content_length: &str) -> Option<u128> {
    let content_length = content_length.trim_start();
    let content_length = content_length.strip_prefix('+').unwrap_or(content_length);

    if content_length.starts_with('-') {
        return None;
    }

    let mut digits = content_length.bytes().take_while(u8::is_ascii_digit);
    let first_digit = digits.next()?;
    let mut length = u128::from(first_digit - b'0');

    for digit in digits {
        length = length
            .saturating_mul(10)
            .saturating_add(u128::from(digit - b'0'));
    }

    Some(length)
}

/// Converts an image model file into a URL or data URI string.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertImageModelFileToDataUri`: URL files are returned as-is, base64 file
/// data is embedded directly, and raw bytes are base64-encoded into a data URI.
pub fn convert_image_model_file_to_data_uri(file: &ImageModelFile) -> String {
    match file {
        ImageModelFile::Url { url, .. } => url.as_str().to_string(),
        ImageModelFile::File {
            media_type, data, ..
        } => {
            let base64 = match data {
                FileDataContent::Bytes(bytes) => encode_base64(bytes),
                FileDataContent::Base64(base64) => base64.clone(),
            };

            format!("data:{media_type};base64,{base64}")
        }
    }
}

/// Validates that a URL is safe to download from.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `validateDownloadUrl`:
/// `http`, `https`, and `data` URLs are accepted, while local protocols,
/// localhost-style hostnames, and private IPv4/IPv6 addresses are rejected to
/// avoid accidental internal network access.
pub fn validate_download_url(url: &str) -> Result<(), DownloadError> {
    let parsed =
        Url::parse(url).map_err(|_| DownloadError::new(url, format!("Invalid URL: {url}")))?;

    match parsed.scheme() {
        "data" => return Ok(()),
        "http" | "https" => {}
        scheme => {
            return Err(DownloadError::new(
                url,
                format!("URL scheme must be http, https, or data, got {scheme}:"),
            ));
        }
    }

    let host = parsed
        .host()
        .ok_or_else(|| DownloadError::new(url, "URL must have a hostname"))?;

    match host {
        Host::Domain(hostname) => validate_download_hostname(url, hostname),
        Host::Ipv4(ip) => validate_download_ipv4(url, ip),
        Host::Ipv6(ip) => validate_download_ipv6(url, ip),
    }
}

fn validate_download_hostname(url: &str, hostname: &str) -> Result<(), DownloadError> {
    let hostname = hostname.to_ascii_lowercase();

    if hostname == "localhost" || hostname.ends_with(".local") || hostname.ends_with(".localhost") {
        return Err(DownloadError::new(
            url,
            format!("URL with hostname {hostname} is not allowed"),
        ));
    }

    Ok(())
}

fn validate_download_ipv4(url: &str, ip: Ipv4Addr) -> Result<(), DownloadError> {
    if is_private_download_ipv4(ip) {
        Err(DownloadError::new(
            url,
            format!("URL with IP address {ip} is not allowed"),
        ))
    } else {
        Ok(())
    }
}

fn validate_download_ipv6(url: &str, ip: Ipv6Addr) -> Result<(), DownloadError> {
    if is_private_download_ipv6(ip) {
        Err(DownloadError::new(
            url,
            format!("URL with IPv6 address [{ip}] is not allowed"),
        ))
    } else {
        Ok(())
    }
}

fn is_private_download_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();

    a == 0
        || a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
}

fn is_private_download_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }

    if let Some(mapped_ipv4) = ip.to_ipv4_mapped() {
        return is_private_download_ipv4(mapped_ipv4);
    }

    let segments = ip.segments();
    let first_segment = segments[0];

    (first_segment & 0xfe00) == 0xfc00 || (first_segment & 0xffc0) == 0xfe80
}

/// Extracts HTTP response headers into the shared header record shape.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `extractResponseHeaders` by
/// turning iterable response header entries into a plain key-value record. Header
/// names and values are preserved as supplied by the response implementation.
pub fn extract_response_headers<K, V, I>(headers: I) -> Headers
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    headers
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
}

/// Creates an API-call error result from a failed HTTP status response.
///
/// This mirrors upstream `createStatusCodeErrorResponseHandler`: it uses the
/// response status text as the error message, preserves request body values,
/// status, headers, and raw response body, and returns the extracted headers
/// beside the constructed [`ApiCallError`].
pub fn create_status_code_error_response_handler(
    options: StatusCodeErrorResponseHandlerOptions,
) -> ResponseHandlerResult<ApiCallError> {
    let StatusCodeErrorResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        status_text,
        response_headers,
        response_body,
    } = options;

    let error = ApiCallError::new(status_text, url, request_body_values)
        .with_status_code(status_code)
        .with_response_headers(response_headers.clone())
        .with_response_body(response_body);

    ResponseHandlerResult::new(error).with_response_headers(response_headers)
}

/// Parses a failed JSON response body into an API-call error when possible.
///
/// This mirrors upstream `createJsonErrorResponseHandler`: empty bodies and
/// malformed JSON error payloads fall back to the response status text, while a
/// valid parsed error payload drives the error message and is preserved as
/// [`ApiCallError::data`]. The retry override closure returns `Some(bool)` to
/// replace the upstream status-code default or `None` to keep it.
pub fn create_json_error_response_handler<T, F, E, M, S, R>(
    options: JsonErrorResponseHandlerOptions,
    validate: F,
    error_to_message: M,
    is_retryable: R,
) -> ResponseHandlerResult<ApiCallError>
where
    T: Serialize,
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
    M: FnOnce(&T) -> S,
    S: Into<String>,
    R: FnOnce(u16, Option<&T>) -> Option<bool>,
{
    if options.response_body.trim().is_empty() {
        let retry_override = is_retryable(options.status_code, None);
        let message = options.status_text.clone();
        return json_error_response_result(options, message, None, retry_override);
    }

    let raw_value = match safe_parse_json(&options.response_body) {
        ParseJsonResult::Success { raw_value, .. } => raw_value,
        ParseJsonResult::Failure { .. } => {
            let retry_override = is_retryable(options.status_code, None);
            let message = options.status_text.clone();
            return json_error_response_result(options, message, None, retry_override);
        }
    };

    match safe_validate_types(raw_value, validate, None) {
        ValidateTypesResult::Success {
            value: parsed_error,
            ..
        } => match serde_json::to_value(&parsed_error) {
            Ok(data) => {
                let retry_override = is_retryable(options.status_code, Some(&parsed_error));
                let message = error_to_message(&parsed_error).into();
                json_error_response_result(options, message, Some(data), retry_override)
            }
            Err(_) => {
                let retry_override = is_retryable(options.status_code, None);
                let message = options.status_text.clone();
                json_error_response_result(options, message, None, retry_override)
            }
        },
        ValidateTypesResult::Failure { .. } => {
            let retry_override = is_retryable(options.status_code, None);
            let message = options.status_text.clone();
            json_error_response_result(options, message, None, retry_override)
        }
    }
}

fn json_error_response_result(
    options: JsonErrorResponseHandlerOptions,
    message: String,
    data: Option<JsonValue>,
    retry_override: Option<bool>,
) -> ResponseHandlerResult<ApiCallError> {
    let JsonErrorResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        response_headers,
        response_body,
        ..
    } = options;

    let mut error = ApiCallError::new(message, url, request_body_values)
        .with_status_code(status_code)
        .with_response_headers(response_headers.clone())
        .with_response_body(response_body);

    if let Some(data) = data {
        error = error.with_data(data);
    }

    if let Some(is_retryable) = retry_override {
        error = error.with_is_retryable(is_retryable);
    }

    ResponseHandlerResult::new(error).with_response_headers(response_headers)
}

/// Parses and validates a successful JSON response body.
///
/// This mirrors upstream `createJsonResponseHandler`: the returned handler
/// result contains the validated value, the raw parsed JSON value, and response
/// headers. JSON parse or validation failures become an [`ApiCallError`] with
/// the upstream `Invalid JSON response` message and the original response
/// context.
pub fn create_json_response_handler<T, F, E>(
    options: JsonResponseHandlerOptions,
    validate: F,
) -> Result<ResponseHandlerResult<T>, Box<ApiCallError>>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let JsonResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        response_headers,
        response_body,
    } = options;

    let raw_value = match safe_parse_json(&response_body) {
        ParseJsonResult::Success { raw_value, .. } => raw_value,
        ParseJsonResult::Failure { .. } => {
            return Err(Box::new(invalid_json_response_error(
                url,
                request_body_values,
                status_code,
                response_headers,
                response_body,
            )));
        }
    };

    match safe_validate_types(raw_value.clone(), validate, None) {
        ValidateTypesResult::Success { value, raw_value } => Ok(ResponseHandlerResult::new(value)
            .with_raw_value(raw_value)
            .with_response_headers(response_headers)),
        ValidateTypesResult::Failure { .. } => Err(Box::new(invalid_json_response_error(
            url,
            request_body_values,
            status_code,
            response_headers,
            response_body,
        ))),
    }
}

fn invalid_json_response_error(
    url: String,
    request_body_values: JsonValue,
    status_code: u16,
    response_headers: Headers,
    response_body: String,
) -> ApiCallError {
    ApiCallError::new("Invalid JSON response", url, request_body_values)
        .with_status_code(status_code)
        .with_response_headers(response_headers)
        .with_response_body(response_body)
}

/// Returns a successful binary response body.
///
/// This mirrors upstream `createBinaryResponseHandler`: the returned handler
/// result contains the response bytes and headers. A missing response body
/// becomes an [`ApiCallError`] with the upstream `Response body is empty`
/// message and original response context.
pub fn create_binary_response_handler(
    options: BinaryResponseHandlerOptions,
) -> Result<ResponseHandlerResult<Vec<u8>>, Box<ApiCallError>> {
    let BinaryResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        response_headers,
        response_body,
    } = options;

    match response_body {
        Some(response_body) => {
            Ok(ResponseHandlerResult::new(response_body).with_response_headers(response_headers))
        }
        None => Err(Box::new(
            ApiCallError::new("Response body is empty", url, request_body_values)
                .with_status_code(status_code)
                .with_response_headers(response_headers),
        )),
    }
}

/// Combines optional HTTP header maps, with later maps overriding earlier ones.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `combineHeaders`: missing
/// maps are ignored, header names are preserved as supplied, and missing values
/// are retained so a later `None` can intentionally override an earlier value.
pub fn combine_headers<K, V, I, H>(headers: H) -> BTreeMap<String, Option<String>>
where
    H: IntoIterator<Item = Option<I>>,
    I: IntoIterator<Item = (K, Option<V>)>,
    K: Into<String>,
    V: Into<String>,
{
    let mut combined_headers = BTreeMap::new();

    for current_headers in headers.into_iter().flatten() {
        for (key, value) in current_headers {
            combined_headers.insert(key.into(), value.map(Into::into));
        }
    }

    combined_headers
}

/// Normalizes optional HTTP header entries into a lower-case header map.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `normalizeHeaders`: missing
/// input becomes an empty map, nullish values are removed, and header names are
/// normalized to lower case.
pub fn normalize_headers<K, V, I>(headers: Option<I>) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    let Some(headers) = headers else {
        return Headers::new();
    };

    headers
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| (key.as_ref().to_ascii_lowercase(), value.into()))
        })
        .collect()
}

/// Appends suffix parts to the normalized `user-agent` header.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `withUserAgentSuffix`: input
/// headers are normalized first, missing header values are removed, and empty
/// user-agent parts are skipped before joining with spaces.
pub fn with_user_agent_suffix<K, V, I, S, P>(
    headers: Option<I>,
    user_agent_suffix_parts: P,
) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
    P: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut headers = normalize_headers(headers);
    let current_user_agent = headers.get("user-agent").map(String::as_str).unwrap_or("");

    let mut user_agent_parts = Vec::new();

    if !current_user_agent.is_empty() {
        user_agent_parts.push(current_user_agent.to_string());
    }

    for part in user_agent_suffix_parts {
        let part = part.as_ref();
        if !part.is_empty() {
            user_agent_parts.push(part.to_string());
        }
    }

    let user_agent = user_agent_parts.join(" ");

    headers.insert("user-agent".to_string(), user_agent);
    headers
}

/// Options for loading a provider API key from an explicit value or environment variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadApiKeyOptions {
    /// Explicit API key value. When present, it is returned without reading the environment.
    pub api_key: Option<String>,

    /// Environment variable to read when `api_key` is not provided.
    pub environment_variable_name: String,

    /// Parameter name used in missing-key error messages.
    pub api_key_parameter_name: String,

    /// Human-readable provider or API description used in error messages.
    pub description: String,
}

impl LoadApiKeyOptions {
    /// Creates API key loading options with the upstream default `apiKey` parameter name.
    pub fn new(
        environment_variable_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            api_key: None,
            environment_variable_name: environment_variable_name.into(),
            api_key_parameter_name: "apiKey".to_string(),
            description: description.into(),
        }
    }

    /// Sets the explicit API key value.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the parameter name used in missing-key error messages.
    pub fn with_api_key_parameter_name(
        mut self,
        api_key_parameter_name: impl Into<String>,
    ) -> Self {
        self.api_key_parameter_name = api_key_parameter_name.into();
        self
    }
}

/// Loads a provider API key from an explicit value or environment variable.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `loadApiKey` for Rust callers:
/// typed explicit values win, missing values read the named environment variable,
/// and missing or non-Unicode environment values produce `LoadApiKeyError`.
pub fn load_api_key(options: LoadApiKeyOptions) -> Result<String, LoadApiKeyError> {
    load_api_key_with_env(options, |name| env::var(name))
}

fn load_api_key_with_env(
    options: LoadApiKeyOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Result<String, LoadApiKeyError> {
    if let Some(api_key) = options.api_key {
        return Ok(api_key);
    }

    match load_env(&options.environment_variable_name) {
        Ok(api_key) => Ok(api_key),
        Err(VarError::NotPresent) => Err(LoadApiKeyError::new(format!(
            "{} API key is missing. Pass it using the '{}' parameter or the {} environment variable.",
            options.description, options.api_key_parameter_name, options.environment_variable_name
        ))),
        Err(VarError::NotUnicode(_)) => Err(LoadApiKeyError::new(format!(
            "{} API key must be a string. The value of the {} environment variable is not a string.",
            options.description, options.environment_variable_name
        ))),
    }
}

/// Options for loading a provider setting from an explicit value or environment variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadSettingOptions {
    /// Explicit setting value. When present, it is returned without reading the environment.
    pub setting_value: Option<String>,

    /// Environment variable to read when `setting_value` is not provided.
    pub environment_variable_name: String,

    /// Parameter name used in missing-setting error messages.
    pub setting_name: String,

    /// Human-readable setting description used in error messages.
    pub description: String,
}

impl LoadSettingOptions {
    /// Creates setting loading options.
    pub fn new(
        environment_variable_name: impl Into<String>,
        setting_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            setting_value: None,
            environment_variable_name: environment_variable_name.into(),
            setting_name: setting_name.into(),
            description: description.into(),
        }
    }

    /// Sets the explicit setting value.
    pub fn with_setting_value(mut self, setting_value: impl Into<String>) -> Self {
        self.setting_value = Some(setting_value.into());
        self
    }
}

/// Loads a required string setting from an explicit value or environment variable.
pub fn load_setting(options: LoadSettingOptions) -> Result<String, LoadSettingError> {
    load_setting_with_env(options, |name| env::var(name))
}

fn load_setting_with_env(
    options: LoadSettingOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Result<String, LoadSettingError> {
    if let Some(setting_value) = options.setting_value {
        return Ok(setting_value);
    }

    match load_env(&options.environment_variable_name) {
        Ok(setting_value) => Ok(setting_value),
        Err(VarError::NotPresent) => Err(LoadSettingError::new(format!(
            "{} setting is missing. Pass it using the '{}' parameter or the {} environment variable.",
            options.description, options.setting_name, options.environment_variable_name
        ))),
        Err(VarError::NotUnicode(_)) => Err(LoadSettingError::new(format!(
            "{} setting must be a string. The value of the {} environment variable is not a string.",
            options.description, options.environment_variable_name
        ))),
    }
}

/// Options for loading an optional provider setting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOptionalSettingOptions {
    /// Explicit setting value. When present, it is returned without reading the environment.
    pub setting_value: Option<String>,

    /// Environment variable to read when `setting_value` is not provided.
    pub environment_variable_name: String,
}

impl LoadOptionalSettingOptions {
    /// Creates optional setting loading options.
    pub fn new(environment_variable_name: impl Into<String>) -> Self {
        Self {
            setting_value: None,
            environment_variable_name: environment_variable_name.into(),
        }
    }

    /// Sets the explicit setting value.
    pub fn with_setting_value(mut self, setting_value: impl Into<String>) -> Self {
        self.setting_value = Some(setting_value.into());
        self
    }
}

/// Loads an optional setting from an explicit value or environment variable.
pub fn load_optional_setting(options: LoadOptionalSettingOptions) -> Option<String> {
    load_optional_setting_with_env(options, |name| env::var(name))
}

fn load_optional_setting_with_env(
    options: LoadOptionalSettingOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Option<String> {
    if let Some(setting_value) = options.setting_value {
        return Some(setting_value);
    }

    load_env(&options.environment_variable_name).ok()
}

/// Maps a media type to the file extension used by upstream provider uploads.
pub fn media_type_to_extension(media_type: &str) -> String {
    let subtype = media_type
        .split_once('/')
        .map_or("", |(_, subtype)| subtype)
        .to_ascii_lowercase();

    match subtype.as_str() {
        "mpeg" => "mp3".to_string(),
        "x-wav" => "wav".to_string(),
        "opus" => "ogg".to_string(),
        "mp4" | "x-m4a" => "m4a".to_string(),
        _ => subtype,
    }
}

/// Strips all file extension segments from a filename.
pub fn strip_file_extension(filename: &str) -> &str {
    filename
        .find('.')
        .map_or(filename, |first_dot_index| &filename[..first_dot_index])
}

/// Removes exactly one trailing slash from a URL-like string when present.
pub fn without_trailing_slash(url: Option<&str>) -> Option<&str> {
    url.map(|url| url.strip_suffix('/').unwrap_or(url))
}

/// Resolves a provider reference to the provider-specific identifier.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `resolveProviderReference`
/// while reusing the crate's shared provider-reference contract.
pub fn resolve_provider_reference<'a>(
    reference: &'a ProviderReference,
    provider: &str,
) -> Result<&'a str, NoSuchProviderReferenceError> {
    reference.provider_id(provider)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env::VarError;
    use std::ffi::OsString;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use crate::language_model::{
        LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage,
        LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelTool, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::{
        FileData, FileDataContent, ImageModelFile, JsonObject, JsonValue, ProviderReference,
        TypeValidationContext, TypeValidationError, Warning,
    };
    use serde_json::json;
    use url::Url;

    use super::{
        Arrayable, Base64DecodeError, BinaryResponseHandlerOptions, DEFAULT_MAX_DOWNLOAD_SIZE,
        DownloadError, InjectJsonInstructionIntoMessagesOptions, InlineFileDataBytesError,
        JsonErrorResponseHandlerOptions, JsonResponseHandlerOptions, LoadApiKeyOptions,
        LoadOptionalSettingOptions, LoadSettingOptions, ParseJsonError, ParseJsonResult,
        ReasoningLevel, ResponseHandlerResult, StatusCodeErrorResponseHandlerOptions, Tool,
        ToolExecutionError, ToolExecutionOptions, ValidateTypesResult,
        add_additional_properties_to_json_schema, as_array, combine_headers,
        convert_base64_to_bytes, convert_bytes_to_base64, convert_image_model_file_to_data_uri,
        convert_inline_file_data_to_bytes, convert_to_base64, create_binary_response_handler,
        create_json_error_response_handler, create_json_response_handler,
        create_status_code_error_response_handler, create_tool_name_mapping, detect_media_type,
        extract_response_headers, filter_nullable, get_top_level_media_type,
        inject_json_instruction, inject_json_instruction_into_messages, is_custom_reasoning,
        is_full_media_type, is_non_nullable, is_parsable_json, is_provider_reference,
        is_url_supported, load_api_key, load_api_key_with_env, load_optional_setting_with_env,
        load_setting, load_setting_with_env, map_reasoning_to_provider_budget,
        map_reasoning_to_provider_effort, media_type_to_extension, normalize_headers, parse_json,
        parse_provider_options, prepare_tools, read_response_with_size_limit,
        remove_undefined_entries, resolve_full_media_type, resolve_provider_reference,
        safe_parse_json, safe_validate_types, strip_file_extension, validate_download_url,
        validate_types, with_user_agent_suffix, without_trailing_slash,
    };

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

    fn object_schema() -> crate::JsonSchema {
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

    fn object_schema_json() -> String {
        serde_json::to_string(&object_schema()).expect("schema serializes")
    }

    #[derive(Debug, Eq, PartialEq)]
    struct Person {
        name: String,
        age: u64,
    }

    fn validate_person(value: &JsonValue) -> Result<Person, &'static str> {
        let object = value.as_object().ok_or("Invalid input")?;
        let name = object
            .get("name")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid input")?;
        let age = object
            .get("age")
            .and_then(JsonValue::as_u64)
            .ok_or("Invalid input")?;

        Ok(Person {
            name: name.to_string(),
            age,
        })
    }

    #[derive(Debug, Eq, PartialEq, serde::Serialize)]
    struct ErrorPayload {
        code: String,
        message: String,
    }

    fn validate_error_payload(value: &JsonValue) -> Result<ErrorPayload, &'static str> {
        let object = value.as_object().ok_or("Invalid error")?;
        let code = object
            .get("code")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid error")?;
        let message = object
            .get("message")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid error")?;

        Ok(ErrorPayload {
            code: code.to_string(),
            message: message.to_string(),
        })
    }

    fn expected_schema_instruction(prompt: &str) -> String {
        format!(
            "{prompt}\n\nJSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
            object_schema_json()
        )
    }

    #[test]
    fn inject_json_instruction_adds_schema_to_prompt() {
        assert_eq!(
            inject_json_instruction(Some("Generate weather"), Some(&object_schema()), None, None),
            expected_schema_instruction("Generate weather")
        );
    }

    #[test]
    fn inject_json_instruction_uses_generic_json_suffix_without_schema() {
        assert_eq!(
            inject_json_instruction(Some("Generate data"), None, None, None),
            "Generate data\n\nYou MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_omits_empty_prompt() {
        assert_eq!(
            inject_json_instruction(Some(""), Some(&object_schema()), None, None),
            format!(
                "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                object_schema_json()
            )
        );
    }

    #[test]
    fn inject_json_instruction_uses_custom_schema_lines() {
        assert_eq!(
            inject_json_instruction(
                Some("Generate weather"),
                Some(&object_schema()),
                Some("Custom schema:"),
                Some("Follow this exactly."),
            ),
            format!(
                "Generate weather\n\nCustom schema:\n{}\nFollow this exactly.",
                object_schema_json()
            )
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_updates_existing_system_message() {
        let messages = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Generate weather")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use Brisbane")),
            ])),
        ];

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(messages.clone())
                    .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_schema_instruction("Generate weather")
                )),
                messages[1].clone(),
            ]
        );
        assert_eq!(
            messages[0],
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Generate weather"))
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_inserts_system_message() {
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Generate weather")),
        ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![user_message.clone()])
                    .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(format!(
                    "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                    object_schema_json()
                ))),
                user_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_preserves_system_provider_options() {
        let provider_options = BTreeMap::from([(
            "test-provider".to_string(),
            json!({ "trace": "abc" })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            inject_json_instruction_into_messages(InjectJsonInstructionIntoMessagesOptions::new(
                vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate data")
                        .with_provider_options(provider_options.clone()),
                )]
            )),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Generate data\n\nYou MUST answer with JSON.")
                    .with_provider_options(provider_options),
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_uses_custom_schema_lines() {
        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate weather"),
                )])
                .with_schema(object_schema())
                .with_schema_prefix("Custom schema:")
                .with_schema_suffix("Follow this exactly.")
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(format!(
                    "Generate weather\n\nCustom schema:\n{}\nFollow this exactly.",
                    object_schema_json()
                ))
            )]
        );
    }

    #[test]
    fn reasoning_level_serializes_upstream_strings() {
        assert_eq!(
            serde_json::to_value(ReasoningLevel::Xhigh).expect("reasoning level serializes"),
            json!("xhigh")
        );
        assert_eq!(
            serde_json::from_value::<ReasoningLevel>(json!("minimal"))
                .expect("reasoning level deserializes"),
            ReasoningLevel::Minimal
        );
    }

    #[test]
    fn reasoning_level_converts_from_custom_reasoning_efforts() {
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::High),
            Ok(ReasoningLevel::High)
        );
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::ProviderDefault),
            Err(LanguageModelReasoningEffort::ProviderDefault)
        );
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::None),
            Err(LanguageModelReasoningEffort::None)
        );
    }

    #[test]
    fn is_custom_reasoning_matches_upstream_default_handling() {
        assert!(!is_custom_reasoning(None));
        assert!(!is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::ProviderDefault
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::None
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Minimal
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Low
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Medium
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::High
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Xhigh
        )));
    }

    #[test]
    fn map_reasoning_to_provider_effort_returns_direct_match_without_warning() {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Low, "low".to_string()),
            (ReasoningLevel::Medium, "medium".to_string()),
            (ReasoningLevel::High, "high".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Medium, &effort_map, &mut warnings),
            Some("medium".to_string())
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_effort_warns_for_renamed_match() {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Minimal, &effort_map, &mut warnings),
            Some("low".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                        .to_string()
                ),
            }]
        );

        warnings.clear();
        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Xhigh, &effort_map, &mut warnings),
            Some("max".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"xhigh\" is not directly supported by this model. mapped to effort \"max\"."
                        .to_string()
                ),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_effort_warns_for_missing_level() {
        let effort_map = BTreeMap::from([(ReasoningLevel::Medium, "medium".to_string())]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::High, &effort_map, &mut warnings),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_budget_uses_default_percentages_and_clamps() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                64_000,
                64_000,
                None,
                None,
                &mut warnings,
            ),
            Some(19_200)
        );
        assert_eq!(warnings, Vec::new());

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Xhigh,
                64_000,
                50_000,
                None,
                None,
                &mut warnings,
            ),
            Some(50_000)
        );
        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                None,
                None,
                &mut warnings,
            ),
            Some(1024)
        );
        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                Some(512),
                None,
                &mut warnings,
            ),
            Some(512)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_uses_custom_percentages() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                10_000,
                10_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            Some(5000)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_warns_for_missing_custom_percentage() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::High,
                64_000,
                64_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    #[test]
    fn arrayable_serializes_single_or_array_values() {
        assert_eq!(
            serde_json::to_value(Arrayable::single("value")).expect("single value serializes"),
            json!("value")
        );
        assert_eq!(
            serde_json::to_value(Arrayable::array(vec!["a", "b"])).expect("array value serializes"),
            json!(["a", "b"])
        );
    }

    #[test]
    fn arrayable_deserializes_single_or_array_values() {
        assert_eq!(
            serde_json::from_value::<Arrayable<String>>(json!("value"))
                .expect("single value deserializes"),
            Arrayable::single("value".to_string())
        );
        assert_eq!(
            serde_json::from_value::<Arrayable<String>>(json!(["a", "b"]))
                .expect("array value deserializes"),
            Arrayable::array(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn as_array_returns_empty_array_for_missing_value() {
        assert_eq!(as_array::<String>(None), Vec::<String>::new());
    }

    #[test]
    fn as_array_wraps_single_value_in_array() {
        assert_eq!(as_array(Some(Arrayable::single("value"))), vec!["value"]);
    }

    #[test]
    fn as_array_returns_array_values_unchanged() {
        let value = vec!["a", "b"];

        assert_eq!(as_array(Some(Arrayable::array(value.clone()))), value);
    }

    #[test]
    fn add_additional_properties_to_json_schema_closes_nested_objects() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                },
                "age": { "type": "number" }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "user": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "name": { "type": "string" }
                        }
                    },
                    "age": { "type": "number" }
                }
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_closes_objects_in_arrays_and_unions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "ingredients": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                },
                "response": {
                    "type": ["object", "null"],
                    "properties": {
                        "ok": { "type": "boolean" }
                    }
                }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "ingredients": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "name": { "type": "string" }
                            }
                        }
                    },
                    "response": {
                        "type": ["object", "null"],
                        "additionalProperties": false,
                        "properties": {
                            "ok": { "type": "boolean" }
                        }
                    }
                }
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_visits_compositions_and_definitions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "response": {
                    "anyOf": [
                        { "type": "object", "properties": { "name": { "type": "string" } } },
                        { "type": "string" }
                    ],
                    "allOf": [
                        { "type": "object", "properties": { "age": { "type": "number" } } }
                    ],
                    "oneOf": [
                        { "type": "object", "properties": { "success": { "type": "boolean" } } }
                    ]
                },
                "node": { "$ref": "#/definitions/Node" }
            },
            "definitions": {
                "Node": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "value": { "type": "string" }
                    }
                }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "response": {
                        "anyOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "name": { "type": "string" } }
                            },
                            { "type": "string" }
                        ],
                        "allOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "age": { "type": "number" } }
                            }
                        ],
                        "oneOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "success": { "type": "boolean" } }
                            }
                        ]
                    },
                    "node": { "$ref": "#/definitions/Node" }
                },
                "definitions": {
                    "Node": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "value": { "type": "string" }
                        }
                    }
                }
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_leaves_non_object_schema_unchanged() {
        let schema = json!({
            "type": "string"
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "string"
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn is_non_nullable_reports_present_values() {
        assert!(is_non_nullable(&Some("value")));
        assert!(!is_non_nullable::<&str>(&None));
    }

    #[test]
    fn filter_nullable_removes_missing_values() {
        let values = vec![Some(1), None, Some(2), None, Some(3)];

        assert_eq!(filter_nullable(values), vec![1, 2, 3]);
    }

    #[test]
    fn filter_nullable_preserves_falsy_equivalent_values() {
        let values = vec![Some(json!(0)), Some(json!(false)), Some(json!("")), None];

        assert_eq!(
            filter_nullable(values),
            vec![json!(0), json!(false), json!("")]
        );
    }

    #[test]
    fn remove_undefined_entries_removes_missing_values() {
        let record = remove_undefined_entries([
            ("present", Some(json!("value"))),
            ("missing", None),
            ("alsoPresent", Some(json!({ "nested": true }))),
        ]);

        assert_eq!(
            record,
            BTreeMap::from([
                ("alsoPresent".to_string(), json!({ "nested": true })),
                ("present".to_string(), json!("value")),
            ])
        );
    }

    #[test]
    fn remove_undefined_entries_preserves_falsy_equivalent_values() {
        let record = remove_undefined_entries([
            ("zero", Some(json!(0))),
            ("false", Some(json!(false))),
            ("emptyString", Some(json!(""))),
            ("nullish", None),
        ]);

        assert_eq!(
            record,
            BTreeMap::from([
                ("emptyString".to_string(), json!("")),
                ("false".to_string(), json!(false)),
                ("zero".to_string(), json!(0)),
            ])
        );
    }

    #[test]
    fn remove_undefined_entries_handles_json_null_values_as_missing() {
        let record: BTreeMap<String, Option<serde_json::Value>> = serde_json::from_value(json!({
            "keep": "value",
            "drop": null
        }))
        .expect("record deserializes");

        assert_eq!(
            remove_undefined_entries(record),
            BTreeMap::from([("keep".to_string(), json!("value"))])
        );
    }

    #[test]
    fn is_provider_reference_accepts_plain_records() {
        assert!(is_provider_reference(&json!({
            "openai": "file-abc123"
        })));
        assert!(is_provider_reference(&json!({
            "fileId": "abc"
        })));
    }

    #[test]
    fn is_provider_reference_rejects_tagged_file_data_objects() {
        assert!(!is_provider_reference(&json!({
            "type": "reference",
            "reference": {
                "fileId": "abc"
            }
        })));
        assert!(!is_provider_reference(&json!({
            "type": "data",
            "data": "x"
        })));
    }

    #[test]
    fn is_provider_reference_rejects_non_objects_and_arrays() {
        assert!(!is_provider_reference(&JsonValue::Null));
        assert!(!is_provider_reference(&json!("some-string")));
        assert!(!is_provider_reference(&json!(42)));
        assert!(!is_provider_reference(&json!([1, 2, 3])));
    }

    #[test]
    fn validate_types_returns_validated_values() {
        let value = json!({ "name": "John", "age": 30 });

        let person = validate_types(value, validate_person, None).expect("person validates");

        assert_eq!(
            person,
            Person {
                name: "John".to_string(),
                age: 30,
            }
        );
    }

    #[test]
    fn validate_types_wraps_validation_errors_with_context() {
        let value = json!({ "name": "John", "age": "30" });
        let context = TypeValidationContext::new()
            .with_field("person.age")
            .with_entity_name("person")
            .with_entity_id("user-1");

        let error = validate_types(value.clone(), validate_person, Some(context.clone()))
            .expect_err("invalid person should fail validation");

        assert_eq!(error.value(), &value);
        assert_eq!(error.context(), Some(&context));
        assert_eq!(error.cause_message(), "Invalid input");
        assert!(
            error
                .message()
                .starts_with("Type validation failed for person.age (person, id: \"user-1\"):")
        );
    }

    #[test]
    fn safe_validate_types_preserves_raw_value_after_transformation() {
        let value = json!({ "count": "42" });

        let parsed = safe_validate_types(
            value.clone(),
            |value| {
                let count = value
                    .get("count")
                    .and_then(JsonValue::as_str)
                    .and_then(|count| count.parse::<u64>().ok())
                    .ok_or("Expected numeric string")?;

                Ok::<_, &'static str>(json!({ "count": count }))
            },
            None,
        );

        assert_eq!(
            parsed,
            ValidateTypesResult::success(json!({ "count": 42 }), value.clone())
        );
        assert!(parsed.is_success());
        assert!(!parsed.is_failure());
        assert_eq!(parsed.value(), Some(&json!({ "count": 42 })));
        assert_eq!(parsed.raw_value(), &value);
        assert!(parsed.error().is_none());
    }

    #[test]
    fn safe_validate_types_returns_error_and_raw_value_on_failure() {
        let value = json!({ "name": "John", "age": "30" });
        let parsed = safe_validate_types(value.clone(), validate_person, None);

        assert!(parsed.is_failure());
        assert!(parsed.value().is_none());
        assert_eq!(parsed.raw_value(), &value);

        let error = parsed.error().expect("validation error is returned");
        assert_eq!(error.value(), &value);
        assert_eq!(error.cause_message(), "Invalid input");
    }

    #[test]
    fn parse_provider_options_returns_none_for_missing_provider_options() {
        let provider_options = BTreeMap::from([(
            "openai".to_string(),
            json!({ "name": "John", "age": 30 })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            parse_provider_options("anthropic", Some(&provider_options), |_| {
                Result::<Person, &'static str>::Err("validator should not run")
            })
            .expect("missing provider options are ignored"),
            None
        );
        assert_eq!(
            parse_provider_options("openai", None, |_| {
                Result::<Person, &'static str>::Err("validator should not run")
            })
            .expect("missing provider options map is ignored"),
            None
        );
    }

    #[test]
    fn parse_provider_options_returns_validated_provider_options() {
        let provider_options = BTreeMap::from([(
            "openai".to_string(),
            json!({ "name": "John", "age": 30 })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            parse_provider_options("openai", Some(&provider_options), validate_person)
                .expect("provider options validate"),
            Some(Person {
                name: "John".to_string(),
                age: 30,
            })
        );
    }

    #[test]
    fn parse_provider_options_reports_invalid_argument_on_validation_failure() {
        let provider_options = BTreeMap::from([(
            "openai".to_string(),
            json!({ "name": "John", "age": "30" })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        let error = parse_provider_options("openai", Some(&provider_options), validate_person)
            .expect_err("invalid provider options are rejected");

        assert_eq!(error.argument(), "providerOptions");
        assert_eq!(error.message(), "invalid openai provider options");
    }

    #[test]
    fn parse_json_parses_json_values_without_schema() {
        assert_eq!(
            parse_json(r#"{"foo":"bar","items":[1,true,null]}"#).expect("JSON parses"),
            json!({
                "foo": "bar",
                "items": [1, true, null],
            })
        );
        assert_eq!(parse_json("0").expect("number JSON parses"), json!(0));
        assert_eq!(
            parse_json(r#""hello""#).expect("string JSON parses"),
            json!("hello")
        );
    }

    #[test]
    fn parse_json_wraps_invalid_json_in_provider_error() {
        let error = parse_json("invalid json").expect_err("invalid JSON fails");

        assert_eq!(error.text(), "invalid json");
        assert!(
            error
                .message()
                .starts_with("JSON parsing failed: Text: invalid json.\nError message:")
        );
    }

    #[test]
    fn parse_json_rejects_proto_properties() {
        let error = parse_json(r#"{ "a": 5, "c": { "d": 0, "__proto__": { "isAdmin": true } } }"#)
            .expect_err("prototype keys are rejected");

        assert_eq!(
            error.cause_message(),
            "Object contains forbidden prototype property"
        );
    }

    #[test]
    fn parse_json_rejects_constructor_prototype_objects() {
        let error = parse_json(r#"{ "constructor": { "prototype": { "isAdmin": true } } }"#)
            .expect_err("constructor prototype objects are rejected");

        assert_eq!(
            error.cause_message(),
            "Object contains forbidden prototype property"
        );
    }

    #[test]
    fn parse_json_allows_safe_constructor_properties() {
        assert_eq!(
            parse_json(r#"{ "constructor": "string value" }"#).expect("JSON parses"),
            json!({ "constructor": "string value" })
        );
        assert_eq!(
            parse_json(r#"{ "constructor": null }"#).expect("JSON parses"),
            json!({ "constructor": null })
        );
        assert_eq!(
            parse_json(r#"{ "constructor": { "safe": true } }"#).expect("JSON parses"),
            json!({ "constructor": { "safe": true } })
        );
    }

    #[test]
    fn safe_parse_json_returns_success_with_raw_value() {
        let parsed = safe_parse_json(r#"{"foo":"bar","items":[1,true,null]}"#);
        let expected_value = json!({
            "foo": "bar",
            "items": [1, true, null],
        });

        assert_eq!(
            parsed,
            ParseJsonResult::success(expected_value.clone(), expected_value.clone())
        );
        assert!(parsed.is_success());
        assert!(!parsed.is_failure());
        assert_eq!(parsed.value(), Some(&expected_value));
        assert_eq!(parsed.raw_value(), Some(&expected_value));
        assert!(parsed.error().is_none());
    }

    #[test]
    fn safe_parse_json_returns_json_parse_error_without_raw_value_on_invalid_json() {
        let parsed = safe_parse_json("invalid json");

        assert!(parsed.is_failure());
        assert!(parsed.value().is_none());
        assert!(parsed.raw_value().is_none());

        let error = parsed.error().expect("parse error is returned");
        let json_parse_error = error
            .as_json_parse_error()
            .expect("failure is a JSON parse error");
        assert_eq!(json_parse_error.text(), "invalid json");
        assert!(
            json_parse_error
                .message()
                .starts_with("JSON parsing failed: Text: invalid json.\nError message:")
        );
    }

    #[test]
    fn safe_parse_json_returns_json_parse_error_for_forbidden_prototype_properties() {
        let parsed = safe_parse_json(r#"{ "__proto__": { "isAdmin": true } }"#);
        let error = parsed.error().expect("parse error is returned");

        assert_eq!(
            error
                .as_json_parse_error()
                .expect("secure parse failure uses JSON parse error")
                .cause_message(),
            "Object contains forbidden prototype property"
        );
        assert!(parsed.raw_value().is_none());
    }

    #[test]
    fn parse_json_error_can_wrap_type_validation_failures() {
        let validation_error =
            TypeValidationError::new(json!({ "age": "30" }), "Expected number", None);
        let parse_error = ParseJsonError::from(validation_error.clone());

        assert_eq!(
            parse_error.as_type_validation_error(),
            Some(&validation_error)
        );
        assert!(parse_error.as_json_parse_error().is_none());
        assert_eq!(parse_error.to_string(), validation_error.to_string());
    }

    #[test]
    fn is_parsable_json_matches_secure_parse_result() {
        assert!(is_parsable_json(r#"{"foo":"bar"}"#));
        assert!(is_parsable_json("[1,2,3]"));
        assert!(is_parsable_json(r#""hello""#));
        assert!(!is_parsable_json("invalid"));
        assert!(!is_parsable_json(r#"{ "foo": }"#));
        assert!(!is_parsable_json(
            r#"{ "\u005f\u005fproto__": { "isAdmin": true } }"#
        ));
    }

    #[test]
    fn convert_inline_file_data_to_bytes_encodes_text_as_utf8() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Text {
                text: "hello\nworld".to_string(),
            })
            .expect("text data converts"),
            b"hello\nworld".to_vec()
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_returns_raw_bytes_unchanged() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Bytes(vec![0, 1, 2, 255]),
            })
            .expect("raw bytes convert"),
            vec![0, 1, 2, 255]
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_decodes_base64_data() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Base64("SGVsbG8=".to_string()),
            })
            .expect("base64 data converts"),
            b"Hello".to_vec()
        );
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Base64("-_8=".to_string()),
            })
            .expect("base64url data converts"),
            vec![251, 255]
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_rejects_non_inline_file_data() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Url {
                url: Url::parse("https://example.com/file.txt").expect("valid URL"),
            })
            .expect_err("URL file data is not inline"),
            InlineFileDataBytesError::NonInlineFileData
        );

        let reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-abc123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Reference { reference })
                .expect_err("provider references are not inline"),
            InlineFileDataBytesError::NonInlineFileData
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_rejects_invalid_base64_data() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Base64("not valid base64!".to_string()),
            })
            .expect_err("invalid base64 does not convert"),
            InlineFileDataBytesError::InvalidBase64Data
        );
    }

    #[test]
    fn convert_base64_to_bytes_decodes_standard_and_url_safe_data() {
        assert_eq!(
            convert_base64_to_bytes("SGVsbG8=").expect("base64 decodes"),
            b"Hello".to_vec()
        );
        assert_eq!(
            convert_base64_to_bytes("-_8=").expect("base64url decodes"),
            vec![251, 255]
        );
        assert_eq!(
            convert_base64_to_bytes("SG V sb\tG8=\n").expect("whitespace is ignored"),
            b"Hello".to_vec()
        );
    }

    #[test]
    fn convert_base64_to_bytes_rejects_invalid_data() {
        assert_eq!(
            convert_base64_to_bytes("not valid base64!").expect_err("invalid data fails"),
            Base64DecodeError
        );
    }

    #[test]
    fn convert_bytes_to_base64_encodes_raw_bytes() {
        assert_eq!(convert_bytes_to_base64(b"Hello"), "SGVsbG8=");
        assert_eq!(convert_bytes_to_base64(&[251, 255]), "+/8=");
        assert_eq!(convert_bytes_to_base64(&[]), "");
    }

    #[test]
    fn convert_to_base64_passes_strings_through_and_encodes_bytes() {
        assert_eq!(
            convert_to_base64(&FileDataContent::Base64("already-base64".to_string())),
            "already-base64"
        );
        assert_eq!(
            convert_to_base64(&FileDataContent::Bytes(b"Hello".to_vec())),
            "SGVsbG8="
        );
    }

    #[test]
    fn get_top_level_media_type_matches_upstream_edge_cases() {
        assert_eq!(get_top_level_media_type("image/png"), "image");
        assert_eq!(get_top_level_media_type("audio/*"), "audio");
        assert_eq!(get_top_level_media_type("text"), "text");
        assert_eq!(get_top_level_media_type(""), "");
        assert_eq!(get_top_level_media_type("/"), "");
        assert_eq!(get_top_level_media_type("image/"), "image");
    }

    #[test]
    fn is_full_media_type_requires_concrete_subtype() {
        assert!(is_full_media_type("image/png"));
        assert!(is_full_media_type("application/pdf"));
        assert!(!is_full_media_type("image"));
        assert!(!is_full_media_type("image/*"));
        assert!(!is_full_media_type("image/"));
        assert!(!is_full_media_type("/"));
    }

    #[test]
    fn is_url_supported_matches_media_type_and_url_patterns() {
        let supported_urls = BTreeMap::from([
            (
                "text/plain".to_string(),
                vec![r"^https://docs\.example\.com/.+\.txt$".to_string()],
            ),
            (
                "image/png".to_string(),
                vec![r"^https://images\.example\.com/.+".to_string()],
            ),
        ]);

        assert!(is_url_supported(
            "text/plain",
            "https://docs.example.com/readme.txt",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "text/plain",
            "https://docs.example.com/readme.md",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "image/png",
            "https://docs.example.com/readme.txt",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_matches_wildcards_and_top_level_media_types() {
        let supported_urls = BTreeMap::from([
            (
                "image/*".to_string(),
                vec![r"^https://cdn\.example\.com/images/".to_string()],
            ),
            (
                "*/*".to_string(),
                vec![r"^https://public\.example\.com/".to_string()],
            ),
        ]);

        assert!(is_url_supported(
            "image/png",
            "https://cdn.example.com/images/cat.png",
            &supported_urls
        ));
        assert!(is_url_supported(
            "image",
            "https://cdn.example.com/images/cat.png",
            &supported_urls
        ));
        assert!(is_url_supported(
            "video/mp4",
            "https://public.example.com/video.mp4",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "audio",
            "https://cdn.example.com/images/cat.png",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_lowercases_media_type_keys_and_urls_before_matching() {
        let supported_urls = BTreeMap::from([(
            "TEXT/PLAIN".to_string(),
            vec![r"^https://example\.com/path$".to_string()],
        )]);

        assert!(is_url_supported(
            "text/plain",
            "https://EXAMPLE.com/PATH",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_ignores_invalid_regex_sources() {
        let supported_urls = BTreeMap::from([(
            "*".to_string(),
            vec!["[".to_string(), r"^https://example\.com$".to_string()],
        )]);

        assert!(is_url_supported(
            "text/plain",
            "https://example.com",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "text/plain",
            "https://another.example.com",
            &supported_urls
        ));
    }

    #[test]
    fn read_response_with_size_limit_reads_chunks_within_limit() {
        let chunks = [b"abcd".as_slice(), b"efgh".as_slice()];

        let body =
            read_response_with_size_limit("https://example.com/file", chunks, Some("8"), Some(100))
                .expect("body is within limit");

        assert_eq!(body, b"abcdefgh");
    }

    #[test]
    fn read_response_with_size_limit_rejects_large_content_length_early() {
        let error = read_response_with_size_limit(
            "https://example.com/large",
            [b"small".as_slice()],
            Some("1000 bytes"),
            Some(100),
        )
        .expect_err("content-length exceeds limit");

        assert_eq!(error.url(), "https://example.com/large");
        assert_eq!(
            error.message(),
            "Download of https://example.com/large exceeded maximum size of 100 bytes (Content-Length: 1000)."
        );
    }

    #[test]
    fn read_response_with_size_limit_rejects_streams_that_exceed_limit() {
        let chunks = [vec![1; 40], vec![2; 40]];

        let error =
            read_response_with_size_limit("https://example.com/stream", chunks, None, Some(50))
                .expect_err("streamed bytes exceed limit");

        assert_eq!(
            error.message(),
            "Download of https://example.com/stream exceeded maximum size of 50 bytes."
        );
    }

    #[test]
    fn read_response_with_size_limit_checks_larger_actual_body_even_when_length_claims_small() {
        let chunks = [vec![42; 60]];

        let error =
            read_response_with_size_limit("https://example.com/liar", chunks, Some("10"), Some(50))
                .expect_err("actual body still exceeds limit");

        assert_eq!(
            error.message(),
            "Download of https://example.com/liar exceeded maximum size of 50 bytes."
        );
    }

    #[test]
    fn read_response_with_size_limit_uses_upstream_default_limit_and_ignores_invalid_lengths() {
        assert_eq!(DEFAULT_MAX_DOWNLOAD_SIZE, 2 * 1024 * 1024 * 1024);

        let body = read_response_with_size_limit(
            "https://example.com/empty",
            [b"ok".as_slice()],
            Some("not-a-number"),
            None,
        )
        .expect("invalid content-length is ignored");

        assert_eq!(body, b"ok");
    }

    #[test]
    fn detect_media_type_matches_top_level_signature_tables() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff]),
                Some("image"),
            ),
            Some("image/png")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x25, 0x50, 0x44, 0x46, 0x00]),
                Some("application"),
            ),
            Some("application/pdf")
        );
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(vec![0xff, 0xfb]), Some("audio")),
            Some("audio/mpeg")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x1a, 0x45, 0xdf, 0xa3]),
                Some("video"),
            ),
            Some("video/webm")
        );
    }

    #[test]
    fn detect_media_type_handles_base64_and_id3_prefixed_mp3() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Base64("iVBORw0KGgo=".to_string()),
                Some("image"),
            ),
            Some("image/png")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![
                    0x49, 0x44, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfb,
                ]),
                Some("audio"),
            ),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_returns_none_for_unsupported_or_unmatched_data() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff]),
                Some("text"),
            ),
            None
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
                Some("image"),
            ),
            None
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Base64("not valid base64!".to_string()),
                None,
            ),
            None
        );
    }

    #[test]
    fn detect_media_type_without_top_level_type_uses_upstream_order() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70]),
                None,
            ),
            Some("video/mp4")
        );
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(vec![0x1a, 0x45, 0xdf, 0xa3]), None,),
            Some("audio/webm")
        );
    }

    #[test]
    fn resolve_full_media_type_returns_full_media_type_as_is() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47]),
            },
            "image/jpeg",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("full media type resolves"),
            "image/jpeg"
        );
    }

    #[test]
    fn resolve_full_media_type_detects_inline_byte_subtype() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]),
            },
            "image",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("inline bytes resolve"),
            "image/png"
        );
    }

    #[test]
    fn resolve_full_media_type_treats_wildcard_as_top_level() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Base64("iVBORw0KGgo=".to_string()),
            },
            "image/*",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("wildcard media type resolves"),
            "image/png"
        );
    }

    #[test]
    fn resolve_full_media_type_detects_application_pdf() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x25, 0x50, 0x44, 0x46, 0x2d]),
            },
            "application",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("application subtype resolves"),
            "application/pdf"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_non_inline_byte_data() {
        let part = LanguageModelFilePart::new(
            FileData::Url {
                url: Url::parse("https://example.com/file.png").expect("valid URL"),
            },
            "image",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("top-level URL media type requires a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"image\" must specify subtype since it is not passed as inline bytes"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_unrecognized_inline_bytes() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
            },
            "image",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("unrecognized inline bytes require a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"image\" must specify subtype since it could not be auto-detected"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_returns_url_as_is() {
        let file = ImageModelFile::url(
            Url::parse("https://example.com/image.png?width=100&height=200").expect("valid URL"),
        );

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "https://example.com/image.png?width=100&height=200"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_embeds_base64_data() {
        let file = ImageModelFile::file(
            "image/png",
            FileDataContent::Base64("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ".to_string()),
        );

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_encodes_raw_bytes() {
        let file = ImageModelFile::file("image/webp", FileDataContent::Bytes(b"Hello".to_vec()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/webp;base64,SGVsbG8="
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_handles_empty_raw_bytes() {
        let file = ImageModelFile::file("image/png", FileDataContent::Bytes(Vec::new()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/png;base64,"
        );
    }

    #[test]
    fn download_error_retains_status_and_cause_messages() {
        let status_error =
            DownloadError::with_status("https://example.com/missing.png", 404, "Not Found");
        assert_eq!(status_error.url(), "https://example.com/missing.png");
        assert_eq!(status_error.status_code(), Some(404));
        assert_eq!(status_error.status_text(), Some("Not Found"));
        assert_eq!(
            status_error.message(),
            "Failed to download https://example.com/missing.png: 404 Not Found"
        );
        assert_eq!(status_error.to_string(), status_error.message());

        let cause_error =
            DownloadError::with_cause_message("https://example.com/file", "connection refused");
        assert_eq!(
            cause_error.message(),
            "Failed to download https://example.com/file: connection refused"
        );
        assert_eq!(cause_error.status_code(), None);
        assert_eq!(cause_error.status_text(), None);
    }

    #[test]
    fn validate_download_url_allows_public_http_https_data_and_ip_urls() {
        assert!(validate_download_url("https://example.com/image.png").is_ok());
        assert!(validate_download_url("http://example.com/image.png").is_ok());
        assert!(validate_download_url("https://203.0.113.1/file").is_ok());
        assert!(validate_download_url("https://example.com:8080/file").is_ok());
        assert!(validate_download_url("data:text/plain;base64,aGVsbG8=").is_ok());
    }

    #[test]
    fn validate_download_url_rejects_invalid_and_unsupported_schemes() {
        assert_eq!(
            validate_download_url("not-a-url")
                .expect_err("invalid URL is rejected")
                .message(),
            "Invalid URL: not-a-url"
        );
        assert_eq!(
            validate_download_url("file:///etc/passwd")
                .expect_err("file scheme is rejected")
                .message(),
            "URL scheme must be http, https, or data, got file:"
        );
        assert_eq!(
            validate_download_url("ftp://example.com/file")
                .expect_err("ftp scheme is rejected")
                .message(),
            "URL scheme must be http, https, or data, got ftp:"
        );
        assert_eq!(
            validate_download_url("javascript:alert(1)")
                .expect_err("javascript scheme is rejected")
                .message(),
            "URL scheme must be http, https, or data, got javascript:"
        );
    }

    #[test]
    fn validate_download_url_rejects_local_hostnames() {
        for url in [
            "http://localhost/file",
            "http://localhost:3000/file",
            "http://myhost.local/file",
            "http://app.localhost/file",
        ] {
            assert!(
                validate_download_url(url)
                    .expect_err("local hostname is rejected")
                    .message()
                    .contains("is not allowed"),
                "{url} should be rejected"
            );
        }
    }

    #[test]
    fn validate_download_url_rejects_private_ipv4_addresses() {
        for url in [
            "http://127.0.0.1/file",
            "http://127.255.0.1/file",
            "http://10.0.0.1/file",
            "http://172.16.0.1/file",
            "http://172.31.255.255/file",
            "http://192.168.1.1/file",
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/file",
        ] {
            assert!(
                validate_download_url(url)
                    .expect_err("private IPv4 address is rejected")
                    .message()
                    .contains("IP address"),
                "{url} should be rejected"
            );
        }

        assert!(validate_download_url("http://172.15.0.1/file").is_ok());
        assert!(validate_download_url("http://172.32.0.1/file").is_ok());
    }

    #[test]
    fn validate_download_url_rejects_private_ipv6_addresses() {
        for url in [
            "http://[::1]/file",
            "http://[::]/file",
            "http://[fc00::1]/file",
            "http://[fd12::1]/file",
            "http://[fe80::1]/file",
            "http://[::ffff:127.0.0.1]/file",
            "http://[::ffff:10.0.0.1]/file",
            "http://[::ffff:169.254.169.254]/file",
        ] {
            assert!(
                validate_download_url(url)
                    .expect_err("private IPv6 address is rejected")
                    .message()
                    .contains("IPv6 address"),
                "{url} should be rejected"
            );
        }

        assert!(validate_download_url("http://[::ffff:203.0.113.1]/file").is_ok());
    }

    #[test]
    fn extract_response_headers_preserves_response_header_entries() {
        let headers = extract_response_headers([
            ("content-type", "application/json"),
            ("x-request-id", "req_123"),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-request-id".to_string(), "req_123".to_string()),
            ])
        );
    }

    #[test]
    fn extract_response_headers_lets_later_entries_override_duplicates() {
        let headers = extract_response_headers([
            ("x-provider", "first"),
            ("x-provider", "second"),
            ("x-empty", ""),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("x-empty".to_string(), "".to_string()),
                ("x-provider".to_string(), "second".to_string()),
            ])
        );
    }

    #[test]
    fn response_handler_result_serializes_optional_metadata() {
        let value = json!({ "name": "John" });
        let raw_value = json!({ "name": "John", "extraField": "ignored" });
        let result = ResponseHandlerResult::new(value.clone())
            .with_raw_value(raw_value.clone())
            .with_response_headers(BTreeMap::from([(
                "x-request-id".to_string(),
                "req_123".to_string(),
            )]));

        let serialized = serde_json::to_value(&result).expect("result serializes");

        assert_eq!(
            serialized,
            json!({
                "value": value,
                "rawValue": raw_value,
                "responseHeaders": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn response_handler_result_deserializes_minimal_result() {
        let result: ResponseHandlerResult<JsonValue> =
            serde_json::from_value(json!({ "value": "ok" })).expect("result deserializes");

        assert_eq!(result.value(), &json!("ok"));
        assert_eq!(result.raw_value(), None);
        assert_eq!(result.response_headers(), None);
    }

    #[test]
    fn binary_response_handler_options_use_camel_case_json() {
        let options = BinaryResponseHandlerOptions::new(
            "https://api.example.com/files",
            json!({ "file": "test" }),
            200,
            vec![1, 2, 3, 4],
        )
        .with_response_headers(BTreeMap::from([(
            "content-type".to_string(),
            "application/octet-stream".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/files",
                "requestBodyValues": { "file": "test" },
                "statusCode": 200,
                "responseHeaders": {
                    "content-type": "application/octet-stream"
                },
                "responseBody": [1, 2, 3, 4]
            })
        );

        let deserialized: BinaryResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn binary_response_handler_options_deserialize_missing_body() {
        let options: BinaryResponseHandlerOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/files",
            "requestBodyValues": {},
            "statusCode": 204,
            "responseHeaders": {}
        }))
        .expect("options deserialize");

        assert_eq!(options.response_body, None);
        assert_eq!(options.response_headers, BTreeMap::new());
    }

    #[test]
    fn create_binary_response_handler_returns_bytes_and_headers() {
        let response_headers = BTreeMap::from([(
            "content-type".to_string(),
            "application/octet-stream".to_string(),
        )]);
        let options = BinaryResponseHandlerOptions::new(
            "https://api.example.com/files",
            json!({ "file": "test" }),
            200,
            vec![1, 2, 3, 4],
        )
        .with_response_headers(response_headers.clone());

        let result = create_binary_response_handler(options).expect("binary response is handled");

        assert_eq!(result.value(), &vec![1, 2, 3, 4]);
        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(result.raw_value(), None);
    }

    #[test]
    fn create_binary_response_handler_preserves_empty_byte_body() {
        let options = BinaryResponseHandlerOptions::new(
            "https://api.example.com/files",
            json!({}),
            200,
            Vec::<u8>::new(),
        );

        let result =
            create_binary_response_handler(options).expect("empty binary body is still readable");

        assert_eq!(result.value(), &Vec::<u8>::new());
    }

    #[test]
    fn create_binary_response_handler_returns_api_call_error_for_missing_body() {
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_500".to_string())]);
        let options = BinaryResponseHandlerOptions::empty(
            "https://api.example.com/files",
            json!({ "file": "test" }),
            500,
        )
        .with_response_headers(response_headers.clone());

        let error = create_binary_response_handler(options).expect_err("missing body is rejected");

        assert_eq!(error.message(), "Response body is empty");
        assert_eq!(error.url(), "https://api.example.com/files");
        assert_eq!(error.request_body_values(), &json!({ "file": "test" }));
        assert_eq!(error.status_code(), Some(500));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), None);
        assert!(error.is_retryable());
    }

    #[test]
    fn json_error_response_handler_options_use_camel_case_json() {
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            400,
            "Bad Request",
            r#"{"code":"bad_request","message":"Invalid model"}"#,
        )
        .with_response_headers(BTreeMap::from([(
            "x-request-id".to_string(),
            "req_400".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/models",
                "requestBodyValues": { "model": "test" },
                "statusCode": 400,
                "statusText": "Bad Request",
                "responseHeaders": {
                    "x-request-id": "req_400"
                },
                "responseBody": "{\"code\":\"bad_request\",\"message\":\"Invalid model\"}"
            })
        );

        let deserialized: JsonErrorResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn create_json_error_response_handler_uses_parsed_error_data() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            400,
            "Bad Request",
            r#"{"code":"bad_request","message":"Invalid model"}"#,
        )
        .with_response_headers(response_headers.clone());

        let result = create_json_error_response_handler(
            options,
            validate_error_payload,
            |error| format!("{}: {}", error.code, error.message),
            |status_code, error| {
                assert_eq!(status_code, 400);
                assert_eq!(error.map(|error| error.code.as_str()), Some("bad_request"));
                Some(false)
            },
        );
        let error = result.value();

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(error.message(), "bad_request: Invalid model");
        assert_eq!(error.url(), "https://api.example.com/models");
        assert_eq!(error.request_body_values(), &json!({ "model": "test" }));
        assert_eq!(error.status_code(), Some(400));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(
            error.response_body(),
            Some("{\"code\":\"bad_request\",\"message\":\"Invalid model\"}")
        );
        assert_eq!(
            error.data(),
            Some(&json!({ "code": "bad_request", "message": "Invalid model" }))
        );
        assert!(!error.is_retryable());
    }

    #[test]
    fn create_json_error_response_handler_falls_back_for_empty_body() {
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            400,
            "Bad Request",
            " \n\t ",
        );

        let result = create_json_error_response_handler(
            options,
            validate_error_payload,
            |error| error.message.clone(),
            |status_code, error: Option<&ErrorPayload>| {
                assert_eq!(status_code, 400);
                assert!(error.is_none());
                Some(true)
            },
        );
        let error = result.value();

        assert_eq!(error.message(), "Bad Request");
        assert_eq!(error.response_body(), Some(" \n\t "));
        assert_eq!(error.data(), None);
        assert!(error.is_retryable());
    }

    #[test]
    fn create_json_error_response_handler_falls_back_for_invalid_json() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            502,
            "Bad Gateway",
            "{not json",
        )
        .with_response_headers(response_headers.clone());

        let result = create_json_error_response_handler(
            options,
            validate_error_payload,
            |error| error.message.clone(),
            |_, error: Option<&ErrorPayload>| {
                assert!(error.is_none());
                None
            },
        );
        let error = result.value();

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(error.message(), "Bad Gateway");
        assert_eq!(error.status_code(), Some(502));
        assert_eq!(error.response_body(), Some("{not json"));
        assert_eq!(error.data(), None);
        assert!(error.is_retryable());
    }

    #[test]
    fn json_response_handler_options_use_camel_case_json() {
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            200,
            r#"{"name":"John"}"#,
        )
        .with_response_headers(BTreeMap::from([(
            "content-type".to_string(),
            "application/json".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/models",
                "requestBodyValues": { "model": "test" },
                "statusCode": 200,
                "responseHeaders": {
                    "content-type": "application/json"
                },
                "responseBody": "{\"name\":\"John\"}"
            })
        );

        let deserialized: JsonResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn create_json_response_handler_returns_validated_value_raw_value_and_headers() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/users",
            json!({ "query": "john" }),
            200,
            r#"{"name":"John","age":30,"extraField":"ignored"}"#,
        )
        .with_response_headers(response_headers.clone());

        let result = create_json_response_handler(options, validate_person)
            .expect("valid JSON response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "John".to_string(),
                age: 30,
            }
        );
        assert_eq!(
            result.raw_value(),
            Some(&json!({ "name": "John", "age": 30, "extraField": "ignored" }))
        );
        assert_eq!(result.response_headers(), Some(&response_headers));
    }

    #[test]
    fn create_json_response_handler_returns_api_call_error_for_invalid_json() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/users",
            json!({ "query": "john" }),
            502,
            "{not json",
        )
        .with_response_headers(response_headers.clone());

        let error = create_json_response_handler(options, |value| {
            Ok::<JsonValue, &'static str>(value.clone())
        })
        .expect_err("invalid JSON response becomes an API call error");

        assert_eq!(error.message(), "Invalid JSON response");
        assert_eq!(error.url(), "https://api.example.com/users");
        assert_eq!(error.request_body_values(), &json!({ "query": "john" }));
        assert_eq!(error.status_code(), Some(502));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), Some("{not json"));
        assert!(error.is_retryable());
    }

    #[test]
    fn create_json_response_handler_returns_api_call_error_for_validation_failure() {
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/users",
            json!({ "query": "john" }),
            200,
            r#"{"name":"John"}"#,
        );

        let error = create_json_response_handler(options, validate_person)
            .expect_err("schema validation failure becomes an API call error");

        assert_eq!(error.message(), "Invalid JSON response");
        assert_eq!(error.status_code(), Some(200));
        assert_eq!(error.response_body(), Some("{\"name\":\"John\"}"));
        assert!(!error.is_retryable());
    }

    #[test]
    fn status_code_error_response_handler_options_use_camel_case_json() {
        let options = StatusCodeErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            404,
            "Not Found",
            "missing",
        )
        .with_response_headers(BTreeMap::from([(
            "x-request-id".to_string(),
            "req_404".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/models",
                "requestBodyValues": { "model": "test" },
                "statusCode": 404,
                "statusText": "Not Found",
                "responseHeaders": {
                    "x-request-id": "req_404"
                },
                "responseBody": "missing"
            })
        );

        let deserialized: StatusCodeErrorResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn create_status_code_error_response_handler_returns_api_call_error_result() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "text/plain".to_string())]);
        let options = StatusCodeErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            429,
            "Too Many Requests",
            "retry later",
        )
        .with_response_headers(response_headers.clone());

        let result = create_status_code_error_response_handler(options);
        let error = result.value();

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(error.message(), "Too Many Requests");
        assert_eq!(error.url(), "https://api.example.com/models");
        assert_eq!(error.request_body_values(), &json!({ "model": "test" }));
        assert_eq!(error.status_code(), Some(429));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), Some("retry later"));
        assert!(error.is_retryable());
    }

    #[test]
    fn combine_headers_returns_empty_map_for_missing_groups() {
        assert_eq!(
            combine_headers::<String, String, Vec<(String, Option<String>)>, _>([None, None,]),
            BTreeMap::new()
        );
    }

    #[test]
    fn combine_headers_preserves_keys_and_combines_groups() {
        let headers = combine_headers([
            Some(vec![
                ("Authorization", Some("Bearer token")),
                ("X-Feature", Some("alpha")),
            ]),
            None,
            Some(vec![("X-Feature", Some("beta")), ("X-Disabled", None)]),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                (
                    "Authorization".to_string(),
                    Some("Bearer token".to_string())
                ),
                ("X-Disabled".to_string(), None),
                ("X-Feature".to_string(), Some("beta".to_string())),
            ])
        );
    }

    #[test]
    fn combine_headers_allows_missing_values_to_override_present_values() {
        let headers = combine_headers([
            Some(vec![("x-enabled", Some("true")), ("x-empty", Some(""))]),
            Some(vec![("x-enabled", None)]),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("x-empty".to_string(), Some("".to_string())),
                ("x-enabled".to_string(), None),
            ])
        );
    }

    #[test]
    fn normalize_headers_returns_empty_map_for_missing_input() {
        assert_eq!(
            normalize_headers::<String, String, Vec<(String, Option<String>)>>(None),
            BTreeMap::new()
        );
    }

    #[test]
    fn normalize_headers_lowercases_keys_and_filters_missing_values() {
        let headers = normalize_headers(Some(vec![
            ("Authorization", Some("Bearer token")),
            ("X-Feature", Some("beta")),
            ("X-Ignore", None),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("x-feature".to_string(), "beta".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_preserves_empty_strings_and_allows_later_overrides() {
        let headers = normalize_headers(Some(vec![
            ("CONTENT-TYPE", Some("text/plain")),
            ("content-type", Some("application/json")),
            ("x-empty", Some("")),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-empty".to_string(), "".to_string()),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_creates_user_agent_header() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Content-Type", Some("application/json")),
                ("Authorization", Some("Bearer token")),
            ]),
            ["ai-sdk/0.0.0-test", "provider/test-openai"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "ai-sdk/0.0.0-test provider/test-openai".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_appends_to_existing_header_and_filters_empty_parts() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("User-Agent", Some("TestApp/0.0.0-test")),
                ("Accept", Some("application/json")),
            ]),
            ["", "ai-sdk/0.0.0-test", "provider/test-anthropic"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("accept".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "TestApp/0.0.0-test ai-sdk/0.0.0-test provider/test-anthropic".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_removes_missing_headers_before_appending() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Content-Type", Some("application/json")),
                ("Authorization", None),
                ("User-Agent", Some("TestApp/0.0.0-test")),
                ("Accept", Some("application/json")),
                ("Cache-Control", None),
            ]),
            ["ai-sdk/0.0.0-test"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("accept".to_string(), "application/json".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "TestApp/0.0.0-test ai-sdk/0.0.0-test".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_sets_empty_user_agent_when_no_parts_exist() {
        assert_eq!(
            with_user_agent_suffix::<String, String, Vec<(String, Option<String>)>, String, _>(
                None,
                Vec::new(),
            ),
            BTreeMap::from([("user-agent".to_string(), String::new())])
        );
    }

    #[test]
    fn create_tool_name_mapping_maps_provider_defined_tools() {
        let tools = vec![
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "custom-computer-tool",
                JsonObject::new(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "openai.code-interpreter",
                "custom-code-tool",
                JsonObject::new(),
            )),
        ];
        let provider_tool_names = BTreeMap::from([
            (
                "anthropic.computer-use".to_string(),
                "computer_use".to_string(),
            ),
            (
                "openai.code-interpreter".to_string(),
                "code_interpreter".to_string(),
            ),
        ]);

        let mapping = create_tool_name_mapping(&tools, &provider_tool_names);

        assert_eq!(
            mapping.to_provider_tool_name("custom-computer-tool"),
            "computer_use"
        );
        assert_eq!(
            mapping.to_provider_tool_name("custom-code-tool"),
            "code_interpreter"
        );
        assert_eq!(
            mapping.to_custom_tool_name("computer_use"),
            "custom-computer-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("code_interpreter"),
            "custom-code-tool"
        );
    }

    #[test]
    fn create_tool_name_mapping_ignores_function_tools() {
        let tools = vec![LanguageModelTool::Function(LanguageModelFunctionTool::new(
            "weather",
            object_schema(),
        ))];
        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("weather"), "weather");
        assert_eq!(mapping.to_custom_tool_name("weather"), "weather");
    }

    #[test]
    fn create_tool_name_mapping_passes_through_unknown_provider_tool_ids() {
        let tools = vec![LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "unknown.tool",
            "custom-tool",
            JsonObject::new(),
        ))];
        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("custom-tool"), "custom-tool");
        assert_eq!(mapping.to_custom_tool_name("unknown-name"), "unknown-name");
    }

    #[test]
    fn create_tool_name_mapping_handles_mixed_and_empty_tool_sets() {
        let provider_tool_names = BTreeMap::from([(
            "anthropic.computer-use".to_string(),
            "computer_use".to_string(),
        )]);
        let mixed_tools = vec![
            LanguageModelTool::Function(LanguageModelFunctionTool::new(
                "function-tool",
                object_schema(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "provider-tool",
                JsonObject::new(),
            )),
        ];

        let empty_mapping =
            create_tool_name_mapping(Vec::<LanguageModelTool>::new().iter(), &BTreeMap::new());
        assert_eq!(empty_mapping.to_provider_tool_name("any-tool"), "any-tool");
        assert_eq!(empty_mapping.to_custom_tool_name("any-tool"), "any-tool");

        let mapping = create_tool_name_mapping(&mixed_tools, &provider_tool_names);
        assert_eq!(
            mapping.to_provider_tool_name("function-tool"),
            "function-tool"
        );
        assert_eq!(
            mapping.to_provider_tool_name("provider-tool"),
            "computer_use"
        );
        assert_eq!(mapping.to_custom_tool_name("computer_use"), "provider-tool");
    }

    #[test]
    fn tool_prepares_upstream_function_tool_shape() {
        let tool = Tool::new("weather", object_schema())
            .with_description("Look up weather.")
            .with_input_example(
                json!({
                    "city": "Brisbane"
                })
                .as_object()
                .expect("input example is an object")
                .clone(),
            )
            .with_strict(true);

        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
                    .with_description("Look up weather.")
                    .with_input_example(
                        json!({ "city": "Brisbane" })
                            .as_object()
                            .expect("input example is an object")
                            .clone()
                    )
                    .with_strict(true)
            )
        );
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "description": "Look up weather.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
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
                "strict": true
            })
        );
    }

    #[test]
    fn prepare_tools_returns_none_for_empty_tool_sets() {
        assert_eq!(prepare_tools(Vec::<Tool>::new().iter()), None);
    }

    #[test]
    fn prepare_tools_converts_high_level_tools() {
        let tools = vec![Tool::new("weather", object_schema())];

        assert_eq!(
            prepare_tools(&tools),
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
            )])
        );
    }

    #[test]
    fn tool_execution_options_serialize_as_camel_case() {
        let options = ToolExecutionOptions::new(
            "call-1",
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Weather?"),
                )],
            ))],
        );

        assert_eq!(
            serde_json::to_value(options).expect("execution options serialize"),
            json!({
                "toolCallId": "call-1",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn tool_executor_returns_json_results() {
        let tool = Tool::new("weather", object_schema()).with_execute(|input, options| {
            ready(Ok(json!({
                "input": input,
                "toolCallId": options.tool_call_id
            })))
        });

        assert!(tool.is_executable());

        let result = poll_ready(
            tool.execute(
                json!({
                    "city": "Brisbane"
                }),
                ToolExecutionOptions::new("call-1", Vec::new()),
            )
            .expect("tool has an executor"),
        )
        .expect("tool execution succeeds");

        assert_eq!(
            result,
            json!({
                "input": {
                    "city": "Brisbane"
                },
                "toolCallId": "call-1"
            })
        );
    }

    #[test]
    fn tool_execution_error_retains_message() {
        let error = ToolExecutionError::new("Tool failed.");

        assert_eq!(error.message(), "Tool failed.");
        assert_eq!(error.to_string(), "Tool failed.");
        assert_eq!(
            serde_json::to_value(error).expect("tool execution error serializes"),
            json!({
                "message": "Tool failed."
            })
        );
    }

    #[test]
    fn load_api_key_returns_explicit_value_without_reading_environment() {
        let api_key = load_api_key(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider")
                .with_api_key("explicit-key"),
        )
        .expect("explicit API key loads");

        assert_eq!(api_key, "explicit-key");
    }

    #[test]
    fn load_api_key_reads_environment_when_value_is_missing() {
        let api_key = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider"),
            |name| {
                assert_eq!(name, "AI_SDK_RUST_TEST_API_KEY");
                Ok("env-key".to_string())
            },
        )
        .expect("environment API key loads");

        assert_eq!(api_key, "env-key");
    }

    #[test]
    fn load_api_key_reports_upstream_missing_message() {
        let error = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider")
                .with_api_key_parameter_name("token"),
            |_| Err(VarError::NotPresent),
        )
        .expect_err("missing API key is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider API key is missing. Pass it using the 'token' parameter or the AI_SDK_RUST_TEST_API_KEY environment variable."
        );
    }

    #[test]
    fn load_api_key_reports_non_unicode_environment_values_as_non_strings() {
        let error = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider"),
            |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode API key is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider API key must be a string. The value of the AI_SDK_RUST_TEST_API_KEY environment variable is not a string."
        );
    }

    #[test]
    fn load_setting_returns_explicit_value_without_reading_environment() {
        let setting = load_setting(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider")
                .with_setting_value("https://example.com"),
        )
        .expect("explicit setting loads");

        assert_eq!(setting, "https://example.com");
    }

    #[test]
    fn load_setting_reads_environment_when_value_is_missing() {
        let setting = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |name| {
                assert_eq!(name, "AI_SDK_RUST_TEST_BASE_URL");
                Ok("https://env.example.com".to_string())
            },
        )
        .expect("environment setting loads");

        assert_eq!(setting, "https://env.example.com");
    }

    #[test]
    fn load_setting_reports_upstream_missing_message() {
        let error = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |_| Err(VarError::NotPresent),
        )
        .expect_err("missing setting is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider setting is missing. Pass it using the 'baseURL' parameter or the AI_SDK_RUST_TEST_BASE_URL environment variable."
        );
    }

    #[test]
    fn load_setting_reports_non_unicode_environment_values_as_non_strings() {
        let error = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode setting is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider setting must be a string. The value of the AI_SDK_RUST_TEST_BASE_URL environment variable is not a string."
        );
    }

    #[test]
    fn load_optional_setting_prefers_explicit_value() {
        let setting = load_optional_setting_with_env(
            LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL")
                .with_setting_value("explicit"),
            |_| panic!("environment should not be read when explicit setting is present"),
        );

        assert_eq!(setting.as_deref(), Some("explicit"));
    }

    #[test]
    fn load_optional_setting_reads_environment_when_value_is_missing() {
        let setting = load_optional_setting_with_env(
            LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
            |_| Ok("env-setting".to_string()),
        );

        assert_eq!(setting.as_deref(), Some("env-setting"));
    }

    #[test]
    fn load_optional_setting_returns_none_for_missing_or_non_unicode_environment_values() {
        assert_eq!(
            load_optional_setting_with_env(
                LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
                |_| Err(VarError::NotPresent),
            ),
            None
        );

        assert_eq!(
            load_optional_setting_with_env(
                LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
                |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
            ),
            None
        );
    }

    #[test]
    fn media_type_to_extension_maps_common_audio_media_types() {
        for (media_type, expected_extension) in [
            ("audio/mpeg", "mp3"),
            ("audio/mp3", "mp3"),
            ("audio/wav", "wav"),
            ("audio/x-wav", "wav"),
            ("audio/webm", "webm"),
            ("audio/ogg", "ogg"),
            ("audio/opus", "ogg"),
            ("audio/mp4", "m4a"),
            ("audio/x-m4a", "m4a"),
            ("audio/flac", "flac"),
            ("audio/aac", "aac"),
        ] {
            assert_eq!(
                media_type_to_extension(media_type),
                expected_extension,
                "{media_type} maps to {expected_extension}"
            );
        }
    }

    #[test]
    fn media_type_to_extension_lowercases_subtypes_and_handles_invalid_values() {
        assert_eq!(media_type_to_extension("AUDIO/MPEG"), "mp3");
        assert_eq!(media_type_to_extension("AUDIO/MP3"), "mp3");
        assert_eq!(media_type_to_extension("nope"), "");
    }

    #[test]
    fn strip_file_extension_strips_single_extension() {
        assert_eq!(strip_file_extension("report.pdf"), "report");
    }

    #[test]
    fn strip_file_extension_returns_input_when_there_is_no_dot() {
        assert_eq!(strip_file_extension("report"), "report");
    }

    #[test]
    fn strip_file_extension_strips_all_extension_segments() {
        assert_eq!(strip_file_extension("archive.tar.gz"), "archive");
    }

    #[test]
    fn strip_file_extension_strips_a_trailing_dot() {
        assert_eq!(strip_file_extension("report."), "report");
    }

    #[test]
    fn without_trailing_slash_removes_one_trailing_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com/")),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn without_trailing_slash_preserves_values_without_trailing_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com/v1")),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn without_trailing_slash_preserves_missing_url() {
        assert_eq!(without_trailing_slash(None), None);
    }

    #[test]
    fn without_trailing_slash_only_removes_the_final_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com//")),
            Some("https://api.example.com/")
        );
    }

    #[test]
    fn resolve_provider_reference_returns_provider_specific_identifier() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz".to_string()),
            ("openai".to_string(), "file-abc".to_string()),
        ]))
        .expect("provider reference is valid");

        assert_eq!(
            resolve_provider_reference(&reference, "openai").expect("openai reference is present"),
            "file-abc"
        );
        assert_eq!(
            resolve_provider_reference(&reference, "anthropic")
                .expect("anthropic reference is present"),
            "file-xyz"
        );
    }

    #[test]
    fn resolve_provider_reference_reports_missing_provider_context() {
        let reference = ProviderReference::try_from(BTreeMap::from([(
            "anthropic".to_string(),
            "file-xyz".to_string(),
        )]))
        .expect("provider reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("missing provider reference is rejected");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }

    #[test]
    fn resolve_provider_reference_rejects_empty_references() {
        let reference =
            ProviderReference::try_from(BTreeMap::new()).expect("empty reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("empty reference cannot satisfy provider lookup");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }
}
