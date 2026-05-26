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
    TranscriptionModelRequest, TranscriptionModelResponse, TranscriptionModelResult,
    TranscriptionModelSegment, Warning, combine_headers, convert_base64_to_bytes,
    create_binary_response_handler, create_json_error_response_handler,
    create_json_response_handler, load_api_key, parse_provider_options, post_json_to_api,
    post_to_api, with_user_agent_suffix,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/deepgram` API calls.
pub const DEFAULT_DEEPGRAM_BASE_URL: &str = "https://api.deepgram.com";

/// Settings for the upstream Deepgram provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepgramProviderSettings {
    /// Deepgram API key. When omitted, `DEEPGRAM_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl DeepgramProviderSettings {
    /// Creates empty Deepgram provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Deepgram API key.
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

/// Upstream Deepgram provider foundation.
#[derive(Clone)]
pub struct DeepgramProvider {
    settings: DeepgramProviderSettings,
    transport: DeepgramTransport,
    current_date: DeepgramDateProvider,
}

/// Deepgram speech model for `/v1/speak` calls.
#[derive(Clone)]
pub struct DeepgramSpeechModel {
    model_id: String,
    settings: DeepgramProviderSettings,
    transport: DeepgramTransport,
    current_date: DeepgramDateProvider,
}

/// Deepgram transcription model for `/v1/listen` calls.
#[derive(Clone)]
pub struct DeepgramTranscriptionModel {
    model_id: String,
    settings: DeepgramProviderSettings,
    transport: DeepgramTransport,
    current_date: DeepgramDateProvider,
}

/// Future returned by an injected Deepgram HTTP transport.
pub type DeepgramTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Deepgram provider models.
pub type DeepgramTransport =
    Arc<dyn Fn(ProviderApiRequest) -> DeepgramTransportFuture + Send + Sync>;

type DeepgramDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type DeepgramSpeechGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = SpeechModelResult> + Send + 'a>>;
type DeepgramTranscriptionGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>;

impl DeepgramProvider {
    /// Creates a Deepgram provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(DeepgramProviderSettings::new())
    }

    /// Creates a provider from explicit Deepgram settings.
    pub fn from_settings(settings: DeepgramProviderSettings) -> Self {
        Self {
            settings,
            transport: default_deepgram_transport(),
            current_date: default_deepgram_date_provider(),
        }
    }

    /// Sets the Deepgram API key for this provider.
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
    pub fn with_transport(mut self, transport: DeepgramTransport) -> Self {
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
    pub fn transcription(&self, model_id: impl Into<String>) -> DeepgramTranscriptionModel {
        self.transcription_model(model_id)
            .expect("Deepgram transcription models are supported")
    }

    /// Creates a transcription model.
    pub fn transcription_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<DeepgramTranscriptionModel, NoSuchModelError> {
        Ok(DeepgramTranscriptionModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Creates a speech model.
    pub fn speech(&self, model_id: impl Into<String>) -> DeepgramSpeechModel {
        self.speech_model(model_id)
            .expect("Deepgram speech models are supported")
    }

    /// Creates a speech model.
    pub fn speech_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<DeepgramSpeechModel, NoSuchModelError> {
        Ok(DeepgramSpeechModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Deepgram does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "Deepgram does not provide language models",
        ))
    }

    /// Reports that Deepgram does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            "Deepgram does not provide text embedding models",
        ))
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Deepgram does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            "Deepgram does not provide image models",
        ))
    }
}

impl Default for DeepgramProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for DeepgramProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        DeepgramProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        DeepgramProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        DeepgramProvider::image_model(self, model_id)
    }
}

impl ProviderWithSpeechModel for DeepgramProvider {
    type SpeechModel = DeepgramSpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        DeepgramProvider::speech_model(self, model_id)
    }
}

impl ProviderWithTranscriptionModel for DeepgramProvider {
    type TranscriptionModel = DeepgramTranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        DeepgramProvider::transcription_model(self, model_id)
    }
}

