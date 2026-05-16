use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};

/// Returns a stable human-readable message for optional error-like values.
///
/// This mirrors upstream `getErrorMessage` behavior for the Rust boundary:
/// `None` maps to `unknown error`, strings display as-is, and JSON or error
/// values use their [`fmt::Display`] representation.
pub fn get_error_message(error: Option<&dyn fmt::Display>) -> String {
    error.map_or_else(|| "unknown error".to_string(), ToString::to_string)
}

/// Provider and model interface specification versions supported by this crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum SpecificationVersion {
    /// The upstream provider-v4 interface version.
    #[serde(rename = "v4")]
    V4,
}

impl SpecificationVersion {
    /// Returns the upstream specification version string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "v4",
        }
    }
}

impl fmt::Display for SpecificationVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The upstream provider model categories used when reporting missing models.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelType {
    /// Language model lookup.
    LanguageModel,

    /// Embedding model lookup.
    EmbeddingModel,

    /// Image model lookup.
    ImageModel,

    /// Transcription model lookup.
    TranscriptionModel,

    /// Speech model lookup.
    SpeechModel,

    /// Reranking model lookup.
    RerankingModel,

    /// Video model lookup.
    VideoModel,
}

impl ModelType {
    /// Returns the upstream provider-v4 model type string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LanguageModel => "languageModel",
            Self::EmbeddingModel => "embeddingModel",
            Self::ImageModel => "imageModel",
            Self::TranscriptionModel => "transcriptionModel",
            Self::SpeechModel => "speechModel",
            Self::RerankingModel => "rerankingModel",
            Self::VideoModel => "videoModel",
        }
    }
}

impl fmt::Display for ModelType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Error returned when a provider cannot resolve a requested model id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoSuchModelError {
    model_id: String,
    model_type: ModelType,
    message: String,
}

impl NoSuchModelError {
    /// Creates an error for a missing provider model.
    pub fn new(model_id: impl Into<String>, model_type: ModelType) -> Self {
        let model_id = model_id.into();
        let message = no_such_model_default_message(&model_id, model_type);

        Self {
            model_id,
            model_type,
            message,
        }
    }

    /// Creates an error with a caller-supplied message.
    pub fn with_message(
        model_id: impl Into<String>,
        model_type: ModelType,
        message: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            model_type,
            message: message.into(),
        }
    }

    /// Returns the requested provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the upstream provider model category that was requested.
    pub fn model_type(&self) -> ModelType {
        self.model_type
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the requested provider-specific model id.
    pub fn into_model_id(self) -> String {
        self.model_id
    }

    /// Converts this error into its retained context and message.
    pub fn into_parts(self) -> (String, ModelType, String) {
        (self.model_id, self.model_type, self.message)
    }
}

impl fmt::Display for NoSuchModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoSuchModelError {}

fn no_such_model_default_message(model_id: &str, model_type: ModelType) -> String {
    format!("No such {model_type}: {model_id}")
}

/// A provider-v4 provider for required model lookups.
///
/// Upstream `ProviderV4` requires language, embedding, and image model lookup
/// methods. Optional upstream provider methods are represented as opt-in
/// extension traits so providers do not need placeholder associated types for
/// unsupported capabilities.
pub trait Provider {
    /// Language model type returned by this provider.
    type LanguageModel: crate::language_model::LanguageModel;

    /// Embedding model type returned by this provider.
    type EmbeddingModel: crate::embedding_model::EmbeddingModel;

    /// Image model type returned by this provider.
    type ImageModel: crate::image_model::ImageModel;

    /// Returns the provider interface version implemented by this provider.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Returns the language model with the given provider-specific id.
    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError>;

    /// Returns the embedding model with the given provider-specific id.
    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError>;

    /// Returns the image model with the given provider-specific id.
    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError>;
}

/// Optional provider-v4 transcription model lookup support.
pub trait ProviderWithTranscriptionModel: Provider {
    /// Transcription model type returned by this provider.
    type TranscriptionModel: crate::transcription_model::TranscriptionModel;

    /// Returns the transcription model with the given provider-specific id.
    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError>;
}

/// Optional provider-v4 speech model lookup support.
pub trait ProviderWithSpeechModel: Provider {
    /// Speech model type returned by this provider.
    type SpeechModel: crate::speech_model::SpeechModel;

    /// Returns the speech model with the given provider-specific id.
    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError>;
}

/// Optional provider-v4 reranking model lookup support.
pub trait ProviderWithRerankingModel: Provider {
    /// Reranking model type returned by this provider.
    type RerankingModel: crate::reranking_model::RerankingModel;

    /// Returns the reranking model with the given provider-specific id.
    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError>;
}

/// Optional provider-v4 video model lookup support.
pub trait ProviderWithVideoModel: Provider {
    /// Video model type returned by this provider.
    type VideoModel: crate::video_model::VideoModel;

    /// Returns the video model with the given provider-specific id.
    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError>;
}

/// Optional provider-v4 file upload support.
pub trait ProviderWithFiles: Provider {
    /// Files interface type returned by this provider.
    type Files: crate::files::Files;

    /// Returns the files interface for this provider.
    fn files(&self) -> Self::Files;
}

/// Optional provider-v4 skill upload support.
pub trait ProviderWithSkills: Provider {
    /// Skills interface type returned by this provider.
    type Skills: crate::skills::Skills;

    /// Returns the skills interface for this provider.
    fn skills(&self) -> Self::Skills;
}

/// Error returned when a provider cannot support requested functionality.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedFunctionalityError {
    functionality: String,
    message: String,
}

impl UnsupportedFunctionalityError {
    /// Creates an error with the upstream default unsupported-functionality message.
    pub fn new(functionality: impl Into<String>) -> Self {
        let functionality = functionality.into();
        Self {
            message: format!("'{functionality}' functionality not supported."),
            functionality,
        }
    }

    /// Creates an error with a provider-specific message.
    pub fn with_message(functionality: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            functionality: functionality.into(),
            message: message.into(),
        }
    }

    /// Returns the unsupported functionality identifier.
    pub fn functionality(&self) -> &str {
        &self.functionality
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the unsupported functionality identifier.
    pub fn into_functionality(self) -> String {
        self.functionality
    }
}

