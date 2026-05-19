pub use ai_sdk_open_responses::open_responses::*;

#[cfg(test)]
mod tests {
    use super::{
        OpenResponsesProvider, OpenResponsesProviderSettings, OpenResponsesTransport,
        OpenResponsesTransportFuture, create_open_responses,
    };
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::generate_object::{GenerateObjectOptions, generate_object};
    use crate::generate_text::{GenerateTextInclude, GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelCustomPart, LanguageModelFilePart, LanguageModelFunctionTool,
        LanguageModelMessage, LanguageModelProviderTool, LanguageModelReasoningEffort,
        LanguageModelReasoningPart, LanguageModelResponseFormat, LanguageModelSource,
        LanguageModelStreamPart, LanguageModelTextPart, LanguageModelTool,
        LanguageModelToolApprovalRequestPart, LanguageModelToolApprovalResponsePart,
        LanguageModelToolCallPart, LanguageModelToolChoice, LanguageModelToolContentPart,
        LanguageModelToolMessage, LanguageModelToolResultContentPart,
        LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderMetadata, ProviderOptions};
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

    fn openai_metadata_value<'a>(
        provider_metadata: &'a Option<ProviderMetadata>,
        key: &str,
    ) -> Option<&'a JsonValue> {
        provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("openai"))
            .and_then(|metadata| metadata.get(key))
    }

    fn open_responses_test_shell_tool() -> LanguageModelTool {
        let mut args = JsonObject::new();
        args.insert(
            "environment".to_string(),
            json!({
                "type": "containerAuto"
            }),
        );
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.shell",
            "shell",
            args,
        ))
    }

    fn open_responses_test_local_shell_tool() -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.local_shell",
            "local_shell",
            JsonObject::new(),
        ))
    }

    fn open_responses_test_apply_patch_tool() -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.apply_patch",
            "apply_patch",
            JsonObject::new(),
        ))
    }

    fn open_responses_test_custom_tool() -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.custom",
            "write_sql",
            JsonObject::new(),
        ))
    }

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
    fn open_responses_provider_converts_tool_approval_responses_to_mcp_input() {
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
                        "id": "resp_approval",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Approval recorded"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
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
        let approval_provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "approvalId": "approval_1"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval_1", false)
                        .with_reason("policy block"),
                ),
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval_1", false),
                ),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "mcp_call_1",
                    "mcp.deploy",
                    LanguageModelToolResultOutput::execution_denied()
                        .with_reason("policy block")
                        .with_provider_options(approval_provider_options),
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
        assert_eq!(result.content.len(), 1);

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
                        "type": "item_reference",
                        "id": "approval_1"
                    },
                    {
                        "type": "mcp_approval_response",
                        "approval_request_id": "approval_1",
                        "approve": false
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_aliases_mcp_calls_from_prompt_approval_metadata() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_mcp_approval_alias",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "mcp_call_after_approval",
                                "type": "mcp_call",
                                "status": "completed",
                                "approval_request_id": "approval_1",
                                "arguments": "{\"target\":\"prod\"}",
                                "name": "deploy",
                                "server_label": "deployments",
                                "output": "{\"deployed\":true}"
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 5
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
        let approval_metadata: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "approvalRequestId": "approval_1"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "pending_tool_call_1",
                        "mcp.deploy",
                        json!({
                            "target": "prod"
                        }),
                    )
                    .with_provider_executed(true)
                    .with_provider_options(approval_metadata),
                ),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new(
                        "approval_1",
                        "pending_tool_call_1",
                    ),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval_1", true),
                ),
            ])),
        ])));

        let tool_calls = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_calls[0].tool_call_id, "pending_tool_call_1");
        assert_eq!(tool_calls[0].tool_name, "mcp.deploy");
        assert_eq!(tool_results[0].tool_call_id, "pending_tool_call_1");
        assert_eq!(tool_results[0].tool_name, "mcp.deploy");
    }

    #[test]
    fn open_responses_provider_uses_item_references_for_stored_assistant_history() {
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
                        "id": "resp_refs",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "References accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
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
        let item_options = |item_id: &str| -> ProviderOptions {
            serde_json::from_value(json!({
                "openai": {
                    "itemId": item_id
                }
            }))
            .expect("provider options deserialize")
        };

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(
                    LanguageModelTextPart::new("stored text")
                        .with_provider_options(item_options("message_item")),
                ),
                LanguageModelAssistantContentPart::Reasoning(
                    LanguageModelReasoningPart::new("stored reasoning")
                        .with_provider_options(item_options("reasoning_item")),
                ),
                LanguageModelAssistantContentPart::Custom(
                    LanguageModelCustomPart::new("openai.compaction")
                        .with_provider_options(item_options("compaction_item")),
                ),
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "provider_call_1",
                        "mcp.lookup",
                        json!({
                            "query": "rust"
                        }),
                    )
                    .with_provider_executed(true)
                    .with_provider_options(item_options("mcp_call_item")),
                ),
                LanguageModelAssistantContentPart::ToolResult(
                    LanguageModelToolResultPart::new(
                        "provider_call_1",
                        "mcp.lookup",
                        LanguageModelToolResultOutput::json(json!({
                            "answer": "ok"
                        })),
                    )
                    .with_provider_options(item_options("mcp_result_item")),
                ),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                        "type": "item_reference",
                        "id": "message_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "reasoning_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "compaction_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "mcp_call_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "mcp_result_item"
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_reasoning_history_with_store_false() {
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
                        "id": "resp_reasoning_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Reasoning accepted"
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
        let reasoning_options =
            |item_id: Option<&str>, encrypted_content: Option<&str>| -> ProviderOptions {
                let mut openai = JsonObject::new();
                if let Some(item_id) = item_id {
                    openai.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));
                }
                if let Some(encrypted_content) = encrypted_content {
                    openai.insert(
                        "reasoningEncryptedContent".to_string(),
                        JsonValue::String(encrypted_content.to_string()),
                    );
                }

                let mut options = ProviderOptions::new();
                options.insert("openai".to_string(), openai);
                options
            };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible before reasoning",
                        )),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("First reasoning step")
                                .with_provider_options(reasoning_options(
                                    Some("reasoning_001"),
                                    None,
                                )),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Second reasoning step")
                                .with_provider_options(reasoning_options(
                                    Some("reasoning_001"),
                                    Some("encrypted_content_001"),
                                )),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Reasoning without item id")
                                .with_provider_options(reasoning_options(
                                    None,
                                    Some("encrypted_without_id"),
                                )),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible after reasoning",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible before reasoning"
                            }
                        ]
                    },
                    {
                        "type": "reasoning",
                        "id": "reasoning_001",
                        "encrypted_content": "encrypted_content_001",
                        "summary": [
                            {
                                "type": "summary_text",
                                "text": "First reasoning step"
                            },
                            {
                                "type": "summary_text",
                                "text": "Second reasoning step"
                            }
                        ]
                    },
                    {
                        "type": "reasoning",
                        "encrypted_content": "encrypted_without_id",
                        "summary": [
                            {
                                "type": "summary_text",
                                "text": "Reasoning without item id"
                            }
                        ]
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible after reasoning"
                            }
                        ]
                    }
                ],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_compaction_history_with_store_false() {
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
                        "id": "resp_compaction_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Compaction accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 2
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
        let compaction_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "compaction_001",
                "encryptedContent": "encrypted_compaction"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible before compaction",
                        )),
                        LanguageModelAssistantContentPart::Custom(
                            LanguageModelCustomPart::new("openai.compaction")
                                .with_provider_options(compaction_options),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible after compaction",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible before compaction"
                            }
                        ]
                    },
                    {
                        "type": "compaction",
                        "id": "compaction_001",
                        "encrypted_content": "encrypted_compaction"
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible after compaction"
                            }
                        ]
                    }
                ],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_text_item_id_and_phase_with_store_false() {
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
                        "id": "resp_text_phase",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Text history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
                            "output_tokens": 2
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
        let text_options = |item_id: &str, phase: Option<&str>| -> ProviderOptions {
            let mut openai = JsonObject::new();
            openai.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));
            if let Some(phase) = phase {
                openai.insert("phase".to_string(), JsonValue::String(phase.to_string()));
            }

            let mut options = ProviderOptions::new();
            options.insert("openai".to_string(), openai);
            options
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("I will search for that")
                                .with_provider_options(text_options("msg_001", Some("commentary"))),
                        ),
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("The capital of France is Paris.")
                                .with_provider_options(text_options(
                                    "msg_002",
                                    Some("final_answer"),
                                )),
                        ),
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("No phase")
                                .with_provider_options(text_options("msg_003", None)),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "I will search for that"
                            }
                        ],
                        "id": "msg_001",
                        "phase": "commentary"
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "The capital of France is Paris."
                            }
                        ],
                        "id": "msg_002",
                        "phase": "final_answer"
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "No phase"
                            }
                        ],
                        "id": "msg_003"
                    }
                ],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_warns_for_unstored_reasoning_without_encrypted_content() {
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
                        "id": "resp_reasoning_warning",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Reasoning warning accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 1,
                            "output_tokens": 2
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "reasoning_without_encryption"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Reasoning without encrypted content")
                                .with_provider_options(item_options),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Reasoning without provider options"),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                crate::warning::Warning::Other { message }
                    if message == "Reasoning parts without encrypted content are not supported when store is false. Skipping reasoning parts."
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                crate::warning::Warning::Other { message }
                    if message.starts_with("Non-OpenAI reasoning parts are not supported.")
            )
        }));
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
                "input": [],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_skips_conversation_history_items() {
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
                        "id": "resp_conversation",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Conversation accepted"
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
        let item_options = |item_id: &str| -> ProviderOptions {
            serde_json::from_value(json!({
                "openai": {
                    "itemId": item_id
                }
            }))
            .expect("provider options deserialize")
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "conversation": "conv_123",
                "previousResponseId": "resp_previous"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                    ])),
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("Stored text")
                                .with_provider_options(item_options("message_existing")),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Stored reasoning")
                                .with_provider_options(item_options("reasoning_existing")),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_weather",
                                "get_weather",
                                json!({
                                    "location": "San Francisco"
                                }),
                            )
                            .with_provider_options(item_options("call_existing")),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Fresh assistant text",
                        )),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_weather",
                            "get_weather",
                            LanguageModelToolResultOutput::json(json!({
                                "temp": 72
                            })),
                        )),
                    ])),
                ])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            result.warnings.first(),
            Some(crate::warning::Warning::Unsupported { feature, details })
                if feature == "conversation"
                    && details.as_deref()
                        == Some("conversation and previousResponseId cannot be used together")
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

        assert_eq!(request_body["conversation"], "conv_123");
        assert_eq!(request_body["previous_response_id"], "resp_previous");
        assert_eq!(
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "Hello"
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Fresh assistant text"
                        }
                    ]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_weather",
                    "output": "{\"temp\":72}"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_hosted_tool_search_history_with_store_false() {
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
                        "id": "resp_tool_search_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Tool search accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
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
        let item_options = |item_id: &str| -> ProviderOptions {
            serde_json::from_value(json!({
                "openai": {
                    "itemId": item_id
                }
            }))
            .expect("provider options deserialize")
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "tsc_hosted_123",
                                "tool_search",
                                JsonValue::String(
                                    json!({
                                        "arguments": {
                                            "paths": ["get_weather"]
                                        },
                                        "call_id": null
                                    })
                                    .to_string(),
                                ),
                            )
                            .with_provider_executed(true)
                            .with_provider_options(item_options("tsc_hosted_123")),
                        ),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "tsc_hosted_123",
                                "tool_search",
                                LanguageModelToolResultOutput::json(json!({
                                    "tools": [
                                        {
                                            "type": "function",
                                            "name": "get_weather",
                                            "defer_loading": true
                                        }
                                    ]
                                })),
                            )
                            .with_provider_options(item_options("tso_hosted_456")),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "tool_search_call",
                    "id": "tsc_hosted_123",
                    "execution": "server",
                    "call_id": null,
                    "status": "completed",
                    "arguments": {
                        "paths": ["get_weather"]
                    }
                },
                {
                    "type": "tool_search_output",
                    "id": "tso_hosted_456",
                    "execution": "server",
                    "call_id": null,
                    "status": "completed",
                    "tools": [
                        {
                            "type": "function",
                            "name": "get_weather",
                            "defer_loading": true
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_client_tool_search_output_with_store_false() {
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
                        "id": "resp_client_tool_search_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Client tool search accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 11,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "tsc_client_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_abc123",
                                "tool_search",
                                JsonValue::String(
                                    json!({
                                        "arguments": {
                                            "goal": "Find weather tools"
                                        },
                                        "call_id": "call_abc123"
                                    })
                                    .to_string(),
                                ),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_abc123",
                            "tool_search",
                            LanguageModelToolResultOutput::json(json!({
                                "tools": [
                                    {
                                        "type": "function",
                                        "name": "get_weather",
                                        "description": "Get weather",
                                        "defer_loading": true,
                                        "parameters": {
                                            "type": "object",
                                            "properties": {
                                                "location": {
                                                    "type": "string"
                                                }
                                            },
                                            "required": ["location"]
                                        }
                                    }
                                ]
                            })),
                        )),
                    ])),
                ])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "tool_search_call",
                    "id": "tsc_client_1",
                    "execution": "client",
                    "call_id": "call_abc123",
                    "status": "completed",
                    "arguments": {
                        "goal": "Find weather tools"
                    }
                },
                {
                    "type": "tool_search_output",
                    "execution": "client",
                    "call_id": "call_abc123",
                    "status": "completed",
                    "tools": [
                        {
                            "type": "function",
                            "name": "get_weather",
                            "description": "Get weather",
                            "defer_loading": true,
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "location": {
                                        "type": "string"
                                    }
                                },
                                "required": ["location"]
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_warns_for_unstored_hosted_tool_results() {
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
                        "id": "resp_web_search_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
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
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Let me search.",
                        )),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "ws_123",
                                "web_search",
                                json!({
                                    "query": "Rust AI SDK"
                                }),
                            )
                            .with_provider_executed(true),
                        ),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "ws_123",
                                "web_search",
                                LanguageModelToolResultOutput::json(json!({
                                    "sources": [
                                        {
                                            "type": "url",
                                            "url": "https://example.test"
                                        }
                                    ]
                                })),
                            ),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Search complete.",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![crate::warning::Warning::Other {
                message:
                    "Results for OpenAI tool web_search are not sent to the API when store is false"
                        .to_string()
            }]
        );
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
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Let me search."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Search complete."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_skips_assistant_execution_denied_tool_results() {
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
                        "id": "resp_denied_tool_results",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Denied results skipped"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
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
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "I need approval before running the first tool.",
                        )),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "ws_denied_direct",
                                "web_search",
                                LanguageModelToolResultOutput::execution_denied()
                                    .with_reason("User denied the tool execution"),
                            ),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "The first tool was not run.",
                        )),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "ws_denied_json",
                                "web_search",
                                LanguageModelToolResultOutput::json(json!({
                                    "type": "execution-denied",
                                    "reason": "User denied the tool execution"
                                })),
                            ),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "The second tool was not run.",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "I need approval before running the first tool."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "The first tool was not run."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "The second tool was not run."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_local_shell_history_with_store_false() {
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
                        "id": "resp_local_shell_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Local shell history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "local_shell_item_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_local_shell_1",
                                "local_shell",
                                json!({
                                    "action": {
                                        "type": "exec",
                                        "command": ["ls"],
                                        "timeoutMs": 1000,
                                        "user": "builder",
                                        "workingDirectory": "/tmp/work",
                                        "env": {
                                            "RUST_LOG": "debug"
                                        }
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_local_shell_1",
                            "local_shell",
                            LanguageModelToolResultOutput::json(json!({
                                "output": "example output"
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_local_shell_tool())
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "local_shell_call",
                    "call_id": "call_local_shell_1",
                    "id": "local_shell_item_1",
                    "action": {
                        "type": "exec",
                        "command": ["ls"],
                        "timeout_ms": 1000,
                        "user": "builder",
                        "working_directory": "/tmp/work",
                        "env": {
                            "RUST_LOG": "debug"
                        }
                    }
                },
                {
                    "type": "local_shell_call_output",
                    "call_id": "call_local_shell_1",
                    "output": "example output"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_shell_history_with_store_false() {
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
                        "id": "resp_shell_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Shell history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "shell_item_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_shell_1",
                                "shell",
                                json!({
                                    "action": {
                                        "commands": ["ls -la"],
                                        "timeoutMs": 1000,
                                        "maxOutputLength": 2000
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_shell_1",
                            "shell",
                            LanguageModelToolResultOutput::json(json!({
                                "output": [
                                    {
                                        "stdout": "ok\n",
                                        "stderr": "",
                                        "outcome": {
                                            "type": "exit",
                                            "exitCode": 0
                                        }
                                    }
                                ]
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_shell_tool())
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "shell_call",
                    "call_id": "call_shell_1",
                    "id": "shell_item_1",
                    "status": "completed",
                    "action": {
                        "commands": ["ls -la"],
                        "timeout_ms": 1000,
                        "max_output_length": 2000
                    }
                },
                {
                    "type": "shell_call_output",
                    "call_id": "call_shell_1",
                    "output": [
                        {
                            "stdout": "ok\n",
                            "stderr": "",
                            "outcome": {
                                "type": "exit",
                                "exit_code": 0
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_stored_assistant_shell_outputs() {
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
                        "id": "resp_assistant_shell_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Stored shell history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "shell_output_item"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "call_shell_stored",
                                "shell",
                                LanguageModelToolResultOutput::json(json!({
                                    "output": [
                                        {
                                            "stdout": "",
                                            "stderr": "timed out",
                                            "outcome": {
                                                "type": "timeout"
                                            }
                                        }
                                    ]
                                })),
                            )
                            .with_provider_options(item_options),
                        ),
                    ]),
                )])
                .with_tool(open_responses_test_shell_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "shell_call_output",
                    "call_id": "call_shell_stored",
                    "output": [
                        {
                            "stdout": "",
                            "stderr": "timed out",
                            "outcome": {
                                "type": "timeout"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_apply_patch_history_with_store_false() {
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
                        "id": "resp_apply_patch_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Apply patch history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 11,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "apply_patch_item_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_apply_patch_1",
                                "apply_patch",
                                json!({
                                    "callId": "call_apply_patch_1",
                                    "operation": {
                                        "type": "create_file",
                                        "path": "index.html",
                                        "diff": "+<!doctype html>\n+<html></html>"
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_apply_patch_1",
                            "apply_patch",
                            LanguageModelToolResultOutput::json(json!({
                                "status": "completed",
                                "output": "Created index.html"
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_apply_patch_tool())
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "apply_patch_call",
                    "call_id": "call_apply_patch_1",
                    "id": "apply_patch_item_1",
                    "status": "completed",
                    "operation": {
                        "type": "create_file",
                        "path": "index.html",
                        "diff": "+<!doctype html>\n+<html></html>"
                    }
                },
                {
                    "type": "apply_patch_call_output",
                    "call_id": "call_apply_patch_1",
                    "status": "completed",
                    "output": "Created index.html"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_stored_apply_patch_outputs() {
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
                        "id": "resp_stored_apply_patch_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Stored apply patch history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "apply_patch_item_2"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_apply_patch_2",
                                "apply_patch",
                                json!({
                                    "callId": "call_apply_patch_2",
                                    "operation": {
                                        "type": "delete_file",
                                        "path": "temp.txt"
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_apply_patch_2",
                            "apply_patch",
                            LanguageModelToolResultOutput::json(json!({
                                "status": "incomplete",
                                "output": "Deletion denied"
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_apply_patch_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "item_reference",
                    "id": "apply_patch_item_2"
                },
                {
                    "type": "apply_patch_call_output",
                    "call_id": "call_apply_patch_2",
                    "status": "incomplete",
                    "output": "Deletion denied"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_custom_tool_calls() {
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
                        "id": "resp_custom_tool_calls",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Custom tool calls accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "custom_tool_item_3"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_custom_1",
                                "write_sql",
                                JsonValue::String("SELECT * FROM users".to_string()),
                            ),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_custom_2",
                                "write_sql",
                                json!({
                                    "query": "SELECT 1"
                                }),
                            ),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_custom_3",
                                "write_sql",
                                JsonValue::String("SELECT stored".to_string()),
                            )
                            .with_provider_options(item_options),
                        ),
                    ]),
                )])
                .with_tool(open_responses_test_custom_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom_1",
                    "name": "write_sql",
                    "input": "SELECT * FROM users"
                },
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom_2",
                    "name": "write_sql",
                    "input": "{\"query\":\"SELECT 1\"}"
                },
                {
                    "type": "item_reference",
                    "id": "custom_tool_item_3"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_custom_tool_outputs() {
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
                        "id": "resp_custom_tool_outputs",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Custom tool outputs accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 15,
                            "output_tokens": 5
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Tool(
                    LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_text",
                            "write_sql",
                            LanguageModelToolResultOutput::text("Query executed successfully."),
                        )),
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_json",
                            "write_sql",
                            LanguageModelToolResultOutput::json(json!({
                                "rows": [1, 2]
                            })),
                        )),
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_denied",
                            "write_sql",
                            LanguageModelToolResultOutput::execution_denied()
                                .with_reason("User denied the tool execution"),
                        )),
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_content",
                            "write_sql",
                            LanguageModelToolResultOutput::content(vec![
                                LanguageModelToolResultContentPart::Text(
                                    LanguageModelTextPart::new("Here is the file:"),
                                ),
                                LanguageModelToolResultContentPart::File(
                                    LanguageModelFilePart::new(
                                        FileData::Url {
                                            url: Url::parse("https://example.com/test.pdf")
                                                .expect("valid URL"),
                                        },
                                        "application/pdf",
                                    ),
                                ),
                            ]),
                        )),
                    ]),
                )])
                .with_tool(open_responses_test_custom_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_text",
                    "output": "Query executed successfully."
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_json",
                    "output": "{\"rows\":[1,2]}"
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_denied",
                    "output": "User denied the tool execution"
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_content",
                    "output": [
                        {
                            "type": "input_text",
                            "text": "Here is the file:"
                        },
                        {
                            "type": "input_file",
                            "file_url": "https://example.com/test.pdf"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_stringifies_assistant_function_call_arguments() {
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
                        "id": "resp_tool_args",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Arguments accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
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

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                    "Checking tools",
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_object",
                    "get_weather",
                    json!({
                        "location": "Brisbane"
                    }),
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_string",
                    "get_weather",
                    JsonValue::String("{\"location\":\"Berlin\"}".to_string()),
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_null",
                    "get_weather",
                    JsonValue::Null,
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Checking tools"
                            }
                        ]
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_object",
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Brisbane\"}"
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_string",
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Berlin\"}"
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_null",
                        "name": "get_weather",
                        "arguments": "{}"
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_maps_reasoning_effort_and_summary_options() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_reasoning",
                        "created_at": 1711115037,
                        "model": "gemma-7b-it",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Reasoning accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new(
                "lmstudio",
                "https://api.lmstudio.test/v1/responses",
            )
            .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gemma-7b-it");
        let prompt = || {
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Hello"),
                )],
            ))]
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "lmstudio": {
                "reasoningSummary": "auto",
                "store": false,
                "metadata": {
                    "trace": "ignored"
                }
            }
        }))
        .expect("provider options deserialize");

        let minimal_result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt())
                    .with_reasoning(LanguageModelReasoningEffort::Minimal)
                    .with_provider_options(provider_options),
            ),
        );
        let none_result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt())
                    .with_reasoning(LanguageModelReasoningEffort::None),
            ),
        );
        let default_result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt())
                    .with_reasoning(LanguageModelReasoningEffort::ProviderDefault),
            ),
        );

        assert_eq!(minimal_result.warnings.len(), 1);
        assert!(matches!(
            minimal_result.warnings.first(),
            Some(crate::warning::Warning::Compatibility { feature, details })
                if feature == "reasoning"
                    && details.as_deref() == Some(
                        "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                    )
        ));
        assert!(none_result.warnings.is_empty());
        assert!(default_result.warnings.is_empty());

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        let bodies = requests
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
            bodies[0]["reasoning"],
            json!({
                "effort": "low",
                "summary": "auto"
            })
        );
        assert!(bodies[0].get("reasoningSummary").is_none());
        assert!(bodies[0].get("store").is_none());
        assert!(bodies[0].get("metadata").is_none());
        assert_eq!(
            bodies[1]["reasoning"],
            json!({
                "effort": "none"
            })
        );
        assert!(bodies[2].get("reasoning").is_none());
    }

    #[test]
    fn open_responses_provider_maps_openai_responses_provider_options_to_request_body() {
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
                        "id": "resp_openai_options",
                        "created_at": 1711115037,
                        "model": "gpt-5.1",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "{\"answer\":\"mapped\"}"
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
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-5.1");
        let response_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                }
            },
            "required": ["answer"]
        }))
        .expect("schema deserializes");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "previousResponseId": "resp_prev",
                "maxToolCalls": 3,
                "parallelToolCalls": false,
                "promptCacheKey": "cache-key",
                "promptCacheRetention": "24h",
                "safetyIdentifier": "safe-user",
                "serviceTier": "priority",
                "textVerbosity": "low",
                "strictJsonSchema": false,
                "reasoningEffort": "high",
                "reasoningSummary": "detailed",
                "contextManagement": [
                    {
                        "type": "compaction",
                        "compactThreshold": 2048
                    }
                ],
                "logprobs": true,
                "passThroughUnsupportedFiles": true,
                "systemMessageMode": "developer",
                "forceReasoning": true,
                "caching": "auto"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return JSON"),
                    )]),
                )])
                .with_response_format(
                    LanguageModelResponseFormat::json()
                        .with_schema(response_schema.clone())
                        .with_name("response"),
                )
                .with_provider_options(provider_options)
                .with_reasoning(LanguageModelReasoningEffort::Minimal),
            ),
        );

        assert!(result.warnings.is_empty());

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

        assert_eq!(request_body["previous_response_id"], "resp_prev");
        assert_eq!(request_body["max_tool_calls"], 3);
        assert_eq!(request_body["parallel_tool_calls"], false);
        assert_eq!(request_body["prompt_cache_key"], "cache-key");
        assert_eq!(request_body["prompt_cache_retention"], "24h");
        assert_eq!(request_body["safety_identifier"], "safe-user");
        assert_eq!(request_body["service_tier"], "priority");
        assert_eq!(
            request_body["context_management"],
            json!([
                {
                    "type": "compaction",
                    "compact_threshold": 2048
                }
            ])
        );
        assert_eq!(request_body["top_logprobs"], 20);
        assert_eq!(
            request_body["include"],
            json!(["message.output_text.logprobs"])
        );
        assert_eq!(
            request_body["text"],
            json!({
                "format": {
                    "type": "json_schema",
                    "name": "response",
                    "schema": response_schema,
                    "strict": false
                },
                "verbosity": "low"
            })
        );
        assert_eq!(
            request_body["reasoning"],
            json!({
                "effort": "high",
                "summary": "detailed"
            })
        );
        assert_eq!(request_body["caching"], "auto");

        for leaked_key in [
            "previousResponseId",
            "maxToolCalls",
            "parallelToolCalls",
            "promptCacheKey",
            "promptCacheRetention",
            "safetyIdentifier",
            "serviceTier",
            "textVerbosity",
            "strictJsonSchema",
            "reasoningEffort",
            "reasoningSummary",
            "contextManagement",
            "logprobs",
            "passThroughUnsupportedFiles",
            "systemMessageMode",
            "forceReasoning",
        ] {
            assert!(
                request_body.get(leaked_key).is_none(),
                "{leaked_key} should not leak into the Open Responses request body"
            );
        }
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
    fn open_responses_provider_handles_prompt_file_defaults_and_unsupported_files() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_file_defaults",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "File defaults accepted"
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

        let rejected = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("AQIDBAU=".to_string()),
                    },
                    "text/plain",
                )),
            ])),
        ])));

        assert_eq!(rejected.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            openai_metadata_value(&rejected.provider_metadata, "errorMessage")
                .and_then(JsonValue::as_str),
            Some("file part media type text/plain")
        );
        assert!(
            captured_requests
                .lock()
                .expect("captured requests mutex is not poisoned")
                .is_empty()
        );

        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "passThroughUnsupportedFiles": true
            }
        }))
        .expect("provider options deserialize");
        let accepted = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Base64("AQIDBAU=".to_string()),
                            },
                            "application/pdf",
                        )),
                        LanguageModelUserContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Base64(
                                        "bmFtZSxyb2xlCkFkYSxlbmdpbmVlcgo=".to_string(),
                                    ),
                                },
                                "text/csv",
                            )
                            .with_filename("names.csv"),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            accepted
                .content
                .iter()
                .filter_map(|part| match part {
                    LanguageModelContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["File defaults accepted"]
        );
        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        assert_eq!(requests.len(), 1);
        let request_body = requests[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_file",
                            "filename": "part-0.pdf",
                            "file_data": "data:application/pdf;base64,AQIDBAU="
                        },
                        {
                            "type": "input_file",
                            "filename": "names.csv",
                            "file_data": "data:text/csv;base64,bmFtZSxyb2xlCkFkYSxlbmdpbmVlcgo="
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_maps_deprecated_file_id_prefixes() {
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
                        "id": "resp_file_id_prefixes",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "File ids accepted"
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
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key")
                .with_file_id_prefix("assistant-")
                .with_file_id_prefix("file-"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("assistant-img-abc123".to_string()),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("file-pdf-xyz789".to_string()),
                    },
                    "application/pdf",
                )),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
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
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_image",
                            "file_id": "assistant-img-abc123"
                        },
                        {
                            "type": "input_file",
                            "file_id": "file-pdf-xyz789"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_converts_tool_result_file_content_outputs() {
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
                        "id": "resp_tool_files",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Tool output accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 7,
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
        let image_data_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "imageDetail": "original"
            }
        }))
        .expect("provider options deserialize");
        let image_url_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "imageDetail": "high"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call_files",
                    "render_report",
                    LanguageModelToolResultOutput::content(vec![
                        LanguageModelToolResultContentPart::Text(LanguageModelTextPart::new(
                            "First result",
                        )),
                        LanguageModelToolResultContentPart::File(LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )
                        .with_provider_options(image_data_options)),
                        LanguageModelToolResultContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Url {
                                    url: Url::parse("https://example.com/photo.jpg")
                                        .expect("url parses"),
                                },
                                "image/jpeg",
                            )
                            .with_provider_options(image_url_options),
                        ),
                        LanguageModelToolResultContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Base64("JVBERi0=".to_string()),
                                },
                                "application/pdf",
                            )
                            .with_filename("report.pdf"),
                        ),
                        LanguageModelToolResultContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/report.pdf")
                                    .expect("url parses"),
                            },
                            "application/pdf",
                        )),
                    ]),
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                        "type": "function_call_output",
                        "call_id": "call_files",
                        "output": [
                            {
                                "type": "input_text",
                                "text": "First result"
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,AAECAw==",
                                "detail": "original"
                            },
                            {
                                "type": "input_image",
                                "image_url": "https://example.com/photo.jpg",
                                "detail": "high"
                            },
                            {
                                "type": "input_file",
                                "filename": "report.pdf",
                                "file_data": "data:application/pdf;base64,JVBERi0="
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
    fn open_responses_provider_resolves_top_level_image_media_types() {
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
                        "id": "resp_top_level_images",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Top-level images accepted"
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
        let png_base64 = "iVBORw0KGgo=";

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/x.png").expect("url parses"),
                    },
                    "image",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image/*",
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                                "type": "input_image",
                                "image_url": "data:image/png;base64,iVBORw0KGgo="
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,iVBORw0KGgo="
                            },
                            {
                                "type": "input_image",
                                "image_url": "https://example.com/x.png"
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,iVBORw0KGgo="
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
    fn open_responses_provider_prepares_function_tool_strict_modes() {
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
                        "id": "resp_strict_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Strict tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 7,
                            "output_tokens": 2
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
        let empty_object_schema = || {
            json_object(json!({
                "type": "object",
                "properties": {}
            }))
        };

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use strict tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("strict_tool", empty_object_schema())
                        .with_description("A strict tool")
                        .with_strict(true),
                ))
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("non_strict_tool", empty_object_schema())
                        .with_description("A non-strict tool")
                        .with_strict(false),
                ))
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("default_tool", empty_object_schema())
                        .with_description("A default tool"),
                )),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

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
                    "type": "function",
                    "name": "strict_tool",
                    "description": "A strict tool",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    },
                    "strict": true
                },
                {
                    "type": "function",
                    "name": "non_strict_tool",
                    "description": "A non-strict tool",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    },
                    "strict": false
                },
                {
                    "type": "function",
                    "name": "default_tool",
                    "description": "A default tool",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            ])
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
    fn open_responses_provider_prepares_web_search_preview_and_local_shell_tools() {
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
                        "id": "resp_preview_shell_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Preview search and local shell prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 2
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use preview search and local shell"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search_preview",
                    "previewSearch",
                    json_object(json!({
                        "searchContextSize": "medium",
                        "userLocation": {
                            "type": "approximate",
                            "country": "US",
                            "city": "Seattle",
                            "region": "Washington",
                            "timezone": "America/Los_Angeles"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.local_shell",
                    "localShell",
                    JsonObject::new(),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "previewSearch".to_string(),
                }),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
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
                    "type": "web_search_preview",
                    "search_context_size": "medium",
                    "user_location": {
                        "type": "approximate",
                        "country": "US",
                        "city": "Seattle",
                        "region": "Washington",
                        "timezone": "America/Los_Angeles"
                    }
                },
                {
                    "type": "local_shell"
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "web_search_preview"
            })
        );
    }

    #[test]
    fn open_responses_provider_prepares_code_interpreter_and_image_generation_options() {
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
                        "id": "resp_hosted_tool_options",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted tool options prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 2
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use hosted tool options"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.code_interpreter",
                    "codeRunner",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.code_interpreter",
                    "existingContainer",
                    json_object(json!({
                        "container": "container-123"
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.image_generation",
                    "imageMaker",
                    json_object(json!({
                        "background": "opaque",
                        "inputFidelity": "high",
                        "inputImageMask": {
                            "fileId": "file-mask",
                            "imageUrl": "https://example.com/mask.png"
                        },
                        "model": "gpt-image-1",
                        "moderation": "auto",
                        "partialImages": 3,
                        "quality": "high",
                        "outputCompression": 100,
                        "outputFormat": "png",
                        "size": "1536x1024"
                    })),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "imageMaker".to_string(),
                }),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

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
                    "type": "code_interpreter",
                    "container": {
                        "type": "auto"
                    }
                },
                {
                    "type": "code_interpreter",
                    "container": "container-123"
                },
                {
                    "type": "image_generation",
                    "background": "opaque",
                    "input_fidelity": "high",
                    "input_image_mask": {
                        "file_id": "file-mask",
                        "image_url": "https://example.com/mask.png"
                    },
                    "model": "gpt-image-1",
                    "moderation": "auto",
                    "partial_images": 3,
                    "quality": "high",
                    "output_compression": 100,
                    "output_format": "png",
                    "size": "1536x1024"
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "image_generation"
            })
        );
    }

    #[test]
    fn open_responses_provider_prepares_custom_tool_formats_and_choice() {
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
                        "id": "resp_custom_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Custom tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 2
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use custom tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.custom",
                    "write_sql",
                    json_object(json!({
                        "description": "Write a SQL SELECT query.",
                        "format": {
                            "type": "grammar",
                            "syntax": "regex",
                            "definition": "SELECT .+"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.custom",
                    "generate_json",
                    json_object(json!({
                        "format": {
                            "type": "grammar",
                            "syntax": "lark",
                            "definition": "start: \"{\" \"}\""
                        }
                    })),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "write_sql".to_string(),
                }),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

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
                    "type": "custom",
                    "name": "write_sql",
                    "description": "Write a SQL SELECT query.",
                    "format": {
                        "type": "grammar",
                        "syntax": "regex",
                        "definition": "SELECT .+"
                    }
                },
                {
                    "type": "custom",
                    "name": "generate_json",
                    "format": {
                        "type": "grammar",
                        "syntax": "lark",
                        "definition": "start: \"{\" \"}\""
                    }
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "custom",
                "name": "write_sql"
            })
        );
    }

    #[test]
    fn open_responses_provider_prepares_apply_patch_and_tool_search_tools() {
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
                        "id": "resp_prepared_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 2
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
        let defer_loading_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "deferLoading": true
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use prepared tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.tool_search",
                    "toolSearch",
                    json_object(json!({
                        "execution": "client",
                        "description": "Find available tools",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "goal": {
                                    "type": "string"
                                }
                            },
                            "required": ["goal"],
                            "additionalProperties": false
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.apply_patch",
                    "apply_patch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new(
                        "get_weather",
                        json_object(json!({
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string"
                                }
                            },
                            "required": ["location"],
                            "additionalProperties": false
                        })),
                    )
                    .with_description("Get the current weather")
                    .with_provider_options(defer_loading_options),
                ))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "apply_patch".to_string(),
                }),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

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
                    "type": "tool_search",
                    "execution": "client",
                    "description": "Find available tools",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "goal": {
                                "type": "string"
                            }
                        },
                        "required": ["goal"],
                        "additionalProperties": false
                    }
                },
                {
                    "type": "apply_patch"
                },
                {
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get the current weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string"
                            }
                        },
                        "required": ["location"],
                        "additionalProperties": false
                    },
                    "defer_loading": true
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "apply_patch"
            })
        );
    }

    #[test]
    fn open_responses_provider_prepares_shell_tool_environment_skills() {
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
                        "id": "resp_shell_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Shell prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 2
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use shell"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(
                    LanguageModelProviderTool::new(
                        "openai.shell",
                        "shell",
                        json_object(json!({
                            "environment": {
                                "type": "containerAuto",
                                "fileIds": ["file-1", "file-2"],
                                "memoryLimit": "16g",
                                "networkPolicy": {
                                    "type": "allowlist",
                                    "allowedDomains": ["example.com", "api.test.org"],
                                    "domainSecrets": [
                                        {
                                            "domain": "api.test.org",
                                            "name": "API_KEY",
                                            "value": "secret123"
                                        }
                                    ]
                                },
                                "skills": [
                                    {
                                        "type": "skillReference",
                                        "providerReference": {
                                            "openai": "skill_abc"
                                        },
                                        "version": "1.0.0"
                                    },
                                    {
                                        "type": "skillReference",
                                        "providerReference": {
                                            "openai": "skill_latest"
                                        }
                                    },
                                    {
                                        "type": "inline",
                                        "name": "my-skill",
                                        "description": "A test skill",
                                        "source": {
                                            "type": "base64",
                                            "mediaType": "application/zip",
                                            "data": "dGVzdA=="
                                        }
                                    }
                                ]
                            }
                        })),
                    ),
                )),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

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
                    "type": "shell",
                    "environment": {
                        "type": "container_auto",
                        "file_ids": ["file-1", "file-2"],
                        "memory_limit": "16g",
                        "network_policy": {
                            "type": "allowlist",
                            "allowed_domains": ["example.com", "api.test.org"],
                            "domain_secrets": [
                                {
                                    "domain": "api.test.org",
                                    "name": "API_KEY",
                                    "value": "secret123"
                                }
                            ]
                        },
                        "skills": [
                            {
                                "type": "skill_reference",
                                "skill_id": "skill_abc",
                                "version": "1.0.0"
                            },
                            {
                                "type": "skill_reference",
                                "skill_id": "skill_latest",
                                "version": "latest"
                            },
                            {
                                "type": "inline",
                                "name": "my-skill",
                                "description": "A test skill",
                                "source": {
                                    "type": "base64",
                                    "media_type": "application/zip",
                                    "data": "dGVzdA=="
                                }
                            }
                        ]
                    }
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_rejects_unresolved_shell_skill_reference() {
        let transport: OpenResponsesTransport = Arc::new(|_| -> OpenResponsesTransportFuture {
            panic!("transport should not be used")
        });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use shell"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(
                    LanguageModelProviderTool::new(
                        "openai.shell",
                        "shell",
                        json_object(json!({
                            "environment": {
                                "type": "containerAuto",
                                "skills": [
                                    {
                                        "type": "skillReference",
                                        "providerReference": {
                                            "anthropic": "skill_abc"
                                        }
                                    }
                                ]
                            }
                        })),
                    ),
                )),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            openai_metadata_value(&result.provider_metadata, "errorMessage"),
            Some(&json!(
                "No provider reference found for provider 'openai'. Available providers: anthropic"
            ))
        );
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref()),
            Some(&json!({ "model": "gpt-4.1-mini" }))
        );
    }

    #[test]
    fn open_responses_provider_maps_allowed_tools_to_tool_choice() {
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
                        "id": "resp_allowed_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Allowed tools accepted"
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
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "allowedTools": {
                    "toolNames": ["get_weather"]
                }
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use one allowed tool"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new(
                        "get_weather",
                        json_object(json!({
                            "type": "object",
                            "properties": {}
                        })),
                    )
                    .with_description("Get weather"),
                ))
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new(
                        "get_time",
                        json_object(json!({
                            "type": "object",
                            "properties": {}
                        })),
                    )
                    .with_description("Get time"),
                ))
                .with_tool_choice(LanguageModelToolChoice::Required)
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
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
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "type": "function",
                    "name": "get_time",
                    "description": "Get time",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "allowed_tools",
                "mode": "auto",
                "tools": [
                    {
                        "type": "function",
                        "name": "get_weather"
                    }
                ]
            })
        );
        assert!(request_body.get("allowedTools").is_none());
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
    fn open_responses_provider_maps_additional_response_tool_items() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_additional_tool_items",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "custom_item",
                                "type": "custom_tool_call",
                                "call_id": "custom_1",
                                "name": "write_sql",
                                "input": "select 1"
                            },
                            {
                                "id": "tsc_1",
                                "type": "tool_search_call",
                                "execution": "server",
                                "call_id": null,
                                "status": "completed",
                                "arguments": {
                                    "goal": "Find a weather tool"
                                }
                            },
                            {
                                "id": "tso_1",
                                "type": "tool_search_output",
                                "execution": "server",
                                "call_id": null,
                                "status": "completed",
                                "tools": [
                                    {
                                        "type": "function",
                                        "name": "get_weather"
                                    }
                                ]
                            },
                            {
                                "id": "local_shell_item",
                                "type": "local_shell_call",
                                "call_id": "local_shell_1",
                                "action": {
                                    "type": "exec",
                                    "command": ["pwd"]
                                }
                            },
                            {
                                "id": "shell_item",
                                "type": "shell_call",
                                "call_id": "shell_1",
                                "status": "completed",
                                "action": {
                                    "commands": ["echo hi"]
                                }
                            },
                            {
                                "id": "shell_output_item",
                                "type": "shell_call_output",
                                "call_id": "shell_1",
                                "status": "completed",
                                "output": [
                                    {
                                        "stdout": "hi",
                                        "stderr": "",
                                        "outcome": {
                                            "type": "exit",
                                            "exit_code": 0
                                        }
                                    },
                                    {
                                        "stdout": "",
                                        "stderr": "timed out",
                                        "outcome": {
                                            "type": "timeout"
                                        }
                                    }
                                ]
                            },
                            {
                                "id": "patch_item",
                                "type": "apply_patch_call",
                                "call_id": "patch_1",
                                "status": "completed",
                                "operation": {
                                    "type": "update_file",
                                    "path": "src/lib.rs",
                                    "diff": "@@"
                                }
                            },
                            {
                                "id": "mcp_1",
                                "type": "mcp_call",
                                "status": "completed",
                                "arguments": "{\"query\":\"rust\"}",
                                "name": "lookup",
                                "server_label": "docs",
                                "output": "{\"answer\":\"ok\"}"
                            },
                            {
                                "id": "mcp_pending_1",
                                "type": "mcp_approval_request",
                                "server_label": "deployments",
                                "name": "deploy",
                                "arguments": "{\"target\":\"prod\"}",
                                "approval_request_id": "approval_1"
                            },
                            {
                                "id": "computer_1",
                                "type": "computer_call",
                                "status": "completed"
                            },
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Additional tools mapped"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 13,
                            "output_tokens": 8
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use additional tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.tool_search",
                    "toolSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.local_shell",
                    "localShell",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.shell",
                    "hostShell",
                    json_object(json!({
                        "environment": {
                            "type": "containerAuto"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.apply_patch",
                    "patchTool",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(
                    LanguageModelProviderTool::new("openai.mcp", "mcpTool", JsonObject::new()),
                )),
            ),
        );

        assert_eq!(&result.finish_reason.unified, &FinishReason::ToolCalls);

        let tool_calls = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();
        let approvals = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolApprovalRequest(approval) => Some(approval),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 8);
        assert_eq!(tool_results.len(), 4);
        assert_eq!(approvals.len(), 1);

        assert_eq!(tool_calls[0].tool_name, "write_sql");
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[0].input)
                .expect("custom tool input parses"),
            json!("select 1")
        );
        assert_eq!(tool_calls[1].tool_name, "toolSearch");
        assert_eq!(tool_calls[1].tool_call_id, "tsc_1");
        assert_eq!(tool_calls[1].provider_executed, Some(true));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[1].input)
                .expect("tool search input parses"),
            json!({
                "arguments": {
                    "goal": "Find a weather tool"
                },
                "call_id": null
            })
        );
        assert_eq!(tool_results[0].tool_call_id, "tsc_1");
        assert_eq!(tool_results[0].tool_name, "toolSearch");
        assert_eq!(
            tool_results[0].result.as_value(),
            &json!({
                "tools": [
                    {
                        "type": "function",
                        "name": "get_weather"
                    }
                ]
            })
        );

        assert_eq!(tool_calls[2].tool_name, "localShell");
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[2].input)
                .expect("local shell input parses"),
            json!({
                "action": {
                    "type": "exec",
                    "command": ["pwd"]
                }
            })
        );
        assert_eq!(tool_calls[3].tool_name, "hostShell");
        assert_eq!(tool_calls[3].provider_executed, Some(true));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[3].input).expect("shell input parses"),
            json!({
                "action": {
                    "commands": ["echo hi"]
                }
            })
        );
        assert_eq!(tool_results[1].tool_name, "hostShell");
        assert_eq!(
            tool_results[1].result.as_value(),
            &json!({
                "output": [
                    {
                        "stdout": "hi",
                        "stderr": "",
                        "outcome": {
                            "type": "exit",
                            "exitCode": 0
                        }
                    },
                    {
                        "stdout": "",
                        "stderr": "timed out",
                        "outcome": {
                            "type": "timeout"
                        }
                    }
                ]
            })
        );

        assert_eq!(tool_calls[4].tool_name, "patchTool");
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[4].input)
                .expect("apply patch input parses"),
            json!({
                "callId": "patch_1",
                "operation": {
                    "type": "update_file",
                    "path": "src/lib.rs",
                    "diff": "@@"
                }
            })
        );
        assert_eq!(tool_calls[5].tool_name, "mcp.lookup");
        assert_eq!(tool_calls[5].provider_executed, Some(true));
        assert_eq!(tool_calls[5].dynamic, Some(true));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[5].input).expect("mcp input parses"),
            json!({
                "query": "rust"
            })
        );
        assert_eq!(tool_results[2].tool_name, "mcp.lookup");
        assert_eq!(tool_results[2].dynamic, Some(true));
        assert_eq!(
            tool_results[2].result.as_value(),
            &json!({
                "type": "call",
                "serverLabel": "docs",
                "name": "lookup",
                "arguments": "{\"query\":\"rust\"}",
                "output": "{\"answer\":\"ok\"}"
            })
        );

        assert_eq!(tool_calls[6].tool_name, "mcp.deploy");
        assert_eq!(tool_calls[6].provider_executed, Some(true));
        assert_eq!(tool_calls[6].dynamic, Some(true));
        assert_ne!(tool_calls[6].tool_call_id, "mcp_pending_1");
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_pending_1")
        );
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "approvalRequestId")
                .and_then(JsonValue::as_str),
            Some("approval_1")
        );
        assert_eq!(approvals[0].approval_id, "approval_1");
        assert_eq!(approvals[0].tool_call_id, tool_calls[6].tool_call_id);
        assert_eq!(tool_calls[7].tool_name, "computer_use");
        assert_eq!(tool_calls[7].input, "");
        assert_eq!(tool_calls[7].provider_executed, Some(true));
        assert_eq!(
            tool_results[3].result.as_value(),
            &json!({
                "type": "computer_use_tool_result",
                "status": "completed"
            })
        );
    }

    #[test]
    fn open_responses_provider_maps_text_sources_and_compaction_metadata() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_metadata_items",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "reasoning_1",
                                "type": "reasoning",
                                "encrypted_content": "encrypted-reasoning",
                                "summary": []
                            },
                            {
                                "id": "message_1",
                                "type": "message",
                                "phase": "final",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Cited answer",
                                        "annotations": [
                                            {
                                                "type": "url_citation",
                                                "url": "https://example.com/article",
                                                "title": "Example Article"
                                            },
                                            {
                                                "type": "file_citation",
                                                "file_id": "file_123",
                                                "filename": "guide.md",
                                                "index": 7
                                            },
                                            {
                                                "type": "container_file_citation",
                                                "container_id": "container_123",
                                                "file_id": "cfile_123",
                                                "filename": "results.csv"
                                            },
                                            {
                                                "type": "file_path",
                                                "file_id": "path_file_123",
                                                "index": 3
                                            }
                                        ]
                                    }
                                ]
                            },
                            {
                                "id": "compaction_1",
                                "type": "compaction",
                                "encrypted_content": "encrypted-context"
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

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use sources")),
            ])),
        ])));

        assert_eq!(&result.finish_reason.unified, &FinishReason::Stop);
        assert!(matches!(
            &result.content[0],
            LanguageModelContent::Reasoning(reasoning)
                if reasoning.text.is_empty()
                    && reasoning
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("itemId"))
                        .and_then(JsonValue::as_str)
                        == Some("reasoning_1")
                    && reasoning
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-reasoning")
        ));
        assert!(matches!(
            &result.content[1],
            LanguageModelContent::Text(text)
                if text.text == "Cited answer"
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("itemId"))
                        .and_then(JsonValue::as_str)
                        == Some("message_1")
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("phase"))
                        .and_then(JsonValue::as_str)
                        == Some("final")
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("annotations"))
                        .and_then(JsonValue::as_array)
                        .is_some_and(|annotations| annotations.len() == 4)
        ));

        let sources = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::Source(source) => Some(source),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 4);
        assert!(matches!(
            sources[0],
            LanguageModelSource::Url(source)
                if source.id == "source-0"
                    && source.url == "https://example.com/article"
                    && source.title.as_deref() == Some("Example Article")
        ));
        assert!(matches!(
            sources[1],
            LanguageModelSource::Document(source)
                if source.id == "source-1"
                    && source.media_type == "text/plain"
                    && source.title == "guide.md"
                    && source.filename.as_deref() == Some("guide.md")
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("fileId"))
                        .and_then(JsonValue::as_str)
                        == Some("file_123")
        ));
        assert!(matches!(
            sources[2],
            LanguageModelSource::Document(source)
                if source.id == "source-2"
                    && source.media_type == "text/plain"
                    && source.title == "results.csv"
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("containerId"))
                        .and_then(JsonValue::as_str)
                        == Some("container_123")
        ));
        assert!(matches!(
            sources[3],
            LanguageModelSource::Document(source)
                if source.id == "source-3"
                    && source.media_type == "application/octet-stream"
                    && source.title == "path_file_123"
                    && source.filename.as_deref() == Some("path_file_123")
        ));
        assert!(matches!(
            result.content.last(),
            Some(LanguageModelContent::Custom(custom))
                if custom.kind == "openai.compaction"
                    && custom
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("type"))
                        .and_then(JsonValue::as_str)
                        == Some("compaction")
                    && custom
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("encryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-context")
        ));
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
    fn open_responses_provider_reports_unsupported_embedding_and_image() {
        let provider = OpenResponsesProvider::from_settings(OpenResponsesProviderSettings::new(
            "openai",
            "https://api.openai.test/v1/responses",
        ));
        let embedding = match provider.embedding_model("embedding-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        let image = match provider.image_model("image-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);

        let trait_model =
            Provider::language_model(&provider, "gpt-4.1-mini").expect("language model exists");
        assert_eq!(trait_model.provider(), "openai.responses");
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
    fn open_responses_provider_stream_failed_response_sets_raw_reason_and_usage() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_failed","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.failed","response":{"id":"resp_stream_failed","created_at":1711115037,"model":"gpt-4.1-mini","status":"failed","error":{"type":"rate_limit_error","code":"rate_limit_exceeded","message":"rate limited","param":null},"usage":{"input_tokens":6,"input_tokens_details":{"cached_tokens":2},"output_tokens":4,"output_tokens_details":{"reasoning_tokens":1}}}}"#,
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
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Say hello")),
            ])),
        ])));

        assert!(
            !result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Error(_)))
        );
        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream includes finish part");
        assert_eq!(finish.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            finish.finish_reason.raw.as_deref(),
            Some("rate_limit_exceeded")
        );
        assert_eq!(finish.usage.input_tokens.total, Some(6));
        assert_eq!(finish.usage.input_tokens.no_cache, Some(4));
        assert_eq!(finish.usage.input_tokens.cache_read, Some(2));
        assert_eq!(finish.usage.output_tokens.total, Some(4));
        assert_eq!(finish.usage.output_tokens.text, Some(3));
        assert_eq!(finish.usage.output_tokens.reasoning, Some(1));
    }

    #[test]
    fn open_responses_provider_streams_text_sources_reasoning_and_compaction_metadata() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_metadata","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"message_1","type":"message","phase":"final_answer","role":"assistant","content":[]}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"message_1","output_index":0,"content_index":0,"delta":"Cited answer"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"message_1","output_index":0,"content_index":0,"text":"Cited answer"}"#,
                    "",
                    r#"data: {"type":"response.output_text.annotation.added","item_id":"message_1","output_index":0,"content_index":0,"annotation_index":0,"annotation":{"type":"url_citation","url":"https://example.com/article","title":"Example Article"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.annotation.added","item_id":"message_1","output_index":0,"content_index":0,"annotation_index":1,"annotation":{"type":"file_citation","file_id":"file_123","filename":"guide.md","index":7}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"message_1","type":"message","phase":"final_answer","role":"assistant","content":[{"type":"output_text","text":"Cited answer"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"reasoning_1","type":"reasoning","encrypted_content":"encrypted-reasoning","summary":[]}}"#,
                    "",
                    r#"data: {"type":"response.reasoning_summary_text.delta","item_id":"reasoning_1","output_index":1,"summary_index":0,"delta":"thinking"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"reasoning_1","type":"reasoning","encrypted_content":"encrypted-reasoning","summary":[{"type":"summary_text","text":"thinking"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"compaction_1","type":"compaction","encrypted_content":"encrypted-context"}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_metadata","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":7,"output_tokens":5}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use sources")),
            ])),
        ])));

        let text_start = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::TextStart(text_start) => Some(text_start),
                _ => None,
            })
            .expect("stream includes text start");
        assert_eq!(text_start.id, "message_1");
        assert_eq!(
            text_start
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("phase"))
                .and_then(JsonValue::as_str),
            Some("final_answer")
        );

        let sources = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::Source(source) => Some(source),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 2);
        assert!(matches!(
            sources[0],
            LanguageModelSource::Url(source)
                if source.id == "source-0"
                    && source.url == "https://example.com/article"
                    && source.title.as_deref() == Some("Example Article")
        ));
        assert!(matches!(
            sources[1],
            LanguageModelSource::Document(source)
                if source.id == "source-1"
                    && source.title == "guide.md"
                    && source.filename.as_deref() == Some("guide.md")
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("fileId"))
                        .and_then(JsonValue::as_str)
                        == Some("file_123")
        ));

        let text_end = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::TextEnd(text_end) => Some(text_end),
                _ => None,
            })
            .expect("stream includes text end");
        assert_eq!(
            text_end
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("annotations"))
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(2)
        );

        let reasoning_start = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ReasoningStart(reasoning_start) => Some(reasoning_start),
                _ => None,
            })
            .expect("stream includes reasoning start");
        assert_eq!(reasoning_start.id, "reasoning_1:0");
        assert_eq!(
            reasoning_start
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
                .and_then(JsonValue::as_str),
            Some("encrypted-reasoning")
        );
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ReasoningDelta(delta) if delta.id == "reasoning_1:0" && delta.delta == "thinking"))
        );
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ReasoningEnd(end) if end.id == "reasoning_1:0"
                    && end
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-reasoning")))
        );

        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Custom(custom) if custom.kind == "openai.compaction"
                    && custom
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("encryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-context")))
        );
    }

    #[test]
    fn open_responses_provider_streams_hosted_tool_outputs() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_hosted_tools","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"ws_123","type":"web_search_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"ws_123","type":"web_search_call","status":"completed","action":{"type":"search","query":"AI SDK Rust","sources":[{"type":"url","url":"https://example.com"}]}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"fs_123","type":"file_search_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"fs_123","type":"file_search_call","status":"completed","queries":["rust sdk"],"results":[{"attributes":{"kind":"docs"},"file_id":"file_123","filename":"guide.md","score":0.91,"text":"Guide text"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":2,"item":{"id":"ci_123","type":"code_interpreter_call","status":"in_progress","container_id":"container_123"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"ci_123","type":"code_interpreter_call","status":"completed","code":"print(1)","container_id":"container_123","outputs":[{"type":"logs","logs":"1"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":3,"item":{"id":"ig_123","type":"image_generation_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":3,"item":{"id":"ig_123","type":"image_generation_call","status":"completed","result":"base64-image"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"message_1","output_index":4,"content_index":0,"delta":"Hosted tools streamed"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"message_1","output_index":4,"content_index":0,"text":"Hosted tools streamed"}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_hosted_tools","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":11,"output_tokens":7}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result =
            poll_ready(
                model.do_stream(
                    LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                        LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                            LanguageModelTextPart::new("Use hosted tools"),
                        )]),
                    )])
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.web_search",
                        "liveSearch",
                        JsonObject::new(),
                    )))
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.file_search",
                        "docSearch",
                        JsonObject::new(),
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
                ),
            );

        let tool_calls = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 4);
        assert_eq!(tool_results.len(), 4);
        assert!(
            tool_calls
                .iter()
                .all(|tool_call| tool_call.provider_executed == Some(true))
        );
        assert_eq!(tool_calls[0].tool_name, "liveSearch");
        assert_eq!(tool_results[0].tool_name, "liveSearch");
        assert_eq!(
            tool_results[0].result.as_value(),
            &json!({
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
        assert_eq!(tool_calls[1].tool_name, "docSearch");
        assert_eq!(
            tool_results[1].result.as_value(),
            &json!({
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
        assert_eq!(tool_calls[2].tool_name, "codeRunner");
        assert_eq!(
            tool_calls[2].input,
            json!({
                "code": "print(1)",
                "containerId": "container_123"
            })
            .to_string()
        );
        assert_eq!(
            tool_results[2].result.as_value(),
            &json!({
                "outputs": [
                    {
                        "type": "logs",
                        "logs": "1"
                    }
                ]
            })
        );
        assert_eq!(tool_calls[3].tool_name, "imageMaker");
        assert_eq!(
            tool_results[3].result.as_value(),
            &json!({
                "result": "base64-image"
            })
        );
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ToolInputStart(start) if start.id == "ws_123" && start.provider_executed == Some(true)))
        );
        assert_eq!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => {
                    Some(finish.finish_reason.unified.clone())
                }
                _ => None,
            }),
            Some(FinishReason::Stop)
        );
    }

    #[test]
    fn open_responses_provider_streams_tool_input_delta_refinements() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_tool_deltas","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"custom_item","type":"custom_tool_call","call_id":"custom_1","name":"sqlWriter","input":""}}"#,
                    "",
                    r#"data: {"type":"response.custom_tool_call_input.delta","output_index":0,"delta":"select "}"#,
                    "",
                    r#"data: {"type":"response.custom_tool_call_input.delta","output_index":0,"delta":"1"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"custom_item","type":"custom_tool_call","call_id":"custom_1","name":"sqlWriter","input":"select 1"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"ci_123","type":"code_interpreter_call","status":"in_progress","container_id":"container_123"}}"#,
                    "",
                    r#"data: {"type":"response.code_interpreter_call_code.delta","output_index":1,"delta":"print("}"#,
                    "",
                    r#"data: {"type":"response.code_interpreter_call_code.delta","output_index":1,"delta":"1)\n"}"#,
                    "",
                    r#"data: {"type":"response.code_interpreter_call_code.done","output_index":1,"code":"print(1)\n"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"ci_123","type":"code_interpreter_call","status":"completed","code":"print(1)\n","container_id":"container_123","outputs":[{"type":"logs","logs":"1"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":2,"item":{"id":"ig_123","type":"image_generation_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.image_generation_call.partial_image","output_index":2,"item_id":"ig_123","partial_image_b64":"partial-base64"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"ig_123","type":"image_generation_call","status":"completed","result":"final-base64"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":3,"item":{"id":"patch_1","type":"apply_patch_call","call_id":"patch_call_1","operation":{"type":"update_file","path":"README.md"}}}"#,
                    "",
                    r#"data: {"type":"response.apply_patch_call_operation_diff.delta","output_index":3,"delta":"@@\n-old\n+new\n"}"#,
                    "",
                    r#"data: {"type":"response.apply_patch_call_operation_diff.done","output_index":3,"diff":"@@\n-old\n+new\n"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":3,"item":{"id":"patch_1","type":"apply_patch_call","call_id":"patch_call_1","status":"completed","operation":{"type":"update_file","path":"README.md","diff":"@@\n-old\n+new\n"}}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_tool_deltas","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":15,"output_tokens":9}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result =
            poll_ready(
                model.do_stream(
                    LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                        LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                            LanguageModelTextPart::new("Use streaming tool deltas"),
                        )]),
                    )])
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.code_interpreter",
                        "codeRunner",
                        JsonObject::new(),
                    )))
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.image_generation",
                        "imageMaker",
                        JsonObject::new(),
                    )))
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.apply_patch",
                        "patchTool",
                        JsonObject::new(),
                    ))),
                ),
            );

        let input_deltas_for = |tool_call_id: &str| {
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ToolInputDelta(delta) if delta.id == tool_call_id => {
                        Some(delta.delta.as_str())
                    }
                    _ => None,
                })
                .fold(String::new(), |mut input, delta| {
                    input.push_str(delta);
                    input
                })
        };
        let tool_call_by_id = |tool_call_id: &str| {
            result
                .stream
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::ToolCall(tool_call)
                        if tool_call.tool_call_id == tool_call_id =>
                    {
                        Some(tool_call)
                    }
                    _ => None,
                })
                .expect("stream includes expected tool call")
        };

        assert_eq!(input_deltas_for("custom_1"), "select 1");
        assert_eq!(
            tool_call_by_id("custom_1").input,
            json!("select 1").to_string()
        );

        assert_eq!(
            input_deltas_for("ci_123"),
            r#"{"containerId":"container_123","code":"print(1)\n"}"#
        );
        assert_eq!(
            tool_call_by_id("ci_123").input,
            json!({
                "code": "print(1)\n",
                "containerId": "container_123"
            })
            .to_string()
        );
        assert_eq!(tool_call_by_id("ci_123").provider_executed, Some(true));

        let image_results = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result)
                    if tool_result.tool_call_id == "ig_123" =>
                {
                    Some(tool_result)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(image_results.len(), 2);
        assert_eq!(image_results[0].preliminary, Some(true));
        assert_eq!(
            image_results[0].result.as_value(),
            &json!({
                "result": "partial-base64"
            })
        );
        assert_eq!(image_results[1].preliminary, None);
        assert_eq!(
            image_results[1].result.as_value(),
            &json!({
                "result": "final-base64"
            })
        );

        assert_eq!(
            input_deltas_for("patch_call_1"),
            r#"{"callId":"patch_call_1","operation":{"type":"update_file","path":"README.md","diff":"@@\n-old\n+new\n"}}"#
        );
        assert_eq!(
            tool_call_by_id("patch_call_1").input,
            json!({
                "callId": "patch_call_1",
                "operation": {
                    "type": "update_file",
                    "path": "README.md",
                    "diff": "@@\n-old\n+new\n"
                }
            })
            .to_string()
        );
    }

    #[test]
    fn open_responses_provider_streams_additional_tool_items() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_extra_tools","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"custom_item","type":"custom_tool_call","call_id":"custom_1","name":"write_sql","input":"select 1"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"tsc_1","type":"tool_search_call","execution":"server","call_id":null,"status":"completed","arguments":{"goal":"Find a weather tool"}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"tso_1","type":"tool_search_output","execution":"server","call_id":null,"status":"completed","tools":[{"type":"function","name":"get_weather"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":3,"item":{"id":"local_1","type":"local_shell_call","call_id":"local_call_1","action":{"type":"exec","command":"pwd","timeout_ms":1000}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":4,"item":{"id":"shell_1","type":"shell_call","call_id":"shell_call_1","action":{"commands":["echo hi"]}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":5,"item":{"id":"shell_out_1","type":"shell_call_output","call_id":"shell_call_1","output":[{"stdout":"hi\n","stderr":"","outcome":{"type":"exit","exit_code":0}}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":6,"item":{"id":"patch_1","type":"apply_patch_call","call_id":"patch_call_1","status":"completed","operation":{"type":"update_file","path":"README.md","diff":"@@\n"}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":7,"item":{"id":"mcp_1","type":"mcp_call","server_label":"server","name":"lookup","arguments":"{\"query\":\"rust\"}","output":{"ok":true}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":8,"item":{"id":"mcp_approval_1","type":"mcp_approval_request","approval_request_id":"approval_1","name":"approve","arguments":"{}"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":9,"item":{"id":"mcp_call_after_approval","type":"mcp_call","approval_request_id":"approval_1","server_label":"server","name":"approve","arguments":"{}","output":{"approved":true}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":10,"item":{"id":"computer_1","type":"computer_call","status":"completed"}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_extra_tools","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":13,"output_tokens":8}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use additional tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.tool_search",
                    "toolSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.local_shell",
                    "localShell",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.shell",
                    "hostShell",
                    json_object(json!({
                        "environment": {
                            "type": "containerAuto"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.apply_patch",
                    "patchTool",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(
                    LanguageModelProviderTool::new("openai.mcp", "mcpTool", JsonObject::new()),
                )),
            ),
        );

        let tool_calls = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 9);
        assert_eq!(tool_results.len(), 5);
        assert_eq!(tool_calls[0].tool_call_id, "custom_1");
        assert_eq!(tool_calls[0].tool_name, "write_sql");
        assert_eq!(tool_calls[0].input, "\"select 1\"");
        assert_eq!(
            openai_metadata_value(&tool_calls[0].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("custom_item")
        );
        assert_eq!(tool_calls[1].tool_name, "toolSearch");
        assert_eq!(tool_calls[1].provider_executed, Some(true));
        assert_eq!(
            openai_metadata_value(&tool_calls[1].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("tsc_1")
        );
        assert_eq!(tool_results[0].tool_call_id, "tsc_1");
        assert_eq!(
            openai_metadata_value(&tool_results[0].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("tso_1")
        );
        assert_eq!(
            tool_results[0].result.as_value(),
            &json!({
                "tools": [
                    {
                        "type": "function",
                        "name": "get_weather"
                    }
                ]
            })
        );
        assert_eq!(tool_calls[2].tool_name, "localShell");
        assert_eq!(
            openai_metadata_value(&tool_calls[2].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("local_1")
        );
        assert_eq!(tool_calls[3].tool_name, "hostShell");
        assert_eq!(tool_calls[3].provider_executed, Some(true));
        assert_eq!(
            openai_metadata_value(&tool_calls[3].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("shell_1")
        );
        assert_eq!(
            tool_results[1].result.as_value(),
            &json!({
                "output": [
                    {
                        "stdout": "hi\n",
                        "stderr": "",
                        "outcome": {
                            "type": "exit",
                            "exitCode": 0
                        }
                    }
                ]
            })
        );
        assert_eq!(tool_calls[4].tool_name, "patchTool");
        assert_eq!(
            openai_metadata_value(&tool_calls[4].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("patch_1")
        );
        assert_eq!(tool_calls[5].tool_name, "mcp.lookup");
        assert_eq!(tool_calls[5].provider_executed, Some(true));
        assert_eq!(tool_calls[5].dynamic, Some(true));
        assert_eq!(tool_results[2].tool_name, "mcp.lookup");
        assert_eq!(tool_results[2].dynamic, Some(true));
        assert_eq!(
            openai_metadata_value(&tool_results[2].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_1")
        );
        assert_eq!(tool_calls[6].tool_name, "mcp.approve");
        assert_ne!(tool_calls[6].tool_call_id, "mcp_approval_1");
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_approval_1")
        );
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "approvalRequestId")
                .and_then(JsonValue::as_str),
            Some("approval_1")
        );
        let approval_tool_call_id = tool_calls[6].tool_call_id.clone();
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ToolApprovalRequest(approval) if approval.approval_id == "approval_1" && approval.tool_call_id == approval_tool_call_id.as_str()))
        );
        assert_eq!(tool_calls[7].tool_name, "mcp.approve");
        assert_eq!(tool_calls[7].tool_call_id, approval_tool_call_id);
        assert_eq!(tool_results[3].tool_call_id, tool_calls[7].tool_call_id);
        assert_eq!(
            openai_metadata_value(&tool_results[3].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_call_after_approval")
        );
        assert_eq!(tool_calls[8].tool_name, "computer_use");
        assert_eq!(
            tool_results[4].result.as_value(),
            &json!({
                "type": "computer_use_tool_result",
                "status": "completed"
            })
        );
        assert_eq!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => {
                    Some(finish.finish_reason.unified.clone())
                }
                _ => None,
            }),
            Some(FinishReason::ToolCalls)
        );
    }

    #[test]
    fn open_responses_streams_function_call_argument_deltas() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_tool","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":"","namespace":"weather_ns"}}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"location\""}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"location\":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":"","namespace":"weather_ns"}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_tool","created_at":1711115037,"model":"gpt-4.1-mini","output":[{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":"{\"location\":\"Brisbane\"}"}],"usage":{"input_tokens":6,"output_tokens":3}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let stream_result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Weather?")),
            ])),
        ])));

        let tool_call = stream_result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .expect("stream includes a tool call");
        assert_eq!(tool_call.tool_call_id, "call_weather");
        assert_eq!(tool_call.tool_name, "weather");
        assert_eq!(tool_call.input, r#"{"location":"Brisbane"}"#);
        assert_eq!(
            openai_metadata_value(&tool_call.provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("fc_1")
        );
        assert_eq!(
            openai_metadata_value(&tool_call.provider_metadata, "namespace")
                .and_then(JsonValue::as_str),
            Some("weather_ns")
        );
        let input_end = stream_result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ToolInputEnd(input_end) => Some(input_end),
                _ => None,
            })
            .expect("stream includes tool input end");
        assert_eq!(
            openai_metadata_value(&input_end.provider_metadata, "namespace")
                .and_then(JsonValue::as_str),
            Some("weather_ns")
        );
        assert_eq!(
            stream_result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => {
                    Some(finish.finish_reason.unified.clone())
                }
                _ => None,
            }),
            Some(FinishReason::ToolCalls)
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
