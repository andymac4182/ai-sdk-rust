use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};

/// OpenAI-compatible Vercel AI Gateway base URL.
pub const VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";

const VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_PROVIDER_NAME: &str = "vercel-ai-gateway";

/// Settings for Vercel AI Gateway's OpenAI-compatible provider surface.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelAiGatewayOpenAICompatibleSettings {
    /// OpenAI-compatible Gateway base URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// AI Gateway API key. When omitted, `AI_GATEWAY_API_KEY` and then
    /// `AI_SDK_RUST_AI_GATEWAY_API_KEY` are read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl VercelAiGatewayOpenAICompatibleSettings {
    /// Creates empty Vercel AI Gateway OpenAI-compatible settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the OpenAI-compatible Gateway base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the AI Gateway API key.
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

/// Vercel AI Gateway provider using the OpenAI-compatible API.
#[derive(Clone)]
pub struct VercelAiGatewayOpenAICompatibleProvider {
    settings: VercelAiGatewayOpenAICompatibleSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl VercelAiGatewayOpenAICompatibleProvider {
    /// Creates a Vercel AI Gateway OpenAI-compatible provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(VercelAiGatewayOpenAICompatibleSettings::new())
    }

    /// Creates a provider from explicit Vercel AI Gateway settings.
    pub fn from_settings(settings: VercelAiGatewayOpenAICompatibleSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the AI Gateway API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the OpenAI-compatible Gateway base URL for this provider.
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

    /// Creates a Gateway OpenAI-compatible chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().language_model(model_id)
    }

    /// Alias for [`VercelAiGatewayOpenAICompatibleProvider::language_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.language_model(model_id)
    }

    /// Creates a Gateway OpenAI-compatible embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Alias for [`VercelAiGatewayOpenAICompatibleProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`VercelAiGatewayOpenAICompatibleProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_PROVIDER_NAME,
            self.settings
                .base_url
                .as_deref()
                .unwrap_or(VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL),
        );

        if let Some(api_key) = vercel_ai_gateway_api_key(self.settings.api_key.as_ref()) {
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

impl Default for VercelAiGatewayOpenAICompatibleProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates a Vercel AI Gateway OpenAI-compatible provider with explicit settings.
pub fn create_vercel_ai_gateway_openai_compatible(
    settings: VercelAiGatewayOpenAICompatibleSettings,
) -> VercelAiGatewayOpenAICompatibleProvider {
    VercelAiGatewayOpenAICompatibleProvider::from_settings(settings)
}

/// Creates a Vercel AI Gateway OpenAI-compatible language model.
pub fn vercel_ai_gateway_openai_compatible(
    model_id: impl Into<String>,
) -> OpenAICompatibleChatLanguageModel {
    VercelAiGatewayOpenAICompatibleProvider::new().language_model(model_id)
}

/// Creates a Vercel AI Gateway OpenAI-compatible embedding model.
pub fn vercel_ai_gateway_openai_compatible_embedding(
    model_id: impl Into<String>,
) -> OpenAICompatibleEmbeddingModel {
    VercelAiGatewayOpenAICompatibleProvider::new().embedding_model(model_id)
}

fn vercel_ai_gateway_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_env_setting("AI_GATEWAY_API_KEY"))
        .or_else(|| non_empty_env_setting("AI_SDK_RUST_AI_GATEWAY_API_KEY"))
}

