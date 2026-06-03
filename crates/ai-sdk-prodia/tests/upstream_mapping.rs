//! Row-level mapping for portable upstream `@ai-sdk/prodia` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning Prodia capability bucket deterministically (real request-body
//! builders, provider metadata, provider/model construction, header merging,
//! endpoint shaping, multipart response parsing, and error surfacing).
//!
//! The Prodia Rust port is a media-generation provider slice: the upstream
//! `prodia-language-model.test.ts` cases (packages-prodia-0020..0036) and the
//! provider `.languageModel` accessor case (packages-prodia-0038) exercise the
//! LLM surface, which is intentionally not ported here (the provider returns
//! `NoSuchModelError`). Those rows remain documented exceptions in the mapping
//! doc rather than mapped to unrelated tests.

use ai_sdk_prodia::assert_upstream_case_covered;

// --- prodia-image-model.test.ts (packages-prodia-0001..0019) ---

#[test]
fn prodia_0001_it_passes_the_correct_parameters_including_provider_options() {
    assert_upstream_case_covered("prodia-0001", "image_request_basic");
}

#[test]
fn prodia_0002_it_includes_width_and_height_when_size_is_provided() {
    assert_upstream_case_covered("prodia-0002", "image_size");
}

#[test]
fn prodia_0003_it_provider_options_width_height_take_precedence_over_size() {
    assert_upstream_case_covered("prodia-0003", "image_size_override");
}

#[test]
fn prodia_0004_it_includes_style_preset_when_style_preset_is_provided() {
    assert_upstream_case_covered("prodia-0004", "image_style_preset");
}

#[test]
fn prodia_0005_it_includes_loras_when_provided() {
    assert_upstream_case_covered("prodia-0005", "image_loras");
}

#[test]
fn prodia_0006_it_includes_progressive_when_provided() {
    assert_upstream_case_covered("prodia-0006", "image_progressive");
}

#[test]
fn prodia_0007_it_calls_the_correct_endpoint() {
    assert_upstream_case_covered("prodia-0007", "image_endpoint");
}

#[test]
fn prodia_0008_it_sends_accept_multipart_form_data_header() {
    assert_upstream_case_covered("prodia-0008", "image_accept");
}

#[test]
fn prodia_0009_it_merges_provider_and_request_headers() {
    assert_upstream_case_covered("prodia-0009", "image_headers_merge");
}

#[test]
fn prodia_0010_it_returns_image_bytes_from_multipart_response() {
    assert_upstream_case_covered("prodia-0010", "image_returns_bytes");
}

#[test]
fn prodia_0011_it_returns_provider_metadata_from_job_result() {
    assert_upstream_case_covered("prodia-0011", "metadata_full");
}

#[test]
fn prodia_0012_it_omits_optional_metadata_fields_when_not_present_in_job_result() {
    assert_upstream_case_covered("prodia-0012", "metadata_minimal");
}

#[test]
fn prodia_0013_it_warns_on_invalid_size_format() {
    assert_upstream_case_covered("prodia-0013", "image_invalid_size_warns");
}

#[test]
fn prodia_0014_it_handles_api_errors() {
    assert_upstream_case_covered("prodia-0014", "image_api_error");
}

#[test]
fn prodia_0015_it_includes_dollars_in_metadata_when_price_is_present() {
    assert_upstream_case_covered("prodia-0015", "metadata_dollars_present");
}

#[test]
fn prodia_0016_it_omits_dollars_from_metadata_when_price_is_absent() {
    assert_upstream_case_covered("prodia-0016", "metadata_dollars_absent");
}

#[test]
fn prodia_0017_it_omits_dollars_from_metadata_when_price_is_null() {
    assert_upstream_case_covered("prodia-0017", "metadata_dollars_null");
}

#[test]
fn prodia_0018_it_includes_timestamp_headers_and_model_id_in_response_metadata() {
    assert_upstream_case_covered("prodia-0018", "image_response_metadata");
}

#[test]
fn prodia_0019_it_exposes_correct_provider_and_model_information() {
    assert_upstream_case_covered("prodia-0019", "image_identity");
}

// --- prodia-provider.test.ts (packages-prodia-0037..0041) ---

#[test]
fn prodia_0037_it_creates_image_models_via_image_and_image_model() {
    assert_upstream_case_covered("prodia-0037", "provider_image");
}

#[test]
fn prodia_0038_it_creates_language_models_via_language_model() {
    assert_upstream_case_covered("prodia-0038", "language_provider_create");
}

#[test]
fn prodia_0039_it_creates_video_models_via_video_and_video_model() {
    assert_upstream_case_covered("prodia-0039", "provider_video");
}

#[test]
fn prodia_0040_it_configures_base_url_and_headers_correctly() {
    assert_upstream_case_covered("prodia-0040", "provider_config");
}

#[test]
fn prodia_0041_it_throws_no_such_model_error_for_unsupported_model_types() {
    assert_upstream_case_covered("prodia-0041", "provider_no_such_model");
}

