//! Row-level mapping for portable upstream `@ai-sdk/replicate` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning Replicate image/video capability bucket deterministically.

use ai_sdk_replicate::assert_upstream_case_covered;

#[test]
fn replicate_0001_it_should_pass_the_model_and_the_settings() {
    assert_upstream_case_covered("replicate-0001", "image-request");
}

#[test]
fn replicate_0002_it_should_call_the_correct_url() {
    assert_upstream_case_covered("replicate-0002", "image-url");
}

#[test]
fn replicate_0003_it_should_pass_headers_and_set_the_prefer_header() {
    assert_upstream_case_covered("replicate-0003", "image-prefer");
}

#[test]
fn replicate_0004_it_should_set_custom_wait_time_in_prefer_header_when_maxwaittimeinseconds_i() {
    assert_upstream_case_covered("replicate-0004", "image-prefer-wait-time");
}

#[test]
fn replicate_0005_it_should_not_include_maxwaittimeinseconds_in_request_body() {
    assert_upstream_case_covered("replicate-0005", "image-prefer-wait-time");
}

#[test]
fn replicate_0006_it_should_extract_the_generated_image_from_array_response() {
    assert_upstream_case_covered("replicate-0006", "image-response-array");
}

#[test]
fn replicate_0007_it_should_extract_the_generated_image_from_string_response() {
    assert_upstream_case_covered("replicate-0007", "image-response-string");
}

#[test]
fn replicate_0008_it_should_return_response_metadata() {
    assert_upstream_case_covered("replicate-0008", "image-metadata");
}

#[test]
fn replicate_0009_it_should_include_response_headers_in_metadata() {
    assert_upstream_case_covered("replicate-0009", "image-metadata-headers");
}

#[test]
fn replicate_0010_it_should_set_version_in_request_body_for_versioned_models() {
    assert_upstream_case_covered("replicate-0010", "image-version-url");
}

#[test]
fn replicate_0011_it_should_send_image_when_url_file_is_provided() {
    assert_upstream_case_covered("replicate-0011", "image-file-url");
}

#[test]
fn replicate_0012_it_should_convert_uint8array_file_to_data_uri() {
    assert_upstream_case_covered("replicate-0012", "image-file-bytes");
}

#[test]
fn replicate_0013_it_should_convert_file_with_base64_string_data_to_data_uri() {
    assert_upstream_case_covered("replicate-0013", "image-file-base64");
}

#[test]
fn replicate_0014_it_should_send_mask_for_inpainting() {
    assert_upstream_case_covered("replicate-0014", "image-mask");
}

#[test]
fn replicate_0015_it_should_warn_when_multiple_files_are_provided() {
    assert_upstream_case_covered("replicate-0015", "image-warn-multiple-files");
}

#[test]
fn replicate_0016_it_should_pass_provider_options_with_image_editing() {
    assert_upstream_case_covered("replicate-0016", "image-provider-options");
}

#[test]
fn replicate_0017_it_should_report_maximagespercall_as_8_for_flux_2_models() {
    assert_upstream_case_covered("replicate-0017", "image-flux2-max-images-8");
}

#[test]
fn replicate_0018_it_should_report_maximagespercall_as_1_for_non_flux_2_models() {
    assert_upstream_case_covered("replicate-0018", "image-non-flux2-max-images-1");
}

#[test]
fn replicate_0019_it_should_send_single_image_as_input_image_for_flux_2_models() {
    assert_upstream_case_covered("replicate-0019", "image-flux2-single-image");
}

#[test]
fn replicate_0020_it_should_send_multiple_images_as_input_image_input_image_2_etc_for_flux_2() {
    assert_upstream_case_covered("replicate-0020", "image-flux2-multiple-images");
}

#[test]
fn replicate_0021_it_should_warn_when_more_than_8_images_are_provided_for_flux_2_models() {
    assert_upstream_case_covered("replicate-0021", "image-flux2-warn-too-many");
}

#[test]
fn replicate_0022_it_should_warn_and_ignore_mask_for_flux_2_models() {
    assert_upstream_case_covered("replicate-0022", "image-flux2-warn-mask");
}

#[test]
fn replicate_0023_it_should_call_correct_url_for_flux_2_models() {
    assert_upstream_case_covered("replicate-0023", "image-flux2-url");
}

#[test]
fn replicate_0024_it_creates_a_provider_with_required_settings() {
    assert_upstream_case_covered("replicate-0024", "provider-default");
}

#[test]
fn replicate_0025_it_creates_a_provider_with_custom_settings() {
    assert_upstream_case_covered("replicate-0025", "provider-custom");
}

#[test]
fn replicate_0026_it_creates_an_image_model_instance() {
    assert_upstream_case_covered("replicate-0026", "provider-image-instance");
}

#[test]
fn replicate_0027_it_creates_a_video_model_instance() {
    assert_upstream_case_covered("replicate-0027", "provider-video-instance");
}

#[test]
fn replicate_0028_it_uses_custom_baseurl_for_video_model_when_provided() {
    assert_upstream_case_covered("replicate-0028", "provider-video-baseurl");
}

#[test]
fn replicate_0029_it_passes_custom_fetch_to_video_model() {
    assert_upstream_case_covered("replicate-0029", "provider-video-transport");
}

#[test]
fn replicate_0030_it_should_expose_correct_provider_and_model_information() {
    assert_upstream_case_covered("replicate-0030", "video-info");
}

