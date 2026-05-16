use crate::VERSION;
use crate::file_data::FileDataContent;
use crate::generate_text::GeneratedFile;
use crate::headers::Headers;
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::provider_utils::{Base64DecodeError, detect_media_type, with_user_agent_suffix};
use crate::speech_model::{
    NoSpeechGeneratedError, SpeechModel, SpeechModelCallOptions, SpeechModelResponse,
    SpeechModelResult,
};
use crate::warning::Warning;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

/// A generated audio file returned by high-level speech generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedAudioFile {
    file: GeneratedFile,
    format: String,
}

impl GeneratedAudioFile {
    /// Creates a generated audio file from base64-encoded audio data.
    pub fn from_base64(media_type: impl Into<String>, base64: impl Into<String>) -> Self {
        Self::new(media_type, FileDataContent::Base64(base64.into()))
    }

    /// Creates a generated audio file from raw audio bytes.
    pub fn from_bytes(media_type: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(media_type, FileDataContent::Bytes(bytes.into()))
    }

    /// Creates a generated audio file from existing file-data content.
    pub fn new(media_type: impl Into<String>, data: FileDataContent) -> Self {
        let media_type = media_type.into();
        let format = audio_format_from_media_type(&media_type);

        Self {
            file: GeneratedFile::new(media_type, data),
            format,
        }
    }

    /// Returns the IANA media type of the generated audio.
    pub fn media_type(&self) -> &str {
        self.file.media_type()
    }

    /// Returns the generated audio format, e.g. `mp3` or `wav`.
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the generated audio as base64-encoded data.
    pub fn base64(&self) -> String {
        self.file.base64()
    }

    /// Returns the generated audio as raw bytes.
    pub fn bytes(&self) -> Result<Vec<u8>, Base64DecodeError> {
        self.file.bytes()
    }

    /// Upstream-named alias for [`GeneratedAudioFile::bytes`].
    pub fn uint8_array(&self) -> Result<Vec<u8>, Base64DecodeError> {
        self.bytes()
    }

    /// Returns the retained generated file representation.
    pub fn file(&self) -> &GeneratedFile {
        &self.file
    }

    /// Converts this audio file into the retained generated file representation.
    pub fn into_file(self) -> GeneratedFile {
        self.file
    }
}

impl Serialize for GeneratedAudioFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("GeneratedAudioFile", 3)?;
        state.serialize_field("base64", &self.base64())?;
        state.serialize_field("mediaType", self.media_type())?;
        state.serialize_field("format", &self.format)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for GeneratedAudioFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct GeneratedAudioFileFields {
            base64: String,
            media_type: String,
        }

        let file = GeneratedAudioFileFields::deserialize(deserializer)?;
        Ok(Self::from_base64(file.media_type, file.base64))
    }
}

/// Upstream class alias for generated audio files.
pub type DefaultGeneratedAudioFile = GeneratedAudioFile;

/// Result of a high-level `generate_speech` call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechResult {
    /// The generated audio file.
    pub audio: GeneratedAudioFile,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Response metadata from provider calls.
    pub responses: Vec<SpeechModelResponse>,

    /// Provider-specific metadata returned by the provider.
    pub provider_metadata: ProviderMetadata,
}

impl SpeechResult {
    /// Creates a high-level speech result.
    pub fn new(
        audio: GeneratedAudioFile,
        warnings: Vec<Warning>,
        responses: Vec<SpeechModelResponse>,
        provider_metadata: ProviderMetadata,
    ) -> Result<Self, NoSpeechGeneratedError> {
        if audio_data_is_empty(audio.file.data()) {
            return Err(NoSpeechGeneratedError::new(responses));
        }

        Ok(Self {
            audio,
            warnings,
            responses,
            provider_metadata,
        })
    }
}

/// Upstream-compatible experimental result alias for [`SpeechResult`].
pub type ExperimentalSpeechResult = SpeechResult;