impl DeepgramSpeechModel {
    fn new(
        model_id: impl Into<String>,
        settings: DeepgramProviderSettings,
        transport: DeepgramTransport,
        current_date: DeepgramDateProvider,
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
        "deepgram.speech"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: DeepgramTransport) -> Self {
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
        let (request_body, query_params, warnings) =
            match deepgram_speech_request(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return deepgram_speech_result_from_error(
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
                return deepgram_speech_result_from_error(
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
            with_query_params(deepgram_url("/v1/speak"), query_params),
            request_body,
        )
        .with_headers(request_headers)
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
                    deepgram_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => deepgram_speech_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                request_body_for_error,
                warnings,
                timestamp,
            ),
            Err(error) => deepgram_speech_result_from_handled_error(
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
            Some(deepgram_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl SpeechModel for DeepgramSpeechModel {
    type GenerateFuture<'a>
        = DeepgramSpeechGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        DeepgramSpeechModel::provider(self)
    }

    fn model_id(&self) -> &str {
        DeepgramSpeechModel::model_id(self)
    }

    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl DeepgramTranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        settings: DeepgramProviderSettings,
        transport: DeepgramTransport,
        current_date: DeepgramDateProvider,
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
        "deepgram.transcription"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: DeepgramTransport) -> Self {
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
        let audio = match deepgram_audio_bytes(&options.audio) {
            Ok(audio) => audio,
            Err(message) => {
                return deepgram_transcription_result_from_error(
                    &self.model_id,
                    message,
                    None,
                    None,
                    None,
                    Vec::new(),
                    timestamp,
                );
            }
        };
        let (query_params, warnings) =
            match deepgram_transcription_query_params(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return deepgram_transcription_result_from_error(
                        &self.model_id,
                        error.to_string(),
                        None,
                        None,
                        None,
                        Vec::new(),
                        timestamp,
                    );
                }
            };
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return deepgram_transcription_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let headers = combine_headers([
            Some(vec![(
                "Content-Type".to_string(),
                Some(options.media_type.clone()),
            )]),
            Some(request_headers.into_iter().collect::<Vec<_>>()),
        ]);
        let post_options = PostToApiOptions::new(
            with_query_params(deepgram_url("/v1/listen"), query_params),
            ProviderApiRequestBody::Bytes { content: audio },
            JsonValue::Object(JsonObject::new()),
        )
        .with_headers(headers)
        .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    deepgram_transcription_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    deepgram_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => deepgram_transcription_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                response.raw_value,
                warnings,
                timestamp,
            ),
            Err(error) => deepgram_transcription_result_from_handled_error(
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
            Some(deepgram_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl TranscriptionModel for DeepgramTranscriptionModel {
    type GenerateFuture<'a>
        = DeepgramTranscriptionGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        DeepgramTranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &str {
        DeepgramTranscriptionModel::model_id(self)
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Deepgram provider with explicit settings.
pub fn create_deepgram(settings: DeepgramProviderSettings) -> DeepgramProvider {
    DeepgramProvider::from_settings(settings)
}

/// Creates a Deepgram transcription model using the default provider settings.
pub fn deepgram(model_id: impl Into<String>) -> DeepgramTranscriptionModel {
    DeepgramProvider::new().transcription(model_id)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepgramSpeechModelOptions {
    #[serde(
        default,
        rename = "bitRate",
        alias = "bit_rate",
        skip_serializing_if = "Option::is_none"
    )]
    pub bit_rate: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    #[serde(
        default,
        rename = "sampleRate",
        alias = "sample_rate",
        skip_serializing_if = "Option::is_none"
    )]
    pub sample_rate: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback: Option<String>,
    #[serde(
        default,
        rename = "callbackMethod",
        alias = "callback_method",
        skip_serializing_if = "Option::is_none"
    )]
    pub callback_method: Option<String>,
    #[serde(
        default,
        rename = "mipOptOut",
        alias = "mip_opt_out",
        skip_serializing_if = "Option::is_none"
    )]
    pub mip_opt_out: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<StringOrStringArray>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum StringOrStringArray {
    String(String),
    Array(Vec<String>),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepgramTranscriptionModelOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(
        default,
        rename = "detectLanguage",
        alias = "detect_language",
        skip_serializing_if = "Option::is_none"
    )]
    pub detect_language: Option<bool>,
    #[serde(
        default,
        rename = "smartFormat",
        alias = "smart_format",
        skip_serializing_if = "Option::is_none"
    )]
    pub smart_format: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub punctuate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraphs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarize: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topics: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intents: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<bool>,
    #[serde(
        default,
        rename = "detectEntities",
        alias = "detect_entities",
        skip_serializing_if = "Option::is_none"
    )]
    pub detect_entities: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redact: Option<StringOrStringArray>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyterm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diarize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utterances: Option<bool>,
    #[serde(
        default,
        rename = "uttSplit",
        alias = "utt_split",
        skip_serializing_if = "Option::is_none"
    )]
    pub utt_split: Option<f64>,
    #[serde(
        default,
        rename = "fillerWords",
        alias = "filler_words",
        skip_serializing_if = "Option::is_none"
    )]
    pub filler_words: Option<bool>,
}

type QueryParams = Vec<(String, String)>;

fn deepgram_speech_request(
    model_id: &str,
    options: &SpeechModelCallOptions,
) -> Result<(JsonValue, QueryParams, Vec<Warning>), ai_sdk_rust::InvalidArgumentError> {
    let mut warnings = Vec::new();
    let deepgram_options = parse_provider_options(
        "deepgram",
        options.provider_options.as_ref(),
        deepgram_speech_model_options,
    )?;
    let mut body = JsonObject::new();
    body.insert("text".to_string(), JsonValue::String(options.text.clone()));

    let mut query_params = vec![("model".to_string(), model_id.to_string())];
    apply_deepgram_output_format(
        options.output_format.as_deref().unwrap_or("mp3"),
        &mut query_params,
    );

    if let Some(deepgram_options) = deepgram_options {
        apply_deepgram_speech_options(deepgram_options, &mut query_params, &mut warnings);
    }

    if let Some(voice) = options.voice.as_ref() {
        if voice != model_id {
            warnings.push(Warning::Unsupported {
                feature: "voice".to_string(),
                details: Some(format!(
                    "Deepgram TTS models embed the voice in the model ID. The voice parameter \"{voice}\" was ignored. Use the model ID to select a voice (e.g., \"aura-2-helena-en\")."
                )),
            });
        }
    }
    if options.speed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "speed".to_string(),
            details: Some(
                "Deepgram TTS REST API does not support speed adjustment. Speed parameter was ignored."
                    .to_string(),
            ),
        });
    }
    if let Some(language) = options.language.as_ref() {
        warnings.push(Warning::Unsupported {
            feature: "language".to_string(),
            details: Some(format!(
                "Deepgram TTS models are language-specific via the model ID. Language parameter \"{language}\" was ignored. Select a model with the appropriate language suffix (e.g., \"-en\" for English)."
            )),
        });
    }
    if options.instructions.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "instructions".to_string(),
            details: Some(
                "Deepgram TTS REST API does not support instructions. Instructions parameter was ignored."
                    .to_string(),
            ),
        });
    }

    Ok((JsonValue::Object(body), query_params, warnings))
}

