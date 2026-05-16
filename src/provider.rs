use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::json::{JsonObject, JsonValue};

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
}

impl NoSuchModelError {
    /// Creates an error for a missing provider model.
    pub fn new(model_id: impl Into<String>, model_type: ModelType) -> Self {
        Self {
            model_id: model_id.into(),
            model_type,
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

    /// Converts this error into the requested provider-specific model id.
    pub fn into_model_id(self) -> String {
        self.model_id
    }
}

impl fmt::Display for NoSuchModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "No such {}: {}", self.model_type, self.model_id)
    }
}

impl std::error::Error for NoSuchModelError {}

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
    context: Option<TypeValidationContext>,
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
            context,
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
        self.context.as_ref()
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
        (self.value, self.cause_message, self.context)
    }
}

impl fmt::Display for TypeValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TypeValidationError {}

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
        EmptyResponseBodyError, InvalidArgumentError, InvalidPromptError, InvalidResponseDataError,
        JsonParseError, LoadApiKeyError, LoadSettingError, ModelType, NoContentGeneratedError,
        NoSuchModelError, ProviderOptions, TooManyEmbeddingValuesForCallError,
        TypeValidationContext, TypeValidationError, UnsupportedFunctionalityError,
    };
    use serde_json::json;

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
    fn no_such_model_error_matches_upstream_context() {
        let error = NoSuchModelError::new("gpt-4.1", ModelType::LanguageModel);

        assert_eq!(error.model_id(), "gpt-4.1");
        assert_eq!(error.model_type(), ModelType::LanguageModel);
        assert_eq!(error.to_string(), "No such languageModel: gpt-4.1");
        assert_eq!(error.into_model_id(), "gpt-4.1");
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
