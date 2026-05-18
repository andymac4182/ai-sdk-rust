use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, GetFromApiOptions, HandledFetchError, Headers, JsonObject,
    JsonValue, LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    PostJsonToApiOptions, PostToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderWithTranscriptionModel, RuntimeEnvironment, TranscriptionModel,
    TranscriptionModelCallOptions, TranscriptionModelRequest, TranscriptionModelResponse,
    TranscriptionModelResult, TranscriptionModelSegment, Warning, combine_headers,
    convert_base64_to_bytes, create_json_error_response_handler, create_json_response_handler,
    delay, get_from_api, load_api_key, parse_provider_options, post_json_to_api, post_to_api,
    with_user_agent_suffix,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/assemblyai` API calls.
pub const DEFAULT_ASSEMBLYAI_BASE_URL: &str = "https://api.assemblyai.com";

/// Default polling interval used by upstream AssemblyAI transcription.
pub const DEFAULT_ASSEMBLYAI_POLLING_INTERVAL_MILLIS: u64 = 3_000;

/// Settings for the upstream AssemblyAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyAIProviderSettings {
    /// AssemblyAI API key. When omitted, `ASSEMBLYAI_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Poll interval in milliseconds between transcript status checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polling_interval: Option<u64>,
}

impl AssemblyAIProviderSettings {
    /// Creates empty AssemblyAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the AssemblyAI API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the polling interval in milliseconds.
    pub fn with_polling_interval(mut self, polling_interval: u64) -> Self {
        self.polling_interval = Some(polling_interval);
        self
    }
}

/// Upstream AssemblyAI provider foundation.
#[derive(Clone)]
pub struct AssemblyAIProvider {
    settings: AssemblyAIProviderSettings,
    transport: AssemblyAITransport,
    current_date: AssemblyAIDateProvider,
}

/// AssemblyAI transcription model.
#[derive(Clone)]
pub struct AssemblyAITranscriptionModel {
    model_id: String,
    settings: AssemblyAIProviderSettings,
    transport: AssemblyAITransport,
    current_date: AssemblyAIDateProvider,
}

/// Future returned by an injected AssemblyAI HTTP transport.
pub type AssemblyAITransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by AssemblyAI provider models.
pub type AssemblyAITransport =
    Arc<dyn Fn(ProviderApiRequest) -> AssemblyAITransportFuture + Send + Sync>;

type AssemblyAIDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type AssemblyAITranscriptionGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>;

impl AssemblyAIProvider {
    /// Creates an AssemblyAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(AssemblyAIProviderSettings::new())
    }

    /// Creates a provider from explicit AssemblyAI settings.
    pub fn from_settings(settings: AssemblyAIProviderSettings) -> Self {
        Self {
            settings,
            transport: default_assemblyai_transport(),
            current_date: default_assemblyai_date_provider(),
        }
    }

    /// Sets the AssemblyAI API key for this provider.
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
    pub fn with_transport(mut self, transport: AssemblyAITransport) -> Self {
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
    pub fn transcription(&self, model_id: impl Into<String>) -> AssemblyAITranscriptionModel {
        self.transcription_model(model_id)
            .expect("AssemblyAI transcription models are supported")
    }

    /// Creates a transcription model.
    pub fn transcription_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<AssemblyAITranscriptionModel, NoSuchModelError> {
        Ok(AssemblyAITranscriptionModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that AssemblyAI does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "AssemblyAI does not provide language models",
        ))
    }

    /// Reports that AssemblyAI does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that AssemblyAI does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for AssemblyAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AssemblyAIProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        AssemblyAIProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        AssemblyAIProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        AssemblyAIProvider::image_model(self, model_id)
    }
}

impl ProviderWithTranscriptionModel for AssemblyAIProvider {
    type TranscriptionModel = AssemblyAITranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        AssemblyAIProvider::transcription_model(self, model_id)
    }
}

