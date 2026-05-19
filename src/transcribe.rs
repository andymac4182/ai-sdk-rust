use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::VERSION;
use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::language_model::ProviderAbortSignal;
use crate::provider::ProviderMetadata;
use crate::provider::ProviderOptions;
use crate::provider_utils::{
    DownloadError, DownloadedBlob, convert_base64_to_bytes, detect_media_type,
    with_user_agent_suffix,
};
use crate::transcription_model::{
    NoTranscriptGeneratedError, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelResponseMetadata, TranscriptionModelResult, TranscriptionModelSegment,
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
    pub responses: Vec<TranscriptionModelResponseMetadata>,

    /// Provider-specific metadata returned by the provider.
    pub provider_metadata: ProviderMetadata,
}

impl TranscriptionResult {
    /// Creates a high-level transcription result.
    pub fn new<R, I>(
        text: impl Into<String>,
        segments: Vec<TranscriptionModelSegment>,
        warnings: Vec<Warning>,
        responses: I,
        provider_metadata: ProviderMetadata,
    ) -> Result<Self, NoTranscriptGeneratedError>
    where
        R: Into<TranscriptionModelResponseMetadata>,
        I: IntoIterator<Item = R>,
    {
        let text = text.into();
        let responses = responses.into_iter().map(Into::into).collect::<Vec<_>>();

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

/// Audio input accepted by high-level transcription.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum TranscribeAudio {
    /// Inline audio data.
    Data { data: FileDataContent },

    /// URL audio that must be downloaded before the model call.
    Url { url: Url },
}

impl TranscribeAudio {
    /// Creates inline transcription audio.
    pub fn data(data: FileDataContent) -> Self {
        Self::Data { data }
    }

    /// Creates URL transcription audio.
    pub fn url(url: Url) -> Self {
        Self::Url { url }
    }
}

impl From<FileDataContent> for TranscribeAudio {
    fn from(data: FileDataContent) -> Self {
        Self::data(data)
    }
}

impl From<Url> for TranscribeAudio {
    fn from(url: Url) -> Self {
        Self::url(url)
    }
}

/// Future returned by a transcription download function.
pub type TranscribeDownloadFuture =
    Pin<Box<dyn Future<Output = Result<DownloadedBlob, DownloadError>> + Send>>;

/// Function used to download URL audio before transcription.
pub type TranscribeDownloadFunction =
    dyn Fn(Url) -> TranscribeDownloadFuture + Send + Sync + 'static;

/// Runtime download callback used for URL audio.
#[derive(Clone)]
pub struct TranscribeDownload {
    download: Arc<TranscribeDownloadFunction>,
}

impl TranscribeDownload {
    /// Creates a URL-audio download callback.
    pub fn new<F, Fut>(download: F) -> Self
    where
        F: Fn(Url) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<DownloadedBlob, DownloadError>> + Send + 'static,
    {
        Self {
            download: Arc::new(move |url| Box::pin(download(url))),
        }
    }

    /// Downloads URL audio.
    pub fn download(&self, url: Url) -> TranscribeDownloadFuture {
        (self.download)(url)
    }
}

impl fmt::Debug for TranscribeDownload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TranscribeDownload")
            .finish_non_exhaustive()
    }
}

/// Error returned by high-level transcription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscribeError {
    /// The model produced no transcript text.
    NoTranscriptGenerated(NoTranscriptGeneratedError),

    /// URL audio could not be downloaded.
    Download(DownloadError),
}

impl TranscribeError {
    /// Returns the inner no-transcript error, when present.
    pub fn as_no_transcript_generated(&self) -> Option<&NoTranscriptGeneratedError> {
        match self {
            Self::NoTranscriptGenerated(error) => Some(error),
            Self::Download(_) => None,
        }
    }

    /// Returns the inner download error, when present.
    pub fn as_download_error(&self) -> Option<&DownloadError> {
        match self {
            Self::NoTranscriptGenerated(_) => None,
            Self::Download(error) => Some(error),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        match self {
            Self::NoTranscriptGenerated(error) => error.message(),
            Self::Download(error) => error.message(),
        }
    }
}

impl fmt::Display for TranscribeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTranscriptGenerated(error) => error.fmt(formatter),
            Self::Download(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TranscribeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoTranscriptGenerated(error) => Some(error),
            Self::Download(error) => Some(error),
        }
    }
}

impl From<NoTranscriptGeneratedError> for TranscribeError {
    fn from(error: NoTranscriptGeneratedError) -> Self {
        Self::NoTranscriptGenerated(error)
    }
}

impl From<DownloadError> for TranscribeError {
    fn from(error: DownloadError) -> Self {
        Self::Download(error)
    }
}

