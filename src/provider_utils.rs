use std::collections::BTreeMap;
use std::env::{self, VarError};
use std::fmt;
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelStreamPart,
    LanguageModelSupportedUrls, LanguageModelSystemMessage, LanguageModelTool,
    LanguageModelToolCall, LanguageModelToolInputDelta, LanguageModelToolInputEnd,
    LanguageModelToolInputExample, LanguageModelToolInputStart,
};
use crate::provider::{
    ApiCallError, EmptyResponseBodyError, InvalidArgumentError, InvalidResponseDataError,
    JsonParseError, LoadApiKeyError, LoadSettingError, ProviderMetadata, ProviderOptions,
    TypeValidationContext, TypeValidationError, UnsupportedFunctionalityError,
};
use crate::warning::Warning;

pub use crate::provider::get_error_message;

const DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX: &str = "JSON schema:";
const DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX: &str =
    "You MUST answer with a JSON object that matches the JSON schema above.";
const DEFAULT_JSON_INSTRUCTION_SUFFIX: &str = "You MUST answer with JSON.";
const FETCH_FAILED_ERROR_MESSAGES: [&str; 2] = ["fetch failed", "failed to fetch"];
const BUN_NETWORK_ERROR_CODES: [&str; 7] = [
    "ConnectionRefused",
    "ConnectionClosed",
    "FailedToOpenSocket",
    "ECONNRESET",
    "ECONNREFUSED",
    "ETIMEDOUT",
    "EPIPE",
];
const DEFAULT_ID_ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const DEFAULT_ID_SEPARATOR: &str = "-";
const DEFAULT_ID_SIZE: usize = 16;
static ID_GENERATOR_COUNTER: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);

/// Default maximum response download size used by upstream provider-utils: 2 GiB.
pub const DEFAULT_MAX_DOWNLOAD_SIZE: usize = 2 * 1024 * 1024 * 1024;

struct DelayState {
    completed: bool,
    waker: Option<Waker>,
}

struct DelayFuture {
    delay: Option<Duration>,
    state: Option<Arc<Mutex<DelayState>>>,
}

impl DelayFuture {
    fn new(delay_in_ms: Option<i64>) -> Self {
        Self {
            delay: delay_in_ms.map(|delay| Duration::from_millis(delay.max(0) as u64)),
            state: None,
        }
    }
}

impl Future for DelayFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(delay) = self.delay else {
            return Poll::Ready(());
        };

        if let Some(state) = &self.state {
            let mut state = state.lock().expect("delay state mutex is not poisoned");
            if state.completed {
                Poll::Ready(())
            } else {
                state.waker = Some(context.waker().clone());
                Poll::Pending
            }
        } else {
            let state = Arc::new(Mutex::new(DelayState {
                completed: false,
                waker: Some(context.waker().clone()),
            }));
            let sleeper_state = Arc::clone(&state);

            let _sleeper = std::thread::spawn(move || {
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }

                let waker = {
                    let mut state = sleeper_state
                        .lock()
                        .expect("delay state mutex is not poisoned");
                    state.completed = true;
                    state.waker.take()
                };

                if let Some(waker) = waker {
                    waker.wake();
                }
            });

            self.state = Some(state);
            Poll::Pending
        }
    }
}

/// Creates a future that resolves after a delay in milliseconds.
///
/// This mirrors upstream provider-utils `delay`: `None` resolves immediately,
/// while numeric delays use timer-like deferred completion. JavaScript
/// `AbortSignal` cancellation is intentionally omitted from the Rust boundary.
pub fn delay(delay_in_ms: Option<i64>) -> impl Future<Output = ()> {
    DelayFuture::new(delay_in_ms)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_true() -> bool {
    true
}

fn is_streaming_tool_call_type_validation_none(value: &StreamingToolCallTypeValidation) -> bool {
    matches!(value, StreamingToolCallTypeValidation::None)
}

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

/// Options for creating upstream-style provider-utils ID generators.
///
/// Upstream `createIdGenerator` creates non-cryptographic random IDs with an
/// optional prefix. Rust represents the generator configuration as explicit
/// data, while [`create_id_generator`] returns the callable generator.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdGeneratorOptions {
    /// Optional ID prefix. When present, generated IDs are
    /// `{prefix}{separator}{random_part}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// Separator between the prefix and random part.
    #[serde(default = "default_id_separator")]
    pub separator: String,

    /// Length of the random ID part.
    #[serde(default = "default_id_size")]
    pub size: usize,

    /// Alphabet used for the random ID part.
    #[serde(default = "default_id_alphabet")]
    pub alphabet: String,
}

impl Default for IdGeneratorOptions {
    fn default() -> Self {
        Self {
            prefix: None,
            separator: default_id_separator(),
            size: DEFAULT_ID_SIZE,
            alphabet: default_id_alphabet(),
        }
    }
}

impl IdGeneratorOptions {
    /// Creates ID generator options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the optional generated ID prefix.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the separator between the prefix and random part.
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Sets the length of the random ID part.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Sets the alphabet used for the random ID part.
    pub fn with_alphabet(mut self, alphabet: impl Into<String>) -> Self {
        self.alphabet = alphabet.into();
        self
    }
}

/// Serialized provider model options for workflow step boundaries.
///
/// This mirrors the upstream `serializeModelOptions` result shape: the model
/// identifier is preserved and the config contains only JSON-serializable
/// entries. Rust callers represent non-serializable JavaScript values such as
/// functions as `None` when using [`serialize_model_options`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedModelOptions {
    /// Provider model identifier.
    pub model_id: String,

    /// JSON-serializable provider configuration.
    pub config: JsonObject,
}

impl SerializedModelOptions {
    /// Creates serialized model options.
    pub fn new(model_id: impl Into<String>, config: JsonObject) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }
}

/// Scalar value stored in dependency-free multipart form data.
///
/// Upstream `convertToFormData` appends JavaScript strings and `Blob`s to a
/// browser `FormData` object. Rust keeps the same logical boundary as ordered
/// text or byte entries so HTTP adapters can choose their multipart encoder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FormDataValue {
    /// Text form value.
    Text {
        /// Text content for this form field.
        value: String,
    },

    /// Binary form value.
    Bytes {
        /// Raw bytes for this form field.
        value: Vec<u8>,
    },
}

impl FormDataValue {
    /// Creates a text form value.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    /// Creates a binary form value.
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            value: value.into(),
        }
    }
}

impl From<String> for FormDataValue {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<&str> for FormDataValue {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<Vec<u8>> for FormDataValue {
    fn from(value: Vec<u8>) -> Self {
        Self::bytes(value)
    }
}

/// Input value accepted by [`convert_to_form_data`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FormDataInputValue {
    /// Single text value.
    Text {
        /// Text content for this form field.
        value: String,
    },

    /// Single binary value.
    Bytes {
        /// Raw bytes for this form field.
        value: Vec<u8>,
    },

    /// Repeated values for this form field.
    Array {
        /// Ordered values for this form field.
        values: Vec<FormDataValue>,
    },
}

impl FormDataInputValue {
    /// Creates a single text input value.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    /// Creates a single binary input value.
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            value: value.into(),
        }
    }

    /// Creates repeated input values.
    pub fn array(values: Vec<FormDataValue>) -> Self {
        Self::Array { values }
    }

    fn into_values(self) -> Vec<FormDataValue> {
        match self {
            Self::Text { value } => vec![FormDataValue::text(value)],
            Self::Bytes { value } => vec![FormDataValue::bytes(value)],
            Self::Array { values } => values,
        }
    }
}

impl From<FormDataValue> for FormDataInputValue {
    fn from(value: FormDataValue) -> Self {
        match value {
            FormDataValue::Text { value } => Self::text(value),
            FormDataValue::Bytes { value } => Self::bytes(value),
        }
    }
}

impl From<String> for FormDataInputValue {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<&str> for FormDataInputValue {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<Vec<u8>> for FormDataInputValue {
    fn from(value: Vec<u8>) -> Self {
        Self::bytes(value)
    }
}

/// One ordered multipart form data entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDataEntry {
    /// Field name for this entry.
    pub name: String,

    /// Field value.
    pub value: FormDataValue,
}

impl FormDataEntry {
    /// Creates a form data entry.
    pub fn new(name: impl Into<String>, value: FormDataValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// Dependency-free representation of ordered multipart form data entries.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormData {
    /// Ordered form entries.
    pub entries: Vec<FormDataEntry>,
}

impl FormData {
    /// Creates empty form data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a form entry.
    pub fn append(&mut self, name: impl Into<String>, value: FormDataValue) {
        self.entries.push(FormDataEntry::new(name, value));
    }

    /// Returns true when at least one entry exists for the field name.
    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    /// Returns the first value for the field name.
    pub fn get(&self, name: &str) -> Option<&FormDataValue> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| &entry.value)
    }

    /// Returns all values for the field name in append order.
    pub fn get_all(&self, name: &str) -> Vec<&FormDataValue> {
        self.entries
            .iter()
            .filter(|entry| entry.name == name)
            .map(|entry| &entry.value)
            .collect()
    }
}

/// Options for [`convert_to_form_data`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertToFormDataOptions {
    /// Whether multi-value array fields use the upstream `[]` suffix.
    #[serde(default = "default_true")]
    pub use_array_brackets: bool,
}

impl Default for ConvertToFormDataOptions {
    fn default() -> Self {
        Self {
            use_array_brackets: true,
        }
    }
}

impl ConvertToFormDataOptions {
    /// Creates options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether multi-value array fields use the upstream `[]` suffix.
    pub fn with_use_array_brackets(mut self, use_array_brackets: bool) -> Self {
        self.use_array_brackets = use_array_brackets;
        self
    }
}

/// Converts an input map to ordered multipart form data entries.
///
/// This mirrors upstream `convertToFormData`: missing values are skipped,
/// empty arrays add no entries, one-element arrays use the original key, and
/// multi-element arrays use a `[]` suffix unless disabled by options.
pub fn convert_to_form_data(
    input: impl IntoIterator<Item = (String, Option<FormDataInputValue>)>,
    options: ConvertToFormDataOptions,
) -> FormData {
    let mut form_data = FormData::new();

    for (key, value) in input {
        let Some(value) = value else {
            continue;
        };

        let values = value.into_values();
        let form_key = if values.len() > 1 && options.use_array_brackets {
            format!("{key}[]")
        } else {
            key
        };

        for value in values {
            form_data.append(form_key.clone(), value);
        }
    }

    form_data
}

/// Options for downloading a URL into a dependency-free blob value.
///
/// Upstream `downloadBlob` accepts a URL and an optional `maxBytes` value.
/// Rust omits JavaScript-only `AbortSignal` and lets callers inject the HTTP
/// transport in [`download_blob`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBlobOptions {
    /// URL to download.
    pub url: String,

    /// Maximum accepted response body size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

impl DownloadBlobOptions {
    /// Creates download options for a URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_bytes: None,
        }
    }

    /// Sets the maximum accepted response body size in bytes.
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }
}

/// HTTP response data supplied to [`download_blob`] by an adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBlobResponse {
    /// HTTP response status code.
    pub status_code: u16,

    /// HTTP response status text.
    pub status_text: String,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub headers: Headers,

    /// Downloaded response body bytes. Missing bodies are treated as empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,

    /// Final URL after an HTTP redirect, when the adapter followed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
}

impl DownloadBlobResponse {
    /// Creates a response without a body.
    pub fn new(status_code: u16, status_text: impl Into<String>) -> Self {
        Self {
            status_code,
            status_text: status_text.into(),
            headers: Headers::new(),
            body: None,
            final_url: None,
        }
    }

    /// Creates a response with byte body content.
    pub fn bytes(
        status_code: u16,
        status_text: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self::new(status_code, status_text).with_body(body)
    }

    /// Adds response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Adds byte body content.
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Marks the response as redirected to a final URL.
    pub fn with_final_url(mut self, final_url: impl Into<String>) -> Self {
        self.final_url = Some(final_url.into());
        self
    }

    fn is_success_status(&self) -> bool {
        (200..=299).contains(&self.status_code)
    }
}

/// Dependency-free blob returned by [`download_blob`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedBlob {
    /// Downloaded bytes.
    pub data: Vec<u8>,

    /// Response media type from the `content-type` header, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

impl DownloadedBlob {
    /// Creates a downloaded blob from bytes.
    pub fn new(data: impl Into<Vec<u8>>) -> Self {
        Self {
            data: data.into(),
            media_type: None,
        }
    }

    /// Sets the response media type.
    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }
}

/// Validation mode for OpenAI-compatible streaming tool-call deltas.
///
/// Upstream `StreamingToolCallTracker` accepts `none`, `if-present`, or
/// `required` to control whether the delta `type` field must be `function`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamingToolCallTypeValidation {
    /// Do not validate the delta `type` field.
    #[default]
    None,

    /// Validate the delta `type` field only when it is present.
    IfPresent,

    /// Require the delta `type` field to be exactly `function`.
    Required,
}

/// Options for a [`StreamingToolCallTracker`].
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingToolCallTrackerOptions {
    /// How to validate the `type` field on newly observed tool-call deltas.
    #[serde(
        default,
        skip_serializing_if = "is_streaming_tool_call_type_validation_none"
    )]
    pub type_validation: StreamingToolCallTypeValidation,
}

impl StreamingToolCallTrackerOptions {
    /// Creates streaming tool-call tracker options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the delta `type` validation mode.
    pub fn with_type_validation(
        mut self,
        type_validation: StreamingToolCallTypeValidation,
    ) -> Self {
        self.type_validation = type_validation;
        self
    }
}

/// Function payload carried by an OpenAI-compatible streaming tool-call delta.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingToolCallDeltaFunction {
    /// Name of the function-style tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Incremental JSON input text for the tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

impl StreamingToolCallDeltaFunction {
    /// Creates an empty streaming tool-call function payload.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tool function name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the incremental JSON arguments text.
    pub fn with_arguments(mut self, arguments: impl Into<String>) -> Self {
        self.arguments = Some(arguments.into());
        self
    }
}

/// Minimal OpenAI-compatible streaming tool-call delta.
///
/// The upstream tracker accepts provider-specific delta extensions. Rust keeps
/// those extensions in [`StreamingToolCallDelta::extra`] so provider adapters can
/// preserve metadata without forcing a provider-specific type into this crate.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingToolCallDelta {
    /// Tool-call index in the provider stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Provider-supplied tool-call identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Provider-supplied delta type, expected to be `function` depending on validation mode.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,

    /// Function tool-call details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<StreamingToolCallDeltaFunction>,

    /// Provider-specific extension fields preserved from the source delta.
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

impl StreamingToolCallDelta {
    /// Creates an empty streaming tool-call delta.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the provider stream index for the delta.
    pub fn with_index(mut self, index: usize) -> Self {
        self.index = Some(index);
        self
    }

    /// Sets the provider tool-call identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the provider delta type.
    pub fn with_type(mut self, call_type: impl Into<String>) -> Self {
        self.call_type = Some(call_type.into());
        self
    }

    /// Sets the function payload for the delta.
    pub fn with_function(mut self, function: StreamingToolCallDeltaFunction) -> Self {
        self.function = Some(function);
        self
    }

