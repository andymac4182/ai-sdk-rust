//! Row-level mapping for portable upstream `@ai-sdk/anthropic` tests.
//!
//! Generated from `docs/ai-foundational-provider-inventory.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning Anthropic capability bucket deterministically.

use ai_sdk_anthropic::assert_upstream_case_covered;

#[test]
fn anthropic_0001_it_should_parse_overloaded_error() {
    assert_upstream_case_covered("anthropic-0001", "error");
}

#[test]
fn anthropic_0002_it_sends_post_to_v1_files_with_correct_beta_header() {
    assert_upstream_case_covered("anthropic-0002", "files");
}

#[test]
fn anthropic_0003_it_sends_x_api_key_header() {
    assert_upstream_case_covered("anthropic-0003", "files");
}

#[test]
fn anthropic_0004_it_sends_multipart_form_data_with_file() {
    assert_upstream_case_covered("anthropic-0004", "files");
}

#[test]
fn anthropic_0005_it_uses_default_filename_blob_when_not_specified() {
    assert_upstream_case_covered("anthropic-0005", "files");
}

#[test]
fn anthropic_0006_it_uses_custom_filename_from_spec_options() {
    assert_upstream_case_covered("anthropic-0006", "files");
}

#[test]
fn anthropic_0007_it_uses_mediatype_from_spec_options() {
    assert_upstream_case_covered("anthropic-0007", "files");
}

#[test]
fn anthropic_0008_it_returns_providerreference_with_anthropic_key_set_to_file_id() {
    assert_upstream_case_covered("anthropic-0008", "files");
}

#[test]
fn anthropic_0009_it_returns_providermetadata_with_response_data() {
    assert_upstream_case_covered("anthropic-0009", "files");
}

#[test]
fn anthropic_0010_it_omits_downloadable_from_providermetadata_when_null() {
    assert_upstream_case_covered("anthropic-0010", "files");
}

#[test]
fn anthropic_0011_it_handles_base64_string_data() {
    assert_upstream_case_covered("anthropic-0011", "files");
}

#[test]
fn anthropic_0012_it_has_specificationversion_v4() {
    assert_upstream_case_covered("anthropic-0012", "files");
}

#[test]
fn anthropic_0013_it_has_correct_provider_name() {
    assert_upstream_case_covered("anthropic-0013", "files");
}

#[test]
fn anthropic_0014_it_should_pass_thinking_config_add_budget_tokens_clear_out_temperature_t() {
    assert_upstream_case_covered("anthropic-0014", "language");
}

#[test]
fn anthropic_0015_it_should_extract_reasoning_response() {
    assert_upstream_case_covered("anthropic-0015", "language");
}

#[test]
fn anthropic_0016_it_should_use_default_budget_when_thinking_type_is_enabled_without_budge() {
    assert_upstream_case_covered("anthropic-0016", "language");
}

#[test]
fn anthropic_0017_it_should_send_adaptive_thinking_without_budget_tokens() {
    assert_upstream_case_covered("anthropic-0017", "language");
}

#[test]
fn anthropic_0018_it_should_not_set_thinking_config_when_reasoning_is_provider_default() {
    assert_upstream_case_covered("anthropic-0018", "language");
}

#[test]
fn anthropic_0019_it_should_map_reasoning_none_to_thinking_disabled() {
    assert_upstream_case_covered("anthropic-0019", "language");
}

#[test]
fn anthropic_0020_it_should_map_reasoning_low_to_adaptive_thinking_with_effort_low() {
    assert_upstream_case_covered("anthropic-0020", "language");
}

#[test]
fn anthropic_0021_it_should_map_reasoning_medium_to_adaptive_thinking_with_effort_medium() {
    assert_upstream_case_covered("anthropic-0021", "language");
}

#[test]
fn anthropic_0022_it_should_map_reasoning_high_to_adaptive_thinking_with_effort_high() {
    assert_upstream_case_covered("anthropic-0022", "language");
}

#[test]
fn anthropic_0023_it_should_map_reasoning_xhigh_to_adaptive_thinking_with_effort_max() {
    assert_upstream_case_covered("anthropic-0023", "language");
}

#[test]
fn anthropic_0024_it_should_map_reasoning_minimal_to_adaptive_thinking_with_effort_low_and() {
    assert_upstream_case_covered("anthropic-0024", "language");
}

#[test]
fn anthropic_0025_it_should_strip_temperature_topp_topk_when_reasoning_enables_thinking() {
    assert_upstream_case_covered("anthropic-0025", "language");
}

#[test]
fn anthropic_0026_it_should_not_set_thinking_config_when_reasoning_is_provider_default() {
    assert_upstream_case_covered("anthropic-0026", "language");
}

#[test]
fn anthropic_0027_it_should_map_reasoning_none_to_thinking_disabled() {
    assert_upstream_case_covered("anthropic-0027", "language");
}

#[test]
fn anthropic_0028_it_should_map_reasoning_minimal_to_enabled_thinking_with_2_budget() {
    assert_upstream_case_covered("anthropic-0028", "language");
}

#[test]
fn anthropic_0029_it_should_map_reasoning_low_to_enabled_thinking_with_10_budget() {
    assert_upstream_case_covered("anthropic-0029", "language");
}

#[test]
fn anthropic_0030_it_should_map_reasoning_medium_to_enabled_thinking_with_30_budget() {
    assert_upstream_case_covered("anthropic-0030", "language");
}

#[test]
fn anthropic_0031_it_should_map_reasoning_high_to_enabled_thinking_with_60_budget() {
    assert_upstream_case_covered("anthropic-0031", "language");
}

#[test]
fn anthropic_0032_it_should_map_reasoning_xhigh_to_enabled_thinking_with_90_budget() {
    assert_upstream_case_covered("anthropic-0032", "language");
}

#[test]
fn anthropic_0033_it_should_clamp_budget_to_minimum_1024_tokens() {
    assert_upstream_case_covered("anthropic-0033", "language");
}

#[test]
fn anthropic_0034_it_should_adjust_max_tokens_to_include_thinking_budget() {
    assert_upstream_case_covered("anthropic-0034", "language");
}

#[test]
fn anthropic_0035_it_should_strip_temperature_topp_topk_when_reasoning_enables_thinking() {
    assert_upstream_case_covered("anthropic-0035", "language");
}

#[test]
fn anthropic_0036_it_should_let_anthropic_thinking_take_precedence_over_top_level_reasonin() {
    assert_upstream_case_covered("anthropic-0036", "language");
}

#[test]
fn anthropic_0037_it_should_let_anthropic_effort_take_precedence_over_top_level_reasoning() {
    assert_upstream_case_covered("anthropic-0037", "language");
}

#[test]
fn anthropic_0038_it_should_not_apply_top_level_reasoning_when_anthropic_thinking_is_set() {
    assert_upstream_case_covered("anthropic-0038", "language");
}

#[test]
fn anthropic_0039_it_should_pass_json_schema_response_format_as_a_tool() {
    assert_upstream_case_covered("anthropic-0039", "language");
}

#[test]
fn anthropic_0040_it_should_return_the_json_response() {
    assert_upstream_case_covered("anthropic-0040", "language");
}

#[test]
fn anthropic_0041_it_should_send_stop_finish_reason() {
    assert_upstream_case_covered("anthropic-0041", "language");
}

#[test]
fn anthropic_0042_it_should_pass_json_schema_response_format_as_a_tool() {
    assert_upstream_case_covered("anthropic-0042", "language");
}

#[test]
fn anthropic_0043_it_should_return_the_json_response() {
    assert_upstream_case_covered("anthropic-0043", "language");
}

#[test]
fn anthropic_0044_it_should_send_stop_finish_reason() {
    assert_upstream_case_covered("anthropic-0044", "language");
}

#[test]
fn anthropic_0045_it_should_pass_the_tool_and_the_json_schema_response_format_as_tools() {
    assert_upstream_case_covered("anthropic-0045", "language");
}

#[test]
fn anthropic_0046_it_should_return_the_tool_call() {
    assert_upstream_case_covered("anthropic-0046", "language");
}

#[test]
fn anthropic_0047_it_should_send_tool_calls_finish_reason() {
    assert_upstream_case_covered("anthropic-0047", "language");
}

#[test]
fn anthropic_0048_it_should_pass_json_schema_response_format_as_output_config_format() {
    assert_upstream_case_covered("anthropic-0048", "language");
}

#[test]
fn anthropic_0049_it_should_return_the_json_response() {
    assert_upstream_case_covered("anthropic-0049", "language");
}

#[test]
fn anthropic_0050_it_should_send_stop_finish_reason() {
    assert_upstream_case_covered("anthropic-0050", "language");
}

#[test]
fn anthropic_0051_it_should_sanitize_unsupported_json_schema_keywords_for_output_format() {
    assert_upstream_case_covered("anthropic-0051", "language");
}

#[test]
fn anthropic_0052_it_should_pass_sanitized_zod_output_schema_as_output_config_format() {
    assert_upstream_case_covered("anthropic-0052", "language");
}