fn deepgram_speech_model_options(value: &JsonValue) -> Result<DeepgramSpeechModelOptions, String> {
    let options = serde_json::from_value::<DeepgramSpeechModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;

    if let Some(callback_method) = options.callback_method.as_deref() {
        if !matches!(callback_method, "POST" | "PUT") {
            return Err("callbackMethod must be POST or PUT".to_string());
        }
    }

    Ok(options)
}

fn apply_deepgram_output_format(output_format: &str, query_params: &mut QueryParams) {
    let format_lower = output_format.to_lowercase();

    match format_lower.as_str() {
        "mp3" => set_query_param(query_params, "encoding", "mp3"),
        "wav" | "linear16" => {
            set_query_param(query_params, "encoding", "linear16");
            set_query_param(query_params, "container", "wav");
        }
        "mulaw" => {
            set_query_param(query_params, "encoding", "mulaw");
            set_query_param(query_params, "container", "wav");
        }
        "alaw" => {
            set_query_param(query_params, "encoding", "alaw");
            set_query_param(query_params, "container", "wav");
        }
        "opus" | "ogg" => {
            set_query_param(query_params, "encoding", "opus");
            set_query_param(query_params, "container", "ogg");
        }
        "flac" => set_query_param(query_params, "encoding", "flac"),
        "aac" => set_query_param(query_params, "encoding", "aac"),
        "pcm" => {
            set_query_param(query_params, "encoding", "linear16");
            set_query_param(query_params, "container", "none");
        }
        other => apply_compound_deepgram_output_format(other, query_params),
    }
}

fn apply_compound_deepgram_output_format(output_format: &str, query_params: &mut QueryParams) {
    let parts = output_format.split('_').collect::<Vec<_>>();
    let Some(first_part) = parts.first().copied() else {
        return;
    };
    let sample_rate = parts.get(1).and_then(|value| value.parse::<u64>().ok());

    match first_part {
        "linear16" | "mulaw" | "alaw" => {
            set_query_param(query_params, "encoding", first_part);
            set_query_param(query_params, "container", "wav");
            if let Some(sample_rate) = sample_rate {
                set_query_param(query_params, "sample_rate", sample_rate.to_string());
            }
        }
        "opus" => {
            set_query_param(query_params, "encoding", "opus");
            set_query_param(query_params, "container", "ogg");
        }
        "mp3" | "flac" | "aac" => {
            set_query_param(query_params, "encoding", first_part);
            if first_part == "flac" {
                if let Some(sample_rate) = sample_rate {
                    set_query_param(query_params, "sample_rate", sample_rate.to_string());
                }
            }
        }
        "wav" => {
            set_query_param(query_params, "encoding", "linear16");
            set_query_param(query_params, "container", "wav");
            if let Some(sample_rate) = sample_rate {
                set_query_param(query_params, "sample_rate", sample_rate.to_string());
            }
        }
        "ogg" => {
            set_query_param(query_params, "encoding", "opus");
            set_query_param(query_params, "container", "ogg");
        }
        _ => {}
    }
}

fn apply_deepgram_speech_options(
    options: DeepgramSpeechModelOptions,
    query_params: &mut QueryParams,
    warnings: &mut Vec<Warning>,
) {
    if let Some(encoding) = options.encoding.as_deref() {
        let encoding = encoding.to_lowercase();
        set_query_param(query_params, "encoding", &encoding);

        if matches!(encoding.as_str(), "mp3" | "flac" | "aac") {
            remove_query_param(query_params, "container");
        } else if encoding == "opus" {
            set_query_param(query_params, "container", "ogg");
        } else if matches!(encoding.as_str(), "linear16" | "mulaw" | "alaw")
            && get_query_param(query_params, "container").is_none()
        {
            set_query_param(query_params, "container", "wav");
        }

        if matches!(encoding.as_str(), "mp3" | "opus" | "aac") {
            remove_query_param(query_params, "sample_rate");
        }
        if matches!(encoding.as_str(), "linear16" | "mulaw" | "alaw" | "flac") {
            remove_query_param(query_params, "bit_rate");
        }
    }

    if let Some(container) = options.container.as_deref() {
        let container = container.to_lowercase();
        let encoding = get_query_param(query_params, "encoding").unwrap_or_default();
        if matches!(encoding.as_str(), "mp3" | "flac" | "aac") {
            warnings.push(Warning::Unsupported {
                feature: "providerOptions".to_string(),
                details: Some(format!(
                    "Encoding \"{encoding}\" does not support container parameter. Container \"{container}\" was ignored."
                )),
            });
            remove_query_param(query_params, "container");
        } else if container == "ogg" {
            set_query_param(query_params, "container", "ogg");
            set_query_param(query_params, "encoding", "opus");
            remove_query_param(query_params, "sample_rate");
        } else {
            set_query_param(query_params, "container", container);
            if get_query_param(query_params, "encoding").is_none() {
                let default_encoding =
                    if get_query_param(query_params, "container").as_deref() == Some("ogg") {
                        "opus"
                    } else {
                        "linear16"
                    };
                set_query_param(query_params, "encoding", default_encoding);
            }
        }
    }

    if let Some(sample_rate) = options.sample_rate {
        set_query_param(query_params, "sample_rate", sample_rate.to_string());
    }
    if let Some(bit_rate) = options.bit_rate {
        let encoding = get_query_param(query_params, "encoding").unwrap_or_default();
        if matches!(encoding.as_str(), "linear16" | "mulaw" | "alaw" | "flac") {
            remove_query_param(query_params, "bit_rate");
        } else if let Some(value) = json_query_value(bit_rate) {
            set_query_param(query_params, "bit_rate", value);
        }
    }
    if let Some(callback) = options.callback {
        set_query_param(query_params, "callback", callback);
    }
    if let Some(callback_method) = options.callback_method {
        set_query_param(query_params, "callback_method", callback_method);
    }
    if let Some(mip_opt_out) = options.mip_opt_out {
        set_query_param(query_params, "mip_opt_out", mip_opt_out.to_string());
    }
    if let Some(tag) = options.tag {
        set_query_param(query_params, "tag", string_or_array_value(tag));
    }
}

