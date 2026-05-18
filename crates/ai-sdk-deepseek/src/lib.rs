use std::env;
use std::sync::Arc;

use ai_sdk_rust::{
    Headers, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport, Provider, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

/// Default base URL for upstream `@ai-sdk/deepseek` API calls.
pub const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

/// Settings for the upstream DeepSeek provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekProviderSettings {
    /// Base URL for DeepSeek API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// DeepSeek API key. When omitted, `DEEPSEEK_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl DeepSeekProviderSettings {
    /// Creates empty DeepSeek provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the DeepSeek API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the DeepSeek API key.
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

/// Upstream DeepSeek provider foundation.
#[derive(Clone)]
pub struct DeepSeekProvider {
    settings: DeepSeekProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl DeepSeekProvider {
    /// Creates a DeepSeek provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(DeepSeekProviderSettings::new())
    }

    /// Creates a provider from explicit DeepSeek settings.
    pub fn from_settings(settings: DeepSeekProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the DeepSeek API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the DeepSeek API base URL for this provider.
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

    /// Creates a DeepSeek chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a DeepSeek chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Reports that DeepSeek does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`DeepSeekProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that DeepSeek does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("deepseek", deepseek_base_url(&self.settings))
                .with_user_agent_suffix(format!("ai-sdk/deepseek/{}", ai_sdk_rust::VERSION));

        if let Some(api_key) = deepseek_api_key(self.settings.api_key.as_ref()) {
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

impl Default for DeepSeekProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for DeepSeekProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(DeepSeekProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        DeepSeekProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        DeepSeekProvider::image_model(self, model_id)
    }
}

/// Creates a DeepSeek provider with explicit settings.
pub fn create_deepseek(settings: DeepSeekProviderSettings) -> DeepSeekProvider {
    DeepSeekProvider::from_settings(settings)
}

/// Creates a DeepSeek chat language model using default provider settings.
pub fn deep_seek(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    DeepSeekProvider::new().language_model(model_id)
}

/// Deprecated upstream spelling alias for [`deep_seek`].
pub fn deepseek(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    deep_seek(model_id)
}

fn deepseek_base_url(settings: &DeepSeekProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_DEEPSEEK_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn deepseek_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("DEEPSEEK_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DEEPSEEK_BASE_URL, DeepSeekProvider, DeepSeekProviderSettings, create_deepseek,
        deep_seek, deepseek,
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
    fn deepseek_provider_creates_chat_model_with_headers_and_base_url() {
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
                        "id": "chatcmpl-deepseek",
                        "created": 1711115037,
                        "model": "deepseek-chat",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from DeepSeek"
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
                    "req_deepseek".to_string(),
                )])))))
            });
        let provider = create_deepseek(
            DeepSeekProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.deepseek.test/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("deepseek-chat");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.5)
                .with_top_p(0.3),
        ));

        assert_eq!(model.provider(), "deepseek.chat");
        assert_eq!(model.model_id(), "deepseek-chat");
        assert_eq!(result.text, "Hello from DeepSeek");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.deepseek.test/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/deepseek/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "deepseek-chat",
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
    fn deepseek_provider_uses_default_base_url_and_function_aliases() {
        let model = deep_seek("deepseek-reasoner");
        let deprecated_model = deepseek("deepseek-chat");

        assert_eq!(model.provider(), "deepseek.chat");
        assert_eq!(model.model_id(), "deepseek-reasoner");
        assert_eq!(deprecated_model.provider(), "deepseek.chat");
        assert_eq!(deprecated_model.model_id(), "deepseek-chat");
        assert_eq!(
            super::deepseek_base_url(&DeepSeekProviderSettings::new()),
            DEFAULT_DEEPSEEK_BASE_URL
        );
    }

    #[test]
    fn deepseek_provider_reports_unsupported_model_families() {
        let provider = DeepSeekProvider::new();

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
    fn deepseek_provider_implements_provider_trait() {
        let provider = DeepSeekProvider::new();
        let model =
            Provider::language_model(&provider, "deepseek-chat").expect("language model resolves");

        assert_eq!(model.provider(), "deepseek.chat");
        assert_eq!(model.model_id(), "deepseek-chat");
    }

    #[test]
    fn deepseek_provider_settings_serde_accepts_upstream_base_url() {
        let settings: DeepSeekProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.deepseek.test/",
            "apiKey": "key",
            "headers": {
                "x-provider": "deepseek"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            DeepSeekProviderSettings::new()
                .with_base_url("https://api.deepseek.test/")
                .with_api_key("key")
                .with_header("x-provider", "deepseek")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.deepseek.test/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "deepseek"
                }
            })
        );
    }
}