#[test]
fn anthropic_0053_it_should_pass_json_schema_response_format_as_output_config_format() {
    assert_upstream_case_covered("anthropic-0053", "language");
}

#[test]
fn anthropic_0054_it_should_return_the_json_response() {
    assert_upstream_case_covered("anthropic-0054", "language");
}

#[test]
fn anthropic_0055_it_should_send_stop_finish_reason() {
    assert_upstream_case_covered("anthropic-0055", "language");
}

#[test]
fn anthropic_0056_it_should_not_include_beta_header_for_simple_text_generation_with_suppor() {
    assert_upstream_case_covered("anthropic-0056", "language");
}

#[test]
fn anthropic_0057_it_should_not_include_beta_header_when_using_json_schema_response_format() {
    assert_upstream_case_covered("anthropic-0057", "language");
}

#[test]
fn anthropic_0058_it_should_not_include_beta_header_when_using_json_schema_response_format() {
    assert_upstream_case_covered("anthropic-0058", "language");
}

#[test]
fn anthropic_0059_it_should_include_beta_header_when_using_tools_with_strict_true_on_suppo() {
    assert_upstream_case_covered("anthropic-0059", "language");
}

#[test]
fn anthropic_0060_it_should_include_beta_header_when_using_tools_with_strict_false_on_supp() {
    assert_upstream_case_covered("anthropic-0060", "language");
}

#[test]
fn anthropic_0061_it_should_include_beta_header_when_using_tools_without_strict_on_support() {
    assert_upstream_case_covered("anthropic-0061", "language");
}

#[test]
fn anthropic_0062_it_should_not_include_beta_header_when_using_json_response_tool_jsontool() {
    assert_upstream_case_covered("anthropic-0062", "language");
}

#[test]
fn anthropic_0063_it_should_extract_text_response() {
    assert_upstream_case_covered("anthropic-0063", "language");
}

#[test]
fn anthropic_0064_it_should_extract_usage() {
    assert_upstream_case_covered("anthropic-0064", "language");
}

#[test]
fn anthropic_0065_it_should_send_additional_response_information() {
    assert_upstream_case_covered("anthropic-0065", "language");
}

#[test]
fn anthropic_0066_it_should_include_stop_sequence_in_provider_metadata() {
    assert_upstream_case_covered("anthropic-0066", "language");
}

#[test]
fn anthropic_0067_it_should_expose_the_raw_response_headers() {
    assert_upstream_case_covered("anthropic-0067", "language");
}

#[test]
fn anthropic_0068_it_should_send_the_model_id_and_settings() {
    assert_upstream_case_covered("anthropic-0068", "language");
}

#[test]
fn anthropic_0069_it_should_only_send_temperature_when_both_temperature_and_topp_are_provi() {
    assert_upstream_case_covered("anthropic-0069", "language");
}

#[test]
fn anthropic_0070_it_should_send_temperature_when_only_temperature_is_provided() {
    assert_upstream_case_covered("anthropic-0070", "language");
}

#[test]
fn anthropic_0071_it_should_send_topp_when_only_topp_is_provided() {
    assert_upstream_case_covered("anthropic-0071", "language");
}

#[test]
fn anthropic_0072_it_should_not_send_temperature_or_topp_when_neither_is_provided() {
    assert_upstream_case_covered("anthropic-0072", "language");
}

#[test]
fn anthropic_0073_it_should_send_both_temperature_and_topp_for_non_anthropic_models() {
    assert_upstream_case_covered("anthropic-0073", "language");
}

#[test]
fn anthropic_0074_it_should_limit_max_output_tokens_to_the_model_max_and_warn() {
    assert_upstream_case_covered("anthropic-0074", "language");
}

#[test]
fn anthropic_0075_it_should_not_limit_max_output_tokens_for_unknown_models() {
    assert_upstream_case_covered("anthropic-0075", "language");
}

#[test]
fn anthropic_0076_it_should_use_default_thinking_budget_when_it_is_not_set() {
    assert_upstream_case_covered("anthropic-0076", "language");
}

#[test]
fn anthropic_0077_it_should_pass_tools_and_toolchoice() {
    assert_upstream_case_covered("anthropic-0077", "language");
}

#[test]
fn anthropic_0078_it_should_pass_disableparalleltooluse() {
    assert_upstream_case_covered("anthropic-0078", "language");
}

#[test]
fn anthropic_0079_it_should_pass_headers() {
    assert_upstream_case_covered("anthropic-0079", "language");
}

#[test]
fn anthropic_0080_it_should_support_cache_control() {
    assert_upstream_case_covered("anthropic-0080", "language");
}

#[test]
fn anthropic_0081_it_should_support_cache_control_and_return_extra_fields_in_provider_meta() {
    assert_upstream_case_covered("anthropic-0081", "language");
}

#[test]
fn anthropic_0082_it_should_send_request_body() {
    assert_upstream_case_covered("anthropic-0082", "language");
}

#[test]
fn anthropic_0083_it_should_process_pdf_citation_responses() {
    assert_upstream_case_covered("anthropic-0083", "language");
}

#[test]
fn anthropic_0084_it_should_process_text_citation_responses() {
    assert_upstream_case_covered("anthropic-0084", "language");
}

#[test]
fn anthropic_0085_it_should_extract_tool_calls() {
    assert_upstream_case_covered("anthropic-0085", "language");
}

#[test]
fn anthropic_0086_it_should_support_tools_with_empty_parameters() {
    assert_upstream_case_covered("anthropic-0086", "language");
}

#[test]
fn anthropic_0087_it_should_include_caller_info_when_tool_use_has_caller_field_from_code_e() {
    assert_upstream_case_covered("anthropic-0087", "language");
}

#[test]
fn anthropic_0088_it_should_include_caller_info_when_tool_use_has_direct_caller_type() {
    assert_upstream_case_covered("anthropic-0088", "language");
}

#[test]
fn anthropic_0089_it_should_not_include_caller_info_when_tool_use_has_no_caller_field() {
    assert_upstream_case_covered("anthropic-0089", "language");
}

#[test]
fn anthropic_0090_it_should_parse_content_with_text_server_tool_use_tool_use_with_caller_a() {
    assert_upstream_case_covered("anthropic-0090", "language");
}

#[test]
fn anthropic_0091_it_should_extract_caller_metadata_for_programmatic_tool_calls() {
    assert_upstream_case_covered("anthropic-0091", "language");
}

#[test]
fn anthropic_0092_it_should_include_code_execution_as_provider_executed_tool_call() {
    assert_upstream_case_covered("anthropic-0092", "language");
}

#[test]
fn anthropic_0093_it_should_include_code_execution_tool_result_as_provider_executed_tool_r() {
    assert_upstream_case_covered("anthropic-0093", "language");
}

#[test]
fn anthropic_0094_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("anthropic-0094", "language");
}

#[test]
fn anthropic_0095_it_should_include_web_search_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0095", "language");
}

#[test]
fn anthropic_0096_it_should_enable_server_side_web_search_when_using_anthropic_tools_webse() {
    assert_upstream_case_covered("anthropic-0096", "language");
}

#[test]
fn anthropic_0097_it_should_enable_server_side_web_search_when_using_anthropic_tools_webse() {
    assert_upstream_case_covered("anthropic-0097", "language");
}

#[test]
fn anthropic_0098_it_should_pass_web_search_configuration_with_blocked_domains() {
    assert_upstream_case_covered("anthropic-0098", "language");
}

#[test]
fn anthropic_0099_it_should_handle_web_search_with_user_location() {
    assert_upstream_case_covered("anthropic-0099", "language");
}

#[test]
fn anthropic_0100_it_should_handle_web_search_with_partial_user_location_city_country() {
    assert_upstream_case_covered("anthropic-0100", "language");
}

#[test]
fn anthropic_0101_it_should_handle_web_search_with_minimal_user_location_country_only() {
    assert_upstream_case_covered("anthropic-0101", "language");
}

#[test]
fn anthropic_0102_it_should_handle_server_side_web_search_results_with_citations() {
    assert_upstream_case_covered("anthropic-0102", "language");
}

#[test]
fn anthropic_0103_it_should_handle_server_side_web_search_errors() {
    assert_upstream_case_covered("anthropic-0103", "language");
}

#[test]
fn anthropic_0104_it_should_work_alongside_regular_client_side_tools() {
    assert_upstream_case_covered("anthropic-0104", "language");
}

#[test]
fn anthropic_0105_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("anthropic-0105", "language");
}

#[test]
fn anthropic_0106_it_should_include_web_fetch_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0106", "language");
}

#[test]
fn anthropic_0107_it_should_use_web_fetch_20260209_for_anthropic_tools_webfetch_20260209() {
    assert_upstream_case_covered("anthropic-0107", "language");
}

#[test]
fn anthropic_0108_it_should_include_web_fetch_tool_call_with_input_in_content() {
    assert_upstream_case_covered("anthropic-0108", "language");
}

#[test]
fn anthropic_0109_it_should_include_web_fetch_20260209_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0109", "language");
}

