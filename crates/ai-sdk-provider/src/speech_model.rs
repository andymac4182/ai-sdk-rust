use serde::{Deserialize, Serialize};
use std::{fmt, future::Future};
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::ProviderAbortSignal;
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

/// A provider-v4 speech model.
///
/// The upstream TypeScript contract exposes a `doGenerate` method returning a
/// `PromiseLike<SpeechModelV4Result>`. This Rust trait maps that boundary to an
/// associated [`Future`] without introducing an async-trait dependency.
pub trait SpeechModel {
    /// Future returned by [`SpeechModel::do_generate`].
    type GenerateFuture<'a>: Future<Output = SpeechModelResult> + Send + 'a
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

    /// Generates speech audio from the supplied text options.
    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_>;
}

/// Generated speech audio returned by a speech model.
pub type SpeechModelAudio = FileDataContent;

/// Options passed to a speech model provider call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelCallOptions {
    /// Text to convert to speech.
    pub text: String,

    /// Provider-specific voice identifier to use for speech synthesis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,

    /// Desired audio output format, such as `mp3` or `wav`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,

    /// Provider instructions for the generated speech style or delivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    /// Speech generation speed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,

    /// Language code for speech generation, or provider-specific automatic detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Abort signal for cancelling the operation.
    #[serde(default, skip)]
    pub abort_signal: Option<ProviderAbortSignal>,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl SpeechModelCallOptions {
    /// Creates speech model call options with the required input text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            voice: None,
            output_format: None,
            instructions: None,
            speed: None,
            language: None,
            provider_options: None,
            abort_signal: None,
            headers: None,
        }
    }

    /// Sets the provider-specific voice identifier.
    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    /// Sets the desired audio output format.
    pub fn with_output_format(mut self, output_format: impl Into<String>) -> Self {
        self.output_format = Some(output_format.into());
        self
    }

    /// Sets provider instructions for speech generation.
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Sets the speech generation speed.
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = Some(speed);
        self
    }

    /// Sets the language code for speech generation.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets the abort signal for the speech generation call.
    pub fn with_abort_signal(mut self, abort_signal: ProviderAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
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

/// Optional request information for telemetry and debugging speech calls.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelRequest {
    /// Raw request HTTP body that was sent to the provider API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl SpeechModelRequest {
    /// Creates empty request metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the raw provider request body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }
}

/// Response information for telemetry and debugging speech calls.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelResponse {
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

impl SpeechModelResponse {
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

/// High-level speech response metadata.
///
/// Upstream `SpeechModelResponseMetadata` uses the same JSON shape as the
/// provider-v4 speech response, including the optional provider response body.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelResponseMetadata {
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

impl SpeechModelResponseMetadata {
    /// Creates high-level speech response metadata.
    pub fn new(timestamp: OffsetDateTime, model_id: impl Into<String>) -> Self {
        Self {
            timestamp,
            model_id: model_id.into(),
            headers: None,
            body: None,
        }
    }

