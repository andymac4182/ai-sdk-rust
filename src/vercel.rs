use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{ModelType, NoSuchModelError, Provider};

/// Default base URL for upstream `@ai-sdk/vercel` v0 model API calls.
pub const DEFAULT_VERCEL_BASE_URL: &str = "https://api.v0.dev/v1";

/// Settings for the upstream Vercel v0 provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelProviderSettings {
    /// Base URL for Vercel API calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Vercel API key. When omitted, `VERCEL_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl VercelProviderSettings {
    /// Creates empty Vercel provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Vercel API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Vercel API key.
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

/// Upstream Vercel v0 provider.
#[derive(Clone)]
pub struct VercelProvider {
    settings: VercelProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl VercelProvider {
    /// Creates a Vercel provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(VercelProviderSettings::new())
    }

    /// Creates a provider from explicit Vercel settings.
    pub fn from_settings(settings: VercelProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Vercel API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Vercel API base URL for this provider.
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

    /// Creates a Vercel chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().language_model(model_id)
    }

    /// Alias for [`VercelProvider::language_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.language_model(model_id)
    }

    /// Reports that Vercel does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`VercelProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Vercel does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "vercel",
            self.settings
                .base_url
                .as_deref()
                .unwrap_or(DEFAULT_VERCEL_BASE_URL),
        )
        .with_user_agent_suffix(format!("ai-sdk/vercel/{}", crate::VERSION));

        if let Some(api_key) = vercel_api_key(self.settings.api_key.as_ref()) {
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

impl Default for VercelProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for VercelProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(VercelProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        VercelProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        VercelProvider::image_model(self, model_id)
    }
}

/// Creates a Vercel provider with explicit settings.
pub fn create_vercel(settings: VercelProviderSettings) -> VercelProvider {
    VercelProvider::from_settings(settings)
}

/// Creates a Vercel language model using the default provider settings.
pub fn vercel(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    VercelProvider::new().language_model(model_id)
}

fn vercel_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("VERCEL_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_VERCEL_BASE_URL, VercelProvider, VercelProviderSettings, create_vercel};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderMetadata};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[test]
    fn vercel_provider_creates_openai_compatible_chat_model() {
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
                        "id": "chatcmpl-vercel",
                        "created": 1711115037,
                        "model": "v0-1.5-md",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Vercel"
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
                    "req_vercel".to_string(),
                )])))))
            });
        let provider = create_vercel(
            VercelProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.v0.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("v0-1.5-md");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "vercel.chat");
        assert_eq!(model.model_id(), "v0-1.5-md");
        assert_eq!(result.text, "Hello from Vercel");
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-vercel")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("vercel"),
            None
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.v0.test/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/vercel/0.1.0")),
            "Vercel user-agent suffix is included"
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| !value.contains("ai-sdk/openai-compatible/0.1.0")),
            "Vercel wrapper overrides the generic OpenAI-compatible suffix"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "v0-1.5-md",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.0
            }))
        );
    }

    #[test]
    fn vercel_provider_uses_default_base_url_and_function_alias() {
        let model = super::vercel("v0-1.0-md");

        assert_eq!(model.provider(), "vercel.chat");
        assert_eq!(model.model_id(), "v0-1.0-md");
        assert_eq!(DEFAULT_VERCEL_BASE_URL, "https://api.v0.dev/v1");
    }

    #[test]
    fn vercel_provider_reports_unsupported_model_families() {
        let provider = VercelProvider::new();

        let embedding = match provider.embedding_model("embedding-model") {
            Ok(_) => panic!("embedding models are not supported"),
            Err(error) => error,
        };
        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        assert_eq!(embedding.model_id(), "embedding-model");

        let text_embedding = match provider.text_embedding_model("embedding-model") {
            Ok(_) => panic!("text embedding models are not supported"),
            Err(error) => error,
        };
        assert_eq!(text_embedding.model_type(), ModelType::EmbeddingModel);

        let image = match provider.image_model("image-model") {
            Ok(_) => panic!("image models are not supported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.model_id(), "image-model");
    }

    #[test]
    fn vercel_provider_implements_provider_trait() {
        let provider = VercelProvider::new();
        let model =
            Provider::language_model(&provider, "v0-1.5-lg").expect("language model is supported");

        assert_eq!(model.provider(), "vercel.chat");
        assert_eq!(model.model_id(), "v0-1.5-lg");
        let embedding = match Provider::embedding_model(&provider, "embedding") {
            Ok(_) => panic!("embedding is unsupported"),
            Err(error) => error,
        };
        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        let image = match Provider::image_model(&provider, "image") {
            Ok(_) => panic!("image is unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => {
                struct NoopWake;

                impl Wake for NoopWake {
                    fn wake(self: Arc<Self>) {}
                }

                let waker = Waker::from(Arc::new(NoopWake));
                let mut context = Context::from_waker(&waker);
                loop {
                    match Pin::new(&mut future).poll(&mut context) {
                        Poll::Ready(value) => break value,
                        Poll::Pending => std::thread::yield_now(),
                    }
                }
            }
        }
    }
}
