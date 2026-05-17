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

/// Default base URL for upstream `@ai-sdk/baseten` Model API calls.
pub const DEFAULT_BASETEN_BASE_URL: &str = "https://inference.baseten.co/v1";

/// Settings for the upstream Baseten provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BasetenProviderSettings {
    /// Baseten API base URL for Model API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Dedicated model URL for Baseten custom chat or embedding endpoints.
    #[serde(
        default,
        rename = "modelURL",
        alias = "modelUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_url: Option<String>,

    /// Baseten API key. When omitted, `BASETEN_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl BasetenProviderSettings {
    /// Creates empty Baseten provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Baseten Model API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the dedicated Baseten model URL.
    pub fn with_model_url(mut self, model_url: impl Into<String>) -> Self {
        self.model_url = Some(model_url.into());
        self
    }

    /// Sets the Baseten API key.
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

/// Upstream Baseten provider foundation.
#[derive(Clone)]
pub struct BasetenProvider {
    settings: BasetenProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl BasetenProvider {
    /// Creates a Baseten provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(BasetenProviderSettings::new())
    }

    /// Creates a provider from explicit Baseten settings.
    pub fn from_settings(settings: BasetenProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Baseten API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Baseten Model API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Sets the dedicated Baseten model URL for this provider.
    pub fn with_model_url(mut self, model_url: impl Into<String>) -> Self {
        self.settings.model_url = Some(model_url.into());
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

    /// Creates a Baseten chat language model.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        self.chat_model(model_id)
    }

    /// Creates a Baseten chat language model.
    pub fn chat_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        let model_id = model_id.into();
        let base_url = baseten_chat_base_url(&self.settings).map_err(|message| {
            NoSuchModelError::with_message(model_id.clone(), ModelType::LanguageModel, message)
        })?;

        Ok(self
            .openai_compatible_provider(base_url)
            .chat_model(model_id))
    }

    /// Creates a Baseten embedding model.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        let model_id = model_id.into();
        let base_url = baseten_embedding_base_url(&self.settings).map_err(|message| {
            NoSuchModelError::with_message(model_id.clone(), ModelType::EmbeddingModel, message)
        })?;

        Ok(self
            .openai_compatible_provider(base_url)
            .embedding_model(model_id))
    }

    /// Deprecated upstream alias for [`BasetenProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Baseten does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self, base_url: String) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new("baseten", base_url)
            .with_user_agent_suffix(format!("ai-sdk/baseten/{}", crate::VERSION));

        if let Some(api_key) = baseten_api_key(self.settings.api_key.as_ref()) {
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

impl Default for BasetenProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for BasetenProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        BasetenProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        BasetenProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        BasetenProvider::image_model(self, model_id)
    }
}

/// Creates a Baseten provider with explicit settings.
pub fn create_baseten(settings: BasetenProviderSettings) -> BasetenProvider {
    BasetenProvider::from_settings(settings)
}

/// Creates a Baseten chat language model using default provider settings.
pub fn baseten(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    BasetenProvider::new()
        .language_model(model_id)
        .expect("default Baseten chat model configuration is valid")
}

fn baseten_chat_base_url(settings: &BasetenProviderSettings) -> Result<String, String> {
    if let Some(model_url) = baseten_model_url(settings) {
        if model_url.contains("/sync/v1") {
            return Ok(model_url);
        }

        if model_url.contains("/predict") {
            return Err(
                "Not supported. You must use a /sync/v1 endpoint for chat models.".to_string(),
            );
        }
    }

    Ok(baseten_base_url(settings))
}

fn baseten_embedding_base_url(settings: &BasetenProviderSettings) -> Result<String, String> {
    let model_url = baseten_model_url(settings).ok_or_else(|| {
        "No model URL provided for embeddings. Please set modelURL option for embeddings."
            .to_string()
    })?;

    if !model_url.contains("/sync") {
        return Err(
            "Not supported. You must use a /sync or /sync/v1 endpoint for embeddings.".to_string(),
        );
    }

    if model_url.contains("/sync/v1") {
        Ok(model_url)
    } else {
        Ok(format!("{model_url}/v1"))
    }
}

fn baseten_base_url(settings: &BasetenProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_BASETEN_BASE_URL.to_string());

    strip_one_trailing_slash(base_url)
}

fn baseten_model_url(settings: &BasetenProviderSettings) -> Option<String> {
    non_empty_optional_setting(settings.model_url.clone()).map(strip_one_trailing_slash)
}

fn strip_one_trailing_slash(value: String) -> String {
    without_trailing_slash(Some(value.as_str()))
        .unwrap_or(&value)
        .to_string()
}

