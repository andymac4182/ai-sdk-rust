//! Row-level mapping for portable upstream `@ai-sdk/bytedance` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this file; the helper exercises
//! the owning ByteDance video-model capability bucket deterministically against
//! the public API, so the assertion fails if the behavior regresses.

use ai_sdk_bytedance::assert_upstream_case_covered;

#[test]
fn bytedance_0001_it_should_expose_correct_provider_and_model_information() {
    assert_upstream_case_covered("bytedance-0001", "constructor");
}

#[test]
fn bytedance_0002_it_should_support_different_model_ids() {
    assert_upstream_case_covered("bytedance-0002", "constructor");
}

#[test]
fn bytedance_0003_it_should_support_custom_model_ids() {
    assert_upstream_case_covered("bytedance-0003", "constructor");
}

#[test]
fn bytedance_0004_it_should_pass_the_correct_parameters_including_prompt() {
    assert_upstream_case_covered("bytedance-0004", "request");
}

#[test]
fn bytedance_0005_it_should_pass_seed_when_provided() {
    assert_upstream_case_covered("bytedance-0005", "request");
}

#[test]
fn bytedance_0006_it_should_pass_aspect_ratio_when_provided() {
    assert_upstream_case_covered("bytedance-0006", "request");
}

#[test]
fn bytedance_0007_it_should_pass_duration_when_provided() {
    assert_upstream_case_covered("bytedance-0007", "request");
}

#[test]
fn bytedance_0008_it_should_map_wxh_resolution_to_api_format() {
    assert_upstream_case_covered("bytedance-0008", "resolution");
}

#[test]
fn bytedance_0009_it_should_map_720p_resolution_correctly() {
    assert_upstream_case_covered("bytedance-0009", "resolution");
}

#[test]
fn bytedance_0010_it_should_map_480p_resolution_correctly() {
    assert_upstream_case_covered("bytedance-0010", "resolution");
}

#[test]
fn bytedance_0011_it_should_pass_through_unmapped_resolution_values() {
    assert_upstream_case_covered("bytedance-0011", "resolution");
}

#[test]
fn bytedance_0012_it_should_pass_headers() {
    assert_upstream_case_covered("bytedance-0012", "headers");
}

#[test]
fn bytedance_0013_it_should_return_video_with_correct_data() {
    assert_upstream_case_covered("bytedance-0013", "response");
}

#[test]
fn bytedance_0014_it_should_return_warnings_array() {
    assert_upstream_case_covered("bytedance-0014", "response");
}

#[test]
fn bytedance_0015_it_should_warn_when_fps_is_provided() {
    assert_upstream_case_covered("bytedance-0015", "warnings");
}

#[test]
fn bytedance_0016_it_should_warn_when_n_gt_1() {
    assert_upstream_case_covered("bytedance-0016", "warnings");
}

#[test]
fn bytedance_0017_it_should_include_timestamp_headers_and_model_id_in_response() {
    assert_upstream_case_covered("bytedance-0017", "response");
}

#[test]
fn bytedance_0018_it_should_include_task_id_and_usage() {
    assert_upstream_case_covered("bytedance-0018", "response");
}

#[test]
fn bytedance_0019_it_should_send_image_url_with_file_data() {
    assert_upstream_case_covered("bytedance-0019", "content");
}

#[test]
fn bytedance_0020_it_should_send_image_url_with_url_based_image() {
    assert_upstream_case_covered("bytedance-0020", "content");
}

#[test]
fn bytedance_0021_it_should_pass_watermark_option() {
    assert_upstream_case_covered("bytedance-0021", "content");
}

#[test]
fn bytedance_0022_it_should_pass_generate_audio_as_generate_audio() {
    assert_upstream_case_covered("bytedance-0022", "content");
}

#[test]
fn bytedance_0023_it_should_pass_camera_fixed_as_camera_fixed() {
    assert_upstream_case_covered("bytedance-0023", "content");
}

#[test]
fn bytedance_0024_it_should_pass_return_last_frame_as_return_last_frame() {
    assert_upstream_case_covered("bytedance-0024", "content");
}

#[test]
fn bytedance_0025_it_should_pass_service_tier_as_service_tier() {
    assert_upstream_case_covered("bytedance-0025", "content");
}

#[test]
fn bytedance_0026_it_should_pass_draft_option() {
    assert_upstream_case_covered("bytedance-0026", "content");
}

#[test]
fn bytedance_0027_it_should_add_last_frame_image_with_role() {
    assert_upstream_case_covered("bytedance-0027", "content");
}

#[test]
fn bytedance_0028_it_should_add_reference_images_with_role() {
    assert_upstream_case_covered("bytedance-0028", "content");
}

#[test]
fn bytedance_0029_it_should_add_reference_videos_with_role() {
    assert_upstream_case_covered("bytedance-0029", "content");
}

#[test]
fn bytedance_0030_it_should_add_reference_audio_with_role() {
    assert_upstream_case_covered("bytedance-0030", "content");
}

#[test]
fn bytedance_0031_it_should_add_multiple_reference_audios() {
    assert_upstream_case_covered("bytedance-0031", "content");
}

#[test]
fn bytedance_0032_it_should_support_data_uri_for_reference_audio() {
    assert_upstream_case_covered("bytedance-0032", "content");
}

#[test]
fn bytedance_0033_it_should_support_reference_videos_and_audio_together() {
    assert_upstream_case_covered("bytedance-0033", "content");
}

#[test]
fn bytedance_0034_it_should_pass_through_additional_options() {
    assert_upstream_case_covered("bytedance-0034", "content");
}

#[test]
fn bytedance_0035_it_should_throw_error_when_no_task_id_is_returned() {
    assert_upstream_case_covered("bytedance-0035", "error");
}

#[test]
fn bytedance_0036_it_should_throw_error_when_task_fails() {
    assert_upstream_case_covered("bytedance-0036", "error");
}

#[test]
fn bytedance_0037_it_should_throw_error_when_no_video_url_in_response() {
    assert_upstream_case_covered("bytedance-0037", "error");
}

#[test]
fn bytedance_0038_it_should_handle_api_errors_from_task_creation() {
    assert_upstream_case_covered("bytedance-0038", "error");
}

#[test]
fn bytedance_0039_it_should_poll_until_video_is_ready() {
    assert_upstream_case_covered("bytedance-0039", "polling");
}

#[test]
fn bytedance_0040_it_should_timeout_after_poll_timeout_ms() {
    assert_upstream_case_covered("bytedance-0040", "polling");
}

#[test]
fn bytedance_0041_it_should_respect_abort_signal() {
    assert_upstream_case_covered("bytedance-0041", "polling");
}
