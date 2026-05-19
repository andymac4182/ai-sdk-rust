use std::env;
use std::sync::Arc;

use ai_sdk_open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
};
use ai_sdk_openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleModelEntry, OpenAICompatibleModelListResponse, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use ai_sdk_provider::{Headers, NoSuchModelError, Provider};
use ai_sdk_provider_utils::HandledFetchError;
use serde::{Deserialize, Serialize};

/// OpenAI-compatible Vercel AI Gateway base URL.
pub const VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";

const VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_PROVIDER_NAME: &str = "vercel-ai-gateway";

/// Settings for Vercel AI Gateway's OpenAI-compatible provider surface.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelAiGatewayOpenAICompatibleSettings {
    /// OpenAI-compatible Gateway base URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// AI Gateway API key. When omitted, `AI_GATEWAY_API_KEY`, then
    /// `AI_SDK_RUST_AI_GATEWAY_API_KEY`, then `VERCEL_OIDC_TOKEN` are read at
    /// model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl VercelAiGatewayOpenAICompatibleSettings {
    /// Creates empty Vercel AI Gateway OpenAI-compatible settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the OpenAI-compatible Gateway base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the AI Gateway API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Vercel AI Gateway provider using the OpenAI-compatible API.
#[derive(Clone)]
pub struct VercelAiGatewayOpenAICompatibleProvider {
    settings: VercelAiGatewayOpenAICompatibleSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl VercelAiGatewayOpenAICompatibleProvider {
    /// Creates a Vercel AI Gateway OpenAI-compatible provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(VercelAiGatewayOpenAICompatibleSettings::new())
    }

    /// Creates a provider from explicit Vercel AI Gateway settings.
    pub fn from_settings(settings: VercelAiGatewayOpenAICompatibleSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the AI Gateway API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the OpenAI-compatible Gateway base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates a Gateway OpenAI-compatible chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().language_model(model_id)
    }

    /// Alias for [`VercelAiGatewayOpenAICompatibleProvider::language_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.language_model(model_id)
    }

    /// Creates a Gateway OpenAI Responses API language model.
    pub fn responses(&self, model_id: impl Into<String>) -> OpenResponsesLanguageModel {
        self.open_responses_provider().language_model(model_id)
    }

    /// Creates a Gateway OpenAI-compatible embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Alias for [`VercelAiGatewayOpenAICompatibleProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`VercelAiGatewayOpenAICompatibleProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Gateway OpenAI-compatible image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.openai_compatible_provider().image_model(model_id)
    }

    /// Alias for [`VercelAiGatewayOpenAICompatibleProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.image_model(model_id)
    }

    /// Lists models from Vercel AI Gateway's OpenAI-compatible `/models` endpoint.
    pub async fn list_models(
        &self,
    ) -> Result<OpenAICompatibleModelListResponse, HandledFetchError> {
        self.openai_compatible_provider().list_models().await
    }

    /// Retrieves one model from Vercel AI Gateway's OpenAI-compatible `/models/{model}` endpoint.
    pub async fn retrieve_model(
        &self,
        model_id: impl AsRef<str>,
    ) -> Result<OpenAICompatibleModelEntry, HandledFetchError> {
        self.openai_compatible_provider()
            .retrieve_model(model_id)
            .await
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_PROVIDER_NAME,
            self.settings
                .base_url
                .as_deref()
                .unwrap_or(VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL),
        )
        .with_supports_json_object_response_format(false);

        if let Some(api_key) = vercel_ai_gateway_auth_token(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }

    fn open_responses_provider(&self) -> OpenResponsesProvider {
        let mut settings = OpenResponsesProviderSettings::new(
            VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_PROVIDER_NAME,
            format!(
                "{}/responses",
                self.settings
                    .base_url
                    .as_deref()
                    .unwrap_or(VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL)
                    .trim_end_matches('/')
            ),
        );

        if let Some(api_key) = vercel_ai_gateway_auth_token(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenResponsesProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for VercelAiGatewayOpenAICompatibleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for VercelAiGatewayOpenAICompatibleProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(VercelAiGatewayOpenAICompatibleProvider::language_model(
            self, model_id,
        ))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(VercelAiGatewayOpenAICompatibleProvider::embedding_model(
            self, model_id,
        ))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(VercelAiGatewayOpenAICompatibleProvider::image_model(
            self, model_id,
        ))
    }
}

/// Creates a Vercel AI Gateway OpenAI-compatible provider with explicit settings.
pub fn create_vercel_ai_gateway_openai_compatible(
    settings: VercelAiGatewayOpenAICompatibleSettings,
) -> VercelAiGatewayOpenAICompatibleProvider {
    VercelAiGatewayOpenAICompatibleProvider::from_settings(settings)
}

/// Creates a Vercel AI Gateway OpenAI-compatible language model.
pub fn vercel_ai_gateway_openai_compatible(
    model_id: impl Into<String>,
) -> OpenAICompatibleChatLanguageModel {
    VercelAiGatewayOpenAICompatibleProvider::new().language_model(model_id)
}

