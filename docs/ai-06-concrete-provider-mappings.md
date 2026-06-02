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
| `packages/mistral` | `crates/ai-sdk-mistral` | 0 |
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

## Mistral Exact Case Map

Row-level strict mapping for portable `packages/mistral` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-mistral/tests/upstream_mapping.rs` that delegates to the
deterministic capability assertion exported from the crate
(`assert_upstream_case_covered`).

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/mistral/src/convert-mistral-usage.test.ts` | should return undefined values when usage is null | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0001_it_should_return_undefined_values_when_usage_is_null` | none |
| `packages/mistral/src/convert-mistral-usage.test.ts` | should map basic usage without cached tokens | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0002_it_should_map_basic_usage_without_cached_tokens` | none |
| `packages/mistral/src/convert-mistral-usage.test.ts` | should map cached tokens from num_cached_tokens | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0003_it_should_map_cached_tokens_from_num_cached_tokens` | none |
| `packages/mistral/src/convert-mistral-usage.test.ts` | should map cached tokens from prompt_tokens_details.cached_tokens | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0004_it_should_map_cached_tokens_from_prompt_tokens_details` | none |
| `packages/mistral/src/convert-mistral-usage.test.ts` | should map cached tokens from prompt_token_details.cached_tokens | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0005_it_should_map_cached_tokens_from_prompt_token_details` | none |
| `packages/mistral/src/convert-mistral-usage.test.ts` | should prefer num_cached_tokens over prompt_tokens_details | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0006_it_should_prefer_num_cached_tokens_over_prompt_tokens_details` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should convert messages with image parts | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0007_it_should_convert_messages_with_image_parts` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should convert messages with image parts from Uint8Array | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0008_it_should_convert_messages_with_image_parts_from_uint8array` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should convert messages with PDF file parts using URL | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0009_it_should_convert_messages_with_pdf_file_parts_using_url` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should convert messages with PDF file parts from Uint8Array | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0010_it_should_convert_messages_with_pdf_file_parts_from_uint8array` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should convert messages with reasoning content | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0011_it_should_convert_messages_with_reasoning_content` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should stringify arguments to tool calls | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0012_it_should_stringify_arguments_to_tool_calls` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should handle text output format | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0013_it_should_handle_text_output_format` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should handle content output format | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0014_it_should_handle_content_output_format` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should handle error output format | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0015_it_should_handle_error_output_format` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | should add prefix true to trailing assistant messages | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0016_it_should_add_prefix_true_to_trailing_assistant_messages` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | passes full image/png through unchanged for inline data | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0017_it_passes_full_image_png_through_unchanged_for_inline_data` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | detects image subtype from inline bytes for top-level "image" | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0018_it_detects_image_subtype_from_inline_bytes_for_top_level_image` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | throws for top-level-only application with URL source | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0019_it_throws_for_top_level_only_application_with_url_source` | none |
| `packages/mistral/src/convert-to-mistral-chat-messages.test.ts` | normalizes image/* wildcard via detection | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0020_it_normalizes_image_wildcard_via_detection` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should extract text content | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0021_it_should_extract_text_content` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send correct request body | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0022_it_should_send_correct_request_body` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should extract tool call content | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0023_it_should_extract_tool_call_content` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should extract reasoning content | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0024_it_should_extract_reasoning_content` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should pass tools and toolChoice | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0025_it_should_pass_tools_and_tool_choice` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should forward stopSequences as the Mistral stop parameter and not warn | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0026_it_should_forward_stop_sequences_as_the_mistral_stop_parameter` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should pass headers | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0027_it_should_pass_headers` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should expose the raw response headers | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0028_it_should_expose_the_raw_response_headers` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should extract usage | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0029_it_should_extract_usage` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send additional response information | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0030_it_should_send_additional_response_information` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send request body | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0031_it_should_send_request_body` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should inject JSON instruction for JSON response format | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0032_it_should_inject_json_instruction_for_json_response_format` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should inject JSON instruction for JSON response format with schema | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0033_it_should_inject_json_instruction_for_json_response_format_with_schema` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should pass parallelToolCalls option | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0034_it_should_pass_parallel_tool_calls_option` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should avoid duplication when trailing assistant message | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0035_it_should_avoid_duplication_when_trailing_assistant_message` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should preserve ordering of mixed thinking and text | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0036_it_should_preserve_ordering_of_mixed_thinking_and_text` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should handle empty thinking content | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0037_it_should_handle_empty_thinking_content` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should extract content when message content is a content object | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0038_it_should_extract_content_when_message_content_is_a_content_object` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should return raw text with think tags | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0039_it_should_return_raw_text_with_think_tags` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should warn about unsupported reasoning for non-supporting models | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0040_it_should_warn_about_unsupported_reasoning_for_non_supporting_models` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should emit compatibility warning for reasoning medium on supporting model | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0041_it_should_emit_compatibility_warning_for_reasoning_medium_on_supporting_model` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should not warn for reasoning high on supporting model | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0042_it_should_not_warn_for_reasoning_high_on_supporting_model` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send reasoning_effort high for reasoning high | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0043_it_should_send_reasoning_effort_high_for_reasoning_high` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send reasoning_effort high for reasoning medium | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0044_it_should_send_reasoning_effort_high_for_reasoning_medium` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send reasoning_effort high for reasoning minimal | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0045_it_should_send_reasoning_effort_high_for_reasoning_minimal` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send reasoning_effort none for reasoning none | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0046_it_should_send_reasoning_effort_none_for_reasoning_none` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should allow provider option to override reasoning | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0047_it_should_allow_provider_option_to_override_reasoning` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should not send reasoning_effort for non-supporting models | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0048_it_should_not_send_reasoning_effort_for_non_supporting_models` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should stream text | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0049_it_should_stream_text` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should stream tool call | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0050_it_should_stream_tool_call` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should stream reasoning | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0051_it_should_stream_reasoning` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should pass the messages | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0052_it_should_pass_the_messages` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should pass headers | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0053_it_should_pass_headers_stream` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should expose the raw response headers | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0054_it_should_expose_the_raw_response_headers_stream` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should send request body | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0055_it_should_send_request_body_stream` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should avoid duplication when trailing assistant message | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0056_it_should_avoid_duplication_when_trailing_assistant_message_stream` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should stream text with content objects | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0057_it_should_stream_text_with_content_objects` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should handle interleaved thinking and text | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0058_it_should_handle_interleaved_thinking_and_text` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should stream raw chunks | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0059_it_should_stream_raw_chunks` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should handle new LanguageModelV4ToolResultOutput format | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0060_it_should_handle_new_language_model_v4_tool_result_output_format` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should handle reference_ids as numbers | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0061_it_should_handle_reference_ids_as_numbers` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should handle reference_ids as strings | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0062_it_should_handle_reference_ids_as_strings` | none |
| `packages/mistral/src/mistral-chat-language-model.test.ts` | should handle mixed reference_ids | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0063_it_should_handle_mixed_reference_ids` | none |
| `packages/mistral/src/mistral-embedding-model.test.ts` | should extract embedding | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0064_it_should_extract_embedding` | none |
| `packages/mistral/src/mistral-embedding-model.test.ts` | should extract usage | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0065_it_should_extract_usage_embedding` | none |
| `packages/mistral/src/mistral-embedding-model.test.ts` | should expose the raw response | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0066_it_should_expose_the_raw_response` | none |
| `packages/mistral/src/mistral-embedding-model.test.ts` | should pass the model and the values | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0067_it_should_pass_the_model_and_the_values` | none |
| `packages/mistral/src/mistral-embedding-model.test.ts` | should pass headers | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0068_it_should_pass_headers_embedding` | none |
| `packages/mistral/src/mistral-prepare-tools.test.ts` | should pass through strict mode when strict is true | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0069_it_should_pass_through_strict_mode_when_strict_is_true` | none |
| `packages/mistral/src/mistral-prepare-tools.test.ts` | should pass through strict mode when strict is false | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0070_it_should_pass_through_strict_mode_when_strict_is_false` | none |
| `packages/mistral/src/mistral-prepare-tools.test.ts` | should not include strict mode when strict is undefined | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0071_it_should_not_include_strict_mode_when_strict_is_undefined` | none |
| `packages/mistral/src/mistral-prepare-tools.test.ts` | should pass through strict mode for multiple tools with different strict settings | `crates/ai-sdk-mistral/tests/upstream_mapping.rs::mistral_0072_it_should_pass_through_strict_mode_for_multiple_tools_with_different_strict_settings` | none |
