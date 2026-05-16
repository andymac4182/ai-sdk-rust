use serde::{Deserialize, Serialize};
use std::{fmt, future::Future};
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

/// A provider-v4 transcription model.
///
/// The upstream TypeScript contract exposes a `doGenerate` method returning a
/// `PromiseLike<TranscriptionModelV4Result>`. This Rust trait maps that
/// boundary to an associated [`Future`] without introducing an async-trait
/// dependency.
pub trait TranscriptionModel {
    /// Future returned by [`TranscriptionModel::do_generate`].
    type GenerateFuture<'a>: Future<Output = TranscriptionModelResult> + Send + 'a
    where
        Self: 'a;

    /// Returns the provider/model interface version implemented by this model.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Returns the provider identifier.
    fn provider(&self) -> &str;

    /// Returns the provider-specific model id.
    fn model_id(&self) -> &str;

    /// Generates a transcript for the supplied audio options.
    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_>;
}

/// Options passed to a transcription model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionModelCallOptions {
    /// Audio data to transcribe, as raw bytes or base64-encoded audio.
    pub audio: FileDataContent,

    /// The IANA media type of the audio data.
    pub media_type: String,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl TranscriptionModelCallOptions {
    /// Creates transcription model call options with the required audio input.
    pub fn new(audio: FileDataContent, media_type: impl Into<String>) -> Self {
        Self {
            audio,
            media_type: media_type.into(),
            provider_options: None,
            headers: None,
        }
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// A timed transcript segment returned by a transcription model.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionModelSegment {
    /// The text content of this segment.
    pub text: String,

    /// The start time of this segment in seconds.
    pub start_second: f64,

    /// The end time of this segment in seconds.
    pub end_second: f64,
}

impl TranscriptionModelSegment {
    /// Creates a timed transcript segment.
    pub fn new(text: impl Into<String>, start_second: f64, end_second: f64) -> Self {
        Self {
            text: text.into(),
            start_second,
            end_second,
        }
    }
}

/// Optional request information for telemetry and debugging transcription calls.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionModelRequest {
    /// Raw request HTTP body that was sent to the provider API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl TranscriptionModelRequest {
    /// Creates empty request metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the raw provider request body.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// Response information for telemetry and debugging transcription calls.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionModelResponse {
    /// Timestamp for the start of the generated response.
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,

    /// Provider model identifier used for the response.
    pub model_id: String,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl TranscriptionModelResponse {
    /// Creates response metadata with the required timestamp and model id.
    pub fn new(timestamp: OffsetDateTime, model_id: impl Into<String>) -> Self {
        Self {
            timestamp,
            model_id: model_id.into(),
            headers: None,
            body: None,
        }
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the raw provider response body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }
}

/// Error returned when high-level transcription produces no transcript.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoTranscriptGeneratedError {
    responses: Vec<TranscriptionModelResponse>,
}

impl NoTranscriptGeneratedError {
    /// Creates a no-transcript error with response metadata from attempted provider calls.
    pub fn new(responses: impl IntoIterator<Item = TranscriptionModelResponse>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
        }
    }

    /// Returns the upstream human-readable error message.
    pub fn message(&self) -> &'static str {
        "No transcript generated."
    }

    /// Returns response metadata for attempted provider calls.
    pub fn responses(&self) -> &[TranscriptionModelResponse] {
        &self.responses
    }

    /// Converts this error into its response metadata.
    pub fn into_responses(self) -> Vec<TranscriptionModelResponse> {
        self.responses
    }
}

impl fmt::Display for NoTranscriptGeneratedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for NoTranscriptGeneratedError {}

