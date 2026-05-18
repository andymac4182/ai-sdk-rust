use std::env;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    Headers, InputTokenUsage, JsonObject, JsonValue, LanguageModel, LanguageModelCallOptions,
    LanguageModelGenerateResult, LanguageModelStreamPart, LanguageModelStreamResult,
    LanguageModelUsage, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport, OutputTokenUsage, Provider,
    ProviderOptions, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

/// Default base URL for upstream `@ai-sdk/moonshotai` API calls.
pub const DEFAULT_MOONSHOTAI_BASE_URL: &str = "https://api.moonshot.ai/v1";

/// Settings for the upstream MoonshotAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoonshotAIProviderSettings {
    /// Moonshot API key. When omitted, `MOONSHOT_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Base URL for MoonshotAI API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl MoonshotAIProviderSettings {
    /// Creates empty MoonshotAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the MoonshotAI API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the MoonshotAI API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Upstream MoonshotAI `thinking` provider option.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoonshotAIThinkingOptions {
    /// Moonshot thinking mode.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Thinking token budget. Upstream accepts values of at least 1024.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u64>,
}

impl MoonshotAIThinkingOptions {
    /// Creates empty thinking options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the thinking mode.
    pub fn with_type(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Sets the thinking token budget.
    pub fn with_budget_tokens(mut self, budget_tokens: u64) -> Self {
        self.budget_tokens = Some(budget_tokens);
        self
    }
}

/// Upstream MoonshotAI language model provider options.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoonshotAILanguageModelOptions {
    /// Thinking controls forwarded to MoonshotAI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<MoonshotAIThinkingOptions>,

    /// Reasoning history mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_history: Option<String>,
}

impl MoonshotAILanguageModelOptions {
    /// Creates empty MoonshotAI language model options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets thinking options.
    pub fn with_thinking(mut self, thinking: MoonshotAIThinkingOptions) -> Self {
        self.thinking = Some(thinking);
        self
    }

    /// Sets the reasoning history mode.
    pub fn with_reasoning_history(mut self, reasoning_history: impl Into<String>) -> Self {
        self.reasoning_history = Some(reasoning_history.into());
        self
    }

    /// Converts these options into provider options for a model call.
    pub fn into_provider_options(self) -> ProviderOptions {
        let mut provider_options = ProviderOptions::new();
        let value = serde_json::to_value(self).expect("MoonshotAI provider options serialize");

        if let JsonValue::Object(options) = value
            && !options.is_empty()
        {
            provider_options.insert("moonshotai".to_string(), options);
        }

        provider_options
    }
}

/// Upstream MoonshotAI provider foundation.
#[derive(Clone)]
pub struct MoonshotAIProvider {
    settings: MoonshotAIProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// MoonshotAI chat language model.
#[derive(Clone)]
pub struct MoonshotAILanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

impl MoonshotAIProvider {
    /// Creates a MoonshotAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(MoonshotAIProviderSettings::new())
    }

    /// Creates a provider from explicit MoonshotAI settings.
    pub fn from_settings(settings: MoonshotAIProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the MoonshotAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the MoonshotAI API base URL for this provider.
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

    /// Creates a MoonshotAI chat model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> MoonshotAILanguageModel {
        MoonshotAILanguageModel {
            inner: self.openai_compatible_provider().chat_model(model_id),
        }
    }

    /// Creates a MoonshotAI language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> MoonshotAILanguageModel {
        self.chat_model(model_id)
    }

    /// Reports that MoonshotAI does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`MoonshotAIProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that MoonshotAI does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "moonshotai",
            moonshotai_base_url(&self.settings),
        )
        .with_include_usage(true)
        .with_user_agent_suffix(format!("ai-sdk/moonshotai/{}", ai_sdk_rust::VERSION));

        if let Some(api_key) = moonshotai_api_key(self.settings.api_key.as_ref()) {
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

impl Default for MoonshotAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for MoonshotAIProvider {
    type LanguageModel = MoonshotAILanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(MoonshotAIProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        MoonshotAIProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        MoonshotAIProvider::image_model(self, model_id)
    }
}

impl MoonshotAILanguageModel {
    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        self.inner.provider()
    }
}

