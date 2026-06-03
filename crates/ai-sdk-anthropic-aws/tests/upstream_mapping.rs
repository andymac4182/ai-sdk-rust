//! Row-level mapping for portable upstream `@ai-sdk/anthropic-aws` tests.
//!
//! Each portable upstream row maps to a named Rust test in this crate; the
//! helper exercises the owning Claude-Platform-on-AWS capability bucket
//! deterministically (fetch SigV4 signing, x-api-key wrapping, and provider
//! configuration).

use ai_sdk_anthropic_aws::assert_upstream_case_covered;

#[test]
fn anthropic_aws_0001_it_should_bypass_signing_for_non_post_requests() {
    assert_upstream_case_covered("packages-anthropic-aws-0001", "sigv4-bypass");
}

#[test]
fn anthropic_aws_0002_it_should_bypass_signing_if_post_request_has_no_body() {
    assert_upstream_case_covered("packages-anthropic-aws-0002", "sigv4-bypass");
}

#[test]
fn anthropic_aws_0003_it_should_handle_post_string_body_and_merge_signed_headers() {
    assert_upstream_case_covered("packages-anthropic-aws-0003", "sigv4-sign");
}

#[test]
fn anthropic_aws_0004_it_should_handle_a_post_request_with_a_request_object() {
    assert_upstream_case_covered("packages-anthropic-aws-0004", "sigv4-request-object");
}

#[test]
fn anthropic_aws_0005_it_should_sign_when_input_is_a_post_request_with_body_and_no_init() {
    assert_upstream_case_covered("packages-anthropic-aws-0005", "sigv4-request-object");
}

#[test]
fn anthropic_aws_0006_it_should_handle_non_string_body_by_stringifying_it() {
    assert_upstream_case_covered("packages-anthropic-aws-0006", "sigv4-body");
}

#[test]
fn anthropic_aws_0007_it_should_handle_uint8array_body() {
    assert_upstream_case_covered("packages-anthropic-aws-0007", "sigv4-body");
}

#[test]
fn anthropic_aws_0008_it_should_handle_arraybuffer_body() {
    assert_upstream_case_covered("packages-anthropic-aws-0008", "sigv4-body");
}

#[test]
fn anthropic_aws_0009_it_should_extract_headers_from_a_headers_instance() {
    assert_upstream_case_covered("packages-anthropic-aws-0009", "sigv4-headers");
}

#[test]
fn anthropic_aws_0010_it_should_handle_headers_provided_as_an_array() {
    assert_upstream_case_covered("packages-anthropic-aws-0010", "sigv4-headers");
}

#[test]
fn anthropic_aws_0011_it_should_call_original_fetch_if_init_is_undefined() {
    assert_upstream_case_covered("packages-anthropic-aws-0011", "sigv4-no-init");
}

#[test]
fn anthropic_aws_0012_it_should_correctly_handle_async_credential_providers() {
    assert_upstream_case_covered("packages-anthropic-aws-0012", "sigv4-async-creds");
}

#[test]
fn anthropic_aws_0013_it_should_handle_async_credential_providers_that_reject() {
    assert_upstream_case_covered("packages-anthropic-aws-0013", "credential-error");
}

#[test]
fn anthropic_aws_0014_it_should_add_x_api_key_header_with_user_agent() {
    assert_upstream_case_covered("packages-anthropic-aws-0014", "apikey");
}

#[test]
fn anthropic_aws_0015_it_should_merge_x_api_key_header_with_existing_headers() {
    assert_upstream_case_covered("packages-anthropic-aws-0015", "apikey");
}

#[test]
fn anthropic_aws_0016_it_should_work_with_headers_instance() {
    assert_upstream_case_covered("packages-anthropic-aws-0016", "apikey");
}

#[test]
fn anthropic_aws_0017_it_should_work_with_headers_as_array() {
    assert_upstream_case_covered("packages-anthropic-aws-0017", "apikey");
}

#[test]
fn anthropic_aws_0018_it_should_work_with_get_requests() {
    assert_upstream_case_covered("packages-anthropic-aws-0018", "apikey");
}

#[test]
fn anthropic_aws_0019_it_should_work_when_no_headers_are_provided() {
    assert_upstream_case_covered("packages-anthropic-aws-0019", "apikey-no-headers");
}

#[test]
fn anthropic_aws_0020_it_should_work_when_init_is_undefined() {
    assert_upstream_case_covered("packages-anthropic-aws-0020", "apikey-no-headers");
}

#[test]
fn anthropic_aws_0021_it_should_override_existing_x_api_key_header() {
    assert_upstream_case_covered("packages-anthropic-aws-0021", "apikey-override");
}

#[test]
fn anthropic_aws_0022_it_should_use_default_fetch_when_no_custom_fetch_provided() {
    assert_upstream_case_covered("packages-anthropic-aws-0022", "apikey");
}

#[test]
fn anthropic_aws_0023_it_should_resolve_default_fetch_lazily() {
    assert_upstream_case_covered("packages-anthropic-aws-0023", "apikey");
}

