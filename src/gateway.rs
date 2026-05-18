pub use ai_sdk_gateway::gateway::*;

#[cfg(test)]
mod tests {
    use super::{
        GatewayProvider, GatewayProviderSettings, GatewayTransport, GatewayTransportFuture,
    };
    use crate::embed::{EmbedOptions, embed};
    use crate::generate_image::{GenerateImageOptions, generate_image};
    use crate::generate_object::{GenerateObjectOptions, generate_object};
    use crate::generate_text::{GenerateTextContentPart, GenerateTextOptions, generate_text};
    use crate::generate_video::{GenerateVideoOptions, generate_video};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{FinishReason, LanguageModelFileData, LanguageModelSource};
    use crate::prompt::Prompt;
    use crate::provider::ProviderOptions;
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        Tool, json_schema,
    };
    use crate::rerank::{RerankDocuments, RerankOptions, rerank};
    use crate::stream_object::{StreamObjectOptions, stream_object};
    use crate::stream_text::{StreamTextOptions, TextStreamPart, stream_text};
    use serde_json::json;
    use std::env;
    use std::fs;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    fn json_object(value: JsonValue) -> JsonObject {
        value.as_object().cloned().expect("JSON value is an object")
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

    fn gateway_stream_body() -> String {
        [
            json!({
                "type": "stream-start",
                "warnings": []
            }),
            json!({
                "type": "response-metadata",
                "id": "resp_gateway",
                "timestamp": "2024-01-02T03:04:05Z",
                "modelId": "openai/gpt-4.1-mini",
                "headers": {
                    "x-request-id": "stream_req"
                }
            }),
            json!({
                "type": "text-start",
                "id": "0"
            }),
            json!({
                "type": "text-delta",
                "id": "0",
                "delta": "Hello "
            }),
            json!({
                "type": "text-delta",
                "id": "0",
                "delta": "Gateway"
            }),
            json!({
                "type": "text-end",
                "id": "0"
            }),
            json!({
                "type": "finish",
                "finishReason": {
                    "unified": "stop",
                    "raw": "stop"
                },
                "usage": {
                    "inputTokens": {
                        "total": 2
                    },
                    "outputTokens": {
                        "total": 3
                    }
                }
            }),
        ]
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .chain(["data: [DONE]\n\n".to_string()])
        .collect::<String>()
    }

    fn gateway_sse_body(events: impl IntoIterator<Item = JsonValue>) -> String {
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .chain(["data: [DONE]\n\n".to_string()])
            .collect::<String>()
    }

    #[test]
    fn gateway_model_generates_text_through_generate_text() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "id": "test-id",
                    "created": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "content": {
                        "type": "text",
                        "text": "Hello from Gateway"
                    },
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 4,
                        "completion_tokens": 3
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req_gateway".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token")
                .with_header("x-provider", "provider-value")
                .with_vercel_request_id("req_gateway_context"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(12)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello from Gateway");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(3));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/language-model");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("maxOutputTokens").cloned()),
            Some(json!(12))
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-gateway-protocol-version")
                .map(String::as_str),
            Some("0.0.1")
        );
        assert_eq!(
            request
                .headers
                .get("ai-gateway-auth-method")
                .map(String::as_str),
            Some("api-key")
        );
        assert_eq!(
            request
                .headers
                .get("ai-language-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request
                .headers
                .get("ai-language-model-id")
                .map(String::as_str),
            Some("openai/gpt-4.1-mini")
        );
        assert_eq!(
            request
                .headers
                .get("ai-language-model-streaming")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("provider-value")
        );
        assert_eq!(
            request
                .headers
                .get("ai-o11y-request-id")
                .map(String::as_str),
            Some("req_gateway_context")
        );
    }

    #[test]
    fn gateway_model_generates_object_through_generate_object() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "id": "test-object-id",
                    "created": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "content": {
                        "type": "text",
                        "text": "{\"answer\":\"Gateway object\",\"count\":2}"
                    },
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 8,
                        "completion_tokens": 6
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req_gateway_object".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let object_schema = json_object(json!({
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
        }));
        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Return a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema.clone()))
            .with_max_output_tokens(32)
            .with_temperature(0.0),
        ))
        .expect("object is generated");

        assert_eq!(result.object["answer"], "Gateway object");
        assert_eq!(result.object["count"], 2);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.total, Some(6));
        assert_eq!(
            result.response.headers.as_ref().and_then(|headers| {
                headers.get("x-request-id").map(std::string::String::as_str)
            }),
            Some("req_gateway_object")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/language-model");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("responseFormat"),
            Some(&json!({
                "type": "json",
                "schema": object_schema
            }))
        );
        assert_eq!(request_body.get("maxOutputTokens"), Some(&json!(32)));
        assert_eq!(
            request_body
                .get("prompt")
                .and_then(JsonValue::as_array)
                .and_then(|prompt| prompt.first())
                .and_then(|message| message.get("content")),
            Some(&json!([
                {
                    "type": "text",
                    "text": "Return a JSON object with answer and count."
                }
            ]))
        );
    }

    #[test]
    fn gateway_model_maps_standard_generate_content_parts_through_generate_text() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "id": "test-id",
                    "created": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "content": [
                        {
                            "type": "text",
                            "text": "Summary"
                        },
                        {
                            "type": "reasoning",
                            "text": "Need search context."
                        },
                        {
                            "type": "source",
                            "sourceType": "url",
                            "id": "src_1",
                            "url": "https://example.com/source",
                            "title": "Example Source"
                        },
                        {
                            "type": "file",
                            "mediaType": "text/plain",
                            "data": {
                                "type": "data",
                                "data": "ZGF0YQ=="
                            }
                        },
                        {
                            "type": "custom",
                            "kind": "gateway.provider-annotation"
                        }
                    ],
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 4,
                        "completion_tokens": 3
                    },
                    "providerMetadata": {
                        "gateway": {
                            "generationId": "gen_123"
                        }
                    }
                })
                .to_string(),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Summarize"))
                .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "Summary");
        assert_eq!(
            result.reasoning_text,
            Some("Need search context.".to_string())
        );
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].media_type(), "text/plain");
        assert_eq!(result.files[0].base64(), "ZGF0YQ==");
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("generationId"))
                .and_then(JsonValue::as_str),
            Some("gen_123")
        );
        assert!(matches!(
            &result.content[0],
            GenerateTextContentPart::Text(text) if text.text == "Summary"
        ));
        assert!(matches!(
            &result.content[1],
            GenerateTextContentPart::Reasoning(reasoning)
                if reasoning.text == "Need search context."
        ));
        assert!(matches!(
            &result.content[2],
            GenerateTextContentPart::Source(LanguageModelSource::Url(source))
                if source.id == "src_1"
                    && source.url == "https://example.com/source"
                    && source.title.as_deref() == Some("Example Source")
        ));
        assert!(matches!(
            &result.content[3],
            GenerateTextContentPart::File(file)
                if file.file.media_type() == "text/plain"
                    && file.file.base64() == "ZGF0YQ=="
        ));
        assert!(matches!(
            &result.content[4],
            GenerateTextContentPart::Custom(custom)
                if custom.kind == "gateway.provider-annotation"
        ));
    }

    #[test]
    fn gateway_model_runs_generate_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            let call_number = {
                let mut requests = captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned");
                requests.push(request.clone());
                requests.len()
            };

            let response = match call_number {
                1 => json!({
                    "id": "gateway-tool-loop-1",
                    "created": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call_1",
                            "toolName": "weather",
                            "input": "{\"city\":\"Brisbane\"}"
                        }
                    ],
                    "finish_reason": "tool-calls",
                    "usage": {
                        "prompt_tokens": 6,
                        "completion_tokens": 3
                    }
                }),
                2 => json!({
                    "id": "gateway-tool-loop-2",
                    "created": 1711115040,
                    "model": "openai/gpt-4.1-mini",
                    "content": [
                        {
                            "type": "text",
                            "text": "The weather in Brisbane is sunny."
                        }
                    ],
                    "finish_reason": "stop",
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
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
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

        let request_bodies = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
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
            request_bodies[0],
            json!({
                "prompt": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "name": "weather",
                        "description": "Get weather",
                        "inputSchema": input_schema.clone()
                    }
                ]
            })
        );
        assert_eq!(
            request_bodies[1],
            json!({
                "prompt": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    },
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool-call",
                                "toolCallId": "call_1",
                                "toolName": "weather",
                                "input": {
                                    "city": "Brisbane"
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "content": [
                            {
                                "type": "tool-result",
                                "toolCallId": "call_1",
                                "toolName": "weather",
                                "output": {
                                    "type": "json",
                                    "value": {
                                        "city": "Brisbane",
                                        "forecast": "sunny",
                                        "toolCallId": "call_1"
                                    }
                                }
                            }
                        ]
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "name": "weather",
                        "description": "Get weather",
                        "inputSchema": input_schema.clone()
                    }
                ]
            })
        );
    }

    #[test]
    fn gateway_model_streams_text_through_stream_text() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                gateway_stream_body(),
            )
            .with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(12)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello Gateway");
        assert_eq!(result.text_stream, vec!["Hello ", "Gateway"]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(2));
        assert_eq!(result.usage.output_tokens.total, Some(3));
        assert!(result.errors.is_empty());

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(
            request
                .headers
                .get("ai-language-model-streaming")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("maxOutputTokens").cloned()),
            Some(json!(12))
        );
    }

    #[test]
    fn gateway_model_streams_object_through_stream_object() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                gateway_sse_body([
                    json!({
                        "type": "stream-start",
                        "warnings": []
                    }),
                    json!({
                        "type": "response-metadata",
                        "id": "resp_gateway_object",
                        "timestamp": "2024-01-02T03:04:05Z",
                        "modelId": "openai/gpt-4.1-mini"
                    }),
                    json!({
                        "type": "text-start",
                        "id": "0"
                    }),
                    json!({
                        "type": "text-delta",
                        "id": "0",
                        "delta": "{\"answer\":\"Gateway "
                    }),
                    json!({
                        "type": "text-delta",
                        "id": "0",
                        "delta": "stream object\",\"count\":3}"
                    }),
                    json!({
                        "type": "text-end",
                        "id": "0"
                    }),
                    json!({
                        "type": "finish",
                        "finishReason": {
                            "unified": "stop",
                            "raw": "stop"
                        },
                        "usage": {
                            "inputTokens": {
                                "total": 9
                            },
                            "outputTokens": {
                                "total": 7
                            }
                        }
                    }),
                ]),
            )
            .with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let object_schema = json_object(json!({
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
        }));

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Stream a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema.clone()))
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
        assert_eq!(result.error, None);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(9));
        assert_eq!(result.usage.output_tokens.total, Some(7));
        assert_eq!(result.response.id.as_deref(), Some("resp_gateway_object"));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .headers
                .get("ai-language-model-streaming")
                .map(String::as_str),
            Some("true")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("responseFormat"),
            Some(&json!({
                "type": "json",
                "schema": object_schema
            }))
        );
        assert_eq!(request_body.get("includeRawChunks"), Some(&json!(false)));
    }

    #[test]
    fn gateway_model_runs_stream_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            let call_number = {
                let mut requests = captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned");
                requests.push(request.clone());
                requests.len()
            };

            let body = match call_number {
                1 => gateway_sse_body([
                    json!({
                        "type": "tool-call",
                        "toolCallId": "call_1",
                        "toolName": "weather",
                        "input": "{\"city\":\"Brisbane\"}"
                    }),
                    json!({
                        "type": "finish",
                        "finishReason": {
                            "unified": "tool-calls",
                            "raw": "tool-calls"
                        },
                        "usage": {
                            "inputTokens": {
                                "total": 6
                            },
                            "outputTokens": {
                                "total": 3
                            }
                        }
                    }),
                ]),
                2 => gateway_sse_body([
                    json!({
                        "type": "text-start",
                        "id": "0",
                        "providerMetadata": {
                            "gateway": {
                                "request": "continued"
                            }
                        }
                    }),
                    json!({
                        "type": "text-delta",
                        "id": "0",
                        "delta": "Brisbane is sunny."
                    }),
                    json!({
                        "type": "text-end",
                        "id": "0"
                    }),
                    json!({
                        "type": "finish",
                        "finishReason": {
                            "unified": "stop",
                            "raw": "stop"
                        },
                        "usage": {
                            "inputTokens": {
                                "total": 10
                            },
                            "outputTokens": {
                                "total": 7
                            }
                        }
                    }),
                ]),
                other => panic!("unexpected request #{other}"),
            };

            Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", body)
                .with_headers(Headers::from([(
                    "content-type".to_string(),
                    "text/event-stream".to_string(),
                )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
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
        assert_eq!(result.usage.input_tokens.total, Some(10));
        assert_eq!(result.usage.output_tokens.total, Some(7));
        assert_eq!(result.total_usage.input_tokens.total, Some(16));
        assert_eq!(result.total_usage.output_tokens.total, Some(10));
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);
        assert_eq!(result.tool_results[0].output["forecast"], "sunny");
        assert!(result.errors.is_empty());

        let request_bodies = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
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
            request_bodies[0],
            json!({
                "headers": {
                    "user-agent": "ai/0.1.0"
                },
                "prompt": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "name": "weather",
                        "description": "Get weather",
                        "inputSchema": input_schema.clone()
                    }
                ]
            })
        );
        assert_eq!(
            request_bodies[1],
            json!({
                "headers": {
                    "user-agent": "ai/0.1.0"
                },
                "prompt": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    },
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool-call",
                                "toolCallId": "call_1",
                                "toolName": "weather",
                                "input": {
                                    "city": "Brisbane"
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "content": [
                            {
                                "type": "tool-result",
                                "toolCallId": "call_1",
                                "toolName": "weather",
                                "output": {
                                    "type": "json",
                                    "value": {
                                        "city": "Brisbane",
                                        "forecast": "sunny",
                                        "toolCallId": "call_1"
                                    }
                                }
                            }
                        ]
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "name": "weather",
                        "description": "Get weather",
                        "inputSchema": input_schema.clone()
                    }
                ]
            })
        );
    }

    #[test]
    fn gateway_model_streams_standard_content_parts_through_stream_text() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                gateway_sse_body([
                    json!({
                        "type": "stream-start",
                        "warnings": []
                    }),
                    json!({
                        "type": "response-metadata",
                        "id": "resp_gateway",
                        "timestamp": "2024-01-02T03:04:05Z",
                        "modelId": "openai/gpt-4.1-mini"
                    }),
                    json!({
                        "type": "reasoning-start",
                        "id": "r1"
                    }),
                    json!({
                        "type": "reasoning-delta",
                        "id": "r1",
                        "delta": "Need search context."
                    }),
                    json!({
                        "type": "reasoning-end",
                        "id": "r1"
                    }),
                    json!({
                        "type": "source",
                        "sourceType": "url",
                        "id": "src_1",
                        "url": "https://example.com/source",
                        "title": "Example Source"
                    }),
                    json!({
                        "type": "file",
                        "mediaType": "text/plain",
                        "data": {
                            "type": "data",
                            "data": "ZGF0YQ=="
                        }
                    }),
                    json!({
                        "type": "custom",
                        "kind": "gateway.provider-annotation"
                    }),
                    json!({
                        "type": "text-start",
                        "id": "0"
                    }),
                    json!({
                        "type": "text-delta",
                        "id": "0",
                        "delta": "Summary"
                    }),
                    json!({
                        "type": "text-end",
                        "id": "0"
                    }),
                    json!({
                        "type": "finish",
                        "finishReason": {
                            "unified": "stop",
                            "raw": "stop"
                        },
                        "usage": {
                            "inputTokens": {
                                "total": 4
                            },
                            "outputTokens": {
                                "total": 3
                            }
                        },
                        "providerMetadata": {
                            "gateway": {
                                "stream": "complete"
                            }
                        }
                    }),
                ]),
            )
            .with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Summarize"))
                .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "Summary");
        assert_eq!(
            result.reasoning_text,
            Some("Need search context.".to_string())
        );
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].media_type, "text/plain");
        assert!(matches!(
            &result.files[0].data,
            LanguageModelFileData::Data { data }
                if serde_json::to_value(data).expect("file data serializes")
                    == json!("ZGF0YQ==")
        ));
        assert_eq!(result.custom_parts.len(), 1);
        assert_eq!(result.custom_parts[0].kind, "gateway.provider-annotation");
        assert_eq!(result.response.id.as_deref(), Some("resp_gateway"));
        assert_eq!(
            result.response.model_id.as_deref(),
            Some("openai/gpt-4.1-mini")
        );
        assert_eq!(
            result.response.timestamp,
            Some(
                time::OffsetDateTime::parse(
                    "2024-01-02T03:04:05Z",
                    &time::format_description::well_known::Rfc3339
                )
                .expect("timestamp parses")
            )
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("stream"))
                .and_then(JsonValue::as_str),
            Some("complete")
        );
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::ReasoningDelta(_)))
        );
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Source(_)))
        );
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::File(_)))
        );
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Custom(_)))
        );
    }

    #[test]
    fn gateway_embedding_model_embeds_through_embed() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "embeddings": [[0.1, 0.2, 0.3]],
                    "usage": {
                        "tokens": 4
                    },
                    "providerMetadata": {
                        "gateway": {
                            "routing": "test"
                        }
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "embed_req".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .embedding_model("openai/text-embedding-3-small");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "dimensions": 64
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(embed(
            EmbedOptions::new(&model, "sunny day at the beach")
                .with_provider_options(provider_options)
                .with_header("Custom-Header", "test-value"),
        ));

        assert_eq!(result.embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(result.usage.tokens, 4);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("routing"))
                .and_then(JsonValue::as_str),
            Some("test")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/embedding-model");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-embedding-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request.headers.get("ai-model-id").map(String::as_str),
            Some("openai/text-embedding-3-small")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("test-value")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("values").cloned(),
            Some(json!(["sunny day at the beach"]))
        );
        assert_eq!(
            request_body
                .get("providerOptions")
                .and_then(|options| options.get("openai"))
                .and_then(|openai| openai.get("dimensions"))
                .and_then(JsonValue::as_u64),
            Some(64)
        );
    }

    #[test]
    fn gateway_image_model_generates_through_generate_image() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "images": ["iVBORw0KGgo=", "iVBORw0KGgoAAAANSUhEUg=="],
                    "warnings": [
                        {
                            "type": "unsupported",
                            "feature": "size",
                            "details": "Use aspect ratio instead."
                        },
                        {
                            "type": "other",
                            "message": "Gateway routed request"
                        }
                    ],
                    "usage": {
                        "inputTokens": 27,
                        "outputTokens": 6240,
                        "totalTokens": 6267
                    },
                    "providerMetadata": {
                        "openai": {
                            "images": [
                                {
                                    "revisedPrompt": "A small red cube"
                                }
                            ]
                        },
                        "gateway": {
                            "routing": {
                                "provider": "openai"
                            },
                            "generationId": "gen_image_123"
                        }
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "img_req".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token")
                .with_header("x-provider", "provider-value"),
        )
        .with_transport(transport)
        .image_model("openai/gpt-image-1");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "quality": "high"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_image(
            GenerateImageOptions::new(&model, "A small red cube")
                .with_n(2)
                .with_size("1024x1024")
                .with_aspect_ratio("1:1")
                .with_seed(42)
                .with_provider_options(provider_options)
                .with_header("Custom-Header", "test-value"),
        ))
        .expect("image generation succeeds");

        assert_eq!(result.images.len(), 2);
        assert_eq!(result.image.base64(), "iVBORw0KGgo=");
        assert_eq!(result.warnings.len(), 2);
        assert_eq!(result.usage.input_tokens, Some(27));
        assert_eq!(result.usage.output_tokens, Some(6240));
        assert_eq!(result.usage.total_tokens, Some(6267));
        assert_eq!(
            result
                .provider_metadata
                .get("gateway")
                .and_then(|metadata| metadata.extra.get("generationId"))
                .and_then(JsonValue::as_str),
            Some("gen_image_123")
        );
        assert_eq!(
            result
                .responses
                .first()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("img_req")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/image-model");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-image-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request.headers.get("ai-model-id").map(String::as_str),
            Some("openai/gpt-image-1")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("test-value")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("provider-value")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("prompt").and_then(JsonValue::as_str),
            Some("A small red cube")
        );
        assert_eq!(request_body.get("n").and_then(JsonValue::as_u64), Some(2));
        assert_eq!(
            request_body
                .get("providerOptions")
                .and_then(|options| options.get("openai"))
                .and_then(|openai| openai.get("quality"))
                .and_then(JsonValue::as_str),
            Some("high")
        );
    }

    #[test]
    fn gateway_reranking_model_reranks_through_rerank() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "ranking": [
                        {
                            "index": 0,
                            "relevanceScore": 0.89
                        },
                        {
                            "index": 2,
                            "relevanceScore": 0.15
                        }
                    ],
                    "providerMetadata": {
                        "gateway": {
                            "cost": "0.002"
                        }
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "rerank_req".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token")
                .with_header("x-provider", "provider-value"),
        )
        .with_transport(transport)
        .reranking_model("cohere/rerank-v3.5");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "maxTokensPerDoc": 512
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(rerank(
            RerankOptions::new(
                &model,
                RerankDocuments::text([
                    "Paris is the capital of France.",
                    "Berlin is the capital of Germany.",
                    "Madrid is the capital of Spain.",
                ]),
                "What is the capital of France?",
            )
            .with_top_n(2)
            .with_provider_options(provider_options)
            .with_header("Custom-Header", "test-value"),
        ));

        assert_eq!(result.ranking.len(), 2);
        assert_eq!(result.ranking[0].original_index, 0);
        assert_eq!(result.ranking[0].score, 0.89);
        assert_eq!(result.ranking[1].original_index, 2);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("cost"))
                .and_then(JsonValue::as_str),
            Some("0.002")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("rerank_req")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/reranking-model");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-reranking-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request.headers.get("ai-model-id").map(String::as_str),
            Some("cohere/rerank-v3.5")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("test-value")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("provider-value")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body
                .pointer("/documents/type")
                .and_then(JsonValue::as_str),
            Some("text")
        );
        assert_eq!(
            request_body
                .pointer("/documents/values/0")
                .and_then(JsonValue::as_str),
            Some("Paris is the capital of France.")
        );
        assert_eq!(
            request_body.get("query").and_then(JsonValue::as_str),
            Some("What is the capital of France?")
        );
        assert_eq!(
            request_body.get("topN").and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            request_body
                .pointer("/providerOptions/cohere/maxTokensPerDoc")
                .and_then(JsonValue::as_u64),
            Some(512)
        );
    }

    #[test]
    fn gateway_video_model_generates_through_generate_video() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "result",
                        "videos": [
                            {
                                "type": "base64",
                                "data": "AAAAIGZ0eXBtcDQy",
                                "mediaType": "video/mp4"
                            }
                        ],
                        "warnings": [
                            {
                                "type": "compatibility",
                                "feature": "resolution",
                                "details": "Resolution was adjusted."
                            }
                        ],
                        "providerMetadata": {
                            "gateway": {
                                "routing": {
                                    "provider": "google"
                                },
                                "generationId": "gen_video_123"
                            }
                        }
                    })
                ),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "video_req".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token")
                .with_header("x-provider", "provider-value"),
        )
        .with_transport(transport)
        .video_model("google/veo-2.0-generate-001");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "google": {
                "enhancePrompt": true
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_video(
            GenerateVideoOptions::new(&model, "A tiny animation")
                .with_n(1)
                .with_aspect_ratio("16:9")
                .with_resolution("1280x720")
                .with_duration(5.0)
                .with_fps(24.0)
                .with_seed(42)
                .with_provider_options(provider_options)
                .with_header("Custom-Header", "test-value"),
        ))
        .expect("video generation succeeds");

        assert_eq!(result.video.media_type(), "video/mp4");
        assert_eq!(result.video.base64(), "AAAAIGZ0eXBtcDQy");
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result
                .provider_metadata
                .get("gateway")
                .and_then(|metadata| metadata.get("generationId"))
                .and_then(JsonValue::as_str),
            Some("gen_video_123")
        );
        assert_eq!(
            result
                .responses
                .first()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("video_req")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/video-model");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-video-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request.headers.get("ai-model-id").map(String::as_str),
            Some("google/veo-2.0-generate-001")
        );
        assert_eq!(
            request.headers.get("accept").map(String::as_str),
            Some("text/event-stream")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("test-value")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("provider-value")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("prompt").and_then(JsonValue::as_str),
            Some("A tiny animation")
        );
        assert_eq!(request_body.get("n").and_then(JsonValue::as_u64), Some(1));
        assert_eq!(
            request_body.get("aspectRatio").and_then(JsonValue::as_str),
            Some("16:9")
        );
        assert_eq!(
            request_body.get("resolution").and_then(JsonValue::as_str),
            Some("1280x720")
        );
        assert_eq!(
            request_body.get("duration").and_then(JsonValue::as_f64),
            Some(5.0)
        );
        assert_eq!(
            request_body.get("fps").and_then(JsonValue::as_f64),
            Some(24.0)
        );
        assert_eq!(
            request_body.get("seed").and_then(JsonValue::as_u64),
            Some(42)
        );
        assert_eq!(
            request_body
                .pointer("/providerOptions/google/enhancePrompt")
                .and_then(JsonValue::as_bool),
            Some(true)
        );
    }
    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI model call"]
    fn live_gateway_openai_generate_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-gateway-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(16)
            .with_temperature(0.0),
        ));

        assert!(
            result.text.to_lowercase().contains("rust-gateway-ok"),
            "gateway response did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI object call"]
    fn live_gateway_openai_generate_object() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway object test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let object_schema = json_object(json!({
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
        }));
        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return only a JSON object with marker exactly \"rust-gateway-object-ok\" and count exactly 7.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(64)
            .with_temperature(0.0),
        ))
        .expect("gateway object generation succeeds");

        assert_eq!(
            result.object.get("marker").and_then(JsonValue::as_str),
            Some("rust-gateway-object-ok")
        );
        assert_eq!(
            result.object.get("count").and_then(JsonValue::as_i64),
            Some(7)
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI model stream call"]
    fn live_gateway_openai_stream_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway stream test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-gateway-stream-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(20)
            .with_temperature(0.0),
        ));

        assert!(
            result
                .text
                .to_lowercase()
                .contains("rust-gateway-stream-ok"),
            "gateway stream response did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI stream object call"]
    fn live_gateway_openai_stream_object() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway stream object test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let object_schema = json_object(json!({
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
        }));
        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt(
                    "Return only a JSON object with marker exactly \"rust-gateway-stream-object-ok\" and count exactly 8.",
                ),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema))
            .with_max_output_tokens(64)
            .with_temperature(0.0),
        ));

        assert_eq!(result.error, None);
        let object = result.object.expect("gateway stream object is generated");
        assert_eq!(
            object.get("marker").and_then(JsonValue::as_str),
            Some("rust-gateway-stream-object-ok")
        );
        assert_eq!(object.get("count").and_then(JsonValue::as_i64), Some(8));
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI embedding call"]
    fn live_gateway_openai_embed() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway embedding test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_EMBEDDING_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_EMBEDDING_MODEL"))
            .unwrap_or_else(|_| "openai/text-embedding-3-small".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .embedding_model(model_id);
        let result = poll_ready(embed(EmbedOptions::new(
            &model,
            "rust gateway embedding ok",
        )));

        assert!(
            !result.embedding.is_empty(),
            "gateway embedding response was empty"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI image call"]
    fn live_gateway_openai_generate_image() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway image test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_IMAGE_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_IMAGE_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-image-1".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .image_model(model_id);
        let result = poll_ready(generate_image(GenerateImageOptions::new(
            &model,
            "A small plain rust-colored square on a white background",
        )))
        .expect("gateway image generation succeeds");

        assert!(
            !result.image.base64().is_empty(),
            "gateway image response was empty"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live reranking call"]
    fn live_gateway_rerank() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway reranking test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_RERANKING_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_RERANKING_MODEL"))
            .unwrap_or_else(|_| "cohere/rerank-v4-fast".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .reranking_model(model_id);
        let result = poll_ready(rerank(RerankOptions::new(
            &model,
            RerankDocuments::text([
                "Paris is the capital of France.",
                "Berlin is the capital of Germany.",
                "Madrid is the capital of Spain.",
            ]),
            "What is the capital of France?",
        )));

        assert!(
            !result.ranking.is_empty(),
            "gateway reranking response was empty"
        );
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
}
