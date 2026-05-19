pub use ai_sdk_gateway::{
    VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL, VercelAiGatewayOpenAICompatibleProvider,
    VercelAiGatewayOpenAICompatibleSettings, create_vercel_ai_gateway_openai_compatible,
    vercel_ai_gateway_auth_token_with_env, vercel_ai_gateway_openai_compatible,
    vercel_ai_gateway_openai_compatible_embedding, vercel_ai_gateway_openai_compatible_image,
    vercel_ai_gateway_openai_responses,
};

#[cfg(test)]
mod tests {
    use super::{
        VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL, VercelAiGatewayOpenAICompatibleProvider,
        VercelAiGatewayOpenAICompatibleSettings, create_vercel_ai_gateway_openai_compatible,
        vercel_ai_gateway_auth_token_with_env, vercel_ai_gateway_openai_compatible,
        vercel_ai_gateway_openai_compatible_embedding, vercel_ai_gateway_openai_compatible_image,
        vercel_ai_gateway_openai_responses,
    };
    use crate::embed::{EmbedManyOptions, EmbedOptions, embed, embed_many};
    use crate::embedding_model::EmbeddingModel;
    use crate::file_data::{FileData, FileDataContent};
    use crate::gateway::{GatewayProviderOptions, GatewayProviderTimeouts};
    use crate::generate_image::{GenerateImageOptions, generate_image};
    use crate::generate_object::{GenerateObjectOptions, generate_object};
    use crate::generate_text::{GenerateTextOptions, PrepareStepResult, generate_text};
    use crate::headers::Headers;
    use crate::image_model::ImageModel;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelFileData,
        LanguageModelFilePart, LanguageModelMessage, LanguageModelProviderTool,
        LanguageModelReasoningPart, LanguageModelResponseFormat, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{Provider, ProviderOptions};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        Tool, json_schema,
    };
    use crate::stream_object::{StreamObjectOptions, stream_object};
    use crate::stream_text::{StreamTextOptions, stream_text};
    use crate::telemetry::{TelemetryOptions, create_open_telemetry_integration};
    use crate::warning::Warning;
    use ai_sdk_mcp::{McpClientConfig, MockMcpTransport, create_mcp_client};
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
    fn vercel_ai_gateway_openai_compatible_passes_gateway_provider_options_through_openai_chat() {
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
                        "id": "chatcmpl-vercel-gateway-options",
                        "created": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Gateway options accepted"
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
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1"),
        )
        .with_transport(transport);
        let model = provider.language_model("openai/gpt-4.1-mini");
        let provider_options = GatewayProviderOptions::new()
            .with_order(["vertex", "anthropic"])
            .with_models(["anthropic/claude-sonnet-4.6", "google/gemini-3-pro"])
            .with_zero_data_retention(true)
            .with_provider_timeouts(
                GatewayProviderTimeouts::new().with_byok_timeout("openai", 5000),
            )
            .into_provider_options();

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use Gateway routing"))
                .expect("prompt is valid")
                .with_provider_options(provider_options),
        ));

        assert_eq!(result.text, "Gateway options accepted");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body["providerOptions"],
            json!({
                "gateway": {
                    "order": ["vertex", "anthropic"],
                    "models": ["anthropic/claude-sonnet-4.6", "google/gemini-3-pro"],
                    "zeroDataRetention": true,
                    "providerTimeouts": {
                        "byok": {
                            "openai": 5000
                        }
                    }
                }
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
    fn vercel_ai_gateway_openai_compatible_maps_chat_image_outputs_through_generate_text() {
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
                        "id": "chatcmpl-vercel-gateway-image-output",
                        "created": 1711115037,
                        "model": "google/gemini-2.5-flash-image-preview",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Here is an image.",
                                    "images": [
                                        {
                                            "type": "image_url",
                                            "image_url": {
                                                "url": "data:image/png;base64,aW1hZ2UtZGF0YQ=="
                                            }
                                        }
                                    ]
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 8,
                            "completion_tokens": 12,
                            "total_tokens": 20
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1"),
        )
        .with_transport(transport);
        let model = provider.language_model("google/gemini-2.5-flash-image-preview");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "vercelAiGateway": {
                "modalities": ["text", "image"]
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Generate one small image and describe it."),
            )
            .expect("prompt is valid")
            .with_provider_options(provider_options),
        ));

        assert_eq!(result.text, "Here is an image.");
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].media_type(), "image/png");
        assert_eq!(result.files[0].base64(), "aW1hZ2UtZGF0YQ==");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body.get("modalities"),
            Some(&json!(["text", "image"]))
        );
        assert_eq!(
            request_body.get("model").and_then(JsonValue::as_str),
            Some("google/gemini-2.5-flash-image-preview")
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_streams_chat_image_outputs_through_stream_text() {
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
                    sse_body([
                        json!({
                            "id": "chatcmpl-vercel-gateway-stream-image-output",
                            "created": 1711115037,
                            "model": "google/gemini-2.5-flash-image-preview",
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
                            "id": "chatcmpl-vercel-gateway-stream-image-output",
                            "created": 1711115037,
                            "model": "google/gemini-2.5-flash-image-preview",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "Here is an image."
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-vercel-gateway-stream-image-output",
                            "created": 1711115037,
                            "model": "google/gemini-2.5-flash-image-preview",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "images": [
                                            {
                                                "type": "image_url",
                                                "image_url": {
                                                    "url": "data:image/png;base64,c3RyZWFtLWltYWdl"
                                                }
                                            }
                                        ]
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-vercel-gateway-stream-image-output",
                            "created": 1711115037,
                            "model": "google/gemini-2.5-flash-image-preview",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 8,
                                "completion_tokens": 12
                            }
                        }),
                    ]),
                )
                .with_headers(Headers::from([(
                    "content-type".to_string(),
                    "text/event-stream".to_string(),
                )])))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1"),
        )
        .with_transport(transport);
        let model = provider.language_model("google/gemini-2.5-flash-image-preview");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "vercelAiGateway": {
                "modalities": ["text", "image"]
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Stream one small image and describe it."),
            )
            .expect("prompt is valid")
            .with_provider_options(provider_options),
        ));

        assert_eq!(result.text, "Here is an image.");
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].media_type, "image/png");
        assert!(matches!(
            &result.files[0].data,
            LanguageModelFileData::Data { data }
                if data == &FileDataContent::Base64("c3RyZWFtLWltYWdl".to_string())
        ));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body.get("modalities"),
            Some(&json!(["text", "image"]))
        );
        assert_eq!(
            request_body.get("stream").and_then(JsonValue::as_bool),
            Some(true)
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_runs_generate_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let response = match call_number {
                    1 => json!({
                        "id": "chatcmpl-gateway-tool-loop-1",
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [
                                        {
                                            "id": "call_1",
                                            "type": "function",
                                            "function": {
                                                "name": "weather",
                                                "arguments": "{\"city\":\"Brisbane\"}"
                                            }
                                        }
                                    ]
                                },
                                "finish_reason": "tool_calls"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 6,
                            "completion_tokens": 3
                        }
                    }),
                    2 => json!({
                        "id": "chatcmpl-gateway-tool-loop-2",
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "The weather in Brisbane is sunny."
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 10,
                            "completion_tokens": 7
                        }
                    }),
                    other => panic!("unexpected request #{other}"),
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response.to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    format!("req_vercel_ai_gateway_tool_loop_{call_number}"),
                )])))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("openai/gpt-4.1-mini");
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Weather?"))
                .expect("prompt is valid")
                .with_tool(
                    Tool::new("weather", input_schema.clone())
                        .with_description("Get weather")
                        .with_execute(|input, options| async move {
                            Ok(json!({
                                "city": input["city"],
                                "forecast": "sunny",
                                "toolCallId": options.tool_call_id
                            }))
                        }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(result.text, "The weather in Brisbane is sunny.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        let request_bodies = requests
            .iter()
            .map(|request| {
                request
                    .body
                    .as_ref()
                    .and_then(ProviderApiRequestBody::as_text)
                    .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                    .expect("request body is JSON")
            })
            .collect::<Vec<_>>();
        assert_eq!(request_bodies.len(), 2);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(
            requests[0].url,
            "https://ai-gateway.test/v1/chat/completions"
        );
        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            requests[0].headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request_bodies[0],
            json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Weather?"
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "description": "Get weather",
                            "parameters": input_schema.clone()
                        }
                    }
                ]
            })
        );
        assert_eq!(
            request_bodies[1],
            json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Weather?"
                    },
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "weather",
                                    "arguments": "{\"city\":\"Brisbane\"}"
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "content": "{\"city\":\"Brisbane\",\"forecast\":\"sunny\",\"toolCallId\":\"call_1\"}",
                        "tool_call_id": "call_1"
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "description": "Get weather",
                            "parameters": input_schema.clone()
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_runs_generate_text_with_mcp_tools() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let response = match call_number {
                    1 => json!({
                        "id": "chatcmpl-gateway-mcp-tool-loop-1",
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [
                                        {
                                            "id": "call_mcp_1",
                                            "type": "function",
                                            "function": {
                                                "name": "mock-tool",
                                                "arguments": "{\"foo\":\"bar\"}"
                                            }
                                        }
                                    ]
                                },
                                "finish_reason": "tool_calls"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 8,
                            "completion_tokens": 4
                        }
                    }),
                    2 => json!({
                        "id": "chatcmpl-gateway-mcp-tool-loop-2",
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "The MCP tool returned Mock tool call result."
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 14,
                            "completion_tokens": 8
                        }
                    }),
                    other => panic!("unexpected request #{other}"),
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response.to_string(),
                ))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1"),
        )
        .with_transport(transport);
        let model = provider.language_model("openai/gpt-4.1-mini");
        let client = create_mcp_client(
            McpClientConfig::new(MockMcpTransport::new())
                .with_client_name("gateway-mcp-test-client"),
        )
        .expect("MCP client initializes");
        let mcp_tools = client.tools().expect("MCP tools build");

        let mut options =
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use the MCP mock tool."))
                .expect("prompt is valid")
                .with_active_tools(["mock-tool"])
                .with_max_steps(2);
        for tool in mcp_tools.into_values() {
            options = options.with_tool(tool);
        }
        let result = poll_ready(generate_text(options));

        assert_eq!(result.text, "The MCP tool returned Mock tool call result.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "mock-tool");
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(
            result.tool_results[0].output["content"][0]["text"],
            "Mock tool call result"
        );
        assert_eq!(
            result.tool_results[0]
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("clientName"))
                .and_then(JsonValue::as_str),
            Some("gateway-mcp-test-client")
        );
        client.close().expect("MCP client closes");

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        let request_bodies = requests
            .iter()
            .map(|request| {
                request
                    .body
                    .as_ref()
                    .and_then(ProviderApiRequestBody::as_text)
                    .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                    .expect("request body is JSON")
            })
            .collect::<Vec<_>>();
        assert_eq!(request_bodies.len(), 2);
        assert_eq!(
            request_bodies[0]["tools"]
                .as_array()
                .expect("tools are serialized")
                .len(),
            1
        );
        assert_eq!(
            request_bodies[0]["tools"][0]["function"]["name"],
            "mock-tool"
        );
        assert_eq!(
            request_bodies[0]["tools"][0]["function"]["description"],
            "A mock tool for testing"
        );
        assert!(
            request_bodies[1]["messages"]
                .as_array()
                .expect("messages are serialized")
                .iter()
                .any(|message| {
                    message["role"] == "tool"
                        && message["tool_call_id"] == "call_mcp_1"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.contains("Mock tool call result"))
                })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_generates_object_through_openai_chat() {
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
                        "id": "chatcmpl-gateway-object",
                        "created": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "{\"answer\":\"Gateway object\",\"count\":2}"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 8,
                            "completion_tokens": 6
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_vercel_ai_gateway_object".to_string(),
                )])))))
            });
        let provider = create_vercel_ai_gateway_openai_compatible(
            VercelAiGatewayOpenAICompatibleSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ai-gateway.test/v1")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("openai/gpt-4.1-mini");
        let object_schema: JsonObject = serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["answer", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Return a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(32)
            .with_temperature(0.0),
        ))
        .expect("object is generated");

        assert_eq!(result.object["answer"], "Gateway object");
        assert_eq!(result.object["count"], 2);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.total, Some(6));
        assert!(
            result.warnings.as_ref().is_some_and(|warnings| {
                warnings.iter().any(|warning| {
                    matches!(
                        warning,
                        Warning::Unsupported { feature, .. } if feature == "responseFormat"
                    )
                })
            }),
            "schema warning is surfaced when structured outputs are not enabled"
        );
        assert_eq!(
            result.response.headers.as_ref().and_then(|headers| {
                headers.get("x-request-id").map(std::string::String::as_str)
            }),
            Some("req_vercel_ai_gateway_object")
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
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(request_body["model"], "openai/gpt-4.1-mini");
        assert_eq!(request_body["max_tokens"], 32);
        assert_eq!(request_body["temperature"], 0.0);
        assert!(
            request_body.get("response_format").is_none(),
            "Gateway OpenAI-compatible requests omit unsupported response_format"
        );
        let messages = request_body["messages"]
            .as_array()
            .expect("messages are sent");
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .is_some_and(|content| content.contains("JSON schema:"))
        );
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(
            messages[1]["content"],
            "Return a JSON object with answer and count."
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_streams_object_through_openai_chat() {
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
                    sse_body([
                        json!({
                            "id": "chatcmpl-gateway-stream-object",
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
                            "id": "chatcmpl-gateway-stream-object",
                            "created": 1711115037,
                            "model": "openai/gpt-4.1-mini",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "{\"answer\":\"Gateway "
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-gateway-stream-object",
                            "created": 1711115037,
                            "model": "openai/gpt-4.1-mini",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "stream object\",\"count\":3}"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-gateway-stream-object",
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
                                "prompt_tokens": 9,
                                "completion_tokens": 7
                            }
                        }),
                    ]),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    (
                        "x-request-id".to_string(),
                        "req_vercel_ai_gateway_stream_object".to_string(),
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
        let model = provider.language_model("openai/gpt-4.1-mini");
        let object_schema: JsonObject = serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["answer", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Stream a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(40)
            .with_temperature(0.0),
        ));

        assert_eq!(
            result.text,
            "{\"answer\":\"Gateway stream object\",\"count\":3}"
        );
        assert_eq!(
            result.object,
            Some(json!({
                "answer": "Gateway stream object",
                "count": 3
            }))
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(9));
        assert_eq!(result.usage.output_tokens.total, Some(7));
        assert!(
            result.warnings.iter().any(|warning| {
                matches!(
                    warning,
                    Warning::Unsupported { feature, .. } if feature == "responseFormat"
                )
            }),
            "schema warning is surfaced when structured outputs are not enabled"
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_vercel_ai_gateway_stream_object")
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
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(request_body["model"], "openai/gpt-4.1-mini");
        assert_eq!(request_body["max_tokens"], 40);
        assert_eq!(request_body["temperature"], 0.0);
        assert_eq!(request_body["stream"], true);
        assert!(
            request_body.get("response_format").is_none(),
            "Gateway OpenAI-compatible stream requests omit unsupported response_format"
        );
        let messages = request_body["messages"]
            .as_array()
            .expect("messages are sent");
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .is_some_and(|content| content.contains("JSON schema:"))
        );
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(
            messages[1]["content"],
            "Stream a JSON object with answer and count."
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_factory_uses_default_base_url() {
        let model = vercel_ai_gateway_openai_compatible("openai/gpt-4.1-mini");
        let embedding =
            vercel_ai_gateway_openai_compatible_embedding("openai/text-embedding-3-small");
        let image = vercel_ai_gateway_openai_compatible_image("google/imagen-4.0-generate-001");

        assert_eq!(model.provider(), "vercel-ai-gateway.chat");
        assert_eq!(model.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(embedding.provider(), "vercel-ai-gateway.embedding");
        assert_eq!(embedding.model_id(), "openai/text-embedding-3-small");
        assert_eq!(image.provider(), "vercel-ai-gateway.image");
        assert_eq!(image.model_id(), "google/imagen-4.0-generate-001");
        assert_eq!(
            VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL,
            "https://ai-gateway.vercel.sh/v1"
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_implements_provider_trait() {
        let provider = VercelAiGatewayOpenAICompatibleProvider::new();
        let language = Provider::language_model(&provider, "openai/gpt-4.1-mini")
            .expect("language models are supported");
        let embedding = Provider::embedding_model(&provider, "openai/text-embedding-3-small")
            .expect("embedding models are supported");
        let image = Provider::image_model(&provider, "google/imagen-4.0-generate-001")
            .expect("image models are supported");

        assert_eq!(language.provider(), "vercel-ai-gateway.chat");
        assert_eq!(language.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(embedding.provider(), "vercel-ai-gateway.embedding");
        assert_eq!(embedding.model_id(), "openai/text-embedding-3-small");
        assert_eq!(image.provider(), "vercel-ai-gateway.image");
        assert_eq!(image.model_id(), "google/imagen-4.0-generate-001");
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_auth_token_matches_gateway_precedence() {
        let explicit = "explicit-api-key".to_string();
        let token = vercel_ai_gateway_auth_token_with_env(
            Some(&explicit),
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", "env-api-key"),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("explicit token resolves");
        assert_eq!(token, "explicit-api-key");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", "env-api-key"),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("gateway api key resolves before compatibility env and OIDC");
        assert_eq!(token, "env-api-key");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("compatibility api key resolves before OIDC");
        assert_eq!(token, "rust-env-api-key");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[("VERCEL_OIDC_TOKEN", "oidc-token")]),
        )
        .expect("OIDC token resolves when API keys are absent");
        assert_eq!(token, "oidc-token");

        let token = vercel_ai_gateway_auth_token_with_env(
            None,
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", ""),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", ""),
                ("VERCEL_OIDC_TOKEN", ""),
            ]),
        );
        assert_eq!(token, None);
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_lists_models() {
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
                                "id": "openai/gpt-4.1-mini",
                                "object": "model",
                                "created": 1711115037,
                                "released": 1710000000,
                                "owned_by": "openai",
                                "name": "GPT 4.1 mini",
                                "description": "Fast OpenAI language model",
                                "context_window": 128000,
                                "max_tokens": 32768,
                                "type": "language",
                                "tags": ["tool-use", "vision"],
                                "pricing": {
                                    "input": "0.0000004",
                                    "output": "0.0000016"
                                }
                            },
                            {
                                "id": "anthropic/claude-sonnet-4.5",
                                "object": "model",
                                "owned_by": "anthropic"
                            },
                            {
                                "id": "google/gemini-2.5-flash",
                                "object": "model",
                                "owned_by": "google"
                            },
                            {
                                "id": "xai/grok-4-fast",
                                "object": "model",
                                "owned_by": "xai"
                            },
                            {
                                "id": "cohere/embed-v4.0",
                                "object": "model",
                                "owned_by": "cohere",
                                "type": "embedding"
                            }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "gateway_models_req".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);

        let result = poll_ready(provider.list_models()).expect("model list succeeds");
        let ids = result.model_ids().collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "openai/gpt-4.1-mini",
                "anthropic/claude-sonnet-4.5",
                "google/gemini-2.5-flash",
                "xai/grok-4-fast",
                "cohere/embed-v4.0"
            ]
        );
        assert_eq!(result.data[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(result.data[0].name.as_deref(), Some("GPT 4.1 mini"));
        assert_eq!(result.data[0].released, Some(1710000000));
        assert_eq!(result.data[0].context_window, Some(128000));
        assert_eq!(result.data[0].max_tokens, Some(32768));
        assert_eq!(result.data[0].model_type.as_deref(), Some("language"));
        assert_eq!(result.data[0].tags, vec!["tool-use", "vision"]);
        assert_eq!(
            result.data[0]
                .pricing
                .as_ref()
                .and_then(|pricing| pricing.get("input"))
                .and_then(JsonValue::as_str),
            Some("0.0000004")
        );
        assert_eq!(result.data[4].model_type.as_deref(), Some("embedding"));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(request.url, "https://ai-gateway.test/v1/models");
        assert!(request.body.is_none());
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
    }

    #[test]
    fn vercel_ai_gateway_openai_compatible_retrieves_model() {
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
                        "id": "openai/gpt-4.1-mini",
                        "object": "model",
                        "created": 1711115037,
                        "owned_by": "openai",
                        "contextWindow": 128000,
                        "maxTokens": 32768,
                        "modelType": "language",
                        "tags": ["tool-use"]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "gateway_model_req".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);

        let result = poll_ready(provider.retrieve_model("openai/gpt-4.1-mini"))
            .expect("model retrieval succeeds");
        assert_eq!(result.id, "openai/gpt-4.1-mini");
        assert_eq!(result.object.as_deref(), Some("model"));
        assert_eq!(result.owned_by.as_deref(), Some("openai"));
        assert_eq!(result.context_window, Some(128000));
        assert_eq!(result.max_tokens, Some(32768));
        assert_eq!(result.model_type.as_deref(), Some("language"));
        assert_eq!(result.tags, vec!["tool-use"]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(
            request.url,
            "https://ai-gateway.test/v1/models/openai%2Fgpt-4.1-mini"
        );
        assert!(request.body.is_none());
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_generates_text() {
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
                        "id": "resp_gateway",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from Gateway Responses"
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
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "gateway_responses_req".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let alias_model = vercel_ai_gateway_openai_responses("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "vercel-ai-gateway.responses");
        assert_eq!(model.model_id(), "openai/gpt-4.1-mini");
        assert_eq!(alias_model.provider(), "vercel-ai-gateway.responses");
        assert_eq!(result.text, "Hello from Gateway Responses");
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("resp_gateway")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("vercel-ai-gateway"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_gateway")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");
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
                .is_some_and(|value| value.contains("ai-sdk/open-responses/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/gpt-4.1-mini",
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Say hello"
                            }
                        ]
                    }
                ],
                "max_output_tokens": 16,
                "temperature": 0.0
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_maps_no_schema_json_response_format() {
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
                        "id": "resp_gateway_json_object",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "{\"answer\":\"ok\"}"
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
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return JSON"),
                    )]),
                )])
                .with_response_format(LanguageModelResponseFormat::json()),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/gpt-4.1-mini",
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Return JSON"
                            }
                        ]
                    }
                ],
                "text": {
                    "format": {
                        "type": "json_object"
                    }
                }
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_maps_api_error_data_to_gateway_metadata_key() {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "input: Invalid input",
                            "type": "invalid_request_error",
                            "param": "input",
                            "code": "invalid_input"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_gateway_responses_error".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use invalid input"))
                .expect("prompt is valid"),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        let metadata = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("vercel-ai-gateway"))
            .expect("Gateway Responses error metadata is present");
        assert_eq!(
            metadata.get("errorMessage").and_then(JsonValue::as_str),
            Some("input: Invalid input")
        );
        assert_eq!(
            metadata.get("errorType").and_then(JsonValue::as_str),
            Some("invalid_request_error")
        );
        assert_eq!(
            metadata.get("errorParam").and_then(JsonValue::as_str),
            Some("input")
        );
        assert_eq!(
            metadata.get("errorCode").and_then(JsonValue::as_str),
            Some("invalid_input")
        );
        assert_eq!(
            metadata.get("statusCode").and_then(JsonValue::as_u64),
            Some(400)
        );
        assert_eq!(
            metadata.get("isRetryable").and_then(JsonValue::as_bool),
            Some(false)
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_gateway_responses_error")
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_converts_file_prompt_parts() {
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
                        "id": "resp_gateway_files",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Gateway file prompt accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
                            "output_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_messages(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                            "Summarize these inputs",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/report.pdf")
                                    .expect("url parses"),
                            },
                            "application/pdf",
                        )),
                    ]),
                )]),
            )
            .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "Gateway file prompt accepted");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/gpt-4.1-mini",
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Summarize these inputs"
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,AAECAw=="
                            },
                            {
                                "type": "input_file",
                                "file_url": "https://example.com/report.pdf"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_generates_object() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport = Arc::new(
            move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_gateway_object",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "{\"answer\":\"Gateway Responses object\",\"count\":4}"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
                            "output_tokens": 7
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_gateway_responses_object".to_string(),
                )])))))
            },
        );
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let object_schema: JsonObject = serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["answer", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Return a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema.clone()))
            .with_schema_name("gateway_answer")
            .with_schema_description("A Gateway Responses answer object.")
            .with_max_output_tokens(32)
            .with_temperature(0.0),
        ))
        .expect("object is generated");

        assert_eq!(result.object["answer"], "Gateway Responses object");
        assert_eq!(result.object["count"], 4);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(9));
        assert_eq!(result.usage.output_tokens.total, Some(7));
        assert!(result.warnings.as_ref().is_none_or(Vec::is_empty));
        assert_eq!(
            result.response.headers.as_ref().and_then(|headers| {
                headers.get("x-request-id").map(std::string::String::as_str)
            }),
            Some("req_gateway_responses_object")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(request_body["model"], "openai/gpt-4.1-mini");
        assert_eq!(request_body["max_output_tokens"], 32);
        assert_eq!(request_body["temperature"], 0.0);
        assert_eq!(
            request_body["text"]["format"],
            json!({
                "type": "json_schema",
                "name": "gateway_answer",
                "description": "A Gateway Responses answer object.",
                "schema": object_schema,
                "strict": true
            })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_passes_gateway_provider_options() {
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
                        "id": "resp_gateway_options",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Gateway Responses options accepted"
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
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let mut provider_options = GatewayProviderOptions::new()
            .with_order(["openai", "anthropic"])
            .with_models(["anthropic/claude-sonnet-4.6"])
            .with_provider_timeouts(
                GatewayProviderTimeouts::new().with_byok_timeout("anthropic", 3000),
            )
            .into_provider_options();
        provider_options.insert(
            "vercelAiGateway".to_string(),
            serde_json::from_value(json!({
                "caching": "auto"
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use Gateway routing"))
                .expect("prompt is valid")
                .with_provider_options(provider_options),
        ));

        assert_eq!(result.text, "Gateway Responses options accepted");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(request_body["caching"], "auto");
        assert_eq!(
            request_body["providerOptions"],
            json!({
                "gateway": {
                    "order": ["openai", "anthropic"],
                    "models": ["anthropic/claude-sonnet-4.6"],
                    "providerTimeouts": {
                        "byok": {
                            "anthropic": 3000
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_prepares_openai_hosted_tools() {
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
                        "id": "resp_gateway_hosted_tools",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Gateway hosted tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 6,
                            "output_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Search Gateway docs"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "gatewaySearch",
                    json_object(json!({
                        "externalWebAccess": false,
                        "searchContextSize": "medium"
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.apply_patch",
                    "apply_patch",
                    JsonObject::new(),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "gatewaySearch".to_string(),
                }),
        ));

        assert_eq!(result.text, "Gateway hosted tools prepared");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");

        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(request_body["model"], "openai/gpt-4.1-mini");
        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "web_search",
                    "external_web_access": false,
                    "search_context_size": "medium"
                },
                {
                    "type": "apply_patch"
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "web_search"
            })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_streams_text() {
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
                    openai_responses_stream_body(),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    (
                        "x-request-id".to_string(),
                        "req_vercel_ai_gateway_responses_stream".to_string(),
                    ),
                ])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Stream with Responses"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello Gateway Responses stream");
        assert_eq!(
            result.text_stream,
            vec!["Hello ", "Gateway Responses stream"]
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(result.response.id.as_deref(), Some("resp_gateway_stream"));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_vercel_ai_gateway_responses_stream")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("vercel-ai-gateway"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_gateway_stream")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");
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
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Stream with Responses"
                            }
                        ]
                    }
                ],
                "max_output_tokens": 16,
                "temperature": 0.0,
                "stream": true
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_streams_object() {
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
                    openai_responses_object_stream_body(),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    (
                        "x-request-id".to_string(),
                        "req_vercel_ai_gateway_responses_stream_object".to_string(),
                    ),
                ])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let object_schema: JsonObject = serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["answer", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Stream a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema.clone()))
            .with_schema_name("gateway_stream_answer")
            .with_schema_description("A streamed Gateway Responses answer object.")
            .with_max_output_tokens(48)
            .with_temperature(0.0),
        ));

        assert_eq!(
            result.text,
            "{\"answer\":\"Gateway Responses stream object\",\"count\":5}"
        );
        assert_eq!(
            result.object,
            Some(json!({
                "answer": "Gateway Responses stream object",
                "count": 5
            }))
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(10));
        assert_eq!(result.usage.output_tokens.total, Some(8));
        assert!(result.warnings.is_empty());
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_vercel_ai_gateway_responses_stream_object")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("vercel-ai-gateway"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_gateway_stream_object")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(request_body["model"], "openai/gpt-4.1-mini");
        assert_eq!(request_body["max_output_tokens"], 48);
        assert_eq!(request_body["temperature"], 0.0);
        assert_eq!(request_body["stream"], true);
        assert_eq!(
            request_body["text"]["format"],
            json!({
                "type": "json_schema",
                "name": "gateway_stream_answer",
                "description": "A streamed Gateway Responses answer object.",
                "schema": object_schema,
                "strict": true
            })
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_streams_file_prompt_parts() {
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
                    openai_responses_stream_body(),
                )
                .with_headers(Headers::from([(
                    "content-type".to_string(),
                    "text/event-stream".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_messages(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                            "Summarize the report",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/report.pdf")
                                    .expect("url parses"),
                            },
                            "application/pdf",
                        )),
                    ]),
                )]),
            )
            .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "Hello Gateway Responses stream");
        assert_eq!(
            result.text_stream,
            vec!["Hello ", "Gateway Responses stream"]
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(result.response.id.as_deref(), Some("resp_gateway_stream"));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("vercel-ai-gateway"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_gateway_stream")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/responses");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "openai/gpt-4.1-mini",
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Summarize the report"
                            },
                            {
                                "type": "input_file",
                                "file_url": "https://example.com/report.pdf"
                            }
                        ]
                    }
                ],
                "stream": true
            }))
        );
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_runs_generate_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let body = if call_number == 1 {
                    json!({
                        "id": "resp_gateway_tool_call",
                        "created_at": 1711115037,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "id": "fc_gateway_weather",
                                "type": "function_call",
                                "call_id": "call_gateway_weather",
                                "name": "weather",
                                "arguments": "{\"location\":\"Brisbane\"}"
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
                            "output_tokens": 3
                        }
                    })
                } else {
                    json!({
                        "id": "resp_gateway_tool_final",
                        "created_at": 1711115038,
                        "model": "openai/gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Brisbane is sunny through Gateway Responses."
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 6
                        }
                    })
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    body.to_string(),
                ))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string"
                }
            },
            "required": ["location"]
        }))
        .expect("schema deserializes");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Weather in Brisbane?"))
                .expect("prompt is valid")
                .with_tool(
                    Tool::new("weather", input_schema.clone())
                        .with_description("Get weather for a location")
                        .with_execute(|input, options| async move {
                            Ok(json!({
                                "location": input
                                    .get("location")
                                    .and_then(JsonValue::as_str)
                                    .unwrap_or("Brisbane"),
                                "forecast": "sunny",
                                "toolCallId": options.tool_call_id
                            }))
                        }),
                )
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "weather".to_string(),
                })
                .with_max_steps(2),
        ));

        assert_eq!(result.text, "Brisbane is sunny through Gateway Responses.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_results.len(), 1);

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| {
            request.method == ProviderApiRequestMethod::Post
                && request.url == "https://ai-gateway.test/v1/responses"
                && request.headers.get("authorization").map(String::as_str)
                    == Some("Bearer test-api-key")
                && request.headers.get("custom-header").map(String::as_str) == Some("value")
        }));

        let first_body = requests[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("first request body is JSON");
        assert_eq!(first_body.get("model"), Some(&json!("openai/gpt-4.1-mini")));
        assert_eq!(
            first_body.get("tools"),
            Some(&json!([
                {
                    "type": "function",
                    "name": "weather",
                    "description": "Get weather for a location",
                    "parameters": input_schema
                }
            ]))
        );
        assert_eq!(
            first_body.get("tool_choice"),
            Some(&json!({
                "type": "function",
                "name": "weather"
            }))
        );

        let second_body = requests[1]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("second request body is JSON");
        let second_input = second_body
            .get("input")
            .and_then(JsonValue::as_array)
            .expect("second request input is an array");
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("item_reference")
                && item.get("id").and_then(JsonValue::as_str) == Some("fc_gateway_weather")
        }));
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(JsonValue::as_str) == Some("call_gateway_weather")
                && item
                    .get("output")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|output| output.contains("\"forecast\":\"sunny\""))
        }));
    }

    #[test]
    fn vercel_ai_gateway_openai_responses_runs_stream_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let body = match call_number {
                    1 => sse_body([
                        json!({
                            "type": "response.created",
                            "response": {
                                "id": "resp_gateway_stream_tool_call",
                                "created_at": 1711115037,
                                "model": "openai/gpt-4.1-mini"
                            }
                        }),
                        json!({
                            "type": "response.output_item.added",
                            "output_index": 0,
                            "item": {
                                "id": "fc_gateway_stream_weather",
                                "type": "function_call",
                                "call_id": "call_gateway_stream_weather",
                                "name": "weather",
                                "arguments": ""
                            }
                        }),
                        json!({
                            "type": "response.function_call_arguments.delta",
                            "item_id": "fc_gateway_stream_weather",
                            "output_index": 0,
                            "delta": "{\"location\""
                        }),
                        json!({
                            "type": "response.function_call_arguments.delta",
                            "item_id": "fc_gateway_stream_weather",
                            "output_index": 0,
                            "delta": ":\"Brisbane\"}"
                        }),
                        json!({
                            "type": "response.function_call_arguments.done",
                            "item_id": "fc_gateway_stream_weather",
                            "output_index": 0,
                            "arguments": "{\"location\":\"Brisbane\"}"
                        }),
                        json!({
                            "type": "response.output_item.done",
                            "output_index": 0,
                            "item": {
                                "id": "fc_gateway_stream_weather",
                                "type": "function_call",
                                "call_id": "call_gateway_stream_weather",
                                "name": "weather",
                                "arguments": "{\"location\":\"Brisbane\"}"
                            }
                        }),
                        json!({
                            "type": "response.completed",
                            "response": {
                                "id": "resp_gateway_stream_tool_call",
                                "created_at": 1711115037,
                                "model": "openai/gpt-4.1-mini",
                                "output": [
                                    {
                                        "id": "fc_gateway_stream_weather",
                                        "type": "function_call",
                                        "call_id": "call_gateway_stream_weather",
                                        "name": "weather",
                                        "arguments": "{\"location\":\"Brisbane\"}"
                                    }
                                ],
                                "usage": {
                                    "input_tokens": 9,
                                    "output_tokens": 3
                                }
                            }
                        }),
                    ]),
                    2 => sse_body([
                        json!({
                            "type": "response.created",
                            "response": {
                                "id": "resp_gateway_stream_tool_final",
                                "created_at": 1711115038,
                                "model": "openai/gpt-4.1-mini"
                            }
                        }),
                        json!({
                            "type": "response.output_text.delta",
                            "item_id": "msg_gateway_stream_tool_final",
                            "output_index": 0,
                            "content_index": 0,
                            "delta": "Brisbane is sunny through "
                        }),
                        json!({
                            "type": "response.output_text.delta",
                            "item_id": "msg_gateway_stream_tool_final",
                            "output_index": 0,
                            "content_index": 0,
                            "delta": "Gateway Responses stream."
                        }),
                        json!({
                            "type": "response.output_text.done",
                            "item_id": "msg_gateway_stream_tool_final",
                            "output_index": 0,
                            "content_index": 0,
                            "text": "Brisbane is sunny through Gateway Responses stream."
                        }),
                        json!({
                            "type": "response.completed",
                            "response": {
                                "id": "resp_gateway_stream_tool_final",
                                "created_at": 1711115038,
                                "model": "openai/gpt-4.1-mini",
                                "usage": {
                                    "input_tokens": 12,
                                    "output_tokens": 7
                                }
                            }
                        }),
                    ]),
                    other => panic!("unexpected request #{other}"),
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", body)
                    .with_headers(Headers::from([
                        ("content-type".to_string(), "text/event-stream".to_string()),
                        (
                            "x-request-id".to_string(),
                            format!("req_vercel_ai_gateway_responses_tool_stream_{call_number}"),
                        ),
                    ])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.responses("openai/gpt-4.1-mini");
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string"
                }
            },
            "required": ["location"]
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Weather in Brisbane?"))
                .expect("prompt is valid")
                .with_tool(
                    Tool::new("weather", input_schema.clone())
                        .with_description("Get weather for a location")
                        .with_execute(|input, options| async move {
                            Ok(json!({
                                "location": input
                                    .get("location")
                                    .and_then(JsonValue::as_str)
                                    .unwrap_or("Brisbane"),
                                "forecast": "sunny",
                                "toolCallId": options.tool_call_id
                            }))
                        }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(
            result.text,
            "Brisbane is sunny through Gateway Responses stream."
        );
        assert_eq!(
            result.text_stream,
            vec![
                "Brisbane is sunny through ".to_string(),
                "Gateway Responses stream.".to_string()
            ]
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| {
            request.method == ProviderApiRequestMethod::Post
                && request.url == "https://ai-gateway.test/v1/responses"
                && request.headers.get("authorization").map(String::as_str)
                    == Some("Bearer test-api-key")
                && request.headers.get("custom-header").map(String::as_str) == Some("value")
        }));

        let request_bodies = requests
            .iter()
            .map(|request| {
                request
                    .body
                    .as_ref()
                    .and_then(ProviderApiRequestBody::as_text)
                    .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                    .expect("request body is JSON")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            request_bodies[0].get("model"),
            Some(&json!("openai/gpt-4.1-mini"))
        );
        assert_eq!(request_bodies[0].get("stream"), Some(&json!(true)));
        assert_eq!(
            request_bodies[0].get("tools"),
            Some(&json!([
                {
                    "type": "function",
                    "name": "weather",
                    "description": "Get weather for a location",
                    "parameters": input_schema
                }
            ]))
        );

        let second_input = request_bodies[1]
            .get("input")
            .and_then(JsonValue::as_array)
            .expect("second request input is an array");
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("item_reference")
                && item.get("id").and_then(JsonValue::as_str) == Some("fc_gateway_stream_weather")
        }));
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(JsonValue::as_str)
                    == Some("call_gateway_stream_weather")
                && item
                    .get("output")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|output| output.contains("\"forecast\":\"sunny\""))
        }));
        assert_eq!(request_bodies[1].get("stream"), Some(&json!(true)));
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
    fn vercel_ai_gateway_openai_compatible_runs_stream_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let body = match call_number {
                    1 => sse_body([
                        json!({
                            "id": "chatcmpl-gateway-tool-stream-1",
                            "model": "openai/gpt-4.1-mini",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "tool_calls": [
                                            {
                                                "index": 0,
                                                "id": "call_1",
                                                "type": "function",
                                                "function": {
                                                    "name": "weather",
                                                    "arguments": "{\"city\""
                                                }
                                            }
                                        ]
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-gateway-tool-stream-1",
                            "model": "openai/gpt-4.1-mini",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "tool_calls": [
                                            {
                                                "index": 0,
                                                "function": {
                                                    "arguments": ":\"Brisbane\"}"
                                                }
                                            }
                                        ]
                                    },
                                    "finish_reason": "tool_calls"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 6,
                                "completion_tokens": 3
                            }
                        }),
                    ]),
                    2 => sse_body([
                        json!({
                            "id": "chatcmpl-gateway-tool-stream-2",
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
                            "id": "chatcmpl-gateway-tool-stream-2",
                            "model": "openai/gpt-4.1-mini",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "Brisbane is sunny."
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-gateway-tool-stream-2",
                            "model": "openai/gpt-4.1-mini",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 10,
                                "completion_tokens": 7
                            }
                        }),
                    ]),
                    other => panic!("unexpected request #{other}"),
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", body)
                    .with_headers(Headers::from([
                        ("content-type".to_string(), "text/event-stream".to_string()),
                        (
                            "x-request-id".to_string(),
                            format!("req_vercel_ai_gateway_tool_stream_{call_number}"),
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
        let model = provider.language_model("openai/gpt-4.1-mini");
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Weather?"))
                .expect("prompt is valid")
                .with_tool(
                    Tool::new("weather", input_schema.clone())
                        .with_description("Get weather")
                        .with_execute(|input, options| async move {
                            Ok(json!({
                                "city": input["city"],
                                "forecast": "sunny",
                                "toolCallId": options.tool_call_id
                            }))
                        }),
                )
                .with_max_steps(2),
        ));

        assert_eq!(result.text, "Brisbane is sunny.");
        assert_eq!(result.text_stream, vec!["Brisbane is sunny."]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        let request_bodies = requests
            .iter()
            .map(|request| {
                request
                    .body
                    .as_ref()
                    .and_then(ProviderApiRequestBody::as_text)
                    .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                    .expect("request body is JSON")
            })
            .collect::<Vec<_>>();
        assert_eq!(request_bodies.len(), 2);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(
            requests[0].url,
            "https://ai-gateway.test/v1/chat/completions"
        );
        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            requests[0].headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request_bodies[0],
            json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Weather?"
                    }
                ],
                "stream": true,
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "description": "Get weather",
                            "parameters": input_schema.clone()
                        }
                    }
                ]
            })
        );
        assert_eq!(
            request_bodies[1],
            json!({
                "model": "openai/gpt-4.1-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Weather?"
                    },
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "weather",
                                    "arguments": "{\"city\":\"Brisbane\"}"
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "content": "{\"city\":\"Brisbane\",\"forecast\":\"sunny\",\"toolCallId\":\"call_1\"}",
                        "tool_call_id": "call_1"
                    }
                ],
                "stream": true,
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "description": "Get weather",
                            "parameters": input_schema.clone()
                        }
                    }
                ]
            })
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
    fn vercel_ai_gateway_openai_compatible_generates_images_through_openai_images_endpoint() {
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
                        "data": [
                            {
                                "b64_json": "aW1hZ2UtMQ=="
                            },
                            {
                                "b64_json": "aW1hZ2UtMg=="
                            }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_vercel_ai_gateway_image".to_string(),
                )])))))
            });
        let provider = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://ai-gateway.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.image_model("google/imagen-4.0-generate-001");

        assert_eq!(model.provider(), "vercel-ai-gateway.image");
        assert_eq!(model.model_id(), "google/imagen-4.0-generate-001");
        assert_eq!(poll_ready(model.max_images_per_call()), Some(10));

        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "vercelAiGateway": {
                "user": "image-user"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_image(
            GenerateImageOptions::new(&model, "A tiny geometric icon")
                .with_n(2)
                .with_size("1024x1024")
                .with_provider_options(provider_options)
                .with_header("x-call", "image"),
        ))
        .expect("image generation succeeds");

        assert_eq!(result.images.len(), 2);
        assert_eq!(result.image.base64(), "aW1hZ2UtMQ==");
        assert_eq!(
            result
                .responses
                .first()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_vercel_ai_gateway_image")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://ai-gateway.test/v1/images/generations");
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
            Some("image")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "google/imagen-4.0-generate-001",
                "prompt": "A tiny geometric icon",
                "n": 2,
                "size": "1024x1024",
                "user": "image-user",
                "response_format": "b64_json"
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
    #[ignore = "requires a Vercel AI Gateway API key, makes a live OpenAI-compatible model call, and exports OTLP telemetry locally"]
    fn live_vercel_ai_gateway_openai_compatible_generate_text_with_otel() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible telemetry test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let receiver =
            ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("local OTLP receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id.clone());
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-vercel-ai-gateway-otel-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("live-gateway-otel")
                    .with_record_inputs(true)
                    .with_record_outputs(true)
                    .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
            ),
        ));

        assert!(
            result
                .text
                .to_lowercase()
                .contains("rust-vercel-ai-gateway-otel-ok"),
            "Gateway OpenAI-compatible telemetry response did not contain expected marker"
        );

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.name == format!("invoke_agent {model_id}")),
            "live Gateway telemetry did not record the operation span"
        );
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.attributes.get("gen_ai.agent.name")
                    == Some(&json!("live-gateway-otel"))),
            "live Gateway telemetry did not include the configured function id"
        );

        assert_live_gateway_otel_payload(
            &receiver,
            &tracer,
            &model_id,
            "live-gateway-otel",
            "ai-sdk-rust-live-gateway-otel",
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key, makes a live OpenAI-compatible stream call, and exports OTLP telemetry locally"]
    fn live_vercel_ai_gateway_openai_compatible_stream_text_with_otel() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible stream telemetry test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let receiver =
            ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("local OTLP receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id.clone());
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-vercel-ai-gateway-stream-otel-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("live-gateway-stream-otel")
                    .with_record_inputs(true)
                    .with_record_outputs(true)
                    .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
            ),
        ));

        assert!(
            result
                .text
                .to_lowercase()
                .contains("rust-vercel-ai-gateway-stream-otel-ok"),
            "Gateway OpenAI-compatible stream telemetry response did not contain expected marker"
        );

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.name == format!("invoke_agent {model_id}")),
            "live Gateway stream telemetry did not record the operation span"
        );
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.attributes.get("gen_ai.agent.name")
                    == Some(&json!("live-gateway-stream-otel"))),
            "live Gateway stream telemetry did not include the configured function id"
        );

        assert_live_gateway_otel_payload(
            &receiver,
            &tracer,
            &model_id,
            "live-gateway-stream-otel",
            "ai-sdk-rust-live-gateway-stream-otel",
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key, makes a live OpenAI-compatible object call, and exports OTLP telemetry locally"]
    fn live_vercel_ai_gateway_openai_compatible_generate_object_with_otel() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible object telemetry test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let receiver =
            ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("local OTLP receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id.clone());
        let object_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "marker": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["marker", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return a JSON object with marker exactly \"rust-vercel-ai-gateway-object-otel-ok\" and count exactly 9.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(80)
            .with_temperature(0.0)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("live-gateway-object-otel")
                    .with_record_inputs(true)
                    .with_record_outputs(true)
                    .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
            ),
        ))
        .expect("Gateway OpenAI-compatible object generation succeeds");

        assert_eq!(
            result.object.get("marker").and_then(JsonValue::as_str),
            Some("rust-vercel-ai-gateway-object-otel-ok")
        );
        assert_eq!(
            result.object.get("count").and_then(JsonValue::as_i64),
            Some(9)
        );

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.name == format!("invoke_agent {model_id}")),
            "live Gateway object telemetry did not record the operation span"
        );
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.attributes.get("gen_ai.agent.name")
                    == Some(&json!("live-gateway-object-otel"))),
            "live Gateway object telemetry did not include the configured function id"
        );

        assert_live_gateway_otel_payload(
            &receiver,
            &tracer,
            &model_id,
            "live-gateway-object-otel",
            "ai-sdk-rust-live-gateway-object-otel",
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key, makes a live OpenAI-compatible stream object call, and exports OTLP telemetry locally"]
    fn live_vercel_ai_gateway_openai_compatible_stream_object_with_otel() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible stream object telemetry test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let receiver =
            ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("local OTLP receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id.clone());
        let object_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "marker": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["marker", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return a JSON object with marker exactly \"rust-vercel-ai-gateway-stream-object-otel-ok\" and count exactly 10.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(80)
            .with_temperature(0.0)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("live-gateway-stream-object-otel")
                    .with_record_inputs(true)
                    .with_record_outputs(true)
                    .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
            ),
        ));
        let object = result
            .object
            .expect("Gateway OpenAI-compatible stream object is generated");

        assert_eq!(
            object.get("marker").and_then(JsonValue::as_str),
            Some("rust-vercel-ai-gateway-stream-object-otel-ok")
        );
        assert_eq!(object.get("count").and_then(JsonValue::as_i64), Some(10));

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.name == format!("invoke_agent {model_id}")),
            "live Gateway stream object telemetry did not record the operation span"
        );
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.attributes.get("gen_ai.agent.name")
                    == Some(&json!("live-gateway-stream-object-otel"))),
            "live Gateway stream object telemetry did not include the configured function id"
        );

        assert_live_gateway_otel_payload(
            &receiver,
            &tracer,
            &model_id,
            "live-gateway-stream-object-otel",
            "ai-sdk-rust-live-gateway-stream-object-otel",
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses API call"]
    fn live_vercel_ai_gateway_openai_responses_generate_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply exactly with: gateway responses ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(20)
            .with_temperature(0.0),
        ));

        assert!(
            result
                .text
                .to_ascii_lowercase()
                .contains("gateway responses ok"),
            "Gateway OpenAI Responses output did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses JSON response-format call"]
    fn live_vercel_ai_gateway_openai_responses_no_schema_json_response_format() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses JSON response-format test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return only this JSON object, with no markdown: {\"gateway\":\"json\"}",
                ),
            )
            .expect("prompt is valid")
            .with_response_format(LanguageModelResponseFormat::json())
            .with_max_output_tokens(40)
            .with_temperature(0.0),
        ));

        let output = result.text.trim();
        let json_output =
            serde_json::from_str::<JsonValue>(output).expect("Gateway output is valid JSON");
        assert_eq!(json_output["gateway"], "json");
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses stream call"]
    fn live_vercel_ai_gateway_openai_responses_stream_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses stream test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply exactly with: gateway responses stream ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0),
        ));

        assert!(
            result
                .text
                .to_ascii_lowercase()
                .contains("gateway responses stream ok"),
            "Gateway OpenAI Responses stream output did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key, makes a live OpenAI Responses model call, and exports OTLP telemetry locally"]
    fn live_vercel_ai_gateway_openai_responses_generate_text_with_otel() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses telemetry test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let receiver =
            ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("local OTLP receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id.clone());
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-gateway-responses-otel-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("live-gateway-responses-otel")
                    .with_record_inputs(true)
                    .with_record_outputs(true)
                    .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
            ),
        ));

        assert!(
            result
                .text
                .to_ascii_lowercase()
                .contains("rust-gateway-responses-otel-ok"),
            "Gateway OpenAI Responses telemetry response did not contain expected marker"
        );

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.name == format!("invoke_agent {model_id}")),
            "live Gateway Responses telemetry did not record the operation span"
        );
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.attributes.get("gen_ai.agent.name")
                    == Some(&json!("live-gateway-responses-otel"))),
            "live Gateway Responses telemetry did not include the configured function id"
        );

        assert_live_gateway_otel_payload(
            &receiver,
            &tracer,
            &model_id,
            "live-gateway-responses-otel",
            "ai-sdk-rust-live-gateway-responses-otel",
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key, makes a live OpenAI Responses stream call, and exports OTLP telemetry locally"]
    fn live_vercel_ai_gateway_openai_responses_stream_text_with_otel() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses stream telemetry test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let receiver =
            ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("local OTLP receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id.clone());
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-gateway-responses-stream-otel-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(24)
            .with_temperature(0.0)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("live-gateway-responses-stream-otel")
                    .with_record_inputs(true)
                    .with_record_outputs(true)
                    .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
            ),
        ));

        assert!(
            result
                .text
                .to_ascii_lowercase()
                .contains("rust-gateway-responses-stream-otel-ok"),
            "Gateway OpenAI Responses stream telemetry response did not contain expected marker"
        );

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.name == format!("invoke_agent {model_id}")),
            "live Gateway Responses stream telemetry did not record the operation span"
        );
        assert!(
            tracer
                .spans
                .iter()
                .any(|span| span.attributes.get("gen_ai.agent.name")
                    == Some(&json!("live-gateway-responses-stream-otel"))),
            "live Gateway Responses stream telemetry did not include the configured function id"
        );

        assert_live_gateway_otel_payload(
            &receiver,
            &tracer,
            &model_id,
            "live-gateway-responses-stream-otel",
            "ai-sdk-rust-live-gateway-responses-stream-otel",
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses object call"]
    fn live_vercel_ai_gateway_openai_responses_generate_object() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses object test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let object_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "marker": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["marker", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return a JSON object with marker exactly \"rust-gateway-responses-object-ok\" and count exactly 11.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(80)
            .with_temperature(0.0),
        ))
        .expect("Gateway OpenAI Responses object generation succeeds");

        assert_eq!(
            result.object.get("marker").and_then(JsonValue::as_str),
            Some("rust-gateway-responses-object-ok")
        );
        assert_eq!(
            result.object.get("count").and_then(JsonValue::as_i64),
            Some(11)
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses stream object call"]
    fn live_vercel_ai_gateway_openai_responses_stream_object() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses stream object test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let object_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "marker": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["marker", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return a JSON object with marker exactly \"rust-gateway-responses-stream-object-ok\" and count exactly 12.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(80)
            .with_temperature(0.0),
        ));
        let object = result
            .object
            .expect("Gateway OpenAI Responses stream object is generated");

        assert_eq!(
            object.get("marker").and_then(JsonValue::as_str),
            Some("rust-gateway-responses-stream-object-ok")
        );
        assert_eq!(object.get("count").and_then(JsonValue::as_i64), Some(12));
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses tool-loop model call"]
    fn live_vercel_ai_gateway_openai_responses_generate_text_tool_loop() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses tool-loop test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Call the weather tool for Brisbane, then reply with a short sentence that includes Brisbane and sunny.",
                ),
            )
            .expect("prompt is valid")
            .with_provider_options(gateway_responses_store_false_provider_options())
            .with_tool(
                Tool::new("weather", input_schema.clone())
                    .with_description("Get the current weather for a city")
                    .with_execute(|input, options| async move {
                        Ok(json!({
                            "city": input
                                .get("city")
                                .and_then(JsonValue::as_str)
                                .unwrap_or("Brisbane"),
                            "forecast": "sunny",
                            "toolCallId": options.tool_call_id
                        }))
                    }),
            )
            .with_prepare_step(|options| async move {
                if options.step_number == 0 {
                    PrepareStepResult::new().with_tool_choice(LanguageModelToolChoice::Tool {
                        tool_name: "weather".to_string(),
                    })
                } else {
                    PrepareStepResult::new()
                }
            })
            .with_max_steps(2)
            .with_max_output_tokens(56)
            .with_temperature(0.0),
        ));

        let text = result.text.to_lowercase();
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_results.len(), 1);
        assert!(
            text.contains("brisbane") && text.contains("sunny"),
            "Gateway OpenAI Responses tool-loop response did not include the expected tool result"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI Responses stream tool-loop model call"]
    fn live_vercel_ai_gateway_openai_responses_stream_text_tool_loop() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI Responses stream tool-loop test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RESPONSES_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .responses(model_id);
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Call the weather tool for Brisbane, then reply with a short sentence that includes Brisbane and sunny.",
                ),
            )
            .expect("prompt is valid")
            .with_provider_options(gateway_responses_store_false_provider_options())
            .with_tool(
                Tool::new("weather", input_schema)
                    .with_description("Get the current weather for a city")
                    .with_execute(|input, options| async move {
                        Ok(json!({
                            "city": input
                                .get("city")
                                .and_then(JsonValue::as_str)
                                .unwrap_or("Brisbane"),
                            "forecast": "sunny",
                            "toolCallId": options.tool_call_id
                        }))
                    }),
            )
            .with_max_steps(2)
            .with_max_output_tokens(56)
            .with_temperature(0.0),
        ));

        let text = result.text.to_lowercase();
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_results.len(), 1);
        assert!(
            text.contains("brisbane") && text.contains("sunny"),
            "Gateway OpenAI Responses stream tool-loop response did not include the expected tool result"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible tool-loop model call"]
    fn live_vercel_ai_gateway_openai_compatible_generate_text_tool_loop() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible tool-loop test because no API key is configured"
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
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("schema deserializes");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Call the weather tool for Brisbane, then reply with a short sentence that includes Brisbane and sunny.",
                ),
            )
            .expect("prompt is valid")
            .with_tool(
                Tool::new("weather", input_schema)
                    .with_description("Get the current weather for a city")
                    .with_execute(|input, options| async move {
                        Ok(json!({
                            "city": input
                                .get("city")
                                .and_then(JsonValue::as_str)
                                .unwrap_or("Brisbane"),
                            "forecast": "sunny",
                            "toolCallId": options.tool_call_id
                        }))
                    }),
            )
            .with_prepare_step(|options| async move {
                if options.step_number == 0 {
                    PrepareStepResult::new().with_tool_choice(LanguageModelToolChoice::Tool {
                        tool_name: "weather".to_string(),
                    })
                } else {
                    PrepareStepResult::new()
                }
            })
            .with_max_steps(2)
            .with_max_output_tokens(48)
            .with_temperature(0.0),
        ));

        let text = result.text.to_lowercase();
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_results.len(), 1);
        assert!(
            text.contains("brisbane") && text.contains("sunny"),
            "Gateway OpenAI-compatible tool-loop response did not include the expected tool result"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible MCP tool-loop model call"]
    fn live_vercel_ai_gateway_openai_compatible_generate_text_mcp_tool_loop() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible MCP tool-loop test because no API key is configured"
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
        let client = create_mcp_client(
            McpClientConfig::new(MockMcpTransport::new())
                .with_client_name("live-gateway-mcp-test-client"),
        )
        .expect("MCP client initializes");
        let mcp_tools = client.tools().expect("MCP tools build");

        let mut options = GenerateTextOptions::from_prompt(
            &model,
            Prompt::from_prompt(
                "Call the mock-tool MCP tool with foo set to bar, then reply with one short sentence about the tool result.",
            ),
        )
        .expect("prompt is valid")
        .with_active_tools(["mock-tool"])
        .with_prepare_step(|options| async move {
            if options.step_number == 0 {
                PrepareStepResult::new().with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "mock-tool".to_string(),
                })
            } else {
                PrepareStepResult::new()
            }
        })
        .with_max_steps(2)
        .with_max_output_tokens(56)
        .with_temperature(0.0);
        for tool in mcp_tools.into_values() {
            options = options.with_tool(tool);
        }

        let result = poll_ready(generate_text(options));

        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_name, "mock-tool");
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(
            result.tool_results[0].output["content"][0]["text"],
            "Mock tool call result"
        );
        assert!(!result.text.trim().is_empty());
        client.close().expect("MCP client closes");
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
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible object call"]
    fn live_vercel_ai_gateway_openai_compatible_generate_object() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible object test because no API key is configured"
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
        let object_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "marker": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["marker", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return a JSON object with marker exactly \"rust-vercel-ai-gateway-object-ok\" and count exactly 7.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(80)
            .with_temperature(0.0),
        ))
        .expect("Gateway OpenAI-compatible object generation succeeds");

        assert_eq!(
            result.object.get("marker").and_then(JsonValue::as_str),
            Some("rust-vercel-ai-gateway-object-ok")
        );
        assert_eq!(
            result.object.get("count").and_then(JsonValue::as_i64),
            Some(7)
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible stream object call"]
    fn live_vercel_ai_gateway_openai_compatible_stream_object() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible stream object test because no API key is configured"
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
        let object_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "marker": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["marker", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return a JSON object with marker exactly \"rust-vercel-ai-gateway-stream-object-ok\" and count exactly 8.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(80)
            .with_temperature(0.0),
        ));
        let object = result
            .object
            .expect("Gateway OpenAI-compatible stream object is generated");

        assert_eq!(
            object.get("marker").and_then(JsonValue::as_str),
            Some("rust-vercel-ai-gateway-stream-object-ok")
        );
        assert_eq!(object.get("count").and_then(JsonValue::as_i64), Some(8));
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

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible image call"]
    fn live_vercel_ai_gateway_openai_compatible_generate_image() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible image test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_IMAGE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_IMAGE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_IMAGE_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_IMAGE_MODEL"))
            .unwrap_or_else(|_| "google/imagen-4.0-fast-generate-001".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .image_model(model_id);
        let result = poll_ready(generate_image(
            GenerateImageOptions::new(
                &model,
                "A simple flat icon of the Rust gear on a white background",
            )
            .with_n(1),
        ))
        .expect("Gateway OpenAI-compatible image generation succeeds");

        assert_eq!(result.images.len(), 1);
        assert!(
            !result.image.base64().is_empty(),
            "Gateway OpenAI-compatible image response was empty"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible chat image-output call"]
    fn live_vercel_ai_gateway_openai_compatible_generate_text_with_image_output() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible chat image-output test because no API key is configured"
            );
            return;
        };
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_IMAGE_CHAT_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_IMAGE_CHAT_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_IMAGE_CHAT_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_IMAGE_CHAT_MODEL"))
            .unwrap_or_else(|_| "google/gemini-2.5-flash-image".to_string());
        let model = VercelAiGatewayOpenAICompatibleProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "vercelAiGateway": {
                "modalities": ["text", "image"]
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Generate a simple flat image of a blue square and describe it in one short sentence.",
                ),
            )
            .expect("prompt is valid")
            .with_provider_options(provider_options)
            .with_max_output_tokens(80)
            .with_temperature(0.0),
        ));

        assert!(
            !result.files.is_empty(),
            "Gateway OpenAI-compatible chat image-output response did not include files"
        );
        assert!(
            result
                .files
                .iter()
                .any(|file| file.media_type().starts_with("image/") && !file.base64().is_empty()),
            "Gateway OpenAI-compatible chat image-output files were empty or non-image"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible model list call"]
    fn live_vercel_ai_gateway_openai_compatible_list_models() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible model list test because no API key is configured"
            );
            return;
        };
        let provider = VercelAiGatewayOpenAICompatibleProvider::new().with_api_key(api_key);
        let result = poll_ready(provider.list_models()).expect("Gateway model list fetch succeeds");

        assert!(
            !result.data.is_empty(),
            "Gateway OpenAI-compatible model list was empty"
        );
        assert!(
            result.model_ids().any(|model_id| model_id.contains('/')),
            "Gateway model list did not include provider-qualified model ids"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI-compatible model retrieval call"]
    fn live_vercel_ai_gateway_openai_compatible_retrieve_model() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!(
                "skipping live Gateway OpenAI-compatible model retrieval test because no API key is configured"
            );
            return;
        };
        let provider = VercelAiGatewayOpenAICompatibleProvider::new().with_api_key(api_key);
        let model_id = env::var("AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_RETRIEVE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_OPENAI_COMPATIBLE_RETRIEVE_MODEL"))
            .or_else(|_| env::var("AI_SDK_RUST_GATEWAY_RETRIEVE_MODEL"))
            .or_else(|_| env::var("AI_GATEWAY_RETRIEVE_MODEL"))
            .ok()
            .or_else(|| {
                poll_ready(provider.list_models())
                    .ok()
                    .and_then(|models| models.data.into_iter().next().map(|model| model.id))
            })
            .unwrap_or_else(|| "openai/gpt-4.1-mini".to_string());
        let result = poll_ready(provider.retrieve_model(&model_id))
            .expect("Gateway model retrieval succeeds");

        assert_eq!(result.id, model_id);
        assert!(
            result.object.as_deref().unwrap_or("model") == "model",
            "Gateway model retrieval returned an unexpected object type"
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

    fn openai_responses_stream_body() -> String {
        sse_body([
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_gateway_stream",
                    "created_at": 1711115037,
                    "model": "openai/gpt-4.1-mini"
                }
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "Hello "
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "Gateway Responses stream"
            }),
            json!({
                "type": "response.output_text.done",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "text": "Hello Gateway Responses stream"
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_gateway_stream",
                    "created_at": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "usage": {
                        "input_tokens": 5,
                        "output_tokens": 4
                    }
                }
            }),
        ])
    }

    fn openai_responses_object_stream_body() -> String {
        sse_body([
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_gateway_stream_object",
                    "created_at": 1711115037,
                    "model": "openai/gpt-4.1-mini"
                }
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_object_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "{\"answer\":\"Gateway Responses "
            }),
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_object_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "stream object\",\"count\":5}"
            }),
            json!({
                "type": "response.output_text.done",
                "item_id": "msg_object_1",
                "output_index": 0,
                "content_index": 0,
                "text": "{\"answer\":\"Gateway Responses stream object\",\"count\":5}"
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_gateway_stream_object",
                    "created_at": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 8
                    }
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

    fn env_lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
        }
    }

    fn json_object(value: JsonValue) -> JsonObject {
        serde_json::from_value(value).expect("value is a JSON object")
    }

    fn otlp_has_span_name(body: &JsonValue, expected: &str) -> bool {
        body.get("resourceSpans")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .flat_map(|resource_span| {
                resource_span
                    .get("scopeSpans")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
            })
            .flat_map(|scope_span| {
                scope_span
                    .get("spans")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
            })
            .any(|span| span.get("name").and_then(JsonValue::as_str) == Some(expected))
    }

    fn otlp_has_string_attribute(body: &JsonValue, key: &str, value: &str) -> bool {
        body.get("resourceSpans")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .flat_map(|resource_span| {
                resource_span
                    .get("scopeSpans")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
            })
            .flat_map(|scope_span| {
                scope_span
                    .get("spans")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
            })
            .flat_map(|span| {
                span.get("attributes")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
            })
            .any(|attribute| {
                attribute.get("key").and_then(JsonValue::as_str) == Some(key)
                    && attribute
                        .get("value")
                        .and_then(|value| value.get("stringValue"))
                        .and_then(JsonValue::as_str)
                        == Some(value)
            })
    }

    fn assert_live_gateway_otel_payload(
        receiver: &ai_sdk_otel::LocalOtlpTraceReceiver,
        tracer: &ai_sdk_otel::MockTracer,
        model_id: &str,
        function_id: &str,
        service_name: &str,
    ) {
        ai_sdk_otel::export_tracer_to_otlp_http_json(
            tracer,
            &ai_sdk_otel::OtlpHttpTraceExportOptions::new(receiver.endpoint())
                .with_service_name(service_name),
        )
        .expect("local OTLP export succeeds");

        let requests = receiver.wait_for_requests(1, std::time::Duration::from_secs(10));
        assert_eq!(requests.len(), 1);
        let body = requests[0].body_json().expect("OTLP body is JSON");
        assert!(
            otlp_has_span_name(&body, &format!("invoke_agent {model_id}")),
            "local OTLP payload did not include the Gateway operation span"
        );
        assert!(
            otlp_has_string_attribute(&body, "gen_ai.agent.name", function_id),
            "local OTLP payload did not include the configured function id"
        );
    }

    fn live_gateway_api_key() -> Option<String> {
        env::var("AI_GATEWAY_API_KEY")
            .or_else(|_| env::var("AI_SDK_RUST_AI_GATEWAY_API_KEY"))
            .or_else(|_| env::var("VERCEL_OIDC_TOKEN"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(load_gateway_api_key_from_dotenv)
    }

    fn gateway_responses_store_false_provider_options() -> ProviderOptions {
        serde_json::from_value(json!({
            "vercelAiGateway": {
                "store": false
            }
        }))
        .expect("provider options deserialize")
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
                "AI_SDK_RUST_AI_GATEWAY_API_KEY" | "AI_GATEWAY_API_KEY" | "VERCEL_OIDC_TOKEN"
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