/// Options for a high-level `generate_speech` call.
pub struct GenerateSpeechOptions<'a, M: SpeechModel + ?Sized> {
    /// Speech model used for the call.
    pub model: &'a M,

    /// Text to convert to speech.
    pub text: String,

    /// Provider-specific voice identifier.
    pub voice: Option<String>,

    /// Desired audio output format, such as `mp3` or `wav`.
    pub output_format: Option<String>,

    /// Provider instructions for speech style or delivery.
    pub instructions: Option<String>,

    /// Speech generation speed.
    pub speed: Option<f64>,

    /// Language code for speech generation, or provider-specific automatic detection.
    pub language: Option<String>,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,
}

impl<'a, M: SpeechModel + ?Sized> GenerateSpeechOptions<'a, M> {
    /// Creates options for a high-level `generate_speech` call.
    pub fn new(model: &'a M, text: impl Into<String>) -> Self {
        Self {
            model,
            text: text.into(),
            voice: None,
            output_format: None,
            instructions: None,
            speed: None,
            language: None,
            provider_options: None,
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
    pub const fn with_speed(mut self, speed: f64) -> Self {
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

/// Generates speech audio using a speech model.
pub async fn generate_speech<M: SpeechModel + ?Sized>(
    options: GenerateSpeechOptions<'_, M>,
) -> Result<SpeechResult, NoSpeechGeneratedError> {
    let GenerateSpeechOptions {
        model,
        text,
        voice,
        output_format,
        instructions,
        speed,
        language,
        provider_options,
        headers,
    } = options;

    let headers = headers_with_ai_user_agent(headers);

    let SpeechModelResult {
        audio,
        warnings,
        request: _,
        response,
        provider_metadata,
    } = model
        .do_generate(SpeechModelCallOptions {
            text,
            voice,
            output_format,
            instructions,
            speed,
            language,
            provider_options,
            headers: Some(headers),
        })
        .await;

    if audio_data_is_empty(&audio) {
        return Err(NoSpeechGeneratedError::new([response]));
    }

    let media_type = detect_media_type(&audio, Some("audio")).unwrap_or("audio/mp3");
    let audio = GeneratedAudioFile::new(media_type, audio);

    Ok(SpeechResult {
        audio,
        warnings,
        responses: vec![response],
        provider_metadata: provider_metadata.unwrap_or_default(),
    })
}

/// Upstream-compatible experimental alias for [`generate_speech`].
pub async fn experimental_generate_speech<M: SpeechModel + ?Sized>(
    options: GenerateSpeechOptions<'_, M>,
) -> Result<ExperimentalSpeechResult, NoSpeechGeneratedError> {
    generate_speech(options).await
}

fn headers_with_ai_user_agent(headers: Option<Headers>) -> Headers {
    let header_entries: Vec<(String, Option<String>)> = headers
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect();

    with_user_agent_suffix(Some(header_entries), [format!("ai/{VERSION}")])
}

fn audio_format_from_media_type(media_type: &str) -> String {
    if media_type != "audio/mpeg"
        && let Some((_, subtype)) = media_type.split_once('/')
        && !subtype.is_empty()
    {
        return subtype.to_string();
    }

    "mp3".to_string()
}

fn audio_data_is_empty(audio: &FileDataContent) -> bool {
    match audio {
        FileDataContent::Bytes(bytes) => bytes.is_empty(),
        FileDataContent::Base64(base64) => base64.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GenerateSpeechOptions, GeneratedAudioFile, SpeechResult, experimental_generate_speech,
        generate_speech,
    };
    use crate::VERSION;
    use crate::file_data::FileDataContent;
    use crate::headers::Headers;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::speech_model::{
        SpeechModel, SpeechModelCallOptions, SpeechModelResponse, SpeechModelResult,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    struct RecordingSpeechModel {
        calls: Mutex<Vec<SpeechModelCallOptions>>,
        results: Mutex<VecDeque<SpeechModelResult>>,
    }

    impl RecordingSpeechModel {
        fn new(results: Vec<SpeechModelResult>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results.into()),
            }
        }

        fn calls(&self) -> Vec<SpeechModelCallOptions> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .clone()
        }
    }

    impl SpeechModel for RecordingSpeechModel {
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

        fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
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
                        SpeechModelResult::new(
                            FileDataContent::Bytes(vec![0xff, 0xfb]),
                            speech_response("fallback"),
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

    fn speech_response(model_id: &str) -> SpeechModelResponse {
        SpeechModelResponse::new(
            OffsetDateTime::parse("2024-01-02T03:04:05Z", &Rfc3339).expect("timestamp parses"),
            model_id,
        )
    }

    #[test]
    fn generated_audio_file_serializes_upstream_shape_and_infers_format() {
        let mpeg_audio = GeneratedAudioFile::from_base64("audio/mpeg", "SUQzBAAAAAAA");

        assert_eq!(mpeg_audio.media_type(), "audio/mpeg");
        assert_eq!(mpeg_audio.format(), "mp3");
        assert_eq!(
            serde_json::to_value(mpeg_audio).expect("audio file serializes"),
            json!({
                "base64": "SUQzBAAAAAAA",
                "mediaType": "audio/mpeg",
                "format": "mp3"
            })
        );

        let wav_audio = GeneratedAudioFile::from_base64("audio/wav", "UklGRgAAAAA=");
        assert_eq!(wav_audio.format(), "wav");
    }

    #[test]
    fn generated_audio_file_deserializes_from_base64_media_type_shape() {
        let audio: GeneratedAudioFile = serde_json::from_value(json!({
            "base64": "UklGRgAAAAA=",
            "mediaType": "audio/wav",
            "format": "wav"
        }))
        .expect("audio file deserializes");

        assert_eq!(audio.media_type(), "audio/wav");
        assert_eq!(audio.format(), "wav");
        assert_eq!(audio.base64(), "UklGRgAAAAA=");
    }

    #[test]
    fn speech_result_serializes_upstream_shape() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "audioId": "aud_123"
            }
        }))
        .expect("provider metadata deserializes");

