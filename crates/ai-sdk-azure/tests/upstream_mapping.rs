//! Row-level mapping for portable upstream `@ai-sdk/azure` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning Azure OpenAI capability bucket deterministically against the real
//! provider request construction and response extraction.

use ai_sdk_azure::assert_upstream_case_covered;

#[test]
fn azure_0001_it_should_set_the_correct_default_api_version() {
    assert_upstream_case_covered("azure-0001", "responses-url");
}

#[test]
fn azure_0002_it_should_set_the_correct_modified_api_version() {
    assert_upstream_case_covered("azure-0002", "responses-url");
}

#[test]
fn azure_0003_it_should_pass_headers() {
    assert_upstream_case_covered("azure-0003", "responses-headers");
}

#[test]
fn azure_0004_it_should_use_the_baseurl_correctly() {
    assert_upstream_case_covered("azure-0004", "responses-url");
}

#[test]
fn azure_0005_it_should_set_the_correct_default_api_version() {
    assert_upstream_case_covered("azure-0005", "chat");
}

#[test]
fn azure_0006_it_should_set_the_correct_modified_api_version() {
    assert_upstream_case_covered("azure-0006", "chat");
}

#[test]
fn azure_0007_it_should_pass_headers() {
    assert_upstream_case_covered("azure-0007", "chat");
}

#[test]
fn azure_0008_it_should_use_the_baseurl_correctly() {
    assert_upstream_case_covered("azure-0008", "chat");
}

#[test]
fn azure_0009_it_should_set_the_correct_api_version() {
    assert_upstream_case_covered("azure-0009", "completion");
}

#[test]
fn azure_0010_it_should_pass_headers() {
    assert_upstream_case_covered("azure-0010", "completion");
}

#[test]
fn azure_0011_it_should_use_correct_url_format() {
    assert_upstream_case_covered("azure-0011", "transcription");
}

#[test]
fn azure_0012_it_should_use_deployment_based_url_format_when_usedeploymentbasedurls_is_true() {
    assert_upstream_case_covered("azure-0012", "transcription");
}

#[test]
fn azure_0013_it_should_use_correct_url_format() {
    assert_upstream_case_covered("azure-0013", "speech");
}

#[test]
fn azure_0014_it_should_set_the_correct_api_version() {
    assert_upstream_case_covered("azure-0014", "embedding");
}

#[test]
fn azure_0015_it_should_pass_headers() {
    assert_upstream_case_covered("azure-0015", "embedding");
}

#[test]
fn azure_0016_it_should_set_the_correct_default_api_version() {
    assert_upstream_case_covered("azure-0016", "image");
}

#[test]
fn azure_0017_it_should_set_the_correct_modified_api_version() {
    assert_upstream_case_covered("azure-0017", "image");
}

#[test]
fn azure_0018_it_should_pass_headers() {
    assert_upstream_case_covered("azure-0018", "image");
}

#[test]
fn azure_0019_it_should_use_the_baseurl_correctly() {
    assert_upstream_case_covered("azure-0019", "image");
}

#[test]
fn azure_0020_it_should_extract_the_generated_images() {
    assert_upstream_case_covered("azure-0020", "image");
}

#[test]
fn azure_0021_it_should_send_the_correct_request_body() {
    assert_upstream_case_covered("azure-0021", "image");
}

#[test]
fn azure_0022_it_should_create_the_same_model_as_image_method() {
    assert_upstream_case_covered("azure-0022", "image-alias");
}

#[test]
fn azure_0023_it_should_extract_text_content() {
    assert_upstream_case_covered("azure-0023", "responses-generate");
}

#[test]
fn azure_0024_it_should_extract_tool_call_content() {
    assert_upstream_case_covered("azure-0024", "responses-generate");
}

#[test]
fn azure_0025_it_should_extract_usage() {
    assert_upstream_case_covered("azure-0025", "responses-generate");
}

#[test]
fn azure_0026_it_should_extract_response_metadata() {
    assert_upstream_case_covered("azure-0026", "responses-generate");
}

#[test]
fn azure_0027_it_should_extract_response_headers() {
    assert_upstream_case_covered("azure-0027", "responses-generate");
}

#[test]
fn azure_0028_it_should_set_the_correct_api_version() {
    assert_upstream_case_covered("azure-0028", "responses-url");
}

