use std::env;
use std::future::{Future, Ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    InputTokenUsage, LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelSupportedUrls,
    LanguageModelUsage, OutputTokenUsage,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{ModelType, NoSuchModelError, Provider, SpecificationVersion};
use crate::provider_utils::without_trailing_slash;

/// Default base URL for upstream `@ai-sdk/deepinfra` API calls.
pub const DEFAULT_DEEPINFRA_BASE_URL: &str = "https://api.deepinfra.com/v1";

/// Settings for the upstream DeepInfra provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepInfraProviderSettings {
    /// Base URL for DeepInfra API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// DeepInfra API key. When omitted, `DEEPINFRA_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl DeepInfraProviderSettings {
    /// Creates empty DeepInfra provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the DeepInfra API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the DeepInfra API key.
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

/// Upstream DeepInfra provider foundation.
#[derive(Clone)]
pub struct DeepInfraProvider {
    settings: DeepInfraProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// DeepInfra chat language model with upstream DeepInfra usage correction.
#[derive(Clone)]
pub struct DeepInfraChatLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

impl DeepInfraChatLanguageModel {
    fn new(inner: OpenAICompatibleChatLanguageModel) -> Self {
        Self { inner }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        self.inner.provider()
    }

    /// Returns whether structured outputs are enabled for this chat model.
    pub fn supports_structured_outputs(&self) -> bool {
        self.inner.supports_structured_outputs()
    }
}

impl LanguageModel for DeepInfraChatLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
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

    fn specification_version(&self) -> SpecificationVersion {
        self.inner.specification_version()
    }

    fn provider(&self) -> &str {
        self.inner.provider()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        self.inner.supported_urls()
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move {
            let mut result = self.inner.do_generate(options).await;
            result.usage = correct_deepinfra_usage(result.usage);
            result
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(async move {
            let mut result = self.inner.do_stream(options).await;
            for part in &mut result.stream {
                if let LanguageModelStreamPart::Finish(finish) = part {
                    finish.usage = correct_deepinfra_usage(finish.usage.clone());
                }
            }
            result
        })
    }
}

impl DeepInfraProvider {
    /// Creates a DeepInfra provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(DeepInfraProviderSettings::new())
    }

    /// Creates a provider from explicit DeepInfra settings.
    pub fn from_settings(settings: DeepInfraProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the DeepInfra API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the DeepInfra API base URL for this provider.
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

    /// Creates a DeepInfra chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a DeepInfra chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
        DeepInfraChatLanguageModel::new(self.openai_compatible_provider().chat_model(model_id))
    }

    /// Alias for [`DeepInfraProvider::chat_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a DeepInfra completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates a DeepInfra embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Alias for [`DeepInfraProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`DeepInfraProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Reports that DeepInfra's custom image model is not ported in this foundation slice.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    /// Alias for [`DeepInfraProvider::image_model`].
    pub fn image(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        self.image_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "deepinfra",
            format!("{}/openai", deepinfra_base_url(&self.settings)),
        )
        .with_user_agent_suffix(format!("ai-sdk/deepinfra/{}", crate::VERSION));

        if let Some(api_key) = deepinfra_api_key(self.settings.api_key.as_ref()) {
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

impl Default for DeepInfraProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for DeepInfraProvider {
    type LanguageModel = DeepInfraChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(DeepInfraProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(DeepInfraProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        DeepInfraProvider::image_model(self, model_id)
    }
}

/// Creates a DeepInfra provider with explicit settings.
pub fn create_deepinfra(settings: DeepInfraProviderSettings) -> DeepInfraProvider {
    DeepInfraProvider::from_settings(settings)
}

/// Creates a DeepInfra chat language model using the default provider settings.
pub fn deepinfra(model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
    DeepInfraProvider::new().language_model(model_id)
}

fn deepinfra_base_url(settings: &DeepInfraProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_DEEPINFRA_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn deepinfra_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("DEEPINFRA_API_KEY").ok()))
}

fn correct_deepinfra_usage(usage: LanguageModelUsage) -> LanguageModelUsage {
    let Some(mut raw) = usage.raw.clone() else {
        return usage;
    };
    let Some(reasoning_tokens) = deepinfra_reasoning_tokens(&raw) else {
        return usage;
    };
    let completion_tokens = json_u64(raw.get("completion_tokens")).unwrap_or_default();

    if reasoning_tokens <= completion_tokens {
        return usage;
    }

    let corrected_completion_tokens = completion_tokens.saturating_add(reasoning_tokens);
    raw.insert(
        "completion_tokens".to_string(),
        JsonValue::from(corrected_completion_tokens),
    );

    if let Some(total_tokens) = json_u64(raw.get("total_tokens")) {
        raw.insert(
            "total_tokens".to_string(),
            JsonValue::from(total_tokens.saturating_add(reasoning_tokens)),
        );
    }

    deepinfra_usage_from_raw(raw)
}

fn deepinfra_usage_from_raw(raw: JsonObject) -> LanguageModelUsage {
    let input_total = json_u64(
        raw.get("prompt_tokens")
            .or_else(|| raw.get("promptTokens"))
            .or_else(|| raw.get("input_tokens"))
            .or_else(|| raw.get("inputTokens")),
    );
    let output_total = json_u64(
        raw.get("completion_tokens")
            .or_else(|| raw.get("completionTokens"))
            .or_else(|| raw.get("output_tokens"))
            .or_else(|| raw.get("outputTokens")),
    );
    let cache_read = json_u64(raw.get("prompt_tokens_details").and_then(|details| {
        details
            .get("cached_tokens")
            .or_else(|| details.get("cachedTokens"))
    }));
    let reasoning_tokens = deepinfra_reasoning_tokens(&raw);

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_total,
            no_cache: input_total
                .zip(cache_read)
                .map(|(total, cached)| total.saturating_sub(cached)),
            cache_read,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: output_total,
            text: output_total
                .map(|total| total.saturating_sub(reasoning_tokens.unwrap_or_default())),
            reasoning: reasoning_tokens,
        },
        raw: Some(raw),
    }
}