impl fmt::Display for UnsupportedFunctionalityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for UnsupportedFunctionalityError {}

/// Error returned when an API key cannot be loaded for a provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadApiKeyError {
    message: String,
}

impl LoadApiKeyError {
    /// Creates an API key loading error with the upstream provider message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the human-readable error message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for LoadApiKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LoadApiKeyError {}

/// Error returned when a provider setting cannot be loaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadSettingError {
    message: String,
}

impl LoadSettingError {
    /// Creates a provider setting loading error with the upstream provider message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the human-readable error message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for LoadSettingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LoadSettingError {}

/// Error returned when a provider or provider utility receives an invalid argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidArgumentError {
    argument: String,
    message: String,
}

impl InvalidArgumentError {
    /// Creates an invalid argument error with the upstream provider message.
    pub fn new(argument: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            argument: argument.into(),
            message: message.into(),
        }
    }

    /// Returns the invalid argument name.
    pub fn argument(&self) -> &str {
        &self.argument
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the invalid argument name and message.
    pub fn into_parts(self) -> (String, String) {
        (self.argument, self.message)
    }
}

impl fmt::Display for InvalidArgumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidArgumentError {}

/// Error returned when a provider cannot process a prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidPromptError {
    prompt: JsonValue,
    message: String,
}

impl InvalidPromptError {
    /// Creates an invalid prompt error with the upstream provider message prefix.
    pub fn new(prompt: impl Into<JsonValue>, message: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            message: format!("Invalid prompt: {}", message.into()),
        }
    }

    /// Returns the prompt value that could not be processed.
    pub fn prompt(&self) -> &JsonValue {
        &self.prompt
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the prompt value that could not be processed.
    pub fn into_prompt(self) -> JsonValue {
        self.prompt
    }
}

impl fmt::Display for InvalidPromptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidPromptError {}

/// Optional context about the value being type-validated.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeValidationContext {
    /// Field path in dot notation, such as `message.metadata` or `message.parts[3].data`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,

    /// Entity name, such as a tool name or data type name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,

    /// Entity identifier, such as a message ID or tool call ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

impl TypeValidationContext {
    /// Creates empty type-validation context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the field path being validated.
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Adds the entity name being validated.
    pub fn with_entity_name(mut self, entity_name: impl Into<String>) -> Self {
        self.entity_name = Some(entity_name.into());
        self
    }

    /// Adds the entity identifier being validated.
    pub fn with_entity_id(mut self, entity_id: impl Into<String>) -> Self {
        self.entity_id = Some(entity_id.into());
        self
    }

    fn message_prefix(&self) -> String {
        let mut prefix = "Type validation failed".to_string();

        if let Some(field) = &self.field {
            prefix.push_str(" for ");
            prefix.push_str(field);
        }

        if self.entity_name.is_some() || self.entity_id.is_some() {
            prefix.push_str(" (");

            let mut separator = "";
            if let Some(entity_name) = &self.entity_name {
                prefix.push_str(entity_name);
                separator = ", ";
            }
            if let Some(entity_id) = &self.entity_id {
                prefix.push_str(separator);
                prefix.push_str("id: \"");
                prefix.push_str(entity_id);
                prefix.push('"');
            }

            prefix.push(')');
        }

        prefix
    }
}

/// Error returned when provider or SDK data fails type validation.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeValidationError {
    value: JsonValue,
    context: Option<Box<TypeValidationContext>>,
    cause_message: String,
    message: String,
}

impl TypeValidationError {
    /// Creates a type-validation error from a failed value and validation cause.
    pub fn new(
        value: impl Into<JsonValue>,
        cause: impl fmt::Display,
        context: Option<TypeValidationContext>,
    ) -> Self {
        Self::with_cause_message(value, cause.to_string(), context)
    }

    /// Creates a type-validation error from a failed value and upstream-style cause message.
    pub fn with_cause_message(
        value: impl Into<JsonValue>,
        cause_message: impl Into<String>,
        context: Option<TypeValidationContext>,
    ) -> Self {
        let value = value.into();
        let cause_message = cause_message.into();
        let rendered_value = serde_json::to_string(&value).expect("JSON values serialize");
        let context_prefix = context.as_ref().map_or_else(
            || "Type validation failed".to_string(),
            |context| context.message_prefix(),
        );

        Self {
            value,
            context: context.map(Box::new),
            message: format!(
                "{context_prefix}: Value: {rendered_value}.\nError message: {cause_message}"
            ),
            cause_message,
        }
    }

    /// Returns the value that failed type validation.
    pub fn value(&self) -> &JsonValue {
        &self.value
    }

    /// Returns optional context about the value being validated.
    pub fn context(&self) -> Option<&TypeValidationContext> {
        self.context.as_deref()
    }

    /// Returns the human-readable validation cause message.
    pub fn cause_message(&self) -> &str {
        &self.cause_message
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the failed value, cause message, and optional validation context.
    pub fn into_parts(self) -> (JsonValue, String, Option<TypeValidationContext>) {
        (
            self.value,
            self.cause_message,
            self.context.map(|context| *context),
        )
    }
}

impl fmt::Display for TypeValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TypeValidationError {}

/// Error returned when an HTTP provider API call fails.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCallError {
    message: String,
    url: String,
    request_body_values: JsonValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_headers: Option<Headers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_body: Option<String>,
    is_retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<JsonValue>,
}

impl ApiCallError {
    /// Creates an API call error with the required upstream context.
    pub fn new(
        message: impl Into<String>,
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self {
            message: message.into(),
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code: None,
            response_headers: None,
            response_body: None,
            is_retryable: false,
            data: None,
        }
    }

    /// Returns whether the upstream default retry rule marks this status as retryable.
    pub const fn is_retryable_status_code(status_code: u16) -> bool {
        matches!(status_code, 408 | 409 | 429 | 500..=599)
    }

    /// Sets the provider response status code and applies the upstream default retry rule.
    pub fn with_status_code(mut self, status_code: u16) -> Self {
        self.is_retryable = Self::is_retryable_status_code(status_code);
        self.status_code = Some(status_code);
        self
    }