#[test]
fn replicate_0031_it_should_support_model_ids_with_versions() {
    assert_upstream_case_covered("replicate-0031", "video-version-id");
}

#[test]
fn replicate_0032_it_should_pass_the_correct_parameters_including_prompt() {
    assert_upstream_case_covered("replicate-0032", "video-body-prompt");
}

#[test]
fn replicate_0033_it_should_use_models_modelid_predictions_for_models_without_version() {
    assert_upstream_case_covered("replicate-0033", "video-url-no-version");
}

#[test]
fn replicate_0034_it_should_use_predictions_with_version_for_models_with_version() {
    assert_upstream_case_covered("replicate-0034", "video-url-with-version");
}

#[test]
fn replicate_0035_it_should_pass_seed_when_provided() {
    assert_upstream_case_covered("replicate-0035", "video-seed");
}

#[test]
fn replicate_0036_it_should_pass_aspect_ratio_when_provided() {
    assert_upstream_case_covered("replicate-0036", "video-aspect-ratio");
}

#[test]
fn replicate_0037_it_should_pass_through_9_16_aspect_ratio() {
    assert_upstream_case_covered("replicate-0037", "video-aspect-ratio");
}

#[test]
fn replicate_0038_it_should_pass_through_1_1_aspect_ratio() {
    assert_upstream_case_covered("replicate-0038", "video-aspect-ratio");
}

#[test]
fn replicate_0039_it_should_pass_through_other_aspect_ratios() {
    assert_upstream_case_covered("replicate-0039", "video-aspect-ratio");
}

#[test]
fn replicate_0040_it_should_pass_resolution_as_size_when_provided() {
    assert_upstream_case_covered("replicate-0040", "video-resolution-size");
}

#[test]
fn replicate_0041_it_should_pass_duration_when_provided() {
    assert_upstream_case_covered("replicate-0041", "video-duration");
}

#[test]
fn replicate_0042_it_should_pass_fps_when_provided() {
    assert_upstream_case_covered("replicate-0042", "video-fps");
}

#[test]
fn replicate_0043_it_should_return_video_with_correct_data() {
    assert_upstream_case_covered("replicate-0043", "video-response-data");
}

#[test]
fn replicate_0044_it_should_return_warnings_array() {
    assert_upstream_case_covered("replicate-0044", "video-warnings-array");
}

#[test]
fn replicate_0045_it_should_include_timestamp_and_modelid_in_response() {
    assert_upstream_case_covered("replicate-0045", "video-response-timestamp-model");
}

#[test]
fn replicate_0046_it_should_include_prediction_metadata() {
    assert_upstream_case_covered("replicate-0046", "video-prediction-metadata");
}

#[test]
fn replicate_0047_it_should_send_url_based_image_directly() {
    assert_upstream_case_covered("replicate-0047", "video-image-url");
}

#[test]
fn replicate_0048_it_should_convert_base64_image_to_data_uri() {
    assert_upstream_case_covered("replicate-0048", "video-image-base64");
}

#[test]
fn replicate_0049_it_should_pass_guidance_scale_option() {
    assert_upstream_case_covered("replicate-0049", "video-custom-options");
}

#[test]
fn replicate_0050_it_should_pass_num_inference_steps_option() {
    assert_upstream_case_covered("replicate-0050", "video-custom-options");
}

#[test]
fn replicate_0051_it_should_pass_motion_bucket_id_for_stable_video_diffusion() {
    assert_upstream_case_covered("replicate-0051", "video-custom-options");
}

#[test]
fn replicate_0052_it_should_pass_prompt_optimizer_for_minimax() {
    assert_upstream_case_covered("replicate-0052", "video-custom-options");
}

#[test]
fn replicate_0053_it_should_pass_through_custom_options() {
    assert_upstream_case_covered("replicate-0053", "video-custom-options");
}

#[test]
fn replicate_0054_it_should_use_maxwaittimeinseconds_in_prefer_header() {
    assert_upstream_case_covered("replicate-0054", "video-prefer-wait-time");
}

#[test]
fn replicate_0055_it_should_use_prefer_wait_when_maxwaittimeinseconds_not_provided() {
    assert_upstream_case_covered("replicate-0055", "video-prefer-default");
}

#[test]
fn replicate_0056_it_should_throw_error_when_prediction_fails() {
    assert_upstream_case_covered("replicate-0056", "video-error-failed");
}

#[test]
fn replicate_0057_it_should_throw_error_when_prediction_is_canceled() {
    assert_upstream_case_covered("replicate-0057", "video-error-canceled");
}

#[test]
fn replicate_0058_it_should_throw_error_when_no_video_url_in_response() {
    assert_upstream_case_covered("replicate-0058", "video-error-no-url");
}

#[test]
fn replicate_0059_it_should_poll_until_prediction_is_done() {
    assert_upstream_case_covered("replicate-0059", "video-poll-end-to-end");
}

#[test]
fn replicate_0060_it_should_timeout_after_polltimeoutms() {
    assert_upstream_case_covered("replicate-0060", "video-poll-timeout");
}

#[test]
fn replicate_0061_it_should_respect_abort_signal() {
    assert_upstream_case_covered("replicate-0061", "video-abort");
}

#[test]
fn replicate_0062_it_should_handle_immediate_success_pollsuntildone_0() {
    assert_upstream_case_covered("replicate-0062", "video-immediate-success");
}

#[test]
fn replicate_0063_it_should_always_return_video_mp4_as_media_type() {
    assert_upstream_case_covered("replicate-0063", "video-media-type");
}