fn baseten_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("BASETEN_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        BasetenProvider, BasetenProviderSettings, DEFAULT_BASETEN_BASE_URL, baseten, create_baseten,
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
    fn baseten_provider_creates_default_chat_model_with_headers_and_base_url() {
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
                        "id": "chatcmpl-baseten",
                        "created": 1711115037,
                        "model": "deepseek-ai/DeepSeek-V3-0324",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Baseten"
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
                    "req_baseten".to_string(),
                )])))))
            });
        let provider = create_baseten(
            BasetenProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://inference.baseten.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider
            .chat_model("deepseek-ai/DeepSeek-V3-0324")
            .expect("chat model is supported");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "baseten.chat");
        assert_eq!(model.model_id(), "deepseek-ai/DeepSeek-V3-0324");
        assert_eq!(result.text, "Hello from Baseten");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://inference.baseten.test/v1/chat/completions"
        );
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
                .is_some_and(|value| value.contains("ai-sdk/baseten/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "deepseek-ai/DeepSeek-V3-0324",
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
    fn baseten_provider_routes_custom_sync_chat_model_url_and_rejects_predict() {
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
                        "id": "chatcmpl-baseten-custom",
                        "created": 1711115037,
                        "model": "custom-chat",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from a dedicated Baseten model"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 5,
                            "completion_tokens": 6,
                            "total_tokens": 11
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = BasetenProvider::new()
            .with_api_key("test-api-key")
            .with_model_url("https://model-123.api.baseten.co/environments/production/sync/v1/")
            .with_transport(transport);
        let model = provider
            .language_model("custom-chat")
            .expect("sync v1 chat model is supported");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "Hello from a dedicated Baseten model");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://model-123.api.baseten.co/environments/production/sync/v1/chat/completions"
        );

        let unsupported = match BasetenProvider::new()
            .with_model_url("https://model-123.api.baseten.co/environments/production/predict")
            .chat_model("custom-chat")
        {
            Ok(_) => panic!("predict chat endpoints should be rejected"),
            Err(error) => error,
        };
        assert_eq!(unsupported.model_type(), ModelType::LanguageModel);
        assert_eq!(
            unsupported.message(),
            "Not supported. You must use a /sync/v1 endpoint for chat models."
        );
    }

    #[test]
    fn baseten_provider_creates_embedding_model_for_sync_urls() {
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
                        "object": "list",
                        "data": [
                            {
                                "object": "embedding",
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3]
                            },
                            {
                                "object": "embedding",
                                "index": 1,
                                "embedding": [0.4, 0.5, 0.6]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 8,
                            "total_tokens": 8
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = BasetenProvider::new()
            .with_api_key("test-api-key")
            .with_model_url("https://model-123.api.baseten.co/environments/production/sync/")
            .with_transport(transport);
        let model = provider
            .embedding_model("embeddings")
            .expect("sync embedding model is supported");
        let result = poll_ready(embed_many(EmbedManyOptions::new(
            &model,
            ["sunny day", "rainy city"],
        )));

        assert_eq!(model.provider(), "baseten.embedding");
        assert_eq!(
            provider
                .text_embedding_model("embeddings")
                .expect("text embedding alias is supported")
                .provider(),
            "baseten.embedding"
        );
        assert_eq!(
            result.embeddings,
            vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]]
        );
        assert_eq!(result.usage.tokens, 8);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://model-123.api.baseten.co/environments/production/sync/v1/embeddings"
        );
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
                "model": "embeddings",
                "input": ["sunny day", "rainy city"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    fn baseten_provider_reports_unsupported_embedding_routes_and_images() {
        let provider = BasetenProvider::new();
        let missing_model_url = match provider.embedding_model("embeddings") {
            Ok(_) => panic!("embedding requires a model URL"),
            Err(error) => error,
        };
        assert_eq!(missing_model_url.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            missing_model_url.message(),
            "No model URL provided for embeddings. Please set modelURL option for embeddings."
        );

        let unsupported_route = match provider
            .with_model_url("https://model-123.api.baseten.co/environments/production/predict")
            .embedding_model("embeddings")
        {
            Ok(_) => panic!("predict embedding endpoints should be rejected"),
            Err(error) => error,
        };
        assert_eq!(unsupported_route.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            unsupported_route.message(),
            "Not supported. You must use a /sync or /sync/v1 endpoint for embeddings."
        );

        let image = match BasetenProvider::new().image_model("image-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.message(), "No such imageModel: image-model");
    }

    #[test]
    fn baseten_provider_uses_default_base_url_and_function_alias() {
        let model = baseten("deepseek-ai/DeepSeek-V3-0324");

        assert_eq!(model.provider(), "baseten.chat");
        assert_eq!(model.model_id(), "deepseek-ai/DeepSeek-V3-0324");
        assert_eq!(
            super::baseten_base_url(&BasetenProviderSettings::new()),
            DEFAULT_BASETEN_BASE_URL
        );
    }

    #[test]
    fn baseten_provider_implements_provider_trait() {
        let provider = BasetenProvider::new()
            .with_model_url("https://model-123.api.baseten.co/environments/production/sync/v1");
        let model =
            Provider::language_model(&provider, "custom-chat").expect("language model exists");
        let embedding =
            Provider::embedding_model(&provider, "embeddings").expect("embedding model exists");

        assert_eq!(model.provider(), "baseten.chat");
        assert_eq!(embedding.provider(), "baseten.embedding");
        assert!(Provider::image_model(&provider, "image-model").is_err());
    }

    #[test]
    fn baseten_provider_settings_serde_accepts_upstream_urls() {
        let settings: BasetenProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://inference.baseten.test/v1/",
            "modelURL": "https://model-123.api.baseten.co/environments/production/sync/v1/",
            "apiKey": "test-api-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            BasetenProviderSettings::new()
                .with_base_url("https://inference.baseten.test/v1/")
                .with_model_url("https://model-123.api.baseten.co/environments/production/sync/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://inference.baseten.test/v1/",
                "modelURL": "https://model-123.api.baseten.co/environments/production/sync/v1/",
                "apiKey": "test-api-key",
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
