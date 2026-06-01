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
| `packages/xai` | `xai_provider_creates_responses_model_with_headers_base_url_and_body`; `xai_provider_creates_chat_model_with_openai_compatible_transport`; `xai_provider_creates_image_model_and_reports_unsupported_embeddings`; `xai_provider_settings_serde_accepts_upstream_base_url`; `xai_provider_implements_provider_trait` |
| `packages/groq` | `groq_provider_creates_chat_model_with_headers_base_url_and_provider_options`; `groq_provider_uses_default_base_url_and_function_alias`; `groq_provider_reports_unsupported_model_families`; `groq_provider_implements_provider_trait`; `groq_provider_settings_serde_accepts_upstream_base_url` |
| `packages/cohere` | `cohere_provider_creates_embedding_model_with_options_headers_and_usage`; `cohere_provider_creates_reranking_model_with_object_warning_and_options`; `cohere_provider_reports_chat_and_image_exceptions`; `cohere_provider_implements_embedding_and_reranking_traits`; `cohere_provider_settings_serde_accepts_upstream_base_url` |
| `packages/fireworks` | `fireworks_provider_creates_chat_model_with_transformed_provider_options`; `fireworks_provider_creates_completion_embedding_and_image_models`; `fireworks_provider_uses_default_base_url_and_function_alias`; `fireworks_provider_implements_provider_trait`; `fireworks_provider_settings_serde_accepts_upstream_base_url` |
| `packages/togetherai` | `togetherai_provider_creates_chat_model_with_headers_base_url_and_body`; `togetherai_provider_creates_completion_model`; `togetherai_provider_creates_embedding_model_aliases`; `togetherai_provider_creates_image_model_and_generates_images`; `togetherai_image_model_maps_api_error_to_metadata`; `togetherai_image_model_reports_unsupported_mask_without_request`; `togetherai_provider_creates_reranking_model`; `togetherai_reranking_model_maps_api_error_to_metadata`; `togetherai_provider_uses_default_base_url_and_function_alias`; `togetherai_api_key_prefers_explicit_then_new_env_then_deprecated_env`; `togetherai_provider_implements_provider_trait`; `togetherai_provider_settings_serde_accepts_upstream_base_url` |

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
| `packages/xai` | `packages/xai/src/responses/xai-responses-language-model.test.ts` | 70 | `xai_provider_creates_responses_model_with_headers_base_url_and_body` | xAI Responses server tools, streaming, response metadata, reasoning, unsupported output modes, error mapping, and live/API behavior remain open. |
| `packages/xai` | `packages/xai/src/responses/xai-responses-prepare-tools.test.ts` | 32 | none | xAI Responses tool preparation remains open. |
| `packages/xai` | `packages/xai/src/responses/convert-to-xai-responses-input.test.ts` | 21 | none | xAI Responses input conversion helper parity remains open. |
| `packages/xai` | `packages/xai/src/responses/convert-xai-responses-usage.test.ts` | 6 | none | xAI Responses usage conversion helper parity remains open. |
| `packages/xai` | `packages/xai/src/xai-video-model.test.ts` | 58 | none | xAI video generation, polling, download, abort, error, and live/API behavior remain open. |
| `packages/xai` | `packages/xai/src/xai-video-options.test-d.ts` | 17 | none | TypeScript-only video option inference is not portable; equivalent runtime option coverage remains open. |
| `packages/groq` | `packages/groq/src/groq-chat-language-model.test.ts` | 41 | `groq_provider_creates_chat_model_with_headers_base_url_and_provider_options` | Groq chat streaming, reasoning, raw message conversion, error mapping, tool results, usage details, and live/API behavior remain open. |
| `packages/groq` | `packages/groq/src/groq-chat-language-model-options.test.ts` | 10 | `groq_provider_creates_chat_model_with_headers_base_url_and_provider_options` | Full Groq option validation and typed option coverage remain open. |
| `packages/groq` | `packages/groq/src/groq-prepare-tools.test.ts` | 17 | none | Groq tool preparation, provider-defined browser search, and unsupported tool warnings remain open. |
| `packages/groq` | `packages/groq/src/convert-to-groq-chat-messages.test.ts` | 11 | none | Groq message conversion helper parity remains open. |
| `packages/groq` | `packages/groq/src/convert-groq-usage.test.ts` | 9 | none | Groq usage conversion helper parity remains open. |
| `packages/groq` | `packages/groq/src/groq-transcription-model.test.ts` | 6 | none | Groq transcription model request/response/error parity remains open. |
| `packages/cohere` | `packages/cohere/src/cohere-embedding-model.test.ts` | 7 | `cohere_provider_creates_embedding_model_with_options_headers_and_usage` | Cohere embedding chunking limit errors, schema validation edge cases, and live/API behavior remain open. |
| `packages/cohere` | `packages/cohere/src/reranking/cohere-reranking-model.test.ts` | 12 | `cohere_provider_creates_reranking_model_with_object_warning_and_options` | Cohere reranking full request matrix, error details, validation edge cases, and live/API behavior remain open. |
| `packages/cohere` | `packages/cohere/src/cohere-chat-language-model.test.ts` | 35 | `cohere_provider_reports_chat_and_image_exceptions` | Cohere chat is explicitly unsupported in this Rust slice; all chat request, stream, citation, tool, error, and live/API cases remain open. |
| `packages/cohere` | `packages/cohere/src/cohere-prepare-tools.test.ts` | 7 | none | Cohere chat tool preparation remains open with the chat model exception. |
| `packages/cohere` | `packages/cohere/src/convert-to-cohere-chat-prompt.test.ts` | 13 | none | Cohere chat prompt conversion remains open with the chat model exception. |
| `packages/fireworks` | `packages/fireworks/src/fireworks-provider.test.ts` | 15 | Fireworks provider construction tests named above | Exact provider factory surface, option aliases, and unsupported-family parity beyond the current wrapper tests remain open. |
| `packages/fireworks` | `packages/fireworks/src/fireworks-image-model.test.ts` | 28 | `fireworks_provider_creates_completion_embedding_and_image_models` | Fireworks custom image request/response, async polling, edit/mask, binary output, abort, error, and live/API behavior remain open. |
| `packages/togetherai` | `packages/togetherai/src/togetherai-provider.test.ts` | 12 | TogetherAI provider construction tests named above | Exact one-to-one upstream split, console deprecation warning semantics, and live/API behavior remain open. |
| `packages/togetherai` | `packages/togetherai/src/togetherai-image-model.test.ts` | 16 | TogetherAI image tests named above | Abort/live behavior, exact thrown-error class parity, and per-case split parity remain open. |
| `packages/togetherai` | `packages/togetherai/src/reranking/togetherai-reranking-model.test.ts` | 12 | TogetherAI reranking tests named above | Abort/live behavior, validation edge cases, and per-case split parity remain open. |
