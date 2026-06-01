# AI-02 OpenAI-Compatible Provider Disposition

Upstream was refreshed on 2026-06-01 with `npx opensrc fetch github:vercel/ai`.
The checked mirror used for this row is
`/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/ai/main`.

None of the AI-02 package rows are marked `verified` in this slice. The rows
stay `in-progress` until every current portable upstream case is either covered
by a named Rust test or moved from the exception column below into a Rust test.

## Rust Evidence

| Package | Named Rust tests added or retained |
| --- | --- |
| `packages/xai` | `xai_provider_creates_responses_model_with_headers_base_url_and_body`; `xai_responses_model_prepares_server_tools_custom_tool_and_usage`; `xai_provider_creates_chat_model_with_openai_compatible_transport`; `xai_provider_creates_image_model_and_reports_unsupported_embeddings`; `xai_provider_settings_serde_accepts_upstream_base_url`; `xai_provider_implements_provider_trait` |
| `packages/groq` | `groq_provider_creates_chat_model_with_headers_base_url_and_provider_options`; `groq_provider_creates_transcription_model_with_headers_options_and_response`; `groq_provider_implements_transcription_trait`; `groq_provider_uses_default_base_url_and_function_alias`; `groq_provider_reports_unsupported_model_families`; `groq_provider_implements_provider_trait`; `groq_provider_settings_serde_accepts_upstream_base_url` |
| `packages/cohere` | `cohere_provider_creates_embedding_model_with_options_headers_and_usage`; `cohere_embedding_model_uses_default_input_type_and_exposes_raw_response`; `cohere_provider_creates_reranking_model_with_object_warning_and_options`; `cohere_reranking_model_sends_text_documents_without_warnings`; `cohere_provider_reports_chat_and_image_exceptions`; `cohere_provider_implements_embedding_and_reranking_traits`; `cohere_provider_settings_serde_accepts_upstream_base_url` |
| `packages/fireworks` | `fireworks_provider_creates_chat_model_with_transformed_provider_options`; `fireworks_provider_creates_completion_embedding_and_image_models`; `fireworks_image_model_sends_workflow_request_and_returns_binary`; `fireworks_image_model_sends_async_edit_input_image_and_warnings`; `fireworks_image_model_maps_image_generation_size_and_aspect_warning`; `fireworks_image_model_aborts_before_request`; `fireworks_provider_uses_default_base_url_and_function_alias`; `fireworks_provider_implements_provider_trait`; `fireworks_provider_settings_serde_accepts_upstream_base_url` |
| `packages/togetherai` | `togetherai_provider_creates_chat_model_with_headers_base_url_and_body`; `togetherai_provider_creates_completion_model`; `togetherai_provider_creates_embedding_model_aliases`; `togetherai_provider_creates_image_model_and_generates_images`; `togetherai_image_model_maps_api_error_to_metadata`; `togetherai_image_model_aborts_before_request`; `togetherai_image_model_reports_unsupported_mask_without_request`; `togetherai_provider_creates_reranking_model`; `togetherai_reranking_model_maps_api_error_to_metadata`; `togetherai_reranking_model_aborts_before_request`; `togetherai_provider_uses_default_base_url_and_function_alias`; `togetherai_api_key_prefers_explicit_then_new_env_then_deprecated_env`; `togetherai_provider_implements_provider_trait`; `togetherai_provider_settings_serde_accepts_upstream_base_url` |

## Current Upstream Case Disposition

Case counts are current upstream `it`/`test` declarations in the refreshed
mirror. Snapshot files and static media fixtures are not counted separately. A
file-level exception applies to every current case in that file that is not
listed in the "covered by" column.

