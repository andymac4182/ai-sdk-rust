use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::warning::Warning;

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
        TranscriptionModelCallOptions, TranscriptionModelRequest, TranscriptionModelResponse,
        TranscriptionModelResult, TranscriptionModelSegment,
    };
    use crate::file_data::FileDataContent;
    use crate::provider::{ProviderMetadata, ProviderOptions};
    use crate::warning::Warning;
    use serde_json::json;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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
