use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
    OpenResponsesTransport,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{ModelType, NoSuchModelError, Provider};
use crate::provider_utils::without_trailing_slash;

/// Default base URL for upstream `@ai-sdk/xai` API calls.
pub const DEFAULT_XAI_BASE_URL: &str = "https://api.x.ai/v1";

/// Settings for the upstream xAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XaiProviderSettings {
    /// Base URL for xAI API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// xAI API key. When omitted, `XAI_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl XaiProviderSettings {
    /// Creates empty xAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the xAI API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the xAI API key.
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

/// Upstream xAI provider foundation.
#[derive(Clone)]
pub struct XaiProvider {
    settings: XaiProviderSettings,
    openai_transport: Option<OpenAICompatibleTransport>,
    responses_transport: Option<OpenResponsesTransport>,
}

impl XaiProvider {
    /// Creates an xAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(XaiProviderSettings::new())
    }

    /// Creates a provider from explicit xAI settings.
    pub fn from_settings(settings: XaiProviderSettings) -> Self {
        Self {
            settings,
            openai_transport: None,
            responses_transport: None,
        }
    }

    /// Sets the xAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the xAI API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the OpenAI-compatible HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.openai_transport = Some(transport);
        self
    }

    /// Replaces the Responses API HTTP transport. This is primarily useful for tests.
    pub fn with_responses_transport(mut self, transport: OpenResponsesTransport) -> Self {
        self.responses_transport = Some(transport);
        self
    }

    /// Creates the default xAI Responses language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenResponsesLanguageModel {
        self.responses(model_id)
    }

    /// Creates an xAI Responses language model.
    pub fn responses(&self, model_id: impl Into<String>) -> OpenResponsesLanguageModel {
        self.open_responses_provider().language_model(model_id)
    }

    /// Creates an xAI chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Alias for [`XaiProvider::chat_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Reports that xAI does not expose embedding models through this Rust slice.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`XaiProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Creates an xAI image model over the shared OpenAI-compatible image route.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.openai_compatible_provider().image_model(model_id)
    }

    /// Alias for [`XaiProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.image_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("xai", xai_base_url(&self.settings))
                .with_include_usage(true)
                .with_supports_structured_outputs(true)
                .with_user_agent_suffix(format!("ai-sdk/xai/{}", crate::VERSION));

        if let Some(api_key) = xai_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.openai_transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }

    fn open_responses_provider(&self) -> OpenResponsesProvider {
        let mut settings = OpenResponsesProviderSettings::new(
            "xai",
            format!("{}/responses", xai_base_url(&self.settings)),
        )
        .with_user_agent_suffix(format!("ai-sdk/xai/{}", crate::VERSION))
        .with_file_id_prefix("file-");

        if let Some(api_key) = xai_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenResponsesProvider::from_settings(settings);

        if let Some(transport) = &self.responses_transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for XaiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for XaiProvider {
    type LanguageModel = OpenResponsesLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(XaiProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        XaiProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(XaiProvider::image_model(self, model_id))
    }
}

/// Creates an xAI provider with explicit settings.
pub fn create_xai(settings: XaiProviderSettings) -> XaiProvider {
    XaiProvider::from_settings(settings)
}

/// Creates an xAI Responses language model using default provider settings.
pub fn xai(model_id: impl Into<String>) -> OpenResponsesLanguageModel {
    XaiProvider::new().language_model(model_id)
}

fn xai_base_url(settings: &XaiProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_XAI_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn xai_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("XAI_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_XAI_BASE_URL, XaiProvider, XaiProviderSettings, create_xai, xai};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        LanguageModelProviderTool, LanguageModelTool, LanguageModelToolChoice,
    };
    use crate::open_responses::{OpenResponsesTransport, OpenResponsesTransportFuture};
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider};
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
    fn xai_provider_creates_responses_model_with_headers_base_url_and_body() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_xai",
                        "created_at": 1711115037,
                        "model": "grok-4",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from xAI"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_xai(
            XaiProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.xai.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_responses_transport(transport);
        let model = provider.language_model("grok-4");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16),
        ));

        assert_eq!(model.provider(), "xai.responses");
        assert_eq!(result.text, "Hello from xAI");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.xai.test/v1/responses");
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
                .is_some_and(|value| value.contains("ai-sdk/xai/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "grok-4",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Say hello"
                            }
                        ]
                    }
                ],
                "max_output_tokens": 16
            }))
        );
    }

    #[test]
    fn xai_responses_model_prepares_server_tools_custom_tool_and_usage() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_xai_tools",
                        "created_at": 1711115037,
                        "model": "grok-4",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "xAI hosted tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
                            "input_tokens_details": {
                                "cached_tokens": 3
                            },
                            "output_tokens": 8,
                            "output_tokens_details": {
                                "reasoning_tokens": 2
                            }
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = XaiProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.xai.test/v1/")
            .with_responses_transport(transport);
        let model = provider.responses("grok-4");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use hosted tools"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "liveSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.custom",
                    "write_sql",
                    JsonObject::from_iter([
                        (
                            "description".to_string(),
                            JsonValue::String("Write SQL statements.".to_string()),
                        ),
                        (
                            "format".to_string(),
                            json!({
                                "type": "grammar",
                                "syntax": "lark",
                                "definition": "start: SELECT"
                            }),
                        ),
                    ]),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "liveSearch".to_string(),
                }),
        ));

        assert_eq!(result.text, "xAI hosted tools prepared");
        assert_eq!(result.usage.input_tokens.total, Some(10));
        assert_eq!(result.usage.input_tokens.no_cache, Some(7));
        assert_eq!(result.usage.input_tokens.cache_read, Some(3));
        assert_eq!(result.usage.output_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.text, Some(6));
        assert_eq!(result.usage.output_tokens.reasoning, Some(2));

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(request_body["model"], "grok-4");
        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "web_search"
                },
                {
                    "type": "custom",
                    "name": "write_sql",
                    "description": "Write SQL statements.",
                    "format": {
                        "type": "grammar",
                        "syntax": "lark",
                        "definition": "start: SELECT"
                    }
                }
            ])
        );
        assert_eq!(request_body["tool_choice"], json!({ "type": "web_search" }));
    }

    #[test]
    fn xai_provider_creates_chat_model_with_openai_compatible_transport() {
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
                        "id": "chatcmpl-xai",
                        "created": 1711115037,
                        "model": "grok-3",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from xAI chat"
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
                ))))
            });
        let provider = XaiProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.xai.test/v1/")
            .with_transport(transport);
        let model = provider.chat("grok-3");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "xai.chat");
        assert_eq!(model.model_id(), "grok-3");
        assert_eq!(result.text, "Hello from xAI chat");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.xai.test/v1/chat/completions");
    }

    #[test]
    fn xai_provider_creates_image_model_and_reports_unsupported_embeddings() {
        let provider = XaiProvider::new();
        let default_model = xai("grok-4");
        let image = provider.image("grok-2-image");
        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");

        assert_eq!(default_model.provider(), "xai.responses");
        assert_eq!(image.provider(), "xai.image");
        assert_eq!(image.model_id(), "grok-2-image");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(DEFAULT_XAI_BASE_URL, "https://api.x.ai/v1");
    }

    #[test]
    fn xai_provider_settings_serde_accepts_upstream_base_url() {
        let settings: XaiProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.xai.test/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "xai"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            XaiProviderSettings::new()
                .with_base_url("https://api.xai.test/v1/")
                .with_api_key("key")
                .with_header("x-provider", "xai")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.xai.test/v1/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "xai"
                }
            })
        );
    }

    #[test]
    fn xai_provider_implements_provider_trait() {
        let provider = XaiProvider::new();
        let model = Provider::language_model(&provider, "grok-4").expect("language model resolves");
        let image = Provider::image_model(&provider, "grok-2-image").expect("image resolves");

        assert_eq!(model.provider(), "xai.responses");
        assert_eq!(image.provider(), "xai.image");
    }
}
