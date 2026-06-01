use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, GetFromApiOptions, HandledFetchError, Headers, JsonObject,
    JsonValue, LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    PostJsonToApiOptions, PostToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderWithTranscriptionModel, RuntimeEnvironment, TranscriptionModel,
    TranscriptionModelCallOptions, TranscriptionModelResponse, TranscriptionModelResult,
    TranscriptionModelSegment, Warning, combine_headers, convert_base64_to_bytes,
    create_json_error_response_handler, create_json_response_handler, delay, get_from_api,
    load_api_key, media_type_to_extension, parse_provider_options, post_json_to_api, post_to_api,
    with_user_agent_suffix,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/gladia` API calls.
pub const DEFAULT_GLADIA_BASE_URL: &str = "https://api.gladia.io";

/// Default Gladia transcription model id used by upstream.
pub const DEFAULT_GLADIA_TRANSCRIPTION_MODEL_ID: &str = "default";

/// Default polling interval used by upstream Gladia transcription.
pub const DEFAULT_GLADIA_POLLING_INTERVAL_MILLIS: u64 = 1_000;

/// Default polling timeout used by upstream Gladia transcription.
pub const DEFAULT_GLADIA_POLLING_TIMEOUT_MILLIS: u64 = 60_000;

/// Provider-specific Gladia transcription options.
pub type GladiaTranscriptionModelOptions = JsonObject;

/// Settings for the upstream Gladia provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GladiaProviderSettings {
    /// Gladia API key. When omitted, `GLADIA_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl GladiaProviderSettings {
    /// Creates empty Gladia provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Gladia API key.
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

/// Upstream Gladia provider foundation.
#[derive(Clone)]
pub struct GladiaProvider {
    settings: GladiaProviderSettings,
    transport: GladiaTransport,
    current_date: GladiaDateProvider,
}

/// Gladia transcription model for upload/initiate/poll calls.
#[derive(Clone)]
pub struct GladiaTranscriptionModel {
    model_id: String,
    settings: GladiaProviderSettings,
    transport: GladiaTransport,
    current_date: GladiaDateProvider,
}

/// Future returned by an injected Gladia HTTP transport.
pub type GladiaTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Gladia provider models.
pub type GladiaTransport = Arc<dyn Fn(ProviderApiRequest) -> GladiaTransportFuture + Send + Sync>;

type GladiaDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type GladiaTranscriptionGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>;

impl GladiaProvider {
    /// Creates a Gladia provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(GladiaProviderSettings::new())
    }

    /// Creates a provider from explicit Gladia settings.
    pub fn from_settings(settings: GladiaProviderSettings) -> Self {
        Self {
            settings,
            transport: default_gladia_transport(),
            current_date: default_gladia_date_provider(),
        }
    }

    /// Sets the Gladia API key for this provider.
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
    pub fn with_transport(mut self, transport: GladiaTransport) -> Self {
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

    /// Creates the default Gladia transcription model.
    pub fn transcription(&self) -> GladiaTranscriptionModel {
        self.transcription_model(DEFAULT_GLADIA_TRANSCRIPTION_MODEL_ID)
            .expect("Gladia default transcription model is supported")
    }

    /// Creates a transcription model. Upstream exposes only the `default` model id.
    pub fn transcription_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GladiaTranscriptionModel, NoSuchModelError> {
        let model_id = model_id.into();
        if model_id != DEFAULT_GLADIA_TRANSCRIPTION_MODEL_ID {
            return Err(NoSuchModelError::with_message(
                model_id,
                ModelType::TranscriptionModel,
                "Gladia only provides the default transcription model",
            ));
        }

        Ok(GladiaTranscriptionModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Gladia does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "Gladia does not provide language models",
        ))
    }

    /// Reports that Gladia does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            "Gladia does not provide embedding models",
        ))
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Gladia does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            "Gladia does not provide image models",
        ))
    }
}

impl Default for GladiaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GladiaProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        GladiaProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        GladiaProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        GladiaProvider::image_model(self, model_id)
    }
}

