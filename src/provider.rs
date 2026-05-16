use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::json::JsonObject;

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
    use super::{ModelType, NoSuchModelError, ProviderOptions};
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