fn deepgram_transcription_query_params(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> Result<(QueryParams, Vec<Warning>), ai_sdk_rust::InvalidArgumentError> {
    let warnings = Vec::new();
    let deepgram_options = parse_provider_options(
        "deepgram",
        options.provider_options.as_ref(),
        deepgram_transcription_model_options,
    )?;
    let mut query_params = vec![
        ("model".to_string(), model_id.to_string()),
        ("diarize".to_string(), "true".to_string()),
    ];

    if let Some(options) = deepgram_options {
        insert_option_string(&mut query_params, "language", options.language);
        insert_option_bool(
            &mut query_params,
            "detect_language",
            options.detect_language,
        );
        insert_option_bool(&mut query_params, "smart_format", options.smart_format);
        insert_option_bool(&mut query_params, "punctuate", options.punctuate);
        if let Some(summarize) = options.summarize.and_then(json_query_value) {
            set_query_param(&mut query_params, "summarize", summarize);
        }
        insert_option_bool(&mut query_params, "topics", options.topics);
        insert_option_bool(
            &mut query_params,
            "detect_entities",
            options.detect_entities,
        );
        if let Some(redact) = options.redact {
            set_query_param(&mut query_params, "redact", string_or_array_value(redact));
        }
        insert_option_string(&mut query_params, "search", options.search);
        insert_option_bool(&mut query_params, "diarize", options.diarize);
        insert_option_bool(&mut query_params, "utterances", options.utterances);
        insert_option_f64(&mut query_params, "utt_split", options.utt_split);
        insert_option_bool(&mut query_params, "filler_words", options.filler_words);
    }

    Ok((query_params, warnings))
}

fn deepgram_transcription_model_options(
    value: &JsonValue,
) -> Result<DeepgramTranscriptionModelOptions, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

fn set_query_param(
    query_params: &mut QueryParams,
    name: impl Into<String>,
    value: impl Into<String>,
) {
    let name = name.into();
    let value = value.into();
    if let Some((_, existing_value)) = query_params
        .iter_mut()
        .find(|(existing_name, _)| existing_name == &name)
    {
        *existing_value = value;
    } else {
        query_params.push((name, value));
    }
}

fn remove_query_param(query_params: &mut QueryParams, name: &str) {
    query_params.retain(|(existing_name, _)| existing_name != name);
}

fn get_query_param(query_params: &QueryParams, name: &str) -> Option<String> {
    query_params
        .iter()
        .find(|(existing_name, _)| existing_name == name)
        .map(|(_, value)| value.clone())
}

fn insert_option_string(query_params: &mut QueryParams, name: &str, value: Option<String>) {
    if let Some(value) = value {
        set_query_param(query_params, name, value);
    }
}

fn insert_option_bool(query_params: &mut QueryParams, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        set_query_param(query_params, name, value.to_string());
    }
}

fn insert_option_f64(query_params: &mut QueryParams, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        set_query_param(query_params, name, value.to_string());
    }
}