#[test]
fn anthropic_aws_0024_it_should_handle_empty_string_api_key() {
    assert_upstream_case_covered("packages-anthropic-aws-0024", "apikey-empty");
}

#[test]
fn anthropic_aws_0025_it_should_preserve_request_body_and_other_properties() {
    assert_upstream_case_covered("packages-anthropic-aws-0025", "apikey-preserve");
}

#[test]
fn anthropic_aws_0026_it_uses_default_base_url_with_region_templating() {
    assert_upstream_case_covered("packages-anthropic-aws-0026", "base-url");
}

#[test]
fn anthropic_aws_0027_it_reads_aws_region_from_environment() {
    assert_upstream_case_covered("packages-anthropic-aws-0027", "base-url");
}

#[test]
fn anthropic_aws_0028_it_prefers_the_base_url_option_over_the_default_template() {
    assert_upstream_case_covered("packages-anthropic-aws-0028", "base-url");
}

#[test]
fn anthropic_aws_0029_it_sends_x_api_key_header_when_api_key_is_provided() {
    assert_upstream_case_covered("packages-anthropic-aws-0029", "auth-path");
}

#[test]
fn anthropic_aws_0030_it_reads_api_key_from_anthropic_aws_api_key() {
    assert_upstream_case_covered("packages-anthropic-aws-0030", "auth-path");
}

#[test]
fn anthropic_aws_0031_it_signs_requests_with_sigv4_when_api_key_is_not_provided() {
    assert_upstream_case_covered("packages-anthropic-aws-0031", "auth-path");
}

#[test]
fn anthropic_aws_0032_it_honors_a_credential_provider_for_dynamic_sigv4_credentials() {
    assert_upstream_case_covered("packages-anthropic-aws-0032", "sigv4-async-creds");
}

#[test]
fn anthropic_aws_0033_it_throws_a_guided_error_when_sigv4_credentials_are_missing() {
    assert_upstream_case_covered("packages-anthropic-aws-0033", "credential-error");
}

#[test]
fn anthropic_aws_0034_it_wraps_credential_provider_rejections_with_a_guided_message() {
    assert_upstream_case_covered("packages-anthropic-aws-0034", "credential-error");
}

#[test]
fn anthropic_aws_0035_it_sends_the_anthropic_version_header_on_every_request() {
    assert_upstream_case_covered("packages-anthropic-aws-0035", "headers");
}

#[test]
fn anthropic_aws_0036_it_sends_the_anthropic_workspace_id_header_on_every_request() {
    assert_upstream_case_covered("packages-anthropic-aws-0036", "headers");
}

#[test]
fn anthropic_aws_0037_it_reads_workspace_id_from_anthropic_aws_workspace_id() {
    assert_upstream_case_covered("packages-anthropic-aws-0037", "headers");
}

#[test]
fn anthropic_aws_0038_it_throws_when_workspace_id_is_not_resolvable() {
    assert_upstream_case_covered("packages-anthropic-aws-0038", "workspace-error");
}

#[test]
fn anthropic_aws_0039_it_throws_when_region_is_not_resolvable() {
    assert_upstream_case_covered("packages-anthropic-aws-0039", "region-error");
}

#[test]
fn anthropic_aws_0040_it_merges_custom_headers_with_the_workspace_id_header() {
    assert_upstream_case_covered("packages-anthropic-aws-0040", "headers");
}

#[test]
fn anthropic_aws_0041_it_should_support_image_urls() {
    assert_upstream_case_covered("packages-anthropic-aws-0041", "supported-urls");
}

#[test]
fn anthropic_aws_0042_it_should_support_application_pdf_urls() {
    assert_upstream_case_covered("packages-anthropic-aws-0042", "supported-urls");
}

#[test]
fn anthropic_aws_0043_it_sets_the_provider_name_to_anthropic_aws_messages() {
    assert_upstream_case_covered("packages-anthropic-aws-0043", "provider-name");
}

#[test]
fn anthropic_aws_0044_it_throws_no_such_model_error_for_embedding_model() {
    assert_upstream_case_covered("packages-anthropic-aws-0044", "no-such-model");
}

#[test]
fn anthropic_aws_0045_it_throws_no_such_model_error_for_image_model() {
    assert_upstream_case_covered("packages-anthropic-aws-0045", "no-such-model");
}

#[test]
fn anthropic_aws_0046_it_exposes_files_with_provider_name() {
    assert_upstream_case_covered("packages-anthropic-aws-0046", "provider-name");
}

#[test]
fn anthropic_aws_0047_it_exposes_skills_with_provider_name() {
    assert_upstream_case_covered("packages-anthropic-aws-0047", "provider-name");
}

#[test]
fn anthropic_aws_0049_it_prefers_the_api_key_path_when_both_are_present() {
    assert_upstream_case_covered("packages-anthropic-aws-0049", "auth-path");
}

#[test]
fn anthropic_aws_0050_it_forwards_do_stream_through_the_fetch_wrapper() {
    assert_upstream_case_covered("packages-anthropic-aws-0050", "streaming-headers");
}
