use std::env;
use std::sync::Arc;

use ai_sdk_rust::{
    Headers, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport, Provider, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

/// Default base URL for upstream `@ai-sdk/mistral` API calls.
pub const DEFAULT_MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";

/// Settings for the upstream Mistral provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MistralProviderSettings {
    /// Base URL for Mistral API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Mistral API key. When omitted, `MISTRAL_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl MistralProviderSettings {
    /// Creates empty Mistral provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Mistral API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Mistral API key.
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

/// Upstream Mistral provider foundation.
#[derive(Clone)]
pub struct MistralProvider {
    settings: MistralProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl MistralProvider {
    /// Creates a Mistral provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(MistralProviderSettings::new())
    }

    /// Creates a provider from explicit Mistral settings.
    pub fn from_settings(settings: MistralProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Mistral API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Mistral API base URL for this provider.
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

    /// Creates a Mistral chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Mistral chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Creates a Mistral embedding model.
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Creates a Mistral embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding(model_id)
    }

    /// Deprecated upstream alias for [`MistralProvider::embedding`].
    pub fn text_embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding(model_id)
    }

    /// Deprecated upstream alias for [`MistralProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Reports that Mistral does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("mistral", mistral_base_url(&self.settings))
                .with_user_agent_suffix(format!("ai-sdk/mistral/{}", ai_sdk_rust::VERSION));

        if let Some(api_key) = mistral_api_key(self.settings.api_key.as_ref()) {
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

impl Default for MistralProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for MistralProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(MistralProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(MistralProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        MistralProvider::image_model(self, model_id)
    }
}

/// Creates a Mistral provider with explicit settings.
pub fn create_mistral(settings: MistralProviderSettings) -> MistralProvider {
    MistralProvider::from_settings(settings)
}

/// Creates a Mistral chat language model using default provider settings.
pub fn mistral(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    MistralProvider::new().language_model(model_id)
}

fn mistral_base_url(settings: &MistralProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_MISTRAL_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn mistral_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("MISTRAL_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MISTRAL_BASE_URL, MistralProvider, MistralProviderSettings, create_mistral,
        mistral, mistral_base_url,
    };
    use ai_sdk_rust::{
        EmbeddingModel, EmbeddingModelCallOptions, GenerateTextOptions, Headers, JsonValue,
        ModelType, OpenAICompatibleTransport, OpenAICompatibleTransportFuture, Prompt, Provider,
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        generate_text,
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
    fn mistral_provider_creates_chat_model_with_headers_and_base_url() {
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
                        "id": "chatcmpl-mistral",
                        "created": 1711115037,
                        "model": "mistral-small-latest",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Mistral"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 5
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_mistral_chat".to_string(),
                )])))))
            });
        let provider = create_mistral(
            MistralProviderSettings::new()
                .with_base_url("https://proxy.example.com/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("mistral-small-latest");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "mistral.chat");
        assert_eq!(result.text, "Hello from Mistral");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://proxy.example.com/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/mistral/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("model").cloned()),
            Some(json!("mistral-small-latest"))
        );
    }

    #[test]
    fn mistral_provider_creates_embedding_model_with_usage_and_headers() {
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
                        "id": "embed-mistral",
                        "object": "list",
                        "data": [
                            {
                                "object": "embedding",
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3]
                            }
                        ],
                        "model": "mistral-embed",
                        "usage": {
                            "prompt_tokens": 3,
                            "total_tokens": 3
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_mistral_embedding".to_string(),
                )])))))
            });
        let provider = MistralProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport);
        let model = provider.embedding("mistral-embed");
        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec![
            "sunny day".to_string(),
        ])));

        assert_eq!(model.provider(), "mistral.embedding");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2, 0.3]]);
        assert_eq!(result.usage.expect("usage is mapped").tokens, 3);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.mistral.ai/v1/embeddings");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "mistral-embed",
                "input": ["sunny day"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    fn mistral_provider_uses_default_base_url_and_function_alias() {
        let provider = MistralProvider::new();
        let model = mistral("mistral-large-latest");
        let trait_model =
            Provider::language_model(&provider, "mistral-small-latest").expect("model resolves");

        assert_eq!(
            mistral_base_url(&MistralProviderSettings::new()),
            DEFAULT_MISTRAL_BASE_URL
        );
        assert_eq!(model.provider(), "mistral.chat");
        assert_eq!(model.model_id(), "mistral-large-latest");
        assert_eq!(trait_model.provider(), "mistral.chat");
        assert_eq!(trait_model.model_id(), "mistral-small-latest");
        assert_eq!(
            provider.embedding_model("mistral-embed").provider(),
            "mistral.embedding"
        );
        assert_eq!(
            provider.text_embedding("mistral-embed").provider(),
            "mistral.embedding"
        );
    }

    #[test]
    fn mistral_provider_reports_unsupported_image_models() {
        let provider = MistralProvider::new();
        let error = provider
            .image_model("image")
            .err()
            .expect("image models are unsupported");

        assert_eq!(error.model_id(), "image");
        assert_eq!(error.model_type(), ModelType::ImageModel);
        assert_eq!(
            Provider::image_model(&provider, "image")
                .err()
                .expect("provider trait image lookup is unsupported")
                .model_type(),
            ModelType::ImageModel
        );
    }

    #[test]
    fn mistral_provider_settings_serde_accepts_upstream_base_url() {
        let settings: MistralProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://proxy.example.com/v1",
            "apiKey": "key",
            "headers": {
                "x-provider": "mistral"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            MistralProviderSettings::new()
                .with_base_url("https://proxy.example.com/v1")
                .with_api_key("key")
                .with_header("x-provider", "mistral")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://proxy.example.com/v1",
                "apiKey": "key",
                "headers": {
                    "x-provider": "mistral"
                }
            })
        );
    }
}
