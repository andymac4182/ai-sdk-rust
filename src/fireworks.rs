use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{NoSuchModelError, Provider};
use crate::provider_utils::without_trailing_slash;

/// Default base URL for upstream `@ai-sdk/fireworks` API calls.
pub const DEFAULT_FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";

/// Settings for the upstream Fireworks provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireworksProviderSettings {
    /// Base URL for Fireworks API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Fireworks API key. When omitted, `FIREWORKS_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl FireworksProviderSettings {
    /// Creates empty Fireworks provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Fireworks API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Fireworks API key.
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

/// Upstream Fireworks provider foundation.
#[derive(Clone)]
pub struct FireworksProvider {
    settings: FireworksProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl FireworksProvider {
    /// Creates a Fireworks provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(FireworksProviderSettings::new())
    }

    /// Creates a provider from explicit Fireworks settings.
    pub fn from_settings(settings: FireworksProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Fireworks API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Fireworks API base URL for this provider.
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

    /// Creates a Fireworks chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a Fireworks chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Creates a Fireworks completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates a Fireworks embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`FireworksProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Fireworks image model over the shared OpenAI-compatible image route.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.openai_compatible_provider().image_model(model_id)
    }

    /// Alias for [`FireworksProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.image_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("fireworks", fireworks_base_url(&self.settings))
                .with_transform_request_body(transform_fireworks_chat_request_body)
                .with_error_to_message(fireworks_error_to_message)
                .with_user_agent_suffix(format!("ai-sdk/fireworks/{}", crate::VERSION));

        if let Some(api_key) = fireworks_api_key(self.settings.api_key.as_ref()) {
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

impl Default for FireworksProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for FireworksProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(FireworksProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(FireworksProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(FireworksProvider::image_model(self, model_id))
    }
}

/// Creates a Fireworks provider with explicit settings.
pub fn create_fireworks(settings: FireworksProviderSettings) -> FireworksProvider {
    FireworksProvider::from_settings(settings)
}

/// Creates a Fireworks chat language model using default provider settings.
pub fn fireworks(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    FireworksProvider::new().language_model(model_id)
}

fn fireworks_base_url(settings: &FireworksProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_FIREWORKS_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn fireworks_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("FIREWORKS_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn fireworks_error_to_message(value: &JsonValue) -> Option<String> {
    value
        .get("error")
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn transform_fireworks_chat_request_body(value: JsonValue) -> JsonValue {
    let JsonValue::Object(mut body) = value else {
        return value;
    };

    if let Some(JsonValue::Object(thinking)) = body.remove("thinking") {
        let mut transformed = JsonObject::new();

        if let Some(kind) = thinking.get("type") {
            transformed.insert("type".to_string(), kind.clone());
        }

        if let Some(budget_tokens) = thinking.get("budgetTokens") {
            transformed.insert("budget_tokens".to_string(), budget_tokens.clone());
        }

        body.insert("thinking".to_string(), JsonValue::Object(transformed));
    }

    if let Some(reasoning_history) = body.remove("reasoningHistory") {
        body.insert("reasoning_history".to_string(), reasoning_history);
    }

    if let Some(JsonValue::String(reasoning_effort)) = body.get_mut("reasoning_effort") {
        if reasoning_effort == "minimal" {
            *reasoning_effort = "low".to_string();
        } else if reasoning_effort == "xhigh" {
            *reasoning_effort = "high".to_string();
        }
    }

    JsonValue::Object(body)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_FIREWORKS_BASE_URL, FireworksProvider, FireworksProviderSettings, create_fireworks,
        fireworks,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{Provider, ProviderOptions};
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
    fn fireworks_provider_creates_chat_model_with_transformed_provider_options() {
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
                        "id": "chatcmpl-fireworks",
                        "created": 1711115037,
                        "model": "accounts/fireworks/models/llama-v3p1-8b-instruct",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Fireworks"
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
                    "req_fireworks".to_string(),
                )])))))
            });
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "fireworks": {
                "thinking": {
                    "type": "enabled",
                    "budgetTokens": 2048
                },
                "reasoningHistory": "interleaved",
                "reasoning_effort": "xhigh"
            }
        }))
        .expect("provider options deserialize");
        let provider = create_fireworks(
            FireworksProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.fireworks.test/inference/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat_model("accounts/fireworks/models/llama-v3p1-8b-instruct");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "fireworks.chat");
        assert_eq!(result.text, "Hello from Fireworks");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.fireworks.test/inference/v1/chat/completions"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/fireworks/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "accounts/fireworks/models/llama-v3p1-8b-instruct",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "thinking": {
                    "type": "enabled",
                    "budget_tokens": 2048
                },
                "reasoning_history": "interleaved",
                "reasoning_effort": "high"
            }))
        );
    }

    #[test]
    fn fireworks_provider_creates_completion_embedding_and_image_models() {
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
                        "model": "nomic-ai/nomic-embed-text-v1.5",
                        "data": [
                            {
                                "index": 0,
                                "embedding": [0.1, 0.2]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "total_tokens": 2
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = FireworksProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.fireworks.test/inference/v1/")
            .with_transport(transport);
        let completion = provider.completion_model("accounts/fireworks/models/completion");
        let embedding = provider.embedding_model("nomic-ai/nomic-embed-text-v1.5");
        let text_embedding = provider.text_embedding_model("nomic-ai/nomic-embed-text-v1.5");
        let image = provider.image_model("accounts/fireworks/models/flux-1-dev-fp8");
        let result = poll_ready(embed_many(EmbedManyOptions::new(&embedding, ["hello"])));

        assert_eq!(completion.provider(), "fireworks.completion");
        assert_eq!(embedding.provider(), "fireworks.embedding");
        assert_eq!(text_embedding.provider(), "fireworks.embedding");
        assert_eq!(image.provider(), "fireworks.image");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.fireworks.test/inference/v1/embeddings"
        );
    }

    #[test]
    fn fireworks_provider_uses_default_base_url_and_function_alias() {
        let model = fireworks("accounts/fireworks/models/llama-v3p1-8b-instruct");

        assert_eq!(model.provider(), "fireworks.chat");
        assert_eq!(
            model.model_id(),
            "accounts/fireworks/models/llama-v3p1-8b-instruct"
        );
        assert_eq!(
            DEFAULT_FIREWORKS_BASE_URL,
            "https://api.fireworks.ai/inference/v1"
        );
    }

    #[test]
    fn fireworks_provider_implements_provider_trait() {
        let provider = FireworksProvider::new();
        let model =
            Provider::language_model(&provider, "accounts/fireworks/models/chat").expect("model");
        let embedding = Provider::embedding_model(&provider, "embed").expect("embedding");
        let image = Provider::image_model(&provider, "image").expect("image");

        assert_eq!(model.provider(), "fireworks.chat");
        assert_eq!(embedding.provider(), "fireworks.embedding");
        assert_eq!(image.provider(), "fireworks.image");
    }

    #[test]
    fn fireworks_provider_settings_serde_accepts_upstream_base_url() {
        let settings: FireworksProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.fireworks.test/inference/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "fireworks"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            FireworksProviderSettings::new()
                .with_base_url("https://api.fireworks.test/inference/v1/")
                .with_api_key("key")
                .with_header("x-provider", "fireworks")
        );
    }
}
