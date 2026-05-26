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

/// Default base URL for upstream `@ai-sdk/cerebras` API calls.
pub const DEFAULT_CEREBRAS_BASE_URL: &str = "https://api.cerebras.ai/v1";

/// Settings for the upstream Cerebras provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CerebrasProviderSettings {
    /// Base URL for Cerebras API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Cerebras API key. When omitted, `CEREBRAS_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl CerebrasProviderSettings {
    /// Creates empty Cerebras provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Cerebras API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Cerebras API key.
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

/// Upstream Cerebras provider foundation.
#[derive(Clone)]
pub struct CerebrasProvider {
    settings: CerebrasProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl CerebrasProvider {
    /// Creates a Cerebras provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(CerebrasProviderSettings::new())
    }

    /// Creates a provider from explicit Cerebras settings.
    pub fn from_settings(settings: CerebrasProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Cerebras API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Cerebras API base URL for this provider.
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

    /// Creates a Cerebras chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Cerebras chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Reports that Cerebras does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`CerebrasProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Cerebras does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("cerebras", cerebras_base_url(&self.settings))
                .with_supports_structured_outputs(true)
                .with_user_agent_suffix(format!("ai-sdk/cerebras/{}", crate::VERSION));

        if let Some(api_key) = cerebras_api_key(self.settings.api_key.as_ref()) {
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

impl Default for CerebrasProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CerebrasProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(CerebrasProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        CerebrasProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        CerebrasProvider::image_model(self, model_id)
    }
}

/// Creates a Cerebras provider with explicit settings.
pub fn create_cerebras(settings: CerebrasProviderSettings) -> CerebrasProvider {
    CerebrasProvider::from_settings(settings)
}

/// Creates a Cerebras chat language model using default provider settings.
pub fn cerebras(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    CerebrasProvider::new().language_model(model_id)
}

fn cerebras_base_url(settings: &CerebrasProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_CEREBRAS_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn cerebras_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("CEREBRAS_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        CerebrasProvider, CerebrasProviderSettings, DEFAULT_CEREBRAS_BASE_URL, cerebras,
        create_cerebras,
    };
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::LanguageModelResponseFormat;
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
    fn cerebras_provider_creates_chat_model_with_headers_base_url_and_structured_outputs() {
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
                        "id": "chatcmpl-cerebras",
                        "created": 1711115037,
                        "model": "llama3.1-8b",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Cerebras"
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
                    "req_cerebras".to_string(),
                )])))))
            });
        let provider = create_cerebras(
            CerebrasProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.cerebras.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("llama3.1-8b");
        let mut schema = JsonObject::new();
        schema.insert("type".to_string(), JsonValue::String("object".to_string()));
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_response_format(
                    LanguageModelResponseFormat::json()
                        .with_schema(schema)
                        .with_name("answer"),
                ),
        ));

        assert_eq!(model.provider(), "cerebras.chat");
        assert_eq!(model.model_id(), "llama3.1-8b");
        assert!(model.supports_structured_outputs());
        assert_eq!(result.text, "Hello from Cerebras");
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cerebras"))
                .is_some_and(|metadata| metadata.is_empty())
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.cerebras.test/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/cerebras/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "llama3.1-8b",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.0,
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "schema": {
                            "type": "object"
                        },
                        "strict": true,
                        "name": "answer"
                    }
                }
            }))
        );
    }

    #[test]
    fn cerebras_provider_uses_default_base_url_and_function_alias() {
        let model = cerebras("llama3.1-8b");

        assert_eq!(model.provider(), "cerebras.chat");
        assert_eq!(model.model_id(), "llama3.1-8b");
        assert!(model.supports_structured_outputs());
        assert_eq!(
            super::cerebras_base_url(&CerebrasProviderSettings::new()),
            DEFAULT_CEREBRAS_BASE_URL
        );
    }

    #[test]
    fn cerebras_provider_reports_unsupported_model_families() {
        let provider = CerebrasProvider::new();
        let embedding = match provider.embedding_model("embedding-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            embedding.message(),
            "No such embeddingModel: embedding-model"
        );
        let text_embedding = match provider.text_embedding_model("embedding-model") {
            Ok(_) => panic!("text embedding models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(text_embedding.model_type(), ModelType::EmbeddingModel);
        let image = match provider.image_model("image-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.message(), "No such imageModel: image-model");
    }

    #[test]
    fn cerebras_provider_implements_provider_trait() {
        let provider = CerebrasProvider::new();
        let model =
            Provider::language_model(&provider, "llama3.1-8b").expect("language model exists");

        assert_eq!(model.provider(), "cerebras.chat");
        assert!(Provider::embedding_model(&provider, "embedding-model").is_err());
        assert!(Provider::image_model(&provider, "image-model").is_err());
    }

    #[test]
    fn cerebras_provider_settings_serde_accepts_upstream_base_url() {
        let settings: CerebrasProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.cerebras.test/v1/",
            "apiKey": "test-api-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            CerebrasProviderSettings::new()
                .with_base_url("https://api.cerebras.test/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.cerebras.test/v1/",
                "apiKey": "test-api-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
    }

    #[test]
    fn cerebras_provider_creates_a_provider_instance_with_default_options() {
        let provider = CerebrasProvider::new();

        assert_eq!(
            super::cerebras_base_url(&CerebrasProviderSettings::new()),
            DEFAULT_CEREBRAS_BASE_URL
        );
        assert_eq!(provider.specification_version().as_str(), "v4");
    }

    #[test]
    fn cerebras_provider_creates_a_provider_instance_with_custom_options() {
        let settings = CerebrasProviderSettings::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cerebras.test/v1/")
            .with_header("custom-header", "value");
        let provider = create_cerebras(settings.clone());

        assert_eq!(
            super::cerebras_base_url(&settings),
            "https://api.cerebras.test/v1"
        );
        assert_eq!(provider.chat("llama3.1-8b").provider(), "cerebras.chat");
    }

    #[test]
    fn cerebras_provider_passes_header() {
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
                        "id": "chatcmpl-cerebras",
                        "created": 1711115037,
                        "model": "llama3.1-8b",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Cerebras"
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
                ))))
            });
        let provider = create_cerebras(
            CerebrasProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.cerebras.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);

        let model = provider.chat("llama3.1-8b");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16),
        ));

        assert_eq!(result.text, "Hello from Cerebras");
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
    }

    #[test]
    fn cerebras_provider_returns_a_chat_model_when_called_as_a_function() {
        let model = cerebras("llama3.1-8b");

        assert_eq!(model.provider(), "cerebras.chat");
        assert_eq!(model.model_id(), "llama3.1-8b");
        assert!(model.supports_structured_outputs());
    }

    #[test]
    fn cerebras_provider_constructs_a_language_model_with_correct_configuration() {
        let provider = CerebrasProvider::new();
        let model =
            Provider::language_model(&provider, "llama3.1-8b").expect("language model exists");

        assert_eq!(model.provider(), "cerebras.chat");
        assert_eq!(model.model_id(), "llama3.1-8b");
        assert!(model.supports_structured_outputs());
    }

    #[test]
    fn cerebras_provider_throws_nosuchmodelerror_when_attempting_to_create_embedding_model() {
        let provider = CerebrasProvider::new();

        let embedding = match provider.embedding_model("embedding-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };

        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            embedding.message(),
            "No such embeddingModel: embedding-model"
        );
    }

    #[test]
    fn cerebras_provider_constructs_a_chat_model_with_correct_configuration() {
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
                        "id": "chatcmpl-cerebras",
                        "created": 1711115037,
                        "model": "llama3.1-8b",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Cerebras"
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
                ))))
            });
        let provider = create_cerebras(
            CerebrasProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.cerebras.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("llama3.1-8b");
        let mut schema = JsonObject::new();
        schema.insert("type".to_string(), JsonValue::String("object".to_string()));
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_response_format(
                    LanguageModelResponseFormat::json()
                        .with_schema(schema)
                        .with_name("answer"),
                ),
        ));

        assert_eq!(result.text, "Hello from Cerebras");
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cerebras"))
                .is_some_and(|metadata| metadata.is_empty())
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.cerebras.test/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/cerebras/0.1.0"))
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