/// Options for a high-level `transcribe` call.
pub struct TranscribeOptions<'a, M: TranscriptionModel + ?Sized> {
    /// Transcription model used for the call.
    pub model: &'a M,

    /// Audio data to transcribe.
    pub audio: TranscribeAudio,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Abort signal for cancelling the transcription call.
    pub abort_signal: Option<ProviderAbortSignal>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,

    /// Download function used when audio is supplied as a URL.
    pub download: Option<TranscribeDownload>,
}

impl<'a, M: TranscriptionModel + ?Sized> TranscribeOptions<'a, M> {
    /// Creates options for a high-level `transcribe` call.
    pub fn new(model: &'a M, audio: impl Into<TranscribeAudio>) -> Self {
        Self {
            model,
            audio: audio.into(),
            provider_options: None,
            abort_signal: None,
            headers: None,
            download: None,
        }
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets the abort signal for the transcription call.
    pub fn with_abort_signal(mut self, abort_signal: ProviderAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
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

    /// Sets the download callback for URL audio.
    pub fn with_download(mut self, download: TranscribeDownload) -> Self {
        self.download = Some(download);
        self
    }
}

/// Generates a transcript using a transcription model.
pub async fn transcribe<M: TranscriptionModel + ?Sized>(
    options: TranscribeOptions<'_, M>,
) -> Result<TranscriptionResult, TranscribeError> {
    let TranscribeOptions {
        model,
        audio,
        provider_options,
        abort_signal,
        headers,
        download,
    } = options;

    let audio = resolve_audio_data(audio, download.as_ref()).await?;
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
            abort_signal,
            headers: Some(headers),
        })
        .await;

    if text.is_empty() {
        return Err(NoTranscriptGeneratedError::new([response]).into());
    }

    Ok(TranscriptionResult {
        text,
        segments,
        language,
        duration_in_seconds,
        warnings,
        responses: vec![response.into()],
        provider_metadata: provider_metadata.unwrap_or_default(),
    })
}

/// Upstream-compatible experimental alias for [`transcribe`].
pub async fn experimental_transcribe<M: TranscriptionModel + ?Sized>(
    options: TranscribeOptions<'_, M>,
) -> Result<ExperimentalTranscriptionResult, TranscribeError> {
    transcribe(options).await
}