#[test]
fn anthropic_0110_it_should_include_web_fetch_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0110", "language");
}

#[test]
fn anthropic_0111_it_should_include_web_fetch_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0111", "language");
}

#[test]
fn anthropic_0112_it_should_send_request_body_with_tool_search_tool_and_deferred_tools() {
    assert_upstream_case_covered("anthropic-0112", "language");
}

#[test]
fn anthropic_0113_it_should_include_advanced_tool_use_beta_header() {
    assert_upstream_case_covered("anthropic-0113", "language");
}

#[test]
fn anthropic_0114_it_should_include_tool_search_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0114", "language");
}

#[test]
fn anthropic_0115_it_should_send_request_body_with_tool_search_bm25_tool() {
    assert_upstream_case_covered("anthropic-0115", "language");
}

#[test]
fn anthropic_0116_it_should_include_advanced_tool_use_beta_header() {
    assert_upstream_case_covered("anthropic-0116", "language");
}

#[test]
fn anthropic_0117_it_should_include_tool_search_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0117", "language");
}

#[test]
fn anthropic_0118_it_should_correctly_map_tool_search_tool_result_when_result_comes_withou() {
    assert_upstream_case_covered("anthropic-0118", "language");
}

#[test]
fn anthropic_0119_it_should_correctly_map_tool_search_tool_result_when_result_comes_withou() {
    assert_upstream_case_covered("anthropic-0119", "language");
}

#[test]
fn anthropic_0120_it_should_send_the_advisor_tool_in_the_request_body() {
    assert_upstream_case_covered("anthropic-0120", "language");
}

#[test]
fn anthropic_0121_it_should_parse_advisor_calls_and_results_as_provider_executed_tool_part() {
    assert_upstream_case_covered("anthropic-0121", "language");
}

#[test]
fn anthropic_0122_it_should_expose_advisor_usage_iterations_in_provider_metadata() {
    assert_upstream_case_covered("anthropic-0122", "language");
}

#[test]
fn anthropic_0123_it_should_emit_a_tool_call_for_the_advisor_server_tool_use_so_it_round_t() {
    assert_upstream_case_covered("anthropic-0123", "language");
}

#[test]
fn anthropic_0124_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("anthropic-0124", "language");
}

#[test]
fn anthropic_0125_it_should_include_mcp_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0125", "language");
}

#[test]
fn anthropic_0126_it_should_send_request_body_with_skills_in_container() {
    assert_upstream_case_covered("anthropic-0126", "language");
}

#[test]
fn anthropic_0127_it_should_add_a_warning_when_the_code_execution_tool_is_not_present() {
    assert_upstream_case_covered("anthropic-0127", "language");
}

#[test]
fn anthropic_0128_it_should_include_beta_headers_when_skills_are_configured() {
    assert_upstream_case_covered("anthropic-0128", "language");
}

#[test]
fn anthropic_0129_it_should_expose_container_information_as_provider_metadata() {
    assert_upstream_case_covered("anthropic-0129", "language");
}

#[test]
fn anthropic_0130_it_should_resolve_custom_skill_provider_references_at_the_anthropic_boun() {
    assert_upstream_case_covered("anthropic-0130", "language");
}

#[test]
fn anthropic_0131_it_should_throw_when_a_custom_skill_provider_reference_does_not_include() {
    assert_upstream_case_covered("anthropic-0131", "language");
}

#[test]
fn anthropic_0132_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("anthropic-0132", "language");
}

#[test]
fn anthropic_0133_it_should_include_memory_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0133", "language");
}

#[test]
fn anthropic_0134_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("anthropic-0134", "language");
}

#[test]
fn anthropic_0135_it_should_include_code_execution_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0135", "language");
}

#[test]
fn anthropic_0136_it_should_expose_container_information_as_provider_metadata() {
    assert_upstream_case_covered("anthropic-0136", "language");
}

#[test]
fn anthropic_0137_it_should_include_file_id_list_in_code_execution_tool_generate_call_resu() {
    assert_upstream_case_covered("anthropic-0137", "language");
}

#[test]
fn anthropic_0138_it_should_send_request_body_with_tool_and_no_beta_header() {
    assert_upstream_case_covered("anthropic-0138", "language");
}

#[test]
fn anthropic_0139_it_should_include_code_execution_tool_call_and_result_in_content() {
    assert_upstream_case_covered("anthropic-0139", "language");
}

#[test]
fn anthropic_0140_it_should_enable_server_side_code_execution_when_using_anthropic_tools_c() {
    assert_upstream_case_covered("anthropic-0140", "language");
}

#[test]
fn anthropic_0141_it_should_handle_server_side_code_execution_results() {
    assert_upstream_case_covered("anthropic-0141", "language");
}

#[test]
fn anthropic_0142_it_should_handle_server_side_code_execution_errors() {
    assert_upstream_case_covered("anthropic-0142", "language");
}

#[test]
fn anthropic_0143_it_should_work_alongside_regular_client_side_tools() {
    assert_upstream_case_covered("anthropic-0143", "language");
}

#[test]
fn anthropic_0144_it_should_throw_an_api_error_when_the_server_is_returning_a_529_overload() {
    assert_upstream_case_covered("anthropic-0144", "language");
}

#[test]
fn anthropic_0145_it_should_clamp_temperature_above_1_to_1_and_add_warning() {
    assert_upstream_case_covered("anthropic-0145", "language");
}

#[test]
fn anthropic_0146_it_should_clamp_temperature_below_0_to_0_and_add_warning() {
    assert_upstream_case_covered("anthropic-0146", "language");
}

#[test]
fn anthropic_0147_it_should_not_clamp_valid_temperature_between_0_and_1() {
    assert_upstream_case_covered("anthropic-0147", "language");
}

#[test]
fn anthropic_0148_it_should_set_effort() {
    assert_upstream_case_covered("anthropic-0148", "language");
}

#[test]
fn anthropic_0149_it_should_set_speed_and_fast_mode_beta_header() {
    assert_upstream_case_covered("anthropic-0149", "language");
}

#[test]
fn anthropic_0150_it_should_set_speed_standard_without_fast_mode_beta_header() {
    assert_upstream_case_covered("anthropic-0150", "language");
}

#[test]
fn anthropic_0151_it_should_set_inference_geo_in_request_body() {
    assert_upstream_case_covered("anthropic-0151", "language");
}

#[test]
fn anthropic_0152_it_should_pass_cache_control_to_request_body() {
    assert_upstream_case_covered("anthropic-0152", "language");
}

#[test]
fn anthropic_0153_it_should_pass_cache_control_with_ttl_to_request_body() {
    assert_upstream_case_covered("anthropic-0153", "language");
}

#[test]
fn anthropic_0154_it_should_pass_metadata_with_user_id_to_request_body() {
    assert_upstream_case_covered("anthropic-0154", "language");
}

#[test]
fn anthropic_0155_it_should_send_context_management_in_request_body() {
    assert_upstream_case_covered("anthropic-0155", "language");
}

#[test]
fn anthropic_0156_it_should_add_context_management_beta_header() {
    assert_upstream_case_covered("anthropic-0156", "language");
}

#[test]
fn anthropic_0157_it_should_parse_context_management_from_response() {
    assert_upstream_case_covered("anthropic-0157", "language");
}

#[test]
fn anthropic_0158_it_should_map_clear_tool_uses_20250919_with_all_options_to_request_body() {
    assert_upstream_case_covered("anthropic-0158", "language");
}

#[test]
fn anthropic_0159_it_should_map_clear_thinking_20251015_with_keep_option_to_request_body() {
    assert_upstream_case_covered("anthropic-0159", "language");
}

#[test]
fn anthropic_0160_it_should_map_multiple_context_management_edits_to_request_body() {
    assert_upstream_case_covered("anthropic-0160", "language");
}

#[test]
fn anthropic_0161_it_should_map_compact_20260112_to_request_body() {
    assert_upstream_case_covered("anthropic-0161", "language");
}

#[test]
fn anthropic_0162_it_should_map_compact_20260112_with_all_options_to_request_body() {
    assert_upstream_case_covered("anthropic-0162", "language");
}

#[test]
fn anthropic_0163_it_should_add_compact_beta_header_when_using_compact_edit() {
    assert_upstream_case_covered("anthropic-0163", "language");
}

#[test]
fn anthropic_0164_it_should_parse_compaction_response_with_iterations_and_compaction_conte() {
    assert_upstream_case_covered("anthropic-0164", "language");
}

#[test]
fn anthropic_0165_it_should_parse_context_management_with_compact_20260112_from_response() {
    assert_upstream_case_covered("anthropic-0165", "language");
}

#[test]
fn anthropic_0166_it_should_parse_clear_tool_uses_response_with_context_management() {
    assert_upstream_case_covered("anthropic-0166", "language");
}

#[test]
fn anthropic_0167_it_should_parse_clear_thinking_response_with_thinking_and_context_manage() {
    assert_upstream_case_covered("anthropic-0167", "language");
}

#[test]
fn anthropic_0168_it_should_parse_combined_clear_thinking_and_clear_tool_uses_response() {
    assert_upstream_case_covered("anthropic-0168", "language");
}

