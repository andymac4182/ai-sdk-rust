use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, HandledFetchError, Headers, JsonObject, JsonValue,
    LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    PostJsonToApiOptions, PostToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderWithSpeechModel, ProviderWithTranscriptionModel, RuntimeEnvironment,
    SpeechModel, SpeechModelCallOptions, SpeechModelRequest, SpeechModelResponse,
    SpeechModelResult, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelResponse, TranscriptionModelResult, TranscriptionModelSegment, Warning,
    combine_headers, convert_base64_to_bytes, create_binary_response_handler,
    create_json_error_response_handler, create_json_response_handler, load_api_key,
    media_type_to_extension, parse_provider_options, post_json_to_api, post_to_api,
    with_user_agent_suffix,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/elevenlabs` API calls.
pub const DEFAULT_ELEVENLABS_BASE_URL: &str = "https://api.elevenlabs.io";

/// Default ElevenLabs voice used by upstream when a speech call omits `voice`.
pub const DEFAULT_ELEVENLABS_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM";

/// Provider-specific ElevenLabs speech options.
pub type ElevenLabsSpeechModelOptions = JsonObject;

/// Provider-specific ElevenLabs transcription options.
pub type ElevenLabsTranscriptionModelOptions = JsonObject;

/// Settings for the upstream ElevenLabs provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevenLabsProviderSettings {
    /// ElevenLabs API key. When omitted, `ELEVENLABS_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl ElevenLabsProviderSettings {
    /// Creates empty ElevenLabs provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the ElevenLabs API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Upstream ElevenLabs provider foundation.
#[derive(Clone)]
pub struct ElevenLabsProvider {
    settings: ElevenLabsProviderSettings,
    transport: ElevenLabsTransport,
    current_date: ElevenLabsDateProvider,
}

/// ElevenLabs speech model for `/v1/text-to-speech/{voiceId}` calls.
#[derive(Clone)]
pub struct ElevenLabsSpeechModel {
    model_id: String,
    settings: ElevenLabsProviderSettings,
    transport: ElevenLabsTransport,
    current_date: ElevenLabsDateProvider,
}

/// ElevenLabs transcription model for `/v1/speech-to-text` calls.
#[derive(Clone)]
pub struct ElevenLabsTranscriptionModel {
    model_id: String,
    settings: ElevenLabsProviderSettings,
    transport: ElevenLabsTransport,
    current_date: ElevenLabsDateProvider,
}

/// Future returned by an injected ElevenLabs HTTP transport.
pub type ElevenLabsTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by ElevenLabs provider models.
pub type ElevenLabsTransport =
    Arc<dyn Fn(ProviderApiRequest) -> ElevenLabsTransportFuture + Send + Sync>;

type ElevenLabsDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type ElevenLabsSpeechGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = SpeechModelResult> + Send + 'a>>;
type ElevenLabsTranscriptionGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>;

impl ElevenLabsProvider {
    /// Creates an ElevenLabs provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(ElevenLabsProviderSettings::new())
    }

    /// Creates a provider from explicit ElevenLabs settings.
    pub fn from_settings(settings: ElevenLabsProviderSettings) -> Self {
        Self {
            settings,
            transport: default_elevenlabs_transport(),
            current_date: default_elevenlabs_date_provider(),
        }
    }

    /// Sets the ElevenLabs API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: ElevenLabsTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Replaces the response timestamp provider. This is primarily useful for tests.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    /// Creates a transcription model.
    pub fn transcription(&self, model_id: impl Into<String>) -> ElevenLabsTranscriptionModel {
        self.transcription_model(model_id)
            .expect("ElevenLabs transcription models are supported")
    }

    /// Creates a transcription model.
    pub fn transcription_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ElevenLabsTranscriptionModel, NoSuchModelError> {
        Ok(ElevenLabsTranscriptionModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Creates a speech model.
    pub fn speech(&self, model_id: impl Into<String>) -> ElevenLabsSpeechModel {
        self.speech_model(model_id)
            .expect("ElevenLabs speech models are supported")
    }

    /// Creates a speech model.
    pub fn speech_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ElevenLabsSpeechModel, NoSuchModelError> {
        Ok(ElevenLabsSpeechModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that ElevenLabs does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "ElevenLabs does not provide language models",
        ))
    }

    /// Reports that ElevenLabs does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            "ElevenLabs does not provide embedding models",
        ))
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that ElevenLabs does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            "ElevenLabs does not provide image models",
        ))
    }
}

impl Default for ElevenLabsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for ElevenLabsProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        ElevenLabsProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        ElevenLabsProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        ElevenLabsProvider::image_model(self, model_id)
    }
}

impl ProviderWithSpeechModel for ElevenLabsProvider {
    type SpeechModel = ElevenLabsSpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        ElevenLabsProvider::speech_model(self, model_id)
    }
}

impl ProviderWithTranscriptionModel for ElevenLabsProvider {
    type TranscriptionModel = ElevenLabsTranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        ElevenLabsProvider::transcription_model(self, model_id)
    }
}