async fn resolve_audio_data(
    audio: TranscribeAudio,
    download: Option<&TranscribeDownload>,
) -> Result<FileDataContent, TranscribeError> {
    match audio {
        TranscribeAudio::Data { data } => Ok(normalize_audio_data(data)),
        TranscribeAudio::Url { url } => {
            let Some(download) = download else {
                return Err(DownloadError::new(
                    url.to_string(),
                    "URL audio requires a download function",
                )
                .into());
            };

            let blob = download.download(url).await?;
            Ok(FileDataContent::Bytes(blob.data))
        }
    }
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
        ExperimentalTranscriptionResult, TranscribeAudio, TranscribeDownload, TranscribeOptions,
        TranscriptionResult, experimental_transcribe, transcribe,
    };
    use crate::ProviderAbortController;
    use crate::VERSION;
    use crate::file_data::FileDataContent;
    use crate::headers::Headers;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::provider_utils::{DownloadError, DownloadedBlob};
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
        TranscriptionModelResponseMetadata, TranscriptionModelResult, TranscriptionModelSegment,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    use url::Url;

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

    fn transcription_response_metadata(model_id: &str) -> TranscriptionModelResponseMetadata {
        TranscriptionModelResponseMetadata::from_response(transcription_response(model_id))
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
                vec![transcription_response_metadata("transcribe-test")],
                ProviderMetadata::new(),
            )
            .expect("transcript is present")
        );
    }

    #[test]
    fn transcription_result_drops_provider_response_body_from_metadata() {
        let result = TranscriptionResult::new(
            "Hello.",
            Vec::new(),
            Vec::new(),
            vec![
                transcription_response("transcribe-test")
                    .with_header("x-request-id", "req_123")
                    .with_body(json!({
                        "text": "Hello."
                    })),
            ],
            ProviderMetadata::new(),
        )
        .expect("transcript is present");

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "text": "Hello.",
                "segments": [],
                "warnings": [],
                "responses": [
                    {
                        "timestamp": "2024-01-02T03:04:05Z",
                        "modelId": "transcribe-test",
                        "headers": {
                            "x-request-id": "req_123"
                        }
                    }
                ],
                "providerMetadata": {}
            })
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
        let expected_metadata = TranscriptionModelResponseMetadata::from_response(response);

        assert_eq!(error.message(), "No transcript generated.");
        assert_eq!(error.responses(), &[expected_metadata]);
    }

    #[test]
    fn experimental_transcription_result_alias_preserves_shape() {
        let result: ExperimentalTranscriptionResult = TranscriptionResult::new(
            "Hello.",
            Vec::new(),
            Vec::new(),
            vec![transcription_response_metadata("transcribe-test")],
            ProviderMetadata::new(),
        )
        .expect("transcript is present");

        assert_eq!(result.text, "Hello.");
    }

    #[test]
    fn transcribe_audio_serializes_upstream_file_data_variants() {
        let url = Url::parse("https://example.com/audio.wav").expect("url parses");

        assert_eq!(
            serde_json::to_value(TranscribeAudio::data(FileDataContent::Base64(
                "UklGRgAAAAA=".to_string()
            )))
            .expect("audio serializes"),
            json!({
                "type": "data",
                "data": "UklGRgAAAAA="
            })
        );
        assert_eq!(
            serde_json::to_value(TranscribeAudio::url(url)).expect("url audio serializes"),
            json!({
                "type": "url",
                "url": "https://example.com/audio.wav"
            })
        );
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
                abort_signal: None,
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
    fn transcribe_forwards_abort_signal_to_model_call() {
        let abort_controller = ProviderAbortController::new();
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "Hello world.",
            Vec::new(),
            transcription_response("openai/whisper-1"),
        )]);

        let result = poll_ready(transcribe(
            TranscribeOptions::new(&model, FileDataContent::Base64("//s=".to_string()))
                .with_abort_signal(abort_controller.signal()),
        ))
        .expect("transcription succeeds");

        assert_eq!(result.text, "Hello world.");
        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        let call_signal = calls[0].abort_signal.clone().expect("abort signal set");
        assert!(!call_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(call_signal.is_aborted());
        assert_eq!(call_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn transcribe_downloads_url_audio_before_model_call() {
        let downloaded_urls = Arc::new(Mutex::new(Vec::new()));
        let download = TranscribeDownload::new({
            let downloaded_urls = Arc::clone(&downloaded_urls);
            move |url| {
                downloaded_urls
                    .lock()
                    .expect("download urls lock is not poisoned")
                    .push(url);

                ready(Ok(
                    DownloadedBlob::new(vec![0xff, 0xfb]).with_media_type("audio/ogg")
                ))
            }
        });
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "Downloaded transcript.",
            Vec::new(),
            transcription_response("openai/whisper-1"),
        )]);
        let url = Url::parse("https://example.com/audio.mp3").expect("url parses");

        let result = poll_ready(transcribe(
            TranscribeOptions::new(&model, url.clone()).with_download(download),
        ))
        .expect("transcription succeeds");

        assert_eq!(result.text, "Downloaded transcript.");
        assert_eq!(
            *downloaded_urls
                .lock()
                .expect("download urls lock is not poisoned"),
            vec![url]
        );
        assert_eq!(
            model.calls(),
            vec![TranscriptionModelCallOptions {
                audio: FileDataContent::Bytes(vec![0xff, 0xfb]),
                media_type: "audio/mpeg".to_string(),
                provider_options: Some(ProviderOptions::new()),
                abort_signal: None,
                headers: Some({
                    let mut headers = Headers::new();
                    headers.insert("user-agent".to_string(), format!("ai/{VERSION}"));
                    headers
                }),
            }]
        );
    }

    #[test]
    fn transcribe_url_audio_requires_download_callback() {
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "unreachable",
            Vec::new(),
            transcription_response("transcribe-test"),
        )]);
        let url = Url::parse("https://example.com/audio.mp3").expect("url parses");

        let error = poll_ready(transcribe(TranscribeOptions::new(&model, url.clone())))
            .expect_err("missing download function errors");

        let download_error = error
            .as_download_error()
            .expect("error is a download failure");
        assert_eq!(download_error.url(), url.as_str());
        assert_eq!(
            download_error.message(),
            "URL audio requires a download function"
        );
        assert_eq!(model.calls(), Vec::new());
    }

    #[test]
    fn transcribe_propagates_url_download_errors() {
        let download = TranscribeDownload::new(|url| {
            ready(Err(DownloadError::with_cause_message(
                url.to_string(),
                "network down",
            )))
        });
        let model = RecordingTranscriptionModel::new(vec![TranscriptionModelResult::new(
            "unreachable",
            Vec::new(),
            transcription_response("transcribe-test"),
        )]);
        let url = Url::parse("https://example.com/audio.mp3").expect("url parses");

        let error = poll_ready(transcribe(
            TranscribeOptions::new(&model, url.clone()).with_download(download),
        ))
        .expect_err("download failure errors");

        let download_error = error
            .as_download_error()
            .expect("error is a download failure");
        assert_eq!(download_error.url(), url.as_str());
        assert_eq!(
            download_error.message(),
            "Failed to download https://example.com/audio.mp3: network down"
        );
        assert_eq!(model.calls(), Vec::new());
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
                abort_signal: None,
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
            vec![
                transcription_response_metadata("transcribe-test")
                    .with_header("x-request-id", "req_123")
            ]
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

        let no_transcript = error
            .as_no_transcript_generated()
            .expect("error is no-transcript");
        let expected_metadata = TranscriptionModelResponseMetadata::from_response(response);
        assert_eq!(no_transcript.message(), "No transcript generated.");
        assert_eq!(no_transcript.responses(), &[expected_metadata]);
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