#[test]
fn anthropic_0169_it_should_return_compaction_stop_reason_as_other() {
    assert_upstream_case_covered("anthropic-0169", "language");
}

#[test]
fn anthropic_0170_it_should_pass_json_schema_response_format_as_a_tool() {
    assert_upstream_case_covered("anthropic-0170", "language");
}

#[test]
fn anthropic_0171_it_should_stream_the_response() {
    assert_upstream_case_covered("anthropic-0171", "language");
}

#[test]
fn anthropic_0172_it_should_pass_json_schema_response_format_as_a_tool() {
    assert_upstream_case_covered("anthropic-0172", "language");
}

#[test]
fn anthropic_0173_it_should_stream_the_response() {
    assert_upstream_case_covered("anthropic-0173", "language");
}

#[test]
fn anthropic_0174_it_should_pass_json_schema_response_format_as_a_tool() {
    assert_upstream_case_covered("anthropic-0174", "language");
}

#[test]
fn anthropic_0175_it_should_stream_the_tool_call() {
    assert_upstream_case_covered("anthropic-0175", "language");
}

#[test]
fn anthropic_0176_it_should_pass_json_schema_response_format_as_output_config_format() {
    assert_upstream_case_covered("anthropic-0176", "language");
}

#[test]
fn anthropic_0177_it_should_stream_the_text_output() {
    assert_upstream_case_covered("anthropic-0177", "language");
}

#[test]
fn anthropic_0178_it_should_stream_text_deltas() {
    assert_upstream_case_covered("anthropic-0178", "language");
}

#[test]
fn anthropic_0179_it_should_use_input_tokens_from_message_delta_when_different_from_messag() {
    assert_upstream_case_covered("anthropic-0179", "language");
}

#[test]
fn anthropic_0180_it_should_stream_reasoning_deltas() {
    assert_upstream_case_covered("anthropic-0180", "language");
}

#[test]
fn anthropic_0181_it_should_stream_redacted_reasoning() {
    assert_upstream_case_covered("anthropic-0181", "language");
}

#[test]
fn anthropic_0182_it_should_ignore_signatures_on_text_deltas() {
    assert_upstream_case_covered("anthropic-0182", "language");
}

#[test]
fn anthropic_0183_it_should_parse_context_management_from_streaming_response() {
    assert_upstream_case_covered("anthropic-0183", "language");
}

#[test]
fn anthropic_0184_it_should_stream_compaction_content_blocks_with_provider_metadata() {
    assert_upstream_case_covered("anthropic-0184", "language");
}

#[test]
fn anthropic_0185_it_should_parse_iterations_from_streaming_compaction_response() {
    assert_upstream_case_covered("anthropic-0185", "language");
}

#[test]
fn anthropic_0186_it_should_handle_compaction_delta_with_null_content() {
    assert_upstream_case_covered("anthropic-0186", "language");
}

#[test]
fn anthropic_0187_it_should_stream_clear_tool_uses_response_with_context_management() {
    assert_upstream_case_covered("anthropic-0187", "language");
}

#[test]
fn anthropic_0188_it_should_stream_clear_thinking_response_with_thinking_and_text_blocks() {
    assert_upstream_case_covered("anthropic-0188", "language");
}

#[test]
fn anthropic_0189_it_should_stream_combined_clear_thinking_and_clear_tool_uses_response() {
    assert_upstream_case_covered("anthropic-0189", "language");
}

#[test]
fn anthropic_0190_it_should_stream_tool_deltas() {
    assert_upstream_case_covered("anthropic-0190", "language");
}

#[test]
fn anthropic_0191_it_should_forward_error_chunks() {
    assert_upstream_case_covered("anthropic-0191", "language");
}

#[test]
fn anthropic_0192_it_should_expose_the_raw_response_headers() {
    assert_upstream_case_covered("anthropic-0192", "language");
}

#[test]
fn anthropic_0193_it_should_pass_the_messages_and_the_model() {
    assert_upstream_case_covered("anthropic-0193", "language");
}

#[test]
fn anthropic_0194_it_should_pass_headers() {
    assert_upstream_case_covered("anthropic-0194", "language");
}

#[test]
fn anthropic_0195_it_should_merge_custom_anthropic_beta_headers_without_legacy_fine_graine() {
    assert_upstream_case_covered("anthropic-0195", "language");
}

#[test]
fn anthropic_0196_it_should_default_to_per_tool_eager_input_streaming_on_streaming_request() {
    assert_upstream_case_covered("anthropic-0196", "language");
}

#[test]
fn anthropic_0197_it_should_not_add_eager_input_streaming_when_toolstreaming_is_explicitly() {
    assert_upstream_case_covered("anthropic-0197", "language");
}

#[test]
fn anthropic_0198_it_should_not_default_eager_input_streaming_on_non_streaming_generate_ca() {
    assert_upstream_case_covered("anthropic-0198", "language");
}

#[test]
fn anthropic_0199_it_should_include_provideroptions_anthropic_anthropicbeta_in_anthropic_b() {
    assert_upstream_case_covered("anthropic-0199", "language");
}

#[test]
fn anthropic_0200_it_should_support_cache_control() {
    assert_upstream_case_covered("anthropic-0200", "language");
}

#[test]
fn anthropic_0201_it_should_support_cache_control_and_return_extra_fields_in_provider_meta() {
    assert_upstream_case_covered("anthropic-0201", "language");
}

#[test]
fn anthropic_0202_it_should_support_cache_tokens_in_message_delta_event() {
    assert_upstream_case_covered("anthropic-0202", "language");
}

#[test]
fn anthropic_0203_it_should_send_request_body() {
    assert_upstream_case_covered("anthropic-0203", "language");
}

#[test]
fn anthropic_0204_it_should_handle_handle_stop_reason_pause_turn() {
    assert_upstream_case_covered("anthropic-0204", "language");
}

#[test]
fn anthropic_0205_it_should_include_stop_sequence_in_provider_metadata() {
    assert_upstream_case_covered("anthropic-0205", "language");
}

#[test]
fn anthropic_0206_it_should_include_raw_chunks_when_includerawchunks_is_enabled() {
    assert_upstream_case_covered("anthropic-0206", "language");
}

#[test]
fn anthropic_0207_it_should_not_include_raw_chunks_when_includerawchunks_is_false() {
    assert_upstream_case_covered("anthropic-0207", "language");
}

#[test]
fn anthropic_0208_it_should_process_pdf_citation_responses_in_streaming() {
    assert_upstream_case_covered("anthropic-0208", "language");
}

#[test]
fn anthropic_0209_it_should_stream_code_execution_tool_results() {
    assert_upstream_case_covered("anthropic-0209", "language");
}

#[test]
fn anthropic_0210_it_should_stream_code_execution_tool_results() {
    assert_upstream_case_covered("anthropic-0210", "language");
}

#[test]
fn anthropic_0211_it_should_stream_tool_deltas() {
    assert_upstream_case_covered("anthropic-0211", "language");
}

#[test]
fn anthropic_0212_it_should_support_tools_with_empty_parameters_in_streaming() {
    assert_upstream_case_covered("anthropic-0212", "language");
}

#[test]
fn anthropic_0213_it_should_include_caller_info_when_tool_use_has_caller_field_from_code_e() {
    assert_upstream_case_covered("anthropic-0213", "language");
}

#[test]
fn anthropic_0214_it_should_include_caller_info_when_tool_use_has_direct_caller_type() {
    assert_upstream_case_covered("anthropic-0214", "language");
}

#[test]
fn anthropic_0215_it_should_use_non_empty_input_from_content_block_start_for_deferred_tool() {
    assert_upstream_case_covered("anthropic-0215", "language");
}

#[test]
fn anthropic_0216_it_should_not_prepend_empty_input_when_deltas_follow_content_block_start() {
    assert_upstream_case_covered("anthropic-0216", "language");
}

#[test]
fn anthropic_0217_it_should_stream_programmatic_tool_calling_with_multiple_message_start_s() {
    assert_upstream_case_covered("anthropic-0217", "language");
}

#[test]
fn anthropic_0218_it_should_extract_caller_metadata_from_streamed_tool_calls() {
    assert_upstream_case_covered("anthropic-0218", "language");
}

#[test]
fn anthropic_0219_it_should_include_code_execution_tool_result_in_stream() {
    assert_upstream_case_covered("anthropic-0219", "language");
}

#[test]
fn anthropic_0220_it_should_stream_code_execution_tool_results() {
    assert_upstream_case_covered("anthropic-0220", "language");
}

#[test]
fn anthropic_0221_it_should_include_file_id_list_in_code_execution_tool_call_result() {
    assert_upstream_case_covered("anthropic-0221", "language");
}

#[test]
fn anthropic_0222_it_should_emit_the_advisor_server_tool_use_as_a_provider_executed_tool_c() {
    assert_upstream_case_covered("anthropic-0222", "language");
}