| Package | Upstream file | Cases | Covered by | Explicit exception for remaining portable cases |
| --- | --- | ---: | --- | --- |
| `packages/xai` | `packages/xai/src/xai-provider.test.ts` | 12 | xAI provider construction tests named above | Video, files, model-family factory parity, and exact thrown-error surfaces remain open. |
| `packages/xai` | `packages/xai/src/xai-chat-language-model.test.ts` | 49 | `xai_provider_creates_chat_model_with_openai_compatible_transport` | xAI chat request conversion, streaming, tool calls, finish reasons, usage metadata, error bodies, warnings, provider options, and live/API edge behavior remain open. |
| `packages/xai` | `packages/xai/src/xai-prepare-tools.test.ts` | 14 | none | xAI tool conversion is not yet ported as provider-specific logic. |
| `packages/xai` | `packages/xai/src/convert-to-xai-chat-messages.test.ts` | 16 | none | xAI message conversion helper parity remains open. |
| `packages/xai` | `packages/xai/src/convert-xai-chat-usage.test.ts` | 7 | none | xAI usage conversion helper parity remains open. |
| `packages/xai` | `packages/xai/src/xai-image-model.test.ts` | 21 | `xai_provider_creates_image_model_and_reports_unsupported_embeddings` | Image generation request/response, warning, error, and live/API behavior remain open. |
| `packages/xai` | `packages/xai/src/xai-error.test.ts` | 2 | none | xAI error schema mapping remains open. |
| `packages/xai` | `packages/xai/src/files/xai-files.test.ts` | 12 | none | xAI file upload/list/retrieve/delete behavior remains open. |
| `packages/xai` | `packages/xai/src/responses/xai-responses-language-model.test.ts` | 70 | `xai_provider_creates_responses_model_with_headers_base_url_and_body`; `xai_responses_model_prepares_server_tools_custom_tool_and_usage` | Shared Open Responses request setup, shared server-tool/custom-tool serialization, and usage conversion have deterministic coverage. Remaining portable cases are xAI-specific provider tool ids and tool-call name mapping, streaming, response metadata including cost ticks, reasoning edge cases, unsupported output modes, error mapping, and live/API behavior. |
| `packages/xai` | `packages/xai/src/responses/xai-responses-prepare-tools.test.ts` | 32 | `xai_responses_model_prepares_server_tools_custom_tool_and_usage` for shared `openai.web_search` and `openai.custom` compatibility | xAI-specific tool ids (`xai.web_search`, `xai.x_search`, `xai.code_execution`, `xai.view_image`, `xai.view_x_video`, `xai.file_search`, `xai.mcp`), provider-specific tool-choice mapping, unsupported warnings, and exact split parity remain open. |
| `packages/xai` | `packages/xai/src/responses/convert-to-xai-responses-input.test.ts` | 21 | none | xAI Responses input conversion helper parity remains open. |
| `packages/xai` | `packages/xai/src/responses/convert-xai-responses-usage.test.ts` | 6 | none | xAI Responses usage conversion helper parity remains open. |
| `packages/xai` | `packages/xai/src/xai-video-model.test.ts` | 58 | none | xAI video generation, polling, download, abort, error, and live/API behavior remain open. |
| `packages/xai` | `packages/xai/src/xai-video-options.test-d.ts` | 17 | none | TypeScript-only video option inference is not portable; equivalent runtime option coverage remains open. |
| `packages/groq` | `packages/groq/src/groq-chat-language-model.test.ts` | 41 | `groq_provider_creates_chat_model_with_headers_base_url_and_provider_options` | Groq chat streaming, reasoning, raw message conversion, error mapping, tool results, usage details, and live/API behavior remain open. |
| `packages/groq` | `packages/groq/src/groq-chat-language-model-options.test.ts` | 10 | `groq_provider_creates_chat_model_with_headers_base_url_and_provider_options` | Full Groq option validation and typed option coverage remain open. |
| `packages/groq` | `packages/groq/src/groq-prepare-tools.test.ts` | 17 | none | Groq tool preparation, provider-defined browser search, and unsupported tool warnings remain open. |
| `packages/groq` | `packages/groq/src/convert-to-groq-chat-messages.test.ts` | 11 | none | Groq message conversion helper parity remains open. |
| `packages/groq` | `packages/groq/src/convert-groq-usage.test.ts` | 9 | none | Groq usage conversion helper parity remains open. |
| `packages/groq` | `packages/groq/src/groq-transcription-model.test.ts` | 6 | `groq_provider_creates_transcription_model_with_headers_options_and_response`; `groq_provider_implements_transcription_trait` | Multipart model/header/provider-option request shaping, text/segment/language/duration/raw-response mapping, and trait construction are covered. Remaining exact exceptions are upstream `_internal.currentDate` injection, thrown-error class identity, live/API acceptance, and any future option validation beyond the deterministic provider-option matrix. |
| `packages/cohere` | `packages/cohere/src/cohere-embedding-model.test.ts` | 7 | `cohere_provider_creates_embedding_model_with_options_headers_and_usage`; `cohere_embedding_model_uses_default_input_type_and_exposes_raw_response` | Embedding extraction, raw response, usage, model/text values, input type, output dimension, and headers are covered. Remaining exact exceptions are chunking limit errors, schema validation edge cases, thrown-error class identity, and live/API behavior. |
| `packages/cohere` | `packages/cohere/src/reranking/cohere-reranking-model.test.ts` | 12 | `cohere_provider_creates_reranking_model_with_object_warning_and_options`; `cohere_reranking_model_sends_text_documents_without_warnings` | Object-document stringification/warning, text-document request shaping, headers, warning/no-warning paths, ranking, raw response body, and API error metadata are covered. Remaining exact exceptions are validation edge cases, thrown-error class identity, and live/API behavior. |
| `packages/cohere` | `packages/cohere/src/cohere-chat-language-model.test.ts` | 35 | `cohere_provider_reports_chat_and_image_exceptions` | Cohere chat is explicitly unsupported in this Rust slice; all chat request, stream, citation, tool, error, and live/API cases remain open. |
| `packages/cohere` | `packages/cohere/src/cohere-prepare-tools.test.ts` | 7 | none | Cohere chat tool preparation remains open with the chat model exception. |
| `packages/cohere` | `packages/cohere/src/convert-to-cohere-chat-prompt.test.ts` | 13 | none | Cohere chat prompt conversion remains open with the chat model exception. |
| `packages/fireworks` | `packages/fireworks/src/fireworks-provider.test.ts` | 15 | Fireworks provider construction tests named above | Exact provider factory surface, option aliases, and unsupported-family parity beyond the current wrapper tests remain open. |
| `packages/fireworks` | `packages/fireworks/src/fireworks-image-model.test.ts` | 28 | `fireworks_provider_creates_completion_embedding_and_image_models`; `fireworks_image_model_sends_workflow_request_and_returns_binary`; `fireworks_image_model_sends_async_edit_input_image_and_warnings`; `fireworks_image_model_maps_image_generation_size_and_aspect_warning`; `fireworks_image_model_aborts_before_request` | Workflow and image-generation URL routing, prompt/aspect/size/seed/samples/provider-option request shaping, custom headers/auth/user-agent, binary responses, async submit/poll/download, edit input-image data URI, multiple-image and mask warnings, response headers, provider/model info, and abort-before-request are covered. Remaining exact exceptions are empty-body/API error split coverage, URL/base64 edit split duplication, multi-poll timing with JavaScript delay semantics, timeout/failure thrown-error class identity, and live/API behavior. |
| `packages/togetherai` | `packages/togetherai/src/togetherai-provider.test.ts` | 12 | TogetherAI provider construction tests named above | Provider behavior is mapped case-by-case below. Remaining exact exceptions are console deprecation warning semantics and live/API behavior. |
| `packages/togetherai` | `packages/togetherai/src/togetherai-image-model.test.ts` | 16 | TogetherAI image tests named above; `togetherai_image_model_aborts_before_request` | Parameters including size/seed, multiple image count, URL, headers, API error metadata, aspect-ratio warning, abort-before-request, response metadata/headers, provider/model info, URL/base64/bytes image inputs, unsupported mask, multiple-file warning, and provider options are covered. Remaining exact exceptions are JavaScript thrown-error class identity for the mask/error paths and live/API behavior. |
| `packages/togetherai` | `packages/togetherai/src/reranking/togetherai-reranking-model.test.ts` | 12 | TogetherAI reranking tests named above; `togetherai_reranking_model_aborts_before_request` | JSON-document and text-document request shaping, headers, warning/no-warning paths, ranking, raw response body, response headers/model/id metadata, API error metadata, and abort-before-request are covered. Remaining exact exceptions are validation edge cases, JavaScript thrown-error class identity, and live/API behavior. |

