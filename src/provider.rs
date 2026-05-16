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
        EmptyResponseBodyError, LoadApiKeyError, LoadSettingError, ModelType, NoSuchModelError,
        ProviderOptions, TooManyEmbeddingValuesForCallError, UnsupportedFunctionalityError,
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