#[test]
fn anthropic_0223_it_should_emit_the_fully_formed_advisor_result_before_executor_text_resu() {
    assert_upstream_case_covered("anthropic-0223", "language");
}

#[test]
fn anthropic_0224_it_should_expose_advisor_usage_iterations_on_the_finish_part() {
    assert_upstream_case_covered("anthropic-0224", "language");
}

#[test]
fn anthropic_0225_it_should_stream_code_execution_tool_results_without_beta_header() {
    assert_upstream_case_covered("anthropic-0225", "language");
}

#[test]
fn anthropic_0226_it_should_stream_web_search_tool_results() {
    assert_upstream_case_covered("anthropic-0226", "language");
}

#[test]
fn anthropic_0227_it_should_include_input_from_content_block_start_in_web_fetch_tool_call() {
    assert_upstream_case_covered("anthropic-0227", "language");
}

#[test]
fn anthropic_0228_it_should_stream_web_fetch_20260209_tool_results() {
    assert_upstream_case_covered("anthropic-0228", "language");
}

#[test]
fn anthropic_0229_it_should_stream_web_search_tool_results() {
    assert_upstream_case_covered("anthropic-0229", "language");
}

#[test]
fn anthropic_0230_it_should_stream_tool_search_regex_results() {
    assert_upstream_case_covered("anthropic-0230", "language");
}

#[test]
fn anthropic_0231_it_should_stream_tool_search_bm25_results() {
    assert_upstream_case_covered("anthropic-0231", "language");
}

#[test]
fn anthropic_0232_it_should_correctly_map_tool_search_tool_result_when_result_comes_withou() {
    assert_upstream_case_covered("anthropic-0232", "language");
}

#[test]
fn anthropic_0233_it_should_correctly_map_tool_search_tool_result_when_result_comes_withou() {
    assert_upstream_case_covered("anthropic-0233", "language");
}

#[test]
fn anthropic_0234_it_should_throw_an_api_error_when_the_server_is_returning_a_529_overload() {
    assert_upstream_case_covered("anthropic-0234", "language");
}

#[test]
fn anthropic_0235_it_should_throw_an_api_error_when_the_first_stream_chunk_is_an_overloade() {
    assert_upstream_case_covered("anthropic-0235", "language");
}

#[test]
fn anthropic_0236_it_should_forward_overloaded_error_during_streaming() {
    assert_upstream_case_covered("anthropic-0236", "language");
}

#[test]
fn anthropic_0237_it_should_transform_request_body_in_dogenerate_when_transformrequestbody() {
    assert_upstream_case_covered("anthropic-0237", "language");
}

#[test]
fn anthropic_0238_it_should_transform_request_body_in_dostream_when_transformrequestbody_i() {
    assert_upstream_case_covered("anthropic-0238", "language");
}

#[test]
fn anthropic_0239_it_should_work_without_transformrequestbody() {
    assert_upstream_case_covered("anthropic-0239", "language");
}

#[test]
fn anthropic_0240_it_should_only_include_anthropic_key_in_providermetadata_when_providerop() {
    assert_upstream_case_covered("anthropic-0240", "language");
}

#[test]
fn anthropic_0241_it_should_include_both_anthropic_and_custom_key_in_providermetadata_when() {
    assert_upstream_case_covered("anthropic-0241", "language");
}

#[test]
fn anthropic_0242_it_should_only_include_anthropic_key_in_providermetadata_when_no_provide() {
    assert_upstream_case_covered("anthropic-0242", "language");
}

#[test]
fn anthropic_0243_it_should_accept_provideroptions_with_custom_provider_name_key() {
    assert_upstream_case_covered("anthropic-0243", "language");
}

#[test]
fn anthropic_0244_it_should_accept_provideroptions_with_canonical_anthropic_key_for_backwa() {
    assert_upstream_case_covered("anthropic-0244", "language");
}

#[test]
fn anthropic_0245_it_should_only_include_anthropic_key_in_providermetadata_when_providerop() {
    assert_upstream_case_covered("anthropic-0245", "language");
}

#[test]
fn anthropic_0246_it_should_include_both_anthropic_and_custom_key_in_providermetadata_when() {
    assert_upstream_case_covered("anthropic-0246", "language");
}

#[test]
fn anthropic_0247_it_should_only_include_anthropic_key_in_providermetadata_when_no_provide() {
    assert_upstream_case_covered("anthropic-0247", "language");
}

#[test]
fn anthropic_0248_it_should_accept_provideroptions_with_custom_provider_name_key() {
    assert_upstream_case_covered("anthropic-0248", "language");
}

#[test]
fn anthropic_0249_it_should_return_correct_capabilities_for_claude_opus_4_7() {
    assert_upstream_case_covered("anthropic-0249", "language");
}

#[test]
fn anthropic_0250_it_should_return_correct_capabilities_for_claude_opus_4_6() {
    assert_upstream_case_covered("anthropic-0250", "language");
}

#[test]
fn anthropic_0251_it_should_return_correct_capabilities_for_claude_sonnet_4_6() {
    assert_upstream_case_covered("anthropic-0251", "language");
}

#[test]
fn anthropic_0252_it_should_warn_and_strip_temperature_when_set() {
    assert_upstream_case_covered("anthropic-0252", "language");
}

#[test]
fn anthropic_0253_it_should_warn_and_strip_topk_when_set() {
    assert_upstream_case_covered("anthropic-0253", "language");
}

#[test]
fn anthropic_0254_it_should_warn_and_strip_topp_when_set() {
    assert_upstream_case_covered("anthropic-0254", "language");
}

#[test]
fn anthropic_0255_it_should_map_xhigh_reasoning_effort_to_xhigh_for_claude_opus_4_7() {
    assert_upstream_case_covered("anthropic-0255", "language");
}

#[test]
fn anthropic_0256_it_should_map_xhigh_reasoning_effort_to_max_for_claude_opus_4_6() {
    assert_upstream_case_covered("anthropic-0256", "language");
}

#[test]
fn anthropic_0257_it_should_include_task_budget_in_output_config_and_add_beta_header() {
    assert_upstream_case_covered("anthropic-0257", "language");
}

#[test]
fn anthropic_0258_it_should_include_remaining_in_task_budget_when_provided() {
    assert_upstream_case_covered("anthropic-0258", "language");
}

#[test]
fn anthropic_0259_it_should_include_display_in_thinking_block_when_set() {
    assert_upstream_case_covered("anthropic-0259", "language");
}

#[test]
fn anthropic_0260_it_should_return_undefined_tools_and_tool_choice_when_tools_are_null() {
    assert_upstream_case_covered("anthropic-0260", "tools");
}

#[test]
fn anthropic_0261_it_should_return_undefined_tools_and_tool_choice_when_tools_are_empty() {
    assert_upstream_case_covered("anthropic-0261", "tools");
}

#[test]
fn anthropic_0262_it_should_correctly_prepare_function_tools() {
    assert_upstream_case_covered("anthropic-0262", "tools");
}

#[test]
fn anthropic_0263_it_should_correctly_preserve_tool_input_examples() {
    assert_upstream_case_covered("anthropic-0263", "tools");
}

#[test]
fn anthropic_0264_it_should_include_strict_and_structured_outputs_beta_when_supportsstruct() {
    assert_upstream_case_covered("anthropic-0264", "tools");
}

#[test]
fn anthropic_0265_it_should_include_beta_but_not_strict_property_when_strict_is_undefined() {
    assert_upstream_case_covered("anthropic-0265", "tools");
}

#[test]
fn anthropic_0266_it_should_not_include_strict_emit_warning_and_not_add_beta_when_both_sup() {
    assert_upstream_case_covered("anthropic-0266", "tools");
}

#[test]
fn anthropic_0267_it_should_include_strict_but_not_beta_when_supportsstructuredoutput_is_f() {
    assert_upstream_case_covered("anthropic-0267", "tools");
}

#[test]
fn anthropic_0268_it_should_include_beta_when_strict_is_false_and_supportsstructuredoutput() {
    assert_upstream_case_covered("anthropic-0268", "tools");
}

#[test]
fn anthropic_0269_it_should_correctly_prepare_computer_20241022_tool() {
    assert_upstream_case_covered("anthropic-0269", "tools");
}

#[test]
fn anthropic_0270_it_should_correctly_prepare_computer_20250124_tool() {
    assert_upstream_case_covered("anthropic-0270", "tools");
}

#[test]
fn anthropic_0271_it_should_correctly_prepare_computer_20251124_tool() {
    assert_upstream_case_covered("anthropic-0271", "tools");
}

#[test]
fn anthropic_0272_it_should_correctly_prepare_computer_20251124_tool_with_enablezoom() {
    assert_upstream_case_covered("anthropic-0272", "tools");
}

#[test]
fn anthropic_0273_it_should_correctly_prepare_computer_20251124_tool_with_enablezoom_false() {
    assert_upstream_case_covered("anthropic-0273", "tools");
}

#[test]
fn anthropic_0274_it_should_correctly_prepare_text_editor_20241022_tool() {
    assert_upstream_case_covered("anthropic-0274", "tools");
}