fn json_query_value(value: JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value),
        JsonValue::Number(value) => Some(value.to_string()),
        JsonValue::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn string_or_array_value(value: StringOrStringArray) -> String {
    match value {
        StringOrStringArray::String(value) => value,
        StringOrStringArray::Array(values) => values.join(","),
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

fn deepgram_audio_bytes(audio: &FileDataContent) -> Result<Vec<u8>, String> {
    match audio {
        FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
        FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
            .map_err(|error| format!("invalid base64 transcription audio: {error}")),
    }
}

fn deepgram_url(path: &str) -> String {
    format!("{DEFAULT_DEEPGRAM_BASE_URL}{path}")
}

fn deepgram_provider_header_entries(
    settings: &DeepgramProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "authorization".to_string(),
        Some(format!(
            "Token {}",
            deepgram_api_key(settings.api_key.as_ref())?
        )),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/deepgram/{}", ai_sdk_rust::VERSION)],
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

fn deepgram_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("DEEPGRAM_API_KEY", "Deepgram");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramErrorData {
    error: DeepgramErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramErrorBody {
    message: String,
    code: i64,
}

fn deepgram_error_data(value: &JsonValue) -> Result<DeepgramErrorData, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramTranscriptionApiResponse {
    metadata: Option<DeepgramTranscriptionMetadata>,
    results: Option<DeepgramTranscriptionResults>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramTranscriptionMetadata {
    duration: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramTranscriptionResults {
    channels: Vec<DeepgramTranscriptionChannel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramTranscriptionChannel {
    detected_language: Option<String>,
    alternatives: Vec<DeepgramTranscriptionAlternative>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramTranscriptionAlternative {
    transcript: String,
    #[serde(default)]
    words: Vec<DeepgramTranscriptionWord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DeepgramTranscriptionWord {
    word: String,
    start: f64,
    end: f64,
}

fn deepgram_transcription_response(
    value: &JsonValue,
) -> Result<DeepgramTranscriptionApiResponse, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

fn deepgram_speech_result_from_response(
    model_id: &str,
    audio: Vec<u8>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let mut response = SpeechModelResponse::new(timestamp, model_id);

    if let Some(headers) = response_headers {
        response = with_speech_response_headers(response, headers);
    }

    let mut result = SpeechModelResult::new(FileDataContent::Bytes(audio), response)
        .with_request(SpeechModelRequest::new().with_body(request_body));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn deepgram_speech_result_from_handled_error(
    model_id: &str,
    error: HandledFetchError,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let (message, headers, body) = deepgram_handled_error_parts(error);

    deepgram_speech_result_from_error(
        model_id,
        message,
        request_body,
        headers,
        body,
        warnings,
        timestamp,
    )
}

fn deepgram_speech_result_from_error(
    model_id: &str,
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<String>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let response_body = raw_body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.map(JsonValue::String))
        .unwrap_or_else(|| request_body.clone());
    let mut response = SpeechModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = response_headers {
        response = with_speech_response_headers(response, headers);
    }

    let mut result = SpeechModelResult::new(FileDataContent::Bytes(Vec::new()), response)
        .with_request(SpeechModelRequest::new().with_body(request_body))
        .with_provider_metadata(deepgram_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn deepgram_transcription_result_from_response(
    model_id: &str,
    response: DeepgramTranscriptionApiResponse,
    response_headers: Option<Headers>,
    raw_response: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let (text, segments, language) = deepgram_transcription_content(&response);
    let response_body = raw_response
        .unwrap_or_else(|| serde_json::to_value(&response).expect("response serializes"));
    let mut model_response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body.clone());

    if let Some(headers) = response_headers {
        model_response = with_transcription_response_headers(model_response, headers);
    }

    let mut result = TranscriptionModelResult::new(text, segments, model_response)
        .with_request(TranscriptionModelRequest::new().with_body(response_body.to_string()));

    if let Some(language) = language {
        result = result.with_language(language);
    }
    if let Some(duration) = response.metadata.and_then(|metadata| metadata.duration) {
        result = result.with_duration_in_seconds(duration);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn deepgram_transcription_result_from_handled_error(
    model_id: &str,
    error: HandledFetchError,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let (message, headers, body) = deepgram_handled_error_parts(error);

    deepgram_transcription_result_from_error(
        model_id, message, headers, body, None, warnings, timestamp,
    )
}

fn deepgram_transcription_result_from_error(
    model_id: &str,
    message: String,
    response_headers: Option<Headers>,
    raw_body: Option<String>,
    request_body: Option<String>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body = raw_body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.map(JsonValue::String))
        .unwrap_or_else(|| JsonValue::Object(JsonObject::new()));
    let mut response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = response_headers {
        response = with_transcription_response_headers(response, headers);
    }

    let mut result = TranscriptionModelResult::new("", Vec::new(), response)
        .with_provider_metadata(deepgram_error_metadata(message));

    if let Some(request_body) = request_body {
        result = result.with_request(TranscriptionModelRequest::new().with_body(request_body));
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn deepgram_transcription_content(
    response: &DeepgramTranscriptionApiResponse,
) -> (String, Vec<TranscriptionModelSegment>, Option<String>) {
    let Some(channel) = response
        .results
        .as_ref()
        .and_then(|results| results.channels.first())
    else {
        return (String::new(), Vec::new(), None);
    };
    let Some(alternative) = channel.alternatives.first() else {
        return (String::new(), Vec::new(), channel.detected_language.clone());
    };
    let segments = alternative
        .words
        .iter()
        .map(|word| TranscriptionModelSegment::new(&word.word, word.start, word.end))
        .collect();

    (
        alternative.transcript.clone(),
        segments,
        channel.detected_language.clone(),
    )
}

fn deepgram_handled_error_parts(
    error: HandledFetchError,
) -> (String, Option<Headers>, Option<String>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    }
}

fn with_speech_response_headers(
    mut response: SpeechModelResponse,
    headers: Headers,
) -> SpeechModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_transcription_response_headers(
    mut response: TranscriptionModelResponse,
    headers: Headers,
) -> TranscriptionModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn deepgram_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("deepgram".to_string(), provider);
    metadata
}

fn default_deepgram_date_provider() -> DeepgramDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_deepgram_transport() -> DeepgramTransport {
    Arc::new(|request| Box::pin(ready(execute_deepgram_request(request))))
}

fn execute_deepgram_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_deepgram_get_request(request),
        ProviderApiRequestMethod::Post => execute_deepgram_post_request(request),
    }
}

fn execute_deepgram_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    deepgram_provider_api_response(response)
}

fn execute_deepgram_post_request(
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
        Some(ProviderApiRequestBody::FormData { .. }) => {
            return Err(FetchErrorInfo::new(
                "multipart form data is not supported by the Deepgram transport",
            ));
        }
        None => builder.send_empty(),
    };

    deepgram_provider_api_response(response)
}

fn deepgram_provider_api_response(
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
        DEFAULT_DEEPGRAM_BASE_URL, DeepgramProvider, DeepgramProviderSettings, DeepgramTransport,
        DeepgramTransportFuture, create_deepgram, deepgram,
    };
    use ai_sdk_rust::{
        FileDataContent, ModelType, Provider, ProviderApiRequest, ProviderApiRequestBody,
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

    fn test_waker() -> Waker {
        Waker::from(Arc::new(NoopWake))
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    fn capture_transport(
        responses: Vec<ProviderApiResponse>,
    ) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, DeepgramTransport) {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let responses = Arc::new(Mutex::new(responses.into_iter()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let responses_for_transport = Arc::clone(&responses);
        let transport: DeepgramTransport = Arc::new(move |request| -> DeepgramTransportFuture {
            captured_requests_for_transport
                .lock()
                .expect("captured requests mutex is not poisoned")
                .push(request.clone());
            let response = responses_for_transport
                .lock()
                .expect("responses mutex is not poisoned")
                .next()
                .expect("test response is available");

            Box::pin(ready(Ok(response)))
        });

        (captured_requests, transport)
    }

    fn fixed_timestamp() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(0).expect("unix epoch is valid")
    }

    #[test]
    fn deepgram_speech_model_sends_headers_body_query_options_and_metadata() {
        let (captured_requests, transport) = capture_transport(vec![
            ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3]).with_headers(
                [("content-type".to_string(), "audio/mp3".to_string())]
                    .into_iter()
                    .collect(),
            ),
        ]);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "deepgram": {
                "encoding": "mp3",
                "bitRate": 48000,
                "container": "wav",
                "callback": "https://example.com/callback",
                "callbackMethod": "POST",
                "mipOptOut": true,
                "tag": ["tag1", "tag2"]
            }
        }))
        .expect("provider options deserialize");
        let provider = create_deepgram(
            DeepgramProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-header-value"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.speech("aura-2-helena-en").do_generate(
                SpeechModelCallOptions::new("Hello, welcome to Deepgram!")
                    .with_output_format("wav")
                    .with_provider_options(provider_options)
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        assert_eq!(result.audio, FileDataContent::Bytes(vec![1, 2, 3]));
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(result.response.model_id, "aura-2-helena-en");

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        let request = requests.first().expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert!(
            request
                .url
                .starts_with(&format!("{DEFAULT_DEEPGRAM_BASE_URL}/v1/speak?"))
        );
        assert!(request.url.contains("model=aura-2-helena-en"));
        assert!(request.url.contains("encoding=mp3"));
        assert!(request.url.contains("bit_rate=48000"));
        assert!(!request.url.contains("container=wav"));
        assert!(
            request
                .url
                .contains("callback=https%3A%2F%2Fexample.com%2Fcallback")
        );
        assert!(request.url.contains("callback_method=POST"));
        assert!(request.url.contains("mip_opt_out=true"));
        assert!(request.url.contains("tag=tag1%2Ctag2"));
        assert_eq!(
            request.headers.get("authorization"),
            Some(&"Token test-api-key".to_string())
        );
        assert_eq!(
            request.headers.get("custom-provider-header"),
            Some(&"provider-header-value".to_string())
        );
        assert_eq!(
            request.headers.get("custom-request-header"),
            Some(&"request-header-value".to_string())
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .expect("user agent is set")
                .contains("ai-sdk/deepgram/")
        );

        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "text": "Hello, welcome to Deepgram!"
            })
        );
    }

    #[test]
    fn deepgram_speech_model_maps_format_and_warnings() {
        let (captured_requests, transport) =
            capture_transport(vec![ProviderApiResponse::bytes(200, "OK", vec![9])]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aura-2-helena-en");

        let result = poll_ready(
            model.do_generate(
                SpeechModelCallOptions::new("Hello.")
                    .with_output_format("linear16_16000")
                    .with_voice("different-voice")
                    .with_speed(1.5)
                    .with_language("en")
                    .with_instructions("slowly"),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "voice".to_string(),
                    details: Some("Deepgram TTS models embed the voice in the model ID. The voice parameter \"different-voice\" was ignored. Use the model ID to select a voice (e.g., \"aura-2-helena-en\").".to_string())
                },
                Warning::Unsupported {
                    feature: "speed".to_string(),
                    details: Some("Deepgram TTS REST API does not support speed adjustment. Speed parameter was ignored.".to_string())
                },
                Warning::Unsupported {
                    feature: "language".to_string(),
                    details: Some("Deepgram TTS models are language-specific via the model ID. Language parameter \"en\" was ignored. Select a model with the appropriate language suffix (e.g., \"-en\" for English).".to_string())
                },
                Warning::Unsupported {
                    feature: "instructions".to_string(),
                    details: Some("Deepgram TTS REST API does not support instructions. Instructions parameter was ignored.".to_string())
                }
            ]
        );

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        let request = requests.first().expect("request is captured");
        assert!(request.url.contains("encoding=linear16"));
        assert!(request.url.contains("container=wav"));
        assert!(request.url.contains("sample_rate=16000"));
    }

    #[test]
    fn deepgram_transcription_model_sends_audio_query_headers_and_maps_response() {
        let (captured_requests, transport) = capture_transport(vec![
            ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "metadata": {
                        "duration": 2.5
                    },
                    "results": {
                        "channels": [
                            {
                                "detected_language": "en",
                                "alternatives": [
                                    {
                                        "transcript": "hello world",
                                        "words": [
                                            { "word": "hello", "start": 0.0, "end": 0.5 },
                                            { "word": "world", "start": 0.5, "end": 1.0 }
                                        ]
                                    }
                                ]
                            }
                        ]
                    }
                })
                .to_string(),
            )
            .with_headers(
                [("x-request-id".to_string(), "req_123".to_string())]
                    .into_iter()
                    .collect(),
            ),
        ]);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "deepgram": {
                "detectLanguage": true,
                "smartFormat": true,
                "paragraphs": true,
                "intents": true,
                "sentiment": true,
                "diarize": false,
                "redact": ["pci", "numbers"],
                "replace": "hello:world",
                "uttSplit": 0.8,
                "fillerWords": true,
                "keyterm": "important"
            }
        }))
        .expect("provider options deserialize");
        let provider = create_deepgram(
            DeepgramProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-header-value"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.transcription("nova-3").do_generate(
                TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3]),
                    "audio/wav",
                )
                .with_provider_options(provider_options)
                .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        assert_eq!(result.text, "hello world");
        assert_eq!(
            result.segments,
            vec![
                TranscriptionModelSegment::new("hello", 0.0, 0.5),
                TranscriptionModelSegment::new("world", 0.5, 1.0)
            ]
        );
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(result.duration_in_seconds, Some(2.5));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req_123".to_string())
        );

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        let request = requests.first().expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert!(
            request
                .url
                .starts_with(&format!("{DEFAULT_DEEPGRAM_BASE_URL}/v1/listen?"))
        );
        assert!(request.url.contains("model=nova-3"));
        assert!(request.url.contains("detect_language=true"));
        assert!(request.url.contains("smart_format=true"));
        assert!(request.url.contains("diarize=false"));
        assert!(request.url.contains("redact=pci%2Cnumbers"));
        assert!(request.url.contains("utt_split=0.8"));
        assert!(request.url.contains("filler_words=true"));
        assert!(!request.url.contains("paragraphs="));
        assert!(!request.url.contains("intents="));
        assert!(!request.url.contains("sentiment="));
        assert!(!request.url.contains("replace="));
        assert!(!request.url.contains("keyterm="));
        assert_eq!(
            request.headers.get("authorization"),
            Some(&"Token test-api-key".to_string())
        );
        assert_eq!(
            request.headers.get("content-type"),
            Some(&"audio/wav".to_string())
        );
        assert_eq!(
            request.headers.get("custom-provider-header"),
            Some(&"provider-header-value".to_string())
        );
        assert_eq!(
            request.headers.get("custom-request-header"),
            Some(&"request-header-value".to_string())
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_bytes),
            Some(&[1, 2, 3][..])
        );
    }

    #[test]
    fn deepgram_models_map_api_errors_to_metadata() {
        let (captured_requests, transport) = capture_transport(vec![
            ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({
                    "error": {
                        "message": "bad request",
                        "code": 123
                    }
                })
                .to_string(),
            )
            .with_headers(
                [("x-request-id".to_string(), "req_123".to_string())]
                    .into_iter()
                    .collect(),
            ),
        ]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aura-2-helena-en");

        let result = poll_ready(model.do_generate(SpeechModelCallOptions::new("Hello.")));

        assert_eq!(result.audio, FileDataContent::Bytes(Vec::new()));
        assert_eq!(
            result.provider_metadata,
            Some(
                serde_json::from_value(json!({
                    "deepgram": {
                        "errorMessage": "bad request"
                    }
                }))
                .expect("metadata deserializes")
            )
        );
        assert!(
            !captured_requests
                .lock()
                .expect("captured requests mutex is not poisoned")
                .is_empty()
        );
    }

    #[test]
    fn deepgram_speech_model_includes_request_body_in_response() {
        let (captured_requests, transport) =
            capture_transport(vec![ProviderApiResponse::bytes(200, "OK", vec![9])]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aura-2-helena-en");

        let result = poll_ready(model.do_generate(SpeechModelCallOptions::new("Hello.")));

        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref()),
            Some(&json!({
                "text": "Hello."
            }))
        );
        assert_eq!(result.response.model_id, "aura-2-helena-en");
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert!(
            !captured_requests
                .lock()
                .expect("captured requests mutex is not poisoned")
                .is_empty()
        );
    }

    #[test]
    fn deepgram_speech_model_cleans_up_incompatible_parameters_when_encoding_changes_via_provider_options()
     {
        let (captured_requests, transport) =
            capture_transport(vec![ProviderApiResponse::bytes(200, "OK", vec![9])]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aura-2-helena-en");

        let _ = poll_ready(
            model.do_generate(
                SpeechModelCallOptions::new("Hello, welcome to Deepgram!")
                    .with_output_format("linear16_16000")
                    .with_provider_options(
                        serde_json::from_value(json!({
                            "deepgram": {
                                "encoding": "mp3"
                            }
                        }))
                        .expect("provider options deserialize"),
                    ),
            ),
        );

        let request = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .first()
            .cloned()
            .expect("request is captured");
        assert!(request.url.contains("encoding=mp3"));
        assert!(!request.url.contains("sample_rate="));

        let (captured_requests, transport) =
            capture_transport(vec![ProviderApiResponse::bytes(200, "OK", vec![9])]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aura-2-helena-en");

        let _ = poll_ready(
            model.do_generate(
                SpeechModelCallOptions::new("Hello, welcome to Deepgram!")
                    .with_output_format("mp3")
                    .with_provider_options(
                        serde_json::from_value(json!({
                            "deepgram": {
                                "encoding": "linear16",
                                "bitRate": 48000
                            }
                        }))
                        .expect("provider options deserialize"),
                    ),
            ),
        );

        let request = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .first()
            .cloned()
            .expect("request is captured");
        assert!(request.url.contains("encoding=linear16"));
        assert!(!request.url.contains("bit_rate="));
    }

    #[test]
    fn deepgram_speech_model_cleans_up_incompatible_parameters_when_container_changes_encoding_implicitly()
     {
        let (captured_requests, transport) =
            capture_transport(vec![ProviderApiResponse::bytes(200, "OK", vec![9])]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aura-2-helena-en");

        let _ = poll_ready(
            model.do_generate(
                SpeechModelCallOptions::new("Hello, welcome to Deepgram!")
                    .with_output_format("linear16_16000")
                    .with_provider_options(
                        serde_json::from_value(json!({
                            "deepgram": {
                                "container": "ogg"
                            }
                        }))
                        .expect("provider options deserialize"),
                    ),
            ),
        );

        let request = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .first()
            .cloned()
            .expect("request is captured");
        assert!(request.url.contains("encoding=opus"));
        assert!(request.url.contains("container=ogg"));
        assert!(!request.url.contains("sample_rate="));
    }

    #[test]
    fn deepgram_transcription_model_uses_real_date_when_no_custom_date_provider_is_specified() {
        let (captured_requests, transport) = capture_transport(vec![ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "metadata": {
                    "duration": 1.0
                },
                "results": {
                    "channels": [
                        {
                            "alternatives": [
                                {
                                    "transcript": "hello",
                                    "words": []
                                }
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .transcription("nova-3");
        let before = OffsetDateTime::now_utc();
        let result = poll_ready(model.do_generate(TranscriptionModelCallOptions::new(
            FileDataContent::Bytes(vec![1, 2, 3]),
            "audio/wav",
        )));
        let after = OffsetDateTime::now_utc();

        assert!(result.response.timestamp >= before);
        assert!(result.response.timestamp <= after);
        assert_eq!(result.response.model_id, "nova-3");
        assert_eq!(result.text, "hello");
        assert!(
            !captured_requests
                .lock()
                .expect("captured requests mutex is not poisoned")
                .is_empty()
        );
    }

    #[test]
    fn deepgram_transcription_model_returns_detected_language_from_inline_response() {
        let (captured_requests, transport) = capture_transport(vec![ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "metadata": {
                    "duration": 1.0
                },
                "results": {
                    "channels": [
                        {
                            "detected_language": "sv",
                            "alternatives": [
                                {
                                    "transcript": "hej",
                                    "words": []
                                }
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .transcription("nova-3");

        let result = poll_ready(model.do_generate(TranscriptionModelCallOptions::new(
            FileDataContent::Bytes(vec![1, 2, 3]),
            "audio/wav",
        )));

        assert_eq!(result.language.as_deref(), Some("sv"));
        assert_eq!(result.text, "hej");
        assert!(
            !captured_requests
                .lock()
                .expect("captured requests mutex is not poisoned")
                .is_empty()
        );
    }

    #[test]
    fn deepgram_transcription_model_returns_undefined_language_when_not_detected() {
        let (captured_requests, transport) = capture_transport(vec![ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "metadata": {
                    "duration": 1.0
                },
                "results": {
                    "channels": [
                        {
                            "alternatives": [
                                {
                                    "transcript": "hello",
                                    "words": []
                                }
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )]);
        let model = DeepgramProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .transcription("nova-3");

        let result = poll_ready(model.do_generate(TranscriptionModelCallOptions::new(
            FileDataContent::Bytes(vec![1, 2, 3]),
            "audio/wav",
        )));

        assert_eq!(result.language, None);
        assert_eq!(result.text, "hello");
        assert!(
            !captured_requests
                .lock()
                .expect("captured requests mutex is not poisoned")
                .is_empty()
        );
    }

    #[test]
    fn deepgram_error_parses_resource_exhausted_error() {
        let parsed = super::deepgram_error_data(&json!({
            "error": {
                "message": "rate limit exceeded",
                "code": 429
            }
        }))
        .expect("error parses");

        assert_eq!(parsed.error.message, "rate limit exceeded");
        assert_eq!(parsed.error.code, 429);
    }

    #[test]
    fn deepgram_provider_reports_unsupported_model_families_and_traits() {
        let provider = DeepgramProvider::new().with_api_key("test-api-key");

        let language_error = Provider::language_model(&provider, "gpt")
            .err()
            .expect("language models are unsupported");
        assert_eq!(language_error.model_type(), ModelType::LanguageModel);
        assert_eq!(
            language_error.message(),
            "Deepgram does not provide language models"
        );

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            embedding_error.message(),
            "Deepgram does not provide text embedding models"
        );
        let text_embedding_error = provider
            .text_embedding_model("embed")
            .err()
            .expect("text embedding models are unsupported");
        assert_eq!(text_embedding_error.model_type(), ModelType::EmbeddingModel);

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(
            image_error.message(),
            "Deepgram does not provide image models"
        );

        let speech_model = ProviderWithSpeechModel::speech_model(&provider, "aura-2-helena-en")
            .expect("speech model resolves");
        assert_eq!(speech_model.provider(), "deepgram.speech");
        assert_eq!(speech_model.model_id(), "aura-2-helena-en");

        let transcription_model =
            ProviderWithTranscriptionModel::transcription_model(&provider, "nova-3")
                .expect("transcription model resolves");
        assert_eq!(transcription_model.provider(), "deepgram.transcription");
        assert_eq!(transcription_model.model_id(), "nova-3");
    }

    #[test]
    fn deepgram_provider_settings_serde_accepts_upstream_shape_and_default_factory() {
        let settings: DeepgramProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "headers": {
                "x-test": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(settings.headers.get("x-test"), Some(&"value".to_string()));

        let serialized = serde_json::to_value(settings).expect("settings serialize");
        assert_eq!(
            serialized,
            json!({
                "apiKey": "key",
                "headers": {
                    "x-test": "value"
                }
            })
        );

        let model = deepgram("nova-3");
        assert_eq!(model.provider(), "deepgram.transcription");
        assert_eq!(model.model_id(), "nova-3");
    }
}