    /// Creates high-level metadata from a provider-v4 speech response.
    pub fn from_response(response: SpeechModelResponse) -> Self {
        Self {
            timestamp: response.timestamp,
            model_id: response.model_id,
            headers: response.headers,
            body: response.body,
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

impl From<SpeechModelResponse> for SpeechModelResponseMetadata {
    fn from(response: SpeechModelResponse) -> Self {
        Self::from_response(response)
    }
}

/// Error returned when high-level speech generation produces no audio.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoSpeechGeneratedError {
    responses: Vec<SpeechModelResponseMetadata>,
}

impl NoSpeechGeneratedError {
    /// Creates a no-speech error with response metadata from attempted provider calls.
    pub fn new<R, I>(responses: I) -> Self
    where
        R: Into<SpeechModelResponseMetadata>,
        I: IntoIterator<Item = R>,
    {
        Self {
            responses: responses.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns the upstream human-readable error message.
    pub fn message(&self) -> &'static str {
        "No speech audio generated."
    }

    /// Returns response metadata for attempted provider calls.
    pub fn responses(&self) -> &[SpeechModelResponseMetadata] {
        &self.responses
    }

    /// Converts this error into its response metadata.
    pub fn into_responses(self) -> Vec<SpeechModelResponseMetadata> {
        self.responses
    }
}

impl fmt::Display for NoSpeechGeneratedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for NoSpeechGeneratedError {}

/// Result of a speech model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelResult {
    /// Generated audio as base64-encoded audio or raw bytes.
    pub audio: SpeechModelAudio,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Optional request information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<SpeechModelRequest>,

    /// Response information for telemetry and debugging.
    pub response: SpeechModelResponse,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl SpeechModelResult {
    /// Creates a speech result with no warnings.
    pub fn new(audio: SpeechModelAudio, response: SpeechModelResponse) -> Self {
        Self {
            audio,
            warnings: Vec::new(),
            request: None,
            response,
            provider_metadata: None,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Sets optional request information.
    pub fn with_request(mut self, request: SpeechModelRequest) -> Self {
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
        NoSpeechGeneratedError, SpeechModel, SpeechModelCallOptions, SpeechModelRequest,
        SpeechModelResponse, SpeechModelResponseMetadata, SpeechModelResult,
    };
    use crate::file_data::FileDataContent;
    use crate::language_model::ProviderAbortController;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    struct StaticSpeechModel;

    impl SpeechModel for StaticSpeechModel {
        type GenerateFuture<'a>
            = Ready<SpeechModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "speech-test"
        }

        fn do_generate(&self, _options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
            let timestamp =
                OffsetDateTime::parse("2024-01-02T03:04:05Z", &Rfc3339).expect("timestamp parses");

            ready(SpeechModelResult::new(
                FileDataContent::Base64("SUQzBAAAAAAA".to_string()),
                SpeechModelResponse::new(timestamp, self.model_id()),
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
    fn call_options_serializes_upstream_shape_with_speech_settings_and_headers() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "stability": 0.8
            }
        }))
        .expect("provider options deserialize");

        let options = SpeechModelCallOptions::new("Hello from Rust.")
            .with_voice("alloy")
            .with_output_format("mp3")
            .with_instructions("Speak clearly and warmly.")
            .with_speed(1.25)
            .with_language("en")
            .with_provider_options(provider_options)
            .with_header("x-request-id", "req_123");

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "text": "Hello from Rust.",
                "voice": "alloy",
                "outputFormat": "mp3",
                "instructions": "Speak clearly and warmly.",
                "speed": 1.25,
                "language": "en",
                "providerOptions": {
                    "openai": {
                        "stability": 0.8
                    }
                },
                "headers": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn call_options_carries_abort_signal_without_serializing_it() {
        let abort_controller = ProviderAbortController::new();
        let options =
            SpeechModelCallOptions::new("Hello.").with_abort_signal(abort_controller.signal());

        assert!(
            options
                .abort_signal
                .as_ref()
                .is_some_and(|signal| !signal.is_aborted())
        );
        assert_eq!(
            serde_json::to_value(&options).expect("call options serialize"),
            json!({
                "text": "Hello."
            })
        );

        let cloned_signal = options.abort_signal.clone().expect("abort signal set");
        abort_controller.abort_with_reason("manual abort");
        assert!(cloned_signal.is_aborted());
        assert_eq!(cloned_signal.reason(), Some(json!("manual abort")));
    }

    #[test]
    fn call_options_deserializes_minimal_text_and_omits_optional_fields() {
        let options: SpeechModelCallOptions = serde_json::from_value(json!({
            "text": "Hello."
        }))
        .expect("call options deserialize");

        assert_eq!(options, SpeechModelCallOptions::new("Hello."));
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "text": "Hello."
            })
        );
    }

    #[test]
    fn speech_model_trait_exposes_upstream_v4_identity_and_generate_boundary() {
        let model = StaticSpeechModel;
        let options = SpeechModelCallOptions::new("Hello from Rust.").with_voice("alloy");

        let result = poll_ready(model.do_generate(options));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "speech-test");
        assert_eq!(
            result.audio,
            FileDataContent::Base64("SUQzBAAAAAAA".to_string())
        );
        assert_eq!(result.response.model_id, "speech-test");
    }