#[test]
fn anthropic_0275_it_should_correctly_prepare_bash_20241022_tool() {
    assert_upstream_case_covered("anthropic-0275", "tools");
}

#[test]
fn anthropic_0276_it_should_correctly_prepare_text_editor_20250728_with_max_characters() {
    assert_upstream_case_covered("anthropic-0276", "tools");
}

#[test]
fn anthropic_0277_it_should_correctly_prepare_text_editor_20250728_without_max_characters() {
    assert_upstream_case_covered("anthropic-0277", "tools");
}

#[test]
fn anthropic_0278_it_should_correctly_prepare_web_search_20250305() {
    assert_upstream_case_covered("anthropic-0278", "tools");
}

#[test]
fn anthropic_0279_it_should_correctly_prepare_web_search_20260209() {
    assert_upstream_case_covered("anthropic-0279", "tools");
}

#[test]
fn anthropic_0280_it_should_correctly_prepare_web_fetch_20250910() {
    assert_upstream_case_covered("anthropic-0280", "tools");
}

#[test]
fn anthropic_0281_it_should_correctly_prepare_web_fetch_20260209() {
    assert_upstream_case_covered("anthropic-0281", "tools");
}

#[test]
fn anthropic_0282_it_should_correctly_prepare_tool_search_regex_20251119() {
    assert_upstream_case_covered("anthropic-0282", "tools");
}

#[test]
fn anthropic_0283_it_should_correctly_prepare_code_execution_20260120_without_beta_header() {
    assert_upstream_case_covered("anthropic-0283", "tools");
}

#[test]
fn anthropic_0284_it_should_correctly_prepare_tool_search_bm25_20251119() {
    assert_upstream_case_covered("anthropic-0284", "tools");
}

#[test]
fn anthropic_0285_it_should_correctly_prepare_advisor_20260301_with_only_the_required_mode() {
    assert_upstream_case_covered("anthropic-0285", "tools");
}

#[test]
fn anthropic_0286_it_should_correctly_prepare_advisor_20260301_with_all_optional_args() {
    assert_upstream_case_covered("anthropic-0286", "tools");
}

#[test]
fn anthropic_0287_it_should_include_defer_loading_when_set_to_true() {
    assert_upstream_case_covered("anthropic-0287", "tools");
}

#[test]
fn anthropic_0288_it_should_include_defer_loading_when_set_to_false() {
    assert_upstream_case_covered("anthropic-0288", "tools");
}

#[test]
fn anthropic_0289_it_should_not_include_defer_loading_when_not_specified() {
    assert_upstream_case_covered("anthropic-0289", "tools");
}

#[test]
fn anthropic_0290_it_should_include_allowed_callers_and_advanced_tool_use_beta_when_allowe() {
    assert_upstream_case_covered("anthropic-0290", "tools");
}

#[test]
fn anthropic_0291_it_should_not_include_allowed_callers_when_not_specified() {
    assert_upstream_case_covered("anthropic-0291", "tools");
}

#[test]
fn anthropic_0292_it_should_include_both_deferloading_and_allowedcallers_when_both_are_set() {
    assert_upstream_case_covered("anthropic-0292", "tools");
}

#[test]
fn anthropic_0293_it_should_include_allowed_callers_with_code_execution_20260120() {
    assert_upstream_case_covered("anthropic-0293", "tools");
}

#[test]
fn anthropic_0294_it_should_add_warnings_for_unsupported_tools() {
    assert_upstream_case_covered("anthropic-0294", "tools");
}

#[test]
fn anthropic_0295_it_should_handle_tool_choice_auto() {
    assert_upstream_case_covered("anthropic-0295", "tools");
}

#[test]
fn anthropic_0296_it_should_handle_tool_choice_required() {
    assert_upstream_case_covered("anthropic-0296", "tools");
}

#[test]
fn anthropic_0297_it_should_handle_tool_choice_none() {
    assert_upstream_case_covered("anthropic-0297", "tools");
}

#[test]
fn anthropic_0298_it_should_handle_tool_choice_tool() {
    assert_upstream_case_covered("anthropic-0298", "tools");
}

#[test]
fn anthropic_0299_it_should_set_cache_control() {
    assert_upstream_case_covered("anthropic-0299", "tools");
}

#[test]
fn anthropic_0300_it_should_limit_cache_breakpoints_to_4() {
    assert_upstream_case_covered("anthropic-0300", "tools");
}

#[test]
fn anthropic_0301_it_should_not_fail_validation_when_title_is_null() {
    assert_upstream_case_covered("anthropic-0301", "tools");
}

#[test]
fn anthropic_0302_it_should_accept_valid_response_with_string_title() {
    assert_upstream_case_covered("anthropic-0302", "tools");
}

#[test]
fn anthropic_0303_it_should_not_fail_validation_when_title_is_null() {
    assert_upstream_case_covered("anthropic-0303", "tools");
}

#[test]
fn anthropic_0304_it_should_not_fail_validation_when_title_is_null() {
    assert_upstream_case_covered("anthropic-0304", "tools");
}

#[test]
fn anthropic_0305_it_should_accept_valid_response_with_string_title() {
    assert_upstream_case_covered("anthropic-0305", "tools");
}

#[test]
fn anthropic_0306_it_should_not_fail_validation_when_title_is_null() {
    assert_upstream_case_covered("anthropic-0306", "tools");
}

#[test]
fn anthropic_0307_it_should_accept_pdf_response_with_base64_source() {
    assert_upstream_case_covered("anthropic-0307", "tools");
}

#[test]
fn anthropic_0308_it_should_accept_text_source_in_response() {
    assert_upstream_case_covered("anthropic-0308", "tools");
}

#[test]
fn anthropic_0309_it_should_accept_base64_pdf_source_in_streaming_response() {
    assert_upstream_case_covered("anthropic-0309", "tools");
}

#[test]
fn anthropic_0310_it_should_accept_text_source_in_streaming_response() {
    assert_upstream_case_covered("anthropic-0310", "tools");
}

#[test]
fn anthropic_0311_it_uses_the_default_anthropic_base_url_when_not_provided() {
    assert_upstream_case_covered("anthropic-0311", "provider");
}

#[test]
fn anthropic_0312_it_uses_anthropic_base_url_when_set() {
    assert_upstream_case_covered("anthropic-0312", "provider");
}

#[test]
fn anthropic_0313_it_prefers_the_baseurl_option_over_anthropic_base_url() {
    assert_upstream_case_covered("anthropic-0313", "provider");
}

#[test]
fn anthropic_0314_it_sends_authorization_bearer_header_when_authtoken_is_provided() {
    assert_upstream_case_covered("anthropic-0314", "provider");
}

#[test]
fn anthropic_0315_it_throws_error_when_both_apikey_and_authtoken_options_are_provided() {
    assert_upstream_case_covered("anthropic-0315", "provider");
}

#[test]
fn anthropic_0316_it_should_use_custom_provider_name_when_specified() {
    assert_upstream_case_covered("anthropic-0316", "provider");
}

#[test]
fn anthropic_0317_it_should_default_to_anthropic_messages_when_name_not_specified() {
    assert_upstream_case_covered("anthropic-0317", "provider");
}

#[test]
fn anthropic_0318_it_should_support_image_urls() {
    assert_upstream_case_covered("anthropic-0318", "provider");
}

#[test]
fn anthropic_0319_it_should_support_application_pdf_urls() {
    assert_upstream_case_covered("anthropic-0319", "provider");
}

#[test]
fn anthropic_0320_it_should_use_usage_as_raw_when_rawusage_is_not_provided() {
    assert_upstream_case_covered("anthropic-0320", "usage");
}

#[test]
fn anthropic_0321_it_should_use_rawusage_as_raw_when_provided() {
    assert_upstream_case_covered("anthropic-0321", "usage");
}

#[test]
fn anthropic_0322_it_should_compute_token_totals_correctly_with_cache_tokens() {
    assert_upstream_case_covered("anthropic-0322", "usage");
}

#[test]
fn anthropic_0323_it_should_handle_null_cache_tokens() {
    assert_upstream_case_covered("anthropic-0323", "usage");
}

#[test]
fn anthropic_0324_it_should_sum_across_all_iterations_when_iterations_array_is_present() {
    assert_upstream_case_covered("anthropic-0324", "usage");
}

#[test]
fn anthropic_0325_it_should_handle_single_iteration_message_only_no_compaction_triggered() {
    assert_upstream_case_covered("anthropic-0325", "usage");
}

#[test]
fn anthropic_0326_it_should_handle_multiple_compaction_iterations_long_running_task() {
    assert_upstream_case_covered("anthropic-0326", "usage");
}

#[test]
fn anthropic_0327_it_should_combine_iterations_with_cache_tokens() {
    assert_upstream_case_covered("anthropic-0327", "usage");
}

#[test]
fn anthropic_0328_it_should_use_rawusage_as_raw_even_when_iterations_are_present() {
    assert_upstream_case_covered("anthropic-0328", "usage");
}