    /// Adds a provider-specific extension value.
    pub fn with_extra_value(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extra.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackedStreamingToolCall {
    id: Option<String>,
    function_name: String,
    arguments: String,
    has_finished: bool,
    metadata: Option<ProviderMetadata>,
}

type StreamingToolCallGenerateId = dyn Fn() -> String + Send + Sync + 'static;
type StreamingToolCallExtractMetadata =
    dyn Fn(&StreamingToolCallDelta) -> Option<ProviderMetadata> + Send + Sync + 'static;
type StreamingToolCallBuildProviderMetadata =
    dyn Fn(Option<&ProviderMetadata>) -> Option<ProviderMetadata> + Send + Sync + 'static;

/// Tracks streaming tool-call state across multiple OpenAI-compatible deltas.
///
/// Upstream uses a stream controller and enqueues language model stream parts.
/// This Rust boundary returns the parts produced by each processed delta, which
/// keeps the helper dependency-free and easy to compose with any async stream.
#[derive(Clone)]
pub struct StreamingToolCallTracker {
    tool_calls: Vec<Option<TrackedStreamingToolCall>>,
    generate_id: Arc<StreamingToolCallGenerateId>,
    type_validation: StreamingToolCallTypeValidation,
    extract_metadata: Option<Arc<StreamingToolCallExtractMetadata>>,
    build_tool_call_provider_metadata: Option<Arc<StreamingToolCallBuildProviderMetadata>>,
}

impl Default for StreamingToolCallTracker {
    fn default() -> Self {
        Self::from_options(StreamingToolCallTrackerOptions::default())
    }
}

impl StreamingToolCallTracker {
    /// Creates a streaming tool-call tracker with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a streaming tool-call tracker from explicit options.
    pub fn from_options(options: StreamingToolCallTrackerOptions) -> Self {
        Self {
            tool_calls: Vec::new(),
            generate_id: Arc::new(generate_id),
            type_validation: options.type_validation,
            extract_metadata: None,
            build_tool_call_provider_metadata: None,
        }
    }

    /// Sets the ID generator used by the upstream fallback path.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Arc::new(generate_id);
        self
    }

    /// Extracts provider metadata from a newly observed tool-call delta.
    pub fn with_extract_metadata<F>(mut self, extract_metadata: F) -> Self
    where
        F: Fn(&StreamingToolCallDelta) -> Option<ProviderMetadata> + Send + Sync + 'static,
    {
        self.extract_metadata = Some(Arc::new(extract_metadata));
        self
    }

    /// Builds provider metadata for the final `tool-call` stream part.
    pub fn with_tool_call_provider_metadata<F>(mut self, build_provider_metadata: F) -> Self
    where
        F: Fn(Option<&ProviderMetadata>) -> Option<ProviderMetadata> + Send + Sync + 'static,
    {
        self.build_tool_call_provider_metadata = Some(Arc::new(build_provider_metadata));
        self
    }

    /// Processes one provider tool-call delta and returns emitted stream parts.
    pub fn process_delta(
        &mut self,
        delta: StreamingToolCallDelta,
    ) -> Result<Vec<LanguageModelStreamPart>, InvalidResponseDataError> {
        let index = delta.index.unwrap_or(self.tool_calls.len());

        if self
            .tool_calls
            .get(index)
            .and_then(Option::as_ref)
            .is_some()
        {
            self.process_existing_tool_call(index, &delta)
        } else {
            self.process_new_tool_call(index, &delta)
        }
    }

    /// Finalizes unfinished tool calls and returns the emitted stream parts.
    pub fn flush(&mut self) -> Vec<LanguageModelStreamPart> {
        let generate_id = Arc::clone(&self.generate_id);
        let build_provider_metadata = self.build_tool_call_provider_metadata.clone();
        let mut parts = Vec::new();

        for tool_call in self.tool_calls.iter_mut().flatten() {
            if !tool_call.has_finished {
                finish_streaming_tool_call(
                    tool_call,
                    &generate_id,
                    build_provider_metadata.as_deref(),
                    &mut parts,
                );
            }
        }

        parts
    }

    fn process_new_tool_call(
        &mut self,
        index: usize,
        delta: &StreamingToolCallDelta,
    ) -> Result<Vec<LanguageModelStreamPart>, InvalidResponseDataError> {
        self.validate_delta_type(delta)?;

        let id = delta.id.clone().ok_or_else(|| {
            invalid_streaming_tool_call_delta_error(delta, "Expected 'id' to be a string.")
        })?;
        let function = delta.function.as_ref();
        let function_name = function
            .and_then(|function| function.name.clone())
            .ok_or_else(|| {
                invalid_streaming_tool_call_delta_error(
                    delta,
                    "Expected 'function.name' to be a string.",
                )
            })?;
        let arguments = function
            .and_then(|function| function.arguments.clone())
            .unwrap_or_default();

        let mut parts = vec![LanguageModelStreamPart::ToolInputStart(
            LanguageModelToolInputStart::new(id.clone(), function_name.clone()),
        )];
        let metadata = self
            .extract_metadata
            .as_ref()
            .and_then(|extract_metadata| extract_metadata(delta));

        if self.tool_calls.len() <= index {
            self.tool_calls.resize_with(index + 1, || None);
        }

        self.tool_calls[index] = Some(TrackedStreamingToolCall {
            id: Some(id),
            function_name,
            arguments: arguments.clone(),
            has_finished: false,
            metadata,
        });

        if !arguments.is_empty() {
            let tool_call = self.tool_calls[index]
                .as_ref()
                .expect("new tool call was inserted");
            parts.push(LanguageModelStreamPart::ToolInputDelta(
                LanguageModelToolInputDelta::new(
                    tool_call.id.as_deref().unwrap_or_default(),
                    arguments.clone(),
                ),
            ));
        }

        if is_parsable_json(&arguments) {
            let generate_id = Arc::clone(&self.generate_id);
            let build_provider_metadata = self.build_tool_call_provider_metadata.clone();
            let tool_call = self.tool_calls[index]
                .as_mut()
                .expect("new tool call was inserted");
            finish_streaming_tool_call(
                tool_call,
                &generate_id,
                build_provider_metadata.as_deref(),
                &mut parts,
            );
        }

        Ok(parts)
    }

    fn process_existing_tool_call(
        &mut self,
        index: usize,
        delta: &StreamingToolCallDelta,
    ) -> Result<Vec<LanguageModelStreamPart>, InvalidResponseDataError> {
        let Some(tool_call) = self.tool_calls.get_mut(index).and_then(Option::as_mut) else {
            return Ok(Vec::new());
        };

        if tool_call.has_finished {
            return Ok(Vec::new());
        }

        let mut parts = Vec::new();

        if let Some(arguments) = delta
            .function
            .as_ref()
            .and_then(|function| function.arguments.as_ref())
        {
            tool_call.arguments.push_str(arguments);
            parts.push(LanguageModelStreamPart::ToolInputDelta(
                LanguageModelToolInputDelta::new(
                    tool_call.id.as_deref().unwrap_or_default(),
                    arguments.clone(),
                ),
            ));
        }

        if is_parsable_json(&tool_call.arguments) {
            let generate_id = Arc::clone(&self.generate_id);
            let build_provider_metadata = self.build_tool_call_provider_metadata.clone();
            finish_streaming_tool_call(
                tool_call,
                &generate_id,
                build_provider_metadata.as_deref(),
                &mut parts,
            );
        }

        Ok(parts)
    }

    fn validate_delta_type(
        &self,
        delta: &StreamingToolCallDelta,
    ) -> Result<(), InvalidResponseDataError> {
        match self.type_validation {
            StreamingToolCallTypeValidation::None => Ok(()),
            StreamingToolCallTypeValidation::IfPresent => {
                if delta
                    .call_type
                    .as_deref()
                    .is_some_and(|call_type| call_type != "function")
                {
                    Err(invalid_streaming_tool_call_delta_error(
                        delta,
                        "Expected 'function' type.",
                    ))
                } else {
                    Ok(())
                }
            }
            StreamingToolCallTypeValidation::Required => {
                if delta.call_type.as_deref() == Some("function") {
                    Ok(())
                } else {
                    Err(invalid_streaming_tool_call_delta_error(
                        delta,
                        "Expected 'function' type.",
                    ))
                }
            }
        }
    }
}

fn finish_streaming_tool_call(
    tool_call: &mut TrackedStreamingToolCall,
    generate_id: &Arc<StreamingToolCallGenerateId>,
    build_provider_metadata: Option<&StreamingToolCallBuildProviderMetadata>,
    parts: &mut Vec<LanguageModelStreamPart>,
) {
    let id = tool_call
        .id
        .clone()
        .unwrap_or_else(|| (generate_id.as_ref())());

    parts.push(LanguageModelStreamPart::ToolInputEnd(
        LanguageModelToolInputEnd::new(id.clone()),
    ));

    let provider_metadata =
        build_provider_metadata.and_then(|build| build(tool_call.metadata.as_ref()));
    let mut tool_call_part = LanguageModelToolCall::new(
        id,
        tool_call.function_name.clone(),
        tool_call.arguments.clone(),
    );

    if let Some(provider_metadata) = provider_metadata {
        tool_call_part = tool_call_part.with_provider_metadata(provider_metadata);
    }

    parts.push(LanguageModelStreamPart::ToolCall(tool_call_part));
    tool_call.has_finished = true;
}

fn invalid_streaming_tool_call_delta_error(
    delta: &StreamingToolCallDelta,
    message: &'static str,
) -> InvalidResponseDataError {
    InvalidResponseDataError::with_message(
        serde_json::to_value(delta).expect("streaming tool-call deltas serialize"),
        message,
    )
}

fn default_id_alphabet() -> String {
    DEFAULT_ID_ALPHABET.to_string()
}

fn default_id_separator() -> String {
    DEFAULT_ID_SEPARATOR.to_string()
}

const fn default_id_size() -> usize {
    DEFAULT_ID_SIZE
}

/// Runtime indicators used to build the provider-utils user-agent suffix.
///
/// Upstream `getRuntimeEnvironmentUserAgent` probes JavaScript globals in a
/// fixed order. Rust callers can supply equivalent indicators explicitly while
/// the default unknown environment maps to the upstream fallback.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEnvironment {
    /// Whether a browser-like `window` global is present.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_window: bool,

    /// Browser, worker, Deno, Bun, or Node >= 21.1 navigator user agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigator_user_agent: Option<String>,

    /// Node.js `process.version` value for Node runtimes without navigator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_version: Option<String>,

    /// Whether Vercel Edge Runtime is present.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_edge_runtime: bool,
}

impl RuntimeEnvironment {
    /// Creates an unknown runtime environment.
    pub const fn unknown() -> Self {
        Self {
            has_window: false,
            navigator_user_agent: None,
            node_version: None,
            has_edge_runtime: false,
        }
    }

    /// Returns whether this environment maps to the upstream unknown runtime.
    pub fn is_unknown(&self) -> bool {
        !self.has_window
            && self.navigator_user_agent.is_none()
            && self.node_version.is_none()
            && !self.has_edge_runtime
    }

    /// Creates a browser runtime environment.
    pub const fn browser() -> Self {
        Self {
            has_window: true,
            navigator_user_agent: None,
            node_version: None,
            has_edge_runtime: false,
        }
    }

    /// Creates a navigator-backed runtime environment.
    pub fn navigator_user_agent(user_agent: impl Into<String>) -> Self {
        Self {
            has_window: false,
            navigator_user_agent: Some(user_agent.into()),
            node_version: None,
            has_edge_runtime: false,
        }
    }

    /// Creates a Node.js runtime environment.
    pub fn node_js(version: impl Into<String>) -> Self {
        Self {
            has_window: false,
            navigator_user_agent: None,
            node_version: Some(version.into()),
            has_edge_runtime: false,
        }
    }

    /// Creates a Vercel Edge runtime environment.
    pub const fn vercel_edge() -> Self {
        Self {
            has_window: false,
            navigator_user_agent: None,
            node_version: None,
            has_edge_runtime: true,
        }
    }
}

/// Runtime-independent fetch error information for request error normalization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchErrorInfo {
    /// JavaScript-style error name, when the HTTP layer exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    /// Human-readable error message.
    message: String,

    /// Runtime-specific network error code, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,

    /// Message from the wrapped error cause, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cause_message: Option<String>,
}

impl FetchErrorInfo {
    /// Creates fetch error information with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            name: None,
            message: message.into(),
            code: None,
            cause_message: None,
        }
    }

    /// Sets the JavaScript-style error name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the runtime-specific network error code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Sets the wrapped cause message.
    pub fn with_cause_message(mut self, cause_message: impl Into<String>) -> Self {
        self.cause_message = Some(cause_message.into());
        self
    }

    /// Returns the JavaScript-style error name, when available.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the runtime-specific network error code, when available.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Returns the wrapped cause message, when available.
    pub fn cause_message(&self) -> Option<&str> {
        self.cause_message.as_deref()
    }
}

/// Result of normalizing a lower-level fetch error.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum HandledFetchError {
    /// The original error should be propagated unchanged.
    Original {
        /// Original fetch error information.
        error: FetchErrorInfo,
    },

    /// The fetch error should be surfaced as an API-call error.
    ApiCall {
        /// Normalized API-call error.
        error: Box<ApiCallError>,
    },
}

impl HandledFetchError {
    /// Returns the normalized API-call error when one was created.
    pub fn api_call_error(&self) -> Option<&ApiCallError> {
        match self {
            Self::Original { .. } => None,
            Self::ApiCall { error } => Some(error),
        }
    }

    /// Returns the original fetch error when it should be propagated unchanged.
    pub fn original_error(&self) -> Option<&FetchErrorInfo> {
        match self {
            Self::Original { error } => Some(error),
            Self::ApiCall { .. } => None,
        }
    }
}

/// Options for a dependency-free upstream-style `getFromApi` request.
///
/// Rust callers provide an injected transport to [`get_from_api`], so this
/// struct only carries the request metadata that upstream prepares before
/// calling `fetch`: URL, optional headers, and the runtime used for the
/// provider-utils user-agent suffix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetFromApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,
}

impl GetFromApiOptions {
    /// Creates GET API request options for the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            environment: RuntimeEnvironment::unknown(),
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            environment,
        } = self;

        prepare_get_from_api_request(url, Some(headers), &environment)
    }
}

/// Options for a dependency-free upstream-style `postJsonToApi` request.
///
/// Rust callers provide an injected transport to [`post_json_to_api`], so this
/// struct carries the request metadata that upstream prepares before calling
/// `fetch`: URL, optional headers, a JSON body, and the runtime used for the
/// provider-utils user-agent suffix.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostJsonToApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// JSON request body. Upstream stringifies this value for the sent body and
    /// preserves the original value as `requestBodyValues`.
    pub body: JsonValue,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,
}

impl PostJsonToApiOptions {
    /// Creates JSON POST API request options for the given URL and body.
    pub fn new(url: impl Into<String>, body: impl Into<JsonValue>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            body: body.into(),
            environment: RuntimeEnvironment::unknown(),
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the JSON request body.
    pub fn with_body(mut self, body: impl Into<JsonValue>) -> Self {
        self.body = body.into();
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            body,
            environment,
        } = self;

        prepare_post_json_to_api_request(url, Some(headers), body, &environment)
    }
}

/// Options for a dependency-free upstream-style `postToApi` request.
///
/// Rust callers provide an injected transport to [`post_to_api`], so this
/// struct carries the request metadata that upstream prepares before calling
/// `fetch`: URL, optional headers, text or binary body content, body values for
/// response handlers, and the runtime used for the provider-utils user-agent
/// suffix. JavaScript-only `FormData` remains outside this Rust boundary.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostToApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// Request body content sent by the HTTP adapter.
    pub body: ProviderApiRequestBody,

    /// Values supplied to upstream response handlers as `requestBodyValues`.
    pub request_body_values: JsonValue,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,
}

impl PostToApiOptions {
    /// Creates generic POST API request options for the given URL, body, and
    /// response-handler body values.
    pub fn new(
        url: impl Into<String>,
        body: ProviderApiRequestBody,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            body,
            request_body_values: request_body_values.into(),
            environment: RuntimeEnvironment::unknown(),
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the request body content.
    pub fn with_body(mut self, body: ProviderApiRequestBody) -> Self {
        self.body = body;
        self
    }

    /// Sets the response-handler request body values.
    pub fn with_request_body_values(mut self, request_body_values: impl Into<JsonValue>) -> Self {
        self.request_body_values = request_body_values.into();
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            body,
            request_body_values,
            environment,
        } = self;

        prepare_post_to_api_request(url, Some(headers), body, request_body_values, &environment)
    }
}

/// HTTP method for provider API adapter requests.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProviderApiRequestMethod {
    /// Upstream `getFromApi` request method.
    Get,

    /// Upstream `postToApi` request method.
    Post,
}

/// Body content sent by provider API adapter requests.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ProviderApiRequestBody {
    /// Text request body content.
    #[serde(rename = "text")]
    Text {
        /// Text body content.
        content: String,
    },

    /// Binary request body content.
    #[serde(rename = "bytes")]
    Bytes {
        /// Binary body content.
        content: Vec<u8>,
    },
}

impl ProviderApiRequestBody {
    /// Creates text request body content.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            content: content.into(),
        }
    }

    /// Creates binary request body content.
    pub fn bytes(content: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            content: content.into(),
        }
    }

    /// Returns text request body content when this body is text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { content } => Some(content),
            Self::Bytes { .. } => None,
        }
    }

    /// Returns binary request body content when this body is bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Text { .. } => None,
            Self::Bytes { content } => Some(content),
        }
    }
}

