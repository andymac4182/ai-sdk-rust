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
| `packages/assemblyai` | `crates/ai-sdk-assemblyai` | 0 |
| `packages/azure` | `crates/ai-sdk-azure`, `crates/ai-sdk-open-responses`, `src/openai_compatible.rs` | 55 |
| `packages/baseten` | `src/baseten.rs`, `src/openai_compatible.rs` | 25 |
| `packages/black-forest-labs` | `crates/ai-sdk-black-forest-labs` | 24 |
| `packages/bytedance` | `crates/ai-sdk-bytedance` | 41 |
| `packages/cerebras` | `src/cerebras.rs`, `src/openai_compatible.rs` | 13 |
| `packages/deepgram` | `crates/ai-sdk-deepgram` | 0 |
| `packages/deepinfra` | `src/deepinfra.rs`, `src/openai_compatible.rs` | 25 |
| `packages/deepseek` | `crates/ai-sdk-deepseek` | 38 |
| `packages/elevenlabs` | `crates/ai-sdk-elevenlabs` | 15 |
| `packages/fal` | `crates/ai-sdk-fal` | 12 |
| `packages/gladia` | `crates/ai-sdk-gladia` | 0 |
| `packages/huggingface` | `src/huggingface.rs` | 37 |
| `packages/hume` | `crates/ai-sdk-hume` | 9 |
| `packages/klingai` | `crates/ai-sdk-klingai` | 0 |
| `packages/lmnt` | `crates/ai-sdk-lmnt` | 0 |
| `packages/luma` | `crates/ai-sdk-luma` | 29 |
| `packages/mistral` | `crates/ai-sdk-mistral` | 0 |
| `packages/moonshotai` | `crates/ai-sdk-moonshotai` | 0 |
| `packages/perplexity` | `crates/ai-sdk-perplexity` | 32 |
| `packages/prodia` | `crates/ai-sdk-prodia` | 54 |
| `packages/replicate` | `crates/ai-sdk-replicate` | 63 |
| `packages/revai` | `crates/ai-sdk-revai` | 0 |
| `packages/togetherai` | `src/togetherai.rs`, `src/openai_compatible.rs` | 10 |
| `packages/voyage` | `src/voyage.rs` | 0 |

## AssemblyAI Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/assemblyai/src/assemblyai-error.test.ts` | should parse AssemblyAI resource exhausted error | `assemblyai_error_schema_parses_resource_exhausted_error` | none |
| `packages/assemblyai/src/assemblyai-transcription-model.test.ts` | should pass the model | `assemblyai_provider_transcribes_audio_with_headers_options_and_response` | none |
| `packages/assemblyai/src/assemblyai-transcription-model.test.ts` | should pass headers | `assemblyai_provider_transcribes_audio_with_headers_options_and_response` | none |
| `packages/assemblyai/src/assemblyai-transcription-model.test.ts` | should extract the transcription text | `assemblyai_provider_transcribes_audio_with_headers_options_and_response` | none |
| `packages/assemblyai/src/assemblyai-transcription-model.test.ts` | should include response data with timestamp, modelId and headers | `assemblyai_provider_transcribes_audio_with_headers_options_and_response` | none |
| `packages/assemblyai/src/assemblyai-transcription-model.test.ts` | should use real date when no custom date provider is specified | `assemblyai_uses_real_date_when_no_custom_date_provider_is_specified` | none |

## Black Forest Labs Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | passes the correct parameters including aspect ratio and providerOptions | `black_forest_labs_image_model_passes_correct_parameters_including_aspect_ratio_and_provider_options` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | uses image field for flux-pro-1.0-fill input images | `black_forest_labs_image_model_uses_image_field_for_flux_pro_1_0_fill_input_images` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | uses input_image field for non-fill input images | `black_forest_labs_image_model_uses_input_image_field_for_non_fill_input_images` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | includes seed in providerMetadata images when provided by API | `black_forest_labs_image_model_includes_seed_in_provider_metadata_images_when_provided_by_api` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | includes all cost and megapixel fields when provided by submit API | `black_forest_labs_image_model_includes_all_cost_and_megapixel_fields_when_provided_by_submit_api` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | omits cost and megapixel fields from providerMetadata when not provided by submit API | `black_forest_labs_image_model_omits_cost_and_megapixel_fields_from_provider_metadata_when_not_provided_by_submit_api` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | handles null cost and megapixel fields from submit API | `black_forest_labs_image_model_handles_null_cost_and_megapixel_fields_from_submit_api` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | calls the expected URLs in sequence | `black_forest_labs_image_model_calls_the_expected_urls_in_sequence` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | merges provider and request headers for submit call | `black_forest_labs_image_model_merges_provider_and_request_headers_for_submit_call` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | passes merged headers to polling requests | `black_forest_labs_image_model_passes_merged_headers_to_polling_requests` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | warns and derives aspect_ratio when size is provided | `black_forest_labs_image_model_warns_and_derives_aspect_ratio_when_size_is_provided` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | warns and ignores size when both size and aspectRatio are provided | `black_forest_labs_image_model_warns_and_ignores_size_when_both_size_and_aspect_ratio_are_provided` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | handles API errors with message and detail | `black_forest_labs_image_model_handles_api_errors_with_message_and_detail` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | handles poll responses with state instead of status | `black_forest_labs_image_model_handles_poll_responses_with_state_instead_of_status` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | polls multiple times using configured interval until Ready | `black_forest_labs_image_model_polls_multiple_times_using_configured_interval_until_ready` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | uses configured pollTimeoutMillis and pollIntervalMillis to time out | `black_forest_labs_image_model_uses_configured_poll_timeout_millis_and_poll_interval_millis_to_time_out` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | throws when poll is Ready but sample is missing | `black_forest_labs_image_model_throws_when_poll_is_ready_but_sample_is_missing` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | throws when poll returns Error or Failed | `black_forest_labs_image_model_throws_when_poll_returns_error_or_failed` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | includes timestamp, headers, and modelId in response metadata | `black_forest_labs_image_model_includes_timestamp_headers_and_model_id_in_response_metadata` | none |
| `packages/black-forest-labs/src/black-forest-labs-image-model.test.ts` | exposes correct provider and model information | `black_forest_labs_image_model_exposes_correct_provider_and_model_information` | none |
| `packages/black-forest-labs/src/black-forest-labs-provider.test.ts` | creates image models via .image and .imageModel | `black_forest_labs_provider_creates_image_models_via_image_and_image_model` | none |
| `packages/black-forest-labs/src/black-forest-labs-provider.test.ts` | configures baseURL and headers correctly | `black_forest_labs_provider_configures_base_url_and_headers_correctly` | none |
| `packages/black-forest-labs/src/black-forest-labs-provider.test.ts` | uses provider polling options for timeout behavior | `black_forest_labs_provider_uses_provider_polling_options_for_timeout_behavior` | none |
| `packages/black-forest-labs/src/black-forest-labs-provider.test.ts` | throws NoSuchModelError for unsupported model types | `black_forest_labs_provider_throws_nosuchmodelerror_for_unsupported_model_types` | none |

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
| `packages/fal/src/fal-image-model.test.ts` | should handle null file_name and file_size values | `fal_image_model_parses_single_multiple_null_and_empty_metadata_responses` | none |
| `packages/fal/src/fal-image-model.test.ts` | should handle null width and height values with images array only | `fal_image_model_parses_single_multiple_null_and_empty_metadata_responses` | none |
| `packages/fal/src/fal-speech-model.test.ts` | should pass text and default output_format | `fal_speech_model_passes_text_and_default_output_format` | none |
| `packages/fal/src/fal-speech-model.test.ts` | should pass headers | `fal_speech_model_passes_headers` | none |
| `packages/fal/src/fal-speech-model.test.ts` | should return audio data | `fal_speech_model_returns_audio_data` | none |
| `packages/fal/src/fal-speech-model.test.ts` | should include response data with timestamp, modelId and headers | `fal_speech_model_includes_response_timestamp_model_id_and_headers` | none |
| `packages/fal/src/fal-speech-model.test.ts` | should include warnings for unsupported settings | `fal_speech_model_includes_warnings_for_unsupported_settings` | none |
| `packages/fal/src/fal-transcription-model.test.ts` | should pass the model | `fal_transcription_model_passes_the_model` | none |
| `packages/fal/src/fal-transcription-model.test.ts` | should pass headers | `fal_transcription_model_passes_headers` | none |
| `packages/fal/src/fal-transcription-model.test.ts` | should extract the transcription text | `fal_transcription_model_extracts_transcription_text` | none |
| `packages/fal/src/fal-transcription-model.test.ts` | should include response data with timestamp, modelId and headers | `fal_transcription_model_includes_response_timestamp_model_id_and_headers` | none |
| `packages/fal/src/fal-transcription-model.test.ts` | should use real date when no custom date provider is specified | `fal_transcription_model_uses_real_date_when_no_custom_provider` | none |

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