    /// Sets response headers returned by the provider.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = Some(response_headers);
        self
    }

    /// Adds a response header returned by the provider.
    pub fn with_response_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.response_headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the raw response body returned by the provider.
    pub fn with_response_body(mut self, response_body: impl Into<String>) -> Self {
        self.response_body = Some(response_body.into());
        self
    }

    /// Overrides whether the failed API call should be retried.
    pub fn with_is_retryable(mut self, is_retryable: bool) -> Self {
        self.is_retryable = is_retryable;
        self
    }

    /// Sets parsed provider error data.
    pub fn with_data(mut self, data: impl Into<JsonValue>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the URL that was called.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the provider request body values.
    pub fn request_body_values(&self) -> &JsonValue {
        &self.request_body_values
    }

    /// Returns the provider response status code.
    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    /// Returns the provider response headers.
    pub fn response_headers(&self) -> Option<&Headers> {
        self.response_headers.as_ref()
    }

    /// Returns the raw provider response body.
    pub fn response_body(&self) -> Option<&str> {
        self.response_body.as_deref()
    }

    /// Returns whether the failed API call should be retried.
    pub fn is_retryable(&self) -> bool {
        self.is_retryable
    }

    /// Returns parsed provider error data.
    pub fn data(&self) -> Option<&JsonValue> {
        self.data.as_ref()
    }

    /// Converts this error into the provider request body values.
    pub fn into_request_body_values(self) -> JsonValue {
        self.request_body_values
    }
}

impl<'de> Deserialize<'de> for ApiCallError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ApiCallErrorFields {
            message: String,
            url: String,
            request_body_values: JsonValue,
            #[serde(default)]
            status_code: Option<u16>,
            #[serde(default)]
            response_headers: Option<Headers>,
            #[serde(default)]
            response_body: Option<String>,
            #[serde(default)]
            is_retryable: Option<bool>,
            #[serde(default, deserialize_with = "deserialize_optional_json_value")]
            data: Option<JsonValue>,
        }

        let fields = ApiCallErrorFields::deserialize(deserializer)?;
        let is_retryable = fields.is_retryable.unwrap_or_else(|| {
            fields
                .status_code
                .is_some_and(Self::is_retryable_status_code)
        });

        Ok(Self {
            message: fields.message,
            url: fields.url,
            request_body_values: fields.request_body_values,
            status_code: fields.status_code,
            response_headers: fields.response_headers,
            response_body: fields.response_body,
            is_retryable,
            data: fields.data,
        })
    }
}

impl fmt::Display for ApiCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiCallError {}

fn deserialize_optional_json_value<'de, D>(deserializer: D) -> Result<Option<JsonValue>, D::Error>
where
    D: Deserializer<'de>,
{
    JsonValue::deserialize(deserializer).map(Some)
}

/// Error returned when provider JSON parsing fails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonParseError {
    text: String,
    cause_message: String,
    message: String,
}

impl JsonParseError {
    /// Creates a JSON parse error from the failed text and parse error.
    pub fn new(text: impl Into<String>, cause: impl fmt::Display) -> Self {
        Self::with_cause_message(text, cause.to_string())
    }

    /// Creates a JSON parse error from the failed text and upstream-style cause message.
    pub fn with_cause_message(text: impl Into<String>, cause_message: impl Into<String>) -> Self {
        let text = text.into();
        let cause_message = cause_message.into();

        Self {
            message: format!("JSON parsing failed: Text: {text}.\nError message: {cause_message}"),
            text,
            cause_message,
        }
    }

    /// Returns the text that failed JSON parsing.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the human-readable cause message.
    pub fn cause_message(&self) -> &str {
        &self.cause_message
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the failed text and cause message.
    pub fn into_parts(self) -> (String, String) {
        (self.text, self.cause_message)
    }
}

impl fmt::Display for JsonParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for JsonParseError {}

/// Error returned when a provider response has no body to parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmptyResponseBodyError {
    message: String,
}

impl EmptyResponseBodyError {
    /// Creates an empty response body error with the upstream default message.
    pub fn new() -> Self {
        Self::with_message("Empty response body")
    }

    /// Creates an empty response body error with a provider-specific message.
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the human-readable error message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl Default for EmptyResponseBodyError {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EmptyResponseBodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EmptyResponseBodyError {}

/// Error returned when a provider completes without generating content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoContentGeneratedError {
    message: String,
}

impl NoContentGeneratedError {
    /// Creates a no-content error with the upstream default message.
    pub fn new() -> Self {
        Self::with_message("No content generated.")
    }

    /// Creates a no-content error with a provider-specific message.
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the human-readable error message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl Default for NoContentGeneratedError {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NoContentGeneratedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoContentGeneratedError {}

/// Error returned when a provider response contains invalid data.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidResponseDataError {
    data: JsonValue,
    message: String,
}

impl InvalidResponseDataError {
    /// Creates an invalid response data error with the upstream default message.
    pub fn new(data: impl Into<JsonValue>) -> Self {
        let data = data.into();
        let rendered_data = serde_json::to_string(&data).expect("JSON values serialize");

        Self {
            data,
            message: format!("Invalid response data: {rendered_data}."),
        }
    }

    /// Creates an invalid response data error with a provider-specific message.
    pub fn with_message(data: impl Into<JsonValue>, message: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            message: message.into(),
        }
    }

