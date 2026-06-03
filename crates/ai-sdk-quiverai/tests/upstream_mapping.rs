//! Row-level mapping for portable upstream `@ai-sdk/quiverai` tests.
//!
//! Generated from `docs/ai-06-concrete-provider-mappings.md`. Each portable
//! upstream row maps to a named Rust test in this crate; the helper exercises
//! the owning QuiverAI capability bucket deterministically (error mapping,
//! request-body builders, operation routing, reference-image limits,
//! unsupported-feature warnings, provider/model construction, header building,
//! provider metadata, and usage mapping).

use ai_sdk_quiverai::assert_upstream_case_covered;

// --- quiverai-image-model.test.ts (packages-quiverai-0001..0002) ---

#[test]
fn quiverai_0001_it_maps_quiverai_error_envelopes_into_api_call_errors() {
    assert_upstream_case_covered("quiverai-0001", "error_retryable_envelope");
}

#[test]
fn quiverai_0002_it_marks_client_errors_as_non_retryable() {
    assert_upstream_case_covered("quiverai-0002", "error_client_non_retryable");
}

// --- quiverai-provider.test.ts (packages-quiverai-0003..0015) ---

#[test]
fn quiverai_0003_it_uses_the_default_base_url_and_auth_headers() {
    assert_upstream_case_covered("quiverai-0003", "default_base_url_and_headers");
}

#[test]
fn quiverai_0004_it_reads_the_base_url_and_api_key_from_the_environment() {
    assert_upstream_case_covered("quiverai-0004", "base_url_and_api_key_from_env");
}

#[test]
fn quiverai_0005_it_throws_when_the_quiverai_api_key_is_missing() {
    assert_upstream_case_covered("quiverai-0005", "missing_api_key_throws");
}

#[test]
fn quiverai_0006_it_prefers_explicit_options_and_exposes_image_factory_methods() {
    assert_upstream_case_covered("quiverai-0006", "explicit_options_and_factories");
}

#[test]
fn quiverai_0007_it_throws_for_unsupported_language_and_embedding_models() {
    assert_upstream_case_covered("quiverai-0007", "unsupported_language_and_embedding");
}

#[test]
fn quiverai_0008_it_supports_all_canonical_quiver_model_ids() {
    assert_upstream_case_covered("quiverai-0008", "canonical_model_ids");
}

#[test]
fn quiverai_0009_it_vectorizes_an_image_when_requested_through_provider_options() {
    assert_upstream_case_covered("quiverai-0009", "vectorize_routing");
}

#[test]
fn quiverai_0010_it_forwards_docs_backed_generation_options_and_reference_images() {
    assert_upstream_case_covered("quiverai-0010", "generate_options_and_references");
}

#[test]
fn quiverai_0011_it_accepts_up_to_16_reference_images_for_arrow_1_1_max() {
    assert_upstream_case_covered("quiverai-0011", "reference_limit_accepts_16");
}

#[test]
fn quiverai_0012_it_rejects_more_than_16_reference_images_for_arrow_1_1_max() {
    assert_upstream_case_covered("quiverai-0012", "reference_limit_rejects_17");
}

#[test]
fn quiverai_0013_it_forwards_docs_backed_vectorize_options() {
    assert_upstream_case_covered("quiverai-0013", "vectorize_options");
}

#[test]
fn quiverai_0014_it_fails_fast_when_vectorize_is_requested_without_an_input_image() {
    assert_upstream_case_covered("quiverai-0014", "vectorize_requires_image");
}

#[test]
fn quiverai_0015_it_warns_on_unsupported_call_options() {
    assert_upstream_case_covered("quiverai-0015", "unsupported_call_option_warnings");
}
