use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::provider::ProviderMetadata;
use crate::provider::ProviderOptions;
use crate::provider_utils::{convert_base64_to_bytes, detect_media_type, with_user_agent_suffix};
use crate::transcription_model::{
    NoTranscriptGeneratedError, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelResponse, TranscriptionModelResult, TranscriptionModelSegment,
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

/// Options for a high-level `transcribe` call.
pub struct TranscribeOptions<'a, M: TranscriptionModel + ?Sized> {
    /// Transcription model used for the call.
    pub model: &'a M,

    /// Audio data to transcribe.
    pub audio: FileDataContent,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,
}

impl<'a, M: TranscriptionModel + ?Sized> TranscribeOptions<'a, M> {
    /// Creates options for a high-level `transcribe` call.
    pub fn new(model: &'a M, audio: FileDataContent) -> Self {
        Self {
            model,
            audio,
            provider_options: None,
            headers: None,
        }
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets all additional HTTP headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds an additional HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Generates a transcript using a transcription model.
pub async fn transcribe<M: TranscriptionModel + ?Sized>(
    options: TranscribeOptions<'_, M>,
) -> Result<TranscriptionResult, NoTranscriptGeneratedError> {
    let TranscribeOptions {
        model,
        audio,
        provider_options,
        headers,
    } = options;

    let audio = normalize_audio_data(audio);
    let media_type = detect_media_type(&audio, Some("audio"))
        .unwrap_or("audio/wav")
        .to_string();
    let headers = headers_with_ai_user_agent(headers);

    let TranscriptionModelResult {
        text,
        segments,
        language,
        duration_in_seconds,
        warnings,
        request: _,
        response,
        provider_metadata,
    } = model
        .do_generate(TranscriptionModelCallOptions {
            audio,
            media_type,
            provider_options: Some(provider_options.unwrap_or_default()),
            headers: Some(headers),
        })
        .await;

    if text.is_empty() {
        return Err(NoTranscriptGeneratedError::new([response]));
    }

    Ok(TranscriptionResult {
        text,
        segments,
        language,
        duration_in_seconds,
        warnings,
        responses: vec![response],
        provider_metadata: provider_metadata.unwrap_or_default(),
    })
}

/// Upstream-compatible experimental alias for [`transcribe`].
pub async fn experimental_transcribe<M: TranscriptionModel + ?Sized>(
    options: TranscribeOptions<'_, M>,
) -> Result<ExperimentalTranscriptionResult, NoTranscriptGeneratedError> {
    transcribe(options).await
}

fn normalize_audio_data(audio: FileDataContent) -> FileDataContent {
    match audio {
        FileDataContent::Base64(base64) => convert_base64_to_bytes(&base64)
            .map(FileDataContent::Bytes)
            .unwrap_or(FileDataContent::Base64(base64)),
        FileDataContent::Bytes(bytes) => FileDataContent::Bytes(bytes),
    }
}

fn headers_with_ai_user_agent(headers: Option<Headers>) -> Headers {
    let header_entries: Vec<(String, Option<String>)> = headers
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect();

    with_user_agent_suffix(Some(header_entries), [format!("ai/{VERSION}")])
}

#[cfg(test)]
mod tests {
    use super::{
        ExperimentalTranscriptionResult, TranscribeOptions, TranscriptionResult,
        experimental_transcribe, transcribe,
    };
    use crate::VERSION;
    use crate::file_data::FileDataContent;
    use crate::headers::Headers;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
        TranscriptionModelResult, TranscriptionModelSegment,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    struct RecordingTranscriptionModel {
        calls: Mutex<Vec<TranscriptionModelCallOptions>>,
        results: Mutex<VecDeque<TranscriptionModelResult>>,
    }

    impl RecordingTranscriptionModel {
        fn new(results: Vec<TranscriptionModelResult>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results.into()),
            }
        }

        fn calls(&self) -> Vec<TranscriptionModelCallOptions> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .clone()
        }
    }

    impl TranscriptionModel for RecordingTranscriptionModel {
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

        fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .push(options);

            ready(
                self.results
                    .lock()
                    .expect("results lock is not poisoned")
                    .pop_front()
                    .unwrap_or_else(|| {
                        TranscriptionModelResult::new(
                            "fallback transcript",
                            Vec::new(),
                            transcription_response("fallback"),
                        )
                    }),
            )
        }
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

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

    #[test]
    fn transcribe_forwards_normalized_audio_options_headers_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "timestampGranularities": ["word"]
            }
        }))
        .expect("provider options deserialize");
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "Hello world.",
            Vec::new(),
            transcription_response("openai/whisper-1"),
        )]);

        let result = poll_ready(transcribe(
            TranscribeOptions::new(&model, FileDataContent::Base64("//s=".to_string()))
                .with_provider_options(provider_options.clone())
                .with_header("custom-request-header", "request-header-value"),
        ))
        .expect("transcription succeeds");

        assert_eq!(result.text, "Hello world.");
        assert_eq!(
            model.calls(),
            vec![TranscriptionModelCallOptions {
                audio: FileDataContent::Bytes(vec![0xff, 0xfb]),
                media_type: "audio/mpeg".to_string(),
                provider_options: Some(provider_options),
                headers: Some({
                    let mut headers = Headers::new();
                    headers.insert(
                        "custom-request-header".to_string(),
                        "request-header-value".to_string(),
                    );
                    headers.insert("user-agent".to_string(), format!("ai/{VERSION}"));
                    headers
                }),
            }]
        );
    }

    #[test]
    fn transcribe_uses_audio_wav_fallback_and_empty_provider_options() {
        let model = RecordingTranscriptionModel::new(vec![
            TranscriptionModelResult::new(
                "Hello world.",
                vec![TranscriptionModelSegment::new("Hello world.", 0.0, 1.5)],
                transcription_response("transcribe-test").with_header("x-request-id", "req_123"),
            )
            .with_language("en")
            .with_duration_in_seconds(1.5)
            .with_warning(Warning::Other {
                message: "setting ignored".to_string(),
            }),
        ]);

        let result = poll_ready(transcribe(TranscribeOptions::new(
            &model,
            FileDataContent::Bytes(vec![1, 2, 3, 4]),
        )))
        .expect("transcription succeeds");

        assert_eq!(
            model.calls()[0],
            TranscriptionModelCallOptions {
                audio: FileDataContent::Bytes(vec![1, 2, 3, 4]),
                media_type: "audio/wav".to_string(),
                provider_options: Some(ProviderOptions::new()),
                headers: Some({
                    let mut headers = Headers::new();
                    headers.insert("user-agent".to_string(), format!("ai/{VERSION}"));
                    headers
                }),
            }
        );
        assert_eq!(result.text, "Hello world.");
        assert_eq!(
            result.segments,
            vec![TranscriptionModelSegment::new("Hello world.", 0.0, 1.5)]
        );
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(result.duration_in_seconds, Some(1.5));
        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "setting ignored".to_string()
            }]
        );
        assert_eq!(
            result.responses,
            vec![transcription_response("transcribe-test").with_header("x-request-id", "req_123")]
        );
        assert_eq!(result.provider_metadata, ProviderMetadata::new());
    }

    #[test]
    fn transcribe_defaults_missing_provider_metadata_to_empty_object() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "transcriptionId": "tr_123"
            }
        }))
        .expect("provider metadata deserializes");
        let model = RecordingTranscriptionModel::new(vec![
            TranscriptionModelResult::new(
                "Hello.",
                Vec::new(),
                transcription_response("openai/whisper-1"),
            )
            .with_provider_metadata(provider_metadata.clone()),
        ]);

        let result = poll_ready(transcribe(TranscribeOptions::new(
            &model,
            FileDataContent::Bytes(vec![0xff, 0xfb]),
        )))
        .expect("transcription succeeds");

        assert_eq!(result.provider_metadata, provider_metadata);
    }

    #[test]
    fn transcribe_errors_when_model_returns_no_text() {
        let response =
            transcription_response("transcribe-test").with_header("x-request-id", "req_empty");
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "",
            Vec::new(),
            response.clone(),
        )]);

        let error = poll_ready(transcribe(TranscribeOptions::new(
            &model,
            FileDataContent::Bytes(vec![0xff, 0xfb]),
        )))
        .expect_err("empty transcript errors");

        assert_eq!(error.message(), "No transcript generated.");
        assert_eq!(error.responses(), &[response]);
    }

    #[test]
    fn experimental_transcribe_alias_preserves_runtime_behavior() {
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "Experimental transcript.",
            Vec::new(),
            transcription_response("transcribe-test"),
        )]);

        let result = poll_ready(experimental_transcribe(TranscribeOptions::new(
            &model,
            FileDataContent::Bytes(vec![0xff, 0xfb]),
        )))
        .expect("transcription succeeds");

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(result.text, "Experimental transcript.");
    }
}
