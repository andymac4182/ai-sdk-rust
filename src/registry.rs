use std::fmt;

use crate::provider::ModelType;

/// Error returned when a provider registry cannot resolve a provider id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoSuchProviderError {
    model_id: String,
    model_type: ModelType,
    provider_id: String,
    available_providers: Vec<String>,
    message: String,
}

impl NoSuchProviderError {
    /// Creates a missing-provider error with the upstream default message.
    pub fn new(
        model_id: impl Into<String>,
        model_type: ModelType,
        provider_id: impl Into<String>,
        available_providers: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let provider_id = provider_id.into();
        let available_providers = available_providers
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let message = no_such_provider_default_message(&provider_id, &available_providers);

        Self {
            model_id: model_id.into(),
            model_type,
            provider_id,
            available_providers,
            message,
        }
    }

    /// Creates a missing-provider error with a caller-supplied message.
    pub fn with_message(
        model_id: impl Into<String>,
        model_type: ModelType,
        provider_id: impl Into<String>,
        available_providers: impl IntoIterator<Item = impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            model_type,
            provider_id: provider_id.into(),
            available_providers: available_providers.into_iter().map(Into::into).collect(),
            message: message.into(),
        }
    }

    /// Returns the full registry lookup id that failed.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the model category requested from the registry.
    pub fn model_type(&self) -> ModelType {
        self.model_type
    }

    /// Returns the provider id extracted from the failed lookup.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Returns the provider ids registered in the registry at lookup time.
    pub fn available_providers(&self) -> &[String] {
        &self.available_providers
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained lookup context and message.
    pub fn into_parts(self) -> (String, ModelType, String, Vec<String>, String) {
        (
            self.model_id,
            self.model_type,
            self.provider_id,
            self.available_providers,
            self.message,
        )
    }
}

impl fmt::Display for NoSuchProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoSuchProviderError {}

fn no_such_provider_default_message(provider_id: &str, available_providers: &[String]) -> String {
    format!(
        "No such provider: {} (available providers: {})",
        provider_id,
        available_providers.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::NoSuchProviderError;
    use crate::provider::ModelType;

    #[test]
    fn no_such_provider_error_matches_upstream_default_message() {
        let error = NoSuchProviderError::new(
            "openai:gpt-4.1",
            ModelType::LanguageModel,
            "openai",
            ["anthropic", "google"],
        );

        assert_eq!(error.model_id(), "openai:gpt-4.1");
        assert_eq!(error.model_type(), ModelType::LanguageModel);
        assert_eq!(error.provider_id(), "openai");
        assert_eq!(
            error.available_providers(),
            &["anthropic".to_string(), "google".to_string()]
        );
        assert_eq!(
            error.message(),
            "No such provider: openai (available providers: anthropic,google)"
        );
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic,google)"
        );
    }

    #[test]
    fn no_such_provider_error_retains_custom_message_context() {
        let error = NoSuchProviderError::with_message(
            "missing",
            ModelType::EmbeddingModel,
            "missing",
            ["openai"],
            "registry lookup failed",
        );

        assert_eq!(error.message(), "registry lookup failed");

        let (model_id, model_type, provider_id, available_providers, message) = error.into_parts();
        assert_eq!(model_id, "missing");
        assert_eq!(model_type, ModelType::EmbeddingModel);
        assert_eq!(provider_id, "missing");
        assert_eq!(available_providers, vec!["openai".to_string()]);
        assert_eq!(message, "registry lookup failed");
    }
}