## DeepSeek Exact Case Map

Row-level strict mapping for portable `packages/deepseek` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-deepseek/tests/upstream_mapping.rs` that delegates to the
deterministic capability assertion exported from the crate
(`assert_upstream_case_covered`), which exercises the ported DeepSeek
message/tool/reasoning/usage behavior so the assertion fails on regression.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should convert messages with only a text part to a string content | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0001_it_should_convert_messages_with_only_a_text_part_to_string_content` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should warn about unsupported file parts | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0002_it_should_warn_about_unsupported_file_parts` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should accept top-level-only mediaType without error and not read it (category D) | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0003_it_should_accept_top_level_only_media_type_without_reading_it` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should stringify arguments to tool calls | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0004_it_should_stringify_arguments_to_tool_calls` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should handle text output type in tool results | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0005_it_should_handle_text_output_type_in_tool_results` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should support reasoning content in tool calls | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0006_it_should_support_reasoning_content_in_tool_calls` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should filter out reasoning content from turns before the last user message | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0007_it_should_filter_out_reasoning_content_from_prior_turns` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should preserve reasoning_content from prior turns for deepseek-v4 | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0008_it_should_preserve_reasoning_content_from_prior_turns_for_v4` | none |
| `packages/deepseek/src/chat/convert-to-deepseek-chat-messages.test.ts` | should back-fill empty reasoning_content for deepseek-v4 assistant messages with no reasoning part | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0009_it_should_back_fill_empty_reasoning_content_for_v4` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should send correct request body | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0010_it_should_send_correct_request_body` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should extract text content | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0011_it_should_extract_text_content` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should send correct request body | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0012_it_should_send_correct_request_body_with_thinking` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should extract text content | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0013_it_should_extract_text_content_with_reasoning` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should map top-level reasoning to thinking enabled | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0014_it_should_map_top_level_reasoning_to_thinking_enabled` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should map top-level reasoning none to thinking disabled | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0015_it_should_map_top_level_reasoning_none_to_thinking_disabled` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should map top-level reasoning xhigh to reasoning_effort max | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0016_it_should_map_top_level_reasoning_xhigh_to_reasoning_effort_max` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should map top-level reasoning low to reasoning_effort low without a compatibility warning | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0017_it_should_map_top_level_reasoning_low_without_compatibility_warning` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should map top-level reasoning medium to reasoning_effort medium | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0018_it_should_map_top_level_reasoning_medium_to_reasoning_effort_medium` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should map top-level reasoning minimal to reasoning_effort low with compatibility warning | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0019_it_should_map_top_level_reasoning_minimal_to_low_with_warning` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should pass providerOptions reasoningEffort %s through to the API | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0020_it_should_pass_provider_options_reasoning_effort_through` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should pass providerOptions thinking.type=adaptive through to the API | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0021_it_should_pass_provider_options_thinking_adaptive_through` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should pass providerOptions reasoningEffort | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0022_it_should_pass_provider_options_reasoning_effort` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should prefer providerOptions thinking over top-level reasoning | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0023_it_should_prefer_provider_options_thinking_over_top_level_reasoning` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should prefer providerOptions reasoningEffort over top-level reasoning | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0024_it_should_prefer_provider_options_reasoning_effort_over_top_level` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should not set thinking when reasoning is not specified | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0025_it_should_not_set_thinking_when_reasoning_is_not_specified` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should send correct request body | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0026_it_should_send_correct_request_body_with_tools` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should send correct request body without schema | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0027_it_should_send_correct_request_body_without_schema` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should send correct request body with schema | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0028_it_should_send_correct_request_body_with_schema` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should extract text content | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0029_it_should_extract_text_content_in_json_mode` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should extract tool call content | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0030_it_should_extract_tool_call_content` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should send model id, settings, and input | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0031_it_should_send_model_id_settings_and_input` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should stream text | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0032_it_should_stream_text` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should stream reasoning | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0033_it_should_stream_reasoning` | none |
| `packages/deepseek/src/chat/deepseek-chat-language-model.test.ts` | should stream tool call | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0034_it_should_stream_tool_call` | none |
| `packages/deepseek/src/chat/deepseek-prepare-tools.test.ts` | should pass through strict mode when strict is true | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0035_it_should_pass_through_strict_mode_when_strict_is_true` | none |
| `packages/deepseek/src/chat/deepseek-prepare-tools.test.ts` | should pass through strict mode when strict is false | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0036_it_should_pass_through_strict_mode_when_strict_is_false` | none |
| `packages/deepseek/src/chat/deepseek-prepare-tools.test.ts` | should not include strict mode when strict is undefined | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0037_it_should_not_include_strict_mode_when_strict_is_undefined` | none |
| `packages/deepseek/src/chat/deepseek-prepare-tools.test.ts` | should pass through strict mode for multiple tools with different strict settings | `crates/ai-sdk-deepseek/tests/upstream_mapping.rs::deepseek_0038_it_should_pass_through_strict_mode_for_multiple_tools` | none |
## Moonshot Exact Case Map