    /// Returns the provider response data that failed validation.
    pub fn data(&self) -> &JsonValue {
        &self.data
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the provider response data that failed validation.
    pub fn into_data(self) -> JsonValue {
        self.data
    }
}

impl fmt::Display for InvalidResponseDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidResponseDataError {}

/// Error returned when an embedding model call contains too many values.
#[derive(Clone, Debug, PartialEq)]
pub struct TooManyEmbeddingValuesForCallError {
    provider: String,
    model_id: String,
    max_embeddings_per_call: usize,
    values: Vec<JsonValue>,
}

impl TooManyEmbeddingValuesForCallError {
    /// Creates an error for an embedding call that exceeded the provider limit.
    pub fn new<V, I>(
        provider: impl Into<String>,
        model_id: impl Into<String>,
        max_embeddings_per_call: usize,
        values: I,
    ) -> Self
    where
        V: Into<JsonValue>,
        I: IntoIterator<Item = V>,
    {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
            max_embeddings_per_call,
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns the provider name associated with the embedding model.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the provider-specific embedding model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the maximum values the model supports in one embedding call.
    pub fn max_embeddings_per_call(&self) -> usize {
        self.max_embeddings_per_call
    }

    /// Returns the values that exceeded the provider limit.
    pub fn values(&self) -> &[JsonValue] {
        &self.values
    }

    /// Returns the number of values that exceeded the provider limit.
    pub fn value_count(&self) -> usize {
        self.values.len()
    }

    /// Converts this error into the values that exceeded the provider limit.
    pub fn into_values(self) -> Vec<JsonValue> {
        self.values
    }
}

impl fmt::Display for TooManyEmbeddingValuesForCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Too many values for a single embedding call. The {} model \"{}\" can only embed up to {} values per call, but {} values were provided.",
            self.provider,
            self.model_id,
            self.max_embeddings_per_call,
            self.values.len()
        )
    }
}

impl std::error::Error for TooManyEmbeddingValuesForCallError {}

/// Additional provider-specific options passed through to a model provider.
///
/// The outer map is keyed by provider name and the inner object contains
/// provider-specific option keys.
pub type ProviderOptions = BTreeMap<String, JsonObject>;

/// Additional provider-specific metadata returned by a model provider.
///
/// The shape matches [`ProviderOptions`], but represents provider outputs rather
/// than provider inputs.
pub type ProviderMetadata = BTreeMap<String, JsonObject>;

