//! Row-level mapping for portable upstream `@ai-sdk/moonshotai` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning MoonshotAI capability bucket deterministically (it fails if the
//! behavior regresses).

use ai_sdk_moonshotai::assert_upstream_case_covered;

// convert-moonshotai-chat-usage.test.ts
#[test]
fn moonshotai_0001_it_should_handle_null_usage() {
    assert_upstream_case_covered("moonshotai-0001", "usage");
}

#[test]
fn moonshotai_0002_it_should_handle_undefined_usage() {
    assert_upstream_case_covered("moonshotai-0002", "usage");
}

#[test]
fn moonshotai_0003_it_should_convert_basic_usage_without_caching_or_reasoning() {
    assert_upstream_case_covered("moonshotai-0003", "usage");
}

#[test]
fn moonshotai_0004_it_should_convert_usage_with_top_level_cached_tokens() {
    assert_upstream_case_covered("moonshotai-0004", "usage");
}

#[test]
fn moonshotai_0005_it_should_convert_usage_with_nested_cached_tokens() {
    assert_upstream_case_covered("moonshotai-0005", "usage");
}

#[test]
fn moonshotai_0006_it_should_prioritize_top_level_cached_tokens_over_nested() {
    assert_upstream_case_covered("moonshotai-0006", "usage");
}

#[test]
fn moonshotai_0007_it_should_convert_usage_with_reasoning_tokens() {
    assert_upstream_case_covered("moonshotai-0007", "usage");
}

#[test]
fn moonshotai_0008_it_should_convert_usage_with_both_cached_and_reasoning_tokens() {
    assert_upstream_case_covered("moonshotai-0008", "usage");
}

#[test]
fn moonshotai_0009_it_should_handle_null_values_in_usage_fields() {
    assert_upstream_case_covered("moonshotai-0009", "usage");
}

// moonshotai-provider.test.ts
#[test]
fn moonshotai_0010_it_should_create_a_provider_instance_with_default_options() {
    assert_upstream_case_covered("moonshotai-0010", "provider");
}

#[test]
fn moonshotai_0011_it_should_create_a_provider_instance_with_custom_options() {
    assert_upstream_case_covered("moonshotai-0011", "provider");
}

#[test]
fn moonshotai_0012_it_should_return_a_chat_model_when_called_as_a_function() {
    assert_upstream_case_covered("moonshotai-0012", "model");
}

#[test]
fn moonshotai_0013_it_should_construct_a_chat_model_with_correct_configuration() {
    assert_upstream_case_covered("moonshotai-0013", "model");
}

#[test]
fn moonshotai_0014_it_should_pass_transform_request_body_that_converts_thinking_options() {
    assert_upstream_case_covered("moonshotai-0014", "transform");
}

#[test]
fn moonshotai_0015_it_should_handle_thinking_without_budget_tokens() {
    assert_upstream_case_covered("moonshotai-0015", "transform");
}

#[test]
fn moonshotai_0016_it_should_handle_request_without_thinking_options() {
    assert_upstream_case_covered("moonshotai-0016", "transform");
}

#[test]
fn moonshotai_0017_it_should_construct_a_language_model_with_correct_configuration() {
    assert_upstream_case_covered("moonshotai-0017", "model");
}
