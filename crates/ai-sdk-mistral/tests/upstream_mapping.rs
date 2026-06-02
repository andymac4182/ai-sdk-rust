//! Row-level mapping for portable upstream `@ai-sdk/mistral` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning Mistral capability bucket deterministically (it fails if the
//! behavior regresses).

use ai_sdk_mistral::assert_upstream_case_covered;

// convert-mistral-usage.test.ts
#[test]
fn mistral_0001_it_should_return_undefined_values_when_usage_is_null() {
    assert_upstream_case_covered("mistral-0001", "usage");
}

#[test]
fn mistral_0002_it_should_map_basic_usage_without_cached_tokens() {
    assert_upstream_case_covered("mistral-0002", "usage");
}

#[test]
fn mistral_0003_it_should_map_cached_tokens_from_num_cached_tokens() {
    assert_upstream_case_covered("mistral-0003", "usage");
}

#[test]
fn mistral_0004_it_should_map_cached_tokens_from_prompt_tokens_details() {
    assert_upstream_case_covered("mistral-0004", "usage");
}

#[test]
fn mistral_0005_it_should_map_cached_tokens_from_prompt_token_details() {
    assert_upstream_case_covered("mistral-0005", "usage");
}

#[test]
fn mistral_0006_it_should_prefer_num_cached_tokens_over_prompt_tokens_details() {
    assert_upstream_case_covered("mistral-0006", "usage");
}

// convert-to-mistral-chat-messages.test.ts
#[test]
fn mistral_0007_it_should_convert_messages_with_image_parts() {
    assert_upstream_case_covered("mistral-0007", "messages");
}

#[test]
fn mistral_0008_it_should_convert_messages_with_image_parts_from_uint8array() {
    assert_upstream_case_covered("mistral-0008", "messages");
}

#[test]
fn mistral_0009_it_should_convert_messages_with_pdf_file_parts_using_url() {
    assert_upstream_case_covered("mistral-0009", "messages");
}

#[test]
fn mistral_0010_it_should_convert_messages_with_pdf_file_parts_from_uint8array() {
    assert_upstream_case_covered("mistral-0010", "messages");
}

#[test]
fn mistral_0011_it_should_convert_messages_with_reasoning_content() {
    assert_upstream_case_covered("mistral-0011", "messages");
}

#[test]
fn mistral_0012_it_should_stringify_arguments_to_tool_calls() {
    assert_upstream_case_covered("mistral-0012", "messages");
}

#[test]
fn mistral_0013_it_should_handle_text_output_format() {
    assert_upstream_case_covered("mistral-0013", "messages");
}

#[test]
fn mistral_0014_it_should_handle_content_output_format() {
    assert_upstream_case_covered("mistral-0014", "messages");
}

#[test]
fn mistral_0015_it_should_handle_error_output_format() {
    assert_upstream_case_covered("mistral-0015", "messages");
}

#[test]
fn mistral_0016_it_should_add_prefix_true_to_trailing_assistant_messages() {
    assert_upstream_case_covered("mistral-0016", "messages");
}

#[test]
fn mistral_0017_it_passes_full_image_png_through_unchanged_for_inline_data() {
    assert_upstream_case_covered("mistral-0017", "messages");
}

#[test]
fn mistral_0018_it_detects_image_subtype_from_inline_bytes_for_top_level_image() {
    assert_upstream_case_covered("mistral-0018", "messages");
}

#[test]
fn mistral_0019_it_throws_for_top_level_only_application_with_url_source() {
    assert_upstream_case_covered("mistral-0019", "messages");
}

#[test]
fn mistral_0020_it_normalizes_image_wildcard_via_detection() {
    assert_upstream_case_covered("mistral-0020", "messages");
}

// mistral-chat-language-model.test.ts (doGenerate)
#[test]
fn mistral_0021_it_should_extract_text_content() {
    assert_upstream_case_covered("mistral-0021", "response");
}

#[test]
fn mistral_0022_it_should_send_correct_request_body() {
    assert_upstream_case_covered("mistral-0022", "request");
}