/// Creates a Vercel AI Gateway OpenAI Responses API language model.
pub fn vercel_ai_gateway_openai_responses(
    model_id: impl Into<String>,
) -> OpenResponsesLanguageModel {
    VercelAiGatewayOpenAICompatibleProvider::new().responses(model_id)
}

/// Creates a Vercel AI Gateway OpenAI-compatible embedding model.
pub fn vercel_ai_gateway_openai_compatible_embedding(
    model_id: impl Into<String>,
) -> OpenAICompatibleEmbeddingModel {
    VercelAiGatewayOpenAICompatibleProvider::new().embedding_model(model_id)
}

/// Creates a Vercel AI Gateway OpenAI-compatible image model.
pub fn vercel_ai_gateway_openai_compatible_image(
    model_id: impl Into<String>,
) -> OpenAICompatibleImageModel {
    VercelAiGatewayOpenAICompatibleProvider::new().image_model(model_id)
}

fn vercel_ai_gateway_auth_token(explicit_api_key: Option<&String>) -> Option<String> {
    vercel_ai_gateway_auth_token_with_env(explicit_api_key, |name| env::var(name).ok())
}

#[doc(hidden)]
pub fn vercel_ai_gateway_auth_token_with_env(
    explicit_api_key: Option<&String>,
    mut load_env: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(load_env("AI_GATEWAY_API_KEY")))
        .or_else(|| non_empty_optional_setting(load_env("AI_SDK_RUST_AI_GATEWAY_API_KEY")))
        .or_else(|| non_empty_optional_setting(load_env("VERCEL_OIDC_TOKEN")))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL, VercelAiGatewayOpenAICompatibleProvider,
        vercel_ai_gateway_auth_token_with_env, vercel_ai_gateway_openai_compatible,
        vercel_ai_gateway_openai_compatible_embedding, vercel_ai_gateway_openai_compatible_image,
        vercel_ai_gateway_openai_responses,
    };
    use ai_sdk_provider::Provider;

    #[test]
    fn vercel_ai_gateway_openai_compatible_factory_uses_default_base_url() {
        let model = vercel_ai_gateway_openai_compatible("openai/gpt-4.1-mini");
        let responses = vercel_ai_gateway_openai_responses("openai/gpt-4.1-mini");
        let embedding =
            vercel_ai_gateway_openai_compatible_embedding("openai/text-embedding-3-small");
        let image = vercel_ai_gateway_openai_compatible_image("google/imagen-4.0-generate-001");

        assert_eq!(model.provider(), "vercel-ai-gateway.chat");
        assert_eq!(model.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(responses.provider(), "vercel-ai-gateway.responses");
        assert_eq!(responses.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(embedding.provider(), "vercel-ai-gateway.embedding");
        assert_eq!(embedding.model_id(), "openai/text-embedding-3-small");
        assert_eq!(image.provider(), "vercel-ai-gateway.image");
        assert_eq!(image.model_id(), "google/imagen-4.0-generate-001");
        assert_eq!(
            VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL,
            "https://ai-gateway.vercel.sh/v1"
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_implements_provider_trait() {
        let provider = VercelAiGatewayOpenAICompatibleProvider::new();
        let language = Provider::language_model(&provider, "openai/gpt-4.1-mini")
            .expect("language models are supported");
        let embedding = Provider::embedding_model(&provider, "openai/text-embedding-3-small")
            .expect("embedding models are supported");
        let image = Provider::image_model(&provider, "google/imagen-4.0-generate-001")
            .expect("image models are supported");

        assert_eq!(language.provider(), "vercel-ai-gateway.chat");
        assert_eq!(language.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(embedding.provider(), "vercel-ai-gateway.embedding");
        assert_eq!(embedding.model_id(), "openai/text-embedding-3-small");
        assert_eq!(image.provider(), "vercel-ai-gateway.image");
        assert_eq!(image.model_id(), "google/imagen-4.0-generate-001");
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_auth_token_matches_gateway_precedence() {
        let explicit = "explicit-api-key".to_string();
        let token = vercel_ai_gateway_auth_token_with_env(
            Some(&explicit),
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", "env-api-key"),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("explicit token resolves");
        assert_eq!(token, "explicit-api-key");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", "env-api-key"),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("gateway api key resolves before compatibility env and OIDC");
        assert_eq!(token, "env-api-key");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("compatibility api key resolves before OIDC");
        assert_eq!(token, "rust-env-api-key");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[("VERCEL_OIDC_TOKEN", "oidc-token")]),
        )
        .expect("OIDC token resolves when API keys are absent");
        assert_eq!(token, "oidc-token");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", ""),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", ""),
                ("VERCEL_OIDC_TOKEN", ""),
            ]),
        );
        assert_eq!(token, None);
    }

    fn env_lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
        }
    }
}