fn non_empty_env_setting(name: &str) -> Option<String> {
    non_empty_optional_setting(env::var(name).ok())
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL, VercelAiGatewayOpenAICompatibleProvider,
        VercelAiGatewayOpenAICompatibleSettings, create_vercel_ai_gateway_openai_compatible,
        vercel_ai_gateway_openai_compatible, vercel_ai_gateway_openai_compatible_embedding,
    };
    use crate::embed::{EmbedManyOptions, EmbedOptions, embed, embed_many};
    use crate::embedding_model::EmbeddingModel;
    use crate::file_data::{FileData, FileDataContent};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelFilePart,
        LanguageModelMessage, LanguageModelReasoningPart, LanguageModelTextPart,
        LanguageModelToolCallPart, LanguageModelToolContentPart, LanguageModelToolMessage,
        LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::ProviderOptions;
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::stream_text::{StreamTextOptions, stream_text};
    use serde_json::json;
    use std::env;
    use std::fs;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    #[test]
    fn vercel_ai_gateway_openai_compatible_generates_text_through_openai_chat() {
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
                        "id": "chatcmpl-vercel-gateway",
                        "created": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Vercel AI Gateway"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 5,
                            "total_tokens": 9
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_vercel_ai_gateway".to_string(),
                )])))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "vercel-ai-gateway.chat");
        assert_eq!(result.text, "Hello from Vercel AI Gateway");
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(5));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-vercel-gateway")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/openai-compatible/0.1.0"))
        );

        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.0
            })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_converts_multimodal_and_tool_history() {
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
                        "id": "chatcmpl-vercel-gateway-history",
                        "created": 1711115037,
                        "model": "openai/gpt-4.1-mini",
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
                            "completion_tokens": 1,
                            "total_tokens": 11
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("openai/gpt-4.1-mini");
        let message_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "priority": "high"
            }
        }))
        .expect("message metadata deserializes");
        let image_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "alt_text": "A sample image"
            }
        }))
        .expect("image metadata deserializes");
        let assistant_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "globalPriority": "high"
            }
        }))
        .expect("assistant metadata deserializes");
        let tool_call_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "function_call_reason": "user request"
            },
            "google": {
                "thoughtSignature": "<Signature A>"
            }
        }))
        .expect("tool call metadata deserializes");
        let tool_result_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "partial": true
            }
        }))
        .expect("tool result metadata deserializes");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                        "Summarize these inputs",
                    )),
                    LanguageModelUserContentPart::File(
                        LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )
                        .with_provider_options(image_metadata),
                    ),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/image.jpg")
                                .expect("URL parses"),
                        },
                        "image/*",
                    )),
                ])
                .with_provider_options(message_metadata),
            ),
            LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                        "Checking that now...",
                    )),
                    LanguageModelAssistantContentPart::Reasoning(
                        LanguageModelReasoningPart::new("Need weather data."),
                    ),
                    LanguageModelAssistantContentPart::ToolCall(
                        LanguageModelToolCallPart::new(
                            "call_1",
                            "weather",
                            json!({ "city": "Brisbane" }),
                        )
                        .with_provider_options(tool_call_metadata),
                    ),
                ])
                .with_provider_options(assistant_metadata),
            ),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(
                    LanguageModelToolResultPart::new(
                        "call_1",
                        "weather",
                        LanguageModelToolResultOutput::json(json!({
                            "temperature": 24
                        })),
                    )
                    .with_provider_options(tool_result_metadata),
                ),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "priority": "high",
                        "content": [
                            {
                                "type": "text",
                                "text": "Summarize these inputs"
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": "data:image/png;base64,AAECAw=="
                                },
                                "alt_text": "A sample image"
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": "https://example.com/image.jpg"
                                }
                            }
                        ]
                    },
                    {
                        "role": "assistant",
                        "content": "Checking that now...",
                        "reasoning_content": "Need weather data.",
                        "globalPriority": "high",
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "weather",
                                    "arguments": "{\"city\":\"Brisbane\"}"
                                },
                                "function_call_reason": "user request",
                                "extra_content": {
                                    "google": {
                                        "thought_signature": "<Signature A>"
                                    }
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "content": "{\"temperature\":24}",
                        "tool_call_id": "call_1",
                        "partial": true
                    }
                ]
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_factory_uses_default_base_url() {
        let model = vercel_ai_gateway_openai_compatible("openai/gpt-4.1-mini");
        let embedding =
            vercel_ai_gateway_openai_compatible_embedding("openai/text-embedding-3-small");

        assert_eq!(model.provider(), "vercel-ai-gateway.chat");
        assert_eq!(model.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(embedding.provider(), "vercel-ai-gateway.embedding");
        assert_eq!(embedding.model_id(), "openai/text-embedding-3-small");
        assert_eq!(
            VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL,
            "https://ai-gateway.vercel.sh/v1"
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_streams_text_through_openai_chat() {
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
                    openai_compatible_chat_stream_body(),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    (
                        "x-request-id".to_string(),
                        "req_vercel_ai_gateway_stream".to_string(),
                    ),
                ])))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("openai/gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(12)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello stream");
        assert_eq!(result.text_stream, vec!["Hello ", "stream"]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(5));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_vercel_ai_gateway_stream")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 12,
                "temperature": 0.0,
                "stream": true
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_embeds_through_openai_embeddings() {
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
                        },
                        "providerMetadata": {
                            "vercel-ai-gateway": {
                                "traceId": "trace-vercel-ai-gateway-embedding"
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_vercel_ai_gateway_embedding".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.embedding_model("openai/text-embedding-3-small");

        assert_eq!(model.provider(), "vercel-ai-gateway.embedding");
        assert_eq!(poll_ready(model.max_embeddings_per_call()), Some(2048));

        let result = poll_ready(embed_many(
            EmbedManyOptions::new(&model, ["sunny day", "rainy city"])
                .with_header("x-call", "embed-many"),
        ));

        assert_eq!(
            result.embeddings,
            vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]]
        );
        assert_eq!(result.usage.tokens, 8);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("vercel-ai-gateway"))
                .and_then(|metadata| metadata.get("traceId"))
                .and_then(JsonValue::as_str),
            Some("trace-vercel-ai-gateway-embedding")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/embeddings");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("embed-many")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/text-embedding-3-small",
                "input": ["sunny day", "rainy city"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible model call"]
    fn live_vercel_ai_gateway_openai_compatible_generate_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-vercel-ai-gateway-openai-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0),
        ));

        assert!(
            result
                .text
                .to_lowercase()
                .contains("rust-vercel-ai-gateway-openai-ok"),
            "Gateway OpenAI-compatible response did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible stream call"]
    fn live_vercel_ai_gateway_openai_compatible_stream_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible stream test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-vercel-ai-gateway-stream-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0),
        ));

        assert!(
            result
                .text
                .to_lowercase()
                .contains("rust-vercel-ai-gateway-stream-ok"),
            "Gateway OpenAI-compatible stream response did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible embedding call"]
    fn live_vercel_ai_gateway_openai_compatible_embed() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible embedding test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_EMBEDDING_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_EMBEDDING_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_EMBEDDING_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_EMBEDDING_MODEL"))
            .unwrap_or_else(|_| "openai/text-embedding-3-small".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .embedding_model(model_id);
        let result = poll_ready(embed(EmbedOptions::new(
            &model,
            "rust vercel ai gateway embedding ok",
        )));

        assert!(
            !result.embedding.is_empty(),
            "Gateway OpenAI-compatible embedding response was empty"
        );
    }

    fn openai_compatible_chat_stream_body() -> String {
        sse_body([
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "openai/gpt-4.1-mini",
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "role": "assistant",
                            "content": ""
                        },
                        "finish_reason": null
                    }
                ]
            }),
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "openai/gpt-4.1-mini",
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "content": "Hello "
                        },
                        "finish_reason": null
                    }
                ]
            }),
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "openai/gpt-4.1-mini",
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "content": "stream"
                        },
                        "finish_reason": null
                    }
                ]
            }),
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "openai/gpt-4.1-mini",
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 4,
                    "completion_tokens": 5
                }
            }),
        ])
    }

    fn sse_body(events: impl IntoIterator<Item = JsonValue>) -> String {
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .chain(["data: [DONE]\n\n".to_string()])
            .collect()
    }

    fn live_gateway_api_key() -> Option<String> {
        env::var("AI_SDK_RUST_AI_GATEWAY_API_KEY")
            .or_else(|_| env::var("AI_GATEWAY_API_KEY"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(load_gateway_api_key_from_dotenv)
    }

    fn load_gateway_api_key_from_dotenv() -> Option<String> {
        let contents = fs::read_to_string(".env.local").ok()?;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((name, value)) = line.split_once('=') else {
                continue;
            };

            if matches!(
                name.trim(),
                "AI_SDK_RUST_AI_GATEWAY_API_KEY" | "AI_GATEWAY_API_KEY"
            ) {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }

        None
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