## TogetherAI Exact Case Map

| Upstream file | Current case(s) | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/togetherai/src/togetherai-provider.test.ts` | default provider options; callable provider returns chat model | `togetherai_provider_uses_default_base_url_and_function_alias`; `togetherai_provider_settings_serde_accepts_upstream_base_url` | none |
| `packages/togetherai/src/togetherai-provider.test.ts` | custom provider options; chat/completion/text-embedding/image/reranking model configuration; custom base URL passed to image model | `togetherai_provider_creates_chat_model_with_headers_base_url_and_body`; `togetherai_provider_creates_completion_model`; `togetherai_provider_creates_embedding_model_aliases`; `togetherai_provider_creates_image_model_and_generates_images`; `togetherai_provider_creates_reranking_model`; `togetherai_provider_implements_provider_trait` | exact JavaScript constructor mock assertions are not portable, but the request/configuration behavior is covered |
| `packages/togetherai/src/togetherai-provider.test.ts` | deprecated `TOGETHER_AI_API_KEY` fallback; `TOGETHER_API_KEY` precedence; explicit api key precedence | `togetherai_api_key_prefers_explicit_then_new_env_then_deprecated_env` | deprecation `console.warn` side effect is JavaScript-runtime-only; Rust covers precedence without console semantics |
| `packages/togetherai/src/togetherai-image-model.test.ts` | correct parameters with size/seed; multiple images; correct URL; headers; response timestamp/model/headers; provider/model info | `togetherai_provider_creates_image_model_and_generates_images` | live/API acceptance only |
| `packages/togetherai/src/togetherai-image-model.test.ts` | API errors | `togetherai_image_model_maps_api_error_to_metadata` | exact thrown-error class identity is JavaScript-specific; Rust stores provider error metadata |
| `packages/togetherai/src/togetherai-image-model.test.ts` | aspect-ratio warning; URL file; bytes file; base64 file; mask; multiple files; provider options with editing | `togetherai_provider_creates_image_model_and_generates_images`; `togetherai_image_model_reports_unsupported_mask_without_request` | mask is represented as warning/error metadata instead of JavaScript thrown-class identity |
| `packages/togetherai/src/togetherai-image-model.test.ts` | abort signal | `togetherai_image_model_aborts_before_request` | live transport abort timing remains live-only |
| `packages/togetherai/src/reranking/togetherai-reranking-model.test.ts` | JSON-document request, headers, warnings, ranking, provider metadata absent, response metadata | `togetherai_provider_creates_reranking_model`; `togetherai_reranking_model_maps_api_error_to_metadata` | live/API acceptance only |
| `packages/togetherai/src/reranking/togetherai-reranking-model.test.ts` | text-document request, headers, no warnings, ranking, provider metadata absent, response metadata | `togetherai_provider_creates_reranking_model` | live/API acceptance only |
| `packages/togetherai/src/reranking/togetherai-reranking-model.test.ts` | abort signal | `togetherai_reranking_model_aborts_before_request` | live transport abort timing remains live-only |

## Live-Only Proof Gaps

AI-02B did not add new ignored live tests because every owned provider row still
has named deterministic parity gaps above. Live-provider proof should be added
only after those gaps close enough that remote acceptance is the remaining
question.

| Package | Live-only proof after deterministic closure | Credential gate |
| --- | --- | --- |
| `packages/xai` | Real xAI Responses hosted tools, Files API, image/video model API acceptance, and remote error bodies. | `XAI_API_KEY` |
| `packages/groq` | Real Groq chat streaming, browser-search provider tool acceptance, and transcription API acceptance. | `GROQ_API_KEY` |
| `packages/cohere` | Real Cohere embedding/rerank API acceptance and any future chat model surface if ported. | `COHERE_API_KEY` |
| `packages/fireworks` | Real Fireworks image workflow/image-generation acceptance, async polling timing, and provider error bodies. | `FIREWORKS_API_KEY` |
| `packages/togetherai` | Real TogetherAI image/rerank API acceptance and transport abort timing against a live request. | `TOGETHER_API_KEY` or deprecated `TOGETHER_AI_API_KEY` |
