use serde::{Deserialize, Serialize};

use crate::provider::ProviderMetadata;
use crate::transcription_model::{
    NoTranscriptGeneratedError, TranscriptionModelResponse, TranscriptionModelSegment,
};
use crate::warning::Warning;

/// Result of a high-level `transcribe` call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
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

    /// Response metadata from provider calls.
    pub responses: Vec<TranscriptionModelResponse>,

    /// Provider-specific metadata returned by the provider.
    pub provider_metadata: ProviderMetadata,
}

impl TranscriptionResult {
    /// Creates a high-level transcription result.
    pub fn new(
        text: impl Into<String>,
        segments: Vec<TranscriptionModelSegment>,
        warnings: Vec<Warning>,
        responses: Vec<TranscriptionModelResponse>,
        provider_metadata: ProviderMetadata,
    ) -> Result<Self, NoTranscriptGeneratedError> {
        let text = text.into();

        if text.is_empty() {
            return Err(NoTranscriptGeneratedError::new(responses));
        }

        Ok(Self {
            text,
            segments,
            language: None,
            duration_in_seconds: None,
            warnings,
            responses,
            provider_metadata,
        })
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
}

/// Upstream-compatible experimental result alias for [`TranscriptionResult`].
pub type ExperimentalTranscriptionResult = TranscriptionResult;

#[cfg(test)]
mod tests {
    use super::{ExperimentalTranscriptionResult, TranscriptionResult};
    use crate::provider::ProviderMetadata;
    use crate::transcription_model::{TranscriptionModelResponse, TranscriptionModelSegment};
    use crate::warning::Warning;
    use serde_json::json;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    fn transcription_response(model_id: &str) -> TranscriptionModelResponse {
        TranscriptionModelResponse::new(
            OffsetDateTime::parse("2024-01-02T03:04:05Z", &Rfc3339).expect("timestamp parses"),
            model_id,
        )
    }

    #[test]
    fn transcription_result_serializes_upstream_shape() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "transcriptionId": "tr_123"
            }
        }))
        .expect("provider metadata deserializes");

        let result = TranscriptionResult::new(
            "Hello world.",
            vec![TranscriptionModelSegment::new("Hello world.", 0.0, 1.5)],
            vec![Warning::Unsupported {
                feature: "speakerDiarization".to_string(),
                details: None,
            }],
            vec![transcription_response("openai/whisper-1").with_header("x-request-id", "req_123")],
            provider_metadata,
        )
        .expect("transcript is present")
        .with_language("en")
        .with_duration_in_seconds(1.5);

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
                "responses": [
                    {
                        "timestamp": "2024-01-02T03:04:05Z",
                        "modelId": "openai/whisper-1",
                        "headers": {
                            "x-request-id": "req_123"
                        }
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "transcriptionId": "tr_123"
                    }
                }
            })
        );
    }

    #[test]
    fn transcription_result_deserializes_minimal_response_and_empty_metadata() {
        let result: TranscriptionResult = serde_json::from_value(json!({
            "text": "Hello.",
            "segments": [],
            "warnings": [],
            "responses": [
                {
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "transcribe-test"
                }
            ],
            "providerMetadata": {}
        }))
        .expect("result deserializes");

        assert_eq!(
            result,
            TranscriptionResult::new(
                "Hello.",
                Vec::new(),
                Vec::new(),
                vec![transcription_response("transcribe-test")],
                ProviderMetadata::new(),
            )
            .expect("transcript is present")
        );
    }

    #[test]
    fn transcription_result_errors_when_text_is_empty() {
        let response =
            transcription_response("transcribe-test").with_header("x-request-id", "req_empty");

        let error = TranscriptionResult::new(
            "",
            Vec::new(),
            Vec::new(),
            vec![response.clone()],
            ProviderMetadata::new(),
        )
        .expect_err("empty transcript errors");

        assert_eq!(error.message(), "No transcript generated.");
        assert_eq!(error.responses(), &[response]);
    }

    #[test]
    fn experimental_transcription_result_alias_preserves_shape() {
        let result: ExperimentalTranscriptionResult = TranscriptionResult::new(
            "Hello.",
            Vec::new(),
            Vec::new(),
            vec![transcription_response("transcribe-test")],
            ProviderMetadata::new(),
        )
        .expect("transcript is present");

        assert_eq!(result.text, "Hello.");
    }
}
