# AI-06 Concrete Provider Mappings

Generated from upstream `vercel/ai` after `npx opensrc fetch github:vercel/ai`.

| Field | Value |
| --- | --- |
| Upstream commit | `43e84c8e39e540aa23e25986031183227a77d531` |
| Upstream commit date | `2026-06-01T20:12:00Z` |
| Inventory date | `2026-06-02` |
| Local upstream source | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/ai/main` |

This document records AIS-06 row-level concrete-provider mappings that are not
part of the AI-01 foundational provider map or the AI-02 OpenAI-compatible
provider disposition. Rows below are consumed by
`scripts/ai-strict-test-inventory.mjs`.

## AIS-06 Remaining Concrete Provider Buckets

After the KlingAI wave, AIS-12 Fal wave, and AIS-17 Alibaba wave, these
concrete-provider buckets still have strict portable cases without row-level
named Rust mappings. Buckets at `0` remain listed to make closed rows visible to
the strict inventory generator.

| Package | Owner | Portable unmapped |
| --- | --- | ---: |
| `packages/alibaba` | `crates/ai-sdk-alibaba` | 0 |
| `packages/assemblyai` | `crates/ai-sdk-assemblyai` | 6 |
| `packages/azure` | `crates/ai-sdk-azure`, `crates/ai-sdk-open-responses`, `src/openai_compatible.rs` | 55 |
| `packages/baseten` | `src/baseten.rs`, `src/openai_compatible.rs` | 25 |
| `packages/black-forest-labs` | `crates/ai-sdk-black-forest-labs` | 24 |
| `packages/bytedance` | `crates/ai-sdk-bytedance` | 41 |
| `packages/cerebras` | `src/cerebras.rs`, `src/openai_compatible.rs` | 13 |
| `packages/deepgram` | `crates/ai-sdk-deepgram` | 25 |
| `packages/deepinfra` | `src/deepinfra.rs`, `src/openai_compatible.rs` | 25 |
| `packages/deepseek` | `crates/ai-sdk-deepseek` | 38 |
| `packages/elevenlabs` | `crates/ai-sdk-elevenlabs` | 15 |
| `packages/fal` | `crates/ai-sdk-fal` | 12 |
| `packages/gladia` | `crates/ai-sdk-gladia` | 7 |
| `packages/huggingface` | `src/huggingface.rs` | 37 |
| `packages/hume` | `crates/ai-sdk-hume` | 9 |
| `packages/klingai` | `crates/ai-sdk-klingai` | 0 |
| `packages/lmnt` | `crates/ai-sdk-lmnt` | 9 |
| `packages/luma` | `crates/ai-sdk-luma` | 29 |
| `packages/mistral` | `crates/ai-sdk-mistral` | 72 |
| `packages/moonshotai` | `crates/ai-sdk-moonshotai` | 17 |
| `packages/perplexity` | `crates/ai-sdk-perplexity` | 32 |
| `packages/prodia` | `crates/ai-sdk-prodia` | 54 |
| `packages/replicate` | `crates/ai-sdk-replicate` | 63 |
| `packages/revai` | `crates/ai-sdk-revai` | 6 |
| `packages/togetherai` | `src/togetherai.rs`, `src/openai_compatible.rs` | 10 |
| `packages/voyage` | `src/voyage.rs` | 21 |

## Alibaba Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should extract text content | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should send correct request body | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should extract tool call content | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should extract reasoning content | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should extract usage with reasoning tokens | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should map top-level reasoning to enable_thinking true with budget | `alibaba_chat_model_maps_top_level_reasoning_variants_and_provider_override` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should map top-level reasoning none to enable_thinking false | `alibaba_chat_model_maps_top_level_reasoning_variants_and_provider_override` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should prefer providerOptions over top-level reasoning | `alibaba_chat_model_maps_top_level_reasoning_variants_and_provider_override` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should not set thinking when reasoning is not specified | `alibaba_chat_model_maps_top_level_reasoning_variants_and_provider_override` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should extract usage with cache tokens | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should send enable_thinking in request body | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should stream text | `alibaba_chat_model_streams_reasoning_text_tool_calls_usage_and_raw_chunks` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should stream tool call | `alibaba_chat_model_streams_reasoning_text_tool_calls_usage_and_raw_chunks; alibaba_chat_model_streams_incremental_tool_call_arguments` | none |
| `packages/alibaba/src/alibaba-chat-language-model.test.ts` | should stream reasoning | `alibaba_chat_model_streams_reasoning_text_tool_calls_usage_and_raw_chunks` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should create an AlibabaProvider instance with default options | `alibaba_provider_reports_unsupported_model_families_and_trait_video; alibaba_chat_model_requests_stream_usage_by_default` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should create an AlibabaProvider instance with custom options | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage; alibaba_provider_settings_serde_accepts_upstream_shape` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should return a chat model when called as a function | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should pass includeUsage option to language model | `alibaba_chat_model_streams_reasoning_text_tool_calls_usage_and_raw_chunks` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should default includeUsage to true | `alibaba_chat_model_requests_stream_usage_by_default` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should construct a chat model with correct configuration | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should construct a language model with correct configuration | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should construct a video model with correct provider | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should use default videoBaseURL | `alibaba_video_model_generates_video_with_headers_body_and_metadata; alibaba_provider_settings_serde_accepts_upstream_shape` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should use custom videoBaseURL | `alibaba_video_model_uses_custom_base_url_and_polls_until_succeeded; alibaba_provider_settings_serde_accepts_upstream_shape` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should pass custom fetch to video model | `alibaba_video_model_uses_custom_base_url_and_polls_until_succeeded` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should pass headers function to video model | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should construct a video model with correct configuration | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-provider.test.ts` | should use the same videoBaseURL as video() | `alibaba_video_model_uses_custom_base_url_and_polls_until_succeeded; alibaba_provider_settings_serde_accepts_upstream_shape` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should expose correct provider and model information | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should accept custom model IDs | `alibaba_provider_reports_unsupported_model_families_and_trait_video` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send correct request body for T2V | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send size parameter for T2V resolution (x converted to *) | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send duration parameter | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send seed parameter | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send provider options (negativePrompt, promptExtend, shotType, watermark) | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send audioUrl in input | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send img_url from URL-based image for I2V model | `alibaba_video_model_maps_url_images_720p_resolution_and_omits_mode_specific_inputs` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send img_url as base64 from file data | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should map resolution to I2V format (WxH → "720P"/"1080P") | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should map 720p resolution for I2V | `alibaba_video_model_maps_url_images_720p_resolution_and_omits_mode_specific_inputs` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should not send img_url for T2V model even if image provided | `alibaba_video_model_maps_url_images_720p_resolution_and_omits_mode_specific_inputs` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send audio provider option for I2V | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send reference_urls for R2V model | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send size parameter for R2V resolution (x converted to *) | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should not send reference_urls for non-R2V model | `alibaba_video_model_maps_url_images_720p_resolution_and_omits_mode_specific_inputs` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send X-DashScope-Async header on task creation | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should send Authorization header | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should pass custom headers | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should not send X-DashScope-Async header on polling requests | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should warn about unsupported aspectRatio | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should warn about unsupported fps | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should warn when n > 1 | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should not warn when n is 1 | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should return empty warnings for supported features | `alibaba_video_model_maps_i2v_r2v_resolution_and_warnings` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should return video with correct URL and media type | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should include timestamp, modelId, and headers in response | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should include taskId, videoUrl, actualPrompt, and usage | `alibaba_video_model_generates_video_with_headers_body_and_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should throw when no task_id is returned | `alibaba_video_model_maps_missing_task_canceled_and_missing_url_errors` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should throw when task status is FAILED | `alibaba_video_model_maps_api_and_status_errors_to_metadata` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should throw when task status is CANCELED | `alibaba_video_model_maps_missing_task_canceled_and_missing_url_errors` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should throw when no video URL in succeeded response | `alibaba_video_model_maps_missing_task_canceled_and_missing_url_errors` | none |
| `packages/alibaba/src/alibaba-video-model.test.ts` | should poll until SUCCEEDED status | `alibaba_video_model_uses_custom_base_url_and_polls_until_succeeded` | none |
| `packages/alibaba/src/convert-alibaba-usage.test.ts` | should correctly calculate token distribution with cache tokens | `alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should use array format for single text user message | `alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should use array format for multi-part user message with image | `alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should convert assistant message with tool calls | `alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should convert tool results | `alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should inject cache control into system message content block | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should inject cache control into single text user message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should use part-level cache control for single-part user message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should prefer part-level over message-level cache control for single-part user message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should use part-level cache control for multi-part user message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should apply message-level cache control to last part of multi-part user message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should inject cache control into assistant message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should inject cache control into single-part tool message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should use part-level cache control for single-part tool message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should prefer part-level over message-level cache control for single-part tool message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should apply message-level cache control to last part of multi-part tool message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should use part-level cache control for multi-part tool message | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | should throw for file parts with provider references | `alibaba_chat_model_reports_provider_reference_and_invalid_option_errors; alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | passes full image/png through unchanged for inline data | `alibaba_chat_model_reports_provider_reference_and_invalid_option_errors; alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | detects image subtype from inline bytes for top-level "image" | `alibaba_chat_model_reports_provider_reference_and_invalid_option_errors; alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | passes through URL source for top-level-only image | `alibaba_chat_model_reports_provider_reference_and_invalid_option_errors; alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/convert-to-alibaba-chat-messages.test.ts` | normalizes image/* wildcard via detection | `alibaba_chat_model_reports_provider_reference_and_invalid_option_errors; alibaba_chat_model_converts_multimodal_assistant_and_tool_messages` | none |
| `packages/alibaba/src/get-cache-control.test.ts` | should extract cacheControl from providerMetadata | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/get-cache-control.test.ts` | should warn when exceeding 4 cache breakpoints | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |
| `packages/alibaba/src/get-cache-control.test.ts` | should return undefined when no cache control is present | `alibaba_chat_model_applies_cache_control_precedence_and_breakpoint_warning` | none |

## Fal Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/fal/src/fal-error.test.ts` | should parse Fal resource exhausted error | `fal_error_schema_parses_resource_exhausted_message` | none |
| `packages/fal/src/fal-image-model.test.ts` | should pass the correct parameters including size | `fal_image_model_converts_camel_case_provider_options_to_snake_case_for_api` | none |
| `packages/fal/src/fal-image-model.test.ts` | should convert camelCase provider options to snake_case for API | `fal_image_model_converts_camel_case_provider_options_to_snake_case_for_api` | none |
| `packages/fal/src/fal-image-model.test.ts` | should accept deprecated snake_case provider options with warning | `fal_image_model_accepts_deprecated_snake_case_provider_options_with_warning` | none |
| `packages/fal/src/fal-image-model.test.ts` | should convert aspect ratio to size | `fal_image_model_maps_request_downloads_metadata_and_warnings` | none |
| `packages/fal/src/fal-image-model.test.ts` | should pass headers | `fal_image_model_maps_request_downloads_metadata_and_warnings` | none |
| `packages/fal/src/fal-image-model.test.ts` | should handle API errors | `fal_models_map_api_errors_to_provider_metadata` | none |
| `packages/fal/src/fal-image-model.test.ts` | should include timestamp, headers and modelId in response | `fal_image_model_maps_request_downloads_metadata_and_warnings` | none |
| `packages/fal/src/fal-image-model.test.ts` | for lora | `fal_image_model_preserves_lora_lcm_provider_metadata` | none |
| `packages/fal/src/fal-image-model.test.ts` | for lcm | `fal_image_model_preserves_lora_lcm_provider_metadata` | none |
| `packages/fal/src/fal-image-model.test.ts` | should expose correct provider and model information | `fal_provider_creates_image_video_models_and_rejects_language_embedding_models` | none |
| `packages/fal/src/fal-image-model.test.ts` | should send edit request with files as data URI | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should send edit request with files and mask | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should send edit request with URL-based file | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should send edit request with base64 string data | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should warn when multiple files are provided | `fal_image_model_warns_when_multiple_images_are_not_enabled` | none |
| `packages/fal/src/fal-image-model.test.ts` | should send image_urls when useMultipleImages is true | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should not warn when multiple files provided with useMultipleImages | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should send single image as image_urls array when useMultipleImages is true | `fal_image_model_maps_edit_files_masks_urls_base64_and_multiple_images` | none |
| `packages/fal/src/fal-image-model.test.ts` | should allow imageUrl via provider options | `fal_image_model_converts_camel_case_provider_options_to_snake_case_for_api` | none |
| `packages/fal/src/fal-image-model.test.ts` | should allow maskUrl via provider options | `fal_image_model_converts_camel_case_provider_options_to_snake_case_for_api` | none |
| `packages/fal/src/fal-image-model.test.ts` | should parse single image response | `fal_image_model_parses_single_multiple_null_and_empty_metadata_responses` | none |
| `packages/fal/src/fal-image-model.test.ts` | should parse multiple images response | `fal_image_model_parses_single_multiple_null_and_empty_metadata_responses` | none |
| `packages/fal/src/fal-image-model.test.ts` | should handle empty timings object | `fal_image_model_parses_single_multiple_null_and_empty_metadata_responses` | none |
| `packages/fal/src/fal-provider.test.ts` | should construct an image model with default configuration | `fal_provider_creates_image_video_models_and_rejects_language_embedding_models` | none |
| `packages/fal/src/fal-provider.test.ts` | should respect custom configuration options | `fal_image_model_maps_request_downloads_metadata_and_warnings` | none |
| `packages/fal/src/fal-provider.test.ts` | should construct a video model with default configuration | `fal_provider_creates_image_video_models_and_rejects_language_embedding_models` | none |
| `packages/fal/src/fal-provider.test.ts` | should respect custom configuration options | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-provider.test.ts` | should support various video model IDs | `fal_provider_creates_image_video_models_and_rejects_language_embedding_models` | none |
| `packages/fal/src/fal-video-model.test.ts` | should expose correct provider and model information | `fal_provider_creates_image_video_models_and_rejects_language_embedding_models` | none |
| `packages/fal/src/fal-video-model.test.ts` | should support different model IDs | `fal_provider_creates_image_video_models_and_rejects_language_embedding_models` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass the correct parameters including prompt | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass seed when provided | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass aspect ratio when provided | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should convert duration to string format | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass headers | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should return video with correct data | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should return warnings array | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should include timestamp, headers and modelId in response | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should include video metadata | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should include seed when present | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should include timings when present | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should include has_nsfw_concepts when present | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should include prompt when present in response | `fal_video_model_maps_queue_response_and_metadata` | none |
| `packages/fal/src/fal-video-model.test.ts` | should send image_url with file data | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should send image_url with URL-based image | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass loop option | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass motionStrength as motion_strength | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass resolution option | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass negativePrompt as negative_prompt | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass promptOptimizer as prompt_optimizer | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should pass through additional options | `fal_video_model_maps_provider_options_poll_overrides_and_custom_passthrough` | none |
| `packages/fal/src/fal-video-model.test.ts` | should throw error when no request ID is returned | `fal_video_model_maps_missing_queue_response_missing_video_and_api_errors` | none |
| `packages/fal/src/fal-video-model.test.ts` | should throw error when no video URL in response | `fal_video_model_maps_missing_queue_response_missing_video_and_api_errors` | none |
| `packages/fal/src/fal-video-model.test.ts` | should handle API errors from queue endpoint | `fal_video_model_maps_missing_queue_response_missing_video_and_api_errors` | none |
| `packages/fal/src/fal-video-model.test.ts` | should poll until video is ready | `fal_video_model_polls_until_ready_and_times_out` | none |
| `packages/fal/src/fal-video-model.test.ts` | should timeout after pollTimeoutMs | `fal_video_model_polls_until_ready_and_times_out` | none |
| `packages/fal/src/fal-video-model.test.ts` | should respect abort signal | `fal_video_model_respects_abort_signal_during_polling` | none |
| `packages/fal/src/fal-video-model.test.ts` | should default to video/mp4 when content_type is not provided | `fal_video_model_polls_until_ready_and_times_out` | none |

## KlingAI Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/klingai/src/klingai-auth.test.ts` | should generate a valid JWT token structure | `klingai_auth_generates_valid_hs256_jwt_structure` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should include correct header with HS256 algorithm | `klingai_auth_generates_valid_hs256_jwt_structure` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should include issuer (iss) matching the access key | `klingai_auth_includes_issuer_exp_and_nbf_claims` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should include exp and nbf claims | `klingai_auth_includes_issuer_exp_and_nbf_claims` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should load access key from environment variable | `klingai_auth_loads_credentials_from_environment` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should prefer explicit accessKey over environment variable | `klingai_auth_prefers_explicit_credentials_over_environment` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should throw when access key is not available | `klingai_auth_reports_missing_explicit_credentials` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should throw when secret key is not available | `klingai_auth_reports_missing_explicit_credentials` | none |
| `packages/klingai/src/klingai-auth.test.ts` | should produce different tokens for different secret keys | `klingai_auth_signs_with_secret_and_changes_for_different_secret` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should construct a video model with default configuration | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should respect custom baseURL | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should pass custom fetch function | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should pass headers as async function | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should be an alias for video | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should throw NoSuchModelError for languageModel | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should throw NoSuchModelError for embeddingModel | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should throw NoSuchModelError for imageModel | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models; klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-provider.test.ts` | should have specificationVersion v4 | `none` | `type-system-impossible`: Rust provider-v4 support is expressed through `ProviderWithVideoModel` and `VideoModel` traits rather than a runtime `specificationVersion` string property. |
| `packages/klingai/src/klingai-video-model.test.ts` | should expose correct provider and model information | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should accept custom model IDs in constructor | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should throw NoSuchModelError for unknown model IDs on generate | `klingai_provider_creates_video_alias_trait_and_rejects_unsupported_models` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send correct request body with required fields | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send prompt when provided | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send image_url from URL-based image | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send image_url as base64 from file data | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send keep_original_sound when provided | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send watermark_info when watermarkEnabled is set | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should pass headers to requests | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should return video with correct URL and media type | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should return empty warnings for supported features | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn about unsupported aspectRatio | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn about unsupported resolution | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_motion_control_maps_required_provider_options_and_image_url; klingai_i2v_model_maps_file_image_and_aspect_ratio_warning` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn about unsupported seed | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_motion_control_maps_required_provider_options_and_image_url; klingai_i2v_model_maps_file_image_and_aspect_ratio_warning` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn about unsupported fps | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_motion_control_maps_required_provider_options_and_image_url; klingai_i2v_model_maps_file_image_and_aspect_ratio_warning` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn about unsupported duration | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_motion_control_maps_required_provider_options_and_image_url; klingai_i2v_model_maps_file_image_and_aspect_ratio_warning` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn when n > 1 | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_motion_control_maps_required_provider_options_and_image_url; klingai_i2v_model_maps_file_image_and_aspect_ratio_warning` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should not warn when n is 1 | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_motion_control_maps_required_provider_options_and_image_url; klingai_i2v_model_maps_file_image_and_aspect_ratio_warning` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should derive model_name kling-v3 for kling-v3.0-motion-control | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send element_list when provided for motion control | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send mode=pro when specified | `klingai_motion_control_maps_required_provider_options_and_image_url` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should POST to /v1/videos/text2video endpoint | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should GET from /v1/videos/text2video/{id} for polling | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send model_name derived from model ID | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should convert dots to hyphens in model_name | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should handle model IDs without dots | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send prompt in request body | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should map SDK aspectRatio to aspect_ratio | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should not warn about aspectRatio for T2V | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should map SDK duration to string | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should not warn about duration for T2V | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send negative_prompt when provided | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send sound when provided | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send cfg_scale when provided | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send camera_control when provided | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should derive model_name kling-v3 for kling-v3.0-t2v | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send multi_shot and shot_type when provided | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send multi_shot with intelligence shot_type | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send voice_list when provided for T2V | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send watermark_info when watermarkEnabled is set for T2V | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should not send element_list for T2V | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn when image is provided for T2V | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should not require motion-control provider options | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should return videos from successful T2V generation | `klingai_t2v_model_maps_body_headers_polling_and_metadata; klingai_t2v_model_maps_extended_provider_options_without_element_list` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should POST to /v1/videos/image2video endpoint | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should GET from /v1/videos/image2video/{id} for polling | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send model_name derived from model ID | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should convert dots to hyphens in I2V model_name | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send image from URL-based input | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send image as base64 from file data | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send image_tail when provided | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send prompt with image | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should map SDK duration to string for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should not warn about duration for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should warn about aspectRatio for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send static_mask when provided | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send dynamic_masks when provided | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should derive model_name kling-v3 for kling-v3.0-i2v | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send multi_shot and multi_prompt for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send element_list when provided for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send voice_list when provided for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send watermark_info when watermarkEnabled is set for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should send negative_prompt for I2V | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should return videos from successful I2V generation | `klingai_i2v_model_maps_file_image_and_aspect_ratio_warning; klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should include timestamp, headers, and modelId in response | `klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should include taskId and video metadata | `klingai_t2v_model_maps_body_headers_polling_and_metadata` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should throw when motion control provider options are missing required fields | `klingai_motion_control_requires_provider_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should throw when klingai provider options are missing entirely for motion control | `klingai_motion_control_requires_provider_options` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should throw when task status is failed | `klingai_video_model_maps_failed_missing_task_and_empty_video_errors` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should throw when no task_id is returned | `klingai_video_model_maps_failed_missing_task_and_empty_video_errors` | none |
| `packages/klingai/src/klingai-video-model.test.ts` | should throw when no videos in response | `klingai_video_model_maps_failed_missing_task_and_empty_video_errors` | none |

## Replicate Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/replicate/src/replicate-image-model.test.ts` | should pass the model and the settings | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0001_it_should_pass_the_model_and_the_settings` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should call the correct url | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0002_it_should_call_the_correct_url` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should pass headers and set the prefer header | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0003_it_should_pass_headers_and_set_the_prefer_header` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should set custom wait time in prefer header when maxWaitTimeInSeconds is specified | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0004_it_should_set_custom_wait_time_in_prefer_header_when_maxwaittimeinseconds_i` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should not include maxWaitTimeInSeconds in request body | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0005_it_should_not_include_maxwaittimeinseconds_in_request_body` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should extract the generated image from array response | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0006_it_should_extract_the_generated_image_from_array_response` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should extract the generated image from string response | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0007_it_should_extract_the_generated_image_from_string_response` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should return response metadata | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0008_it_should_return_response_metadata` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should include response headers in metadata | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0009_it_should_include_response_headers_in_metadata` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should set version in request body for versioned models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0010_it_should_set_version_in_request_body_for_versioned_models` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should send image when URL file is provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0011_it_should_send_image_when_url_file_is_provided` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should convert Uint8Array file to data URI | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0012_it_should_convert_uint8array_file_to_data_uri` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should convert file with base64 string data to data URI | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0013_it_should_convert_file_with_base64_string_data_to_data_uri` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should send mask for inpainting | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0014_it_should_send_mask_for_inpainting` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should warn when multiple files are provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0015_it_should_warn_when_multiple_files_are_provided` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should pass provider options with image editing | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0016_it_should_pass_provider_options_with_image_editing` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should report maxImagesPerCall as 8 for Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0017_it_should_report_maximagespercall_as_8_for_flux_2_models` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should report maxImagesPerCall as 1 for non-Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0018_it_should_report_maximagespercall_as_1_for_non_flux_2_models` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should send single image as input_image for Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0019_it_should_send_single_image_as_input_image_for_flux_2_models` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should send multiple images as input_image, input_image_2, etc. for Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0020_it_should_send_multiple_images_as_input_image_input_image_2_etc_for_flux_2` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should warn when more than 8 images are provided for Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0021_it_should_warn_when_more_than_8_images_are_provided_for_flux_2_models` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should warn and ignore mask for Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0022_it_should_warn_and_ignore_mask_for_flux_2_models` | none |
| `packages/replicate/src/replicate-image-model.test.ts` | should call correct URL for Flux-2 models | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0023_it_should_call_correct_url_for_flux_2_models` | none |
| `packages/replicate/src/replicate-provider.test.ts` | creates a provider with required settings | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0024_it_creates_a_provider_with_required_settings` | none |
| `packages/replicate/src/replicate-provider.test.ts` | creates a provider with custom settings | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0025_it_creates_a_provider_with_custom_settings` | none |
| `packages/replicate/src/replicate-provider.test.ts` | creates an image model instance | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0026_it_creates_an_image_model_instance` | none |
| `packages/replicate/src/replicate-provider.test.ts` | creates a video model instance | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0027_it_creates_a_video_model_instance` | none |
| `packages/replicate/src/replicate-provider.test.ts` | uses custom baseURL for video model when provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0028_it_uses_custom_baseurl_for_video_model_when_provided` | none |
| `packages/replicate/src/replicate-provider.test.ts` | passes custom fetch to video model | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0029_it_passes_custom_fetch_to_video_model` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should expose correct provider and model information | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0030_it_should_expose_correct_provider_and_model_information` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should support model IDs with versions | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0031_it_should_support_model_ids_with_versions` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass the correct parameters including prompt | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0032_it_should_pass_the_correct_parameters_including_prompt` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should use /models/{modelId}/predictions for models without version | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0033_it_should_use_models_modelid_predictions_for_models_without_version` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should use /predictions with version for models with version | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0034_it_should_use_predictions_with_version_for_models_with_version` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass seed when provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0035_it_should_pass_seed_when_provided` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass aspect ratio when provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0036_it_should_pass_aspect_ratio_when_provided` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass through 9:16 aspect ratio | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0037_it_should_pass_through_9_16_aspect_ratio` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass through 1:1 aspect ratio | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0038_it_should_pass_through_1_1_aspect_ratio` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass through other aspect ratios | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0039_it_should_pass_through_other_aspect_ratios` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass resolution as size when provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0040_it_should_pass_resolution_as_size_when_provided` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass duration when provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0041_it_should_pass_duration_when_provided` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass fps when provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0042_it_should_pass_fps_when_provided` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should return video with correct data | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0043_it_should_return_video_with_correct_data` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should return warnings array | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0044_it_should_return_warnings_array` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should include timestamp and modelId in response | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0045_it_should_include_timestamp_and_modelid_in_response` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should include prediction metadata | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0046_it_should_include_prediction_metadata` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should send URL-based image directly | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0047_it_should_send_url_based_image_directly` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should convert base64 image to data URI | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0048_it_should_convert_base64_image_to_data_uri` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass guidance_scale option | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0049_it_should_pass_guidance_scale_option` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass num_inference_steps option | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0050_it_should_pass_num_inference_steps_option` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass motion_bucket_id for Stable Video Diffusion | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0051_it_should_pass_motion_bucket_id_for_stable_video_diffusion` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass prompt_optimizer for MiniMax | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0052_it_should_pass_prompt_optimizer_for_minimax` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should pass through custom options | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0053_it_should_pass_through_custom_options` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should use maxWaitTimeInSeconds in prefer header | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0054_it_should_use_maxwaittimeinseconds_in_prefer_header` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should use prefer: wait when maxWaitTimeInSeconds not provided | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0055_it_should_use_prefer_wait_when_maxwaittimeinseconds_not_provided` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should throw error when prediction fails | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0056_it_should_throw_error_when_prediction_fails` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should throw error when prediction is canceled | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0057_it_should_throw_error_when_prediction_is_canceled` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should throw error when no video URL in response | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0058_it_should_throw_error_when_no_video_url_in_response` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should poll until prediction is done | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0059_it_should_poll_until_prediction_is_done` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should timeout after pollTimeoutMs | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0060_it_should_timeout_after_polltimeoutms` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should respect abort signal | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0061_it_should_respect_abort_signal` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should handle immediate success (pollsUntilDone=0) | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0062_it_should_handle_immediate_success_pollsuntildone_0` | none |
| `packages/replicate/src/replicate-video-model.test.ts` | should always return video/mp4 as media type | `crates/ai-sdk-replicate/tests/upstream_mapping.rs::replicate_0063_it_should_always_return_video_mp4_as_media_type` | none |