impl ElevenLabsSpeechModel {
    fn new(
        model_id: impl Into<String>,
        settings: ElevenLabsProviderSettings,
        transport: ElevenLabsTransport,
        current_date: ElevenLabsDateProvider,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            settings,
            transport,
            current_date,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "elevenlabs.speech"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ElevenLabsTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Returns a copy of this model that uses the supplied timestamp provider.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    async fn do_generate_result(&self, options: SpeechModelCallOptions) -> SpeechModelResult {
        let timestamp = (self.current_date)();
        let (request_body, query_params, warnings, voice_id) =
            match elevenlabs_speech_request(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return elevenlabs_speech_result_from_error(
                        &self.model_id,
                        error.to_string(),
                        JsonValue::Object(JsonObject::new()),
                        None,
                        None,
                        Vec::new(),
                        timestamp,
                    );
                }
            };
        let request_body_for_error = request_body.clone();
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return elevenlabs_speech_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    request_body_for_error,
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(
            with_query_params(
                elevenlabs_url(&format!("/v1/text-to-speech/{voice_id}")),
                query_params,
            ),
            request_body,
        )
        .with_headers(request_headers)
        .with_optional_abort_signal(options.abort_signal.clone())
        .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_binary_response_handler(response.binary_response_handler_options(request))
                    .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    elevenlabs_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => elevenlabs_speech_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                request_body_for_error,
                warnings,
                timestamp,
            ),
            Err(error) => elevenlabs_speech_result_from_handled_error(
                &self.model_id,
                error,
                request_body_for_error,
                warnings,
                timestamp,
            ),
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(elevenlabs_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl SpeechModel for ElevenLabsSpeechModel {
    type GenerateFuture<'a>
        = ElevenLabsSpeechGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ElevenLabsSpeechModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ElevenLabsSpeechModel::model_id(self)
    }

    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl ElevenLabsTranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        settings: ElevenLabsProviderSettings,
        transport: ElevenLabsTransport,
        current_date: ElevenLabsDateProvider,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            settings,
            transport,
            current_date,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "elevenlabs.transcription"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ElevenLabsTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Returns a copy of this model that uses the supplied timestamp provider.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    async fn do_generate_result(
        &self,
        options: TranscriptionModelCallOptions,
    ) -> TranscriptionModelResult {
        let timestamp = (self.current_date)();
        let (form_data, warnings) =
            match elevenlabs_transcription_form_data(&self.model_id, &options) {
                Ok(args) => args,
                Err(message) => {
                    return elevenlabs_transcription_result_from_error(
                        &self.model_id,
                        message,
                        None,
                        None,
                        Vec::new(),
                        timestamp,
                    );
                }
            };
        let (content_type, body) = multipart_body(&form_data, Some(&options.media_type));
        let request_body_values = form_data_request_body_values(&form_data);
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return elevenlabs_transcription_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let post_options = PostToApiOptions::new(
            elevenlabs_url("/v1/speech-to-text"),
            ProviderApiRequestBody::bytes(body),
            request_body_values,
        )
        .with_headers(request_headers)
        .with_header("content-type", content_type)
        .with_optional_abort_signal(options.abort_signal.clone())
        .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    elevenlabs_transcription_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    elevenlabs_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => elevenlabs_transcription_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                response.raw_value,
                warnings,
                timestamp,
            ),
            Err(error) => elevenlabs_transcription_result_from_handled_error(
                &self.model_id,
                error,
                warnings,
                timestamp,
            ),
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(elevenlabs_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl TranscriptionModel for ElevenLabsTranscriptionModel {
    type GenerateFuture<'a>
        = ElevenLabsTranscriptionGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ElevenLabsTranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ElevenLabsTranscriptionModel::model_id(self)
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates an ElevenLabs provider with explicit settings.
pub fn create_elevenlabs(settings: ElevenLabsProviderSettings) -> ElevenLabsProvider {
    ElevenLabsProvider::from_settings(settings)
}

/// Creates an ElevenLabs transcription model using the default provider settings.
pub fn elevenlabs(model_id: impl Into<String>) -> ElevenLabsTranscriptionModel {
    ElevenLabsProvider::new().transcription(model_id)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevenLabsTranscriptionResponse {
    language_code: String,
    text: String,
    #[serde(default)]
    words: Option<Vec<ElevenLabsTranscriptionWord>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevenLabsTranscriptionWord {
    text: String,
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevenLabsErrorData {
    error: ElevenLabsErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ElevenLabsErrorBody {
    message: String,
    code: i64,
}

type QueryParams = Vec<(String, String)>;

fn elevenlabs_speech_request(
    model_id: &str,
    options: &SpeechModelCallOptions,
) -> Result<(JsonValue, QueryParams, Vec<Warning>, String), ai_sdk_rust::InvalidArgumentError> {
    let elevenlabs_options = parse_provider_options(
        "elevenlabs",
        options.provider_options.as_ref(),
        elevenlabs_speech_model_options,
    )?;
    let mut warnings = Vec::new();
    let mut body = JsonObject::new();
    let mut query_params = Vec::new();
    let voice_id = options
        .voice
        .clone()
        .unwrap_or_else(|| DEFAULT_ELEVENLABS_VOICE_ID.to_string());

    body.insert("text".to_string(), JsonValue::String(options.text.clone()));
    body.insert(
        "model_id".to_string(),
        JsonValue::String(model_id.to_string()),
    );
    set_query_param(
        &mut query_params,
        "output_format",
        elevenlabs_output_format(options.output_format.as_deref()),
    );

    if let Some(language) = options.language.as_ref() {
        body.insert(
            "language_code".to_string(),
            JsonValue::String(language.clone()),
        );
    }

    let mut voice_settings = JsonObject::new();
    if let Some(speed) = options.speed {
        voice_settings.insert("speed".to_string(), json_number(speed));
    }

    if let Some(elevenlabs_options) = elevenlabs_options.as_ref() {
        apply_elevenlabs_speech_provider_options(
            elevenlabs_options,
            &mut body,
            &mut voice_settings,
            &mut query_params,
        );
    }

    if !voice_settings.is_empty() {
        body.insert(
            "voice_settings".to_string(),
            JsonValue::Object(voice_settings),
        );
    }

    if options.instructions.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "instructions".to_string(),
            details: Some(
                "ElevenLabs speech models do not support instructions. Instructions parameter was ignored."
                    .to_string(),
            ),
        });
    }

    Ok((JsonValue::Object(body), query_params, warnings, voice_id))
}

fn apply_elevenlabs_speech_provider_options(
    options: &ElevenLabsSpeechModelOptions,
    body: &mut JsonObject,
    voice_settings: &mut JsonObject,
    query_params: &mut QueryParams,
) {
    if let Some(settings) = options.get("voiceSettings").and_then(JsonValue::as_object) {
        insert_json_number(voice_settings, "stability", settings.get("stability"));
        insert_json_number(
            voice_settings,
            "similarity_boost",
            settings.get("similarityBoost"),
        );
        insert_json_number(voice_settings, "style", settings.get("style"));
        insert_json_bool(
            voice_settings,
            "use_speaker_boost",
            settings.get("useSpeakerBoost"),
        );
    }

    if !body.contains_key("language_code") {
        insert_json_string(body, "language_code", options.get("languageCode"));
    }

    if let Some(locators) = options
        .get("pronunciationDictionaryLocators")
        .and_then(JsonValue::as_array)
    {
        let locators = locators
            .iter()
            .filter_map(|locator| locator.as_object())
            .map(|locator| {
                let mut mapped = JsonObject::new();
                if let Some(id) = locator
                    .get("pronunciationDictionaryId")
                    .and_then(JsonValue::as_str)
                {
                    mapped.insert(
                        "pronunciation_dictionary_id".to_string(),
                        JsonValue::String(id.to_string()),
                    );
                }
                if let Some(version_id) = locator.get("versionId").and_then(JsonValue::as_str) {
                    mapped.insert(
                        "version_id".to_string(),
                        JsonValue::String(version_id.to_string()),
                    );
                }
                JsonValue::Object(mapped)
            })
            .collect::<Vec<_>>();
        body.insert(
            "pronunciation_dictionary_locators".to_string(),
            JsonValue::Array(locators),
        );
    }

    insert_json_number(body, "seed", options.get("seed"));
    insert_json_string(body, "previous_text", options.get("previousText"));
    insert_json_string(body, "next_text", options.get("nextText"));
    insert_json_string_array(
        body,
        "previous_request_ids",
        options.get("previousRequestIds"),
    );
    insert_json_string_array(body, "next_request_ids", options.get("nextRequestIds"));
    insert_json_string(
        body,
        "apply_text_normalization",
        options.get("applyTextNormalization"),
    );
    insert_json_bool(
        body,
        "apply_language_text_normalization",
        options.get("applyLanguageTextNormalization"),
    );

    if let Some(enable_logging) = options.get("enableLogging").and_then(JsonValue::as_bool) {
        set_query_param(query_params, "enable_logging", enable_logging.to_string());
    }
}

fn elevenlabs_speech_model_options(
    value: &JsonValue,
) -> Result<ElevenLabsSpeechModelOptions, String> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "ElevenLabs speech provider options must be an object".to_string())
}

fn elevenlabs_transcription_form_data(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> Result<(ai_sdk_rust::FormData, Vec<Warning>), String> {
    let audio = audio_bytes(&options.audio)?;
    let elevenlabs_options = parse_provider_options(
        "elevenlabs",
        options.provider_options.as_ref(),
        elevenlabs_transcription_model_options,
    )
    .map_err(|error| error.to_string())?;
    let mut form_data = ai_sdk_rust::FormData::new();

    form_data.append("model_id", ai_sdk_rust::FormDataValue::text(model_id));
    form_data.append("file", ai_sdk_rust::FormDataValue::bytes(audio));
    form_data.append("diarize", ai_sdk_rust::FormDataValue::text("true"));

    if let Some(options) = elevenlabs_options.as_ref() {
        apply_elevenlabs_transcription_provider_options(options, &mut form_data);
    }

    Ok((form_data, Vec::new()))
}

fn apply_elevenlabs_transcription_provider_options(
    options: &ElevenLabsTranscriptionModelOptions,
    form_data: &mut ai_sdk_rust::FormData,
) {
    if let Some(value) = json_scalar_to_string(options.get("languageCode")) {
        form_data.append("language_code", ai_sdk_rust::FormDataValue::text(value));
    }

    form_data.append(
        "tag_audio_events",
        ai_sdk_rust::FormDataValue::text(
            options
                .get("tagAudioEvents")
                .and_then(JsonValue::as_bool)
                .unwrap_or(true)
                .to_string(),
        ),
    );
    if let Some(value) = json_scalar_to_string(options.get("numSpeakers")) {
        form_data.append("num_speakers", ai_sdk_rust::FormDataValue::text(value));
    }
    form_data.append(
        "timestamps_granularity",
        ai_sdk_rust::FormDataValue::text(
            options
                .get("timestampsGranularity")
                .and_then(JsonValue::as_str)
                .unwrap_or("word"),
        ),
    );
    form_data.append(
        "diarize",
        ai_sdk_rust::FormDataValue::text(
            options
                .get("diarize")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
                .to_string(),
        ),
    );
    form_data.append(
        "file_format",
        ai_sdk_rust::FormDataValue::text(
            options
                .get("fileFormat")
                .and_then(JsonValue::as_str)
                .unwrap_or("other"),
        ),
    );
}

fn elevenlabs_transcription_model_options(
    value: &JsonValue,
) -> Result<ElevenLabsTranscriptionModelOptions, String> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "ElevenLabs transcription provider options must be an object".to_string())
}

fn elevenlabs_provider_header_entries(
    settings: &ElevenLabsProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "xi-api-key".to_string(),
        Some(elevenlabs_api_key(settings.api_key.as_ref())?),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/elevenlabs/{}", ai_sdk_rust::VERSION)],
    )
    .into_iter()
    .map(|(name, value)| (name, Some(value)))
    .collect())
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn elevenlabs_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("ELEVENLABS_API_KEY", "ElevenLabs");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn elevenlabs_url(path: &str) -> String {
    format!("{DEFAULT_ELEVENLABS_BASE_URL}{path}")
}

fn elevenlabs_output_format(output_format: Option<&str>) -> String {
    match output_format.unwrap_or("mp3_44100_128") {
        "mp3" => "mp3_44100_128",
        "mp3_32" => "mp3_44100_32",
        "mp3_64" => "mp3_44100_64",
        "mp3_96" => "mp3_44100_96",
        "mp3_128" => "mp3_44100_128",
        "mp3_192" => "mp3_44100_192",
        "pcm" => "pcm_44100",
        "pcm_16000" => "pcm_16000",
        "pcm_22050" => "pcm_22050",
        "pcm_24000" => "pcm_24000",
        "pcm_44100" => "pcm_44100",
        "ulaw" => "ulaw_8000",
        other => other,
    }
    .to_string()
}

fn elevenlabs_transcription_response(
    value: &JsonValue,
) -> Result<ElevenLabsTranscriptionResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn elevenlabs_error_data(value: &JsonValue) -> Result<ElevenLabsErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn elevenlabs_speech_result_from_response(
    model_id: &str,
    audio: Vec<u8>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let mut response = SpeechModelResponse::new(timestamp, model_id);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = SpeechModelResult::new(FileDataContent::Bytes(audio), response)
        .with_request(SpeechModelRequest::new().with_body(request_body));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn elevenlabs_speech_result_from_handled_error(
    model_id: &str,
    error: HandledFetchError,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let (message, headers, body) = handled_error_parts(error);
    elevenlabs_speech_result_from_error(
        model_id,
        message,
        request_body,
        headers,
        body,
        warnings,
        timestamp,
    )
}

fn elevenlabs_speech_result_from_error(
    model_id: &str,
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let response_body = raw_body.unwrap_or_else(|| request_body.clone());
    let mut response = SpeechModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = SpeechModelResult::new(FileDataContent::Bytes(Vec::new()), response)
        .with_request(SpeechModelRequest::new().with_body(request_body))
        .with_provider_metadata(error_metadata("elevenlabs", message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn elevenlabs_transcription_result_from_response(
    model_id: &str,
    response_value: ElevenLabsTranscriptionResponse,
    response_headers: Option<Headers>,
    raw_value: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body =
        raw_value.unwrap_or_else(|| serde_json::to_value(&response_value).expect("serializes"));
    let mut response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let segments = response_value
        .words
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|word| {
            TranscriptionModelSegment::new(
                word.text,
                word.start.unwrap_or(0.0),
                word.end.unwrap_or(0.0),
            )
        })
        .collect::<Vec<_>>();
    let duration = response_value
        .words
        .as_ref()
        .and_then(|words| words.last())
        .and_then(|word| word.end);
    let mut result = TranscriptionModelResult::new(response_value.text, segments, response)
        .with_language(response_value.language_code);

    if let Some(duration) = duration {
        result = result.with_duration_in_seconds(duration);
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn elevenlabs_transcription_result_from_handled_error(
    model_id: &str,
    error: HandledFetchError,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let (message, headers, body) = handled_error_parts(error);
    elevenlabs_transcription_result_from_error(
        model_id, message, headers, body, warnings, timestamp,
    )
}

fn elevenlabs_transcription_result_from_error(
    model_id: &str,
    message: String,
    response_headers: Option<Headers>,
    raw_body: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body = raw_body.unwrap_or_else(|| JsonValue::Object(JsonObject::new()));
    let mut response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = TranscriptionModelResult::new("", Vec::new(), response)
        .with_provider_metadata(error_metadata("elevenlabs", message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>, Option<JsonValue>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => {
            let body = error
                .response_body()
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .or_else(|| {
                    error
                        .response_body()
                        .map(|body| JsonValue::String(body.to_string()))
                });

            (
                error.message().to_string(),
                error.response_headers().cloned(),
                body,
            )
        }
    }
}

fn error_metadata(provider_name: &str, message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(provider_name.to_string(), provider);
    metadata
}

fn audio_bytes(audio: &FileDataContent) -> Result<Vec<u8>, String> {
    match audio {
        FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
        FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
            .map_err(|error| format!("invalid base64 transcription audio: {error}")),
    }
}

fn json_number(value: f64) -> JsonValue {
    serde_json::Number::from_f64(value)
        .map(JsonValue::Number)
        .unwrap_or(JsonValue::Null)
}

fn insert_json_string(body: &mut JsonObject, name: &str, value: Option<&JsonValue>) {
    if let Some(value) = value.and_then(JsonValue::as_str) {
        body.insert(name.to_string(), JsonValue::String(value.to_string()));
    }
}

fn insert_json_bool(body: &mut JsonObject, name: &str, value: Option<&JsonValue>) {
    if let Some(value) = value.and_then(JsonValue::as_bool) {
        body.insert(name.to_string(), JsonValue::Bool(value));
    }
}

fn insert_json_number(body: &mut JsonObject, name: &str, value: Option<&JsonValue>) {
    if let Some(value) = value.and_then(JsonValue::as_f64) {
        body.insert(name.to_string(), json_number(value));
    }
}

fn insert_json_string_array(body: &mut JsonObject, name: &str, value: Option<&JsonValue>) {
    if let Some(values) = value.and_then(JsonValue::as_array) {
        let values = values
            .iter()
            .filter_map(JsonValue::as_str)
            .map(|value| JsonValue::String(value.to_string()))
            .collect::<Vec<_>>();
        body.insert(name.to_string(), JsonValue::Array(values));
    }
}

fn json_scalar_to_string(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Number(value)) => Some(value.to_string()),
        Some(JsonValue::Bool(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn set_query_param(query_params: &mut QueryParams, name: &str, value: impl Into<String>) {
    let value = value.into();
    if let Some((_, existing_value)) = query_params
        .iter_mut()
        .find(|(existing_name, _)| existing_name == name)
    {
        *existing_value = value;
    } else {
        query_params.push((name.to_string(), value));
    }
}

fn with_query_params(url: String, query_params: QueryParams) -> String {
    if query_params.is_empty() {
        return url;
    }

    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in query_params {
        serializer.append_pair(&name, &value);
    }
    format!("{url}?{}", serializer.finish())
}

fn form_data_request_body_values(form_data: &ai_sdk_rust::FormData) -> JsonValue {
    let mut values = JsonObject::new();

    for entry in &form_data.entries {
        values.insert(
            entry.name.clone(),
            form_data_value_to_request_body_value(&entry.value),
        );
    }

    JsonValue::Object(values)
}

fn form_data_value_to_request_body_value(value: &ai_sdk_rust::FormDataValue) -> JsonValue {
    match value {
        ai_sdk_rust::FormDataValue::Text { value } => JsonValue::String(value.clone()),
        ai_sdk_rust::FormDataValue::Bytes { value } => JsonValue::Array(
            value
                .iter()
                .copied()
                .map(JsonValue::from)
                .collect::<Vec<_>>(),
        ),
    }
}

fn multipart_body(
    form_data: &ai_sdk_rust::FormData,
    media_type: Option<&str>,
) -> (String, Vec<u8>) {
    let boundary = "----ai-sdk-rust-elevenlabs-boundary";
    let audio_filename = media_type
        .map(audio_filename)
        .unwrap_or_else(|| "audio".to_string());
    let media_type = media_type.unwrap_or("application/octet-stream");
    let mut body = Vec::new();

    for entry in &form_data.entries {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());

        match &entry.value {
            ai_sdk_rust::FormDataValue::Text { value } => {
                body.extend_from_slice(
                    format!(
                        "content-disposition: form-data; name=\"{}\"\r\n\r\n",
                        entry.name
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
                body.extend_from_slice(b"\r\n");
            }
            ai_sdk_rust::FormDataValue::Bytes { value } => {
                body.extend_from_slice(
                    format!(
                        "content-disposition: form-data; name=\"{}\"; filename=\"{}\"\r\ncontent-type: {}\r\n\r\n",
                        entry.name, audio_filename, media_type
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value);
                body.extend_from_slice(b"\r\n");
            }
        }
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn audio_filename(media_type: &str) -> String {
    let extension = media_type_to_extension(media_type);

    if extension.is_empty() {
        "audio".to_string()
    } else {
        format!("audio.{extension}")
    }
}

fn default_elevenlabs_date_provider() -> ElevenLabsDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_elevenlabs_transport() -> ElevenLabsTransport {
    Arc::new(|request| Box::pin(ready(execute_elevenlabs_request(request))))
}

fn execute_elevenlabs_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_elevenlabs_get_request(request),
        ProviderApiRequestMethod::Post => execute_elevenlabs_post_request(request),
    }
}

fn execute_elevenlabs_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    elevenlabs_provider_api_response(response)
}

fn execute_elevenlabs_post_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::post(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let builder = builder.config().http_status_as_error(false).build();
    let response = match request.body {
        Some(ProviderApiRequestBody::Text { content }) => builder.send(content),
        Some(ProviderApiRequestBody::Bytes { content }) => builder.send(content),
        Some(ProviderApiRequestBody::FormData { content }) => {
            let (content_type, body) = multipart_body(&content, None);
            builder.header("content-type", content_type).send(body)
        }
        None => builder.send_empty(),
    };

    elevenlabs_provider_api_response(response)
}

fn elevenlabs_provider_api_response(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut response = response.map_err(|error| {
        FetchErrorInfo::new("fetch failed")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Headers>();
    let body = response.body_mut().read_to_vec().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;

    Ok(ProviderApiResponse::bytes(status.as_u16(), status_text, body).with_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::{
        ElevenLabsProviderSettings, ElevenLabsTransport, ElevenLabsTransportFuture,
        create_elevenlabs, elevenlabs,
    };
    use ai_sdk_rust::{
        FileDataContent, ModelType, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions, ProviderWithSpeechModel,
        ProviderWithTranscriptionModel, SpeechModel, SpeechModelCallOptions, TranscriptionModel,
        TranscriptionModelCallOptions, TranscriptionModelSegment, Warning,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use time::OffsetDateTime;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    fn capture_transport<F>(
        handler: F,
    ) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, ElevenLabsTransport)
    where
        F: Fn(&ProviderApiRequest) -> ProviderApiResponse + Send + Sync + 'static,
    {
        let requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured = Arc::clone(&requests);
        let handler = Arc::new(handler);
        let transport = Arc::new(
            move |request: ProviderApiRequest| -> ElevenLabsTransportFuture {
                captured
                    .lock()
                    .expect("requests lock")
                    .push(request.clone());
                let response = handler(&request);
                Box::pin(ready(Ok(response)))
            },
        );

        (requests, transport)
    }

    fn json_response(value: serde_json::Value) -> ProviderApiResponse {
        ProviderApiResponse::text(200, "OK", serde_json::to_string(&value).expect("json"))
            .with_headers(ai_sdk_rust::Headers::from([(
                "content-type".to_string(),
                "application/json".to_string(),
            )]))
    }

    fn request_json_body(request: &ProviderApiRequest) -> serde_json::Value {
        match request.body.as_ref().expect("request body") {
            ProviderApiRequestBody::Text { content } => {
                serde_json::from_str(content).expect("json body")
            }
            other => panic!("expected text JSON body, got {other:?}"),
        }
    }

    #[test]
    fn elevenlabs_speech_model_sends_headers_body_query_options_and_metadata() {
        let (requests, transport) = capture_transport(|_| {
            ProviderApiResponse::bytes(200, "OK", vec![7_u8; 5]).with_headers(
                ai_sdk_rust::Headers::from([("x-request-id".to_string(), "req_123".to_string())]),
            )
        });
        let provider = create_elevenlabs(
            ElevenLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-value"),
        )
        .with_transport(transport)
        .with_current_date(|| OffsetDateTime::UNIX_EPOCH);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "elevenlabs": {
                "voiceSettings": {
                    "stability": 0.5,
                    "similarityBoost": 0.75,
                    "style": 0.25,
                    "useSpeakerBoost": true
                },
                "pronunciationDictionaryLocators": [
                    {
                        "pronunciationDictionaryId": "dict-1",
                        "versionId": "v1"
                    }
                ],
                "seed": 123,
                "previousText": "before",
                "nextText": "after",
                "previousRequestIds": ["prev-1"],
                "nextRequestIds": ["next-1"],
                "applyTextNormalization": "auto",
                "applyLanguageTextNormalization": true,
                "enableLogging": false
            }
        }))
        .expect("provider options");
        let result = poll_ready(
            provider.speech("eleven_multilingual_v2").do_generate(
                SpeechModelCallOptions::new("Hello, world!")
                    .with_voice("voice-123")
                    .with_output_format("pcm")
                    .with_language("es")
                    .with_speed(1.25)
                    .with_provider_options(provider_options)
                    .with_header("Custom-Request-Header", "request-value"),
            ),
        );
        let requests = requests.lock().expect("requests lock");
        let request = &requests[0];

        assert_eq!(result.audio, FileDataContent::Bytes(vec![7_u8; 5]));
        assert_eq!(result.response.timestamp, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(result.response.model_id, "eleven_multilingual_v2");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req_123".to_string())
        );
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.elevenlabs.io/v1/text-to-speech/voice-123?output_format=pcm_44100&enable_logging=false"
        );
        assert_eq!(
            request.headers.get("xi-api-key"),
            Some(&"test-api-key".to_string())
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/elevenlabs/"))
        );
        assert_eq!(
            request.headers.get("custom-provider-header"),
            Some(&"provider-value".to_string())
        );
        assert_eq!(
            request.headers.get("custom-request-header"),
            Some(&"request-value".to_string())
        );
        assert_eq!(
            request_json_body(request),
            json!({
                "text": "Hello, world!",
                "model_id": "eleven_multilingual_v2",
                "language_code": "es",
                "voice_settings": {
                    "speed": 1.25,
                    "stability": 0.5,
                    "similarity_boost": 0.75,
                    "style": 0.25,
                    "use_speaker_boost": true
                },
                "pronunciation_dictionary_locators": [
                    {
                        "pronunciation_dictionary_id": "dict-1",
                        "version_id": "v1"
                    }
                ],
                "seed": 123.0,
                "previous_text": "before",
                "next_text": "after",
                "previous_request_ids": ["prev-1"],
                "next_request_ids": ["next-1"],
                "apply_text_normalization": "auto",
                "apply_language_text_normalization": true
            })
        );
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref()),
            Some(&request_json_body(request))
        );
    }

    #[test]
    fn elevenlabs_speech_model_maps_format_defaults_and_instruction_warning() {
        let (requests, transport) =
            capture_transport(|_| ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3]));
        let provider =
            create_elevenlabs(ElevenLabsProviderSettings::new().with_api_key("test-api-key"))
                .with_transport(transport);
        let result =
            poll_ready(provider.speech("eleven_turbo_v2_5").do_generate(
                SpeechModelCallOptions::new("Hello").with_instructions("Speak slowly"),
            ));
        let requests = requests.lock().expect("requests lock");

        assert_eq!(
            requests[0].url,
            format!(
                "https://api.elevenlabs.io/v1/text-to-speech/{}?output_format=mp3_44100_128",
                super::DEFAULT_ELEVENLABS_VOICE_ID
            )
        );
        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "instructions".to_string(),
                details: Some(
                    "ElevenLabs speech models do not support instructions. Instructions parameter was ignored."
                        .to_string()
                )
            }]
        );
    }

    #[test]
    fn elevenlabs_transcription_model_sends_form_options_and_maps_response() {
        let fixture = json!({
            "language_code": "eng",
            "language_probability": 0.89,
            "text": "Hello from the Vercel AI SDK.",
            "words": [
                {
                    "text": "Hello",
                    "type": "word",
                    "start": 0.199,
                    "end": 0.479
                },
                {
                    "text": " ",
                    "type": "spacing",
                    "start": 0.479,
                    "end": 0.499
                },
                {
                    "text": "SDK.",
                    "type": "word",
                    "start": 1.58,
                    "end": 2.479
                }
            ]
        });
        let (requests, transport) = capture_transport(move |_| json_response(fixture.clone()));
        let provider = create_elevenlabs(
            ElevenLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-value"),
        )
        .with_transport(transport)
        .with_current_date(|| OffsetDateTime::UNIX_EPOCH);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "elevenlabs": {
                "languageCode": "en",
                "fileFormat": "pcm_s16le_16",
                "tagAudioEvents": false,
                "numSpeakers": 2,
                "timestampsGranularity": "character",
                "diarize": true
            }
        }))
        .expect("provider options");

