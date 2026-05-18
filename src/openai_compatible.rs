pub use ai_sdk_openai_compatible::openai_compatible::*;

#[cfg(test)]
mod tests {
    use super::{
        OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
        OpenAICompatibleTransportFuture,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions};
    use crate::file_data::{FileData, FileDataContent};
    use crate::generate_image::{
        GenerateImageOptions, GenerateImagePromptImage, GenerateImagePromptImages, generate_image,
    };
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::image_model::{ImageModel, ImageModelCallOptions};
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelFilePart,
        LanguageModelFunctionTool, LanguageModelMessage, LanguageModelProviderTool,
        LanguageModelReasoningEffort, LanguageModelReasoningPart, LanguageModelResponseFormat,
        LanguageModelStreamPart, LanguageModelSystemMessage, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::prompt::Prompt;
    use crate::provider::ProviderOptions;
    use crate::provider_utils::{
        FormDataValue, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse, Tool,
    };
    use crate::stream_text::{StreamTextOptions, stream_text};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    #[test]
    fn openai_compatible_embedding_model_embeds_through_embed_many() {
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
                            "test-provider": {
                                "traceId": "trace-embedding"
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_embedding".to_string(),
                )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/")
                .with_api_key("test-api-key")
                .with_query_param("api-version", "2026-01-01"),
        )
        .with_transport(transport)
        .embedding_model("text-embedding-3-large");