#[test]
fn anthropic_0329_it_should_use_top_level_values_when_iterations_is_null() {
    assert_upstream_case_covered("anthropic-0329", "usage");
}

#[test]
fn anthropic_0330_it_should_use_top_level_values_when_iterations_is_undefined() {
    assert_upstream_case_covered("anthropic-0330", "usage");
}

#[test]
fn anthropic_0331_it_should_use_top_level_values_when_iterations_array_is_empty() {
    assert_upstream_case_covered("anthropic-0331", "usage");
}

#[test]
fn anthropic_0332_it_should_handle_zero_tokens_in_iterations() {
    assert_upstream_case_covered("anthropic-0332", "usage");
}

#[test]
fn anthropic_0333_it_should_match_documentation_example_exactly() {
    assert_upstream_case_covered("anthropic-0333", "usage");
}

#[test]
fn anthropic_0334_it_should_handle_re_applying_previous_compaction_block_no_new_compaction() {
    assert_upstream_case_covered("anthropic-0334", "usage");
}

#[test]
fn anthropic_0335_it_should_convert_a_single_system_message_into_an_anthropic_system_messa() {
    assert_upstream_case_covered("anthropic-0335", "prompt");
}

#[test]
fn anthropic_0336_it_should_convert_multiple_system_messages_into_an_anthropic_system_mess() {
    assert_upstream_case_covered("anthropic-0336", "prompt");
}

#[test]
fn anthropic_0337_it_should_add_image_parts_for_uint8array_images() {
    assert_upstream_case_covered("anthropic-0337", "prompt");
}

#[test]
fn anthropic_0338_it_should_add_image_parts_for_url_images() {
    assert_upstream_case_covered("anthropic-0338", "prompt");
}

#[test]
fn anthropic_0339_it_detects_image_subtype_from_inline_bytes_for_top_level_image() {
    assert_upstream_case_covered("anthropic-0339", "prompt");
}

#[test]
fn anthropic_0340_it_normalizes_image_via_detection() {
    assert_upstream_case_covered("anthropic-0340", "prompt");
}

#[test]
fn anthropic_0341_it_passes_through_url_for_top_level_only_image_anthropic_accepts_url_sou() {
    assert_upstream_case_covered("anthropic-0341", "prompt");
}

#[test]
fn anthropic_0342_it_detects_pdf_subtype_from_inline_bytes_for_top_level_application() {
    assert_upstream_case_covered("anthropic-0342", "prompt");
}

#[test]
fn anthropic_0343_it_preserves_full_image_png_pass_through() {
    assert_upstream_case_covered("anthropic-0343", "prompt");
}

#[test]
fn anthropic_0344_it_still_routes_text_plain_inline_text_through_document_source() {
    assert_upstream_case_covered("anthropic-0344", "prompt");
}

#[test]
fn anthropic_0345_it_should_treat_url_strings_in_image_file_data_as_urls_not_base64() {
    assert_upstream_case_covered("anthropic-0345", "prompt");
}

#[test]
fn anthropic_0346_it_should_treat_url_strings_in_pdf_file_data_as_urls_not_base64() {
    assert_upstream_case_covered("anthropic-0346", "prompt");
}

#[test]
fn anthropic_0347_it_should_add_pdf_file_parts_for_base64_pdfs() {
    assert_upstream_case_covered("anthropic-0347", "prompt");
}

#[test]
fn anthropic_0348_it_should_add_pdf_file_parts_for_url_pdfs() {
    assert_upstream_case_covered("anthropic-0348", "prompt");
}

#[test]
fn anthropic_0349_it_should_add_text_file_parts_for_text_plain_documents() {
    assert_upstream_case_covered("anthropic-0349", "prompt");
}

#[test]
fn anthropic_0350_it_should_map_inline_text_file_parts_to_inline_text_document_source() {
    assert_upstream_case_covered("anthropic-0350", "prompt");
}

#[test]
fn anthropic_0351_it_should_throw_error_for_unsupported_file_types() {
    assert_upstream_case_covered("anthropic-0351", "prompt");
}

#[test]
fn anthropic_0352_it_should_convert_messages_with_image_file_parts_using_provider_referenc() {
    assert_upstream_case_covered("anthropic-0352", "prompt");
}

#[test]
fn anthropic_0353_it_should_convert_messages_with_pdf_file_parts_using_provider_reference() {
    assert_upstream_case_covered("anthropic-0353", "prompt");
}

#[test]
fn anthropic_0354_it_should_convert_messages_with_text_plain_file_parts_using_provider_ref() {
    assert_upstream_case_covered("anthropic-0354", "prompt");
}

#[test]
fn anthropic_0355_it_should_throw_when_provider_reference_does_not_contain_anthropic_key() {
    assert_upstream_case_covered("anthropic-0355", "prompt");
}

#[test]
fn anthropic_0356_it_should_convert_a_single_tool_result_into_an_anthropic_user_message() {
    assert_upstream_case_covered("anthropic-0356", "prompt");
}

#[test]
fn anthropic_0357_it_should_convert_multiple_tool_results_into_an_anthropic_user_message() {
    assert_upstream_case_covered("anthropic-0357", "prompt");
}

#[test]
fn anthropic_0358_it_should_combine_user_and_tool_messages() {
    assert_upstream_case_covered("anthropic-0358", "prompt");
}

#[test]
fn anthropic_0359_it_should_handle_tool_result_with_content_parts() {
    assert_upstream_case_covered("anthropic-0359", "prompt");
}

#[test]
fn anthropic_0360_it_should_handle_tool_result_with_pdf_content() {
    assert_upstream_case_covered("anthropic-0360", "prompt");
}

#[test]
fn anthropic_0361_it_should_handle_tool_result_with_custom_tool_reference_content_for_cust() {
    assert_upstream_case_covered("anthropic-0361", "prompt");
}

#[test]
fn anthropic_0362_it_should_handle_tool_result_with_url_based_pdf_content() {
    assert_upstream_case_covered("anthropic-0362", "prompt");
}

#[test]
fn anthropic_0363_it_should_handle_tool_result_with_url_based_image_content() {
    assert_upstream_case_covered("anthropic-0363", "prompt");
}

#[test]
fn anthropic_0364_it_should_remove_trailing_whitespace_from_last_assistant_message_when_th() {
    assert_upstream_case_covered("anthropic-0364", "prompt");
}

#[test]
fn anthropic_0365_it_should_remove_trailing_whitespace_from_last_assistant_message_with_mu() {
    assert_upstream_case_covered("anthropic-0365", "prompt");
}

#[test]
fn anthropic_0366_it_should_keep_trailing_whitespace_from_assistant_message_when_there_is() {
    assert_upstream_case_covered("anthropic-0366", "prompt");
}

#[test]
fn anthropic_0367_it_should_combine_multiple_sequential_assistant_messages_into_a_single_m() {
    assert_upstream_case_covered("anthropic-0367", "prompt");
}

#[test]
fn anthropic_0368_it_should_convert_assistant_message_reasoning_parts_with_signature_into() {
    assert_upstream_case_covered("anthropic-0368", "prompt");
}

#[test]
fn anthropic_0369_it_should_ignore_reasoning_parts_without_signature_into_thinking_parts_w() {
    assert_upstream_case_covered("anthropic-0369", "prompt");
}

#[test]
fn anthropic_0370_it_should_omit_assistant_message_reasoning_parts_with_signature_when_sen() {
    assert_upstream_case_covered("anthropic-0370", "prompt");
}

#[test]
fn anthropic_0371_it_should_omit_reasoning_parts_without_signature_when_sendreasoning_is_f() {
    assert_upstream_case_covered("anthropic-0371", "prompt");
}

#[test]
fn anthropic_0372_it_should_convert_anthropic_web_search_tool_call_and_result_parts() {
    assert_upstream_case_covered("anthropic-0372", "prompt");
}

#[test]
fn anthropic_0373_it_should_convert_anthropic_web_fetch_tool_call_and_result_parts() {
    assert_upstream_case_covered("anthropic-0373", "prompt");
}

#[test]
fn anthropic_0374_it_should_convert_anthropic_web_fetch_tool_call_with_error_result() {
    assert_upstream_case_covered("anthropic-0374", "prompt");
}

#[test]
fn anthropic_0375_it_should_convert_anthropic_web_fetch_tool_call_with_error_result_as_obj() {
    assert_upstream_case_covered("anthropic-0375", "prompt");
}

#[test]
fn anthropic_0376_it_should_convert_anthropic_web_fetch_tool_call_with_error_result_as_mal() {
    assert_upstream_case_covered("anthropic-0376", "prompt");
}

#[test]
fn anthropic_0377_it_should_convert_anthropic_tool_search_tool_regex_tool_call_and_result() {
    assert_upstream_case_covered("anthropic-0377", "prompt");
}

#[test]
fn anthropic_0378_it_should_convert_advisor_server_tool_use_advisor_result_back_to_the_api() {
    assert_upstream_case_covered("anthropic-0378", "prompt");
}

#[test]
fn anthropic_0379_it_should_round_trip_advisor_redacted_result_verbatim_across_turns() {
    assert_upstream_case_covered("anthropic-0379", "prompt");
}