impl AssemblyAITranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        settings: AssemblyAIProviderSettings,
        transport: AssemblyAITransport,
        current_date: AssemblyAIDateProvider,
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
        "assemblyai.transcription"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: AssemblyAITransport) -> Self {
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
        let audio = match assemblyai_audio_bytes(&options.audio) {
            Ok(audio) => audio,
            Err(message) => {
                return assemblyai_transcription_result_from_error(
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
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return assemblyai_transcription_result_from_error(
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
        let upload_headers = combine_headers([
            Some(vec![(
                "Content-Type".to_string(),
                Some("application/octet-stream".to_string()),
            )]),
            Some(request_headers.clone().into_iter().collect::<Vec<_>>()),
        ]);
        let upload_options = PostToApiOptions::new(
            assemblyai_url("/v2/upload"),
            ProviderApiRequestBody::Bytes { content: audio },
            JsonValue::Object(JsonObject::new()),
        )
        .with_headers(upload_headers)
        .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let upload = match post_to_api(
            upload_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    assemblyai_upload_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    assemblyai_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers, body) = assemblyai_handled_error_parts(error);
                return assemblyai_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    None,
                    Vec::new(),
                    timestamp,
                );
            }
        };

        let (mut submit_body, warnings) =
            match assemblyai_transcription_request_body(&self.model_id, &options) {
                Ok(args) => args,
                Err(message) => {
                    return assemblyai_transcription_result_from_error(
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
        let JsonValue::Object(submit_body_object) = &mut submit_body else {
            unreachable!("AssemblyAI submit body is always an object");
        };
        submit_body_object.insert(
            "audio_url".to_string(),
            JsonValue::String(upload.value.upload_url),
        );
        let request_body_json = serde_json::to_string(&submit_body)
            .expect("AssemblyAI submit body serializes for request metadata");
        let submit_options =
            PostJsonToApiOptions::new(assemblyai_url("/v2/transcript"), submit_body.clone())
                .with_headers(request_headers.clone())
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let submit = match post_json_to_api(
            submit_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    assemblyai_submit_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    assemblyai_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers, body) = assemblyai_handled_error_parts(error);
                return assemblyai_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    Some(request_body_json),
                    warnings,
                    timestamp,
                );
            }
        };

        match self
            .wait_for_completion(&submit.value.id, request_headers)
            .await
        {
            Ok((transcript, headers)) => assemblyai_transcription_result_from_response(
                &self.model_id,
                transcript,
                headers,
                Some(request_body_json),
                warnings,
                timestamp,
            ),
            Err(message) => assemblyai_transcription_result_from_error(
                &self.model_id,
                message,
                None,
                None,
                Some(request_body_json),
                warnings,
                timestamp,
            ),
        }
    }

    async fn wait_for_completion(
        &self,
        transcript_id: &str,
        headers: BTreeMap<String, Option<String>>,
    ) -> Result<(AssemblyAITranscriptResponse, Option<Headers>), String> {
        let polling_interval = self
            .settings
            .polling_interval
            .unwrap_or(DEFAULT_ASSEMBLYAI_POLLING_INTERVAL_MILLIS);
        let url = assemblyai_url(&format!("/v2/transcript/{transcript_id}"));

        loop {
            let transport = Arc::clone(&self.transport);
            let options = GetFromApiOptions::new(url.clone())
                .with_headers(headers.clone())
                .with_environment(RuntimeEnvironment::unknown());
            let response = get_from_api(
                options,
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        assemblyai_transcript_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        assemblyai_error_data,
                        |data| data.error.message.clone(),
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| assemblyai_handled_error_parts(error).0)?;

            match response.value.status.as_str() {
                "completed" => return Ok((response.value, response.response_headers)),
                "error" => {
                    return Err(format!(
                        "Transcription failed: {}",
                        response.value.error.as_deref().unwrap_or("Unknown error")
                    ));
                }
                _ => {
                    if polling_interval > 0 {
                        delay(Some(polling_interval as i64)).await;
                    } else {
                        delay(None).await;
                    }
                }
            }
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(assemblyai_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl TranscriptionModel for AssemblyAITranscriptionModel {
    type GenerateFuture<'a>
        = AssemblyAITranscriptionGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        AssemblyAITranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &str {
        AssemblyAITranscriptionModel::model_id(self)
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates an AssemblyAI provider with explicit settings.
pub fn create_assemblyai(settings: AssemblyAIProviderSettings) -> AssemblyAIProvider {
    AssemblyAIProvider::from_settings(settings)
}

/// Creates an AssemblyAI transcription model using the default provider settings.
pub fn assemblyai(model_id: impl Into<String>) -> AssemblyAITranscriptionModel {
    AssemblyAIProvider::new().transcription(model_id)
}

/// Provider-specific transcription options accepted by upstream AssemblyAI.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssemblyAITranscriptionModelOptions {
    pub audio_end_at: Option<i64>,
    pub audio_start_from: Option<i64>,
    pub auto_chapters: Option<bool>,
    pub auto_highlights: Option<bool>,
    pub boost_param: Option<String>,
    pub content_safety: Option<bool>,
    pub content_safety_confidence: Option<i64>,
    pub custom_spelling: Option<Vec<JsonValue>>,
    pub disfluencies: Option<bool>,
    pub entity_detection: Option<bool>,
    pub filter_profanity: Option<bool>,
    pub format_text: Option<bool>,
    pub iab_categories: Option<bool>,
    pub language_code: Option<String>,
    pub language_confidence_threshold: Option<f64>,
    pub language_detection: Option<bool>,
    pub multichannel: Option<bool>,
    pub punctuate: Option<bool>,
    pub redact_pii: Option<bool>,
    pub redact_pii_audio: Option<bool>,
    pub redact_pii_audio_quality: Option<String>,
    pub redact_pii_policies: Option<Vec<String>>,
    pub redact_pii_sub: Option<String>,
    pub sentiment_analysis: Option<bool>,
    pub speaker_labels: Option<bool>,
    pub speakers_expected: Option<i64>,
    pub speech_threshold: Option<f64>,
    pub summarization: Option<bool>,
    pub summary_model: Option<String>,
    pub summary_type: Option<String>,
    pub webhook_auth_header_name: Option<String>,
    pub webhook_auth_header_value: Option<String>,
    pub webhook_url: Option<String>,
    pub word_boost: Option<Vec<String>>,
}

impl AssemblyAITranscriptionModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .content_safety_confidence
            .is_some_and(|value| !(25..=100).contains(&value))
        {
            return Err("contentSafetyConfidence must be between 25 and 100");
        }

        if self
            .speech_threshold
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err("speechThreshold must be between 0 and 1");
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AssemblyAIUploadResponse {
    upload_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AssemblyAISubmitResponse {
    id: String,
    status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AssemblyAITranscriptResponse {
    id: String,
    status: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    language_code: Option<String>,
    #[serde(default)]
    words: Option<Vec<AssemblyAITranscriptWord>>,
    #[serde(default)]
    audio_duration: Option<f64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AssemblyAITranscriptWord {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AssemblyAIErrorData {
    error: AssemblyAIErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AssemblyAIErrorBody {
    message: String,
    code: i64,
}

fn assemblyai_audio_bytes(audio: &FileDataContent) -> Result<Vec<u8>, String> {
    match audio {
        FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
        FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
            .map_err(|error| format!("invalid base64 transcription audio: {error}")),
    }
}

fn assemblyai_transcription_request_body(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let assemblyai_options = parse_provider_options(
        "assemblyai",
        options.provider_options.as_ref(),
        assemblyai_transcription_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut body = JsonObject::new();

    body.insert(
        "speech_model".to_string(),
        JsonValue::String(model_id.to_string()),
    );
    insert_option_i64(&mut body, "audio_end_at", assemblyai_options.audio_end_at);
    insert_option_i64(
        &mut body,
        "audio_start_from",
        assemblyai_options.audio_start_from,
    );
    insert_option_bool(&mut body, "auto_chapters", assemblyai_options.auto_chapters);
    insert_option_bool(
        &mut body,
        "auto_highlights",
        assemblyai_options.auto_highlights,
    );
    insert_option_string(&mut body, "boost_param", assemblyai_options.boost_param);
    insert_option_bool(
        &mut body,
        "content_safety",
        assemblyai_options.content_safety,
    );
    insert_option_i64(
        &mut body,
        "content_safety_confidence",
        assemblyai_options.content_safety_confidence,
    );
    insert_option_json_array(
        &mut body,
        "custom_spelling",
        assemblyai_options.custom_spelling,
    );
    insert_option_bool(&mut body, "disfluencies", assemblyai_options.disfluencies);
    insert_option_bool(
        &mut body,
        "entity_detection",
        assemblyai_options.entity_detection,
    );
    insert_option_bool(
        &mut body,
        "filter_profanity",
        assemblyai_options.filter_profanity,
    );
    insert_option_bool(&mut body, "format_text", assemblyai_options.format_text);
    insert_option_bool(
        &mut body,
        "iab_categories",
        assemblyai_options.iab_categories,
    );
    insert_option_string(&mut body, "language_code", assemblyai_options.language_code);
    insert_option_f64(
        &mut body,
        "language_confidence_threshold",
        assemblyai_options.language_confidence_threshold,
    );
    insert_option_bool(
        &mut body,
        "language_detection",
        assemblyai_options.language_detection,
    );
    insert_option_bool(&mut body, "multichannel", assemblyai_options.multichannel);
    insert_option_bool(&mut body, "punctuate", assemblyai_options.punctuate);
    insert_option_bool(&mut body, "redact_pii", assemblyai_options.redact_pii);
    insert_option_bool(
        &mut body,
        "redact_pii_audio",
        assemblyai_options.redact_pii_audio,
    );
    insert_option_string(
        &mut body,
        "redact_pii_audio_quality",
        assemblyai_options.redact_pii_audio_quality,
    );
    insert_option_string_array(
        &mut body,
        "redact_pii_policies",
        assemblyai_options.redact_pii_policies,
    );
    insert_option_string(
        &mut body,
        "redact_pii_sub",
        assemblyai_options.redact_pii_sub,
    );
    insert_option_bool(
        &mut body,
        "sentiment_analysis",
        assemblyai_options.sentiment_analysis,
    );
    insert_option_bool(
        &mut body,
        "speaker_labels",
        assemblyai_options.speaker_labels,
    );
    insert_option_i64(
        &mut body,
        "speakers_expected",
        assemblyai_options.speakers_expected,
    );
    insert_option_f64(
        &mut body,
        "speech_threshold",
        assemblyai_options.speech_threshold,
    );
    insert_option_bool(&mut body, "summarization", assemblyai_options.summarization);
    insert_option_string(&mut body, "summary_model", assemblyai_options.summary_model);
    insert_option_string(&mut body, "summary_type", assemblyai_options.summary_type);
    insert_option_string(
        &mut body,
        "webhook_auth_header_name",
        assemblyai_options.webhook_auth_header_name,
    );
    insert_option_string(
        &mut body,
        "webhook_auth_header_value",
        assemblyai_options.webhook_auth_header_value,
    );
    insert_option_string(&mut body, "webhook_url", assemblyai_options.webhook_url);
    insert_option_string_array(&mut body, "word_boost", assemblyai_options.word_boost);

    Ok((JsonValue::Object(body), Vec::new()))
}

fn assemblyai_transcription_model_options(
    value: &JsonValue,
) -> Result<AssemblyAITranscriptionModelOptions, String> {
    let options = serde_json::from_value::<AssemblyAITranscriptionModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn assemblyai_provider_header_entries(
    settings: &AssemblyAIProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "authorization".to_string(),
        Some(assemblyai_api_key(settings.api_key.as_ref())?),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/assemblyai/{}", ai_sdk_rust::VERSION)],
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

fn assemblyai_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("ASSEMBLYAI_API_KEY", "AssemblyAI");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn assemblyai_url(path: &str) -> String {
    format!("{DEFAULT_ASSEMBLYAI_BASE_URL}{path}")
}

fn assemblyai_upload_response(
    value: &JsonValue,
) -> Result<AssemblyAIUploadResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn assemblyai_submit_response(
    value: &JsonValue,
) -> Result<AssemblyAISubmitResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn assemblyai_transcript_response(
    value: &JsonValue,
) -> Result<AssemblyAITranscriptResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn assemblyai_error_data(value: &JsonValue) -> Result<AssemblyAIErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn assemblyai_handled_error_parts(
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

fn assemblyai_transcription_result_from_response(
    model_id: &str,
    transcript: AssemblyAITranscriptResponse,
    headers: Option<Headers>,
    request_body: Option<String>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body = serde_json::to_value(&transcript).expect("transcript serializes");
    let mut response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body);
    let duration = transcript.audio_duration.or_else(|| {
        transcript
            .words
            .as_ref()
            .and_then(|words| words.last())
            .map(|word| word.end)
    });

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = TranscriptionModelResult::new(
        transcript.text.unwrap_or_default(),
        transcript
            .words
            .unwrap_or_default()
            .into_iter()
            .map(|word| TranscriptionModelSegment::new(word.text, word.start, word.end))
            .collect(),
        response,
    );

    if let Some(language) = transcript.language_code {
        result = result.with_language(language);
    }

    if let Some(duration) = duration {
        result = result.with_duration_in_seconds(duration);
    }

    if let Some(request_body) = request_body {
        result = result.with_request(TranscriptionModelRequest::new().with_body(request_body));
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn assemblyai_transcription_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
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

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = TranscriptionModelResult::new("", Vec::new(), response)
        .with_provider_metadata(assemblyai_error_metadata(message));

    if let Some(request_body) = request_body {
        result = result.with_request(TranscriptionModelRequest::new().with_body(request_body));
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn assemblyai_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("assemblyai".to_string(), provider);
    metadata
}

fn insert_option_bool(body: &mut JsonObject, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::Bool(value));
    }
}

fn insert_option_f64(body: &mut JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn insert_option_i64(body: &mut JsonObject, name: &str, value: Option<i64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn insert_option_json_array(body: &mut JsonObject, name: &str, value: Option<Vec<JsonValue>>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::Array(value));
    }
}

fn insert_option_string(body: &mut JsonObject, name: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value));
    }
}

fn insert_option_string_array(body: &mut JsonObject, name: &str, value: Option<Vec<String>>) {
    if let Some(value) = value {
        body.insert(
            name.to_string(),
            JsonValue::Array(value.into_iter().map(JsonValue::String).collect()),
        );
    }
}

fn default_assemblyai_date_provider() -> AssemblyAIDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_assemblyai_transport() -> AssemblyAITransport {
    Arc::new(|request| Box::pin(ready(execute_assemblyai_request(request))))
}

fn execute_assemblyai_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_assemblyai_get_request(request),
        ProviderApiRequestMethod::Post => execute_assemblyai_post_request(request),
    }
}

fn execute_assemblyai_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    assemblyai_provider_api_response(response)
}

fn execute_assemblyai_post_request(
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
                "multipart form data is not supported by the AssemblyAI transport",
            ));
        }
        None => builder.send_empty(),
    };

    assemblyai_provider_api_response(response)
}

fn assemblyai_provider_api_response(
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
        AssemblyAIProvider, AssemblyAIProviderSettings, AssemblyAITransport,
        AssemblyAITransportFuture, DEFAULT_ASSEMBLYAI_BASE_URL, assemblyai, create_assemblyai,
    };
    use ai_sdk_rust::{
        FileDataContent, ModelType, Provider, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions,
        ProviderWithTranscriptionModel, TranscriptionModel, TranscriptionModelCallOptions,
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

    fn fixed_timestamp() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(0).expect("unix epoch is valid")
    }

    fn json_response(value: serde_json::Value) -> ProviderApiResponse {
        ProviderApiResponse::text(200, "OK", value.to_string())
    }

    fn assemblyai_success_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, AssemblyAITransport)
    {
        assemblyai_success_transport_with_transcript(json!({
            "id": "transcript-123",
            "status": "completed",
            "text": "Hello, world!",
            "language_code": "en_us",
            "audio_duration": 281,
            "words": [
                { "start": 250, "end": 650, "text": "Hello," },
                { "start": 730, "end": 1022, "text": "world" }
            ]
        }))
    }

    fn assemblyai_success_transport_with_transcript(
        transcript: serde_json::Value,
    ) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, AssemblyAITransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: AssemblyAITransport =
            Arc::new(move |request| -> AssemblyAITransportFuture {
                requests_for_transport
                    .lock()
                    .expect("request list mutex is not poisoned")
                    .push(request.clone());

                let response = match (request.method, request.url.as_str()) {
                    (ProviderApiRequestMethod::Post, "https://api.assemblyai.com/v2/upload") => {
                        json_response(json!({
                            "upload_url": "https://storage.assemblyai.com/mock-upload-url"
                        }))
                    }
                    (
                        ProviderApiRequestMethod::Post,
                        "https://api.assemblyai.com/v2/transcript",
                    ) => json_response(json!({
                        "id": "transcript-123",
                        "status": "queued"
                    })),
                    (
                        ProviderApiRequestMethod::Get,
                        "https://api.assemblyai.com/v2/transcript/transcript-123",
                    ) => json_response(transcript.clone()).with_headers(
                        [("x-request-id".to_string(), "req-123".to_string())]
                            .into_iter()
                            .collect(),
                    ),
                    _ => ProviderApiResponse::text(
                        404,
                        "Not Found",
                        json!({"error": {"message": "unexpected request", "code": 404}})
                            .to_string(),
                    ),
                };

                Box::pin(ready(Ok(response)))
            });

        (requests, transport)
    }

    fn request_body_json(request: &ProviderApiRequest) -> serde_json::Value {
        let Some(ProviderApiRequestBody::Text { content }) = request.body.as_ref() else {
            panic!("expected text request body");
        };

        serde_json::from_str(content).expect("request body is valid JSON")
    }

    #[test]
    fn assemblyai_provider_transcribes_audio_with_headers_options_and_response() {
        let (requests, transport) = assemblyai_success_transport();
        let provider = create_assemblyai(
            AssemblyAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("x-extra-header", "extra"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "assemblyai".to_string(),
            serde_json::from_value(json!({
                "autoChapters": true,
                "contentSafetyConfidence": 80,
                "languageDetection": true,
                "wordBoost": ["hello", "world"]
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.transcription("best").do_generate(
                TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3]),
                    "audio/wav",
                )
                .with_header("Custom-Request-Header", "request")
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.text, "Hello, world!");
        assert_eq!(result.language.as_deref(), Some("en_us"));
        assert_eq!(result.duration_in_seconds, Some(281.0));
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[0].text, "Hello,");
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(result.response.model_id, "best");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req-123".to_string())
        );
        assert_eq!(
            result
                .response
                .body
                .as_ref()
                .and_then(|body| body.get("status")),
            Some(&json!("completed"))
        );
        assert!(result.request.is_some());

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(requests[0].url, "https://api.assemblyai.com/v2/upload");
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("content-type"),
            Some(&"application/octet-stream".to_string())
        );
        assert_eq!(
            requests[0].headers.get("custom-request-header"),
            Some(&"request".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-extra-header"),
            Some(&"extra".to_string())
        );
        assert!(
            requests[0]
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/assemblyai/"))
        );
        assert_eq!(
            requests[0].body,
            Some(ProviderApiRequestBody::Bytes {
                content: vec![1, 2, 3]
            })
        );
        assert_eq!(
            request_body_json(&requests[1]),
            json!({
                "audio_url": "https://storage.assemblyai.com/mock-upload-url",
                "auto_chapters": true,
                "content_safety_confidence": 80,
                "language_detection": true,
                "speech_model": "best",
                "word_boost": ["hello", "world"]
            })
        );
        assert_eq!(requests[2].method, ProviderApiRequestMethod::Get);
        assert_eq!(
            requests[2].url,
            "https://api.assemblyai.com/v2/transcript/transcript-123"
        );
    }

    #[test]
    fn assemblyai_transcription_duration_falls_back_to_last_word_end() {
        let (_requests, transport) = assemblyai_success_transport_with_transcript(json!({
            "id": "transcript-123",
            "status": "completed",
            "text": "Hello",
            "words": [
                { "start": 0.0, "end": 1.25, "text": "Hel" },
                { "start": 1.25, "end": 2.5, "text": "lo" }
            ]
        }));
        let provider = create_assemblyai(
            AssemblyAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_polling_interval(0),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(provider.transcription("best").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
        ));

        assert_eq!(result.duration_in_seconds, Some(2.5));
    }

    #[test]
    fn assemblyai_provider_reports_unsupported_model_families_and_trait_transcription() {
        let provider = AssemblyAIProvider::new();
        let language_error = match provider.language_model("some-model") {
            Ok(_) => panic!("language models are unsupported"),
            Err(error) => error,
        };
        let embedding_error = match provider.embedding_model("some-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        let image_error = match provider.image_model("some-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };

        assert_eq!(language_error.model_type(), ModelType::LanguageModel);
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(provider.specification_version().as_str(), "v4");
        assert_eq!(assemblyai("best").provider(), "assemblyai.transcription");

        let trait_transcription =
            ProviderWithTranscriptionModel::transcription_model(&provider, "best")
                .expect("ProviderWithTranscriptionModel creates transcription model");
        assert_eq!(trait_transcription.model_id(), "best");
    }

    #[test]
    fn assemblyai_transcription_model_maps_api_errors_to_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: AssemblyAITransport =
            Arc::new(move |request| -> AssemblyAITransportFuture {
                requests_for_transport
                    .lock()
                    .expect("request list mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "Invalid audio",
                            "code": 400
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = AssemblyAIProvider::from_settings(
            AssemblyAIProviderSettings::new().with_api_key("test-api-key"),
        )
        .with_transport(transport);

        let result = poll_ready(provider.transcription("best").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1, 2, 3]), "audio/wav"),
        ));

        assert!(result.text.is_empty());
        let metadata = result.provider_metadata.expect("provider metadata");
        assert_eq!(
            metadata
                .get("assemblyai")
                .and_then(|provider| provider.get("errorMessage")),
            Some(&json!("Invalid audio"))
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            1
        );
    }

    #[test]
    fn assemblyai_provider_settings_serde_accepts_upstream_shape() {
        let settings: AssemblyAIProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "headers": {
                "x-extra": "1"
            },
            "pollingInterval": 10
        }))
        .expect("settings deserialize");

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(settings.headers.get("x-extra"), Some(&"1".to_string()));
        assert_eq!(settings.polling_interval, Some(10));
        assert_eq!(DEFAULT_ASSEMBLYAI_BASE_URL, "https://api.assemblyai.com");
    }
}
