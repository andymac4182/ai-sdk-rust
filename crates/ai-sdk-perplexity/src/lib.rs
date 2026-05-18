use std::env;
use std::sync::Arc;

use ai_sdk_rust::{
    Headers, LanguageModel, LanguageModelCallOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
    Provider, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

/// Default base URL for upstream `@ai-sdk/perplexity` API calls.
pub const DEFAULT_PERPLEXITY_BASE_URL: &str = "https://api.perplexity.ai";

/// Settings for the upstream Perplexity provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerplexityProviderSettings {
    /// Base URL for Perplexity API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Perplexity API key. When omitted, `PERPLEXITY_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl PerplexityProviderSettings {
    /// Creates empty Perplexity provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Perplexity API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Perplexity API key.
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

/// Upstream Perplexity provider foundation.
#[derive(Clone)]
pub struct PerplexityProvider {
    settings: PerplexityProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// Perplexity chat language model.
#[derive(Clone)]
pub struct PerplexityLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

impl PerplexityProvider {
    /// Creates a Perplexity provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(PerplexityProviderSettings::new())
    }

    /// Creates a provider from explicit Perplexity settings.
    pub fn from_settings(settings: PerplexityProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Perplexity API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Perplexity API base URL for this provider.
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

    /// Creates a Perplexity language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> PerplexityLanguageModel {
        PerplexityLanguageModel {
            inner: self.openai_compatible_provider().chat_model(model_id),
        }
    }

    /// Reports that Perplexity does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`PerplexityProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Perplexity does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "perplexity",
            perplexity_base_url(&self.settings),
        )
        .with_user_agent_suffix(format!("ai-sdk/perplexity/{}", ai_sdk_rust::VERSION));

        if let Some(api_key) = perplexity_api_key(self.settings.api_key.as_ref()) {
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
}

impl Default for PerplexityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for PerplexityProvider {
    type LanguageModel = PerplexityLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(PerplexityProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        PerplexityProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        PerplexityProvider::image_model(self, model_id)
    }
}

impl PerplexityLanguageModel {
    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "perplexity"
    }
}

impl LanguageModel for PerplexityLanguageModel {
    type SupportedUrlsFuture<'a>
        = <OpenAICompatibleChatLanguageModel as LanguageModel>::SupportedUrlsFuture<'a>
    where
        Self: 'a;
    type GenerateFuture<'a>
        = <OpenAICompatibleChatLanguageModel as LanguageModel>::GenerateFuture<'a>
    where
        Self: 'a;
    type Stream = <OpenAICompatibleChatLanguageModel as LanguageModel>::Stream;
    type StreamFuture<'a>
        = <OpenAICompatibleChatLanguageModel as LanguageModel>::StreamFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        PerplexityLanguageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        PerplexityLanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        self.inner.supported_urls()
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        self.inner.do_generate(options)
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        self.inner.do_stream(options)
    }
}

/// Creates a Perplexity provider with explicit settings.
pub fn create_perplexity(settings: PerplexityProviderSettings) -> PerplexityProvider {
    PerplexityProvider::from_settings(settings)
}

/// Creates a Perplexity language model using default provider settings.
pub fn perplexity(model_id: impl Into<String>) -> PerplexityLanguageModel {
    PerplexityProvider::new().language_model(model_id)
}

fn perplexity_base_url(settings: &PerplexityProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_PERPLEXITY_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn perplexity_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("PERPLEXITY_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_PERPLEXITY_BASE_URL, PerplexityProvider, PerplexityProviderSettings,
        create_perplexity, perplexity,
    };
    use ai_sdk_rust::{
        GenerateTextOptions, Headers, JsonValue, ModelType, OpenAICompatibleTransport,
        OpenAICompatibleTransportFuture, Prompt, Provider, ProviderApiRequest,
        ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse, generate_text,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn test_waker() -> Waker {
        Waker::from(Arc::new(NoopWake))
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    #[test]
    fn perplexity_provider_creates_language_model_with_headers_and_base_url() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "pplx-123",
                        "created": 1711115037,
                        "model": "sonar",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Perplexity"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 3,
                            "total_tokens": 7
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_perplexity".to_string(),
                )])))))
            });
        let provider = create_perplexity(
            PerplexityProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.perplexity.test/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("sonar");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.5)
                .with_top_p(0.3),
        ));

        assert_eq!(model.provider(), "perplexity");
        assert_eq!(model.model_id(), "sonar");
        assert_eq!(result.text, "Hello from Perplexity");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.perplexity.test/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/perplexity/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.5,
                "top_p": 0.3
            }))
        );
    }

    #[test]
    fn perplexity_provider_uses_default_base_url_and_function_alias() {
        let model = perplexity("sonar");

        assert_eq!(model.provider(), "perplexity");
        assert_eq!(model.model_id(), "sonar");
        assert_eq!(
            super::perplexity_base_url(&PerplexityProviderSettings::new()),
            DEFAULT_PERPLEXITY_BASE_URL
        );
    }

    #[test]
    fn perplexity_provider_reports_unsupported_model_families() {
        let provider = PerplexityProvider::new();

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);

        let text_embedding_error = provider
            .text_embedding_model("embed")
            .err()
            .expect("text embedding alias is unsupported");
        assert_eq!(text_embedding_error.model_type(), ModelType::EmbeddingModel);

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn perplexity_provider_implements_provider_trait() {
        let provider = PerplexityProvider::new();
        let model = Provider::language_model(&provider, "sonar").expect("language model resolves");

        assert_eq!(model.provider(), "perplexity");
        assert_eq!(model.model_id(), "sonar");
    }

    #[test]
    fn perplexity_provider_settings_serde_accepts_upstream_base_url() {
        let settings: PerplexityProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.perplexity.test/",
            "apiKey": "key",
            "headers": {
                "x-provider": "perplexity"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            PerplexityProviderSettings::new()
                .with_base_url("https://api.perplexity.test/")
                .with_api_key("key")
                .with_header("x-provider", "perplexity")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.perplexity.test/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "perplexity"
                }
            })
        );
    }
}