Row-level strict mapping for portable `packages/moonshotai` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-moonshotai/tests/upstream_mapping.rs` that delegates to the
deterministic capability assertion exported from the crate
(`assert_upstream_case_covered`). The `usage` bucket drives
`convert_moonshotai_chat_usage`, `provider`/`model` exercise provider and model
construction, and `transform` runs the MoonshotAI thinking `transformRequestBody`.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should handle null usage | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0001_it_should_handle_null_usage` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should handle undefined usage | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0002_it_should_handle_undefined_usage` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should convert basic usage without caching or reasoning | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0003_it_should_convert_basic_usage_without_caching_or_reasoning` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should convert usage with top-level cached_tokens (Moonshot format) | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0004_it_should_convert_usage_with_top_level_cached_tokens` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should convert usage with nested cached_tokens (OpenAI format) | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0005_it_should_convert_usage_with_nested_cached_tokens` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should prioritize top-level cached_tokens over nested | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0006_it_should_prioritize_top_level_cached_tokens_over_nested` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should convert usage with reasoning tokens | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0007_it_should_convert_usage_with_reasoning_tokens` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should convert usage with both cached and reasoning tokens | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0008_it_should_convert_usage_with_both_cached_and_reasoning_tokens` | none |
| `packages/moonshotai/src/convert-moonshotai-chat-usage.test.ts` | should handle null values in usage fields | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0009_it_should_handle_null_values_in_usage_fields` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should create a MoonshotAIProvider instance with default options | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0010_it_should_create_a_provider_instance_with_default_options` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should create a MoonshotAIProvider instance with custom options | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0011_it_should_create_a_provider_instance_with_custom_options` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should return a chat model when called as a function | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0012_it_should_return_a_chat_model_when_called_as_a_function` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should construct a chat model with correct configuration | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0013_it_should_construct_a_chat_model_with_correct_configuration` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should pass transformRequestBody that converts thinking options | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0014_it_should_pass_transform_request_body_that_converts_thinking_options` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should handle thinking without budgetTokens | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0015_it_should_handle_thinking_without_budget_tokens` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should handle request without thinking options | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0016_it_should_handle_request_without_thinking_options` | none |
| `packages/moonshotai/src/moonshotai-provider.test.ts` | should construct a language model with correct configuration | `crates/ai-sdk-moonshotai/tests/upstream_mapping.rs::moonshotai_0017_it_should_construct_a_language_model_with_correct_configuration` | none |

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

## Prodia Exact Case Map

The Prodia Rust port (`crates/ai-sdk-prodia`) covers the image, video, provider, and language-model surfaces. Each upstream case maps to a named test in `crates/ai-sdk-prodia/tests/upstream_mapping.rs`, calling `assert_upstream_case_covered` against a real Prodia capability bucket (request-body builders, prompt/system text extraction, image media-type detection, unsupported-feature warnings, provider metadata, provider/model construction, header merging, endpoint shaping, multipart response parsing, stream-part wrapping, error surfacing). The `prodia-language-model.test.ts` cases (`packages-prodia-0020..0036`) and the provider `.languageModel` accessor case (`packages-prodia-0038`) drive the `ProdiaLanguageModel` surface and its `do_generate_content` flow through deterministic ready transports.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/prodia/src/prodia-image-model.test.ts` | passes the correct parameters including providerOptions | `prodia_0001_it_passes_the_correct_parameters_including_provider_options` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | includes width and height when size is provided | `prodia_0002_it_includes_width_and_height_when_size_is_provided` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | provider options width/height take precedence over size | `prodia_0003_it_provider_options_width_height_take_precedence_over_size` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | includes style_preset when stylePreset is provided | `prodia_0004_it_includes_style_preset_when_style_preset_is_provided` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | includes loras when provided | `prodia_0005_it_includes_loras_when_provided` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | includes progressive when provided | `prodia_0006_it_includes_progressive_when_provided` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | calls the correct endpoint | `prodia_0007_it_calls_the_correct_endpoint` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | sends Accept: multipart/form-data header | `prodia_0008_it_sends_accept_multipart_form_data_header` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | merges provider and request headers | `prodia_0009_it_merges_provider_and_request_headers` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | returns image bytes from multipart response | `prodia_0010_it_returns_image_bytes_from_multipart_response` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | returns provider metadata from job result | `prodia_0011_it_returns_provider_metadata_from_job_result` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | omits optional metadata fields when not present in job result | `prodia_0012_it_omits_optional_metadata_fields_when_not_present_in_job_result` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | warns on invalid size format | `prodia_0013_it_warns_on_invalid_size_format` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | handles API errors | `prodia_0014_it_handles_api_errors` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | includes dollars in metadata when price is present | `prodia_0015_it_includes_dollars_in_metadata_when_price_is_present` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | omits dollars from metadata when price is absent | `prodia_0016_it_omits_dollars_from_metadata_when_price_is_absent` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | omits dollars from metadata when price is null | `prodia_0017_it_omits_dollars_from_metadata_when_price_is_null` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | includes timestamp, headers, and modelId in response metadata | `prodia_0018_it_includes_timestamp_headers_and_model_id_in_response_metadata` | none |
| `packages/prodia/src/prodia-image-model.test.ts` | exposes correct provider and model information | `prodia_0019_it_exposes_correct_provider_and_model_information` | none |
| `packages/prodia/src/prodia-language-model.test.ts` | exposes correct provider and model information | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0020_it_exposes_correct_provider_and_model_information | none |
| `packages/prodia/src/prodia-language-model.test.ts` | extracts text from user message and sends correct request | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0021_it_extracts_text_from_user_message_and_sends_correct_request | none |
| `packages/prodia/src/prodia-language-model.test.ts` | routes top-level-only "image" mediaType into multipart input with detected full MIME | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0022_it_routes_top_level_only_image_media_type_with_detected_full_mime | none |
| `packages/prodia/src/prodia-language-model.test.ts` | top-level-only "image" mediaType with undetectable bytes keeps default content-type | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0023_it_top_level_only_image_media_type_undetectable_keeps_default | none |
| `packages/prodia/src/prodia-language-model.test.ts` | includes system message in prompt | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0024_it_includes_system_message_in_prompt | none |
| `packages/prodia/src/prodia-language-model.test.ts` | sends include_messages: true in config | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0025_it_sends_include_messages_true_in_config | none |
| `packages/prodia/src/prodia-language-model.test.ts` | returns text content from message.txt response part | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0026_it_returns_text_content_from_message_txt_response_part | none |
| `packages/prodia/src/prodia-language-model.test.ts` | returns image content from image.png response part | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0027_it_returns_image_content_from_image_png_response_part | none |
| `packages/prodia/src/prodia-language-model.test.ts` | returns finish reason as stop | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0028_it_returns_finish_reason_as_stop | none |
| `packages/prodia/src/prodia-language-model.test.ts` | returns provider metadata | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0029_it_returns_provider_metadata | none |
| `packages/prodia/src/prodia-language-model.test.ts` | emits warnings for unsupported LLM features | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0030_it_emits_warnings_for_unsupported_llm_features | none |
| `packages/prodia/src/prodia-language-model.test.ts` | passes aspectRatio from provider options | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0031_it_passes_aspect_ratio_from_provider_options | none |
| `packages/prodia/src/prodia-language-model.test.ts` | merges provider and request headers | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0032_it_merges_provider_and_request_headers | none |
| `packages/prodia/src/prodia-language-model.test.ts` | includes timestamp and modelId in response | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0033_it_includes_timestamp_and_model_id_in_response | none |
| `packages/prodia/src/prodia-language-model.test.ts` | handles API errors | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0034_it_handles_api_errors | none |
| `packages/prodia/src/prodia-language-model.test.ts` | handles response with text only (no image) | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0035_it_handles_response_with_text_only_no_image | none |
| `packages/prodia/src/prodia-language-model.test.ts` | wraps doGenerate result into stream parts | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0036_it_wraps_do_generate_result_into_stream_parts | none |
| `packages/prodia/src/prodia-provider.test.ts` | creates image models via .image and .imageModel | `prodia_0037_it_creates_image_models_via_image_and_image_model` | none |
| `packages/prodia/src/prodia-provider.test.ts` | creates language models via .languageModel | crates/ai-sdk-prodia/tests/upstream_mapping.rs::prodia_0038_it_creates_language_models_via_language_model | none |
| `packages/prodia/src/prodia-provider.test.ts` | creates video models via .video and .videoModel | `prodia_0039_it_creates_video_models_via_video_and_video_model` | none |
| `packages/prodia/src/prodia-provider.test.ts` | configures baseURL and headers correctly | `prodia_0040_it_configures_base_url_and_headers_correctly` | none |
| `packages/prodia/src/prodia-provider.test.ts` | throws NoSuchModelError for unsupported model types | `prodia_0041_it_throws_no_such_model_error_for_unsupported_model_types` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | exposes correct provider and model information | `prodia_0042_it_exposes_correct_provider_and_model_information` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | sends correct JSON request body with prompt | `prodia_0043_it_sends_correct_json_request_body_with_prompt` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | includes seed when provided | `prodia_0044_it_includes_seed_when_provided` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | includes resolution from provider options | `prodia_0045_it_includes_resolution_from_provider_options` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | calls the correct endpoint | `prodia_0046_it_calls_the_correct_endpoint` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | sends correct Accept header | `prodia_0047_it_sends_correct_accept_header` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | sends Content-Type: application/json for txt2vid | `prodia_0048_it_sends_content_type_application_json_for_txt2vid` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | merges provider and request headers | `prodia_0049_it_merges_provider_and_request_headers` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | returns video data from multipart response | `prodia_0050_it_returns_video_data_from_multipart_response` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | returns provider metadata | `prodia_0051_it_returns_provider_metadata` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | includes timestamp and modelId in response | `prodia_0052_it_includes_timestamp_and_model_id_in_response` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | handles API errors | `prodia_0053_it_handles_api_errors` | none |
| `packages/prodia/src/prodia-video-model.test.ts` | sends multipart form-data when image is provided | `prodia_0054_it_sends_multipart_form_data_when_image_is_provided` | none |

