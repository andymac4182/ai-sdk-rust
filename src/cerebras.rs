use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelGenerateResult, LanguageModelResponseFormat, LanguageModelStreamPart,
    LanguageModelStreamResult, LanguageModelSupportedUrls,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{ModelType, NoSuchModelError, Provider, SpecificationVersion};
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
    pub fn language_model(&self, model_id: impl Into<String>) -> CerebrasChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Cerebras chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> CerebrasChatLanguageModel {
        CerebrasChatLanguageModel {
            inner: self.openai_compatible_provider().chat_model(model_id),
        }
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
                .with_user_agent_suffix(format!("ai-sdk/cerebras/{}", crate::VERSION))
                .with_transform_request_body(transform_cerebras_request_body);

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
    type LanguageModel = CerebrasChatLanguageModel;
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
pub fn cerebras(model_id: impl Into<String>) -> CerebrasChatLanguageModel {
    CerebrasProvider::new().language_model(model_id)
}

/// Cerebras chat language model.
///
/// Wraps the shared OpenAI-compatible chat model and applies the Cerebras-specific
/// response normalization: when a structured-output (`json`) request returns valid
/// text alongside a repeated `tool_calls` finish reason, the repeated tool call is
/// dropped and the unified finish reason is downgraded to `stop`. Mirrors upstream
/// `CerebrasChatLanguageModel` in `@ai-sdk/cerebras`.
#[derive(Clone)]
pub struct CerebrasChatLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

impl CerebrasChatLanguageModel {
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

/// Returns whether a structured-output (`json`) request finished with a raw
/// `tool_calls` reason while also producing non-empty text content. In that case
/// Cerebras returned valid structured output text plus a repeated tool call, and
/// the response should be treated as a normal `stop`.
fn is_structured_output_with_tool_calls_finish_reason(
    raw_finish_reason: Option<&str>,
    has_text: bool,
    response_format: Option<&LanguageModelResponseFormat>,
) -> bool {
    matches!(
        response_format,
        Some(LanguageModelResponseFormat::Json { .. })
    ) && raw_finish_reason == Some("tool_calls")
        && has_text
}

fn generate_result_has_text(result: &LanguageModelGenerateResult) -> bool {
    result.content.iter().any(|part| match part {
        LanguageModelContent::Text(text) => !text.text.is_empty(),
        _ => false,
    })
}

impl LanguageModel for CerebrasChatLanguageModel {
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
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        self.inner.provider()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let response_format = options.response_format.clone();
        let inner = self.inner.do_generate(options);
        Box::pin(async move {
            let mut result = inner.await;

            if !is_structured_output_with_tool_calls_finish_reason(
                result.finish_reason.raw.as_deref(),
                generate_result_has_text(&result),
                response_format.as_ref(),
            ) {
                return result;
            }

            // Cerebras GLM can return valid structured output text while also
            // repeating a tool call. Treat that mixed response as the final answer.
            result
                .content
                .retain(|part| !matches!(part, LanguageModelContent::ToolCall(_)));
            result.finish_reason.unified = FinishReason::Stop;
            result
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        let response_format = options.response_format.clone();
        let inner = self.inner.do_stream(options);
        Box::pin(async move {
            let mut result = inner.await;
            let mut has_text = false;
            let mut normalized = Vec::with_capacity(result.stream.len());

            for part in result.stream.into_iter() {
                if let LanguageModelStreamPart::TextDelta(delta) = &part {
                    if !delta.delta.is_empty() {
                        has_text = true;
                    }
                }

                if let LanguageModelStreamPart::Finish(finish) = &part {
                    if is_structured_output_with_tool_calls_finish_reason(
                        finish.finish_reason.raw.as_deref(),
                        has_text,
                        response_format.as_ref(),
                    ) {
                        let mut finish = finish.clone();
                        finish.finish_reason.unified = FinishReason::Stop;
                        normalized.push(LanguageModelStreamPart::Finish(finish));
                        continue;
                    }
                }

                if matches!(
                    response_format,
                    Some(LanguageModelResponseFormat::Json { .. })
                ) && has_text
                    && matches!(
                        part,
                        LanguageModelStreamPart::ToolInputStart(_)
                            | LanguageModelStreamPart::ToolInputDelta(_)
                            | LanguageModelStreamPart::ToolInputEnd(_)
                            | LanguageModelStreamPart::ToolCall(_)
                    )
                {
                    continue;
                }

                normalized.push(part);
            }

            result.stream = normalized;
            result
        })
    }
}

/// Cerebras expects assistant reasoning history in the `reasoning` field, while
/// the shared OpenAI-compatible converter serializes it as `reasoning_content`.
/// Mirrors upstream `transformCerebrasRequestBody` in `@ai-sdk/cerebras`.
fn transform_cerebras_request_body(mut request_body: JsonValue) -> JsonValue {
    let Some(messages) = request_body
        .get_mut("messages")
        .and_then(JsonValue::as_array_mut)
    else {
        return request_body;
    };

    for message in messages.iter_mut() {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        if object.get("role").and_then(JsonValue::as_str) != Some("assistant") {
            continue;
        }
        let Some(reasoning_content) = object.remove("reasoning_content") else {
            continue;
        };
        if !object.contains_key("reasoning") && !reasoning_content.is_null() {
            object.insert("reasoning".to_string(), reasoning_content);
        }
    }

    request_body
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
        create_cerebras, transform_cerebras_request_body,
    };
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelMessage, LanguageModelResponseFormat, LanguageModelStreamPart,
        LanguageModelTextPart, LanguageModelUserContentPart, LanguageModelUserMessage,
    };
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

    fn json_transport(body: JsonValue) -> OpenAICompatibleTransport {
        Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                body.to_string(),
            ))))
        })
    }

    fn sse_transport(events: Vec<JsonValue>) -> OpenAICompatibleTransport {
        let mut payload = String::new();
        for event in events {
            payload.push_str("data: ");
            payload.push_str(&event.to_string());
            payload.push_str("\n\n");
        }
        payload.push_str("data: [DONE]\n\n");
        Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
            let response = ProviderApiResponse::text(200, "OK", payload.clone()).with_headers(
                Headers::from([("content-type".to_string(), "text/event-stream".to_string())]),
            );
            Box::pin(ready(Ok(response)))
        })
    }

    fn user_prompt() -> Vec<LanguageModelMessage> {
        vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Hello"),
            )],
        ))]
    }

    #[test]
    fn cerebras_preserves_the_captured_first_tool_call_step() {
        // packages-cerebras-0001: structured-output `json` request, first step.
        // Reasoning + a single tool call with raw `tool_calls`, no structured text:
        // the model leaves content untouched and keeps the `tool-calls` finish reason.
        let model = create_cerebras(CerebrasProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(json_transport(json!({
                "id": "chatcmpl-cerebras",
                "model": "zai-glm-4.7",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "",
                            "reasoning_content": "Let me call the tool to get the magic number.",
                            "tool_calls": [
                                {
                                    "id": "85e4fd267",
                                    "type": "function",
                                    "function": {
                                        "name": "nonUsefulTool",
                                        "arguments": "{}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ],
                "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}
            })))
            .chat("zai-glm-4.7");

        let options = LanguageModelCallOptions::new(user_prompt())
            .with_response_format(LanguageModelResponseFormat::json());
        let result = poll_ready(model.do_generate(options));

        assert_eq!(result.finish_reason.raw.as_deref(), Some("tool_calls"));
        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        // The tool call is preserved because there is no structured output text.
        assert!(
            result
                .content
                .iter()
                .any(|part| matches!(part, LanguageModelContent::ToolCall(call) if call.tool_name == "nonUsefulTool"))
        );
        assert!(
            result
                .content
                .iter()
                .any(|part| matches!(part, LanguageModelContent::Reasoning(_)))
        );
    }

    #[test]
    fn cerebras_drops_the_captured_repeated_tool_call_when_structured_output_text_is_present() {
        // packages-cerebras-0002: structured-output `json` request with valid text plus a
        // repeated tool call and raw `tool_calls`. The repeated tool call is dropped and the
        // unified finish reason is downgraded to `stop`.
        let model = create_cerebras(CerebrasProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(json_transport(json!({
                "id": "chatcmpl-cerebras",
                "model": "zai-glm-4.7",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "{\"result\":\"2026\"}",
                            "reasoning_content": "The function returned 2026 as the magic number.",
                            "tool_calls": [
                                {
                                    "id": "0babb4517",
                                    "type": "function",
                                    "function": {
                                        "name": "nonUsefulTool",
                                        "arguments": "{}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ],
                "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}
            })))
            .chat("zai-glm-4.7");

        let options = LanguageModelCallOptions::new(user_prompt())
            .with_response_format(LanguageModelResponseFormat::json());
        let result = poll_ready(model.do_generate(options));

        assert_eq!(result.finish_reason.raw.as_deref(), Some("tool_calls"));
        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        assert!(
            !result
                .content
                .iter()
                .any(|part| matches!(part, LanguageModelContent::ToolCall(_))),
            "structured output text drops the repeated tool call"
        );
        assert!(
            result
                .content
                .iter()
                .any(|part| matches!(part, LanguageModelContent::Text(text) if text.text == "{\"result\":\"2026\"}"))
        );
    }

    #[test]
    fn cerebras_preserves_the_captured_mixed_response_without_structured_output() {
        // packages-cerebras-0003: identical payload but WITHOUT a `json` response format.
        // Normalization does not apply, so the tool call is kept and the finish reason stays
        // `tool-calls`.
        let model = create_cerebras(CerebrasProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(json_transport(json!({
                "id": "chatcmpl-cerebras",
                "model": "zai-glm-4.7",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "{\"result\":\"2026\"}",
                            "reasoning_content": "The function returned 2026 as the magic number.",
                            "tool_calls": [
                                {
                                    "id": "0babb4517",
                                    "type": "function",
                                    "function": {
                                        "name": "nonUsefulTool",
                                        "arguments": "{}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ],
                "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}
            })))
            .chat("zai-glm-4.7");

        // No response format set: a normal (non-structured) request.
        let options = LanguageModelCallOptions::new(user_prompt());
        let result = poll_ready(model.do_generate(options));

        assert_eq!(result.finish_reason.raw.as_deref(), Some("tool_calls"));
        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert!(
            result
                .content
                .iter()
                .any(|part| matches!(part, LanguageModelContent::ToolCall(call) if call.tool_name == "nonUsefulTool")),
            "mixed response without structured output keeps the tool call"
        );
    }

    #[test]
    fn cerebras_normalizes_captured_streamed_structured_output_with_tool_calls_finish_reason() {
        // packages-cerebras-0004: streamed structured-output `json` request that produces text
        // and finishes with raw `tool_calls`. The final finish part is normalized to `stop`.
        let model = create_cerebras(CerebrasProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(sse_transport(vec![
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": null}]
                }),
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [{"index": 0, "delta": {"content": "{\"result\":\"2026\"}"}, "finish_reason": null}]
                }),
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [
                        {
                            "index": 0,
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "bbd2b9d98",
                                        "type": "function",
                                        "function": {"name": "nonUsefulTool", "arguments": "{}"}
                                    }
                                ]
                            },
                            "finish_reason": null
                        }
                    ]
                }),
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
                    "usage": {"prompt_tokens": 433, "completion_tokens": 122, "total_tokens": 555}
                }),
            ]))
            .chat("zai-glm-4.7");

        let options = LanguageModelCallOptions::new(user_prompt())
            .with_response_format(LanguageModelResponseFormat::json());
        let result = poll_ready(model.do_stream(options));

        let finish = result
            .stream
            .iter()
            .rev()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream emits a finish part");
        assert_eq!(finish.finish_reason.raw.as_deref(), Some("tool_calls"));
        assert_eq!(finish.finish_reason.unified, FinishReason::Stop);
    }

    #[test]
    fn cerebras_preserves_the_first_streamed_tool_call_and_drops_the_repeated_one() {
        // packages-cerebras-0005: once structured-output text is streamed, subsequent tool-call
        // parts are dropped from the stream.
        let model = create_cerebras(CerebrasProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(sse_transport(vec![
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "content": "{\"result\":\"2026\"}"}, "finish_reason": null}]
                }),
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [
                        {
                            "index": 0,
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "bbd2b9d98",
                                        "type": "function",
                                        "function": {"name": "nonUsefulTool", "arguments": "{}"}
                                    }
                                ]
                            },
                            "finish_reason": null
                        }
                    ]
                }),
                json!({
                    "id": "chatcmpl-cerebras",
                    "model": "zai-glm-4.7",
                    "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
                    "usage": {"prompt_tokens": 433, "completion_tokens": 122, "total_tokens": 555}
                }),
            ]))
            .chat("zai-glm-4.7");

        let options = LanguageModelCallOptions::new(user_prompt())
            .with_response_format(LanguageModelResponseFormat::json());
        let result = poll_ready(model.do_stream(options));

        let tool_calls: Vec<_> = result
            .stream
            .iter()
            .filter(|part| matches!(part, LanguageModelStreamPart::ToolCall(_)))
            .collect();
        assert!(
            tool_calls.is_empty(),
            "structured output text drops streamed tool calls, got {tool_calls:?}"
        );
        // The structured output text is still streamed.
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::TextDelta(delta) if delta.delta == "{\"result\":\"2026\"}"))
        );
    }

    #[test]
    fn cerebras_converts_assistant_reasoning_content_to_reasoning() {
        // packages-cerebras-0010: the request-body transform renames assistant
        // `reasoning_content` to `reasoning` while leaving other messages untouched.
        let transformed = transform_cerebras_request_body(json!({
            "model": "model-id",
            "messages": [
                {"role": "user", "content": "what is the magic number?"},
                {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "I should call a tool.",
                    "tool_calls": [
                        {
                            "id": "tool-call-id",
                            "type": "function",
                            "function": {"name": "getNumber", "arguments": "{}"}
                        }
                    ]
                },
                {"role": "tool", "tool_call_id": "tool-call-id", "content": "2026"}
            ]
        }));

        assert_eq!(
            transformed,
            json!({
                "model": "model-id",
                "messages": [
                    {"role": "user", "content": "what is the magic number?"},
                    {
                        "role": "assistant",
                        "content": null,
                        "reasoning": "I should call a tool.",
                        "tool_calls": [
                            {
                                "id": "tool-call-id",
                                "type": "function",
                                "function": {"name": "getNumber", "arguments": "{}"}
                            }
                        ]
                    },
                    {"role": "tool", "tool_call_id": "tool-call-id", "content": "2026"}
                ]
            })
        );
    }

    #[test]
    fn cerebras_request_body_transform_keeps_existing_reasoning_field() {
        // Guards the `!('reasoning' in rest)` branch: an existing `reasoning` wins.
        let transformed = transform_cerebras_request_body(json!({
            "model": "model-id",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "reasoning": "keep me",
                    "reasoning_content": "drop me"
                }
            ]
        }));

        assert_eq!(
            transformed,
            json!({
                "model": "model-id",
                "messages": [
                    {"role": "assistant", "content": null, "reasoning": "keep me"}
                ]
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