        let result = SpeechResult::new(
            GeneratedAudioFile::from_base64("audio/mpeg", "SUQzBAAAAAAA"),
            vec![Warning::Other {
                message: "setting ignored".to_string(),
            }],
            vec![speech_response("openai/tts-1").with_header("x-request-id", "req_123")],
            provider_metadata,
        )
        .expect("result has audio");

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "audio": {
                    "base64": "SUQzBAAAAAAA",
                    "mediaType": "audio/mpeg",
                    "format": "mp3"
                },
                "warnings": [
                    {
                        "type": "other",
                        "message": "setting ignored"
                    }
                ],
                "responses": [
                    {
                        "timestamp": "2024-01-02T03:04:05Z",
                        "modelId": "openai/tts-1",
                        "headers": {
                            "x-request-id": "req_123"
                        }
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "audioId": "aud_123"
                    }
                }
            })
        );
    }

    #[test]
    fn speech_result_deserializes_minimal_response_and_empty_metadata() {
        let result: SpeechResult = serde_json::from_value(json!({
            "audio": {
                "base64": "UklGRgAAAAA=",
                "mediaType": "audio/wav",
                "format": "wav"
            },
            "warnings": [],
            "responses": [
                {
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "speech-test"
                }
            ],
            "providerMetadata": {}
        }))
        .expect("result deserializes");

        assert_eq!(result.audio.format(), "wav");
        assert_eq!(result.warnings, Vec::<Warning>::new());
        assert_eq!(result.provider_metadata, ProviderMetadata::new());
        assert_eq!(result.responses, vec![speech_response("speech-test")]);
    }

    #[test]
    fn generate_speech_forwards_options_headers_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "stability": 0.8
            }
        }))
        .expect("provider options deserialize");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "audioId": "aud_456"
            }
        }))
        .expect("provider metadata deserializes");

        let model = RecordingSpeechModel::new(vec![
            SpeechModelResult::new(
                FileDataContent::Bytes(vec![0xff, 0xfb]),
                speech_response("speech-test").with_header("x-response-id", "res_123"),
            )
            .with_warning(Warning::Unsupported {
                feature: "voice".to_string(),
                details: None,
            })
            .with_provider_metadata(provider_metadata.clone()),
        ]);

        let result = poll_ready(generate_speech(
            GenerateSpeechOptions::new(&model, "Hello from Rust.")
                .with_voice("alloy")
                .with_output_format("mp3")
                .with_instructions("Speak clearly.")
                .with_speed(1.25)
                .with_language("en")
                .with_provider_options(provider_options.clone())
                .with_header("custom-request-header", "request-header-value"),
        ))
        .expect("speech generates");

        let mut expected_headers = Headers::new();
        expected_headers.insert(
            "custom-request-header".to_string(),
            "request-header-value".to_string(),
        );
        expected_headers.insert("user-agent".to_string(), format!("ai/{VERSION}"));

        assert_eq!(
            model.calls(),
            vec![SpeechModelCallOptions {
                text: "Hello from Rust.".to_string(),
                voice: Some("alloy".to_string()),
                output_format: Some("mp3".to_string()),
                instructions: Some("Speak clearly.".to_string()),
                speed: Some(1.25),
                language: Some("en".to_string()),
                provider_options: Some(provider_options),
                headers: Some(expected_headers),
            }]
        );
        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(result.audio.media_type(), "audio/mpeg");
        assert_eq!(result.audio.format(), "mp3");
        assert_eq!(
            result.audio.uint8_array().expect("audio bytes decode"),
            vec![0xff, 0xfb]
        );
        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "voice".to_string(),
                details: None,
            }]
        );
        assert_eq!(
            result.responses,
            vec![speech_response("speech-test").with_header("x-response-id", "res_123")]
        );
        assert_eq!(result.provider_metadata, provider_metadata);
    }

    #[test]
    fn generate_speech_uses_default_audio_media_type_when_detection_fails() {
        let model = RecordingSpeechModel::new(vec![SpeechModelResult::new(
            FileDataContent::Bytes(vec![1, 2, 3]),
            speech_response("speech-test"),
        )]);

        let result = poll_ready(generate_speech(GenerateSpeechOptions::new(
            &model,
            "Hello from Rust.",
        )))
        .expect("speech generates");

        assert_eq!(result.audio.media_type(), "audio/mp3");
        assert_eq!(result.audio.format(), "mp3");
    }

    #[test]
    fn generate_speech_errors_when_provider_returns_empty_audio() {
        let response = speech_response("speech-test").with_header("x-response-id", "res_empty");
        let model = RecordingSpeechModel::new(vec![SpeechModelResult::new(
            FileDataContent::Bytes(Vec::new()),
            response.clone(),
        )]);

        let error = poll_ready(generate_speech(GenerateSpeechOptions::new(
            &model,
            "Hello from Rust.",
        )))
        .expect_err("empty audio errors");

        assert_eq!(error.message(), "No speech audio generated.");
        assert_eq!(error.responses(), &[response]);
    }

    #[test]
    fn experimental_generate_speech_aliases_generate_speech() {
        let model = RecordingSpeechModel::new(vec![SpeechModelResult::new(
            FileDataContent::Bytes(vec![0xff, 0xfb]),
            speech_response("speech-test"),
        )]);

        let result = poll_ready(experimental_generate_speech(GenerateSpeechOptions::new(
            &model,
            "Hello from Rust.",
        )))
        .expect("speech generates");

        assert_eq!(result.audio.media_type(), "audio/mpeg");
    }
}