impl ProviderWithTranscriptionModel for GladiaProvider {
    type TranscriptionModel = GladiaTranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        GladiaProvider::transcription_model(self, model_id)
    }
}

impl GladiaTranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        settings: GladiaProviderSettings,
        transport: GladiaTransport,
        current_date: GladiaDateProvider,
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
        "gladia.transcription"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: GladiaTransport) -> Self {
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
        let (upload_form_data, warnings) = match gladia_upload_form_data(&options) {
            Ok(args) => args,
            Err(message) => {
                return gladia_transcription_result_from_error(
                    &self.model_id,
                    message,
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
                return gladia_transcription_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let (content_type, body) = multipart_body(&upload_form_data, Some(&options.media_type));
        let upload_options = PostToApiOptions::new(
            gladia_url("/v2/upload"),
            ProviderApiRequestBody::bytes(body),
            form_data_request_body_values(&upload_form_data),
        )
        .with_headers(request_headers.clone())
        .with_header("content-type", content_type)
        .with_optional_abort_signal(options.abort_signal.clone())
        .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let upload_response = match post_to_api(
            upload_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    gladia_upload_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    gladia_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers, body) = handled_error_parts(error);
                return gladia_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    warnings,
                    timestamp,
                );
            }
        };

        let initiate_body =
            match gladia_transcription_initiate_body(upload_response.value.audio_url, &options) {
                Ok(body) => body,
                Err(error) => {
                    return gladia_transcription_result_from_error(
                        &self.model_id,
                        error.to_string(),
                        None,
                        None,
                        warnings,
                        timestamp,
                    );
                }
            };
        let initiate_options =
            PostJsonToApiOptions::new(gladia_url("/v2/pre-recorded"), initiate_body)
                .with_headers(request_headers.clone())
                .with_optional_abort_signal(options.abort_signal.clone())
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let initiate_response = match post_json_to_api(
            initiate_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    gladia_initiate_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    gladia_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers, body) = handled_error_parts(error);
                return gladia_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    warnings,
                    timestamp,
                );
            }
        };

        match self
            .wait_for_result(
                &initiate_response.value.result_url,
                &request_headers,
                options.abort_signal.clone(),
            )
            .await
        {
            Ok(response) => gladia_transcription_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                response.raw_value,
                warnings,
                timestamp,
            ),
            Err(GladiaResultError::Handled(error)) => {
                let (message, headers, body) = handled_error_parts(error);
                gladia_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    warnings,
                    timestamp,
                )
            }
            Err(GladiaResultError::Provider {
                message,
                result,
                headers,
                raw_value,
            }) => gladia_transcription_result_from_error(
                &self.model_id,
                message,
                headers,
                raw_value.or_else(|| result.map(|result| serde_json::to_value(result).unwrap())),
                warnings,
                timestamp,
            ),
        }
    }

    async fn wait_for_result(
        &self,
        result_url: &str,
        headers: &BTreeMap<String, Option<String>>,
        abort_signal: Option<ai_sdk_rust::ProviderAbortSignal>,
    ) -> Result<ai_sdk_rust::ResponseHandlerResult<GladiaResultResponse>, GladiaResultError> {
        let started = Instant::now();

        loop {
            if started.elapsed().as_millis() > u128::from(DEFAULT_GLADIA_POLLING_TIMEOUT_MILLIS) {
                return Err(GladiaResultError::Provider {
                    message: "Transcription job polling timed out".to_string(),
                    result: None,
                    headers: None,
                    raw_value: None,
                });
            }

            let transport = Arc::clone(&self.transport);
            let response = get_from_api(
                GetFromApiOptions::new(result_url)
                    .with_headers(headers.clone())
                    .with_optional_abort_signal(abort_signal.clone())
                    .with_environment(RuntimeEnvironment::unknown()),
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        gladia_result_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        gladia_error_data,
                        |data| data.error.message.clone(),
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(GladiaResultError::Handled)?;

            match response.value.status.as_str() {
                "done" => return Ok(response),
                "error" => {
                    return Err(GladiaResultError::Provider {
                        message: "Transcription job failed".to_string(),
                        result: Some(response.value),
                        headers: response.response_headers,
                        raw_value: response.raw_value,
                    });
                }
                _ => delay(Some(DEFAULT_GLADIA_POLLING_INTERVAL_MILLIS as i64)).await,
            }
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(gladia_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl TranscriptionModel for GladiaTranscriptionModel {
    type GenerateFuture<'a>
        = GladiaTranscriptionGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        GladiaTranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &str {
        GladiaTranscriptionModel::model_id(self)
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Gladia provider with explicit settings.
pub fn create_gladia(settings: GladiaProviderSettings) -> GladiaProvider {
    GladiaProvider::from_settings(settings)
}

/// Creates a Gladia transcription model using the default provider settings.
pub fn gladia() -> GladiaTranscriptionModel {
    GladiaProvider::new().transcription()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaUploadResponse {
    audio_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaInitiateResponse {
    result_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaResultResponse {
    status: String,
    #[serde(default)]
    result: Option<GladiaResultData>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaResultData {
    metadata: GladiaResultMetadata,
    transcription: GladiaTranscriptionData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaResultMetadata {
    audio_duration: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaTranscriptionData {
    full_transcript: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    utterances: Vec<GladiaUtterance>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaUtterance {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaErrorData {
    error: GladiaErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GladiaErrorBody {
    message: String,
    code: i64,
}

enum GladiaResultError {
    Handled(HandledFetchError),
    Provider {
        message: String,
        result: Option<GladiaResultResponse>,
        headers: Option<Headers>,
        raw_value: Option<JsonValue>,
    },
}

fn gladia_upload_form_data(
    options: &TranscriptionModelCallOptions,
) -> Result<(ai_sdk_rust::FormData, Vec<Warning>), String> {
    let audio = audio_bytes(&options.audio)?;
    let mut form_data = ai_sdk_rust::FormData::new();
    form_data.append("audio", ai_sdk_rust::FormDataValue::bytes(audio));
    Ok((form_data, Vec::new()))
}

fn gladia_transcription_initiate_body(
    audio_url: String,
    options: &TranscriptionModelCallOptions,
) -> Result<JsonValue, ai_sdk_rust::InvalidArgumentError> {
    let gladia_options = parse_provider_options(
        "gladia",
        options.provider_options.as_ref(),
        gladia_transcription_model_options,
    )?;
    let mut body = JsonObject::new();

    if let Some(gladia_options) = gladia_options.as_ref() {
        apply_gladia_transcription_provider_options(gladia_options, &mut body);
    }

    body.insert("audio_url".to_string(), JsonValue::String(audio_url));

    Ok(JsonValue::Object(body))
}

fn apply_gladia_transcription_provider_options(
    options: &GladiaTranscriptionModelOptions,
    body: &mut JsonObject,
) {
    insert_mapped_option(body, "context_prompt", options.get("contextPrompt"));
    insert_mapped_option(body, "custom_vocabulary", options.get("customVocabulary"));
    insert_mapped_option(body, "detect_language", options.get("detectLanguage"));
    insert_mapped_option(
        body,
        "enable_code_switching",
        options.get("enableCodeSwitching"),
    );
    insert_mapped_option(body, "language", options.get("language"));
    insert_mapped_option(body, "callback", options.get("callback"));
    insert_mapped_option(body, "subtitles", options.get("subtitles"));
    insert_mapped_option(body, "diarization", options.get("diarization"));
    insert_mapped_option(body, "translation", options.get("translation"));
    insert_mapped_option(body, "summarization", options.get("summarization"));
    insert_mapped_option(body, "moderation", options.get("moderation"));
    insert_mapped_option(
        body,
        "named_entity_recognition",
        options.get("namedEntityRecognition"),
    );
    insert_mapped_option(body, "chapterization", options.get("chapterization"));
    insert_mapped_option(body, "name_consistency", options.get("nameConsistency"));
    insert_mapped_option(body, "custom_spelling", options.get("customSpelling"));
    insert_mapped_option(
        body,
        "structured_data_extraction",
        options.get("structuredDataExtraction"),
    );
    insert_mapped_option(
        body,
        "structured_data_extraction_config",
        options.get("structuredDataExtractionConfig"),
    );
    insert_mapped_option(body, "sentiment_analysis", options.get("sentimentAnalysis"));
    insert_mapped_option(body, "audio_to_llm", options.get("audioToLlm"));
    insert_mapped_option(body, "audio_to_llm_config", options.get("audioToLlmConfig"));
    insert_mapped_option(body, "custom_metadata", options.get("customMetadata"));
    insert_mapped_option(body, "sentences", options.get("sentences"));
    insert_mapped_option(body, "display_mode", options.get("displayMode"));
    insert_mapped_option(
        body,
        "punctuation_enhanced",
        options.get("punctuationEnhanced"),
    );

    if let Some(config) = custom_vocabulary_config(options.get("customVocabularyConfig")) {
        body.insert("custom_vocabulary_config".to_string(), config);
    }
    if let Some(config) = object_with_mapped_fields(
        options.get("codeSwitchingConfig"),
        &[("languages", "languages")],
    ) {
        body.insert("code_switching_config".to_string(), config);
    }
    if let Some(config) = object_with_mapped_fields(
        options.get("callbackConfig"),
        &[("url", "url"), ("method", "method")],
    ) {
        body.insert("callback_config".to_string(), config);
    }
    if let Some(config) = object_with_mapped_fields(
        options.get("subtitlesConfig"),
        &[
            ("formats", "formats"),
            ("minimumDuration", "minimum_duration"),
            ("maximumDuration", "maximum_duration"),
            ("maximumCharactersPerRow", "maximum_characters_per_row"),
            ("maximumRowsPerCaption", "maximum_rows_per_caption"),
            ("style", "style"),
        ],
    ) {
        body.insert("subtitles_config".to_string(), config);
    }
    if let Some(config) = object_with_mapped_fields(
        options.get("diarizationConfig"),
        &[
            ("numberOfSpeakers", "number_of_speakers"),
            ("minSpeakers", "min_speakers"),
            ("maxSpeakers", "max_speakers"),
            ("enhanced", "enhanced"),
        ],
    ) {
        body.insert("diarization_config".to_string(), config);
    }
    if let Some(config) = object_with_mapped_fields(
        options.get("translationConfig"),
        &[
            ("targetLanguages", "target_languages"),
            ("model", "model"),
            ("matchOriginalUtterances", "match_original_utterances"),
        ],
    ) {
        body.insert("translation_config".to_string(), config);
    }
    if let Some(config) =
        object_with_mapped_fields(options.get("summarizationConfig"), &[("type", "type")])
    {
        body.insert("summarization_config".to_string(), config);
    }
    if let Some(config) = object_with_mapped_fields(
        options.get("customSpellingConfig"),
        &[("spellingDictionary", "spelling_dictionary")],
    ) {
        body.insert("custom_spelling_config".to_string(), config);
    }
}

fn gladia_transcription_model_options(
    value: &JsonValue,
) -> Result<GladiaTranscriptionModelOptions, String> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "Gladia transcription provider options must be an object".to_string())
}

fn custom_vocabulary_config(value: Option<&JsonValue>) -> Option<JsonValue> {
    let config = value?.as_object()?;
    let mut mapped = JsonObject::new();

    if let Some(vocabulary) = config.get("vocabulary").and_then(JsonValue::as_array) {
        let vocabulary = vocabulary
            .iter()
            .map(|item| {
                if item.is_string() {
                    return item.clone();
                }

                let mut mapped_item = JsonObject::new();
                if let Some(item) = item.as_object() {
                    insert_mapped_option(&mut mapped_item, "value", item.get("value"));
                    insert_mapped_option(&mut mapped_item, "intensity", item.get("intensity"));
                    insert_mapped_option(
                        &mut mapped_item,
                        "pronunciations",
                        item.get("pronunciations"),
                    );
                    insert_mapped_option(&mut mapped_item, "language", item.get("language"));
                }
                JsonValue::Object(mapped_item)
            })
            .collect::<Vec<_>>();
        mapped.insert("vocabulary".to_string(), JsonValue::Array(vocabulary));
    }

    insert_mapped_option(
        &mut mapped,
        "default_intensity",
        config.get("defaultIntensity"),
    );

    Some(JsonValue::Object(mapped))
}

fn object_with_mapped_fields(
    value: Option<&JsonValue>,
    fields: &[(&str, &str)],
) -> Option<JsonValue> {
    let object = value?.as_object()?;
    let mut mapped = JsonObject::new();

    for (source, target) in fields {
        insert_mapped_option(&mut mapped, target, object.get(*source));
    }

    Some(JsonValue::Object(mapped))
}

fn insert_mapped_option(body: &mut JsonObject, name: &str, value: Option<&JsonValue>) {
    if let Some(value) = value.filter(|value| !value.is_null()) {
        body.insert(name.to_string(), value.clone());
    }
}

fn gladia_provider_header_entries(
    settings: &GladiaProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "x-gladia-key".to_string(),
        Some(gladia_api_key(settings.api_key.as_ref())?),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/gladia/{}", ai_sdk_rust::VERSION)],
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

fn gladia_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("GLADIA_API_KEY", "Gladia");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn gladia_url(path: &str) -> String {
    format!("{DEFAULT_GLADIA_BASE_URL}{path}")
}

fn gladia_upload_response(value: &JsonValue) -> Result<GladiaUploadResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn gladia_initiate_response(
    value: &JsonValue,
) -> Result<GladiaInitiateResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn gladia_result_response(value: &JsonValue) -> Result<GladiaResultResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn gladia_error_data(value: &JsonValue) -> Result<GladiaErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn gladia_transcription_result_from_response(
    model_id: &str,
    response_value: GladiaResultResponse,
    response_headers: Option<Headers>,
    raw_value: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body =
        raw_value.unwrap_or_else(|| serde_json::to_value(&response_value).expect("serializes"));
    let provider_metadata = provider_metadata("gladia", response_body.clone());
    let Some(result_data) = response_value.result else {
        return gladia_transcription_result_from_error(
            model_id,
            "Transcription result is empty".to_string(),
            response_headers,
            Some(response_body),
            warnings,
            timestamp,
        );
    };
    let mut response = TranscriptionModelResponse::new(timestamp, model_id);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let segments = result_data
        .transcription
        .utterances
        .into_iter()
        .map(|utterance| {
            TranscriptionModelSegment::new(utterance.text, utterance.start, utterance.end)
        })
        .collect::<Vec<_>>();
    let mut result = TranscriptionModelResult::new(
        result_data.transcription.full_transcript,
        segments,
        response,
    )
    .with_duration_in_seconds(result_data.metadata.audio_duration)
    .with_provider_metadata(provider_metadata);

    if let Some(language) = result_data.transcription.languages.into_iter().next() {
        result = result.with_language(language);
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn gladia_transcription_result_from_error(
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
        .with_provider_metadata(error_metadata("gladia", message));

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

fn provider_metadata(provider_name: &str, value: JsonValue) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let object = value.as_object().cloned().unwrap_or_default();
    metadata.insert(provider_name.to_string(), object);
    metadata
}

fn audio_bytes(audio: &FileDataContent) -> Result<Vec<u8>, String> {
    match audio {
        FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
        FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
            .map_err(|error| format!("invalid base64 transcription audio: {error}")),
    }
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
    let boundary = "----ai-sdk-rust-gladia-boundary";
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

fn default_gladia_date_provider() -> GladiaDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_gladia_transport() -> GladiaTransport {
    Arc::new(|request| Box::pin(ready(execute_gladia_request(request))))
}

fn execute_gladia_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_gladia_get_request(request),
        ProviderApiRequestMethod::Post => execute_gladia_post_request(request),
    }
}

fn execute_gladia_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    gladia_provider_api_response(response)
}

fn execute_gladia_post_request(
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

    gladia_provider_api_response(response)
}

fn gladia_provider_api_response(
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
        GladiaProviderSettings, GladiaTransport, GladiaTransportFuture, create_gladia, gladia,
    };
    use ai_sdk_rust::{
        FileDataContent, ModelType, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions,
        ProviderWithTranscriptionModel, TranscriptionModel, TranscriptionModelCallOptions,
        TranscriptionModelSegment,
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

    fn capture_transport<F>(handler: F) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, GladiaTransport)
    where
        F: Fn(&ProviderApiRequest) -> ProviderApiResponse + Send + Sync + 'static,
    {
        let requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured = Arc::clone(&requests);
        let handler = Arc::new(handler);
        let transport = Arc::new(
            move |request: ProviderApiRequest| -> GladiaTransportFuture {
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

    fn upload_fixture() -> serde_json::Value {
        json!({
            "audio_url": "https://api.gladia.io/file/audio-123",
            "audio_metadata": {
                "id": "audio-123"
            }
        })
    }

    fn initiate_fixture() -> serde_json::Value {
        json!({
            "id": "job-123",
            "result_url": "https://api.gladia.io/v2/pre-recorded/job-123"
        })
    }

    fn result_fixture() -> serde_json::Value {
        json!({
            "id": "job-123",
            "status": "done",
            "result": {
                "metadata": {
                    "audio_duration": 36.74
                },
                "transcription": {
                    "languages": ["en"],
                    "utterances": [
                        {
                            "text": "Galileo was an American robotic space program.",
                            "start": 0.14,
                            "end": 5.341
                        },
                        {
                            "text": "It studied Jupiter.",
                            "start": 5.662,
                            "end": 8.099
                        }
                    ],
                    "full_transcript": "Galileo was an American robotic space program. It studied Jupiter."
                }
            }
        })
    }

    #[test]
    fn gladia_transcription_model_uploads_initiates_polls_and_maps_response() {
        let upload = upload_fixture();
        let initiate = initiate_fixture();
        let result = result_fixture();
        let (requests, transport) = capture_transport(move |request| match request.url.as_str() {
            "https://api.gladia.io/v2/upload" => json_response(upload.clone()),
            "https://api.gladia.io/v2/pre-recorded" => json_response(initiate.clone()),
            "https://api.gladia.io/v2/pre-recorded/job-123" => json_response(result.clone())
                .with_headers(ai_sdk_rust::Headers::from([(
                    "x-request-id".to_string(),
                    "req_poll".to_string(),
                )])),
            other => panic!("unexpected request url {other}"),
        });
        let provider = create_gladia(
            GladiaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-value"),
        )
        .with_transport(transport)
        .with_current_date(|| OffsetDateTime::UNIX_EPOCH);
        let response = poll_ready(
            provider.transcription().do_generate(
                TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![82, 73, 70, 70]),
                    "audio/wav",
                )
                .with_header("Custom-Request-Header", "request-value"),
            ),
        );
        let requests = requests.lock().expect("requests lock");

        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(requests[0].url, "https://api.gladia.io/v2/upload");
        assert_eq!(
            requests[0].headers.get("x-gladia-key"),
            Some(&"test-api-key".to_string())
        );
        assert!(
            requests[0]
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/gladia/"))
        );
        assert_eq!(
            requests[0].headers.get("custom-provider-header"),
            Some(&"provider-value".to_string())
        );
        assert_eq!(
            requests[0].headers.get("custom-request-header"),
            Some(&"request-value".to_string())
        );
        assert_eq!(
            requests[0].request_body_values,
            json!({ "audio": [82, 73, 70, 70] })
        );
        let ProviderApiRequestBody::Bytes { content } =
            requests[0].body.as_ref().expect("multipart body")
        else {
            panic!("expected bytes body");
        };
        let body = String::from_utf8_lossy(content);
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.contains("content-type: audio/wav"));
        assert_eq!(
            request_json_body(&requests[1]),
            json!({ "audio_url": "https://api.gladia.io/file/audio-123" })
        );
        assert_eq!(requests[2].method, ProviderApiRequestMethod::Get);
        assert_eq!(
            response.text,
            "Galileo was an American robotic space program. It studied Jupiter."
        );
        assert_eq!(response.language.as_deref(), Some("en"));
        assert_eq!(response.duration_in_seconds, Some(36.74));
        assert_eq!(
            response.segments,
            vec![
                TranscriptionModelSegment::new(
                    "Galileo was an American robotic space program.",
                    0.14,
                    5.341
                ),
                TranscriptionModelSegment::new("It studied Jupiter.", 5.662, 8.099),
            ]
        );
        assert_eq!(response.response.timestamp, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(response.response.model_id, "default");
        assert_eq!(
            response
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req_poll".to_string())
        );
        assert_eq!(
            response
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gladia"))
                .and_then(|metadata| metadata.get("status"))
                .and_then(serde_json::Value::as_str),
            Some("done")
        );
    }

    #[test]
    fn gladia_transcription_model_maps_provider_options_to_initiate_body() {
        let upload = upload_fixture();
        let initiate = initiate_fixture();
        let result = result_fixture();
        let (requests, transport) = capture_transport(move |request| match request.url.as_str() {
            "https://api.gladia.io/v2/upload" => json_response(upload.clone()),
            "https://api.gladia.io/v2/pre-recorded" => json_response(initiate.clone()),
            "https://api.gladia.io/v2/pre-recorded/job-123" => json_response(result.clone()),
            other => panic!("unexpected request url {other}"),
        });
        let provider = create_gladia(GladiaProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "gladia": {
                "contextPrompt": "Jupiter mission",
                "customVocabulary": true,
                "customVocabularyConfig": {
                    "vocabulary": [
                        "Galileo",
                        {
                            "value": "Jupiter",
                            "intensity": 0.8,
                            "pronunciations": ["joo-pi-ter"],
                            "language": "en"
                        }
                    ],
                    "defaultIntensity": 0.4
                },
                "detectLanguage": true,
                "enableCodeSwitching": true,
                "codeSwitchingConfig": {
                    "languages": ["en", "es"]
                },
                "language": "en",
                "callback": true,
                "callbackConfig": {
                    "url": "https://example.com/callback",
                    "method": "POST"
                },
                "subtitles": true,
                "subtitlesConfig": {
                    "formats": ["srt", "vtt"],
                    "minimumDuration": 1,
                    "maximumDuration": 4,
                    "maximumCharactersPerRow": 42,
                    "maximumRowsPerCaption": 2,
                    "style": "default"
                },
                "diarization": true,
                "diarizationConfig": {
                    "numberOfSpeakers": 2,
                    "minSpeakers": 1,
                    "maxSpeakers": 3,
                    "enhanced": true
                },
                "translation": true,
                "translationConfig": {
                    "targetLanguages": ["fr"],
                    "model": "base",
                    "matchOriginalUtterances": true
                },
                "summarization": true,
                "summarizationConfig": {
                    "type": "bullet_points"
                },
                "moderation": true,
                "namedEntityRecognition": true,
                "chapterization": true,
                "nameConsistency": true,
                "customSpelling": true,
                "customSpellingConfig": {
                    "spellingDictionary": {
                        "Jupiter": ["Jupitr"]
                    }
                },
                "structuredDataExtraction": true,
                "structuredDataExtractionConfig": {
                    "classes": ["planet"]
                },
                "sentimentAnalysis": true,
                "audioToLlm": true,
                "audioToLlmConfig": {
                    "prompts": ["summarize"]
                },
                "customMetadata": {
                    "source": "fixture"
                },
                "sentences": true,
                "displayMode": true,
                "punctuationEnhanced": true
            }
        }))
        .expect("provider options");

        let _ = poll_ready(
            provider.transcription().do_generate(
                TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/mpeg")
                    .with_provider_options(provider_options),
            ),
        );
        let requests = requests.lock().expect("requests lock");

        assert_eq!(
            request_json_body(&requests[1]),
            json!({
                "audio_url": "https://api.gladia.io/file/audio-123",
                "context_prompt": "Jupiter mission",
                "custom_vocabulary": true,
                "custom_vocabulary_config": {
                    "vocabulary": [
                        "Galileo",
                        {
                            "value": "Jupiter",
                            "intensity": 0.8,
                            "pronunciations": ["joo-pi-ter"],
                            "language": "en"
                        }
                    ],
                    "default_intensity": 0.4
                },
                "detect_language": true,
                "enable_code_switching": true,
                "code_switching_config": {
                    "languages": ["en", "es"]
                },
                "language": "en",
                "callback": true,
                "callback_config": {
                    "url": "https://example.com/callback",
                    "method": "POST"
                },
                "subtitles": true,
                "subtitles_config": {
                    "formats": ["srt", "vtt"],
                    "minimum_duration": 1,
                    "maximum_duration": 4,
                    "maximum_characters_per_row": 42,
                    "maximum_rows_per_caption": 2,
                    "style": "default"
                },
                "diarization": true,
                "diarization_config": {
                    "number_of_speakers": 2,
                    "min_speakers": 1,
                    "max_speakers": 3,
                    "enhanced": true
                },
                "translation": true,
                "translation_config": {
                    "target_languages": ["fr"],
                    "model": "base",
                    "match_original_utterances": true
                },
                "summarization": true,
                "summarization_config": {
                    "type": "bullet_points"
                },
                "moderation": true,
                "named_entity_recognition": true,
                "chapterization": true,
                "name_consistency": true,
                "custom_spelling": true,
                "custom_spelling_config": {
                    "spelling_dictionary": {
                        "Jupiter": ["Jupitr"]
                    }
                },
                "structured_data_extraction": true,
                "structured_data_extraction_config": {
                    "classes": ["planet"]
                },
                "sentiment_analysis": true,
                "audio_to_llm": true,
                "audio_to_llm_config": {
                    "prompts": ["summarize"]
                },
                "custom_metadata": {
                    "source": "fixture"
                },
                "sentences": true,
                "display_mode": true,
                "punctuation_enhanced": true
            })
        );
    }

    #[test]
    fn gladia_transcription_model_maps_api_and_status_errors_to_metadata() {
        let (requests, transport) = capture_transport(|request| match request.url.as_str() {
            "https://api.gladia.io/v2/upload" => ProviderApiResponse::text(
                429,
                "Too Many Requests",
                r#"{"error":{"message":"Resource has been exhausted (e.g. check quota).","code":429}}"#,
            ),
            other => panic!("unexpected request url {other}"),
        });
        let provider = create_gladia(GladiaProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport);
        let result = poll_ready(provider.transcription().do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
        ));

        assert_eq!(requests.lock().expect("requests lock").len(), 1);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gladia"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(serde_json::Value::as_str),
            Some("Resource has been exhausted (e.g. check quota).")
        );

        let upload = upload_fixture();
        let initiate = initiate_fixture();
        let (requests, transport) = capture_transport(move |request| match request.url.as_str() {
            "https://api.gladia.io/v2/upload" => json_response(upload.clone()),
            "https://api.gladia.io/v2/pre-recorded" => json_response(initiate.clone()),
            "https://api.gladia.io/v2/pre-recorded/job-123" => {
                json_response(json!({ "status": "error", "result": null }))
            }
            other => panic!("unexpected request url {other}"),
        });
        let provider = create_gladia(GladiaProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport);
        let result = poll_ready(provider.transcription().do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
        ));

        assert_eq!(requests.lock().expect("requests lock").len(), 3);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gladia"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(serde_json::Value::as_str),
            Some("Transcription job failed")
        );
    }

    #[test]
    fn gladia_provider_reports_unsupported_model_families_and_trait_transcription() {
        let provider = create_gladia(GladiaProviderSettings::new().with_api_key("test-api-key"));

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
            provider
                .transcription_model("other")
                .err()
                .expect("non-default transcription model unsupported")
                .model_type(),
            ModelType::TranscriptionModel
        );
        assert_eq!(
            ProviderWithTranscriptionModel::transcription_model(&provider, "default")
                .expect("transcription model")
                .provider(),
            "gladia.transcription"
        );
        assert_eq!(gladia().provider(), "gladia.transcription");
    }

    #[test]
    fn gladia_provider_settings_serde_accepts_upstream_shape() {
        let settings: GladiaProviderSettings = serde_json::from_value(json!({
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