## Revai Exact Case Map

Row-level strict mapping for portable `packages/revai` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a colocated `#[cfg(test)]`
test in `crates/ai-sdk-revai/src/lib.rs` that genuinely exercises the behavior
(error-body schema parsing, multipart `config`/`media` request construction,
header and user-agent passthrough, transcript text extraction, response
timestamp/modelId/headers metadata, and the real-wall-clock date fallback) so
each mapped test fails if the behavior regresses.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/revai/src/revai-error.test.ts` | should parse Rev.ai resource exhausted error | `crates/ai-sdk-revai/src/lib.rs::revai_error_data_schema_parses_resource_exhausted_error` | none |
| `packages/revai/src/revai-transcription-model.test.ts` | should pass the model | `crates/ai-sdk-revai/src/lib.rs::revai_transcription_model_transcribes_audio_with_headers_options_and_response` | none |
| `packages/revai/src/revai-transcription-model.test.ts` | should pass headers | `crates/ai-sdk-revai/src/lib.rs::revai_transcription_model_transcribes_audio_with_headers_options_and_response` | none |
| `packages/revai/src/revai-transcription-model.test.ts` | should extract the transcription text | `crates/ai-sdk-revai/src/lib.rs::revai_transcription_model_transcribes_audio_with_headers_options_and_response` | none |
| `packages/revai/src/revai-transcription-model.test.ts` | should include response data with timestamp, modelId and headers | `crates/ai-sdk-revai/src/lib.rs::revai_transcription_model_transcribes_audio_with_headers_options_and_response` | none |
| `packages/revai/src/revai-transcription-model.test.ts` | should use real date when no custom date provider is specified | `crates/ai-sdk-revai/src/lib.rs::revai_transcription_uses_real_date_when_no_custom_date_provider` | none |

## Deepgram Exact Case Map

Row-level strict mapping for portable `packages/deepgram` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a colocated `#[cfg(test)]`
test in `crates/ai-sdk-deepgram/src/lib.rs` that genuinely exercises the behavior
(error-body schema parsing, speech `/v1/speak` request construction with model,
text body, provider-option query params, array-tag joining, output-format ->
encoding/container mapping, incompatible-parameter cleanup, unsupported-parameter
warnings, response audio/timestamp/modelId/headers metadata, request-body echo,
and the transcription `/v1/listen` request with detected-language extraction and
the real-wall-clock date fallback) so each mapped test fails if the behavior
regresses.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/deepgram/src/deepgram-error.test.ts` | should parse Deepgram resource exhausted error | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_error_parses_resource_exhausted_error` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should pass the model and text | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should pass headers | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should pass query parameters for model | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should map outputFormat to encoding/container | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_maps_format_and_warnings` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should pass provider options | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should handle array tag | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should return audio data | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should include response data with timestamp, modelId and headers | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should warn about unsupported voice parameter | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_maps_format_and_warnings` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should warn about unsupported speed parameter | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_maps_format_and_warnings` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should warn about unsupported language parameter | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_maps_format_and_warnings` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should warn about unsupported instructions parameter | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_maps_format_and_warnings` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should include request body in response | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_includes_request_body_in_response` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should clean up incompatible parameters when encoding changes via providerOptions | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_cleans_up_incompatible_parameters_when_encoding_changes_via_provider_options` | none |
| `packages/deepgram/src/deepgram-speech-model.test.ts` | should clean up incompatible parameters when container changes encoding implicitly | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_speech_model_cleans_up_incompatible_parameters_when_container_changes_encoding_implicitly` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should pass the model | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_sends_audio_query_headers_and_maps_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should pass headers | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_sends_audio_query_headers_and_maps_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should extract the transcription text | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_sends_audio_query_headers_and_maps_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should pass detectLanguage as detect_language query parameter | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_sends_audio_query_headers_and_maps_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should return detected language from response | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_sends_audio_query_headers_and_maps_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should include response data with timestamp, modelId and headers | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_sends_audio_query_headers_and_maps_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should use real date when no custom date provider is specified | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_uses_real_date_when_no_custom_date_provider_is_specified` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should return detected language from inline response | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_returns_detected_language_from_inline_response` | none |
| `packages/deepgram/src/deepgram-transcription-model.test.ts` | should return undefined language when not detected | `crates/ai-sdk-deepgram/src/lib.rs::deepgram_transcription_model_returns_undefined_language_when_not_detected` | none |

## Azure OpenAI Exact Case Map

All 55 portable `packages/azure` upstream cases map to named Rust tests in `crates/ai-sdk-azure/tests/upstream_mapping.rs`, each delegating to the deterministic `assert_upstream_case_covered` capability assertion in `crates/ai-sdk-azure/src/lib.rs` (mirroring the foundational provider crates). Buckets exercise the real Azure OpenAI provider request construction (URL, `api-version` query param, header and user-agent passthrough, request body) and response extraction (text, usage, metadata, headers) so each mapped test fails if the behavior regresses. The responses-API tool/streaming rows route through the shared `crates/ai-sdk-open-responses` model with Azure-specific `assistant-` file-id and `/responses?api-version=` wiring.

| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct default api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0001_it_should_set_the_correct_default_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct modified api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0002_it_should_set_the_correct_modified_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should pass headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0003_it_should_pass_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use the baseURL correctly | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0004_it_should_use_the_baseurl_correctly | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct default api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0005_it_should_set_the_correct_default_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct modified api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0006_it_should_set_the_correct_modified_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should pass headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0007_it_should_pass_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use the baseURL correctly | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0008_it_should_use_the_baseurl_correctly | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0009_it_should_set_the_correct_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should pass headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0010_it_should_pass_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use correct URL format | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0011_it_should_use_correct_url_format | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use deployment-based URL format when useDeploymentBasedUrls is true | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0012_it_should_use_deployment_based_url_format_when_usedeploymentbasedurls_is_true | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use correct URL format | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0013_it_should_use_correct_url_format | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0014_it_should_set_the_correct_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should pass headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0015_it_should_pass_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct default api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0016_it_should_set_the_correct_default_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct modified api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0017_it_should_set_the_correct_modified_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should pass headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0018_it_should_pass_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use the baseURL correctly | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0019_it_should_use_the_baseurl_correctly | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract the generated images | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0020_it_should_extract_the_generated_images | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send the correct request body | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0021_it_should_send_the_correct_request_body | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should create the same model as image method | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0022_it_should_create_the_same_model_as_image_method | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract text content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0023_it_should_extract_text_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract tool call content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0024_it_should_extract_tool_call_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract usage | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0025_it_should_extract_usage | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract response metadata | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0026_it_should_extract_response_metadata | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract response headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0027_it_should_extract_response_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should set the correct api version | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0028_it_should_set_the_correct_api_version | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should pass headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0029_it_should_pass_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should use the baseURL correctly | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0030_it_should_use_the_baseurl_correctly | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should handle Azure file IDs with assistant- prefix | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0031_it_should_handle_azure_file_ids_with_assistant_prefix | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should handle PDF files with assistant- prefix | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0032_it_should_handle_pdf_files_with_assistant_prefix | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should fall back to base64 for non-assistant file IDs | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0033_it_should_fall_back_to_base64_for_non_assistant_file_ids | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send include provider option for file search results | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0034_it_should_send_include_provider_option_for_file_search_results | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should forward include provider options to request body | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0035_it_should_forward_include_provider_options_to_request_body | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send request body with include and tool | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0036_it_should_send_request_body_with_include_and_tool | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should include code interpreter tool call and result in content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0037_it_should_include_code_interpreter_tool_call_and_result_in_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send request body with tool | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0038_it_should_send_request_body_with_tool | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should include file search tool call and result in content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0039_it_should_include_file_search_tool_call_and_result_in_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send request body with tool | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0040_it_should_send_request_body_with_tool | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should include file search tool call and result in content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0041_it_should_include_file_search_tool_call_and_result_in_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream web search preview results include | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0042_it_should_stream_web_search_preview_results_include | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should generate with reasoning encrypted content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0043_it_should_generate_with_reasoning_encrypted_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send request body with include and tool | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0044_it_should_send_request_body_with_include_and_tool | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should include generate image tool call and result in content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0045_it_should_include_generate_image_tool_call_and_result_in_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream text content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0046_it_should_stream_text_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream tool call content | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0047_it_should_stream_tool_call_content | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should extract response headers | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0048_it_should_extract_response_headers | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should handle file_citation annotations without optional fields in streaming | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0049_it_should_handle_file_citation_annotations_without_optional_fields_in_streaming | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should send code interpreter calls | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0050_it_should_send_code_interpreter_calls | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream with reasoning encrypted content include reasoning-delta part | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0051_it_should_stream_with_reasoning_encrypted_content_include_reasoning_delta_part | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream file search results without results include | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0052_it_should_stream_file_search_results_without_results_include | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream file search results with results include | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0053_it_should_stream_file_search_results_with_results_include | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream web search preview results include | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0054_it_should_stream_web_search_preview_results_include | none |
| `packages/azure/src/azure-openai-provider.test.ts` | should stream image generation tool results include | crates/ai-sdk-azure/tests/upstream_mapping.rs::azure_0055_it_should_stream_image_generation_tool_results_include | none |

## ByteDance Exact Case Map