    #[test]
    fn result_serializes_audio_response_metadata_and_warnings() {
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T10:00:00Z", &Rfc3339).expect("timestamp parses");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "audioId": "aud_123"
            }
        }))
        .expect("provider metadata deserializes");

        let result = SpeechModelResult::new(
            FileDataContent::Base64("SUQzBAAAAAAA".to_string()),
            SpeechModelResponse::new(response_timestamp, "openai/tts-1")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "duration": 1.3
                })),
        )
        .with_warning(Warning::Unsupported {
            feature: "speed".to_string(),
            details: None,
        })
        .with_request(SpeechModelRequest::new().with_body(json!({
            "model": "tts-1",
            "voice": "alloy"
        })))
        .with_provider_metadata(provider_metadata);

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "audio": "SUQzBAAAAAAA",
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "speed"
                    }
                ],
                "request": {
                    "body": {
                        "model": "tts-1",
                        "voice": "alloy"
                    }
                },
                "response": {
                    "timestamp": "2026-05-16T10:00:00Z",
                    "modelId": "openai/tts-1",
                    "headers": {
                        "x-request-id": "req_123"
                    },
                    "body": {
                        "duration": 1.3
                    }
                },
                "providerMetadata": {
                    "openai": {
                        "audioId": "aud_123"
                    }
                }
            })
        );
    }

    #[test]
    fn result_deserializes_raw_audio_bytes_and_empty_warnings() {
        let result: SpeechModelResult = serde_json::from_value(json!({
            "audio": [73, 68, 51],
            "warnings": [],
            "response": {
                "timestamp": "2026-05-16T10:00:00Z",
                "modelId": "provider/tts"
            }
        }))
        .expect("result deserializes");

        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T10:00:00Z", &Rfc3339).expect("timestamp parses");

        assert_eq!(
            result,
            SpeechModelResult::new(
                FileDataContent::Bytes(vec![73, 68, 51]),
                SpeechModelResponse::new(response_timestamp, "provider/tts"),
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("result serialize"),
            json!({
                "audio": [73, 68, 51],
                "warnings": [],
                "response": {
                    "timestamp": "2026-05-16T10:00:00Z",
                    "modelId": "provider/tts"
                }
            })
        );
    }

    #[test]
    fn result_requires_warnings_and_response_metadata() {
        let missing_warnings = serde_json::from_value::<SpeechModelResult>(json!({
            "audio": "SUQzBAAAAAAA",
            "response": {
                "timestamp": "2026-05-16T10:00:00Z",
                "modelId": "provider/tts"
            }
        }))
        .expect_err("warnings are required");

        assert!(
            missing_warnings
                .to_string()
                .contains("missing field `warnings`")
        );

        let missing_response = serde_json::from_value::<SpeechModelResult>(json!({
            "audio": "SUQzBAAAAAAA",
            "warnings": []
        }))
        .expect_err("response metadata is required");

        assert!(
            missing_response
                .to_string()
                .contains("missing field `response`")
        );
    }

    #[test]
    fn response_metadata_serializes_upstream_shape_with_body() {
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T10:00:00Z", &Rfc3339).expect("timestamp parses");
        let metadata = SpeechModelResponseMetadata::new(response_timestamp, "openai/tts-1")
            .with_header("x-request-id", "req_123")
            .with_body(json!({
                "duration": 1.3
            }));

        assert_eq!(
            serde_json::to_value(metadata).expect("metadata serializes"),
            json!({
                "timestamp": "2026-05-16T10:00:00Z",
                "modelId": "openai/tts-1",
                "headers": {
                    "x-request-id": "req_123"
                },
                "body": {
                    "duration": 1.3
                }
            })
        );
    }

    #[test]
    fn response_metadata_converts_from_provider_response_without_dropping_body() {
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T10:00:00Z", &Rfc3339).expect("timestamp parses");
        let response = SpeechModelResponse::new(response_timestamp, "openai/tts-1")
            .with_header("x-request-id", "req_123")
            .with_body(json!({
                "duration": 1.3
            }));

        assert_eq!(
            SpeechModelResponseMetadata::from_response(response),
            SpeechModelResponseMetadata::new(response_timestamp, "openai/tts-1")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "duration": 1.3
                }))
        );
    }

    #[test]
    fn no_speech_generated_error_matches_upstream_message() {
        let error = NoSpeechGeneratedError::new(Vec::<SpeechModelResponseMetadata>::new());

        assert_eq!(error.message(), "No speech audio generated.");
        assert_eq!(error.to_string(), "No speech audio generated.");
        assert!(error.responses().is_empty());
    }

    #[test]
    fn no_speech_generated_error_retains_response_metadata() {
        let response_timestamp =
            OffsetDateTime::parse("2026-05-16T10:00:00Z", &Rfc3339).expect("timestamp parses");
        let response = SpeechModelResponse::new(response_timestamp, "openai/tts-1")
            .with_header("x-request-id", "req_123")
            .with_body(json!({
                "audio": []
            }));
        let metadata = SpeechModelResponseMetadata::from_response(response.clone());

        let error = NoSpeechGeneratedError::new([response.clone()]);

        assert_eq!(error.responses(), std::slice::from_ref(&metadata));
        assert_eq!(error.into_responses(), vec![metadata]);
    }
}