#[cfg(test)]
mod tests {
    use super::{
        ApiCallError, EmptyResponseBodyError, InvalidArgumentError, InvalidPromptError,
        InvalidResponseDataError, JsonParseError, LoadApiKeyError, LoadSettingError, ModelType,
        NoContentGeneratedError, NoSuchModelError, Provider, ProviderOptions, ProviderWithFiles,
        ProviderWithRerankingModel, ProviderWithSkills, ProviderWithSpeechModel,
        ProviderWithTranscriptionModel, ProviderWithVideoModel, SpecificationVersion,
        TooManyEmbeddingValuesForCallError, TypeValidationContext, TypeValidationError,
        UnsupportedFunctionalityError, get_error_message,
    };
    use std::collections::BTreeMap;
    use std::fmt;
    use std::future::{Ready, ready};

    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
    use crate::file_data::{FileDataContent, ProviderReference};
    use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileResult};
    use crate::image_model::{
        ImageModel, ImageModelCallOptions, ImageModelResponse, ImageModelResult,
    };
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelStreamPart,
        LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelText,
        LanguageModelUsage,
    };
    use crate::reranking_model::{RerankingModel, RerankingModelCallOptions, RerankingModelResult};
    use crate::skills::{Skills, SkillsUploadSkillCallOptions, SkillsUploadSkillResult};
    use crate::speech_model::{
        SpeechModel, SpeechModelAudio, SpeechModelCallOptions, SpeechModelResponse,
        SpeechModelResult,
    };
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
        TranscriptionModelResult,
    };
    use crate::video_model::{
        VideoModel, VideoModelCallOptions, VideoModelResponse, VideoModelResult,
        VideoModelVideoData,
    };
    use serde_json::json;
    use time::OffsetDateTime;

    #[derive(Debug)]
    struct StaticLanguageModel {
        model_id: &'static str,
    }

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
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(LanguageModelSupportedUrls::new())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Text(LanguageModelText::new("ok"))],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    #[derive(Debug)]
    struct StaticEmbeddingModel {
        model_id: &'static str,
    }

    impl EmbeddingModel for StaticEmbeddingModel {
        type MaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;
        type SupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a;
        type EmbedFuture<'a>
            = Ready<EmbeddingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
            ready(Some(16))
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(true)
        }

        fn do_embed(&self, _options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            ready(EmbeddingModelResult::new(vec![vec![1.0, 2.0]]))
        }
    }

    #[derive(Debug)]
    struct StaticImageModel {
        model_id: &'static str,
    }

    impl ImageModel for StaticImageModel {
        type MaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;
        type GenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            ready(Some(4))
        }

        fn do_generate(&self, _options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(ImageModelResult::new(
                vec![FileDataContent::Bytes(Vec::new())],
                ImageModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id),
            ))
        }
    }

    #[derive(Debug)]
    struct StaticTranscriptionModel {
        model_id: &'static str,
    }

    impl TranscriptionModel for StaticTranscriptionModel {
        type GenerateFuture<'a>
            = Ready<TranscriptionModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn do_generate(&self, _options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(TranscriptionModelResult::new(
                "",
                Vec::new(),
                TranscriptionModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id),
            ))
        }
    }

    #[derive(Debug)]
    struct StaticSpeechModel {
        model_id: &'static str,
    }

    impl SpeechModel for StaticSpeechModel {
        type GenerateFuture<'a>
            = Ready<SpeechModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn do_generate(&self, _options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
            let audio: SpeechModelAudio = FileDataContent::Bytes(Vec::new());

            ready(SpeechModelResult::new(
                audio,
                SpeechModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id),
            ))
        }
    }

    #[derive(Debug)]
    struct StaticRerankingModel {
        model_id: &'static str,
    }

    impl RerankingModel for StaticRerankingModel {
        type RerankFuture<'a>
            = Ready<RerankingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn do_rerank(&self, _options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
            ready(RerankingModelResult::new(Vec::new()))
        }
    }

    #[derive(Debug)]
    struct StaticVideoModel {
        model_id: &'static str,
    }

    impl VideoModel for StaticVideoModel {
        type MaxVideosPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;
        type GenerateFuture<'a>
            = Ready<VideoModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn model_id(&self) -> &str {
            self.model_id
        }

        fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
            ready(Some(1))
        }

        fn do_generate(&self, _options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(VideoModelResult::new(
                vec![VideoModelVideoData::base64("AAAAIGZ0eXBtcDQy", "video/mp4")],
                VideoModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id),
            ))
        }
    }

    #[derive(Debug)]
    struct StaticFiles;

    impl Files for StaticFiles {
        type UploadFileFuture<'a>
            = Ready<FilesUploadFileResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn upload_file(&self, _options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
            ready(FilesUploadFileResult::new(provider_reference("file_123")))
        }
    }

    #[derive(Debug)]
    struct StaticSkills;

    impl Skills for StaticSkills {
        type UploadSkillFuture<'a>
            = Ready<SkillsUploadSkillResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "mock-provider"
        }

        fn upload_skill(
            &self,
            _options: SkillsUploadSkillCallOptions,
        ) -> Self::UploadSkillFuture<'_> {
            ready(SkillsUploadSkillResult::new(provider_reference(
                "skill_123",
            )))
        }
    }

    struct StaticProvider;

    impl Provider for StaticProvider {
        type LanguageModel = StaticLanguageModel;
        type EmbeddingModel = StaticEmbeddingModel;
        type ImageModel = StaticImageModel;

        fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "chat",
                ModelType::LanguageModel,
                StaticLanguageModel { model_id: "chat" },
            )
        }

        fn embedding_model(
            &self,
            model_id: &str,
        ) -> Result<Self::EmbeddingModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "embed",
                ModelType::EmbeddingModel,
                StaticEmbeddingModel { model_id: "embed" },
            )
        }

        fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "image",
                ModelType::ImageModel,
                StaticImageModel { model_id: "image" },
            )
        }
    }

    impl ProviderWithTranscriptionModel for StaticProvider {
        type TranscriptionModel = StaticTranscriptionModel;

        fn transcription_model(
            &self,
            model_id: &str,
        ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "transcribe",
                ModelType::TranscriptionModel,
                StaticTranscriptionModel {
                    model_id: "transcribe",
                },
            )
        }
    }

    impl ProviderWithSpeechModel for StaticProvider {
        type SpeechModel = StaticSpeechModel;

        fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "speech",
                ModelType::SpeechModel,
                StaticSpeechModel { model_id: "speech" },
            )
        }
    }

    impl ProviderWithRerankingModel for StaticProvider {
        type RerankingModel = StaticRerankingModel;

        fn reranking_model(
            &self,
            model_id: &str,
        ) -> Result<Self::RerankingModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "rerank",
                ModelType::RerankingModel,
                StaticRerankingModel { model_id: "rerank" },
            )
        }
    }

    impl ProviderWithVideoModel for StaticProvider {
        type VideoModel = StaticVideoModel;

        fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
            lookup_model(
                model_id,
                "video",
                ModelType::VideoModel,
                StaticVideoModel { model_id: "video" },
            )
        }
    }

    impl ProviderWithFiles for StaticProvider {
        type Files = StaticFiles;

        fn files(&self) -> Self::Files {
            StaticFiles
        }
    }

    impl ProviderWithSkills for StaticProvider {
        type Skills = StaticSkills;

        fn skills(&self) -> Self::Skills {
            StaticSkills
        }
    }

    fn lookup_model<T>(
        requested_model_id: &str,
        expected_model_id: &str,
        model_type: ModelType,
        model: T,
    ) -> Result<T, NoSuchModelError> {
        if requested_model_id == expected_model_id {
            Ok(model)
        } else {
            Err(NoSuchModelError::new(requested_model_id, model_type))
        }
    }

    fn provider_reference(id: impl Into<String>) -> ProviderReference {
        ProviderReference::try_from(BTreeMap::from([("mock-provider".to_string(), id.into())]))
            .expect("provider reference has one entry")
    }

    #[test]
    fn get_error_message_matches_upstream_unknown_string_error_and_json_cases() {
        #[derive(Debug)]
        struct ProviderFailure;

        impl fmt::Display for ProviderFailure {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("ProviderFailure: request timed out")
            }
        }

        assert_eq!(get_error_message(None), "unknown error");
        assert_eq!(
            get_error_message(Some(&"something went wrong")),
            "something went wrong"
        );
        assert_eq!(get_error_message(Some(&"")), "");
        assert_eq!(
            get_error_message(Some(&ProviderFailure)),
            "ProviderFailure: request timed out"
        );
        assert_eq!(
            get_error_message(Some(&json!({
                "code": "FAIL",
                "detail": "oops"
            }))),
            "{\"code\":\"FAIL\",\"detail\":\"oops\"}"
        );
        assert_eq!(get_error_message(Some(&json!(42))), "42");
        assert_eq!(get_error_message(Some(&json!(false))), "false");
        assert_eq!(get_error_message(Some(&json!(["a", "b"]))), "[\"a\",\"b\"]");
    }

    #[test]
    fn model_type_serializes_as_upstream_model_type_strings() {
        assert_eq!(
            serde_json::to_value([
                ModelType::LanguageModel,
                ModelType::EmbeddingModel,
                ModelType::ImageModel,
                ModelType::TranscriptionModel,
                ModelType::SpeechModel,
                ModelType::RerankingModel,
                ModelType::VideoModel,
            ])
            .expect("model types serialize"),
            json!([
                "languageModel",
                "embeddingModel",
                "imageModel",
                "transcriptionModel",
                "speechModel",
                "rerankingModel",
                "videoModel"
            ])
        );
    }

    #[test]
    fn model_type_deserializes_from_upstream_model_type_string() {
        let model_type: ModelType =
            serde_json::from_value(json!("rerankingModel")).expect("model type deserializes");

        assert_eq!(model_type, ModelType::RerankingModel);
        assert_eq!(model_type.as_str(), "rerankingModel");
        assert_eq!(model_type.to_string(), "rerankingModel");
    }

    #[test]
    fn specification_version_serializes_as_upstream_v4_literal() {
        assert_eq!(
            serde_json::to_value(SpecificationVersion::V4)
                .expect("specification version serializes"),
            json!("v4")
        );
        assert_eq!(SpecificationVersion::V4.as_str(), "v4");
        assert_eq!(SpecificationVersion::V4.to_string(), "v4");
    }

    #[test]
    fn specification_version_deserializes_from_upstream_v4_literal() {
        let version: SpecificationVersion =
            serde_json::from_value(json!("v4")).expect("specification version deserializes");

        assert_eq!(version, SpecificationVersion::V4);
    }

    #[test]
    fn provider_resolves_required_v4_model_interfaces() {
        let provider = StaticProvider;

        assert_eq!(provider.specification_version(), SpecificationVersion::V4);

        let language_model = provider
            .language_model("chat")
            .expect("language model resolves");
        assert_eq!(
            language_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(language_model.provider(), "mock-provider");
        assert_eq!(language_model.model_id(), "chat");

        let embedding_model = provider
            .embedding_model("embed")
            .expect("embedding model resolves");
        assert_eq!(
            embedding_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(embedding_model.provider(), "mock-provider");
        assert_eq!(embedding_model.model_id(), "embed");

        let image_model = provider.image_model("image").expect("image model resolves");
        assert_eq!(
            image_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(image_model.provider(), "mock-provider");
        assert_eq!(image_model.model_id(), "image");
    }

    #[test]
    fn provider_required_model_lookup_reports_missing_model_type() {
        let provider = StaticProvider;

        let error = provider
            .language_model("missing-chat")
            .expect_err("missing language model reports an error");

        assert_eq!(
            error,
            NoSuchModelError::new("missing-chat", ModelType::LanguageModel)
        );
        assert_eq!(error.to_string(), "No such languageModel: missing-chat");
    }

    #[test]
    fn provider_extension_traits_resolve_optional_v4_interfaces() {
        let provider = StaticProvider;

        let transcription_model =
            ProviderWithTranscriptionModel::transcription_model(&provider, "transcribe")
                .expect("transcription model resolves");
        assert_eq!(transcription_model.provider(), "mock-provider");
        assert_eq!(transcription_model.model_id(), "transcribe");

        let speech_model =
            ProviderWithSpeechModel::speech_model(&provider, "speech").expect("speech resolves");
        assert_eq!(speech_model.provider(), "mock-provider");
        assert_eq!(speech_model.model_id(), "speech");

        let reranking_model = ProviderWithRerankingModel::reranking_model(&provider, "rerank")
            .expect("reranking model resolves");
        assert_eq!(reranking_model.provider(), "mock-provider");
        assert_eq!(reranking_model.model_id(), "rerank");

        let video_model =
            ProviderWithVideoModel::video_model(&provider, "video").expect("video resolves");
        assert_eq!(video_model.provider(), "mock-provider");
        assert_eq!(video_model.model_id(), "video");

        let files = ProviderWithFiles::files(&provider);
        assert_eq!(files.specification_version(), SpecificationVersion::V4);
        assert_eq!(files.provider(), "mock-provider");

        let skills = ProviderWithSkills::skills(&provider);
        assert_eq!(skills.specification_version(), SpecificationVersion::V4);
        assert_eq!(skills.provider(), "mock-provider");
    }

    #[test]
    fn no_such_model_error_matches_upstream_context() {
        let error = NoSuchModelError::new("gpt-4.1", ModelType::LanguageModel);

        assert_eq!(error.model_id(), "gpt-4.1");
        assert_eq!(error.model_type(), ModelType::LanguageModel);
        assert_eq!(error.message(), "No such languageModel: gpt-4.1");
        assert_eq!(error.to_string(), "No such languageModel: gpt-4.1");
        assert_eq!(error.into_model_id(), "gpt-4.1");
    }

    #[test]
    fn no_such_model_error_accepts_upstream_custom_message() {
        let error = NoSuchModelError::with_message(
            "model",
            ModelType::EmbeddingModel,
            "Invalid embeddingModel id for registry: model (must be in the format \"providerId:modelId\")",
        );

        assert_eq!(error.model_id(), "model");
        assert_eq!(error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            error.message(),
            "Invalid embeddingModel id for registry: model (must be in the format \"providerId:modelId\")"
        );
        assert_eq!(
            error.to_string(),
            "Invalid embeddingModel id for registry: model (must be in the format \"providerId:modelId\")"
        );

        let (model_id, model_type, message) = error.into_parts();
        assert_eq!(model_id, "model");
        assert_eq!(model_type, ModelType::EmbeddingModel);
        assert_eq!(
            message,
            "Invalid embeddingModel id for registry: model (must be in the format \"providerId:modelId\")"
        );
    }

    #[test]
    fn unsupported_functionality_error_matches_upstream_default_message() {
        let error = UnsupportedFunctionalityError::new("File URL data");

        assert_eq!(error.functionality(), "File URL data");
        assert_eq!(
            error.message(),
            "'File URL data' functionality not supported."
        );
        assert_eq!(
            error.to_string(),
            "'File URL data' functionality not supported."
        );
        assert_eq!(error.into_functionality(), "File URL data");
    }

    #[test]
    fn unsupported_functionality_error_accepts_provider_specific_message() {
        let error = UnsupportedFunctionalityError::with_message(
            "image/avif",
            "Unsupported image mime type: image/avif, expected one of: image/jpeg, image/png.",
        );

        assert_eq!(error.functionality(), "image/avif");
        assert_eq!(
            error.to_string(),
            "Unsupported image mime type: image/avif, expected one of: image/jpeg, image/png."
        );
    }

    #[test]
    fn load_api_key_error_uses_upstream_message_contract() {
        let message = "OpenAI API key is missing. Pass it using the 'apiKey' parameter or the OPENAI_API_KEY environment variable.";
        let error = LoadApiKeyError::new(message);

        assert_eq!(error.message(), message);
        assert_eq!(error.to_string(), message);
        assert_eq!(error.into_message(), message);
    }

    #[test]
    fn load_setting_error_uses_upstream_message_contract() {
        let message = "Base URL setting is missing. Pass it using the 'baseURL' parameter or the OPENAI_BASE_URL environment variable.";
        let error = LoadSettingError::new(message);

        assert_eq!(error.message(), message);
        assert_eq!(error.to_string(), message);
        assert_eq!(error.into_message(), message);
    }

    #[test]
    fn invalid_argument_error_uses_upstream_message_contract() {
        let message = "The separator \"-\" must not be part of the alphabet \"0123456789-\".";
        let error = InvalidArgumentError::new("separator", message);

        assert_eq!(error.argument(), "separator");
        assert_eq!(error.message(), message);
        assert_eq!(error.to_string(), message);
        assert_eq!(
            error.into_parts(),
            ("separator".to_string(), message.to_string())
        );
    }

    #[test]
    fn invalid_prompt_error_uses_upstream_message_prefix_and_retains_prompt() {
        let prompt = json!({
            "prompt": "Hello",
            "messages": []
        });
        let error =
            InvalidPromptError::new(prompt.clone(), "prompt and messages cannot both be set.");

        assert_eq!(error.prompt(), &prompt);
        assert_eq!(
            error.message(),
            "Invalid prompt: prompt and messages cannot both be set."
        );
        assert_eq!(
            error.to_string(),
            "Invalid prompt: prompt and messages cannot both be set."
        );
        assert_eq!(error.into_prompt(), prompt);
    }

    #[test]
    fn type_validation_context_serializes_as_upstream_camel_case_shape() {
        let context = TypeValidationContext::new()
            .with_field("messages[0].parts[0].input")
            .with_entity_name("weather")
            .with_entity_id("toolu_123");

        let serialized = serde_json::to_value(&context).expect("context serializes");

        assert_eq!(
            serialized,
            json!({
                "field": "messages[0].parts[0].input",
                "entityName": "weather",
                "entityId": "toolu_123"
            })
        );
        assert_eq!(
            serde_json::from_value::<TypeValidationContext>(serialized)
                .expect("context deserializes"),
            context
        );
        assert_eq!(
            serde_json::to_value(TypeValidationContext::new()).expect("empty context serializes"),
            json!({})
        );
    }

    #[test]
    fn type_validation_error_matches_upstream_message_without_context() {
        let value = json!({
            "cities": "San Francisco"
        });
        let error = TypeValidationError::with_cause_message(
            value.clone(),
            "Expected array, received string",
            None,
        );

        assert_eq!(error.value(), &value);
        assert_eq!(error.context(), None);
        assert_eq!(error.cause_message(), "Expected array, received string");
        assert_eq!(
            error.message(),
            "Type validation failed: Value: {\"cities\":\"San Francisco\"}.\nError message: Expected array, received string"
        );
        assert_eq!(
            error.to_string(),
            "Type validation failed: Value: {\"cities\":\"San Francisco\"}.\nError message: Expected array, received string"
        );
        assert_eq!(
            error.into_parts(),
            (value, "Expected array, received string".to_string(), None)
        );
    }

    #[test]
    fn type_validation_error_matches_upstream_context_prefixes() {
        let value = json!({
            "foo": 123
        });
        let context = TypeValidationContext::new()
            .with_field("messages[0].parts[0].input")
            .with_entity_name("weather")
            .with_entity_id("1");
        let error = TypeValidationError::with_cause_message(
            value.clone(),
            "Expected string, received number",
            Some(context.clone()),
        );

        assert_eq!(error.value(), &value);
        assert_eq!(error.context(), Some(&context));
        assert_eq!(
            error.to_string(),
            "Type validation failed for messages[0].parts[0].input (weather, id: \"1\"): Value: {\"foo\":123}.\nError message: Expected string, received number"
        );
        assert_eq!(
            error.into_parts(),
            (
                value,
                "Expected string, received number".to_string(),
                Some(context)
            )
        );
    }

    #[test]
    fn type_validation_error_accepts_display_causes() {
        let cause = serde_json::from_str::<serde_json::Value>("{")
            .expect_err("invalid JSON should produce a serde_json parse error");
        let error = TypeValidationError::new(json!(null), &cause, None);

        assert_eq!(error.value(), &json!(null));
        assert_eq!(error.cause_message(), cause.to_string());
        assert_eq!(
            error.to_string(),
            format!(
                "Type validation failed: Value: null.\nError message: {}",
                cause
            )
        );
    }

    #[test]
    fn api_call_error_serializes_upstream_shape_and_retains_context() {
        let error = ApiCallError::new(
            "Rate limit exceeded",
            "https://api.example.com/v1/responses",
            json!({
                "model": "gpt-4.1",
                "input": "Hello"
            }),
        )
        .with_status_code(429)
        .with_response_header("retry-after", "2")
        .with_response_body("{\"error\":\"rate_limit\"}")
        .with_data(json!({
            "error": {
                "type": "rate_limit"
            }
        }));

        assert_eq!(error.message(), "Rate limit exceeded");
        assert_eq!(error.to_string(), "Rate limit exceeded");
        assert_eq!(error.url(), "https://api.example.com/v1/responses");
        assert_eq!(
            error.request_body_values(),
            &json!({
                "model": "gpt-4.1",
                "input": "Hello"
            })
        );
        assert_eq!(error.status_code(), Some(429));
        assert_eq!(error.response_body(), Some("{\"error\":\"rate_limit\"}"));
        assert!(error.is_retryable());
        assert_eq!(
            error
                .response_headers()
                .and_then(|headers| headers.get("retry-after")),
            Some(&"2".to_string())
        );
        assert_eq!(
            error.data(),
            Some(&json!({
                "error": {
                    "type": "rate_limit"
                }
            }))
        );

        let serialized = serde_json::to_value(&error).expect("API call error serializes");

        assert_eq!(
            serialized,
            json!({
                "message": "Rate limit exceeded",
                "url": "https://api.example.com/v1/responses",
                "requestBodyValues": {
                    "input": "Hello",
                    "model": "gpt-4.1"
                },
                "statusCode": 429,
                "responseHeaders": {
                    "retry-after": "2"
                },
                "responseBody": "{\"error\":\"rate_limit\"}",
                "isRetryable": true,
                "data": {
                    "error": {
                        "type": "rate_limit"
                    }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ApiCallError>(serialized)
                .expect("API call error deserializes"),
            error
        );
    }

    #[test]
    fn api_call_error_uses_upstream_default_retry_rule() {
        for (status_code, expected) in [
            (400, false),
            (408, true),
            (409, true),
            (429, true),
            (499, false),
            (500, true),
            (599, true),
        ] {
            let error = ApiCallError::new("Request failed", "https://api.example.com", json!({}))
                .with_status_code(status_code);

            assert_eq!(
                ApiCallError::is_retryable_status_code(status_code),
                expected
            );
            assert_eq!(error.is_retryable(), expected);
        }

        let error: ApiCallError = serde_json::from_value(json!({
            "message": "Internal Server Error",
            "url": "https://api.example.com",
            "requestBodyValues": {},
            "statusCode": 500
        }))
        .expect("API call error deserializes without explicit retry flag");

        assert!(error.is_retryable());
    }

    #[test]
    fn api_call_error_allows_retry_override_and_explicit_null_data() {
        let error = ApiCallError::new("Request failed", "https://api.example.com", json!({}))
            .with_status_code(500)
            .with_is_retryable(false)
            .with_data(json!(null));

        assert!(!error.is_retryable());
        assert_eq!(error.data(), Some(&json!(null)));
        assert_eq!(
            serde_json::to_value(&error).expect("API call error serializes"),
            json!({
                "message": "Request failed",
                "url": "https://api.example.com",
                "requestBodyValues": {},
                "statusCode": 500,
                "isRetryable": false,
                "data": null
            })
        );

        let deserialized: ApiCallError = serde_json::from_value(json!({
            "message": "Request failed",
            "url": "https://api.example.com",
            "requestBodyValues": {},
            "statusCode": 500,
            "isRetryable": false,
            "data": null
        }))
        .expect("API call error with null data deserializes");

        assert_eq!(deserialized.data(), Some(&json!(null)));
        assert!(!deserialized.is_retryable());
    }

    #[test]
    fn json_parse_error_matches_upstream_message_contract() {
        let text = "{ invalid json";
        let cause_message = "SyntaxError: Expected property name or '}' in JSON at position 2";
        let error = JsonParseError::with_cause_message(text, cause_message);

        assert_eq!(error.text(), text);
        assert_eq!(error.cause_message(), cause_message);
        assert_eq!(
            error.message(),
            "JSON parsing failed: Text: { invalid json.\nError message: SyntaxError: Expected property name or '}' in JSON at position 2"
        );
        assert_eq!(
            error.to_string(),
            "JSON parsing failed: Text: { invalid json.\nError message: SyntaxError: Expected property name or '}' in JSON at position 2"
        );
        assert_eq!(
            error.into_parts(),
            (text.to_string(), cause_message.to_string())
        );
    }

    #[test]
    fn json_parse_error_accepts_display_causes() {
        let cause = serde_json::from_str::<serde_json::Value>("not json")
            .expect_err("invalid JSON should produce a serde_json parse error");
        let error = JsonParseError::new("not json", &cause);

        assert_eq!(error.text(), "not json");
        assert_eq!(error.cause_message(), cause.to_string());
        assert_eq!(
            error.to_string(),
            format!(
                "JSON parsing failed: Text: not json.\nError message: {}",
                cause
            )
        );
    }

    #[test]
    fn empty_response_body_error_matches_upstream_default_message() {
        let error = EmptyResponseBodyError::new();

        assert_eq!(error.message(), "Empty response body");
        assert_eq!(error.to_string(), "Empty response body");
        assert_eq!(error.into_message(), "Empty response body");
        assert_eq!(
            EmptyResponseBodyError::default().to_string(),
            "Empty response body"
        );
    }

    #[test]
    fn empty_response_body_error_accepts_provider_specific_message() {
        let message = "Amazon Bedrock event stream response body is empty.";
        let error = EmptyResponseBodyError::with_message(message);

        assert_eq!(error.message(), message);
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn no_content_generated_error_matches_upstream_default_message() {
        let error = NoContentGeneratedError::new();

        assert_eq!(error.message(), "No content generated.");
        assert_eq!(error.to_string(), "No content generated.");
        assert_eq!(error.into_message(), "No content generated.");
        assert_eq!(
            NoContentGeneratedError::default().to_string(),
            "No content generated."
        );
    }

    #[test]
    fn no_content_generated_error_accepts_provider_specific_message() {
        let message = "Model returned only metadata and no text, tool call, or file content.";
        let error = NoContentGeneratedError::with_message(message);

        assert_eq!(error.message(), message);
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn invalid_response_data_error_matches_upstream_default_message() {
        let error = InvalidResponseDataError::new(json!({
            "state": "completed",
            "assets": {}
        }));

        assert_eq!(
            error.data(),
            &json!({
                "state": "completed",
                "assets": {}
            })
        );
        assert_eq!(
            error.message(),
            "Invalid response data: {\"assets\":{},\"state\":\"completed\"}."
        );
        assert_eq!(
            error.to_string(),
            "Invalid response data: {\"assets\":{},\"state\":\"completed\"}."
        );
        assert_eq!(
            error.into_data(),
            json!({
                "state": "completed",
                "assets": {}
            })
        );
    }

    #[test]
    fn invalid_response_data_error_accepts_provider_specific_message() {
        let data = json!({
            "type": "tool-call-delta",
            "function": {}
        });
        let message = "Expected 'function.name' to be a string.";
        let error = InvalidResponseDataError::with_message(data.clone(), message);

        assert_eq!(error.data(), &data);
        assert_eq!(error.message(), message);
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn too_many_embedding_values_error_matches_upstream_message_and_context() {
        let error = TooManyEmbeddingValuesForCallError::new(
            "openai",
            "text-embedding-3-small",
            2,
            ["first", "second", "third"],
        );

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.model_id(), "text-embedding-3-small");
        assert_eq!(error.max_embeddings_per_call(), 2);
        assert_eq!(error.value_count(), 3);
        assert_eq!(
            error.values(),
            &[json!("first"), json!("second"), json!("third")]
        );
        assert_eq!(
            error.to_string(),
            "Too many values for a single embedding call. The openai model \"text-embedding-3-small\" can only embed up to 2 values per call, but 3 values were provided."
        );
        assert_eq!(
            error.into_values(),
            vec![json!("first"), json!("second"), json!("third")]
        );
    }

    #[test]
    fn provider_options_serialize_as_nested_provider_objects() {
        let options: ProviderOptions = serde_json::from_value(json!({
            "anthropic": {
                "cacheControl": { "type": "ephemeral" }
            }
        }))
        .expect("provider options deserialize");

        assert_eq!(
            serde_json::to_value(options).expect("provider options serialize"),
            json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" }
                }
            })
        );
    }
}
