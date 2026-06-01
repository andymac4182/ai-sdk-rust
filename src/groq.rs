use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{ModelType, NoSuchModelError, Provider};
use crate::provider_utils::without_trailing_slash;

/// Default base URL for upstream `@ai-sdk/groq` API calls.
pub const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Settings for the upstream Groq provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroqProviderSettings {
    /// Base URL for Groq API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Groq API key. When omitted, `GROQ_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl GroqProviderSettings {
    /// Creates empty Groq provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Groq API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Groq API key.
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

/// Upstream Groq provider foundation.
#[derive(Clone)]
pub struct GroqProvider {
    settings: GroqProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl GroqProvider {
    /// Creates a Groq provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(GroqProviderSettings::new())
    }

    /// Creates a provider from explicit Groq settings.
    pub fn from_settings(settings: GroqProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Groq API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Groq API base URL for this provider.
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

    /// Creates a Groq chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Groq chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Reports that Groq does not expose embedding models through this Rust slice.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`GroqProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Groq does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("groq", groq_base_url(&self.settings))
                .with_supports_structured_outputs(true)
                .with_user_agent_suffix(format!("ai-sdk/groq/{}", crate::VERSION));

        if let Some(api_key) = groq_api_key(self.settings.api_key.as_ref()) {
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

impl Default for GroqProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GroqProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(GroqProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        GroqProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        GroqProvider::image_model(self, model_id)
    }
}

/// Creates a Groq provider with explicit settings.
pub fn create_groq(settings: GroqProviderSettings) -> GroqProvider {
    GroqProvider::from_settings(settings)
}

/// Creates a Groq chat language model using default provider settings.
pub fn groq(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    GroqProvider::new().language_model(model_id)
}

fn groq_base_url(settings: &GroqProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_GROQ_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn groq_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("GROQ_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_GROQ_BASE_URL, GroqProvider, GroqProviderSettings, create_groq, groq};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderOptions};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use serde_json::json;
    use std::future::{Future, ready};
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
    fn groq_provider_creates_chat_model_with_headers_base_url_and_provider_options() {
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
                        "id": "chatcmpl-groq",
                        "created": 1711115037,
                        "model": "llama-3.3-70b-versatile",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Groq"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 4,
                            "total_tokens": 8
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_groq".to_string(),
                )])))))
            });
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "groq": {
                "serviceTier": "flex"
            }
        }))
        .expect("provider options deserialize");
        let provider = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("llama-3.3-70b-versatile");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "groq.chat");
        assert_eq!(result.text, "Hello from Groq");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.groq.test/openai/v1/chat/completions"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/groq/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "llama-3.3-70b-versatile",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "service_tier": "flex"
            }))
        );
    }

    #[test]
    fn groq_provider_uses_default_base_url_and_function_alias() {
        let model = groq("llama-3.1-8b-instant");

        assert_eq!(model.provider(), "groq.chat");
        assert_eq!(model.model_id(), "llama-3.1-8b-instant");
        assert_eq!(DEFAULT_GROQ_BASE_URL, "https://api.groq.com/openai/v1");
    }

    #[test]
    fn groq_provider_reports_unsupported_model_families() {
        let provider = GroqProvider::new();

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn groq_provider_implements_provider_trait() {
        let provider = GroqProvider::new();
        let model =
            Provider::language_model(&provider, "llama-3.3-70b-versatile").expect("model resolves");

        assert_eq!(model.provider(), "groq.chat");
        assert_eq!(model.model_id(), "llama-3.3-70b-versatile");
    }

    #[test]
    fn groq_provider_settings_serde_accepts_upstream_base_url() {
        let settings: GroqProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.groq.test/openai/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "groq"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            GroqProviderSettings::new()
                .with_base_url("https://api.groq.test/openai/v1/")
                .with_api_key("key")
                .with_header("x-provider", "groq")
        );
    }
}