/// Runtime-independent provider API request prepared for an HTTP adapter.
///
/// This is the Rust-native request boundary shared by upstream `getFromApi`
/// and `postToApi`: it preserves the fetch method, normalized headers,
/// optional request body content, and the values passed to response handlers as
/// `requestBodyValues`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiRequest {
    /// HTTP method used by the provider API request.
    pub method: ProviderApiRequestMethod,

    /// Provider API URL.
    pub url: String,

    /// Normalized request headers.
    #[serde(default)]
    pub headers: Headers,

    /// Request body content, when the request sends a body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<ProviderApiRequestBody>,

    /// Values supplied to upstream response handlers as `requestBodyValues`.
    pub request_body_values: JsonValue,
}

impl ProviderApiRequest {
    /// Creates a prepared provider API request.
    pub fn new(
        method: ProviderApiRequestMethod,
        url: impl Into<String>,
        headers: Headers,
        body: Option<ProviderApiRequestBody>,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self {
            method,
            url: url.into(),
            headers,
            body,
            request_body_values: request_body_values.into(),
        }
    }

    /// Creates a prepared GET provider API request with empty request body values.
    pub fn get(url: impl Into<String>, headers: Headers) -> Self {
        Self::new(
            ProviderApiRequestMethod::Get,
            url,
            headers,
            None,
            JsonValue::Object(JsonObject::new()),
        )
    }

    /// Creates a prepared POST provider API request.
    pub fn post(
        url: impl Into<String>,
        headers: Headers,
        body: ProviderApiRequestBody,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self::new(
            ProviderApiRequestMethod::Post,
            url,
            headers,
            Some(body),
            request_body_values,
        )
    }
}

/// Body content returned by provider API adapter responses.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ProviderApiResponseBody {
    /// Text response body content.
    #[serde(rename = "text")]
    Text {
        /// Text body content.
        content: String,
    },

    /// Binary response body content.
    #[serde(rename = "bytes")]
    Bytes {
        /// Binary body content.
        content: Vec<u8>,
    },
}

impl ProviderApiResponseBody {
    /// Creates text response body content.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            content: content.into(),
        }
    }

    /// Creates binary response body content.
    pub fn bytes(content: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            content: content.into(),
        }
    }

    /// Returns text response body content when this body is text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { content } => Some(content),
            Self::Bytes { .. } => None,
        }
    }

    /// Returns binary response body content when this body is bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Text { .. } => None,
            Self::Bytes { content } => Some(content),
        }
    }

    fn to_text(&self) -> String {
        match self {
            Self::Text { content } => content.clone(),
            Self::Bytes { content } => String::from_utf8_lossy(content).into_owned(),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Text { content } => content.as_bytes().to_vec(),
            Self::Bytes { content } => content.clone(),
        }
    }
}

/// Runtime-independent provider API response returned by an HTTP adapter.
///
/// This pairs with [`ProviderApiRequest`] as the dependency-free boundary for
/// upstream `getFromApi` and `postToApi`: HTTP adapters can supply status,
/// status text, extracted headers, and an already-read body without committing
/// this crate to a concrete HTTP client.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiResponse {
    /// HTTP response status code.
    pub status_code: u16,

    /// HTTP response status text.
    pub status_text: String,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub headers: Headers,

    /// Response body content, when one was available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<ProviderApiResponseBody>,
}

impl ProviderApiResponse {
    /// Creates a provider API response without a body.
    pub fn new(status_code: u16, status_text: impl Into<String>) -> Self {
        Self {
            status_code,
            status_text: status_text.into(),
            headers: Headers::new(),
            body: None,
        }
    }

    /// Creates a provider API response with a text body.
    pub fn text(status_code: u16, status_text: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(status_code, status_text).with_text_body(body)
    }

    /// Creates a provider API response with a binary body.
    pub fn bytes(
        status_code: u16,
        status_text: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self::new(status_code, status_text).with_bytes_body(body)
    }

    /// Adds response headers extracted from the response.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Adds text response body content.
    pub fn with_text_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(ProviderApiResponseBody::text(body));
        self
    }

    /// Adds binary response body content.
    pub fn with_bytes_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(ProviderApiResponseBody::bytes(body));
        self
    }

    /// Returns whether the status maps to upstream `Response.ok`.
    pub fn is_success_status(&self) -> bool {
        (200..=299).contains(&self.status_code)
    }

    /// Returns text response body content when this response has text content.
    pub fn text_body(&self) -> Option<&str> {
        self.body
            .as_ref()
            .and_then(ProviderApiResponseBody::as_text)
    }

    /// Returns binary response body content when this response has binary content.
    pub fn bytes_body(&self) -> Option<&[u8]> {
        self.body
            .as_ref()
            .and_then(ProviderApiResponseBody::as_bytes)
    }

    /// Builds inputs for [`create_status_code_error_response_handler`].
    pub fn status_code_error_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> StatusCodeErrorResponseHandlerOptions {
        StatusCodeErrorResponseHandlerOptions::new(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
            self.status_text.clone(),
            self.body_as_text(),
        )
        .with_response_headers(self.headers.clone())
    }

    /// Builds inputs for [`create_json_error_response_handler`].
    pub fn json_error_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> JsonErrorResponseHandlerOptions {
        JsonErrorResponseHandlerOptions::new(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
            self.status_text.clone(),
            self.body_as_text(),
        )
        .with_response_headers(self.headers.clone())
    }

    /// Builds inputs for [`create_json_response_handler`].
    pub fn json_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> JsonResponseHandlerOptions {
        JsonResponseHandlerOptions::new(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
            self.body_as_text(),
        )
        .with_response_headers(self.headers.clone())
    }

    /// Builds inputs for [`create_binary_response_handler`].
    pub fn binary_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> BinaryResponseHandlerOptions {
        let options = BinaryResponseHandlerOptions::empty(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
        )
        .with_response_headers(self.headers.clone());

        if let Some(body) = self.body.as_ref().map(ProviderApiResponseBody::to_bytes) {
            BinaryResponseHandlerOptions {
                response_body: Some(body),
                ..options
            }
        } else {
            options
        }
    }

    /// Builds inputs for [`create_event_source_response_handler`].
    pub fn event_source_response_handler_options(&self) -> EventSourceResponseHandlerOptions {
        let options = if let Some(body) = self.body.as_ref().map(ProviderApiResponseBody::to_bytes)
        {
            EventSourceResponseHandlerOptions::new(body)
        } else {
            EventSourceResponseHandlerOptions::empty()
        };

        options.with_response_headers(self.headers.clone())
    }

    fn body_as_text(&self) -> String {
        self.body
            .as_ref()
            .map_or_else(String::new, ProviderApiResponseBody::to_text)
    }
}

/// Error returned by provider API response handlers.
///
/// Upstream `getFromApi` and `postToApi` pass through API-call errors from
/// response handlers but wrap other response-handler failures in a new
/// [`ApiCallError`]. This Rust boundary makes that distinction explicit without
/// depending on JavaScript `Error`/`AbortSignal` runtime mechanics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ProviderApiResponseHandlerError {
    /// The handler already produced an API-call error that should be propagated.
    ApiCall {
        /// API-call error returned by the response handler.
        error: Box<ApiCallError>,
    },

    /// The handler failed for another reason and should be wrapped.
    Other {
        /// Human-readable handler failure message.
        message: String,
    },
}

impl ProviderApiResponseHandlerError {
    /// Creates an API-call handler error.
    pub fn api_call(error: ApiCallError) -> Self {
        Self::ApiCall {
            error: Box::new(error),
        }
    }

    /// Creates an API-call handler error from a boxed API-call error.
    pub fn boxed_api_call(error: Box<ApiCallError>) -> Self {
        Self::ApiCall { error }
    }

    /// Creates a non-API-call handler error.
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other {
            message: message.into(),
        }
    }

    /// Returns the API-call error when this failure should be propagated.
    pub fn api_call_error(&self) -> Option<&ApiCallError> {
        match self {
            Self::ApiCall { error } => Some(error),
            Self::Other { .. } => None,
        }
    }

    /// Returns the non-API-call handler failure message.
    pub fn other_message(&self) -> Option<&str> {
        match self {
            Self::ApiCall { .. } => None,
            Self::Other { message } => Some(message),
        }
    }
}

impl From<ApiCallError> for ProviderApiResponseHandlerError {
    fn from(error: ApiCallError) -> Self {
        Self::api_call(error)
    }
}

impl From<Box<ApiCallError>> for ProviderApiResponseHandlerError {
    fn from(error: Box<ApiCallError>) -> Self {
        Self::boxed_api_call(error)
    }
}

impl fmt::Display for ProviderApiResponseHandlerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiCall { error } => error.fmt(formatter),
            Self::Other { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProviderApiResponseHandlerError {}

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

/// Inputs for the event-source response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createEventSourceResponseHandler`, preserving response headers and a
/// byte-backed event stream without introducing a concrete HTTP client or async
/// stream dependency.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSourceResponseHandlerOptions {
    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw event-source response body bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<Vec<u8>>,
}

impl EventSourceResponseHandlerOptions {
    /// Creates event-source response handler options with a readable body.
    pub fn new(response_body: impl Into<Vec<u8>>) -> Self {
        Self {
            response_headers: Headers::new(),
            response_body: Some(response_body.into()),
        }
    }

    /// Creates event-source response handler options without a response body.
    pub fn empty() -> Self {
        Self {
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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionOptions {
    /// Identifier of the model tool call being executed.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool-specific context configured for the executed tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<JsonValue>,
}

impl ToolExecutionOptions {
    /// Creates tool execution options.
    pub fn new(tool_call_id: impl Into<String>, messages: LanguageModelPrompt) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
            context: None,
        }
    }

    /// Sets the context for the executed tool.
    pub fn with_context(mut self, context: impl Into<JsonValue>) -> Self {
        self.context = Some(context.into());
        self
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum ToolKind {
    Function,
    Dynamic,
    Provider {
        id: String,
        args: JsonObject,
        provider_executed: bool,
        output_schema: Option<JsonSchema>,
        supports_deferred_results: Option<bool>,
    },
}

/// Factory for creating provider-defined tools from shared provider metadata.
///
/// This mirrors upstream `createProviderDefinedToolFactory`: the factory owns
/// the provider tool id and schemas, while Rust callers supply the model-call
/// tool name explicitly because there is no JavaScript object key to infer it
/// from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefinedToolFactory {
    /// Provider tool identifier, typically `<provider-id>.<unique-tool-name>`.
    pub id: String,

    /// JSON Schema 7 object describing the provider tool input.
    pub input_schema: JsonSchema,

    /// Optional JSON Schema 7 object describing the provider tool output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<JsonSchema>,
}

impl ProviderDefinedToolFactory {
    /// Creates a provider-defined tool factory.
    pub fn new(id: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            id: id.into(),
            input_schema,
            output_schema: None,
        }
    }

    /// Sets the expected output schema shared by tools created from this factory.
    pub fn with_output_schema(mut self, output_schema: JsonSchema) -> Self {
        self.output_schema = Some(output_schema);
        self
    }

    /// Creates a provider-defined tool from this factory.
    pub fn tool(&self, name: impl Into<String>, args: JsonObject) -> Tool {
        let mut tool =
            Tool::provider_defined(name, self.id.clone(), args, self.input_schema.clone());

        if let Some(output_schema) = &self.output_schema {
            tool = tool.with_output_schema(output_schema.clone());
        }

        tool
    }
}

/// Factory for creating provider-executed tools from shared provider metadata.
///
/// This mirrors upstream `createProviderExecutedToolFactory` while keeping the
/// runtime-independent Rust tool boundary free of JavaScript callback-only
/// streaming hooks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExecutedToolFactory {
    /// Provider tool identifier, typically `<provider-id>.<unique-tool-name>`.
    pub id: String,

    /// JSON Schema 7 object describing the provider tool input.
    pub input_schema: JsonSchema,

    /// JSON Schema 7 object describing the provider tool output.
    pub output_schema: JsonSchema,

    /// Whether this provider-executed tool supports deferred results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_deferred_results: Option<bool>,
}

impl ProviderExecutedToolFactory {
    /// Creates a provider-executed tool factory.
    pub fn new(id: impl Into<String>, input_schema: JsonSchema, output_schema: JsonSchema) -> Self {
        Self {
            id: id.into(),
            input_schema,
            output_schema,
            supports_deferred_results: None,
        }
    }

    /// Sets whether created provider-executed tools support deferred results.
    pub fn with_supports_deferred_results(mut self, supports_deferred_results: bool) -> Self {
        self.supports_deferred_results = Some(supports_deferred_results);
        self
    }

    /// Creates a provider-executed tool from this factory.
    pub fn tool(&self, name: impl Into<String>, args: JsonObject) -> Tool {
        let mut tool = Tool::provider_executed(
            name,
            self.id.clone(),
            args,
            self.input_schema.clone(),
            self.output_schema.clone(),
        );

        if let Some(supports_deferred_results) = self.supports_deferred_results {
            tool = tool.with_supports_deferred_results(supports_deferred_results);
        }

        tool
    }
}

/// User-defined Rust, dynamic runtime, or provider-defined tool made available to a language model call.
///
/// This mirrors the function and dynamic branches of upstream
/// `@ai-sdk/provider-utils` `Tool`, plus provider tools whose model-facing
/// schema is owned by the provider. Function-style tools carry model-facing
/// schema/description metadata and may include an executor for later
/// client-side tool handling.
#[derive(Clone)]
pub struct Tool {
    kind: ToolKind,

    /// Name of the tool, unique within a model call.
    pub name: String,

    /// Optional display title for the tool.
    ///
    /// This mirrors upstream's deprecated `title` field: it is not sent to the
    /// model, but can be surfaced on high-level tool calls.
    pub title: Option<String>,

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

    /// Tool metadata propagated to generated tool calls and results.
    ///
    /// Unlike provider options, this metadata is not sent to the language
    /// model. It is high-level SDK state for consumers that need to identify a
    /// tool source such as an MCP server.
    pub metadata: Option<JsonObject>,

    /// Whether this tool requires approval before execution.
    ///
    /// This mirrors upstream's deprecated tool-defined `needsApproval` boolean.
    /// Generate-text-level approval configuration can still override it.
    pub needs_approval: Option<bool>,

    execute: Option<Arc<ToolExecuteFunction>>,
}