impl LanguageModel for MoonshotAILanguageModel {
    type SupportedUrlsFuture<'a>
        = <OpenAICompatibleChatLanguageModel as LanguageModel>::SupportedUrlsFuture<'a>
    where
        Self: 'a;
    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;
    type Stream = Vec<LanguageModelStreamPart>;
    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        MoonshotAILanguageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        MoonshotAILanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        self.inner.supported_urls()
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move {
            let mut result = self
                .inner
                .do_generate(transform_moonshotai_call_options(options))
                .await;
            let raw_usage = result.usage.raw.clone().map(JsonValue::Object);
            result.usage = convert_moonshotai_chat_usage(raw_usage.as_ref());
            result
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(async move {
            let mut result = self
                .inner
                .do_stream(transform_moonshotai_call_options(options))
                .await;

            for part in &mut result.stream {
                if let LanguageModelStreamPart::Finish(finish) = part {
                    let raw_usage = finish.usage.raw.clone().map(JsonValue::Object);
                    finish.usage = convert_moonshotai_chat_usage(raw_usage.as_ref());
                }
            }

            result
        })
    }
}

/// Creates a MoonshotAI provider with explicit settings.
pub fn create_moonshotai(settings: MoonshotAIProviderSettings) -> MoonshotAIProvider {
    MoonshotAIProvider::from_settings(settings)
}

/// Creates a MoonshotAI language model using default provider settings.
pub fn moonshotai(model_id: impl Into<String>) -> MoonshotAILanguageModel {
    MoonshotAIProvider::new().language_model(model_id)
}

/// Converts MoonshotAI chat usage metadata into provider-v4 usage.
pub fn convert_moonshotai_chat_usage(usage: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(usage) = usage else {
        return LanguageModelUsage::default();
    };

    if usage.is_null() {
        return LanguageModelUsage::default();
    }

    let prompt_tokens = json_u64(usage.get("prompt_tokens")).unwrap_or_default();
    let completion_tokens = json_u64(usage.get("completion_tokens")).unwrap_or_default();
    let cache_read = json_u64(usage.get("cached_tokens"))
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .and_then(|details| json_u64(details.get("cached_tokens")))
        })
        .unwrap_or_default();
    let reasoning_tokens = usage
        .get("completion_tokens_details")
        .and_then(|details| json_u64(details.get("reasoning_tokens")))
        .unwrap_or_default();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(prompt_tokens),
            no_cache: Some(prompt_tokens.saturating_sub(cache_read)),
            cache_read: Some(cache_read),
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(completion_tokens),
            text: Some(completion_tokens.saturating_sub(reasoning_tokens)),
            reasoning: Some(reasoning_tokens),
        },
        raw: usage.as_object().cloned(),
    }
}