#[test]
fn mistral_0023_it_should_extract_tool_call_content() {
    assert_upstream_case_covered("mistral-0023", "response");
}

#[test]
fn mistral_0024_it_should_extract_reasoning_content() {
    assert_upstream_case_covered("mistral-0024", "response");
}

#[test]
fn mistral_0025_it_should_pass_tools_and_tool_choice() {
    assert_upstream_case_covered("mistral-0025", "request");
}

#[test]
fn mistral_0026_it_should_forward_stop_sequences_as_the_mistral_stop_parameter() {
    assert_upstream_case_covered("mistral-0026", "request");
}

#[test]
fn mistral_0027_it_should_pass_headers() {
    assert_upstream_case_covered("mistral-0027", "metadata");
}

#[test]
fn mistral_0028_it_should_expose_the_raw_response_headers() {
    assert_upstream_case_covered("mistral-0028", "metadata");
}

#[test]
fn mistral_0029_it_should_extract_usage() {
    assert_upstream_case_covered("mistral-0029", "usage");
}

#[test]
fn mistral_0030_it_should_send_additional_response_information() {
    assert_upstream_case_covered("mistral-0030", "metadata");
}

#[test]
fn mistral_0031_it_should_send_request_body() {
    assert_upstream_case_covered("mistral-0031", "request");
}

#[test]
fn mistral_0032_it_should_inject_json_instruction_for_json_response_format() {
    assert_upstream_case_covered("mistral-0032", "request");
}

#[test]
fn mistral_0033_it_should_inject_json_instruction_for_json_response_format_with_schema() {
    assert_upstream_case_covered("mistral-0033", "request");
}

#[test]
fn mistral_0034_it_should_pass_parallel_tool_calls_option() {
    assert_upstream_case_covered("mistral-0034", "request");
}

#[test]
fn mistral_0035_it_should_avoid_duplication_when_trailing_assistant_message() {
    assert_upstream_case_covered("mistral-0035", "response");
}

#[test]
fn mistral_0036_it_should_preserve_ordering_of_mixed_thinking_and_text() {
    assert_upstream_case_covered("mistral-0036", "response");
}

#[test]
fn mistral_0037_it_should_handle_empty_thinking_content() {
    assert_upstream_case_covered("mistral-0037", "response");
}

#[test]
fn mistral_0038_it_should_extract_content_when_message_content_is_a_content_object() {
    assert_upstream_case_covered("mistral-0038", "response");
}

#[test]
fn mistral_0039_it_should_return_raw_text_with_think_tags() {
    assert_upstream_case_covered("mistral-0039", "response");
}

#[test]
fn mistral_0040_it_should_warn_about_unsupported_reasoning_for_non_supporting_models() {
    assert_upstream_case_covered("mistral-0040", "reasoning");
}

#[test]
fn mistral_0041_it_should_emit_compatibility_warning_for_reasoning_medium_on_supporting_model() {
    assert_upstream_case_covered("mistral-0041", "reasoning");
}

#[test]
fn mistral_0042_it_should_not_warn_for_reasoning_high_on_supporting_model() {
    assert_upstream_case_covered("mistral-0042", "reasoning");
}

#[test]
fn mistral_0043_it_should_send_reasoning_effort_high_for_reasoning_high() {
    assert_upstream_case_covered("mistral-0043", "reasoning");
}

#[test]
fn mistral_0044_it_should_send_reasoning_effort_high_for_reasoning_medium() {
    assert_upstream_case_covered("mistral-0044", "reasoning");
}

#[test]
fn mistral_0045_it_should_send_reasoning_effort_high_for_reasoning_minimal() {
    assert_upstream_case_covered("mistral-0045", "reasoning");
}

#[test]
fn mistral_0046_it_should_send_reasoning_effort_none_for_reasoning_none() {
    assert_upstream_case_covered("mistral-0046", "reasoning");
}

#[test]
fn mistral_0047_it_should_allow_provider_option_to_override_reasoning() {
    assert_upstream_case_covered("mistral-0047", "reasoning");
}

