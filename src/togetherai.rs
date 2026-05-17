use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{ModelType, NoSuchModelError, Provider};
use crate::provider_utils::without_trailing_slash;

/// Default base URL for upstream `@ai-sdk/togetherai` API calls.
pub const DEFAULT_TOGETHERAI_BASE_URL: &str = "https://api.together.xyz/v1";

/// Settings for the upstream TogetherAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherAIProviderSettings {
    /// Base URL for TogetherAI API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// TogetherAI API key. When omitted, `TOGETHER_API_KEY` and then
    /// deprecated `TOGETHER_AI_API_KEY` are read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl TogetherAIProviderSettings {
    /// Creates empty TogetherAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the TogetherAI API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the TogetherAI API key.
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

/// Upstream TogetherAI provider foundation.
#[derive(Clone)]
pub struct TogetherAIProvider {
    settings: TogetherAIProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl TogetherAIProvider {
    /// Creates a TogetherAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(TogetherAIProviderSettings::new())
    }

    /// Creates a provider from explicit TogetherAI settings.
    pub fn from_settings(settings: TogetherAIProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the TogetherAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the TogetherAI API base URL for this provider.
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

    /// Creates a TogetherAI chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a TogetherAI chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Alias for [`TogetherAIProvider::chat_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a TogetherAI completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates a TogetherAI embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Alias for [`TogetherAIProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`TogetherAIProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Reports that TogetherAI image models are not part of this foundation slice.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    /// Alias for [`TogetherAIProvider::image_model`].
    pub fn image(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        self.image_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "togetherai",
            togetherai_base_url(&self.settings),
        )
        .with_user_agent_suffix(format!("ai-sdk/togetherai/{}", crate::VERSION));

        if let Some(api_key) = togetherai_api_key(self.settings.api_key.as_ref()) {
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

impl Default for TogetherAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for TogetherAIProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(TogetherAIProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(TogetherAIProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        TogetherAIProvider::image_model(self, model_id)
    }
}

/// Creates a TogetherAI provider with explicit settings.
pub fn create_togetherai(settings: TogetherAIProviderSettings) -> TogetherAIProvider {
    TogetherAIProvider::from_settings(settings)
}

/// Creates a TogetherAI chat language model using the default provider settings.
pub fn togetherai(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    TogetherAIProvider::new().language_model(model_id)
}

fn togetherai_base_url(settings: &TogetherAIProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_TOGETHERAI_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn togetherai_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    togetherai_api_key_from(explicit_api_key, |name| env::var(name).ok())
}

fn togetherai_api_key_from(
    explicit_api_key: Option<&String>,
    load_env: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(load_env("TOGETHER_API_KEY")))
        .or_else(|| non_empty_optional_setting(load_env("TOGETHER_AI_API_KEY")))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_TOGETHERAI_BASE_URL, TogetherAIProvider, TogetherAIProviderSettings,
        create_togetherai, togetherai_api_key_from,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider};
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
    fn togetherai_provider_creates_chat_model_with_headers_base_url_and_body() {
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
                        "id": "chatcmpl-togetherai",
                        "created": 1711115037,
                        "model": "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from TogetherAI"
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
                    "req_togetherai".to_string(),
                )])))))
            });
        let provider = create_togetherai(
            TogetherAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.together.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("meta-llama/Llama-3.3-70B-Instruct-Turbo");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "togetherai.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        assert_eq!(result.text, "Hello from TogetherAI");
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-togetherai")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.together.test/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/togetherai/0.1.0")),
            "TogetherAI user-agent suffix is included"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "meta-llama/Llama-3.3-70B-Instruct-Turbo",
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
    fn togetherai_provider_creates_completion_model() {
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
                        "id": "cmpl-togetherai",
                        "created": 1711115037,
                        "model": "completion-model",
                        "choices": [
                            {
                                "index": 0,
                                "text": " completed",
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1,
                            "total_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.together.test/v1/")
            .with_transport(transport)
            .completion_model("completion-model");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Complete this"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "togetherai.completion");
        assert_eq!(result.text, " completed");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.together.test/v1/completions");
    }

    #[test]
    fn togetherai_provider_creates_embedding_model_aliases() {
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
                        "model": "BAAI/bge-large-en-v1.5",
                        "data": [
                            {
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3]
                            },
                            {
                                "index": 1,
                                "embedding": [0.4, 0.5, 0.6]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "total_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.together.test/v1/")
            .with_transport(transport);
        let model = provider.embedding_model("BAAI/bge-large-en-v1.5");
        let result = poll_ready(embed_many(EmbedManyOptions::new(
            &model,
            ["sunny day", "rainy city"],
        )));

        assert_eq!(model.provider(), "togetherai.embedding");
        assert_eq!(
            provider.embedding("BAAI/bge-large-en-v1.5").provider(),
            "togetherai.embedding"
        );
        assert_eq!(
            provider
                .text_embedding_model("BAAI/bge-large-en-v1.5")
                .provider(),
            "togetherai.embedding"
        );
        assert_eq!(result.embeddings.len(), 2);
        assert_eq!(result.embeddings[0], vec![0.1, 0.2, 0.3]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.together.test/v1/embeddings");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "BAAI/bge-large-en-v1.5",
                "input": ["sunny day", "rainy city"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    fn togetherai_provider_uses_default_base_url_and_function_alias() {
        let model = super::togetherai("meta-llama/Llama-3.3-70B-Instruct-Turbo");

        assert_eq!(model.provider(), "togetherai.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        assert_eq!(DEFAULT_TOGETHERAI_BASE_URL, "https://api.together.xyz/v1");
    }

    #[test]
    fn togetherai_provider_reports_unported_image_model() {
        let provider = TogetherAIProvider::new();

        let image = match provider.image_model("stabilityai/stable-diffusion-xl") {
            Ok(_) => panic!("image models are not supported in this foundation slice"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.model_id(), "stabilityai/stable-diffusion-xl");

        let alias = match provider.image("stabilityai/stable-diffusion-xl") {
            Ok(_) => panic!("image alias is not supported in this foundation slice"),
            Err(error) => error,
        };
        assert_eq!(alias.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn togetherai_api_key_prefers_explicit_then_new_env_then_deprecated_env() {
        let explicit = "explicit-key".to_string();

        assert_eq!(
            togetherai_api_key_from(Some(&explicit), |_| Some("env-key".to_string())),
            Some("explicit-key".to_string())
        );
        assert_eq!(
            togetherai_api_key_from(None, |name| match name {
                "TOGETHER_API_KEY" => Some("new-env-key".to_string()),
                "TOGETHER_AI_API_KEY" => Some("deprecated-env-key".to_string()),
                _ => None,
            }),
            Some("new-env-key".to_string())
        );
        assert_eq!(
            togetherai_api_key_from(None, |name| match name {
                "TOGETHER_AI_API_KEY" => Some("deprecated-env-key".to_string()),
                _ => None,
            }),
            Some("deprecated-env-key".to_string())
        );
        assert_eq!(
            togetherai_api_key_from(Some(&String::new()), |name| match name {
                "TOGETHER_API_KEY" => Some(String::new()),
                "TOGETHER_AI_API_KEY" => Some("deprecated-env-key".to_string()),
                _ => None,
            }),
            Some("deprecated-env-key".to_string())
        );
    }

    #[test]
    fn togetherai_provider_implements_provider_trait() {
        let provider = TogetherAIProvider::new();
        let model = Provider::language_model(&provider, "meta-llama/Llama-3.3-70B-Instruct-Turbo")
            .expect("language model is supported");

        assert_eq!(model.provider(), "togetherai.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        let embedding = Provider::embedding_model(&provider, "BAAI/bge-large-en-v1.5")
            .expect("embedding model is supported");
        assert_eq!(embedding.provider(), "togetherai.embedding");
        let image = match Provider::image_model(&provider, "image-model") {
            Ok(_) => panic!("image is unsupported in this foundation slice"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.model_id(), "image-model");
    }

    #[test]
    fn togetherai_provider_settings_serde_accepts_upstream_base_url() {
        let settings: TogetherAIProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.together.test/v1",
            "apiKey": "test-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            TogetherAIProviderSettings::new()
                .with_base_url("https://api.together.test/v1")
                .with_api_key("test-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.together.test/v1",
                "apiKey": "test-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
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