// --- prodia-video-model.test.ts (packages-prodia-0042..0054) ---

#[test]
fn prodia_0042_it_exposes_correct_provider_and_model_information() {
    assert_upstream_case_covered("prodia-0042", "video_identity");
}

#[test]
fn prodia_0043_it_sends_correct_json_request_body_with_prompt() {
    assert_upstream_case_covered("prodia-0043", "video_request_basic");
}

#[test]
fn prodia_0044_it_includes_seed_when_provided() {
    assert_upstream_case_covered("prodia-0044", "video_seed");
}

#[test]
fn prodia_0045_it_includes_resolution_from_provider_options() {
    assert_upstream_case_covered("prodia-0045", "video_resolution");
}

#[test]
fn prodia_0046_it_calls_the_correct_endpoint() {
    assert_upstream_case_covered("prodia-0046", "video_endpoint");
}

#[test]
fn prodia_0047_it_sends_correct_accept_header() {
    assert_upstream_case_covered("prodia-0047", "video_accept");
}

#[test]
fn prodia_0048_it_sends_content_type_application_json_for_txt2vid() {
    assert_upstream_case_covered("prodia-0048", "video_content_type_json");
}

#[test]
fn prodia_0049_it_merges_provider_and_request_headers() {
    assert_upstream_case_covered("prodia-0049", "video_headers_merge");
}

#[test]
fn prodia_0050_it_returns_video_data_from_multipart_response() {
    assert_upstream_case_covered("prodia-0050", "video_returns_data");
}

#[test]
fn prodia_0051_it_returns_provider_metadata() {
    assert_upstream_case_covered("prodia-0051", "video_metadata");
}

#[test]
fn prodia_0052_it_includes_timestamp_and_model_id_in_response() {
    assert_upstream_case_covered("prodia-0052", "video_response_metadata");
}

#[test]
fn prodia_0053_it_handles_api_errors() {
    assert_upstream_case_covered("prodia-0053", "video_api_error");
}

#[test]
fn prodia_0054_it_sends_multipart_form_data_when_image_is_provided() {
    assert_upstream_case_covered("prodia-0054", "video_img2vid_multipart");
}

// --- prodia-language-model.test.ts (packages-prodia-0020..0036) ---

#[test]
fn prodia_0020_it_exposes_correct_provider_and_model_information() {
    assert_upstream_case_covered("prodia-0020", "language_identity");
}

#[test]
fn prodia_0021_it_extracts_text_from_user_message_and_sends_correct_request() {
    assert_upstream_case_covered("prodia-0021", "language_request_basic");
}

#[test]
fn prodia_0022_it_routes_top_level_only_image_media_type_with_detected_full_mime() {
    assert_upstream_case_covered("prodia-0022", "language_image_full_mime");
}

#[test]
fn prodia_0023_it_top_level_only_image_media_type_undetectable_keeps_default() {
    assert_upstream_case_covered("prodia-0023", "language_image_undetectable");
}

#[test]
fn prodia_0024_it_includes_system_message_in_prompt() {
    assert_upstream_case_covered("prodia-0024", "language_system_message");
}

#[test]
fn prodia_0025_it_sends_include_messages_true_in_config() {
    assert_upstream_case_covered("prodia-0025", "language_include_messages");
}

#[test]
fn prodia_0026_it_returns_text_content_from_message_txt_response_part() {
    assert_upstream_case_covered("prodia-0026", "language_text_content");
}

#[test]
fn prodia_0027_it_returns_image_content_from_image_png_response_part() {
    assert_upstream_case_covered("prodia-0027", "language_image_content");
}

#[test]
fn prodia_0028_it_returns_finish_reason_as_stop() {
    assert_upstream_case_covered("prodia-0028", "language_finish_reason");
}

#[test]
fn prodia_0029_it_returns_provider_metadata() {
    assert_upstream_case_covered("prodia-0029", "language_provider_metadata");
}

#[test]
fn prodia_0030_it_emits_warnings_for_unsupported_llm_features() {
    assert_upstream_case_covered("prodia-0030", "language_warnings");
}

#[test]
fn prodia_0031_it_passes_aspect_ratio_from_provider_options() {
    assert_upstream_case_covered("prodia-0031", "language_aspect_ratio");
}

#[test]
fn prodia_0032_it_merges_provider_and_request_headers() {
    assert_upstream_case_covered("prodia-0032", "language_headers_merge");
}

#[test]
fn prodia_0033_it_includes_timestamp_and_model_id_in_response() {
    assert_upstream_case_covered("prodia-0033", "language_response_metadata");
}

#[test]
fn prodia_0034_it_handles_api_errors() {
    assert_upstream_case_covered("prodia-0034", "language_api_error");
}

#[test]
fn prodia_0035_it_handles_response_with_text_only_no_image() {
    assert_upstream_case_covered("prodia-0035", "language_text_only");
}

#[test]
fn prodia_0036_it_wraps_do_generate_result_into_stream_parts() {
    assert_upstream_case_covered("prodia-0036", "language_stream_parts");
}
