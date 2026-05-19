pub use ai_sdk_open_responses::open_responses::*;

#[cfg(test)]
mod tests {
    use super::{
        OpenResponsesProviderSettings, OpenResponsesTransport, OpenResponsesTransportFuture,
        create_open_responses,
    };
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::generate_object::{GenerateObjectOptions, generate_object};
    use crate::generate_text::{GenerateTextInclude, GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModelFilePart, LanguageModelMessage, LanguageModelProviderTool,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolChoice,
        LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::prompt::Prompt;
    use crate::provider::ProviderOptions;
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        Tool, json_schema,
    };
    use crate::stream_text::{StreamTextOptions, TextStreamPart, stream_text};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    #[test]
    fn open_responses_provider_generates_text_with_request_and_response_metadata() {
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
                        "id": "resp_open",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from Responses"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "input_tokens_details": {
                                "cached_tokens": 2
                            },
                            "output_tokens": 4,
                            "output_tokens_details": {
                                "reasoning_tokens": 1
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_open_responses".to_string(),
                )])))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false,
                "metadata": {
                    "trace": "responses-test"
                }
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "openai.responses");
        assert_eq!(model.model_id(), "gpt-4.1-mini");
        assert_eq!(result.text, "Hello from Responses");
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.input_tokens.no_cache, Some(3));
        assert_eq!(result.usage.input_tokens.cache_read, Some(2));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.text, Some(3));
        assert_eq!(result.usage.output_tokens.reasoning, Some(1));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("resp_open")
        );
        assert!(result.provider_metadata.is_none());

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/responses");
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
                "model": "gpt-4.1-mini",
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
                "temperature": 0.0,
                "store": false,
                "metadata": {
                    "trace": "responses-test"
                }
            }))
        );
    }

    #[test]
    fn open_responses_provider_converts_user_file_prompt_parts() {
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
                        "id": "resp_files",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "File prompt accepted"
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
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let provider_reference = |entries: &[(&str, &str)]| -> ProviderReference {
            ProviderReference::try_from(
                entries
                    .iter()
                    .map(|(provider, file_id)| (provider.to_string(), file_id.to_string()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .expect("provider reference is valid")
        };
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
                                url: Url::parse("https://example.com/photo.jpg")
                                    .expect("url parses"),
                            },
                            "image/jpeg",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Reference {
                                reference: provider_reference(&[
                                    ("openai", "file-img-abc123"),
                                    ("anthropic", "img-xyz"),
                                ]),
                            },
                            "image/png",
                        )),
                        LanguageModelUserContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Base64("JVBERi0=".to_string()),
                                },
                                "application/pdf",
                            )
                            .with_filename("report.pdf"),
                        ),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/report.pdf")
                                    .expect("url parses"),
                            },
                            "application/pdf",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Reference {
                                reference: provider_reference(&[
                                    ("openai", "file-pdf-xyz789"),
                                    ("google", "doc-123"),
                                ]),
                            },
                            "application/pdf",
                        )),
                    ]),
                )]),
            )
            .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "File prompt accepted");

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
                "model": "gpt-4.1-mini",
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
                                "type": "input_image",
                                "image_url": "https://example.com/photo.jpg"
                            },
                            {
                                "type": "input_image",
                                "file_id": "file-img-abc123"
                            },
                            {
                                "type": "input_file",
                                "filename": "report.pdf",
                                "file_data": "data:application/pdf;base64,JVBERi0="
                            },
                            {
                                "type": "input_file",
                                "file_url": "https://example.com/report.pdf"
                            },
                            {
                                "type": "input_file",
                                "file_id": "file-pdf-xyz789"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_generates_object_with_json_schema_response_format() {
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
                        "id": "resp_object",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "{\"answer\":\"Open Responses object\",\"count\":3}"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 6
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
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
            .with_schema_name("answer_object")
            .with_schema_description("An answer object.")
            .with_max_output_tokens(32)
            .with_temperature(0.0),
        ))
        .expect("object is generated");

        assert_eq!(
            result.object,
            json!({
                "answer": "Open Responses object",
                "count": 3
            })
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.total, Some(6));
        assert!(result.warnings.as_ref().is_none_or(Vec::is_empty));

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
        assert_eq!(request_body["model"], "gpt-4.1-mini");
        assert_eq!(request_body["max_output_tokens"], 32);
        assert_eq!(request_body["temperature"], 0.0);
        assert_eq!(
            request_body["text"]["format"],
            json!({
                "type": "json_schema",
                "name": "answer_object",
                "description": "An answer object.",
                "schema": object_schema,
                "strict": true
            })
        );
    }

    #[test]
    fn open_responses_provider_prepares_openai_hosted_tools() {
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
                        "id": "resp_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 7,
                            "output_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use hosted tools"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "liveSearch",
                    json_object(json!({
                        "externalWebAccess": true,
                        "filters": {
                            "allowedDomains": ["example.com", "docs.rs"]
                        },
                        "searchContextSize": "high",
                        "userLocation": {
                            "type": "approximate",
                            "country": "US",
                            "city": "San Francisco",
                            "region": "California",
                            "timezone": "America/Los_Angeles"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.file_search",
                    "fileSearch",
                    json_object(json!({
                        "vectorStoreIds": ["vs_123"],
                        "maxNumResults": 5,
                        "ranking": {
                            "ranker": "auto",
                            "scoreThreshold": 0.25
                        },
                        "filters": {
                            "type": "eq",
                            "key": "kind",
                            "value": "docs"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.code_interpreter",
                    "codeRunner",
                    json_object(json!({
                        "container": {
                            "fileIds": ["file_123", "file_456"]
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.custom",
                    "write_sql",
                    json_object(json!({
                        "description": "Write SQL statements.",
                        "format": {
                            "type": "grammar",
                            "syntax": "lark",
                            "definition": "start: SELECT"
                        }
                    })),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "liveSearch".to_string(),
                }),
        ));

        assert_eq!(result.text, "Hosted tools prepared");
        assert!(result.warnings.is_empty());

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

        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "web_search",
                    "external_web_access": true,
                    "filters": {
                        "allowed_domains": ["example.com", "docs.rs"]
                    },
                    "search_context_size": "high",
                    "user_location": {
                        "type": "approximate",
                        "country": "US",
                        "city": "San Francisco",
                        "region": "California",
                        "timezone": "America/Los_Angeles"
                    }
                },
                {
                    "type": "file_search",
                    "vector_store_ids": ["vs_123"],
                    "max_num_results": 5,
                    "ranking_options": {
                        "ranker": "auto",
                        "score_threshold": 0.25
                    },
                    "filters": {
                        "type": "eq",
                        "key": "kind",
                        "value": "docs"
                    }
                },
                {
                    "type": "code_interpreter",
                    "container": {
                        "type": "auto",
                        "file_ids": ["file_123", "file_456"]
                    }
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
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "web_search"
            })
        );
    }

    #[test]
    fn open_responses_provider_maps_openai_hosted_tool_outputs() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_hosted_tool_outputs",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "ws_123",
                                "type": "web_search_call",
                                "status": "completed",
                                "action": {
                                    "type": "search",
                                    "query": "AI SDK Rust",
                                    "sources": [
                                        {
                                            "type": "url",
                                            "url": "https://example.com"
                                        }
                                    ]
                                }
                            },
                            {
                                "id": "fs_123",
                                "type": "file_search_call",
                                "status": "completed",
                                "queries": ["rust sdk"],
                                "results": [
                                    {
                                        "attributes": {
                                            "kind": "docs"
                                        },
                                        "file_id": "file_123",
                                        "filename": "guide.md",
                                        "score": 0.91,
                                        "text": "Guide text"
                                    }
                                ]
                            },
                            {
                                "id": "ci_123",
                                "type": "code_interpreter_call",
                                "status": "completed",
                                "code": "print(1)",
                                "container_id": "container_123",
                                "outputs": [
                                    {
                                        "type": "logs",
                                        "logs": "1"
                                    }
                                ]
                            },
                            {
                                "id": "ig_123",
                                "type": "image_generation_call",
                                "status": "completed",
                                "result": "base64-image"
                            },
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted tools completed"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 11,
                            "output_tokens": 7
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use hosted tools"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "liveSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.file_search",
                    "docSearch",
                    json_object(json!({
                        "vectorStoreIds": ["vs_123"]
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.code_interpreter",
                    "codeRunner",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.image_generation",
                    "imageMaker",
                    JsonObject::new(),
                ))),
        ));

        assert_eq!(result.text, "Hosted tools completed");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_calls.len(), 4);
        assert_eq!(result.tool_results.len(), 4);
        assert!(
            result
                .tool_calls
                .iter()
                .all(|tool_call| tool_call.provider_executed == Some(true))
        );
        assert!(
            result
                .tool_results
                .iter()
                .all(|tool_result| tool_result.provider_executed == Some(true))
        );
        assert_eq!(result.tool_calls[0].tool_name, "liveSearch");
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_results[0].tool_name, "liveSearch");
        assert_eq!(
            result.tool_results[0].output,
            json!({
                "action": {
                    "type": "search",
                    "query": "AI SDK Rust"
                },
                "sources": [
                    {
                        "type": "url",
                        "url": "https://example.com"
                    }
                ]
            })
        );
        assert_eq!(result.tool_calls[1].tool_name, "docSearch");
        assert_eq!(
            result.tool_results[1].output,
            json!({
                "queries": ["rust sdk"],
                "results": [
                    {
                        "attributes": {
                            "kind": "docs"
                        },
                        "fileId": "file_123",
                        "filename": "guide.md",
                        "score": 0.91,
                        "text": "Guide text"
                    }
                ]
            })
        );
        assert_eq!(result.tool_calls[2].tool_name, "codeRunner");
        assert_eq!(
            result.tool_calls[2].input,
            json!({
                "code": "print(1)",
                "containerId": "container_123"
            })
        );
        assert_eq!(
            result.tool_results[2].output,
            json!({
                "outputs": [
                    {
                        "type": "logs",
                        "logs": "1"
                    }
                ]
            })
        );
        assert_eq!(result.tool_calls[3].tool_name, "imageMaker");
        assert_eq!(
            result.tool_results[3].output,
            json!({
                "result": "base64-image"
            })
        );
    }

    #[test]
    fn open_responses_provider_maps_api_error_data_to_metadata_and_response() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    429,
                    "Too Many Requests",
                    json!({
                        "error": {
                            "message": "Quota exceeded",
                            "type": "insufficient_quota",
                            "param": "model",
                            "code": "quota_exceeded"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_open_responses_error".to_string(),
                )])))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_include(GenerateTextInclude::new().with_response_body(true)),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.text, "");
        let metadata = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("openai"))
            .expect("Open Responses error metadata is present");
        assert_eq!(
            metadata.get("errorMessage").and_then(JsonValue::as_str),
            Some("Quota exceeded")
        );
        assert_eq!(
            metadata.get("errorType").and_then(JsonValue::as_str),
            Some("insufficient_quota")
        );
        assert_eq!(
            metadata.get("errorParam").and_then(JsonValue::as_str),
            Some("model")
        );
        assert_eq!(
            metadata.get("errorCode").and_then(JsonValue::as_str),
            Some("quota_exceeded")
        );
        assert_eq!(
            metadata.get("statusCode").and_then(JsonValue::as_u64),
            Some(429)
        );
        assert_eq!(
            metadata.get("isRetryable").and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_open_responses_error")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref()),
            Some(&json!({
                "error": {
                    "message": "Quota exceeded",
                    "type": "insufficient_quota",
                    "param": "model",
                    "code": "quota_exceeded"
                }
            }))
        );
    }

    #[test]
    fn open_responses_provider_streams_text_with_request_and_response_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenResponsesTransport = Arc::new(
            move |request| -> OpenResponsesTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"Hello"}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":" from Responses"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":"Hello from Responses"}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":5,"output_tokens":4}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse)
                    .with_headers(Headers::from([
                        ("content-type".to_string(), "text/event-stream".to_string()),
                        (
                            "x-request-id".to_string(),
                            "req_open_responses_stream".to_string(),
                        ),
                    ])))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_include_raw_chunks(true),
        ));

        assert_eq!(result.text, "Hello from Responses");
        assert_eq!(result.text_stream, vec!["Hello", " from Responses"]);
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.response.id.as_deref(), Some("resp_stream"));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_open_responses_stream")
        );
        assert!(result.provider_metadata.is_none());
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Raw(_)))
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/responses");
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
                "model": "gpt-4.1-mini",
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
                "temperature": 0.0,
                "stream": true
            }))
        );
    }

    #[test]
    fn open_responses_provider_preserves_stream_error_event_data() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_error","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"error","sequence_number":1,"error":{"type":"server_error","code":"server_error","message":"response failed","param":null}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse)
                    .with_headers(Headers::from([(
                        "content-type".to_string(),
                        "text/event-stream".to_string(),
                    )])))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.response.id.as_deref(), Some("resp_stream_error"));
        assert!(result.provider_metadata.is_none());
        let error = result.errors.first().expect("stream error is captured");
        assert_eq!(error.get("type").and_then(JsonValue::as_str), Some("error"));
        assert_eq!(
            error
                .get("error")
                .and_then(|error| error.get("type"))
                .and_then(JsonValue::as_str),
            Some("server_error")
        );
        assert_eq!(
            error
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(JsonValue::as_str),
            Some("server_error")
        );
        assert_eq!(
            error
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(JsonValue::as_str),
            Some("response failed")
        );
    }

    #[test]
    fn open_responses_provider_runs_generate_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let body = if call_number == 1 {
                    json!({
                        "id": "resp_tool_call",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "fc_weather",
                                "type": "function_call",
                                "call_id": "call_weather",
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
                        "id": "resp_tool_final",
                        "created_at": 1711115038,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Brisbane is sunny."
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 4
                        }
                    })
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    body.to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
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

        assert_eq!(result.text, "Brisbane is sunny.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        assert_eq!(requests.len(), 2);

        let first_body = requests[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("first request body is JSON");
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
            item.get("type").and_then(JsonValue::as_str) == Some("function_call")
                && item.get("call_id").and_then(JsonValue::as_str) == Some("call_weather")
        }));
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(JsonValue::as_str) == Some("call_weather")
                && item
                    .get("output")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|output| output.contains("\"forecast\":\"sunny\""))
        }));
    }

    fn json_object(value: JsonValue) -> JsonObject {
        serde_json::from_value(value).expect("value is a JSON object")
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