        let result = poll_ready(
            provider.transcription("scribe_v1").do_generate(
                TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![82, 73, 70, 70]),
                    "audio/wav",
                )
                .with_provider_options(provider_options)
                .with_header("Custom-Request-Header", "request-value"),
            ),
        );
        let requests = requests.lock().expect("requests lock");
        let request = &requests[0];

        assert_eq!(request.url, "https://api.elevenlabs.io/v1/speech-to-text");
        assert_eq!(
            request.headers.get("xi-api-key"),
            Some(&"test-api-key".to_string())
        );
        assert_eq!(
            request.headers.get("custom-provider-header"),
            Some(&"provider-value".to_string())
        );
        assert_eq!(
            request.headers.get("custom-request-header"),
            Some(&"request-value".to_string())
        );
        assert_eq!(
            request.request_body_values,
            json!({
                "model_id": "scribe_v1",
                "file": [82, 73, 70, 70],
                "diarize": "true",
                "language_code": "en",
                "tag_audio_events": "false",
                "num_speakers": "2",
                "timestamps_granularity": "character",
                "file_format": "pcm_s16le_16"
            })
        );
        assert!(
            request
                .headers
                .get("content-type")
                .is_some_and(|value| value.contains("multipart/form-data"))
        );
        let ProviderApiRequestBody::Bytes { content } =
            request.body.as_ref().expect("multipart body")
        else {
            panic!("expected bytes body");
        };
        let body = String::from_utf8_lossy(content);
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.contains("content-type: audio/wav"));
        assert_eq!(result.text, "Hello from the Vercel AI SDK.");
        assert_eq!(result.language.as_deref(), Some("eng"));
        assert_eq!(result.duration_in_seconds, Some(2.479));
        assert_eq!(
            result.segments,
            vec![
                TranscriptionModelSegment::new("Hello", 0.199, 0.479),
                TranscriptionModelSegment::new(" ", 0.479, 0.499),
                TranscriptionModelSegment::new("SDK.", 1.58, 2.479),
            ]
        );
        assert_eq!(result.response.timestamp, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(result.response.model_id, "scribe_v1");
    }

    #[test]
    fn elevenlabs_transcription_model_applies_upstream_defaults_when_options_object_is_present() {
        let fixture = json!({
            "language_code": "eng",
            "language_probability": 0.89,
            "text": "Hello",
            "words": []
        });
        let (requests, transport) = capture_transport(move |_| json_response(fixture.clone()));
        let provider =
            create_elevenlabs(ElevenLabsProviderSettings::new().with_api_key("test-api-key"))
                .with_transport(transport);
        let provider_options: ProviderOptions =
            serde_json::from_value(json!({ "elevenlabs": {} })).expect("provider options");

        let result = poll_ready(
            provider.transcription("scribe_v1").do_generate(
                TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/mpeg")
                    .with_provider_options(provider_options),
            ),
        );
        let requests = requests.lock().expect("requests lock");

        assert_eq!(
            requests[0].request_body_values,
            json!({
                "model_id": "scribe_v1",
                "file": [1],
                "diarize": "false",
                "tag_audio_events": "true",
                "timestamps_granularity": "word",
                "file_format": "other"
            })
        );
        assert_eq!(result.duration_in_seconds, None);
    }

    #[test]
    fn elevenlabs_transcription_model_uses_real_date_when_no_custom_date_provider_is_specified() {
        let fixture = json!({
            "language_code": "eng",
            "language_probability": 0.89,
            "text": "Hello from the Vercel AI SDK.",
            "words": []
        });
        let (_requests, transport) = capture_transport(move |_| json_response(fixture.clone()));
        // No `with_current_date` override: the default provider must supply a real
        // wall-clock timestamp (mirrors upstream "use real date when no custom date
        // provider is specified").
        let provider =
            create_elevenlabs(ElevenLabsProviderSettings::new().with_api_key("test-api-key"))
                .with_transport(transport);

        let before = OffsetDateTime::now_utc();
        let result = poll_ready(provider.transcription("scribe_v1").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
        ));
        let after = OffsetDateTime::now_utc();

        assert_eq!(result.response.model_id, "scribe_v1");
        // The default provider stamps the current instant, not the UNIX epoch the
        // custom-date tests inject, so the timestamp must fall within the call window.
        assert_ne!(result.response.timestamp, OffsetDateTime::UNIX_EPOCH);
        assert!(result.response.timestamp >= before);
        assert!(result.response.timestamp <= after);
    }

    #[test]
    fn elevenlabs_models_map_api_errors_to_metadata() {
        let (requests, transport) = capture_transport(|_| {
            ProviderApiResponse::text(
                429,
                "Too Many Requests",
                r#"{"error":{"message":"Resource has been exhausted (e.g. check quota).","code":429}}"#,
            )
            .with_headers(ai_sdk_rust::Headers::from([(
                "x-request-id".to_string(),
                "req_error".to_string(),
            )]))
        });
        let provider =
            create_elevenlabs(ElevenLabsProviderSettings::new().with_api_key("test-api-key"))
                .with_transport(transport);
        let result = poll_ready(
            provider
                .speech("eleven_turbo_v2_5")
                .do_generate(SpeechModelCallOptions::new("Hello")),
        );

        assert_eq!(requests.lock().expect("requests lock").len(), 1);
        assert_eq!(result.audio, FileDataContent::Bytes(Vec::new()));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("elevenlabs"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(serde_json::Value::as_str),
            Some("Resource has been exhausted (e.g. check quota).")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req_error".to_string())
        );
    }

    #[test]
    fn elevenlabs_provider_reports_unsupported_model_families_and_traits() {
        let provider =
            create_elevenlabs(ElevenLabsProviderSettings::new().with_api_key("test-api-key"));

        assert_eq!(
            provider
                .language_model("gpt")
                .err()
                .expect("language model unsupported")
                .model_type(),
            ModelType::LanguageModel
        );
        assert_eq!(
            provider
                .embedding_model("embed")
                .err()
                .expect("embedding model unsupported")
                .model_type(),
            ModelType::EmbeddingModel
        );
        assert_eq!(
            provider
                .image_model("image")
                .err()
                .expect("image model unsupported")
                .model_type(),
            ModelType::ImageModel
        );
        assert_eq!(
            ProviderWithSpeechModel::speech_model(&provider, "eleven_turbo_v2_5")
                .expect("speech model")
                .provider(),
            "elevenlabs.speech"
        );
        assert_eq!(
            ProviderWithTranscriptionModel::transcription_model(&provider, "scribe_v1")
                .expect("transcription model")
                .provider(),
            "elevenlabs.transcription"
        );
        assert_eq!(
            elevenlabs("scribe_v1").provider(),
            "elevenlabs.transcription"
        );
    }

    #[test]
    fn elevenlabs_provider_settings_serde_accepts_upstream_shape() {
        let settings: ElevenLabsProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "headers": {
                "x-extra": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(settings.headers.get("x-extra"), Some(&"value".to_string()));
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "apiKey": "key",
                "headers": {
                    "x-extra": "value"
                }
            })
        );
    }
}