fn deepinfra_reasoning_tokens(raw: &JsonObject) -> Option<u64> {
    json_u64(
        raw.get("completion_tokens_details")
            .and_then(|details| {
                details
                    .get("reasoning_tokens")
                    .or_else(|| details.get("reasoningTokens"))
            })
            .or_else(|| raw.get("reasoning_tokens"))
            .or_else(|| raw.get("reasoningTokens")),
    )
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    value.and_then(JsonValue::as_u64)
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DEEPINFRA_BASE_URL, DeepInfraProvider, DeepInfraProviderSettings, create_deepinfra,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{
        LanguageModel, LanguageModelCallOptions, LanguageModelMessage, LanguageModelStreamPart,
        LanguageModelSystemMessage,
    };
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
    fn deepinfra_provider_creates_chat_model_with_headers_and_base_url() {
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
                        "id": "chatcmpl-deepinfra",
                        "created": 1711115037,
                        "model": "meta-llama/Llama-3.3-70B-Instruct",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from DeepInfra"
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
                    "req_deepinfra".to_string(),
                )])))))
            });
        let provider = create_deepinfra(
            DeepInfraProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.deepinfra.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("meta-llama/Llama-3.3-70B-Instruct");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "deepinfra.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(result.text, "Hello from DeepInfra");
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-deepinfra")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("deepinfra"),
            None
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/openai/chat/completions"
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
                .is_some_and(|value| value.contains("ai-sdk/deepinfra/0.1.0")),
            "DeepInfra user-agent suffix is included"
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| !value.contains("ai-sdk/openai-compatible/0.1.0")),
            "DeepInfra wrapper overrides the generic OpenAI-compatible suffix"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "meta-llama/Llama-3.3-70B-Instruct",
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
    fn deepinfra_chat_corrects_reasoning_usage_when_reasoning_exceeds_completion_tokens() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-deepinfra-reasoning",
                        "created": 1711115037,
                        "model": "google/gemma-2-9b-it",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Done"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 10,
                            "prompt_tokens_details": {
                                "cached_tokens": 3
                            },
                            "completion_tokens": 4,
                            "completion_tokens_details": {
                                "reasoning_tokens": 10
                            },
                            "total_tokens": 14
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = DeepInfraProvider::new()
            .with_transport(transport)
            .chat_model("google/gemma-2-9b-it");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Think carefully")),
        ])));

        assert_eq!(result.usage.input_tokens.total, Some(10));
        assert_eq!(result.usage.input_tokens.no_cache, Some(7));
        assert_eq!(result.usage.input_tokens.cache_read, Some(3));
        assert_eq!(result.usage.output_tokens.total, Some(14));
        assert_eq!(result.usage.output_tokens.text, Some(4));
        assert_eq!(result.usage.output_tokens.reasoning, Some(10));
        assert_eq!(
            result
                .usage
                .raw
                .as_ref()
                .and_then(|raw| raw.get("completion_tokens"))
                .and_then(JsonValue::as_u64),
            Some(14)
        );
        assert_eq!(
            result
                .usage
                .raw
                .as_ref()
                .and_then(|raw| raw.get("total_tokens"))
                .and_then(JsonValue::as_u64),
            Some(24)
        );
    }

    #[test]
    fn deepinfra_chat_corrects_stream_finish_reasoning_usage() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-deepinfra-stream",
                            "created": 1711115037,
                            "model": "google/gemma-2-9b-it",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "content": "Done"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-deepinfra-stream",
                            "created": 1711115037,
                            "model": "google/gemma-2-9b-it",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 10,
                                "completion_tokens": 4,
                                "completion_tokens_details": {
                                    "reasoning_tokens": 10
                                },
                                "total_tokens": 14
                            }
                        }),
                    ]),
                ))))
            });
        let model = DeepInfraProvider::new()
            .with_transport(transport)
            .chat_model("google/gemma-2-9b-it");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.usage.output_tokens.total == Some(14)
                    && finish.usage.output_tokens.text == Some(4)
                    && finish.usage.output_tokens.reasoning == Some(10)
                    && finish
                        .usage
                        .raw
                        .as_ref()
                        .and_then(|raw| raw.get("total_tokens"))
                        .and_then(JsonValue::as_u64)
                        == Some(24)
        ));
    }

    #[test]
    fn deepinfra_provider_creates_completion_model() {
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
                        "id": "cmpl-deepinfra",
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
        let model = DeepInfraProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.deepinfra.test/v1/")
            .with_transport(transport)
            .completion_model("completion-model");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Complete this"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "deepinfra.completion");
        assert_eq!(result.text, " completed");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/openai/completions"
        );
    }

    #[test]
    fn deepinfra_provider_creates_embedding_model_aliases() {
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
        let provider = DeepInfraProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.deepinfra.test/v1/")
            .with_transport(transport);
        let model = provider.embedding_model("BAAI/bge-large-en-v1.5");
        let result = poll_ready(embed_many(EmbedManyOptions::new(
            &model,
            ["sunny day", "rainy city"],
        )));

        assert_eq!(model.provider(), "deepinfra.embedding");
        assert_eq!(
            provider.embedding("BAAI/bge-large-en-v1.5").provider(),
            "deepinfra.embedding"
        );
        assert_eq!(
            provider
                .text_embedding_model("BAAI/bge-large-en-v1.5")
                .provider(),
            "deepinfra.embedding"
        );
        assert_eq!(result.embeddings.len(), 2);
        assert_eq!(result.embeddings[0], vec![0.1, 0.2, 0.3]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/openai/embeddings"
        );
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
    fn deepinfra_provider_uses_default_base_url_and_function_alias() {
        let model = super::deepinfra("meta-llama/Llama-3.3-70B-Instruct");

        assert_eq!(model.provider(), "deepinfra.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(DEFAULT_DEEPINFRA_BASE_URL, "https://api.deepinfra.com/v1");
    }

    #[test]
    fn deepinfra_provider_reports_unported_image_model() {
        let provider = DeepInfraProvider::new();

        let image = match provider.image_model("black-forest-labs/FLUX-1-schnell") {
            Ok(_) => panic!("image models are not implemented in this slice"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.model_id(), "black-forest-labs/FLUX-1-schnell");

        let image_alias = match provider.image("black-forest-labs/FLUX-1-schnell") {
            Ok(_) => panic!("image models are not implemented in this slice"),
            Err(error) => error,
        };
        assert_eq!(image_alias.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn deepinfra_provider_implements_provider_trait() {
        let provider = DeepInfraProvider::new();
        let model = Provider::language_model(&provider, "meta-llama/Llama-3.3-70B-Instruct")
            .expect("language model is supported");

        assert_eq!(model.provider(), "deepinfra.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct");
        let embedding = Provider::embedding_model(&provider, "BAAI/bge-large-en-v1.5")
            .expect("embedding model is supported");
        assert_eq!(embedding.provider(), "deepinfra.embedding");
        let image = match Provider::image_model(&provider, "image") {
            Ok(_) => panic!("image is unsupported in this slice"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn deepinfra_provider_settings_serde_accepts_upstream_base_url() {
        let settings: DeepInfraProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.deepinfra.test/v1",
            "apiKey": "test-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            DeepInfraProviderSettings::new()
                .with_base_url("https://api.deepinfra.test/v1")
                .with_api_key("test-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.deepinfra.test/v1",
                "apiKey": "test-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
    }

    fn sse_body(events: impl IntoIterator<Item = JsonValue>) -> String {
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .chain(["data: [DONE]\n\n".to_string()])
            .collect()
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