        assert_eq!(poll_ready(model.max_embeddings_per_call()), Some(2048));
        assert!(poll_ready(model.supports_parallel_calls()));

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
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("traceId"))
                .and_then(JsonValue::as_str),
            Some("trace-embedding")
        );
        assert_eq!(
            result
                .responses
                .as_ref()
                .and_then(|responses| responses.first())
                .and_then(Option::as_ref)
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_embedding")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/embeddings?api-version=2026-01-01"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
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
                "model": "text-embedding-3-large",
                "input": ["sunny day", "rainy city"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    fn openai_compatible_embedding_model_passes_options_and_errors() {
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
                                "embedding": [0.1, 0.2]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .embedding_model("text-embedding-3-small");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai-compatible": {
                "dimensions": 64,
                "user": "user-123"
            },
            "openaiCompatible": {
                "dimensions": 32
            },
            "test-provider": {
                "user": "user-456"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["hello".to_string()])
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'openai-compatible'"
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'test-provider'"
            )
        }));
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "text-embedding-3-small",
                "input": ["hello"],
                "encoding_format": "float",
                "dimensions": 32,
                "user": "user-456"
            }))
        );

        let error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    401,
                    "Unauthorized",
                    json!({
                        "error": {
                            "message": "Invalid API key"
                        }
                    })
                    .to_string(),
                ))))
            });
        let error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(error_transport)
        .embedding_model("text-embedding-3-small");
        let error_result = poll_ready(
            error_model.do_embed(EmbeddingModelCallOptions::new(vec!["hello".to_string()])),
        );

        assert!(error_result.embeddings.is_empty());
        assert_eq!(
            error_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid API key")
        );
    }

    #[test]
    fn openai_compatible_image_model_generates_through_generate_image() {
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
                    "req_image".to_string(),
                )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/")
                .with_api_key("test-api-key")
                .with_query_param("api-version", "2026-01-01"),
        )
        .with_transport(transport)
        .image_model("dall-e-3");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "testProvider": {
                "quality": "hd",
                "user": "user-123"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_image(
            GenerateImageOptions::new(&model, "A photorealistic astronaut riding a horse")
                .with_n(2)
                .with_size("1024x1024")
                .with_provider_options(provider_options),
        ))
        .expect("image generation succeeds");

        assert_eq!(result.images.len(), 2);
        assert_eq!(
            result
                .responses
                .first()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_image")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/images/generations?api-version=2026-01-01"
        );
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
                "model": "dall-e-3",
                "prompt": "A photorealistic astronaut riding a horse",
                "n": 2,
                "size": "1024x1024",
                "quality": "hd",
                "user": "user-123",
                "response_format": "b64_json"
            }))
        );
    }

    #[test]
    fn openai_compatible_image_model_edits_with_files_and_mask() {
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
                                "b64_json": "ZWRpdGVkLWltYWdl"
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .image_model("dall-e-3");
        let result = poll_ready(generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new([GenerateImagePromptImage::bytes(vec![
                137, 80, 78, 71,
            ])])
            .with_text("Add a flamingo to the pool")
            .with_mask(GenerateImagePromptImage::bytes(vec![1, 2, 3])),
        )))
        .expect("image edit succeeds");

        assert_eq!(result.images.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.example.com/images/edits");
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("request body is form data");
        assert_eq!(
            form_data.get("model"),
            Some(&FormDataValue::text("dall-e-3"))
        );
        assert_eq!(
            form_data.get("prompt"),
            Some(&FormDataValue::text("Add a flamingo to the pool"))
        );
        assert_eq!(
            form_data.get("image"),
            Some(&FormDataValue::bytes(vec![137, 80, 78, 71]))
        );
        assert_eq!(
            form_data.get("mask"),
            Some(&FormDataValue::bytes(vec![1, 2, 3]))
        );
    }

    #[test]
    fn openai_compatible_image_model_passes_options_warnings_and_errors() {
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
                                "b64_json": "aW1hZ2U="
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "black-forest-labs",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .image_model("flux-pro");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "black-forest-labs": {
                "quality": "standard"
            },
            "blackForestLabs": {
                "quality": "hd"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A forest")
                    .with_aspect_ratio("16:9")
                    .with_seed(123)
                    .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'black-forest-labs'"
            )
        }));
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "flux-pro",
                "prompt": "A forest",
                "n": 1,
                "quality": "hd",
                "response_format": "b64_json"
            }))
        );

        let error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "Invalid image prompt"
                        }
                    })
                    .to_string(),
                ))))
            });
        let error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(error_transport)
        .image_model("dall-e-3");
        let error_result = poll_ready(
            error_model.do_generate(ImageModelCallOptions::new(1).with_prompt("bad prompt")),
        );

        assert!(error_result.images.is_empty());
        assert_eq!(
            error_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .map(|metadata| &metadata.extra)
                .and_then(|extra| extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid image prompt")
        );
    }

    #[test]
    fn openai_compatible_completion_generates_text_through_generate_text() {
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
                        "id": "cmpl-test",
                        "created": 1711363706,
                        "model": "gpt-3.5-turbo-instruct",
                        "choices": [
                            {
                                "text": "Hello from completion",
                                "index": 0,
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
                    "req_completion".to_string(),
                )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/")
                .with_api_key("test-api-key")
                .with_query_param("api-version", "2026-01-01"),
        )
        .with_transport(transport)
        .completion_model("gpt-3.5-turbo-instruct");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello from completion");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(3));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_completion")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/completions?api-version=2026-01-01"
        );
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
                "model": "gpt-3.5-turbo-instruct",
                "max_tokens": 16,
                "temperature": 0.0,
                "prompt": "user:\nSay hello\n\nassistant:\n",
                "stop": ["\nuser:"]
            }))
        );
    }

    #[test]
    fn openai_compatible_completion_streams_text_through_stream_text() {
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
                            "id": "cmpl-stream-test",
                            "created": 1711363440,
                            "model": "gpt-3.5-turbo-instruct",
                            "choices": [
                                {
                                    "text": "Hello ",
                                    "index": 0,
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "cmpl-stream-test",
                            "created": 1711363440,
                            "model": "gpt-3.5-turbo-instruct",
                            "choices": [
                                {
                                    "text": "completion",
                                    "index": 0,
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "cmpl-stream-test",
                            "created": 1711363440,
                            "model": "gpt-3.5-turbo-instruct",
                            "choices": [
                                {
                                    "text": "",
                                    "index": 0,
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 5,
                                "completion_tokens": 2,
                                "total_tokens": 7
                            }
                        }),
                    ]),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_completion_stream".to_string(),
                )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_api_key("test-api-key")
                .with_include_usage(true),
        )
        .with_transport(transport)
        .completion_model("gpt-3.5-turbo-instruct");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(8),
        ));

        assert_eq!(result.text, "Hello completion");
        assert_eq!(result.text_stream, vec!["Hello ", "completion"]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.total, Some(2));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_completion_stream")
        );

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "model": "gpt-3.5-turbo-instruct",
                "max_tokens": 8,
                "prompt": "user:\nSay hello\n\nassistant:\n",
                "stop": ["\nuser:"],
                "stream": true,
                "stream_options": {
                    "include_usage": true
                }
            })
        );
    }

    #[test]
    fn openai_compatible_completion_passes_options_warnings_and_errors() {
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
                        "choices": [
                            {
                                "text": "ok",
                                "finish_reason": "length"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .completion_model("gpt-3.5-turbo-instruct");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "test-provider": {
                "echo": true,
                "logitBias": {
                    "7": 42
                },
                "suffix": "raw-suffix",
                "someCustomOption": "raw-value",
                "user": "raw-user"
            },
            "testProvider": {
                "someCustomOption": "camel-value",
                "user": "camel-user"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Hello"),
                    )]),
                )])
                .with_top_k(5)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(
                        serde_json::from_value(json!({
                            "type": "object",
                            "properties": {}
                        }))
                        .expect("schema deserializes"),
                    ),
                )
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Length);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'test-provider'"
            )
        }));
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "gpt-3.5-turbo-instruct",
                "echo": true,
                "logitBias": {
                    "7": 42
                },
                "logit_bias": {
                    "7": 42
                },
                "suffix": "raw-suffix",
                "someCustomOption": "camel-value",
                "user": "camel-user",
                "prompt": "user:\nHello\n\nassistant:\n",
                "stop": ["\nuser:"]
            }))
        );

        let error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    429,
                    "Too Many Requests",
                    json!({
                        "error": {
                            "message": "Rate limited"
                        }
                    })
                    .to_string(),
                ))))
            });
        let error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(error_transport)
        .completion_model("gpt-3.5-turbo-instruct");
        let error_result =
            poll_ready(error_model.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(error_result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            error_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Rate limited")
        );
    }

    #[test]
    fn openai_compatible_chat_generates_text_through_generate_text() {
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
                        "id": "chatcmpl-test",
                        "created": 1711115037,
                        "model": "test-chat-model",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from OpenAI-compatible"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 3,
                            "total_tokens": 7,
                            "prompt_tokens_details": {
                                "cached_tokens": 1
                            },
                            "completion_tokens_details": {
                                "reasoning_tokens": 2,
                                "accepted_prediction_tokens": 5,
                                "rejected_prediction_tokens": 1
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_openai_compatible".to_string(),
                )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_query_param("api-version", "2026-01-01"),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello from OpenAI-compatible");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.input_tokens.cache_read, Some(1));
        assert_eq!(result.usage.input_tokens.no_cache, Some(3));
        assert_eq!(result.usage.output_tokens.total, Some(3));
        assert_eq!(result.usage.output_tokens.reasoning, Some(2));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("acceptedPredictionTokens"))
                .and_then(JsonValue::as_u64),
            Some(5)
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/chat/completions?api-version=2026-01-01"
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
                "model": "test-chat-model",
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
    fn openai_compatible_chat_streams_text_through_stream_text() {
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
                        "req_openai_compatible_stream".to_string(),
                    ),
                ])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_api_key("test-api-key")
                .with_query_param("api-version", "2026-01-01")
                .with_include_usage(true),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
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
        assert_eq!(result.usage.output_tokens.text, Some(4));
        assert_eq!(result.usage.output_tokens.reasoning, Some(1));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("acceptedPredictionTokens"))
                .and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_openai_compatible_stream")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/chat/completions?api-version=2026-01-01"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
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
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 12,
                "temperature": 0.0,
                "stream": true,
                "stream_options": {
                    "include_usage": true
                }
            })
        );
    }

    #[test]
    fn openai_compatible_chat_streams_reasoning_raw_chunks_and_parse_errors() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "reasoning_content": "Let me think"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "reasoning": " about this"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "Here's my response"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 2,
                                "completion_tokens": 3
                            }
                        }),
                    ]),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Think first"),
                )])
                .with_include_raw_chunks(true),
            ),
        );

        assert!(matches!(
            result.stream.first(),
            Some(LanguageModelStreamPart::StreamStart(_))
        ));
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ReasoningDelta(part) => Some(part.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Let me think", " about this"]
        );
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::TextDelta(part) => Some(part.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Here's my response"]
        );
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::Stop
                    && finish.usage.input_tokens.total == Some(2)
        ));

        let parse_error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    "data: {not json}\n\ndata: [DONE]\n\n",
                ))))
            });
        let parse_error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(parse_error_transport)
        .chat_model("test-chat-model");
        let parse_error_result =
            poll_ready(parse_error_model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert!(
            parse_error_result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Error(_)))
        );
        assert!(matches!(
            parse_error_result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::Error
        ));
    }

    #[test]
    fn openai_compatible_chat_maps_response_formats_and_warnings() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "{}",
                                    "reasoning_content": "reasoning"
                                },
                                "finish_reason": "length"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("JSON only"),
                )])
                .with_top_k(4)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(
                        serde_json::from_value(json!({
                            "type": "object",
                            "properties": {}
                        }))
                        .expect("schema deserializes"),
                    ),
                ),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Length);
        assert_eq!(result.content.len(), 2);
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn openai_compatible_chat_injects_json_instruction_when_response_format_body_is_disabled() {
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
                        "choices": [
                            {
                                "message": {
                                    "content": "{\"answer\":\"ok\"}"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_supports_json_object_response_format(false),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return an answer."),
                    )]),
                )])
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(response_schema),
                ),
            ),
        );

        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Unsupported { feature, .. } if feature == "responseFormat"
            )
        }));

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert!(request_body.get("response_format").is_none());
        let messages = request_body
            .get("messages")
            .and_then(JsonValue::as_array)
            .expect("messages are sent");
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .is_some_and(|content| content.contains("JSON schema:"))
        );
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Return an answer.");
    }

    #[test]
    fn openai_compatible_chat_passes_tools_tool_choice_and_provider_options() {
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
                        "choices": [
                            {
                                "message": {
                                    "content": "ok"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_supports_structured_outputs(true),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai-compatible": {
                "user": "deprecated-user",
                "reasoningEffort": "low"
            },
            "openaiCompatible": {
                "textVerbosity": "low"
            },
            "test-provider": {
                "reasoningEffort": "medium",
                "someCustomOption": "raw-value",
                "user": "raw-user"
            },
            "testProvider": {
                "someCustomOption": "camel-value",
                "strictJsonSchema": false,
                "user": "camel-user"
            }
        }))
        .expect("provider options deserialize");
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
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use the weather tool"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("weather", input_schema.clone())
                        .with_description("Get weather")
                        .with_strict(false),
                ))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "gateway.unsupported",
                    "unsupported",
                    JsonObject::new(),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "weather".to_string(),
                })
                .with_reasoning(LanguageModelReasoningEffort::High)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(input_schema.clone()),
                )
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'openai-compatible'"
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'test-provider'"
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Unsupported { feature, .. }
                    if feature == "provider-defined tool gateway.unsupported"
            )
        }));

        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Use the weather tool"
                    }
                ],
                "user": "camel-user",
                "reasoning_effort": "medium",
                "verbosity": "low",
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "schema": input_schema,
                        "strict": false,
                        "name": "response"
                    }
                },
                "someCustomOption": "camel-value",
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "description": "Get weather",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "city": {
                                        "type": "string"
                                    }
                                },
                                "required": ["city"]
                            },
                            "strict": false
                        }
                    }
                ],
                "tool_choice": {
                    "type": "function",
                    "function": {
                        "name": "weather"
                    }
                }
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_converts_multimodal_user_messages() {
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
                        "choices": [
                            {
                                "message": {
                                    "content": "ok"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let message_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "priority": "high"
            },
            "ignoredProvider": {
                "ignored": true
            }
        }))
        .expect("metadata deserializes");
        let text_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "sentiment": "positive"
            }
        }))
        .expect("metadata deserializes");
        let image_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "alt_text": "A sample image"
            }
        }))
        .expect("metadata deserializes");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Hello").with_provider_options(text_metadata),
                )])
                .with_provider_options(message_metadata.clone()),
            ),
            LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Summarize these inputs")
                            .with_provider_options(message_metadata.clone()),
                    ),
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
                                .expect("url parses"),
                        },
                        "image/*",
                    )),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Base64("AAECAw==".to_string()),
                        },
                        "audio/wav",
                    )),
                    LanguageModelUserContentPart::File(
                        LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "application/pdf",
                        )
                        .with_filename("report.pdf"),
                    ),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Base64("SGVsbG8=".to_string()),
                        },
                        "text/markdown",
                    )),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/readme.md")
                                .expect("url parses"),
                        },
                        "text/markdown",
                    )),
                ])
                .with_provider_options(message_metadata),
            ),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello",
                        "sentiment": "positive"
                    },
                    {
                        "role": "user",
                        "priority": "high",
                        "content": [
                            {
                                "type": "text",
                                "text": "Summarize these inputs",
                                "priority": "high"
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
                            },
                            {
                                "type": "input_audio",
                                "input_audio": {
                                    "data": "AAECAw==",
                                    "format": "wav"
                                }
                            },
                            {
                                "type": "file",
                                "file": {
                                    "filename": "report.pdf",
                                    "file_data": "data:application/pdf;base64,AAECAw=="
                                }
                            },
                            {
                                "type": "text",
                                "text": "Hello"
                            },
                            {
                                "type": "text",
                                "text": "https://example.com/readme.md"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_rejects_unsupported_file_messages_before_transport() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                panic!("transport should not be called for unsupported prompt conversion")
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                    },
                    "video/mp4",
                )),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("'file part media type video/mp4' functionality not supported")
        );
    }

    #[test]
    fn openai_compatible_chat_converts_assistant_tool_history() {
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
                        "choices": [
                            {
                                "message": {
                                    "content": "ok"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let assistant_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "globalPriority": "high"
            }
        }))
        .expect("metadata deserializes");
        let tool_call_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "function_call_reason": "user request"
            },
            "google": {
                "thoughtSignature": "<Signature A>"
            }
        }))
        .expect("metadata deserializes");
        let tool_result_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "partial": true
            }
        }))
        .expect("metadata deserializes");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
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
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "test-chat-model",
                "messages": [
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
    fn openai_compatible_chat_runs_generate_text_tool_loop_end_to_end() {
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
                        "id": "chatcmpl-tool-loop-1",
                        "model": "test-chat-model",
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
                        "id": "chatcmpl-tool-loop-2",
                        "model": "test-chat-model",
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
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
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
                "model": "test-chat-model",
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
                "model": "test-chat-model",
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
    fn openai_compatible_chat_runs_stream_text_tool_loop_end_to_end() {
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
                            "id": "chatcmpl-tool-stream-1",
                            "model": "test-chat-model",
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
                            "id": "chatcmpl-tool-stream-1",
                            "model": "test-chat-model",
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
                            "id": "chatcmpl-tool-stream-2",
                            "model": "test-chat-model",
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
                            "id": "chatcmpl-tool-stream-2",
                            "model": "test-chat-model",
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
                            "id": "chatcmpl-tool-stream-2",
                            "model": "test-chat-model",
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
                    .with_headers(Headers::from([(
                        "content-type".to_string(),
                        "text/event-stream".to_string(),
                    )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
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
                "model": "test-chat-model",
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
                "model": "test-chat-model",
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
    fn openai_compatible_chat_maps_tool_calls_from_generate() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
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
                                            },
                                            "extra_content": {
                                                "google": {
                                                    "thought_signature": "signature-1"
                                                }
                                            }
                                        }
                                    ]
                                },
                                "finish_reason": "tool_calls"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                    "What is the weather?",
                )),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert!(matches!(
            result.content.first(),
            Some(crate::language_model::LanguageModelContent::ToolCall(tool_call))
                if tool_call.tool_call_id == "call_1"
                    && tool_call.tool_name == "weather"
                    && tool_call.input == "{\"city\":\"Brisbane\"}"
                    && tool_call
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("test-provider"))
                        .and_then(|metadata| metadata.get("thoughtSignature"))
                        .and_then(JsonValue::as_str)
                        == Some("signature-1")
        ));
    }

    #[test]
    fn openai_compatible_chat_streams_tool_calls() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-tool-stream",
                            "created": 1711115037,
                            "model": "test-chat-model",
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
                                                },
                                                "extra_content": {
                                                    "google": {
                                                        "thought_signature": "signature-1"
                                                    }
                                                }
                                            }
                                        ]
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-tool-stream",
                            "created": 1711115037,
                            "model": "test-chat-model",
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
                                "prompt_tokens": 2,
                                "completion_tokens": 1
                            }
                        }),
                    ]),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                    "What is the weather?",
                )),
            ])),
        ])));

        assert!(result.stream.iter().any(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ToolInputStart(start)
                    if start.id == "call_1" && start.tool_name == "weather"
            )
        }));
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ToolInputDelta(delta) => Some(delta.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["{\"city\"", ":\"Brisbane\"}"]
        );
        assert!(result.stream.iter().any(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ToolInputEnd(end) if end.id == "call_1"
            )
        }));
        assert!(result.stream.iter().any(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ToolCall(tool_call)
                    if tool_call.tool_call_id == "call_1"
                        && tool_call.tool_name == "weather"
                        && tool_call.input == "{\"city\":\"Brisbane\"}"
                        && tool_call
                            .provider_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.get("test-provider"))
                            .and_then(|metadata| metadata.get("thoughtSignature"))
                            .and_then(JsonValue::as_str)
                            == Some("signature-1")
            )
        }));
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::ToolCalls
        ));
    }

    fn openai_compatible_chat_stream_body() -> String {
        sse_body([
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "test-chat-model",
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
                "model": "test-chat-model",
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
                "model": "test-chat-model",
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
                "model": "test-chat-model",
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 4,
                    "completion_tokens": 5,
                    "completion_tokens_details": {
                        "reasoning_tokens": 1,
                        "accepted_prediction_tokens": 2,
                        "rejected_prediction_tokens": 3
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

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending in test"),
        }
    }
}