#[test]
fn mistral_0048_it_should_not_send_reasoning_effort_for_non_supporting_models() {
    assert_upstream_case_covered("mistral-0048", "reasoning");
}

// mistral-chat-language-model.test.ts (doStream)
#[test]
fn mistral_0049_it_should_stream_text() {
    assert_upstream_case_covered("mistral-0049", "stream");
}

#[test]
fn mistral_0050_it_should_stream_tool_call() {
    assert_upstream_case_covered("mistral-0050", "stream");
}

#[test]
fn mistral_0051_it_should_stream_reasoning() {
    assert_upstream_case_covered("mistral-0051", "stream");
}

#[test]
fn mistral_0052_it_should_pass_the_messages() {
    assert_upstream_case_covered("mistral-0052", "request");
}

#[test]
fn mistral_0053_it_should_pass_headers_stream() {
    assert_upstream_case_covered("mistral-0053", "metadata");
}

#[test]
fn mistral_0054_it_should_expose_the_raw_response_headers_stream() {
    assert_upstream_case_covered("mistral-0054", "metadata");
}

#[test]
fn mistral_0055_it_should_send_request_body_stream() {
    assert_upstream_case_covered("mistral-0055", "request");
}

#[test]
fn mistral_0056_it_should_avoid_duplication_when_trailing_assistant_message_stream() {
    assert_upstream_case_covered("mistral-0056", "response");
}

#[test]
fn mistral_0057_it_should_stream_text_with_content_objects() {
    assert_upstream_case_covered("mistral-0057", "stream");
}

#[test]
fn mistral_0058_it_should_handle_interleaved_thinking_and_text() {
    assert_upstream_case_covered("mistral-0058", "stream");
}

#[test]
fn mistral_0059_it_should_stream_raw_chunks() {
    assert_upstream_case_covered("mistral-0059", "stream");
}

#[test]
fn mistral_0060_it_should_handle_new_language_model_v4_tool_result_output_format() {
    assert_upstream_case_covered("mistral-0060", "messages");
}

#[test]
fn mistral_0061_it_should_handle_reference_ids_as_numbers() {
    assert_upstream_case_covered("mistral-0061", "response");
}

#[test]
fn mistral_0062_it_should_handle_reference_ids_as_strings() {
    assert_upstream_case_covered("mistral-0062", "response");
}

#[test]
fn mistral_0063_it_should_handle_mixed_reference_ids() {
    assert_upstream_case_covered("mistral-0063", "response");
}

// mistral-embedding-model.test.ts
#[test]
fn mistral_0064_it_should_extract_embedding() {
    assert_upstream_case_covered("mistral-0064", "embedding");
}

#[test]
fn mistral_0065_it_should_extract_usage_embedding() {
    assert_upstream_case_covered("mistral-0065", "embedding");
}

#[test]
fn mistral_0066_it_should_expose_the_raw_response() {
    assert_upstream_case_covered("mistral-0066", "embedding");
}

#[test]
fn mistral_0067_it_should_pass_the_model_and_the_values() {
    assert_upstream_case_covered("mistral-0067", "embedding");
}

#[test]
fn mistral_0068_it_should_pass_headers_embedding() {
    assert_upstream_case_covered("mistral-0068", "embedding");
}

// mistral-prepare-tools.test.ts
#[test]
fn mistral_0069_it_should_pass_through_strict_mode_when_strict_is_true() {
    assert_upstream_case_covered("mistral-0069", "tools");
}

#[test]
fn mistral_0070_it_should_pass_through_strict_mode_when_strict_is_false() {
    assert_upstream_case_covered("mistral-0070", "tools");
}

#[test]
fn mistral_0071_it_should_not_include_strict_mode_when_strict_is_undefined() {
    assert_upstream_case_covered("mistral-0071", "tools");
}

#[test]
fn mistral_0072_it_should_pass_through_strict_mode_for_multiple_tools_with_different_strict_settings()
 {
    assert_upstream_case_covered("mistral-0072", "tools");
}