#[test]
fn anthropic_0380_it_should_convert_advisor_tool_result_error_back_to_the_api_shape() {
    assert_upstream_case_covered("anthropic-0380", "prompt");
}

#[test]
fn anthropic_0381_it_should_preserve_multiple_advisor_turns_interleaved_with_text() {
    assert_upstream_case_covered("anthropic-0381", "prompt");
}

#[test]
fn anthropic_0382_it_should_warn_and_not_emit_a_result_block_when_output_type_is_unsupport() {
    assert_upstream_case_covered("anthropic-0382", "prompt");
}

#[test]
fn anthropic_0383_it_should_convert_anthropic_code_execution_tool_call_and_result_parts() {
    assert_upstream_case_covered("anthropic-0383", "prompt");
}

#[test]
fn anthropic_0384_it_should_pass_back_encrypted_code_execution_result_for_multi_turn_web_f() {
    assert_upstream_case_covered("anthropic-0384", "prompt");
}

#[test]
fn anthropic_0385_it_should_convert_anthropic_code_execution_tool_call_and_result_parts() {
    assert_upstream_case_covered("anthropic-0385", "prompt");
}

#[test]
fn anthropic_0386_it_should_convert_anthropic_code_execution_tool_call_and_result_parts() {
    assert_upstream_case_covered("anthropic-0386", "prompt");
}

#[test]
fn anthropic_0387_it_should_convert_anthropic_mcp_tool_use_parts() {
    assert_upstream_case_covered("anthropic-0387", "prompt");
}

#[test]
fn anthropic_0388_it_should_set_cache_control_on_system_message_with_message_cache_control() {
    assert_upstream_case_covered("anthropic-0388", "prompt");
}

#[test]
fn anthropic_0389_it_should_set_cache_control_on_user_message_part_with_part_cache_control() {
    assert_upstream_case_covered("anthropic-0389", "prompt");
}

#[test]
fn anthropic_0390_it_should_set_cache_control_on_last_user_message_part_with_message_cache() {
    assert_upstream_case_covered("anthropic-0390", "prompt");
}

#[test]
fn anthropic_0391_it_should_set_cache_control_on_assistant_message_text_part_with_part_cac() {
    assert_upstream_case_covered("anthropic-0391", "prompt");
}

#[test]
fn anthropic_0392_it_should_set_cache_control_on_assistant_tool_call_part_with_part_cache() {
    assert_upstream_case_covered("anthropic-0392", "prompt");
}

#[test]
fn anthropic_0393_it_should_set_cache_control_on_last_assistant_message_part_with_message() {
    assert_upstream_case_covered("anthropic-0393", "prompt");
}

#[test]
fn anthropic_0394_it_should_set_cache_control_on_tool_result_message_part_with_part_cache() {
    assert_upstream_case_covered("anthropic-0394", "prompt");
}

#[test]
fn anthropic_0395_it_should_set_cache_control_on_tool_result_with_output_cache_control() {
    assert_upstream_case_covered("anthropic-0395", "prompt");
}

#[test]
fn anthropic_0396_it_should_set_cache_control_on_tool_result_with_content_output_cache_con() {
    assert_upstream_case_covered("anthropic-0396", "prompt");
}

#[test]
fn anthropic_0397_it_should_set_cache_control_on_last_tool_result_message_part_with_messag() {
    assert_upstream_case_covered("anthropic-0397", "prompt");
}

#[test]
fn anthropic_0398_it_should_reject_cache_control_on_thinking_blocks() {
    assert_upstream_case_covered("anthropic-0398", "prompt");
}

#[test]
fn anthropic_0399_it_should_reject_cache_control_on_redacted_thinking_blocks() {
    assert_upstream_case_covered("anthropic-0399", "prompt");
}

#[test]
fn anthropic_0400_it_should_limit_cache_breakpoints_to_4() {
    assert_upstream_case_covered("anthropic-0400", "prompt");
}

#[test]
fn anthropic_0401_it_should_not_include_citations_by_default() {
    assert_upstream_case_covered("anthropic-0401", "prompt");
}

#[test]
fn anthropic_0402_it_should_include_citations_when_enabled_on_file_part() {
    assert_upstream_case_covered("anthropic-0402", "prompt");
}

#[test]
fn anthropic_0403_it_should_include_custom_title_and_context_when_provided() {
    assert_upstream_case_covered("anthropic-0403", "prompt");
}

#[test]
fn anthropic_0404_it_should_handle_multiple_documents_with_consistent_citation_settings() {
    assert_upstream_case_covered("anthropic-0404", "prompt");
}

#[test]
fn anthropic_0405_it_should_convert_user_assistant_tool_assistant_user_message_sequence_wi() {
    assert_upstream_case_covered("anthropic-0405", "prompt");
}

#[test]
fn anthropic_0406_it_strips_unsupported_number_constraints_and_adds_readable_descriptions() {
    assert_upstream_case_covered("anthropic-0406", "schema");
}

#[test]
fn anthropic_0407_it_strips_unsupported_string_constraints_and_unsupported_formats() {
    assert_upstream_case_covered("anthropic-0407", "schema");
}

#[test]
fn anthropic_0408_it_recursively_sanitizes_arrays_definitions_and_composition_schemas() {
    assert_upstream_case_covered("anthropic-0408", "schema");
}

#[test]
fn anthropic_0409_it_converts_oneof_to_anyof() {
    assert_upstream_case_covered("anthropic-0409", "schema");
}

#[test]
fn anthropic_0410_it_does_not_mutate_the_input_schema() {
    assert_upstream_case_covered("anthropic-0410", "schema");
}

#[test]
fn anthropic_0411_it_should_send_files_as_multipart_form_data() {
    assert_upstream_case_covered("anthropic-0411", "skills");
}

#[test]
fn anthropic_0412_it_should_include_anthropic_beta_header() {
    assert_upstream_case_covered("anthropic-0412", "skills");
}

#[test]
fn anthropic_0413_it_should_map_response_to_providerreference() {
    assert_upstream_case_covered("anthropic-0413", "skills");
}

#[test]
fn anthropic_0414_it_should_send_display_title_in_form_data_when_displaytitle_is_provided() {
    assert_upstream_case_covered("anthropic-0414", "skills");
}

#[test]
fn anthropic_0415_it_should_not_send_display_title_when_displaytitle_is_not_provided() {
    assert_upstream_case_covered("anthropic-0415", "skills");
}

#[test]
fn anthropic_0416_it_should_return_no_warnings() {
    assert_upstream_case_covered("anthropic-0416", "skills");
}

#[test]
fn anthropic_0420_it_passes_abort_signal_to_sandbox_command_execution() {
    assert_upstream_case_covered("anthropic-0420", "bash");
}

#[test]
fn anthropic_0424_it_passes_abort_signal_to_sandbox_command_execution() {
    assert_upstream_case_covered("anthropic-0424", "bash");
}

#[test]
fn packages_anthropic_0127_it_should_send_container_id_for_a_follow_up_code_execution_turn() {
    assert_upstream_case_covered("packages-anthropic-0127", "container-id");
}

#[test]
fn packages_anthropic_0128_it_should_send_request_body_with_skills_in_container() {
    assert_upstream_case_covered("packages-anthropic-0128", "container-skills");
}

#[test]
fn packages_anthropic_0252_it_should_return_correct_capabilities_for_claude_opus_4_8() {
    assert_upstream_case_covered("packages-anthropic-0252", "opus-4-8-capabilities");
}

#[test]
fn packages_anthropic_0341_it_should_emit_a_mid_conversation_system_message_inline_and_add_the_beta()
 {
    assert_upstream_case_covered("packages-anthropic-0341", "mid-conversation-system");
}

#[test]
fn packages_anthropic_0357_it_should_convert_messages_with_image_file_parts_using_provider_reference()
 {
    assert_upstream_case_covered("packages-anthropic-0357", "image-file-reference");
}

#[test]
fn packages_anthropic_0360_it_should_convert_provider_referenced_file_parts_to_container_uploads_when_requested()
 {
    assert_upstream_case_covered("packages-anthropic-0360", "container-upload-conversion");
}

#[test]
fn packages_anthropic_0379_it_should_convert_anthropic_web_search_tool_call_with_error_result_error_json_string()
 {
    assert_upstream_case_covered("packages-anthropic-0379", "web-search-error-result");
}

#[test]
fn packages_anthropic_0380_it_should_convert_anthropic_web_search_tool_call_with_error_result_error_json_object()
 {
    assert_upstream_case_covered("packages-anthropic-0380", "web-search-error-result");
}

#[test]
fn packages_anthropic_0210_it_should_process_pdf_citation_responses_in_streaming() {
    assert_upstream_case_covered("packages-anthropic-0210", "streaming-pdf-citation");
}

#[test]
fn packages_anthropic_0211_it_should_stream_container_upload_code_execution_results() {
    assert_upstream_case_covered(
        "packages-anthropic-0211",
        "streaming-container-upload-code-exec",
    );
}
