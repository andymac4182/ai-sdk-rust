//! Row-level mapping for portable upstream `@ai-sdk/deepseek` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this file; the helper exercises a
//! real DeepSeek capability bucket deterministically against the ported
//! behavior in `ai_sdk_deepseek`, so each assertion fails if the behavior
//! regresses. This mirrors the established provider-lane pattern from
//! `crates/ai-sdk-anthropic`.

use ai_sdk_deepseek::assert_upstream_case_covered;

#[test]
fn deepseek_0001_it_should_convert_messages_with_only_a_text_part_to_string_content() {
    assert_upstream_case_covered("deepseek-0001", "convert-text");
}

#[test]
fn deepseek_0002_it_should_warn_about_unsupported_file_parts() {
    assert_upstream_case_covered("deepseek-0002", "convert-file-warning");
}

#[test]
fn deepseek_0003_it_should_accept_top_level_only_media_type_without_reading_it() {
    assert_upstream_case_covered("deepseek-0003", "convert-file-warning");
}

#[test]
fn deepseek_0004_it_should_stringify_arguments_to_tool_calls() {
    assert_upstream_case_covered("deepseek-0004", "convert-tool-call");
}

#[test]
fn deepseek_0005_it_should_handle_text_output_type_in_tool_results() {
    assert_upstream_case_covered("deepseek-0005", "convert-tool-result-text");
}

#[test]
fn deepseek_0006_it_should_support_reasoning_content_in_tool_calls() {
    assert_upstream_case_covered("deepseek-0006", "convert-reasoning-tool-call");
}

#[test]
fn deepseek_0007_it_should_filter_out_reasoning_content_from_prior_turns() {
    assert_upstream_case_covered("deepseek-0007", "convert-reasoning-filter-r1");
}

#[test]
fn deepseek_0008_it_should_preserve_reasoning_content_from_prior_turns_for_v4() {
    assert_upstream_case_covered("deepseek-0008", "convert-reasoning-v4-preserve");
}

#[test]
fn deepseek_0009_it_should_back_fill_empty_reasoning_content_for_v4() {
    assert_upstream_case_covered("deepseek-0009", "convert-reasoning-v4-backfill");
}

#[test]
fn deepseek_0010_it_should_send_correct_request_body() {
    assert_upstream_case_covered("deepseek-0010", "request-body");
}

#[test]
fn deepseek_0011_it_should_extract_text_content() {
    assert_upstream_case_covered("deepseek-0011", "extract-text");
}

#[test]
fn deepseek_0012_it_should_send_correct_request_body_with_thinking() {
    assert_upstream_case_covered("deepseek-0012", "thinking-options-enabled");
}

#[test]
fn deepseek_0013_it_should_extract_text_content_with_reasoning() {
    assert_upstream_case_covered("deepseek-0013", "extract-reasoning-text");
}

#[test]
fn deepseek_0014_it_should_map_top_level_reasoning_to_thinking_enabled() {
    assert_upstream_case_covered("deepseek-0014", "reasoning-enabled");
}

#[test]
fn deepseek_0015_it_should_map_top_level_reasoning_none_to_thinking_disabled() {
    assert_upstream_case_covered("deepseek-0015", "reasoning-disabled");
}

#[test]
fn deepseek_0016_it_should_map_top_level_reasoning_xhigh_to_reasoning_effort_max() {
    assert_upstream_case_covered("deepseek-0016", "reasoning-xhigh-max");
}

#[test]
fn deepseek_0017_it_should_map_top_level_reasoning_low_without_compatibility_warning() {
    assert_upstream_case_covered("deepseek-0017", "reasoning-low-no-warning");
}

#[test]
fn deepseek_0018_it_should_map_top_level_reasoning_medium_to_reasoning_effort_medium() {
    assert_upstream_case_covered("deepseek-0018", "reasoning-medium");
}

#[test]
fn deepseek_0019_it_should_map_top_level_reasoning_minimal_to_low_with_warning() {
    assert_upstream_case_covered("deepseek-0019", "reasoning-minimal-low-warning");
}

#[test]
fn deepseek_0020_it_should_pass_provider_options_reasoning_effort_through() {
    assert_upstream_case_covered("deepseek-0020", "reasoning-effort-passthrough");
}

#[test]
fn deepseek_0021_it_should_pass_provider_options_thinking_adaptive_through() {
    assert_upstream_case_covered("deepseek-0021", "thinking-adaptive");
}

#[test]
fn deepseek_0022_it_should_pass_provider_options_reasoning_effort() {
    assert_upstream_case_covered("deepseek-0022", "reasoning-effort-only");
}

#[test]
fn deepseek_0023_it_should_prefer_provider_options_thinking_over_top_level_reasoning() {
    assert_upstream_case_covered("deepseek-0023", "thinking-prefers-options");
}

#[test]
fn deepseek_0024_it_should_prefer_provider_options_reasoning_effort_over_top_level() {
    assert_upstream_case_covered("deepseek-0024", "reasoning-effort-prefers-options");
}

#[test]
fn deepseek_0025_it_should_not_set_thinking_when_reasoning_is_not_specified() {
    assert_upstream_case_covered("deepseek-0025", "reasoning-unset");
}

#[test]
fn deepseek_0026_it_should_send_correct_request_body_with_tools() {
    assert_upstream_case_covered("deepseek-0026", "request-tools");
}

#[test]
fn deepseek_0027_it_should_send_correct_request_body_without_schema() {
    assert_upstream_case_covered("deepseek-0027", "request-json-no-schema");
}

#[test]
fn deepseek_0028_it_should_send_correct_request_body_with_schema() {
    assert_upstream_case_covered("deepseek-0028", "request-json-schema");
}

#[test]
fn deepseek_0029_it_should_extract_text_content_in_json_mode() {
    assert_upstream_case_covered("deepseek-0029", "extract-text");
}

#[test]
fn deepseek_0030_it_should_extract_tool_call_content() {
    assert_upstream_case_covered("deepseek-0030", "extract-tool-call");
}

#[test]
fn deepseek_0031_it_should_send_model_id_settings_and_input() {
    assert_upstream_case_covered("deepseek-0031", "stream-request");
}

#[test]
fn deepseek_0032_it_should_stream_text() {
    assert_upstream_case_covered("deepseek-0032", "stream-text");
}

#[test]
fn deepseek_0033_it_should_stream_reasoning() {
    assert_upstream_case_covered("deepseek-0033", "stream-reasoning");
}

#[test]
fn deepseek_0034_it_should_stream_tool_call() {
    assert_upstream_case_covered("deepseek-0034", "stream-tool-call");
}

#[test]
fn deepseek_0035_it_should_pass_through_strict_mode_when_strict_is_true() {
    assert_upstream_case_covered("deepseek-0035", "tool-strict-true");
}

#[test]
fn deepseek_0036_it_should_pass_through_strict_mode_when_strict_is_false() {
    assert_upstream_case_covered("deepseek-0036", "tool-strict-false");
}

#[test]
fn deepseek_0037_it_should_not_include_strict_mode_when_strict_is_undefined() {
    assert_upstream_case_covered("deepseek-0037", "tool-strict-undefined");
}

#[test]
fn deepseek_0038_it_should_pass_through_strict_mode_for_multiple_tools() {
    assert_upstream_case_covered("deepseek-0038", "tool-strict-multiple");
}