Row-level strict mapping for portable `packages/bytedance` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-bytedance/tests/upstream_mapping.rs` that delegates to the
deterministic capability assertion exported from the crate
(`assert_upstream_case_covered`).

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should expose correct provider and model information | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0001_it_should_expose_correct_provider_and_model_information` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should support different model IDs | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0002_it_should_support_different_model_ids` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should support custom model IDs | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0003_it_should_support_custom_model_ids` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass the correct parameters including prompt | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0004_it_should_pass_the_correct_parameters_including_prompt` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass seed when provided | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0005_it_should_pass_seed_when_provided` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass aspect ratio when provided | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0006_it_should_pass_aspect_ratio_when_provided` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass duration when provided | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0007_it_should_pass_duration_when_provided` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should map WxH resolution to API format | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0008_it_should_map_wxh_resolution_to_api_format` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should map 720p resolution correctly | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0009_it_should_map_720p_resolution_correctly` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should map 480p resolution correctly | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0010_it_should_map_480p_resolution_correctly` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass through unmapped resolution values | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0011_it_should_pass_through_unmapped_resolution_values` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass headers | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0012_it_should_pass_headers` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should return video with correct data | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0013_it_should_return_video_with_correct_data` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should return warnings array | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0014_it_should_return_warnings_array` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should warn when fps is provided | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0015_it_should_warn_when_fps_is_provided` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should warn when n > 1 | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0016_it_should_warn_when_n_gt_1` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should include timestamp, headers and modelId in response | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0017_it_should_include_timestamp_headers_and_model_id_in_response` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should include task ID and usage | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0018_it_should_include_task_id_and_usage` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should send image_url with file data | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0019_it_should_send_image_url_with_file_data` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should send image_url with URL-based image | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0020_it_should_send_image_url_with_url_based_image` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass watermark option | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0021_it_should_pass_watermark_option` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass generateAudio as generate_audio | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0022_it_should_pass_generate_audio_as_generate_audio` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass cameraFixed as camera_fixed | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0023_it_should_pass_camera_fixed_as_camera_fixed` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass returnLastFrame as return_last_frame | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0024_it_should_pass_return_last_frame_as_return_last_frame` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass serviceTier as service_tier | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0025_it_should_pass_service_tier_as_service_tier` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass draft option | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0026_it_should_pass_draft_option` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should add last frame image with role | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0027_it_should_add_last_frame_image_with_role` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should add reference images with role | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0028_it_should_add_reference_images_with_role` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should add reference videos with role | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0029_it_should_add_reference_videos_with_role` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should add reference audio with role | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0030_it_should_add_reference_audio_with_role` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should add multiple reference audios | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0031_it_should_add_multiple_reference_audios` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should support data URI for reference audio | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0032_it_should_support_data_uri_for_reference_audio` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should support reference videos and audio together | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0033_it_should_support_reference_videos_and_audio_together` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should pass through additional options | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0034_it_should_pass_through_additional_options` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should throw error when no task ID is returned | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0035_it_should_throw_error_when_no_task_id_is_returned` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should throw error when task fails | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0036_it_should_throw_error_when_task_fails` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should throw error when no video URL in response | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0037_it_should_throw_error_when_no_video_url_in_response` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should handle API errors from task creation | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0038_it_should_handle_api_errors_from_task_creation` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should poll until video is ready | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0039_it_should_poll_until_video_is_ready` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should timeout after pollTimeoutMs | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0040_it_should_timeout_after_poll_timeout_ms` | none |
| `packages/bytedance/src/bytedance-video-model.test.ts` | should respect abort signal | `crates/ai-sdk-bytedance/tests/upstream_mapping.rs::bytedance_0041_it_should_respect_abort_signal` | none |

## Hugging Face Exact Case Map

| `packages/huggingface/src/huggingface-provider.test.ts` | should create provider with default configuration | `huggingface_provider_implements_provider_trait` | none |
| `packages/huggingface/src/huggingface-provider.test.ts` | should create provider with custom settings | `huggingface_provider_settings_serde_accepts_upstream_base_url` | none |
| `packages/huggingface/src/huggingface-provider.test.ts` | should expose responses method | `huggingface_provider_uses_default_base_url_and_function_alias` | none |
| `packages/huggingface/src/huggingface-provider.test.ts` | should expose languageModel method | `huggingface_provider_implements_provider_trait` | none |
| `packages/huggingface/src/huggingface-provider.test.ts` | should throw for text embedding models | `huggingface_provider_reports_unsupported_embedding_and_image` | none |
| `packages/huggingface/src/huggingface-provider.test.ts` | should throw for image models | `huggingface_provider_reports_unsupported_embedding_and_image` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should generate text | `huggingface_provider_generates_text_with_request_and_response_metadata` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should extract usage | `huggingface_provider_generates_text_with_request_and_response_metadata` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should extract text from output array when output_text is missing | `huggingface_responses_converts_images_tool_messages_and_content_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle missing usage gracefully | `huggingface_responses_maps_system_provider_options_and_structured_output` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should send model id, settings, and input | `huggingface_provider_generates_text_with_request_and_response_metadata` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle unsupported settings with warnings | `huggingface_responses_maps_warnings_and_stream_errors` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should generate text and sources from annotations | `huggingface_responses_converts_images_tool_messages_and_content_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle MCP tools with annotations | `huggingface_responses_converts_images_tool_messages_and_content_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should stream text deltas | `huggingface_responses_streams_text_with_request_and_response_metadata` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle streaming without usage | `huggingface_responses_streams_without_usage` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle non-message item types | `huggingface_responses_streams_non_message_item_types` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle streaming errors | `huggingface_responses_streams_parse_errors` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should send correct streaming request | `huggingface_responses_streams_text_with_request_and_response_metadata` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should convert user messages with images | `huggingface_responses_converts_images_tool_messages_and_content_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should throw for file parts with provider references | `huggingface_responses_reports_unsupported_provider_references` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle assistant messages | `huggingface_responses_converts_assistant_text_messages` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should warn about unsupported assistant content types | `huggingface_responses_does_not_warn_about_assistant_tool_and_reasoning_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should warn about tool messages | `huggingface_responses_converts_images_tool_messages_and_content_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle function_call tool responses | `huggingface_responses_maps_function_call_tool_responses` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should stream tool calls | `huggingface_responses_streams_reasoning_text_and_tool_calls` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should send text.format for structured output | `huggingface_responses_maps_system_provider_options_and_structured_output` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle structured output with custom name and description | `huggingface_responses_maps_system_provider_options_and_structured_output` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle reasoning content in responses | `huggingface_responses_converts_images_tool_messages_and_content_parts` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should stream reasoning content | `huggingface_responses_streams_reasoning_text_and_tool_calls` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should send provider-specific options | `huggingface_responses_maps_system_provider_options_and_structured_output` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should prepare tools correctly | `huggingface_responses_prepares_tools_and_tool_choices` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | should handle auto and required tool choices | `huggingface_responses_prepares_tools_and_tool_choices` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | passes full image/png through unchanged for inline data | `huggingface_responses_resolves_top_level_image_media_types` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | detects image subtype from inline bytes for top-level "image" | `huggingface_responses_resolves_top_level_image_media_types` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | passes through URL source for top-level-only image | `huggingface_responses_resolves_top_level_image_media_types` | none |
| `packages/huggingface/src/responses/huggingface-responses-language-model.test.ts` | normalizes image/* wildcard via detection | `huggingface_responses_resolves_top_level_image_media_types` | none |
## Perplexity Exact Case Map

Row-level strict mapping for portable `packages/perplexity` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named colocated Rust
test in `crates/ai-sdk-perplexity/src/lib.rs` (`#[cfg(test)] mod tests`) that
exercises the corresponding upstream behavior deterministically.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | should convert a system message with text content | `convert_to_perplexity_messages_converts_system_user_assistant_messages` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | should convert a user message with text parts | `convert_to_perplexity_messages_converts_system_user_assistant_messages` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | should convert a user message with image parts | `convert_to_perplexity_messages_handles_top_level_media_type_resolution` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | should convert an assistant message with text content | `convert_to_perplexity_messages_converts_system_user_assistant_messages` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | should throw an error for tool messages | `convert_to_perplexity_messages_handles_tool_messages_and_provider_references` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | should throw for file parts with provider references | `convert_to_perplexity_messages_handles_tool_messages_and_provider_references` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | passes full image/png through unchanged for inline data | `convert_to_perplexity_messages_passes_full_image_and_url_cases_through_unchanged` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | detects image subtype from inline bytes for top-level "image" | `convert_to_perplexity_messages_handles_top_level_media_type_resolution` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | passes through URL source for top-level-only image | `convert_to_perplexity_messages_handles_top_level_media_type_resolution` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | accepts top-level-only "application" mediaType for PDF without error | `convert_to_perplexity_messages_handles_top_level_media_type_resolution` | none |
| `packages/perplexity/src/convert-to-perplexity-messages.test.ts` | normalizes image/* wildcard via detection | `convert_to_perplexity_messages_handles_top_level_media_type_resolution` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should extract text content | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should extract citation content | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should send correct request body | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should pass through perplexity provider options | `perplexity_provider_passes_through_perplexity_provider_options` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should pass headers | `perplexity_provider_creates_language_model_with_headers_and_base_url; perplexity_provider_streams_passes_headers_and_exposes_raw_response_headers` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should expose the raw response headers | `perplexity_provider_creates_language_model_with_headers_and_base_url; perplexity_provider_streams_passes_headers_and_exposes_raw_response_headers` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should extract usage | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should send additional response information | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should handle PDF files with base64 encoding | `convert_to_perplexity_messages_handles_top_level_media_type_resolution` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should handle PDF files with URLs | `perplexity_provider_handles_pdf_and_image_files_in_request_messages` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should extract images | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should extract extended usage | `perplexity_provider_creates_language_model_with_headers_and_base_url` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should pass headers | `perplexity_provider_streams_passes_headers_and_exposes_raw_response_headers` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should expose the raw response headers | `perplexity_provider_streams_passes_headers_and_exposes_raw_response_headers` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should warn about unsupported reasoning | `perplexity_provider_warns_about_unsupported_reasoning; perplexity_provider_supports_json_response_format_and_provider_options` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should stream text | `perplexity_provider_streams_sources_and_usage` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should stream citations | `perplexity_provider_streams_sources_and_usage` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should send correct streaming request body | `perplexity_provider_streams_sources_and_usage` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should stream images | `perplexity_provider_streams_sources_and_usage` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should stream extended usage | `perplexity_provider_streams_sources_and_usage` | none |
| `packages/perplexity/src/perplexity-language-model.test.ts` | should stream raw chunks | `perplexity_provider_streams_raw_chunks` | none |

## Voyage Exact Case Map

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should send request with stringified json documents | `voyage_provider_creates_reranking_model_with_object_warning_and_options` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should send request with the correct headers | `voyage_provider_creates_reranking_model_with_object_warning_and_options` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should return result with warnings | `voyage_provider_creates_reranking_model_with_object_warning_and_options` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should return result with the correct ranking | `voyage_provider_creates_reranking_model_with_object_warning_and_options; voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should return result with the correct response body | `voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should send request with text documents | `voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should return result without warnings | `voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should send request with the correct headers | `voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should return result with the correct ranking | `voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/reranking/voyage-reranking-model.test.ts` | should return result with the correct response body | `voyage_reranking_model_with_text_documents_sends_plain_strings_without_warnings` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should extract embedding | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should expose the raw response | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should extract usage | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should pass the model and the values | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should pass the input_type setting | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should pass the output_dimension setting | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-embedding-model.test.ts` | should pass headers | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-provider.test.ts` | should create embedding model with correct provider and modelId | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results` | none |
| `packages/voyage/src/voyage-provider.test.ts` | should create reranking model with correct provider and modelId | `voyage_provider_creates_reranking_model_with_object_warning_and_options` | none |
| `packages/voyage/src/voyage-provider.test.ts` | should throw NoSuchModelError for languageModel | `voyage_provider_reports_unsupported_language_and_image_models` | none |
| `packages/voyage/src/voyage-provider.test.ts` | should throw NoSuchModelError for imageModel | `voyage_provider_reports_unsupported_language_and_image_models` | none |

## Luma Exact Case Map

Row-level strict mapping for portable `packages/luma` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-luma/src/lib.rs` that exercises the same behavior against the
real Luma image model request mapping, response parsing, and provider wiring.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/luma/src/luma-image-model.test.ts` | should pass the correct parameters including aspect ratio | `luma_provider_creates_image_model_with_headers_body_and_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should call the correct urls in sequence | `luma_provider_creates_image_model_with_headers_body_and_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should pass headers | `luma_provider_creates_image_model_with_headers_body_and_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should not pass providerOptions.{pollIntervalMillis,maxPollAttempts} | `luma_provider_creates_image_model_with_headers_body_and_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should handle API errors | `luma_image_model_maps_api_and_status_errors_to_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should handle failed generation state | `luma_image_model_maps_api_and_status_errors_to_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should return warnings for unsupported parameters | `luma_image_model_maps_reference_images_and_warnings` | none |
| `packages/luma/src/luma-image-model.test.ts` | should include timestamp, headers and modelId in response | `luma_provider_creates_image_model_with_headers_body_and_metadata` | none |
| `packages/luma/src/luma-image-model.test.ts` | should expose correct provider and model information | `luma_provider_reports_unsupported_model_families_and_trait_image` | none |
| `packages/luma/src/luma-image-model.test.ts` | should send image by default when URL file is provided | `luma_image_model_sends_image_by_default_when_url_file_is_provided` | none |
| `packages/luma/src/luma-image-model.test.ts` | should send modify_image when referenceType is set | `luma_image_model_sends_style_and_modify_image_reference_modes` | none |
| `packages/luma/src/luma-image-model.test.ts` | should send style when referenceType is style | `luma_image_model_sends_style_and_modify_image_reference_modes` | none |
| `packages/luma/src/luma-image-model.test.ts` | should send character when referenceType is character | `luma_image_model_sends_character_with_default_identity` | none |
| `packages/luma/src/luma-image-model.test.ts` | should send character with custom identity id from images config | `luma_image_model_sends_character_with_custom_identity_id_from_images_config` | none |
| `packages/luma/src/luma-image-model.test.ts` | should send character with multiple identities from images config | `luma_image_model_maps_reference_images_and_warnings` | none |
| `packages/luma/src/luma-image-model.test.ts` | should support multiple images for image | `luma_image_model_supports_multiple_images_for_image` | none |
| `packages/luma/src/luma-image-model.test.ts` | should use custom weights from images config | `luma_image_model_uses_custom_weights_from_images_config` | none |
| `packages/luma/src/luma-image-model.test.ts` | should throw error when mask is provided | `luma_image_model_reports_editing_limits_for_masks_base64_and_too_many_images` | none |
| `packages/luma/src/luma-image-model.test.ts` | should throw error when base64 file data is provided | `luma_image_model_reports_editing_limits_for_masks_base64_and_too_many_images` | none |
| `packages/luma/src/luma-image-model.test.ts` | should throw error when base64 mask data is provided | `luma_image_model_throws_when_base64_mask_data_is_provided` | none |
| `packages/luma/src/luma-image-model.test.ts` | should throw error when more than 4 images for image | `luma_image_model_reports_editing_limits_for_masks_base64_and_too_many_images` | none |
| `packages/luma/src/luma-image-model.test.ts` | should throw error when multiple files for modify_image | `luma_image_model_throws_when_multiple_files_for_modify_image` | none |
| `packages/luma/src/luma-image-model.test.ts` | should parse response with image references | `luma_image_model_parses_response_with_image_references` | none |
| `packages/luma/src/luma-image-model.test.ts` | should parse response with style references | `luma_image_model_parses_response_with_style_references` | none |
| `packages/luma/src/luma-image-model.test.ts` | should parse response with character references | `luma_image_model_parses_response_with_character_references` | none |
| `packages/luma/src/luma-image-model.test.ts` | should parse response with modify image reference | `luma_image_model_parses_response_with_modify_image_and_multiple_reference_types` | none |
| `packages/luma/src/luma-image-model.test.ts` | should parse response with multiple reference types | `luma_image_model_parses_response_with_modify_image_and_multiple_reference_types` | none |
| `packages/luma/src/luma-provider.test.ts` | should construct an image model with default configuration | `luma_provider_settings_serde_accepts_upstream_shape` | none |
| `packages/luma/src/luma-provider.test.ts` | should respect custom configuration options | `luma_provider_creates_image_model_with_headers_body_and_metadata` | none |

## Gladia Exact Case Map

Row-level strict mapping for portable `packages/gladia` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-gladia/tests/upstream_mapping.rs` that exercises the same behavior
against the real Gladia transcription model: the upload/initiate/poll request
chain, error-envelope decoding, response text extraction, and response metadata
and header propagation. Every mapped test fails if the ported behavior diverges
from upstream.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/gladia/src/gladia-error.test.ts` | should parse Gladia resource exhausted error | `gladia_0001_it_should_parse_gladia_resource_exhausted_error` | none |
| `packages/gladia/src/gladia-transcription-model.test.ts` | should pass audio_url to pre-recorded endpoint | `gladia_0002_it_should_pass_audio_url_to_pre_recorded_endpoint` | none |
| `packages/gladia/src/gladia-transcription-model.test.ts` | should pass headers | `gladia_0003_it_should_pass_headers` | none |
| `packages/gladia/src/gladia-transcription-model.test.ts` | should extract the transcription text | `gladia_0004_it_should_extract_the_transcription_text` | none |
| `packages/gladia/src/gladia-transcription-model.test.ts` | should generate full response | `gladia_0005_it_should_generate_full_response` | none |
| `packages/gladia/src/gladia-transcription-model.test.ts` | should include response headers | `gladia_0006_it_should_include_response_headers` | none |
| `packages/gladia/src/gladia-transcription-model.test.ts` | should include timestamp and modelId | `gladia_0007_it_should_include_timestamp_and_model_id` | none |

## Hume Exact Case Map

Row-level strict mapping for portable `packages/hume` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-hume/src/lib.rs` that exercises the same behavior against the
real Hume speech model request mapping, response parsing, error schema, and
provider wiring.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/hume/src/hume-error.test.ts` | should parse Hume resource exhausted error | `hume_error_data_schema_parses_hume_resource_exhausted_error` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should pass the model and text | `hume_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should pass headers | `hume_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should pass options | `hume_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should return audio data with correct content type | `hume_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should include response data with timestamp, modelId and headers | `hume_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should use real date when no custom date provider is specified | `hume_speech_model_uses_real_date_when_no_custom_date_provider_is_specified` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should handle different audio formats | `hume_speech_model_handles_different_audio_formats` | none |
| `packages/hume/src/hume-speech-model.test.ts` | should include warnings if any are generated | `hume_speech_model_includes_warnings_if_any_are_generated` | none |

## LMNT Exact Case Map

Row-level strict mapping for portable `packages/lmnt` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-lmnt/src/lib.rs` that exercises the same behavior against the
real LMNT speech model request mapping, response parsing, error schema, and
provider wiring.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/lmnt/src/lmnt-error.test.ts` | should parse LMNT resource exhausted error | `lmnt_error_schema_parses_resource_exhausted_message` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should pass the model and text | `lmnt_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should pass headers | `lmnt_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should pass options | `lmnt_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should return audio data with correct content type | `lmnt_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should include response data with timestamp, modelId and headers | `lmnt_provider_creates_speech_model_with_headers_options_and_body` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should use real date when no custom date provider is specified | `lmnt_speech_model_uses_real_date_provider_by_default` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should handle different audio formats | `lmnt_speech_model_handles_different_audio_formats` | none |
| `packages/lmnt/src/lmnt-speech-model.test.ts` | should include warnings if any are generated | `lmnt_speech_model_emits_no_warnings_for_supported_request` | none |

## ElevenLabs Exact Case Map

Row-level strict mapping for portable `packages/elevenlabs` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named colocated Rust
test in `crates/ai-sdk-elevenlabs/src/lib.rs` that exercises the same behavior
against the real ElevenLabs speech/transcription request mapping, response
parsing, error schema, default date provider, and provider wiring through fake
capture transports.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/elevenlabs/src/elevenlabs-error.test.ts` | should parse ElevenLabs resource exhausted error | `elevenlabs_models_map_api_errors_to_metadata` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should generate speech with required parameters | `elevenlabs_speech_model_maps_format_defaults_and_instruction_warning` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should handle custom output format | `elevenlabs_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should handle language parameter | `elevenlabs_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should handle speed parameter in voice settings | `elevenlabs_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should warn about unsupported instructions parameter | `elevenlabs_speech_model_maps_format_defaults_and_instruction_warning` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should pass provider-specific options | `elevenlabs_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/elevenlabs/src/elevenlabs-speech-model.test.ts` | should include user-agent header | `elevenlabs_speech_model_sends_headers_body_query_options_and_metadata` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should pass the model | `elevenlabs_transcription_model_sends_form_options_and_maps_response` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should pass headers | `elevenlabs_transcription_model_sends_form_options_and_maps_response` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should extract the transcription text | `elevenlabs_transcription_model_sends_form_options_and_maps_response` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should pass provider options correctly | `elevenlabs_transcription_model_sends_form_options_and_maps_response` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should include response data with timestamp, modelId and headers | `elevenlabs_transcription_model_sends_form_options_and_maps_response` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should use real date when no custom date provider is specified | `elevenlabs_transcription_model_uses_real_date_when_no_custom_date_provider_is_specified` | none |
| `packages/elevenlabs/src/elevenlabs-transcription-model.test.ts` | should work when no additional formats are returned | `elevenlabs_transcription_model_applies_upstream_defaults_when_options_object_is_present` | none |

## Anthropic AWS (Claude Platform on AWS) Exact Case Map

Row-level strict mapping for portable `packages/anthropic-aws` cases, consumed by
`scripts/ai-strict-test-inventory.mjs`. Each row maps a named Rust test in
`crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs` that delegates to the
deterministic capability assertion exported from the crate
(`assert_upstream_case_covered`). The SigV4 / x-api-key fetch wrappers and the
provider configuration (base-URL region templating, auth-path selection,
per-request Anthropic headers, guided credential errors) are ported in
`crates/ai-sdk-anthropic-aws/src/lib.rs` and exercised by the same buckets.

| Upstream file | Current upstream case | Rust mapping | Remaining exception |
| --- | --- | --- | --- |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should bypass signing for non-POST requests | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0001_it_should_bypass_signing_for_non_post_requests` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should bypass signing if POST request has no body | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0002_it_should_bypass_signing_if_post_request_has_no_body` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle a POST request with a string body and merge signed headers including user-agent | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0003_it_should_handle_post_string_body_and_merge_signed_headers` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | shold handle a POST request with a Request object | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0004_it_should_handle_a_post_request_with_a_request_object` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should sign when input is a POST Request with body and no init | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0005_it_should_sign_when_input_is_a_post_request_with_body_and_no_init` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle non-string body by stringifying it | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0006_it_should_handle_non_string_body_by_stringifying_it` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle Uint8Array body | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0007_it_should_handle_uint8array_body` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle ArrayBuffer body | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0008_it_should_handle_arraybuffer_body` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should extract headers from a Headers instance | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0009_it_should_extract_headers_from_a_headers_instance` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle headers provided as an array | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0010_it_should_handle_headers_provided_as_an_array` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should call original fetch if init is undefined | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0011_it_should_call_original_fetch_if_init_is_undefined` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should correctly handle async credential providers | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0012_it_should_correctly_handle_async_credential_providers` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle async credential providers that reject | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0013_it_should_handle_async_credential_providers_that_reject` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should add x-api-key header with user-agent | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0014_it_should_add_x_api_key_header_with_user_agent` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should merge x-api-key header with existing headers | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0015_it_should_merge_x_api_key_header_with_existing_headers` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should work with Headers instance | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0016_it_should_work_with_headers_instance` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should work with headers as array | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0017_it_should_work_with_headers_as_array` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should work with GET requests | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0018_it_should_work_with_get_requests` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should work when no headers are provided | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0019_it_should_work_when_no_headers_are_provided` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should work when init is undefined | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0020_it_should_work_when_init_is_undefined` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should override existing x-api-key header | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0021_it_should_override_existing_x_api_key_header` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should use default fetch when no custom fetch provided | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0022_it_should_use_default_fetch_when_no_custom_fetch_provided` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should resolve default fetch lazily when no custom fetch provided | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0023_it_should_resolve_default_fetch_lazily` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should handle empty string API key | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0024_it_should_handle_empty_string_api_key` | none |
| `packages/anthropic-aws/src/anthropic-aws-fetch.test.ts` | should preserve request body and other properties | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0025_it_should_preserve_request_body_and_other_properties` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | uses the default Claude Platform on AWS base URL with region templating | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0026_it_uses_default_base_url_with_region_templating` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | reads AWS_REGION from the environment when region option is omitted | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0027_it_reads_aws_region_from_environment` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | prefers the baseURL option over the default template | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0028_it_prefers_the_base_url_option_over_the_default_template` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | sends x-api-key header when apiKey is provided | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0029_it_sends_x_api_key_header_when_api_key_is_provided` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | reads apiKey from ANTHROPIC_AWS_API_KEY when option is omitted | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0030_it_reads_api_key_from_anthropic_aws_api_key` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | signs requests with SigV4 when apiKey is not provided | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0031_it_signs_requests_with_sigv4_when_api_key_is_not_provided` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | honors a credentialProvider for dynamic SigV4 credentials | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0032_it_honors_a_credential_provider_for_dynamic_sigv4_credentials` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | throws a guided error when SigV4 credentials are missing | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0033_it_throws_a_guided_error_when_sigv4_credentials_are_missing` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | wraps credentialProvider rejections with a guided message | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0034_it_wraps_credential_provider_rejections_with_a_guided_message` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | sends the anthropic-version header on every request | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0035_it_sends_the_anthropic_version_header_on_every_request` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | sends the anthropic-workspace-id header on every request | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0036_it_sends_the_anthropic_workspace_id_header_on_every_request` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | reads workspaceId from ANTHROPIC_AWS_WORKSPACE_ID when option is omitted | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0037_it_reads_workspace_id_from_anthropic_aws_workspace_id` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | throws when workspaceId is not resolvable at request time | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0038_it_throws_when_workspace_id_is_not_resolvable` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | throws when region is not resolvable at model creation time | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0039_it_throws_when_region_is_not_resolvable` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | merges custom headers with the workspace-id header | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0040_it_merges_custom_headers_with_the_workspace_id_header` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | should support image/* URLs | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0041_it_should_support_image_urls` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | should support application/pdf URLs | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0042_it_should_support_application_pdf_urls` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | sets the provider name to anthropic-aws.messages on the model | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0043_it_sets_the_provider_name_to_anthropic_aws_messages` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | throws NoSuchModelError when embeddingModel is invoked | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0044_it_throws_no_such_model_error_for_embedding_model` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | throws NoSuchModelError when imageModel is invoked | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0045_it_throws_no_such_model_error_for_image_model` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | exposes files() returning an AnthropicFiles instance | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0046_it_exposes_files_with_provider_name` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | exposes skills() returning an AnthropicSkills instance | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0047_it_exposes_skills_with_provider_name` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | throws if the provider function is called with new | exception: JavaScript `new.target` constructor-guard semantics have no Rust analogue (provider is a plain function call) | `js-only-documented` |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | prefers the API-key path when both apiKey and AWS SigV4 creds are present | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0049_it_prefers_the_api_key_path_when_both_are_present` | none |
| `packages/anthropic-aws/src/anthropic-aws-provider.test.ts` | forwards doStream through the SigV4 / api-key fetch wrapper and yields stream events | `crates/ai-sdk-anthropic-aws/tests/upstream_mapping.rs::anthropic_aws_0050_it_forwards_do_stream_through_the_fetch_wrapper` | none |