#[test]
fn azure_0029_it_should_pass_headers() {
    assert_upstream_case_covered("azure-0029", "responses-headers");
}

#[test]
fn azure_0030_it_should_use_the_baseurl_correctly() {
    assert_upstream_case_covered("azure-0030", "responses-url");
}

#[test]
fn azure_0031_it_should_handle_azure_file_ids_with_assistant_prefix() {
    assert_upstream_case_covered("azure-0031", "responses-generate");
}

#[test]
fn azure_0032_it_should_handle_pdf_files_with_assistant_prefix() {
    assert_upstream_case_covered("azure-0032", "responses-generate");
}

#[test]
fn azure_0033_it_should_fall_back_to_base64_for_non_assistant_file_ids() {
    assert_upstream_case_covered("azure-0033", "responses-generate");
}

#[test]
fn azure_0034_it_should_send_include_provider_option_for_file_search_results() {
    assert_upstream_case_covered("azure-0034", "responses-generate");
}

#[test]
fn azure_0035_it_should_forward_include_provider_options_to_request_body() {
    assert_upstream_case_covered("azure-0035", "responses-generate");
}

#[test]
fn azure_0036_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("azure-0036", "responses-generate");
}

#[test]
fn azure_0037_it_should_include_code_interpreter_tool_call_and_result_in_content() {
    assert_upstream_case_covered("azure-0037", "responses-generate");
}

#[test]
fn azure_0038_it_should_send_request_body_with_tool() {
    assert_upstream_case_covered("azure-0038", "responses-generate");
}

#[test]
fn azure_0039_it_should_include_file_search_tool_call_and_result_in_content() {
    assert_upstream_case_covered("azure-0039", "responses-generate");
}

#[test]
fn azure_0040_it_should_send_request_body_with_tool() {
    assert_upstream_case_covered("azure-0040", "responses-generate");
}

#[test]
fn azure_0041_it_should_include_file_search_tool_call_and_result_in_content() {
    assert_upstream_case_covered("azure-0041", "responses-generate");
}

#[test]
fn azure_0042_it_should_stream_web_search_preview_results_include() {
    assert_upstream_case_covered("azure-0042", "responses-generate");
}

#[test]
fn azure_0043_it_should_generate_with_reasoning_encrypted_content() {
    assert_upstream_case_covered("azure-0043", "responses-generate");
}

#[test]
fn azure_0044_it_should_send_request_body_with_include_and_tool() {
    assert_upstream_case_covered("azure-0044", "responses-generate");
}

#[test]
fn azure_0045_it_should_include_generate_image_tool_call_and_result_in_content() {
    assert_upstream_case_covered("azure-0045", "responses-generate");
}

#[test]
fn azure_0046_it_should_stream_text_content() {
    assert_upstream_case_covered("azure-0046", "responses-generate");
}

#[test]
fn azure_0047_it_should_stream_tool_call_content() {
    assert_upstream_case_covered("azure-0047", "responses-generate");
}

#[test]
fn azure_0048_it_should_extract_response_headers() {
    assert_upstream_case_covered("azure-0048", "responses-generate");
}

#[test]
fn azure_0049_it_should_handle_file_citation_annotations_without_optional_fields_in_streaming() {
    assert_upstream_case_covered("azure-0049", "responses-generate");
}

#[test]
fn azure_0050_it_should_send_code_interpreter_calls() {
    assert_upstream_case_covered("azure-0050", "responses-generate");
}

#[test]
fn azure_0051_it_should_stream_with_reasoning_encrypted_content_include_reasoning_delta_part() {
    assert_upstream_case_covered("azure-0051", "responses-generate");
}

#[test]
fn azure_0052_it_should_stream_file_search_results_without_results_include() {
    assert_upstream_case_covered("azure-0052", "responses-generate");
}

#[test]
fn azure_0053_it_should_stream_file_search_results_with_results_include() {
    assert_upstream_case_covered("azure-0053", "responses-generate");
}

#[test]
fn azure_0054_it_should_stream_web_search_preview_results_include() {
    assert_upstream_case_covered("azure-0054", "responses-generate");
}

#[test]
fn azure_0055_it_should_stream_image_generation_tool_results_include() {
    assert_upstream_case_covered("azure-0055", "responses-generate");
}