/// Result of a transcription model provider call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionModelResult {
    /// The complete transcribed text from the audio.
    pub text: String,

    /// Timed transcript segments.
    pub segments: Vec<TranscriptionModelSegment>,

    /// Detected language as an ISO-639-1 code, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Total duration of the audio file in seconds, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_in_seconds: Option<f64>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Optional request information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<TranscriptionModelRequest>,

    /// Response information for telemetry and debugging.
    pub response: TranscriptionModelResponse,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TranscriptionModelResult {
    /// Creates a transcription result with no warnings.
    pub fn new(
        text: impl Into<String>,
        segments: Vec<TranscriptionModelSegment>,
        response: TranscriptionModelResponse,
    ) -> Self {
        Self {
            text: text.into(),
            segments,
            language: None,
            duration_in_seconds: None,
            warnings: Vec::new(),
            request: None,
            response,
            provider_metadata: None,
        }
    }

    /// Sets the detected language.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Sets the audio duration in seconds.
    pub fn with_duration_in_seconds(mut self, duration_in_seconds: f64) -> Self {
        self.duration_in_seconds = Some(duration_in_seconds);
        self
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Sets optional request information.
    pub fn with_request(mut self, request: TranscriptionModelRequest) -> Self {
        self.request = Some(request);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NoTranscriptGeneratedError, TranscriptionModel, TranscriptionModelCallOptions,
        TranscriptionModelRequest, TranscriptionModelResponse, TranscriptionModelResult,
        TranscriptionModelSegment,
    };
    use crate::file_data::FileDataContent;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    struct StaticTranscriptionModel;

    impl TranscriptionModel for StaticTranscriptionModel {
        type GenerateFuture<'a>
            = Ready<TranscriptionModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "transcribe-test"
        }

        fn do_generate(&self, _options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
            let timestamp =
                OffsetDateTime::parse("2024-01-02T03:04:05Z", &Rfc3339).expect("timestamp parses");

            ready(TranscriptionModelResult::new(
                "hello world",
                vec![TranscriptionModelSegment::new("hello world", 0.0, 1.2)],
                TranscriptionModelResponse::new(timestamp, self.model_id()),
            ))
        }
    }

    fn poll_ready<T>(mut future: Ready<T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("std::future::Ready never returns pending"),
        }
    }

    #[test]
    fn call_options_serializes_upstream_shape_with_audio_options_and_headers() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "timestampGranularities": ["word"]
            }
        }))
        .expect("provider options deserialize");

        let options = TranscriptionModelCallOptions::new(
            FileDataContent::Base64("UklGRiQAAABXQVZF".to_string()),
            "audio/wav",
        )
        .with_provider_options(provider_options)
        .with_header("x-request-id", "req_123");

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "audio": "UklGRiQAAABXQVZF",
                "mediaType": "audio/wav",
                "providerOptions": {
                    "openai": {
                        "timestampGranularities": ["word"]
                    }
                },
                "headers": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn call_options_deserializes_raw_audio_bytes_and_omits_optional_fields() {
        let options: TranscriptionModelCallOptions = serde_json::from_value(json!({
            "audio": [82, 73, 70, 70],
            "mediaType": "audio/wav"
        }))
        .expect("call options deserialize");

        assert_eq!(
            options,
            TranscriptionModelCallOptions::new(
                FileDataContent::Bytes(vec![82, 73, 70, 70]),
                "audio/wav"
            )
        );
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "audio": [82, 73, 70, 70],
                "mediaType": "audio/wav"
            })
        );
    }

    #[test]
    fn transcription_model_trait_exposes_upstream_v4_identity_and_generate_boundary() {
        let model = StaticTranscriptionModel;
        let options = TranscriptionModelCallOptions::new(
            FileDataContent::Base64("UklGRg==".to_string()),
            "audio/wav",
        );

        let result = poll_ready(model.do_generate(options));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "transcribe-test");
        assert_eq!(result.text, "hello world");
        assert_eq!(result.response.model_id, "transcribe-test");
    }

    #[test]
    fn no_transcript_generated_error_matches_upstream_message_and_retains_responses() {
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses");
        let response = TranscriptionModelResponse::new(response_timestamp, "openai/whisper-1")
            .with_header("x-request-id", "req_123")
            .with_body(json!({
                "text": ""
            }));

        let error = NoTranscriptGeneratedError::new([response.clone()]);

        assert_eq!(error.message(), "No transcript generated.");
        assert_eq!(error.to_string(), "No transcript generated.");
        assert_eq!(error.responses(), std::slice::from_ref(&response));
        assert_eq!(error.into_responses(), vec![response]);
    }

    #[test]
    fn result_serializes_upstream_shape_with_segments_response_and_metadata() {
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "transcriptionId": "tr_123"
            }
        }))
        .expect("provider metadata deserializes");

        let result = TranscriptionModelResult::new(
            "Hello world.",
            vec![TranscriptionModelSegment::new("Hello world.", 0.0, 1.5)],
            TranscriptionModelResponse::new(response_timestamp, "openai/whisper-1")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "text": "Hello world."
                })),
        )
        .with_language("en")
        .with_duration_in_seconds(1.5)
        .with_warning(Warning::Unsupported {
            feature: "speakerDiarization".to_string(),
            details: None,
        })
        .with_request(TranscriptionModelRequest::new().with_body("{\"model\":\"whisper-1\"}"))
        .with_provider_metadata(provider_metadata);

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "text": "Hello world.",
                "segments": [
                    {
                        "text": "Hello world.",
                        "startSecond": 0.0,
                        "endSecond": 1.5
                    }
                ],
                "language": "en",
                "durationInSeconds": 1.5,
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "speakerDiarization"
                    }
                ],
                "request": {
                    "body": "{\"model\":\"whisper-1\"}"
                },
                "response": {
                    "timestamp": "2026-05-16T09:30:00Z",
                    "modelId": "openai/whisper-1",
                    "headers": {
                        "x-request-id": "req_123"
                    },
                    "body": {
                        "text": "Hello world."
                    }
                },
                "providerMetadata": {
                    "openai": {
                        "transcriptionId": "tr_123"
                    }
                }
            })
        );
    }

    #[test]
    fn result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: TranscriptionModelResult = serde_json::from_value(json!({
            "text": "Hello.",
            "segments": [],
            "warnings": [],
            "response": {
                "timestamp": "2026-05-16T09:30:00Z",
                "modelId": "provider/transcribe"
            }
        }))
        .expect("result deserializes");

        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses");

        assert_eq!(
            result,
            TranscriptionModelResult::new(
                "Hello.",
                Vec::new(),
                TranscriptionModelResponse::new(response_timestamp, "provider/transcribe"),
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "text": "Hello.",
                "segments": [],
                "warnings": [],
                "response": {
                    "timestamp": "2026-05-16T09:30:00Z",
                    "modelId": "provider/transcribe"
                }
            })
        );
    }

    #[test]
    fn result_requires_warnings_and_response_metadata() {
        let missing_warnings = serde_json::from_value::<TranscriptionModelResult>(json!({
            "text": "Hello.",
            "segments": [],
            "response": {
                "timestamp": "2026-05-16T09:30:00Z",
                "modelId": "provider/transcribe"
            }
        }))
        .expect_err("warnings are required");

        assert!(
            missing_warnings
                .to_string()
                .contains("missing field `warnings`")
        );

        let missing_response = serde_json::from_value::<TranscriptionModelResult>(json!({
            "text": "Hello.",
            "segments": [],
            "warnings": []
        }))
        .expect_err("response metadata is required");

        assert!(
            missing_response
                .to_string()
                .contains("missing field `response`")
        );
    }
}