fn moonshotai_base_url(settings: &MoonshotAIProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_MOONSHOTAI_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn moonshotai_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("MOONSHOT_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn transform_moonshotai_call_options(
    mut options: LanguageModelCallOptions,
) -> LanguageModelCallOptions {
    if let Some(provider_options) = options.provider_options.as_mut() {
        if let Some(moonshot_options) = provider_options.get_mut("moonshotai") {
            transform_moonshotai_options(moonshot_options);
        }
    }

    options
}

fn transform_moonshotai_options(options: &mut JsonObject) {
    if let Some(JsonValue::Object(thinking)) = options.remove("thinking") {
        let mut transformed = JsonObject::new();

        if let Some(kind) = thinking.get("type") {
            transformed.insert("type".to_string(), kind.clone());
        }

        if let Some(budget_tokens) = thinking.get("budgetTokens") {
            transformed.insert("budget_tokens".to_string(), budget_tokens.clone());
        }

        options.insert("thinking".to_string(), JsonValue::Object(transformed));
    }

    if let Some(reasoning_history) = options.remove("reasoningHistory") {
        options.insert("reasoning_history".to_string(), reasoning_history);
    }
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    match value? {
        JsonValue::Number(number) => number.as_u64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MOONSHOTAI_BASE_URL, MoonshotAILanguageModelOptions, MoonshotAIProvider,
        MoonshotAIProviderSettings, MoonshotAIThinkingOptions, convert_moonshotai_chat_usage,
        create_moonshotai, moonshotai,
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
    fn moonshotai_provider_creates_chat_model_with_headers_options_and_usage() {
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
                        "id": "moonshot-123",
                        "created": 1711115037,
                        "model": "kimi-k2-thinking",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from MoonshotAI"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 100,
                            "completion_tokens": 80,
                            "cached_tokens": 35,
                            "completion_tokens_details": {
                                "reasoning_tokens": 30
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_moonshot".to_string(),
                )])))))
            });
        let provider = create_moonshotai(
            MoonshotAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.moonshot.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat_model("kimi-k2-thinking");
        let provider_options = MoonshotAILanguageModelOptions::new()
            .with_thinking(
                MoonshotAIThinkingOptions::new()
                    .with_type("enabled")
                    .with_budget_tokens(2048),
            )
            .with_reasoning_history("interleaved")
            .into_provider_options();
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "moonshotai.chat");
        assert_eq!(model.model_id(), "kimi-k2-thinking");
        assert_eq!(result.text, "Hello from MoonshotAI");
        assert_eq!(result.usage.input_tokens.total, Some(100));
        assert_eq!(result.usage.input_tokens.no_cache, Some(65));
        assert_eq!(result.usage.input_tokens.cache_read, Some(35));
        assert_eq!(result.usage.output_tokens.total, Some(80));
        assert_eq!(result.usage.output_tokens.text, Some(50));
        assert_eq!(result.usage.output_tokens.reasoning, Some(30));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.moonshot.test/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/moonshotai/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "kimi-k2-thinking",
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
                "reasoning_history": "interleaved"
            }))
        );
    }

    #[test]
    fn moonshotai_provider_uses_default_base_url_and_function_alias() {
        let model = moonshotai("kimi-k2.5");

        assert_eq!(model.provider(), "moonshotai.chat");
        assert_eq!(model.model_id(), "kimi-k2.5");
        assert_eq!(
            super::moonshotai_base_url(&MoonshotAIProviderSettings::new()),
            DEFAULT_MOONSHOTAI_BASE_URL
        );
    }

    #[test]
    fn moonshotai_provider_reports_unsupported_model_families() {
        let provider = MoonshotAIProvider::new();

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
    fn moonshotai_provider_implements_provider_trait() {
        let provider = MoonshotAIProvider::new();
        let model =
            Provider::language_model(&provider, "moonshot-v1-8k").expect("language model resolves");

        assert_eq!(model.provider(), "moonshotai.chat");
        assert_eq!(model.model_id(), "moonshot-v1-8k");
    }

    #[test]
    fn moonshotai_provider_settings_serde_accepts_upstream_base_url() {
        let settings: MoonshotAIProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.moonshot.test/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "moonshotai"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            MoonshotAIProviderSettings::new()
                .with_base_url("https://api.moonshot.test/v1/")
                .with_api_key("key")
                .with_header("x-provider", "moonshotai")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.moonshot.test/v1/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "moonshotai"
                }
            })
        );
    }

    #[test]
    fn moonshotai_language_model_options_serde_match_upstream_shape() {
        let options = MoonshotAILanguageModelOptions::new()
            .with_thinking(
                MoonshotAIThinkingOptions::new()
                    .with_type("enabled")
                    .with_budget_tokens(2048),
            )
            .with_reasoning_history("preserved");

        assert_eq!(
            serde_json::to_value(options).expect("options serialize"),
            json!({
                "thinking": {
                    "type": "enabled",
                    "budgetTokens": 2048
                },
                "reasoningHistory": "preserved"
            })
        );
    }

    #[test]
    fn moonshotai_usage_conversion_handles_null_and_token_details() {
        assert_eq!(
            convert_moonshotai_chat_usage(None),
            ai_sdk_rust::LanguageModelUsage::default()
        );
        assert_eq!(
            convert_moonshotai_chat_usage(Some(&json!({
                "prompt_tokens": 100,
                "completion_tokens": 80,
                "cached_tokens": 35,
                "completion_tokens_details": {
                    "reasoning_tokens": 30
                }
            }))),
            ai_sdk_rust::LanguageModelUsage {
                input_tokens: ai_sdk_rust::InputTokenUsage {
                    total: Some(100),
                    no_cache: Some(65),
                    cache_read: Some(35),
                    cache_write: None,
                },
                output_tokens: ai_sdk_rust::OutputTokenUsage {
                    total: Some(80),
                    text: Some(50),
                    reasoning: Some(30),
                },
                raw: Some(ai_sdk_rust::JsonObject::from_iter([
                    ("cached_tokens".to_string(), json!(35)),
                    (
                        "completion_tokens_details".to_string(),
                        json!({ "reasoning_tokens": 30 }),
                    ),
                    ("completion_tokens".to_string(), json!(80)),
                    ("prompt_tokens".to_string(), json!(100)),
                ])),
            }
        );
    }
}