impl Tool {
    /// Creates a function tool definition.
    pub fn new(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            kind: ToolKind::Function,
            name: name.into(),
            title: None,
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            execute: None,
        }
    }

    /// Creates a dynamic function tool definition.
    ///
    /// Upstream dynamic tools are defined at runtime, but cross the provider-v4
    /// boundary as ordinary function tools. The dynamic flag remains high-level
    /// metadata used when interpreting tool calls and results.
    pub fn dynamic(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            kind: ToolKind::Dynamic,
            name: name.into(),
            title: None,
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            execute: None,
        }
    }

    /// Creates a provider-defined tool that is executed by the caller.
    ///
    /// This is the Rust-native equivalent of upstream provider-defined tool
    /// factories: `id` identifies the provider tool, `args` configures it, and
    /// `name` is the caller-facing tool name used in this model call.
    pub fn provider_defined(
        name: impl Into<String>,
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
    ) -> Self {
        Self {
            kind: ToolKind::Provider {
                id: id.into(),
                args,
                provider_executed: false,
                output_schema: None,
                supports_deferred_results: None,
            },
            name: name.into(),
            title: None,
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            execute: None,
        }
    }

    /// Creates a provider-executed tool.
    ///
    /// Provider-executed tools are sent to the model as provider tools and do
    /// not require a Rust executor because the provider returns tool results.
    pub fn provider_executed(
        name: impl Into<String>,
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
        output_schema: JsonSchema,
    ) -> Self {
        Self {
            kind: ToolKind::Provider {
                id: id.into(),
                args,
                provider_executed: true,
                output_schema: Some(output_schema),
                supports_deferred_results: None,
            },
            name: name.into(),
            title: None,
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            execute: None,
        }
    }

    /// Sets the tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the optional display title for this tool.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
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

    /// Sets the expected output schema for provider-defined tools.
    pub fn with_output_schema(mut self, output_schema: JsonSchema) -> Self {
        if let ToolKind::Provider {
            output_schema: stored_output_schema,
            ..
        } = &mut self.kind
        {
            *stored_output_schema = Some(output_schema);
        }

        self
    }

    /// Sets whether a provider-executed tool supports deferred results.
    pub fn with_supports_deferred_results(mut self, supports_deferred_results: bool) -> Self {
        if let ToolKind::Provider {
            supports_deferred_results: stored_supports_deferred_results,
            ..
        } = &mut self.kind
        {
            *stored_supports_deferred_results = Some(supports_deferred_results);
        }

        self
    }

    /// Adds provider-specific options to this tool.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets high-level tool metadata that is not sent to the provider.
    pub fn with_metadata(mut self, metadata: JsonObject) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Sets whether this tool requires approval before execution.
    ///
    /// This is the Rust equivalent of upstream tool-defined `needsApproval`
    /// when it is configured as a boolean rather than a callback.
    pub fn with_needs_approval(mut self, needs_approval: bool) -> Self {
        self.needs_approval = Some(needs_approval);
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

    /// Returns whether this tool is a provider tool.
    pub fn is_provider_tool(&self) -> bool {
        matches!(self.kind, ToolKind::Provider { .. })
    }

    /// Returns whether this tool is defined dynamically at runtime.
    pub fn is_dynamic(&self) -> bool {
        matches!(self.kind, ToolKind::Dynamic)
    }

    /// Returns whether this tool is executed by the provider.
    pub fn is_provider_executed(&self) -> bool {
        matches!(
            self.kind,
            ToolKind::Provider {
                provider_executed: true,
                ..
            }
        )
    }

    /// Returns the provider tool identifier for provider tools.
    pub fn provider_tool_id(&self) -> Option<&str> {
        match &self.kind {
            ToolKind::Provider { id, .. } => Some(id),
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns the provider tool arguments for provider tools.
    pub fn provider_tool_args(&self) -> Option<&JsonObject> {
        match &self.kind {
            ToolKind::Provider { args, .. } => Some(args),
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns the expected output schema for provider tools when one is configured.
    pub fn output_schema(&self) -> Option<&JsonSchema> {
        match &self.kind {
            ToolKind::Provider { output_schema, .. } => output_schema.as_ref(),
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns whether this provider-executed tool supports deferred results.
    pub fn supports_deferred_results(&self) -> Option<bool> {
        match &self.kind {
            ToolKind::Provider {
                supports_deferred_results,
                ..
            } => *supports_deferred_results,
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns high-level tool metadata when configured.
    pub fn metadata(&self) -> Option<&JsonObject> {
        self.metadata.as_ref()
    }

    /// Returns the optional high-level display title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns whether this tool requires approval before execution when configured.
    pub fn needs_approval(&self) -> Option<bool> {
        self.needs_approval
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
        if let ToolKind::Provider { id, args, .. } = &self.kind {
            return LanguageModelTool::Provider(LanguageModelProviderTool::new(
                id.clone(),
                self.name.clone(),
                args.clone(),
            ));
        }

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
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("title", &self.title)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .field("input_examples", &self.input_examples)
            .field("strict", &self.strict)
            .field("provider_options", &self.provider_options)
            .field("metadata", &self.metadata)
            .field("needs_approval", &self.needs_approval)
            .field("is_executable", &self.is_executable())
            .finish()
    }
}

/// Creates a provider-defined tool factory with an input schema.
pub fn create_provider_defined_tool_factory(
    id: impl Into<String>,
    input_schema: JsonSchema,
) -> ProviderDefinedToolFactory {
    ProviderDefinedToolFactory::new(id, input_schema)
}

/// Creates a provider-defined tool factory with input and output schemas.
pub fn create_provider_defined_tool_factory_with_output_schema(
    id: impl Into<String>,
    input_schema: JsonSchema,
    output_schema: JsonSchema,
) -> ProviderDefinedToolFactory {
    ProviderDefinedToolFactory::new(id, input_schema).with_output_schema(output_schema)
}

/// Creates a provider-executed tool factory with input and output schemas.
pub fn create_provider_executed_tool_factory(
    id: impl Into<String>,
    input_schema: JsonSchema,
    output_schema: JsonSchema,
) -> ProviderExecutedToolFactory {
    ProviderExecutedToolFactory::new(id, input_schema, output_schema)
}

/// Defines a dynamic runtime tool.
///
/// Dynamic tools prepare as provider-v4 function tools, matching upstream
/// `dynamicTool`, while retaining their high-level dynamic identity in Rust.
pub fn dynamic_tool(name: impl Into<String>, input_schema: JsonSchema) -> Tool {
    Tool::dynamic(name, input_schema)
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

/// Serializes provider model options for workflow step boundaries.
///
/// Upstream `serializeModelOptions` keeps JSON-serializable config values and
/// filters out functions, class instances, and other JavaScript-only
/// non-serializable values. Rust's JSON value type cannot hold those values, so
/// callers can pass `None` for config entries that should be omitted; present
/// JSON values, including `null`, are preserved.
pub fn serialize_model_options<K, V, I>(
    model_id: impl Into<String>,
    config: I,
) -> SerializedModelOptions
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<Option<JsonValue>>,
{
    let config = config
        .into_iter()
        .filter_map(|(key, value)| {
            let value: Option<JsonValue> = value.into();
            value.map(|value| (key.into(), value))
        })
        .collect();

    SerializedModelOptions::new(model_id, config)
}

/// Creates a non-cryptographic ID generator using upstream provider-utils rules.
///
/// The total ID length is the optional prefix length plus separator length plus
/// the configured random part length. When a prefix is present, the separator
/// must not occur in the alphabet so generated IDs can be parsed reliably.
pub fn create_id_generator(
    options: IdGeneratorOptions,
) -> Result<impl Fn() -> String + Send + Sync + 'static, InvalidArgumentError> {
    let IdGeneratorOptions {
        prefix,
        separator,
        size,
        alphabet,
    } = options;

    if prefix.is_some() && alphabet.contains(&separator) {
        return Err(InvalidArgumentError::new(
            "separator",
            format!(
                "The separator \"{separator}\" must not be part of the alphabet \"{alphabet}\"."
            ),
        ));
    }

    let alphabet: Vec<char> = alphabet.chars().collect();

    Ok(move || {
        let random_part = generate_random_id_part(&alphabet, size);

        if let Some(prefix) = &prefix {
            let mut id = String::with_capacity(prefix.len() + separator.len() + random_part.len());
            id.push_str(prefix);
            id.push_str(&separator);
            id.push_str(&random_part);
            id
        } else {
            random_part
        }
    })
}

/// Generates a 16-character non-cryptographic random ID using upstream defaults.
pub fn generate_id() -> String {
    let alphabet: Vec<char> = DEFAULT_ID_ALPHABET.chars().collect();
    generate_random_id_part(&alphabet, DEFAULT_ID_SIZE)
}

fn generate_random_id_part(alphabet: &[char], size: usize) -> String {
    if alphabet.is_empty() || size == 0 {
        return String::new();
    }

    let mut seed = random_id_seed() | 1;
    let mut id = String::with_capacity(size);

    for _ in 0..size {
        let random = next_id_random(&mut seed);
        id.push(alphabet[random as usize % alphabet.len()]);
    }

    id
}

fn random_id_seed() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = duration.as_nanos();
    let time_seed = (nanos as u64) ^ ((nanos >> 64) as u64);
    let counter = ID_GENERATOR_COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);

    time_seed ^ counter.rotate_left(17)
}

fn next_id_random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
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

/// Downloads a URL into a dependency-free blob through an injected transport.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `downloadBlob`: the initial
/// URL is SSRF-validated before calling the transport, redirected final URLs are
/// validated when present, non-2xx responses become [`DownloadError`], the
/// response body is read through [`read_response_with_size_limit`], and the
/// `content-type` header becomes the returned blob media type.
pub async fn download_blob<Transport, TransportFuture>(
    options: DownloadBlobOptions,
    transport: Transport,
) -> Result<DownloadedBlob, DownloadError>
where
    Transport: FnOnce(&str) -> TransportFuture,
    TransportFuture: Future<Output = Result<DownloadBlobResponse, DownloadError>>,
{
    let DownloadBlobOptions { url, max_bytes } = options;

    validate_download_url(&url)?;

    let response = transport(&url).await?;

    if let Some(final_url) = response.final_url.as_deref() {
        validate_download_url(final_url)?;
    }

    if !response.is_success_status() {
        return Err(DownloadError::with_status(
            url,
            response.status_code,
            response.status_text,
        ));
    }

    let content_length = header_value(&response.headers, "content-length");
    let response_body =
        read_response_with_size_limit(&url, response.body.as_deref(), content_length, max_bytes)?;

    let mut blob = DownloadedBlob::new(response_body);

    if let Some(media_type) = header_value(&response.headers, "content-type") {
        blob = blob.with_media_type(media_type);
    }

    Ok(blob)
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

fn header_value<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
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

/// Parses a JSON event stream into parsed JSON results.
///
/// This mirrors upstream `parseJsonEventStream`: event-source `data:` payloads
/// are parsed independently, `[DONE]` payloads are ignored, and parse or
/// validation failures are surfaced as safe parse results instead of panicking.
pub fn parse_json_event_stream<T, F, E, B>(
    chunks: impl IntoIterator<Item = B>,
    validate: F,
) -> Vec<ParseJsonResult<T>>
where
    F: Fn(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
    B: AsRef<[u8]>,
{
    let mut bytes = Vec::new();
    for chunk in chunks {
        bytes.extend_from_slice(chunk.as_ref());
    }
    let text = String::from_utf8_lossy(&bytes);

    parse_event_source_data_events(&text)
        .into_iter()
        .filter(|data| data != "[DONE]")
        .map(|data| parse_json_event_data(&data, &validate))
        .collect()
}

fn parse_event_source_data_events(text: &str) -> Vec<String> {
    let mut events = Vec::new();
    let mut data_lines = Vec::new();

    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);

        if line.is_empty() {
            push_event_source_data_event(&mut events, &mut data_lines);
            continue;
        }

        if line.starts_with(':') {
            continue;
        }

        let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| {
            (field, value.strip_prefix(' ').unwrap_or(value))
        });

        if field == "data" {
            data_lines.push(value.to_string());
        }
    }

    push_event_source_data_event(&mut events, &mut data_lines);
    events
}

fn push_event_source_data_event(events: &mut Vec<String>, data_lines: &mut Vec<String>) {
    if data_lines.is_empty() {
        return;
    }

    events.push(data_lines.join("\n"));
    data_lines.clear();
}

fn parse_json_event_data<T, F, E>(data: &str, validate: &F) -> ParseJsonResult<T>
where
    F: Fn(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let raw_value = match safe_parse_json(data) {
        ParseJsonResult::Success { raw_value, .. } => raw_value,
        ParseJsonResult::Failure { error, .. } => return ParseJsonResult::failure(error, None),
    };

    match safe_validate_types(raw_value.clone(), |value| validate(value), None) {
        ValidateTypesResult::Success { value, raw_value } => {
            ParseJsonResult::success(value, raw_value)
        }
        ValidateTypesResult::Failure { error, raw_value } => {
            ParseJsonResult::failure(error, Some(raw_value))
        }
    }
}

/// Parses a successful event-source response body into JSON parse results.
///
/// This mirrors upstream `createEventSourceResponseHandler`: a missing response
/// body throws [`EmptyResponseBodyError`], while a present body is parsed into
/// safe per-event JSON results and returned with extracted response headers.
pub fn create_event_source_response_handler<T, F, E>(
    options: EventSourceResponseHandlerOptions,
    validate: F,
) -> Result<ResponseHandlerResult<Vec<ParseJsonResult<T>>>, EmptyResponseBodyError>
where
    F: Fn(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let EventSourceResponseHandlerOptions {
        response_headers,
        response_body,
    } = options;

    let Some(response_body) = response_body else {
        return Err(EmptyResponseBodyError::new());
    };

    Ok(
        ResponseHandlerResult::new(parse_json_event_stream([response_body], validate))
            .with_response_headers(response_headers),
    )
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

/// Handles a prepared provider API response using success and failure handlers.
///
/// This mirrors the response-processing branch shared by upstream `getFromApi`
/// and `postToApi`: unsuccessful HTTP statuses run the failed-response handler
/// and return its API error, successful statuses run the successful-response
/// handler, and non-API-call handler failures are wrapped in an upstream-shaped
/// [`ApiCallError`] with the response status and headers.
pub fn handle_provider_api_response<T, S, F>(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, Box<ApiCallError>>
where
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    if !response.is_success_status() {
        return match failed_response_handler(request, response) {
            Ok(error_information) => Err(Box::new(error_information.into_value())),
            Err(error) => Err(provider_api_response_handler_error(
                error,
                "Failed to process error response",
                request,
                response,
            )),
        };
    }

    match successful_response_handler(request, response) {
        Ok(result) => Ok(result),
        Err(error) => Err(provider_api_response_handler_error(
            error,
            "Failed to process successful response",
            request,
            response,
        )),
    }
}

/// Executes a prepared provider API request through a caller-supplied transport.
///
/// This is the dependency-free orchestration boundary for upstream
/// `getFromApi` and `postToApi`: HTTP adapters send the prepared request and
/// return a [`ProviderApiResponse`], response statuses are dispatched through
/// [`handle_provider_api_response`], and transport failures are normalized with
/// [`handle_fetch_error`].
pub async fn execute_provider_api_request<T, Transport, TransportFuture, S, F>(
    request: ProviderApiRequest,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    let response = match transport(request.clone()).await {
        Ok(response) => response,
        Err(error) => {
            return Err(handle_fetch_error(
                error,
                request.url,
                request.request_body_values,
            ));
        }
    };

    handle_provider_api_response(
        &request,
        &response,
        successful_response_handler,
        failed_response_handler,
    )
    .map_err(|error| HandledFetchError::ApiCall { error })
}

/// Runs an upstream-style `getFromApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `getFromApi`: request metadata is prepared from [`GetFromApiOptions`], the
/// caller-supplied transport performs the HTTP work, and response handling plus
/// fetch-error normalization are delegated to [`execute_provider_api_request`].
pub async fn get_from_api<T, Transport, TransportFuture, S, F>(
    options: GetFromApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

/// Runs an upstream-style `postJsonToApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `postJsonToApi`: JSON request metadata is prepared from
/// [`PostJsonToApiOptions`], the caller-supplied transport performs the HTTP
/// work, and response handling plus fetch-error normalization are delegated to
/// [`execute_provider_api_request`].
pub async fn post_json_to_api<T, Transport, TransportFuture, S, F>(
    options: PostJsonToApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

/// Runs an upstream-style `postToApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `postToApi`: POST request metadata is prepared from [`PostToApiOptions`],
/// the caller-supplied transport performs the HTTP work, and response handling
/// plus fetch-error normalization are delegated to
/// [`execute_provider_api_request`].
pub async fn post_to_api<T, Transport, TransportFuture, S, F>(
    options: PostToApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

fn provider_api_response_handler_error(
    error: ProviderApiResponseHandlerError,
    message: &'static str,
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Box<ApiCallError> {
    match error {
        ProviderApiResponseHandlerError::ApiCall { error } => error,
        ProviderApiResponseHandlerError::Other { .. } => Box::new(
            ApiCallError::new(
                message,
                request.url.clone(),
                request.request_body_values.clone(),
            )
            .with_status_code(response.status_code)
            .with_response_headers(response.headers.clone()),
        ),
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

/// Appends the provider-utils package and runtime user-agent suffixes to headers.
///
/// This is the Rust-native request-header preparation shared by upstream
/// `getFromApi` and `postToApi`: callers supply their provider headers and an
/// explicit runtime environment, and the result is normalized with
/// `ai-sdk/provider-utils/{VERSION}` plus the upstream runtime suffix.
pub fn with_provider_utils_user_agent<K, V, I>(
    headers: Option<I>,
    environment: &RuntimeEnvironment,
) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    with_user_agent_suffix(
        headers,
        [
            format!("ai-sdk/provider-utils/{}", crate::VERSION),
            get_runtime_environment_user_agent(environment),
        ],
    )
}

/// Prepares the request metadata used by upstream `getFromApi`.
///
/// The returned request has method `GET`, normalized provider-utils user-agent
/// headers, no body, and empty `requestBodyValues`.
pub fn prepare_get_from_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    ProviderApiRequest::get(url, with_provider_utils_user_agent(headers, environment))
}

/// Prepares the request metadata used by upstream `postToApi`.
///
/// Upstream `postToApi` applies the shared provider-utils user-agent headers,
/// sends the supplied body content, and preserves separate body values for
/// response-handler `requestBodyValues`. The Rust boundary supports text and
/// byte bodies and leaves JavaScript-only `FormData` handling to HTTP adapters.
pub fn prepare_post_to_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    body: ProviderApiRequestBody,
    request_body_values: impl Into<JsonValue>,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    ProviderApiRequest::post(
        url,
        with_provider_utils_user_agent(headers, environment),
        body,
        request_body_values,
    )
}

/// Prepares the JSON request metadata used by upstream `postJsonToApi`.
///
/// Upstream `postJsonToApi` adds `Content-Type: application/json`, allows caller
/// headers to override it, stringifies the JSON body for `body.content`, and
/// preserves the original body value for response-handler `requestBodyValues`.
pub fn prepare_post_json_to_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    body: impl Into<JsonValue>,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: Into<String>,
    V: Into<String>,
{
    let body_values = body.into();
    let body_content = body_values.to_string();
    let mut combined_headers = BTreeMap::from([(
        "Content-Type".to_string(),
        Some("application/json".to_string()),
    )]);

    if let Some(headers) = headers {
        for (key, value) in headers {
            combined_headers.insert(key.into(), value.map(Into::into));
        }
    }

    prepare_post_to_api_request(
        url,
        Some(combined_headers),
        ProviderApiRequestBody::text(body_content),
        body_values,
        environment,
    )
}

/// Returns an upstream-style runtime user-agent suffix for provider utilities.
///
/// This mirrors upstream `getRuntimeEnvironmentUserAgent`: browser indicators
/// win first, navigator user agents are lowercased, Node.js versions are
/// included as supplied, Vercel Edge is detected next, and unknown runtimes use
/// the upstream fallback string.
pub fn get_runtime_environment_user_agent(environment: &RuntimeEnvironment) -> String {
    if environment.has_window {
        return "runtime/browser".to_string();
    }

    if let Some(user_agent) = environment
        .navigator_user_agent
        .as_deref()
        .filter(|user_agent| !user_agent.is_empty())
    {
        return format!("runtime/{}", user_agent.to_ascii_lowercase());
    }

    if let Some(version) = environment
        .node_version
        .as_deref()
        .filter(|version| !version.is_empty())
    {
        return format!("runtime/node.js/{version}");
    }

    if environment.has_edge_runtime {
        return "runtime/vercel-edge".to_string();
    }

    "runtime/unknown".to_string()
}

/// Returns whether an error name represents an aborted request.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isAbortError`: JavaScript
/// checks `Error` and `DOMException` instances by their `name` field, while the
/// Rust boundary accepts that runtime-specific name directly.
pub fn is_abort_error(error_name: &str) -> bool {
    matches!(
        error_name,
        "AbortError" | "ResponseAborted" | "TimeoutError"
    )
}

/// Normalizes lower-level fetch/network errors for provider API helpers.
///
/// This mirrors upstream internal `handleFetchError`: abort-style errors are
/// returned unchanged, recognized fetch connection failures become retryable
/// [`ApiCallError`] values, and unknown errors are propagated unchanged.
pub fn handle_fetch_error(
    error: FetchErrorInfo,
    url: impl Into<String>,
    request_body_values: impl Into<JsonValue>,
) -> HandledFetchError {
    if error.name.as_deref().is_some_and(is_abort_error) {
        return HandledFetchError::Original { error };
    }

    if error.name.as_deref() == Some("TypeError")
        && FETCH_FAILED_ERROR_MESSAGES.contains(&error.message.to_lowercase().as_str())
        && let Some(cause_message) = error.cause_message.as_deref()
    {
        return HandledFetchError::ApiCall {
            error: Box::new(
                ApiCallError::new(
                    format!("Cannot connect to API: {cause_message}"),
                    url,
                    request_body_values,
                )
                .with_is_retryable(true),
            ),
        };
    }

    if error
        .code
        .as_deref()
        .is_some_and(|code| BUN_NETWORK_ERROR_CODES.contains(&code))
    {
        return HandledFetchError::ApiCall {
            error: Box::new(
                ApiCallError::new(
                    format!("Cannot connect to API: {}", error.message),
                    url,
                    request_body_values,
                )
                .with_is_retryable(true),
            ),
        };
    }

    HandledFetchError::Original { error }
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
    use std::thread;
    use std::time::Duration;

    use crate::language_model::{
        LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage,
        LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelTool, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::{
        ApiCallError, FileData, FileDataContent, ImageModelFile, JsonObject, JsonValue,
        ProviderMetadata, ProviderReference, TypeValidationContext, TypeValidationError, Warning,
    };
    use serde_json::json;
    use url::Url;

    use super::{
        Arrayable, Base64DecodeError, BinaryResponseHandlerOptions, ConvertToFormDataOptions,
        DEFAULT_MAX_DOWNLOAD_SIZE, DownloadBlobOptions, DownloadBlobResponse, DownloadError,
        DownloadedBlob, EventSourceResponseHandlerOptions, FetchErrorInfo, FormData, FormDataEntry,
        FormDataInputValue, FormDataValue, GetFromApiOptions, HandledFetchError,
        IdGeneratorOptions, InjectJsonInstructionIntoMessagesOptions, InlineFileDataBytesError,
        JsonErrorResponseHandlerOptions, JsonResponseHandlerOptions, LoadApiKeyOptions,
        LoadOptionalSettingOptions, LoadSettingOptions, ParseJsonError, ParseJsonResult,
        PostJsonToApiOptions, PostToApiOptions, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseBody,
        ProviderApiResponseHandlerError, ProviderDefinedToolFactory, ProviderExecutedToolFactory,
        ReasoningLevel, ResponseHandlerResult, RuntimeEnvironment, SerializedModelOptions,
        StatusCodeErrorResponseHandlerOptions, StreamingToolCallDelta,
        StreamingToolCallDeltaFunction, StreamingToolCallTracker, StreamingToolCallTrackerOptions,
        StreamingToolCallTypeValidation, Tool, ToolExecutionError, ToolExecutionOptions,
        ValidateTypesResult, add_additional_properties_to_json_schema, as_array, combine_headers,
        convert_base64_to_bytes, convert_bytes_to_base64, convert_image_model_file_to_data_uri,
        convert_inline_file_data_to_bytes, convert_to_base64, convert_to_form_data,
        create_binary_response_handler, create_event_source_response_handler, create_id_generator,
        create_json_error_response_handler, create_json_response_handler,
        create_provider_defined_tool_factory,
        create_provider_defined_tool_factory_with_output_schema,
        create_provider_executed_tool_factory, create_status_code_error_response_handler,
        create_tool_name_mapping, delay, detect_media_type, download_blob, dynamic_tool,
        execute_provider_api_request, extract_response_headers, filter_nullable, generate_id,
        get_error_message, get_from_api, get_runtime_environment_user_agent,
        get_top_level_media_type, handle_fetch_error, handle_provider_api_response,
        inject_json_instruction, inject_json_instruction_into_messages, is_abort_error,
        is_custom_reasoning, is_full_media_type, is_non_nullable, is_parsable_json,
        is_provider_reference, is_url_supported, load_api_key, load_api_key_with_env,
        load_optional_setting_with_env, load_setting, load_setting_with_env,
        map_reasoning_to_provider_budget, map_reasoning_to_provider_effort,
        media_type_to_extension, normalize_headers, parse_json, parse_json_event_stream,
        parse_provider_options, post_json_to_api, post_to_api, prepare_get_from_api_request,
        prepare_post_json_to_api_request, prepare_post_to_api_request, prepare_tools,
        read_response_with_size_limit, remove_undefined_entries, resolve_full_media_type,
        resolve_provider_reference, safe_parse_json, safe_validate_types, serialize_model_options,
        strip_file_extension, validate_download_url, validate_types,
        with_provider_utils_user_agent, with_user_agent_suffix, without_trailing_slash,
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

    fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        Future::poll(future.as_mut(), &mut context)
    }

    #[test]
    fn provider_utils_reexports_provider_get_error_message() {
        assert_eq!(get_error_message(None), "unknown error");
        assert_eq!(
            get_error_message(Some(&"provider-utils failure")),
            "provider-utils failure"
        );
        assert_eq!(
            get_error_message(Some(&json!({ "code": "bad_request" }))),
            "{\"code\":\"bad_request\"}"
        );
    }

    #[test]
    fn delay_without_duration_resolves_immediately() {
        poll_ready(delay(None));
    }

    #[test]
    fn delay_with_duration_resolves_after_timer_completes() {
        let mut future = Box::pin(delay(Some(10)));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        thread::sleep(Duration::from_millis(30));

        assert!(matches!(poll_once(future.as_mut()), Poll::Ready(())));
    }

    #[test]
    fn delay_zero_and_negative_values_use_timer_like_deferred_completion() {
        for delay_in_ms in [0, -10] {
            let mut future = Box::pin(delay(Some(delay_in_ms)));

            assert!(
                matches!(poll_once(future.as_mut()), Poll::Pending),
                "{delay_in_ms}ms delay should be deferred"
            );

            thread::sleep(Duration::from_millis(5));

            assert!(
                matches!(poll_once(future.as_mut()), Poll::Ready(())),
                "{delay_in_ms}ms delay should complete after the timer runs"
            );
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
    fn serialized_model_options_round_trips_upstream_shape() {
        let options = SerializedModelOptions::new(
            "claude-sonnet-4-20250514",
            json!({
                "provider": "anthropic.messages",
                "baseURL": "https://api.anthropic.com/v1",
                "headers": { "x-api-key": "sk-test" },
                "supportsNativeStructuredOutput": true,
                "supportsStrictTools": false
            })
            .as_object()
            .expect("config is an object")
            .clone(),
        );

        let serialized = serde_json::to_value(&options).expect("model options serialize");

        assert_eq!(
            serialized,
            json!({
                "modelId": "claude-sonnet-4-20250514",
                "config": {
                    "provider": "anthropic.messages",
                    "baseURL": "https://api.anthropic.com/v1",
                    "headers": { "x-api-key": "sk-test" },
                    "supportsNativeStructuredOutput": true,
                    "supportsStrictTools": false
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<SerializedModelOptions>(serialized)
                .expect("model options deserialize"),
            options
        );
    }

    #[test]
    fn serialize_model_options_filters_missing_entries_and_preserves_json_null() {
        let result = serialize_model_options(
            "gpt-4",
            [
                ("provider", Some(json!("openai"))),
                (
                    "headers",
                    Some(json!({ "authorization": "Bearer sk-test" })),
                ),
                ("metadata", Some(JsonValue::Null)),
                ("fetch", None),
                ("generateId", None),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "gpt-4",
                "config": {
                    "provider": "openai",
                    "headers": { "authorization": "Bearer sk-test" },
                    "metadata": null
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_accepts_already_json_config_objects() {
        let config = json!({
            "provider": "test",
            "tags": ["a", "b"],
            "supportsStrictTools": false
        })
        .as_object()
        .expect("config is an object")
        .clone();

        let result = serialize_model_options("model", config);

        assert_eq!(
            result,
            SerializedModelOptions::new(
                "model",
                json!({
                    "provider": "test",
                    "tags": ["a", "b"],
                    "supportsStrictTools": false
                })
                .as_object()
                .expect("config is an object")
                .clone()
            )
        );
    }

    #[test]
    fn form_data_contracts_round_trip_ordered_text_and_bytes_entries() {
        let form_data = FormData {
            entries: vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image", FormDataValue::bytes(vec![1, 2, 3])),
            ],
        };

        let serialized = serde_json::to_value(&form_data).expect("form data serializes");

        assert_eq!(
            serialized,
            json!({
                "entries": [
                    {
                        "name": "model",
                        "value": {
                            "type": "text",
                            "value": "gpt-image-1"
                        }
                    },
                    {
                        "name": "image",
                        "value": {
                            "type": "bytes",
                            "value": [1, 2, 3]
                        }
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<FormData>(serialized).expect("form data deserializes"),
            form_data
        );

        let input_value =
            FormDataInputValue::array(vec![FormDataValue::text("cat"), FormDataValue::text("dog")]);
        let input_serialized =
            serde_json::to_value(&input_value).expect("form data input serializes");
        assert_eq!(
            input_serialized,
            json!({
                "type": "array",
                "values": [
                    {
                        "type": "text",
                        "value": "cat"
                    },
                    {
                        "type": "text",
                        "value": "dog"
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<FormDataInputValue>(input_serialized)
                .expect("form data input deserializes"),
            input_value
        );

        assert_eq!(
            serde_json::from_value::<ConvertToFormDataOptions>(json!({}))
                .expect("options deserialize with defaults"),
            ConvertToFormDataOptions::new()
        );
        assert_eq!(
            serde_json::to_value(ConvertToFormDataOptions::new().with_use_array_brackets(false))
                .expect("options serialize"),
            json!({ "useArrayBrackets": false })
        );
    }

    #[test]
    fn convert_to_form_data_skips_missing_and_uses_upstream_array_key_rules() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("gpt-image-1")),
                ),
                ("mask".to_string(), None),
                (
                    "image".to_string(),
                    Some(FormDataInputValue::array(vec![
                        FormDataValue::bytes(vec![1, 2]),
                        FormDataValue::bytes(vec![3, 4]),
                    ])),
                ),
                (
                    "quality".to_string(),
                    Some(FormDataInputValue::array(vec![FormDataValue::text("high")])),
                ),
                (
                    "empty".to_string(),
                    Some(FormDataInputValue::array(Vec::new())),
                ),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.entries,
            vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image[]", FormDataValue::bytes(vec![1, 2])),
                FormDataEntry::new("image[]", FormDataValue::bytes(vec![3, 4])),
                FormDataEntry::new("quality", FormDataValue::text("high")),
            ]
        );
        assert!(form_data.has("model"));
        assert!(!form_data.has("mask"));
        assert!(!form_data.has("empty"));
        assert_eq!(form_data.get("quality"), Some(&FormDataValue::text("high")));
        assert_eq!(form_data.get_all("image[]").len(), 2);
    }

    #[test]
    fn convert_to_form_data_can_disable_array_bracket_suffix() {
        let form_data = convert_to_form_data(
            vec![(
                "image".to_string(),
                Some(FormDataInputValue::array(vec![
                    FormDataValue::bytes(vec![1]),
                    FormDataValue::bytes(vec![2]),
                ])),
            )],
            ConvertToFormDataOptions::new().with_use_array_brackets(false),
        );

        assert_eq!(
            form_data.entries,
            vec![
                FormDataEntry::new("image", FormDataValue::bytes(vec![1])),
                FormDataEntry::new("image", FormDataValue::bytes(vec![2])),
            ]
        );
        assert!(!form_data.has("image[]"));
        assert_eq!(form_data.get_all("image").len(), 2);
    }

    #[test]
    fn download_blob_contracts_serialize_with_upstream_camel_case_fields() {
        let options = DownloadBlobOptions::new("https://example.com/image.png").with_max_bytes(4);
        assert_eq!(
            serde_json::to_value(&options).expect("options serialize"),
            json!({
                "url": "https://example.com/image.png",
                "maxBytes": 4
            })
        );
        assert_eq!(
            serde_json::from_value::<DownloadBlobOptions>(json!({
                "url": "https://example.com/image.png"
            }))
            .expect("options deserialize"),
            DownloadBlobOptions::new("https://example.com/image.png")
        );

        let response = DownloadBlobResponse::bytes(200, "OK", vec![1, 2, 3])
            .with_headers(BTreeMap::from([(
                "content-type".to_string(),
                "image/png".to_string(),
            )]))
            .with_final_url("https://cdn.example.com/image.png");
        assert_eq!(
            serde_json::to_value(&response).expect("response serializes"),
            json!({
                "statusCode": 200,
                "statusText": "OK",
                "headers": {
                    "content-type": "image/png"
                },
                "body": [1, 2, 3],
                "finalUrl": "https://cdn.example.com/image.png"
            })
        );
        assert_eq!(
            serde_json::from_value::<DownloadBlobResponse>(
                serde_json::to_value(&response).expect("response serializes")
            )
            .expect("response deserializes"),
            response
        );

        let blob = DownloadedBlob::new(vec![1, 2, 3]).with_media_type("image/png");
        assert_eq!(
            serde_json::to_value(&blob).expect("blob serializes"),
            json!({
                "data": [1, 2, 3],
                "mediaType": "image/png"
            })
        );
    }

    #[test]
    fn download_blob_downloads_bytes_and_content_type_through_injected_transport() {
        let result = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/image.png"),
            |url| {
                assert_eq!(url, "https://example.com/image.png");
                ready(Ok(DownloadBlobResponse::bytes(
                    200,
                    "OK",
                    b"test content".to_vec(),
                )
                .with_headers(BTreeMap::from([
                    ("Content-Type".to_string(), "image/png".to_string()),
                    ("Content-Length".to_string(), "12".to_string()),
                ]))))
            },
        ))
        .expect("download succeeds");

        assert_eq!(
            result,
            DownloadedBlob::new(b"test content".to_vec()).with_media_type("image/png")
        );
    }

    #[test]
    fn download_blob_returns_empty_blob_for_missing_body() {
        let result = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/empty.bin"),
            |_| {
                ready(Ok(DownloadBlobResponse::new(200, "OK").with_headers(
                    BTreeMap::from([(
                        "content-type".to_string(),
                        "application/octet-stream".to_string(),
                    )]),
                )))
            },
        ))
        .expect("download succeeds");

        assert_eq!(
            result,
            DownloadedBlob::new(Vec::new()).with_media_type("application/octet-stream")
        );
    }

    #[test]
    fn download_blob_turns_non_success_status_into_download_error() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/not-found.png"),
            |_| ready(Ok(DownloadBlobResponse::new(404, "Not Found"))),
        ))
        .expect_err("non-success status fails");

        assert_eq!(error.url(), "https://example.com/not-found.png");
        assert_eq!(error.status_code(), Some(404));
        assert_eq!(error.status_text(), Some("Not Found"));
        assert_eq!(
            error.message(),
            "Failed to download https://example.com/not-found.png: 404 Not Found"
        );
    }

    #[test]
    fn download_blob_enforces_size_limit_from_headers_and_body_bytes() {
        let content_length_error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/huge.bin").with_max_bytes(3),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(200, "OK", vec![1, 2, 3, 4])
                    .with_headers(BTreeMap::from([(
                        "content-length".to_string(),
                        "4 bytes".to_string(),
                    )]))))
            },
        ))
        .expect_err("content-length over limit fails");
        assert_eq!(
            content_length_error.message(),
            "Download of https://example.com/huge.bin exceeded maximum size of 3 bytes (Content-Length: 4)."
        );

        let body_error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/liar.bin").with_max_bytes(3),
            |_| ready(Ok(DownloadBlobResponse::bytes(200, "OK", vec![1, 2, 3, 4]))),
        ))
        .expect_err("body over limit fails");
        assert_eq!(
            body_error.message(),
            "Download of https://example.com/liar.bin exceeded maximum size of 3 bytes."
        );
    }

    #[test]
    fn download_blob_validates_redirect_final_url() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/redirect"),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(
                    200,
                    "OK",
                    b"secret".to_vec(),
                )
                .with_final_url("http://localhost/admin")))
            },
        ))
        .expect_err("unsafe redirect URL fails");

        assert_eq!(
            error.message(),
            "URL with hostname localhost is not allowed"
        );
    }

    #[test]
    fn download_blob_propagates_transport_download_errors() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/network-error.png"),
            |_| {
                ready(Err(DownloadError::with_cause_message(
                    "https://example.com/network-error.png",
                    "Network error",
                )))
            },
        ))
        .expect_err("transport error propagates");

        assert_eq!(
            error.message(),
            "Failed to download https://example.com/network-error.png: Network error"
        );
    }

    #[test]
    fn streaming_tool_call_tracker_options_serialize_and_deserialize_validation_mode() {
        let options = StreamingToolCallTrackerOptions::new()
            .with_type_validation(StreamingToolCallTypeValidation::IfPresent);

        let serialized = serde_json::to_value(&options).expect("tracker options serialize");

        assert_eq!(
            serialized,
            json!({
                "typeValidation": "if-present"
            })
        );
        assert_eq!(
            serde_json::from_value::<StreamingToolCallTrackerOptions>(serialized)
                .expect("tracker options deserialize"),
            options
        );
        assert_eq!(
            serde_json::from_value::<StreamingToolCallTrackerOptions>(json!({}))
                .expect("empty options deserialize"),
            StreamingToolCallTrackerOptions::default()
        );
    }

    #[test]
    fn streaming_tool_call_delta_round_trips_upstream_shape_with_extensions() {
        let delta = StreamingToolCallDelta::new()
            .with_index(0)
            .with_id("call_1")
            .with_type("function")
            .with_function(
                StreamingToolCallDeltaFunction::new()
                    .with_name("get_weather")
                    .with_arguments(r#"{"city":"London"}"#),
            )
            .with_extra_value(
                "extra_content",
                json!({ "google": { "thought_signature": "sig123" } }),
            );

        let serialized = serde_json::to_value(&delta).expect("delta serializes");

        assert_eq!(
            serialized,
            json!({
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": r#"{"city":"London"}"#
                },
                "extra_content": {
                    "google": { "thought_signature": "sig123" }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<StreamingToolCallDelta>(serialized)
                .expect("delta deserializes"),
            delta
        );
    }

    #[test]
    fn streaming_tool_call_tracker_accumulates_and_finishes_json_arguments() {
        let mut tracker = StreamingToolCallTracker::new();

        let first_parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("get_weather")
                            .with_arguments(r#"{"ci"#),
                    ),
            )
            .expect("first delta succeeds");

        assert_eq!(
            serde_json::to_value(first_parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "get_weather" },
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"{"ci"# }
            ])
        );

        let middle_parts =
            tracker
                .process_delta(StreamingToolCallDelta::new().with_index(0).with_function(
                    StreamingToolCallDeltaFunction::new().with_arguments(r#"ty":"San"#),
                ))
                .expect("middle delta succeeds");

        assert_eq!(
            serde_json::to_value(middle_parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"ty":"San"# }
            ])
        );

        let final_parts = tracker
            .process_delta(StreamingToolCallDelta::new().with_index(0).with_function(
                StreamingToolCallDeltaFunction::new().with_arguments(r#" Francisco"}"#),
            ))
            .expect("final delta succeeds");

        assert_eq!(
            serde_json::to_value(final_parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-delta", "id": "call_1", "delta": r#" Francisco"}"# },
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "get_weather",
                    "input": r#"{"city":"San Francisco"}"#
                }
            ])
        );
    }

    #[test]
    fn streaming_tool_call_tracker_flushes_unfinished_tool_calls_once() {
        let mut tracker = StreamingToolCallTracker::new();

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(r#"{"key":"val"#),
                    ),
            )
            .expect("delta succeeds");

        assert_eq!(
            serde_json::to_value(tracker.flush()).expect("flush parts serialize"),
            json!([
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "fn",
                    "input": r#"{"key":"val"#
                }
            ])
        );
        assert!(tracker.flush().is_empty());
    }

    #[test]
    fn streaming_tool_call_tracker_validates_required_delta_type_and_fields() {
        let mut tracker = StreamingToolCallTracker::from_options(
            StreamingToolCallTrackerOptions::new()
                .with_type_validation(StreamingToolCallTypeValidation::Required),
        );

        let type_error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_function(StreamingToolCallDeltaFunction::new().with_name("fn")),
            )
            .expect_err("missing function type is rejected");

        assert_eq!(type_error.to_string(), "Expected 'function' type.");
        assert_eq!(
            type_error.data(),
            &json!({
                "index": 0,
                "id": "call_1",
                "function": { "name": "fn" }
            })
        );

        let mut tracker = StreamingToolCallTracker::new();
        let id_error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_type("function")
                    .with_function(StreamingToolCallDeltaFunction::new().with_name("fn")),
            )
            .expect_err("missing id is rejected");

        assert_eq!(id_error.to_string(), "Expected 'id' to be a string.");
    }

    #[test]
    fn streaming_tool_call_tracker_attaches_provider_metadata_to_tool_call_events() {
        let mut tracker = StreamingToolCallTracker::new()
            .with_extract_metadata(|delta| {
                let signature = delta
                    .extra
                    .get("extra_content")?
                    .get("google")?
                    .get("thought_signature")?
                    .as_str()?;

                Some(ProviderMetadata::from([(
                    "google".to_string(),
                    json!({ "thoughtSignature": signature })
                        .as_object()
                        .expect("metadata is an object")
                        .clone(),
                )]))
            })
            .with_tool_call_provider_metadata(|metadata| metadata.cloned());

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments("{}"),
                    )
                    .with_extra_value(
                        "extra_content",
                        json!({ "google": { "thought_signature": "sig123" } }),
                    ),
            )
            .expect("delta succeeds");

        assert_eq!(
            serde_json::to_value(parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "fn" },
                { "type": "tool-input-delta", "id": "call_1", "delta": "{}" },
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "fn",
                    "input": "{}",
                    "providerMetadata": {
                        "google": { "thoughtSignature": "sig123" }
                    }
                }
            ])
        );
    }

    #[test]
    fn id_generator_options_serialize_and_deserialize_camel_case_shape() {
        let options = IdGeneratorOptions::new()
            .with_prefix("msg")
            .with_separator("_")
            .with_size(8)
            .with_alphabet("abc");

        assert_eq!(
            serde_json::to_value(&options).expect("options serialize"),
            json!({
                "prefix": "msg",
                "separator": "_",
                "size": 8,
                "alphabet": "abc"
            })
        );
        assert_eq!(
            serde_json::from_value::<IdGeneratorOptions>(json!({}))
                .expect("default options deserialize"),
            IdGeneratorOptions::default()
        );
    }

    #[test]
    fn create_id_generator_creates_random_part_with_configured_size_and_alphabet() {
        let generator =
            create_id_generator(IdGeneratorOptions::new().with_size(12).with_alphabet("ab"))
                .expect("generator is valid");

        let id = generator();

        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|character| "ab".contains(character)));
    }

    #[test]
    fn create_id_generator_adds_prefix_and_separator() {
        let generator = create_id_generator(
            IdGeneratorOptions::new()
                .with_prefix("msg")
                .with_separator("_")
                .with_size(6)
                .with_alphabet("xyz"),
        )
        .expect("generator is valid");

        let id = generator();
        let random_part = id
            .strip_prefix("msg_")
            .expect("prefix and separator are present");

        assert_eq!(random_part.len(), 6);
        assert!(
            random_part
                .chars()
                .all(|character| "xyz".contains(character))
        );
    }

    #[test]
    fn create_id_generator_rejects_separator_inside_alphabet_when_prefixed() {
        let error = match create_id_generator(
            IdGeneratorOptions::new()
                .with_prefix("tool")
                .with_separator("a"),
        ) {
            Ok(_) => panic!("separator in alphabet is invalid when prefixed"),
            Err(error) => error,
        };

        assert_eq!(error.argument(), "separator");
        assert_eq!(
            error.message(),
            "The separator \"a\" must not be part of the alphabet \"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\"."
        );
    }

    #[test]
    fn create_id_generator_allows_default_separator_without_prefix() {
        let generator = create_id_generator(IdGeneratorOptions::new())
            .expect("default unprefixed generator is valid");

        assert_eq!(generator().len(), 16);
    }

    #[test]
    fn generate_id_uses_upstream_default_random_part_length() {
        let id = generate_id();

        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|character| {
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".contains(character)
        }));
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
    fn event_source_response_handler_options_use_camel_case_json() {
        let options = EventSourceResponseHandlerOptions::new(
            b"data: {\"name\":\"John\",\"age\":30}\n\n".to_vec(),
        )
        .with_response_headers(BTreeMap::from([(
            "content-type".to_string(),
            "text/event-stream".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "responseHeaders": {
                    "content-type": "text/event-stream"
                },
                "responseBody": [
                    100, 97, 116, 97, 58, 32, 123, 34, 110, 97, 109, 101, 34, 58,
                    34, 74, 111, 104, 110, 34, 44, 34, 97, 103, 101, 34, 58,
                    51, 48, 125, 10, 10
                ]
            })
        );

        let deserialized: EventSourceResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn event_source_response_handler_options_deserialize_missing_body() {
        let options: EventSourceResponseHandlerOptions = serde_json::from_value(json!({
            "responseHeaders": {}
        }))
        .expect("options deserialize");

        assert_eq!(options.response_body, None);
        assert_eq!(options.response_headers, BTreeMap::new());
    }

    #[test]
    fn parse_json_event_stream_parses_data_events_and_ignores_done() {
        let events = parse_json_event_stream(
            [
                b": keepalive\r\n".as_slice(),
                b"event: message\r\ndata: {\"name\":\r\n".as_slice(),
                b"data: \"John\",\"age\":30}\r\n\r\n".as_slice(),
                b"data: [DONE]\n\n".as_slice(),
            ],
            validate_person,
        );

        assert_eq!(
            events,
            vec![ParseJsonResult::success(
                Person {
                    name: "John".to_string(),
                    age: 30,
                },
                json!({ "name": "John", "age": 30 })
            )]
        );
    }

    #[test]
    fn parse_json_event_stream_preserves_parse_and_validation_failures() {
        let events = parse_json_event_stream(
            [
                b"data: {not json}\n\n".as_slice(),
                b"data: {\"name\":\"John\"}\n\n".as_slice(),
            ],
            validate_person,
        );

        assert_eq!(events.len(), 2);

        let parse_error = events[0].error().expect("parse error is returned");
        assert!(parse_error.as_json_parse_error().is_some());
        assert_eq!(events[0].raw_value(), None);

        let validation_error = events[1].error().expect("validation error is returned");
        assert!(validation_error.as_type_validation_error().is_some());
        assert_eq!(events[1].raw_value(), Some(&json!({ "name": "John" })));
    }

    #[test]
    fn create_event_source_response_handler_returns_results_and_headers() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())]);
        let options = EventSourceResponseHandlerOptions::new(
            b"data: {\"name\":\"John\",\"age\":30}\n\n".to_vec(),
        )
        .with_response_headers(response_headers.clone());

        let result = create_event_source_response_handler(options, validate_person)
            .expect("event source response is handled");

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(
            result.value(),
            &vec![ParseJsonResult::success(
                Person {
                    name: "John".to_string(),
                    age: 30,
                },
                json!({ "name": "John", "age": 30 })
            )]
        );
        assert_eq!(result.raw_value(), None);
    }

    #[test]
    fn create_event_source_response_handler_returns_empty_body_error_for_missing_body() {
        let error = create_event_source_response_handler(
            EventSourceResponseHandlerOptions::empty(),
            validate_person,
        )
        .expect_err("missing body is rejected");

        assert_eq!(error.message(), "Empty response body");
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
    fn with_provider_utils_user_agent_adds_version_and_runtime_suffixes() {
        let headers = with_provider_utils_user_agent(
            Some(vec![
                ("Authorization", Some("Bearer token")),
                ("X-Ignore", None),
            ]),
            &RuntimeEnvironment::navigator_user_agent("Deno/2.0 TEST"),
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/deno/2.0 test",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn with_provider_utils_user_agent_appends_to_existing_user_agent() {
        let headers = with_provider_utils_user_agent(
            Some(vec![("User-Agent", Some("MyApp/1.0"))]),
            &RuntimeEnvironment::node_js("v22.0.0"),
        );

        assert_eq!(
            headers,
            BTreeMap::from([(
                "user-agent".to_string(),
                format!(
                    "MyApp/1.0 ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                    crate::VERSION
                ),
            )])
        );
    }

    #[test]
    fn provider_api_request_serializes_upstream_prepared_request_shape() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/models",
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "ai-sdk/provider-utils/test".to_string(),
                ),
            ]),
            ProviderApiRequestBody::text("{\"model\":\"test\"}"),
            json!({ "model": "test" }),
        );

        let serialized = serde_json::to_value(&request).expect("request serializes");

        assert_eq!(
            serialized,
            json!({
                "method": "POST",
                "url": "https://api.example.com/v1/models",
                "headers": {
                    "content-type": "application/json",
                    "user-agent": "ai-sdk/provider-utils/test"
                },
                "body": {
                    "type": "text",
                    "content": "{\"model\":\"test\"}"
                },
                "requestBodyValues": { "model": "test" }
            })
        );

        let deserialized: ProviderApiRequest =
            serde_json::from_value(serialized).expect("request deserializes");

        assert_eq!(deserialized, request);
        assert_eq!(deserialized.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            deserialized
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text),
            Some("{\"model\":\"test\"}")
        );
    }

    #[test]
    fn provider_api_request_body_supports_binary_content() {
        let body = ProviderApiRequestBody::bytes([1_u8, 2, 3]);
        let serialized = serde_json::to_value(&body).expect("body serializes");

        assert_eq!(
            serialized,
            json!({
                "type": "bytes",
                "content": [1, 2, 3]
            })
        );

        let deserialized: ProviderApiRequestBody =
            serde_json::from_value(serialized).expect("body deserializes");

        assert_eq!(deserialized.as_bytes(), Some([1_u8, 2, 3].as_slice()));
    }

    #[test]
    fn provider_api_response_serializes_upstream_response_metadata_shape() {
        let response = ProviderApiResponse::text(201, "Created", r#"{"ok":true}"#).with_headers(
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        );

        let serialized = serde_json::to_value(&response).expect("response serializes");

        assert_eq!(
            serialized,
            json!({
                "statusCode": 201,
                "statusText": "Created",
                "headers": {
                    "content-type": "application/json"
                },
                "body": {
                    "type": "text",
                    "content": "{\"ok\":true}"
                }
            })
        );

        let deserialized: ProviderApiResponse =
            serde_json::from_value(serialized).expect("response deserializes");

        assert_eq!(deserialized, response);
        assert!(deserialized.is_success_status());
        assert_eq!(deserialized.text_body(), Some(r#"{"ok":true}"#));
        assert_eq!(deserialized.bytes_body(), None);
    }

    #[test]
    fn provider_api_response_body_supports_binary_content() {
        let body = ProviderApiResponseBody::bytes([4_u8, 5, 6]);
        let serialized = serde_json::to_value(&body).expect("body serializes");

        assert_eq!(
            serialized,
            json!({
                "type": "bytes",
                "content": [4, 5, 6]
            })
        );

        let deserialized: ProviderApiResponseBody =
            serde_json::from_value(serialized).expect("body deserializes");

        assert_eq!(deserialized.as_bytes(), Some([4_u8, 5, 6].as_slice()));
        assert_eq!(deserialized.as_text(), None);
    }

    #[test]
    fn provider_api_response_success_status_matches_fetch_ok_range() {
        for status_code in [200, 204, 299] {
            assert!(
                ProviderApiResponse::new(status_code, "OK").is_success_status(),
                "{status_code} should be successful"
            );
        }

        for status_code in [199, 300, 404, 500] {
            assert!(
                !ProviderApiResponse::new(status_code, "Error").is_success_status(),
                "{status_code} should be failed"
            );
        }
    }

    #[test]
    fn provider_api_response_builds_text_response_handler_options() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/chat",
            BTreeMap::from([("authorization".to_string(), "Bearer test".to_string())]),
            ProviderApiRequestBody::text("{\"prompt\":\"hi\"}"),
            json!({ "prompt": "hi" }),
        );
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_123".to_string())]);
        let response =
            ProviderApiResponse::text(429, "Too Many Requests", r#"{"error":"rate_limit"}"#)
                .with_headers(response_headers.clone());

        let status_options = response.status_code_error_response_handler_options(&request);
        assert_eq!(status_options.url, "https://api.example.com/v1/chat");
        assert_eq!(
            status_options.request_body_values,
            json!({ "prompt": "hi" })
        );
        assert_eq!(status_options.status_code, 429);
        assert_eq!(status_options.status_text, "Too Many Requests");
        assert_eq!(status_options.response_headers, response_headers);
        assert_eq!(status_options.response_body, r#"{"error":"rate_limit"}"#);

        let json_error_options = response.json_error_response_handler_options(&request);
        assert_eq!(json_error_options.status_text, "Too Many Requests");
        assert_eq!(
            json_error_options.response_body,
            r#"{"error":"rate_limit"}"#
        );

        let json_options = response.json_response_handler_options(&request);
        assert_eq!(json_options.status_code, 429);
        assert_eq!(json_options.response_body, r#"{"error":"rate_limit"}"#);
    }

    #[test]
    fn provider_api_response_builds_binary_and_event_source_handler_options() {
        let request = ProviderApiRequest::get(
            "https://api.example.com/v1/events",
            BTreeMap::from([("accept".to_string(), "text/event-stream".to_string())]),
        );
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())]);
        let response = ProviderApiResponse::bytes(200, "OK", [b'd', b'a', b't', b'a'])
            .with_headers(response_headers.clone());

        let binary_options = response.binary_response_handler_options(&request);
        assert_eq!(binary_options.url, "https://api.example.com/v1/events");
        assert_eq!(binary_options.request_body_values, json!({}));
        assert_eq!(binary_options.status_code, 200);
        assert_eq!(binary_options.response_headers, response_headers);
        assert_eq!(binary_options.response_body, Some(b"data".to_vec()));

        let event_options = response.event_source_response_handler_options();
        assert_eq!(event_options.response_body, Some(b"data".to_vec()));
        assert_eq!(
            event_options.response_headers,
            BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())])
        );

        let empty_options =
            ProviderApiResponse::new(204, "No Content").binary_response_handler_options(&request);
        assert_eq!(empty_options.response_body, None);
    }

    #[test]
    fn provider_api_response_decodes_binary_text_like_fetch_response_text() {
        let request = ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new());
        let response = ProviderApiResponse::bytes(200, "OK", [b'{', b'}']);

        let options = response.json_response_handler_options(&request);

        assert_eq!(options.response_body, "{}");
    }

    #[test]
    fn provider_api_response_handler_error_serializes_tagged_shape() {
        let error = ProviderApiResponseHandlerError::api_call(
            ApiCallError::new(
                "provider failed",
                "https://api.example.com/v1/data",
                json!({}),
            )
            .with_status_code(500),
        );

        assert_eq!(
            serde_json::to_value(&error).expect("handler error serializes"),
            json!({
                "type": "api-call",
                "error": {
                    "message": "provider failed",
                    "url": "https://api.example.com/v1/data",
                    "requestBodyValues": {},
                    "statusCode": 500,
                    "isRetryable": true
                }
            })
        );

        let deserialized: ProviderApiResponseHandlerError = serde_json::from_value(json!({
            "type": "other",
            "message": "invalid handler state"
        }))
        .expect("handler error deserializes");

        assert_eq!(deserialized.other_message(), Some("invalid handler state"));
        assert_eq!(deserialized.api_call_error(), None);
    }

    #[test]
    fn handle_provider_api_response_returns_successful_handler_result() {
        let request = ProviderApiRequest::get(
            "https://api.example.com/v1/data",
            BTreeMap::from([("authorization".to_string(), "Bearer test".to_string())]),
        );
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_success".to_string())]);
        let response = ProviderApiResponse::text(200, "OK", r#"{"name":"Ada","age":36}"#)
            .with_headers(response_headers.clone());

        let result = handle_provider_api_response(
            &request,
            &response,
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect("successful response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(
            result.raw_value(),
            Some(&json!({ "name": "Ada", "age": 36 }))
        );
        assert_eq!(result.response_headers(), Some(&response_headers));
    }

    #[test]
    fn handle_provider_api_response_returns_failed_handler_api_error() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/chat",
            BTreeMap::new(),
            ProviderApiRequestBody::text("{\"prompt\":\"hi\"}"),
            json!({ "prompt": "hi" }),
        );
        let response = ProviderApiResponse::text(429, "Too Many Requests", "rate limited");

        let error = handle_provider_api_response::<Person, _, _>(
            &request,
            &response,
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect_err("unsuccessful status returns the failed handler error");

        assert_eq!(error.message(), "Too Many Requests");
        assert_eq!(error.status_code(), Some(429));
        assert_eq!(error.response_body(), Some("rate limited"));
        assert_eq!(error.request_body_values(), &json!({ "prompt": "hi" }));
        assert!(error.is_retryable());
    }

    #[test]
    fn handle_provider_api_response_wraps_non_api_handler_failures() {
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_wrapper".to_string())]);
        let request = ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new());
        let response =
            ProviderApiResponse::text(200, "OK", "not json").with_headers(response_headers.clone());

        let error = handle_provider_api_response::<Person, _, _>(
            &request,
            &response,
            |_request, _response| Err(ProviderApiResponseHandlerError::other("validator crashed")),
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect_err("non-api handler errors are wrapped");

        assert_eq!(error.message(), "Failed to process successful response");
        assert_eq!(error.url(), "https://api.example.com/v1/data");
        assert_eq!(error.request_body_values(), &json!({}));
        assert_eq!(error.status_code(), Some(200));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), None);
    }

    #[test]
    fn handle_provider_api_response_passes_through_api_handler_failures() {
        let request = ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new());
        let response = ProviderApiResponse::text(200, "OK", "not json");
        let api_error = ApiCallError::new("Invalid JSON response", request.url.clone(), json!({}))
            .with_status_code(200);

        let error = handle_provider_api_response::<Person, _, _>(
            &request,
            &response,
            |_request, _response| Err(ProviderApiResponseHandlerError::api_call(api_error.clone())),
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect_err("api-call handler errors are passed through");

        assert_eq!(*error, api_error);
    }

    #[test]
    fn execute_provider_api_request_sends_prepared_request_and_handles_success() {
        let request = prepare_get_from_api_request(
            "https://api.example.com/v1/data",
            Some(vec![("Authorization", Some("Bearer test"))]),
            &RuntimeEnvironment::unknown(),
        );
        let expected_request = request.clone();
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_execute".to_string())]);
        let expected_response_headers = response_headers.clone();

        let result = poll_ready(execute_provider_api_request(
            request,
            move |request| {
                assert_eq!(request, expected_request);

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("successful transport response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn execute_provider_api_request_normalizes_transport_failures() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/chat",
            BTreeMap::new(),
            ProviderApiRequestBody::text("{\"prompt\":\"hi\"}"),
            json!({ "prompt": "hi" }),
        );

        let error = poll_ready(execute_provider_api_request(
            request,
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("ECONNRESET")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: ECONNRESET");
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "hi" }));
        assert!(error.is_retryable());
    }

    #[test]
    fn execute_provider_api_request_preserves_abort_transport_failures() {
        let abort_error = FetchErrorInfo::new("Aborted").with_name("AbortError");
        let error = poll_ready(execute_provider_api_request(
            ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new()),
            {
                let abort_error = abort_error.clone();
                move |_request| ready(Err(abort_error))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("abort failure is preserved");

        assert_eq!(error, HandledFetchError::Original { error: abort_error });
    }

    #[test]
    fn get_from_api_options_serialize_camel_case_request_metadata() {
        let options = GetFromApiOptions::new("https://api.example.com/v1/data")
            .with_headers(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ])
            .with_environment(RuntimeEnvironment::node_js("v22.0.0"));

        assert_eq!(
            serde_json::to_value(&options).expect("get-from-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/data",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "environment": {
                    "nodeVersion": "v22.0.0"
                }
            })
        );

        let options: GetFromApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/data"
        }))
        .expect("minimal get-from-api options deserialize");

        assert_eq!(
            options,
            GetFromApiOptions::new("https://api.example.com/v1/data")
        );
    }

    #[test]
    fn get_from_api_prepares_request_and_handles_success() {
        let options = GetFromApiOptions::new("https://api.example.com/v1/data")
            .with_header("Authorization", "Bearer test")
            .with_environment(RuntimeEnvironment::navigator_user_agent("Deno/2.0 TEST"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_get".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(get_from_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Get);
                assert_eq!(request.url, "https://api.example.com/v1/data");
                assert_eq!(request.body, None);
                assert_eq!(request.request_body_values, json!({}));
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/deno/2.0 test",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("get-from-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn get_from_api_normalizes_transport_failures() {
        let error = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.example.com/v1/data"),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("Failed to connect")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("get-from-api transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: Failed to connect");
        assert_eq!(error.url(), "https://api.example.com/v1/data");
        assert_eq!(error.request_body_values(), &json!({}));
        assert!(error.is_retryable());
    }

    #[test]
    fn post_json_to_api_options_serialize_camel_case_request_metadata() {
        let options =
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_headers(vec![
                    ("Authorization", Some("Bearer test")),
                    ("X-Ignore", None),
                ])
                .with_environment(RuntimeEnvironment::vercel_edge());

        assert_eq!(
            serde_json::to_value(&options).expect("post-json-to-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/chat",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "body": {
                    "prompt": "Hi"
                },
                "environment": {
                    "hasEdgeRuntime": true
                }
            })
        );

        let options: PostJsonToApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/chat",
            "body": {
                "prompt": "Hi"
            }
        }))
        .expect("minimal post-json-to-api options deserialize");

        assert_eq!(
            options,
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
        );
    }

    #[test]
    fn post_json_to_api_prepares_request_and_handles_success() {
        let options =
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_header("Authorization", "Bearer test")
                .with_environment(RuntimeEnvironment::node_js("v22.0.0"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_post_json".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(post_json_to_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Post);
                assert_eq!(request.url, "https://api.example.com/v1/chat");
                assert_eq!(request.request_body_values, json!({ "prompt": "Hi" }));
                assert_eq!(
                    request
                        .body
                        .as_ref()
                        .and_then(ProviderApiRequestBody::as_text),
                    Some("{\"prompt\":\"Hi\"}")
                );
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        ("content-type".to_string(), "application/json".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("post-json-to-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn post_json_to_api_normalizes_transport_failures() {
        let error = poll_ready(post_json_to_api(
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" })),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("ECONNREFUSED")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("post-json-to-api transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: ECONNREFUSED");
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "Hi" }));
        assert!(error.is_retryable());
    }

    #[test]
    fn post_to_api_options_serialize_camel_case_request_metadata() {
        let options = PostToApiOptions::new(
            "https://api.example.com/v1/upload",
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
        )
        .with_headers(vec![
            ("Authorization", Some("Bearer test")),
            ("X-Ignore", None),
        ])
        .with_environment(RuntimeEnvironment::vercel_edge());

        assert_eq!(
            serde_json::to_value(&options).expect("post-to-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/upload",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "body": {
                    "type": "bytes",
                    "content": [1, 2, 3]
                },
                "requestBodyValues": {
                    "filename": "image.png"
                },
                "environment": {
                    "hasEdgeRuntime": true
                }
            })
        );

        let options: PostToApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/upload",
            "body": {
                "type": "text",
                "content": "plain body"
            },
            "requestBodyValues": {
                "filename": "notes.txt"
            }
        }))
        .expect("minimal post-to-api options deserialize");

        assert_eq!(
            options,
            PostToApiOptions::new(
                "https://api.example.com/v1/upload",
                ProviderApiRequestBody::text("plain body"),
                json!({ "filename": "notes.txt" })
            )
        );
    }

    #[test]
    fn post_to_api_prepares_request_and_handles_success() {
        let options = PostToApiOptions::new(
            "https://api.example.com/v1/upload",
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
        )
        .with_header("Authorization", "Bearer test")
        .with_environment(RuntimeEnvironment::navigator_user_agent("Bun/1.2 TEST"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_post".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(post_to_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Post);
                assert_eq!(request.url, "https://api.example.com/v1/upload");
                assert_eq!(
                    request.request_body_values,
                    json!({ "filename": "image.png" })
                );
                assert_eq!(
                    request
                        .body
                        .as_ref()
                        .and_then(ProviderApiRequestBody::as_bytes),
                    Some([1_u8, 2, 3].as_slice())
                );
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/bun/1.2 test",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("post-to-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn post_to_api_normalizes_transport_failures() {
        let error = poll_ready(post_to_api(
            PostToApiOptions::new(
                "https://api.example.com/v1/upload",
                ProviderApiRequestBody::bytes([1_u8, 2, 3]),
                json!({ "filename": "image.png" }),
            ),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("EPIPE")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("post-to-api transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: EPIPE");
        assert_eq!(error.url(), "https://api.example.com/v1/upload");
        assert_eq!(
            error.request_body_values(),
            &json!({ "filename": "image.png" })
        );
        assert!(error.is_retryable());
    }

    #[test]
    fn prepare_get_from_api_request_matches_upstream_request_setup() {
        let request = prepare_get_from_api_request(
            "https://api.example.com/data",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            &RuntimeEnvironment::navigator_user_agent("Deno/2.0 TEST"),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(request.url, "https://api.example.com/data");
        assert_eq!(request.body, None);
        assert_eq!(request.request_body_values, json!({}));
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/deno/2.0 test",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_to_api_request_matches_upstream_binary_request_setup() {
        let request = prepare_post_to_api_request(
            "https://api.example.com/upload",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
            &RuntimeEnvironment::vercel_edge(),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.example.com/upload");
        assert_eq!(
            request.request_body_values,
            json!({ "filename": "image.png" })
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_bytes),
            Some([1_u8, 2, 3].as_slice())
        );
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/vercel-edge",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_json_to_api_request_matches_upstream_request_setup() {
        let request = prepare_post_json_to_api_request(
            "https://api.example.com/data",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            json!({ "model": "test", "prompt": "Hello" }),
            &RuntimeEnvironment::node_js("v22.0.0"),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.example.com/data");
        assert_eq!(
            request.request_body_values,
            json!({ "model": "test", "prompt": "Hello" })
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text),
            Some("{\"model\":\"test\",\"prompt\":\"Hello\"}")
        );
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_json_to_api_request_allows_header_overrides() {
        let request = prepare_post_json_to_api_request(
            "https://api.example.com/data",
            Some(vec![
                ("Content-Type", Some("application/custom+json")),
                ("User-Agent", Some("MyApp/1.0")),
            ]),
            json!({ "input": "test" }),
            &RuntimeEnvironment::unknown(),
        );

        assert_eq!(
            request.headers,
            BTreeMap::from([
                (
                    "content-type".to_string(),
                    "application/custom+json".to_string()
                ),
                (
                    "user-agent".to_string(),
                    format!(
                        "MyApp/1.0 ai-sdk/provider-utils/{} runtime/unknown",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn runtime_environment_serializes_camel_case_shape() {
        let environment = RuntimeEnvironment {
            has_window: true,
            navigator_user_agent: Some("Node/Test".to_string()),
            node_version: Some("v22.0.0".to_string()),
            has_edge_runtime: true,
        };

        assert_eq!(
            serde_json::to_value(&environment).expect("runtime environment serializes"),
            json!({
                "hasWindow": true,
                "navigatorUserAgent": "Node/Test",
                "nodeVersion": "v22.0.0",
                "hasEdgeRuntime": true
            })
        );

        let environment: RuntimeEnvironment = serde_json::from_value(json!({
            "navigatorUserAgent": "Deno/2.0",
        }))
        .expect("runtime environment deserializes");

        assert_eq!(
            environment,
            RuntimeEnvironment {
                has_window: false,
                navigator_user_agent: Some("Deno/2.0".to_string()),
                node_version: None,
                has_edge_runtime: false,
            }
        );
    }

    #[test]
    fn runtime_environment_omits_unknown_indicators() {
        assert_eq!(
            serde_json::to_value(RuntimeEnvironment::unknown())
                .expect("unknown runtime environment serializes"),
            json!({})
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_matches_upstream_branches() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::browser()),
            "runtime/browser"
        );
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::navigator_user_agent(
                "Deno/2.0 TEST"
            )),
            "runtime/deno/2.0 test"
        );
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::node_js("v22.0.0")),
            "runtime/node.js/v22.0.0"
        );
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::vercel_edge()),
            "runtime/vercel-edge"
        );
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::unknown()),
            "runtime/unknown"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_uses_upstream_probe_precedence() {
        let browser_environment = RuntimeEnvironment {
            has_window: true,
            navigator_user_agent: Some("Bun/1.2".to_string()),
            node_version: Some("v22.0.0".to_string()),
            has_edge_runtime: true,
        };
        assert_eq!(
            get_runtime_environment_user_agent(&browser_environment),
            "runtime/browser"
        );

        let navigator_environment = RuntimeEnvironment {
            has_window: false,
            navigator_user_agent: Some("Bun/1.2".to_string()),
            node_version: Some("v22.0.0".to_string()),
            has_edge_runtime: true,
        };
        assert_eq!(
            get_runtime_environment_user_agent(&navigator_environment),
            "runtime/bun/1.2"
        );

        let node_environment = RuntimeEnvironment {
            has_window: false,
            navigator_user_agent: Some(String::new()),
            node_version: Some("v20.11.1".to_string()),
            has_edge_runtime: true,
        };
        assert_eq!(
            get_runtime_environment_user_agent(&node_environment),
            "runtime/node.js/v20.11.1"
        );
    }

    #[test]
    fn is_abort_error_matches_upstream_error_names() {
        for error_name in ["AbortError", "ResponseAborted", "TimeoutError"] {
            assert!(
                is_abort_error(error_name),
                "{error_name} should be treated as an abort error"
            );
        }

        for error_name in ["aborterror", "Response aborted", "TypeError", ""] {
            assert!(
                !is_abort_error(error_name),
                "{error_name:?} should not be treated as an abort error"
            );
        }
    }

    #[test]
    fn fetch_error_info_serializes_camel_case_shape() {
        let error = FetchErrorInfo::new("fetch failed")
            .with_name("TypeError")
            .with_code("ECONNRESET")
            .with_cause_message("socket closed");

        assert_eq!(
            serde_json::to_value(&error).expect("fetch error info serializes"),
            json!({
                "name": "TypeError",
                "message": "fetch failed",
                "code": "ECONNRESET",
                "causeMessage": "socket closed"
            })
        );

        let minimal: FetchErrorInfo = serde_json::from_value(json!({
            "message": "unexpected"
        }))
        .expect("minimal fetch error info deserializes");

        assert_eq!(minimal.message(), "unexpected");
        assert_eq!(minimal.name(), None);
        assert_eq!(minimal.code(), None);
        assert_eq!(minimal.cause_message(), None);
    }

    #[test]
    fn handled_fetch_error_serializes_tagged_api_call_result() {
        let result = HandledFetchError::ApiCall {
            error: Box::new(
                crate::ApiCallError::new(
                    "Cannot connect to API: ECONNREFUSED",
                    "https://api.example.com/v1/chat",
                    json!({ "prompt": "test" }),
                )
                .with_is_retryable(true),
            ),
        };

        assert_eq!(
            serde_json::to_value(&result).expect("handled fetch error serializes"),
            json!({
                "type": "api-call",
                "error": {
                    "message": "Cannot connect to API: ECONNREFUSED",
                    "url": "https://api.example.com/v1/chat",
                    "requestBodyValues": { "prompt": "test" },
                    "isRetryable": true
                }
            })
        );

        let original: HandledFetchError = serde_json::from_value(json!({
            "type": "original",
            "error": {
                "name": "AbortError",
                "message": "Aborted"
            }
        }))
        .expect("handled original fetch error deserializes");

        assert_eq!(
            original.original_error().map(FetchErrorInfo::name),
            Some(Some("AbortError"))
        );
        assert!(original.api_call_error().is_none());
    }

    #[test]
    fn handle_fetch_error_returns_abort_errors_unchanged() {
        let error = FetchErrorInfo::new("Aborted").with_name("AbortError");

        let result =
            handle_fetch_error(error.clone(), "https://api.example.com/v1/chat", json!({}));

        assert_eq!(result, HandledFetchError::Original { error });
    }

    #[test]
    fn handle_fetch_error_wraps_node_fetch_failed_type_errors() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("fetch failed")
                .with_name("TypeError")
                .with_cause_message("ECONNREFUSED"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        let HandledFetchError::ApiCall { error } = result else {
            panic!("fetch failed TypeError should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: ECONNREFUSED");
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "test" }));
        assert!(error.is_retryable());
        assert_eq!(error.status_code(), None);
    }

    #[test]
    fn handle_fetch_error_wraps_browser_failed_to_fetch_type_errors() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("Failed to fetch")
                .with_name("TypeError")
                .with_cause_message("Network error"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        let HandledFetchError::ApiCall { error } = result else {
            panic!("failed to fetch TypeError should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: Network error");
        assert!(error.is_retryable());
    }

    #[test]
    fn handle_fetch_error_leaves_fetch_failed_type_errors_without_cause_unchanged() {
        let error = FetchErrorInfo::new("fetch failed").with_name("TypeError");

        let result =
            handle_fetch_error(error.clone(), "https://api.example.com/v1/chat", json!({}));

        assert_eq!(result, HandledFetchError::Original { error });
    }

    #[test]
    fn handle_fetch_error_wraps_bun_network_errors() {
        for code in [
            "ConnectionRefused",
            "ConnectionClosed",
            "FailedToOpenSocket",
            "ECONNRESET",
            "ECONNREFUSED",
            "ETIMEDOUT",
            "EPIPE",
        ] {
            let result = handle_fetch_error(
                FetchErrorInfo::new("socket unavailable").with_code(code),
                "https://api.example.com/v1/chat",
                json!({ "prompt": "test" }),
            );

            let HandledFetchError::ApiCall { error } = result else {
                panic!("{code} should become an API call error");
            };

            assert_eq!(error.message(), "Cannot connect to API: socket unavailable");
            assert!(error.is_retryable());
        }
    }

    #[test]
    fn handle_fetch_error_returns_unknown_errors_unchanged() {
        let error = FetchErrorInfo::new("Something unexpected");

        let result =
            handle_fetch_error(error.clone(), "https://api.example.com/v1/chat", json!({}));

        assert_eq!(result, HandledFetchError::Original { error });
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
    fn tool_metadata_is_retained_but_not_sent_to_provider() {
        let metadata = json!({
            "source": "mcp",
            "server": "weather-tools"
        })
        .as_object()
        .expect("metadata is an object")
        .clone();
        let tool = Tool::new("weather", object_schema()).with_metadata(metadata.clone());

        assert_eq!(tool.metadata(), Some(&metadata));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn tool_title_is_retained_but_not_sent_to_provider() {
        let tool = Tool::new("weather", object_schema()).with_title("Weather information");

        assert_eq!(tool.title(), Some("Weather information"));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn dynamic_tool_prepares_upstream_function_tool_shape() {
        let tool = dynamic_tool("mcpWeather", object_schema())
            .with_description("Runtime weather lookup.")
            .with_strict(true);

        assert!(tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), None);
        assert_eq!(tool.provider_tool_args(), None);
        assert_eq!(tool.output_schema(), None);
        assert_eq!(tool.supports_deferred_results(), None);
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "mcpWeather",
                "description": "Runtime weather lookup.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "strict": true
            })
        );
    }

    #[test]
    fn tool_prepares_upstream_provider_defined_tool_shape() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" }
            }
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let tool = Tool::provider_defined(
            "webSearch",
            "provider.web_search",
            args.clone(),
            object_schema(),
        )
        .with_output_schema(output_schema.clone());

        assert!(tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("provider.web_search"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(tool.supports_deferred_results(), None);
        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "provider.web_search",
                "webSearch",
                args.clone()
            ))
        );
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "provider",
                "id": "provider.web_search",
                "name": "webSearch",
                "args": {
                    "maxResults": 3
                }
            })
        );
    }

    #[test]
    fn tool_prepares_upstream_provider_executed_tool_shape() {
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = json!({ "type": "object" })
            .as_object()
            .expect("output schema is an object")
            .clone();
        let tool = Tool::provider_executed(
            "codeInterpreter",
            "provider.code_interpreter",
            args.clone(),
            object_schema(),
            output_schema.clone(),
        )
        .with_supports_deferred_results(true);

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert!(!tool.is_executable());
        assert_eq!(tool.provider_tool_id(), Some("provider.code_interpreter"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(tool.supports_deferred_results(), Some(true));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "provider",
                "id": "provider.code_interpreter",
                "name": "codeInterpreter",
                "args": {
                    "region": "au"
                }
            })
        );
    }

    #[test]
    fn provider_defined_tool_factory_round_trips_upstream_config_shape() {
        let output_schema = json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" }
            }
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let factory = create_provider_defined_tool_factory_with_output_schema(
            "provider.web_search",
            object_schema(),
            output_schema.clone(),
        );

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.web_search",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "results": { "type": "array" }
                    }
                }
            })
        );

        let deserialized: ProviderDefinedToolFactory = serde_json::from_value(json!({
            "id": "provider.web_search",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "results": { "type": "array" }
                }
            }
        }))
        .expect("factory deserializes");

        assert_eq!(
            deserialized,
            ProviderDefinedToolFactory::new("provider.web_search", object_schema())
                .with_output_schema(output_schema)
        );
    }

    #[test]
    fn provider_defined_tool_factory_creates_provider_tool() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = json!({ "type": "string" })
            .as_object()
            .expect("output schema is an object")
            .clone();
        let tool = create_provider_defined_tool_factory_with_output_schema(
            "provider.web_search",
            object_schema(),
            output_schema.clone(),
        )
        .tool("webSearch", args.clone());

        assert!(tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("provider.web_search"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "provider.web_search",
                "webSearch",
                args
            ))
        );
    }

    #[test]
    fn provider_executed_tool_factory_round_trips_upstream_config_and_creates_tool() {
        let output_schema = json!({
            "type": "object",
            "properties": {
                "result": { "type": "string" }
            }
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let factory = create_provider_executed_tool_factory(
            "provider.code_interpreter",
            object_schema(),
            output_schema.clone(),
        )
        .with_supports_deferred_results(true);

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.code_interpreter",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "result": { "type": "string" }
                    }
                },
                "supportsDeferredResults": true
            })
        );

        let deserialized: ProviderExecutedToolFactory = serde_json::from_value(json!({
            "id": "provider.code_interpreter",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "result": { "type": "string" }
                }
            },
            "supportsDeferredResults": true
        }))
        .expect("factory deserializes");

        assert_eq!(deserialized, factory);

        let tool = factory.tool("codeInterpreter", args.clone());

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("provider.code_interpreter"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(tool.supports_deferred_results(), Some(true));
        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "provider.code_interpreter",
                "codeInterpreter",
                args
            ))
        );
    }

    #[test]
    fn provider_defined_tool_factory_omits_missing_output_schema() {
        let factory = create_provider_defined_tool_factory("provider.web_search", object_schema());

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.web_search",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );

        let deserialized: ProviderDefinedToolFactory = serde_json::from_value(json!({
            "id": "provider.web_search",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }
        }))
        .expect("factory deserializes");

        assert_eq!(deserialized, factory);
    }

    #[test]
    fn provider_executed_tool_factory_omits_missing_deferred_results_support() {
        let output_schema = json!({ "type": "object" })
            .as_object()
            .expect("output schema is an object")
            .clone();
        let factory = create_provider_executed_tool_factory(
            "provider.code_interpreter",
            object_schema(),
            output_schema,
        );

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.code_interpreter",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "outputSchema": {
                    "type": "object"
                }
            })
        );

        let deserialized: ProviderExecutedToolFactory = serde_json::from_value(json!({
            "id": "provider.code_interpreter",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            },
            "outputSchema": {
                "type": "object"
            }
        }))
        .expect("factory deserializes");

        assert_eq!(deserialized, factory);
    }

    #[test]
    fn prepare_tools_returns_none_for_empty_tool_sets() {
        assert_eq!(prepare_tools(Vec::<Tool>::new().iter()), None);
    }

    #[test]
    fn prepare_tools_converts_high_level_tools() {
        let provider_tool_args = json!({ "key": "value" })
            .as_object()
            .expect("args are an object")
            .clone();
        let tools = vec![
            Tool::new("weather", object_schema()),
            dynamic_tool("runtimeWeather", object_schema()),
            Tool::provider_defined(
                "providerTool",
                "provider.tool-id",
                provider_tool_args.clone(),
                object_schema(),
            ),
        ];

        assert_eq!(
            prepare_tools(&tools),
            Some(vec![
                LanguageModelTool::Function(LanguageModelFunctionTool::new(
                    "weather",
                    object_schema()
                )),
                LanguageModelTool::Function(LanguageModelFunctionTool::new(
                    "runtimeWeather",
                    object_schema()
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "provider.tool-id",
                    "providerTool",
                    provider_tool_args
                ))
            ])
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
        )
        .with_context(json!({
            "apiKey": "secret"
        }));

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
                ],
                "context": {
                    "apiKey": "secret"
                }
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
