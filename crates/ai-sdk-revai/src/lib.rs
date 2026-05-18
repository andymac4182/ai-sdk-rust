use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, FormData, FormDataValue, GetFromApiOptions, HandledFetchError,
    Headers, JsonObject, JsonValue, LoadApiKeyError, LoadApiKeyOptions, ModelType,
    NoSuchModelError, OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel,
    OpenAICompatibleImageModel, PostFormDataToApiOptions, Provider, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, ProviderWithTranscriptionModel,
    RuntimeEnvironment, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelResponse, TranscriptionModelResult, TranscriptionModelSegment, Warning,
    combine_headers, convert_base64_to_bytes, create_json_error_response_handler,
    create_json_response_handler, delay, get_from_api, load_api_key, parse_provider_options,
    post_form_data_to_api, with_user_agent_suffix,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/revai` API calls.
pub const DEFAULT_REVAI_BASE_URL: &str = "https://api.rev.ai";

/// Default polling interval used by upstream Rev.ai transcription.
pub const DEFAULT_REVAI_POLLING_INTERVAL_MILLIS: u64 = 1_000;

/// Default polling timeout used by upstream Rev.ai transcription.
pub const DEFAULT_REVAI_POLLING_TIMEOUT_MILLIS: u64 = 60_000;

/// Provider-specific Rev.ai transcription options.
///
/// Upstream exposes these options through a zod object with snake-case keys.
/// The initial Rust package keeps the same JSON boundary so callers can pass
/// the upstream shape through `providerOptions.revai`.
pub type RevaiTranscriptionModelOptions = JsonObject;

/// Settings for the upstream Rev.ai provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevaiProviderSettings {
    /// Rev.ai API key. When omitted, `REVAI_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl RevaiProviderSettings {
    /// Creates empty Rev.ai provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Rev.ai API key.
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

/// Upstream Rev.ai provider foundation.
#[derive(Clone)]
pub struct RevaiProvider {
    settings: RevaiProviderSettings,
    transport: RevaiTransport,
    current_date: RevaiDateProvider,
}

/// Rev.ai transcription model.
#[derive(Clone)]
pub struct RevaiTranscriptionModel {
    model_id: String,
    settings: RevaiProviderSettings,
    transport: RevaiTransport,
    current_date: RevaiDateProvider,
}

/// Future returned by an injected Rev.ai HTTP transport.
pub type RevaiTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Rev.ai provider models.
pub type RevaiTransport = Arc<dyn Fn(ProviderApiRequest) -> RevaiTransportFuture + Send + Sync>;

type RevaiDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type RevaiTranscriptionGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>;

impl RevaiProvider {
    /// Creates a Rev.ai provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(RevaiProviderSettings::new())
    }

    /// Creates a provider from explicit Rev.ai settings.
    pub fn from_settings(settings: RevaiProviderSettings) -> Self {
        Self {
            settings,
            transport: default_revai_transport(),
            current_date: default_revai_date_provider(),
        }
    }

    /// Sets the Rev.ai API key for this provider.
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
    pub fn with_transport(mut self, transport: RevaiTransport) -> Self {
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
    pub fn transcription(&self, model_id: impl Into<String>) -> RevaiTranscriptionModel {
        self.transcription_model(model_id)
            .expect("Rev.ai transcription models are supported")
    }

    /// Creates a transcription model.
    pub fn transcription_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<RevaiTranscriptionModel, NoSuchModelError> {
        Ok(RevaiTranscriptionModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Rev.ai does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "Rev.ai does not provide language models",
        ))
    }

    /// Reports that Rev.ai does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            "Rev.ai does not provide text embedding models",
        ))
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Rev.ai does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            "Rev.ai does not provide image models",
        ))
    }
}

impl Default for RevaiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for RevaiProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        RevaiProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        RevaiProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        RevaiProvider::image_model(self, model_id)
    }
}

impl ProviderWithTranscriptionModel for RevaiProvider {
    type TranscriptionModel = RevaiTranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        RevaiProvider::transcription_model(self, model_id)
    }
}

impl RevaiTranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        settings: RevaiProviderSettings,
        transport: RevaiTransport,
        current_date: RevaiDateProvider,
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
        "revai.transcription"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: RevaiTransport) -> Self {
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
        let (form_data, warnings) = match revai_transcription_form_data(&self.model_id, &options) {
            Ok(args) => args,
            Err(message) => {
                return revai_transcription_result_from_error(
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
                return revai_transcription_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let submit_options =
            PostFormDataToApiOptions::new(revai_url("/speechtotext/v1/jobs"), form_data)
                .with_headers(request_headers.clone())
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let submit = match post_form_data_to_api(
            submit_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    revai_job_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    revai_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers, body) = revai_handled_error_parts(error);
                return revai_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    warnings,
                    timestamp,
                );
            }
        };

        if submit.value.status.as_deref() == Some("failed") {
            return revai_transcription_result_from_error(
                &self.model_id,
                "Failed to submit transcription job to Rev.ai".to_string(),
                submit.response_headers,
                submit.raw_value,
                warnings,
                timestamp,
            );
        }

        let Some(job_id) = submit.value.id.as_deref().filter(|id| !id.is_empty()) else {
            return revai_transcription_result_from_error(
                &self.model_id,
                "No job ID returned from API".to_string(),
                submit.response_headers,
                submit.raw_value,
                warnings,
                timestamp,
            );
        };

        let final_job = match self
            .wait_for_completion(job_id, &request_headers, submit.value.clone())
            .await
        {
            Ok(job) => job,
            Err(message) => {
                return revai_transcription_result_from_error(
                    &self.model_id,
                    message,
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };

        let transcript_url = revai_url(&format!("/speechtotext/v1/jobs/{job_id}/transcript"));
        let transport = Arc::clone(&self.transport);
        let transcript = match get_from_api(
            GetFromApiOptions::new(transcript_url)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown()),
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    revai_transcript_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    revai_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers, body) = revai_handled_error_parts(error);
                return revai_transcription_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    body,
                    warnings,
                    timestamp,
                );
            }
        };

        revai_transcription_result_from_response(
            &self.model_id,
            submit.value.language.or(final_job.language),
            transcript.value,
            transcript.response_headers,
            transcript.raw_value,
            warnings,
            timestamp,
        )
    }

    async fn wait_for_completion(
        &self,
        job_id: &str,
        headers: &BTreeMap<String, Option<String>>,
        initial_job: RevaiJobResponse,
    ) -> Result<RevaiJobResponse, String> {
        let started = Instant::now();
        let mut job = initial_job;

        loop {
            match job.status.as_deref() {
                Some("transcribed") => return Ok(job),
                Some("failed") => return Err("Transcription job failed".to_string()),
                _ => {}
            }

            if started.elapsed().as_millis() > u128::from(DEFAULT_REVAI_POLLING_TIMEOUT_MILLIS) {
                return Err("Transcription job polling timed out".to_string());
            }

            let status_url = revai_url(&format!("/speechtotext/v1/jobs/{job_id}"));
            let transport = Arc::clone(&self.transport);
            let response = get_from_api(
                GetFromApiOptions::new(status_url)
                    .with_headers(headers.clone())
                    .with_environment(RuntimeEnvironment::unknown()),
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        revai_job_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        revai_error_data,
                        |data| data.error.message.clone(),
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| revai_handled_error_parts(error).0)?;

            job = response.value;

            if job.status.as_deref() == Some("failed") {
                return Err("Transcription job failed".to_string());
            }

            if job.status.as_deref() != Some("transcribed") {
                delay(Some(DEFAULT_REVAI_POLLING_INTERVAL_MILLIS as i64)).await;
            }
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(revai_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl TranscriptionModel for RevaiTranscriptionModel {
    type GenerateFuture<'a>
        = RevaiTranscriptionGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        RevaiTranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &str {
        RevaiTranscriptionModel::model_id(self)
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Rev.ai provider with explicit settings.
pub fn create_revai(settings: RevaiProviderSettings) -> RevaiProvider {
    RevaiProvider::from_settings(settings)
}

/// Creates a Rev.ai transcription model using the default provider settings.
pub fn revai(model_id: impl Into<String>) -> RevaiTranscriptionModel {
    RevaiProvider::new().transcription(model_id)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RevaiJobResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RevaiTranscriptResponse {
    #[serde(default)]
    monologues: Option<Vec<RevaiMonologue>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RevaiMonologue {
    #[serde(default)]
    elements: Option<Vec<RevaiTranscriptElement>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RevaiTranscriptElement {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    ts: Option<f64>,
    #[serde(default)]
    end_ts: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RevaiErrorData {
    error: RevaiErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RevaiErrorBody {
    message: String,
    #[serde(default)]
    code: Option<i64>,
}

fn revai_transcription_form_data(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> Result<(FormData, Vec<Warning>), String> {
    let audio = revai_audio_bytes(&options.audio)?;
    let config = revai_transcription_config(model_id, options)?;
    let config_json = serde_json::to_string(&JsonValue::Object(config))
        .expect("Rev.ai transcription config serializes");
    let mut form_data = FormData::new();
    form_data.append("media", FormDataValue::bytes(audio));
    form_data.append("config", FormDataValue::text(config_json));

    Ok((form_data, Vec::new()))
}

fn revai_audio_bytes(audio: &FileDataContent) -> Result<Vec<u8>, String> {
    match audio {
        FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
        FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
            .map_err(|error| format!("invalid base64 transcription audio: {error}")),
    }
}

fn revai_transcription_config(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> Result<JsonObject, String> {
    let revai_options = parse_provider_options(
        "revai",
        options.provider_options.as_ref(),
        revai_transcription_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut config = JsonObject::new();

    config.insert(
        "transcriber".to_string(),
        JsonValue::String(model_id.to_string()),
    );

    for (name, value) in revai_options {
        if !value.is_null() {
            config.insert(name, value);
        }
    }

    Ok(config)
}

fn revai_transcription_model_options(
    value: &JsonValue,
) -> Result<RevaiTranscriptionModelOptions, String> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "Rev.ai provider options must be an object".to_string())
}

fn revai_provider_header_entries(
    settings: &RevaiProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "authorization".to_string(),
        Some(format!(
            "Bearer {}",
            revai_api_key(settings.api_key.as_ref())?
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
        [format!("ai-sdk/revai/{}", ai_sdk_rust::VERSION)],
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

fn revai_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("REVAI_API_KEY", "Rev.ai");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn revai_url(path: &str) -> String {
    format!("{DEFAULT_REVAI_BASE_URL}{path}")
}

fn revai_job_response(value: &JsonValue) -> Result<RevaiJobResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn revai_transcript_response(
    value: &JsonValue,
) -> Result<RevaiTranscriptResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn revai_error_data(value: &JsonValue) -> Result<RevaiErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn revai_handled_error_parts(
    error: HandledFetchError,
) -> (String, Option<Headers>, Option<JsonValue>) {
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

fn revai_transcription_result_from_response(
    model_id: &str,
    language: Option<String>,
    transcript: RevaiTranscriptResponse,
    headers: Option<Headers>,
    raw_value: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body = raw_value
        .unwrap_or_else(|| serde_json::to_value(&transcript).expect("transcript serializes"));
    let mut response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let (text, segments, duration) = revai_transcription_parts(&transcript);
    let mut result =
        TranscriptionModelResult::new(text, segments, response).with_duration_in_seconds(duration);

    if let Some(language) = language {
        result = result.with_language(language);
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn revai_transcription_parts(
    transcript: &RevaiTranscriptResponse,
) -> (String, Vec<TranscriptionModelSegment>, f64) {
    let mut duration = 0.0;
    let mut segments = Vec::new();
    let mut monologue_texts = Vec::new();

    for monologue in transcript.monologues.as_deref().unwrap_or_default() {
        let mut current_segment_text = String::new();
        let mut segment_start_second = 0.0;
        let mut has_started_segment = false;
        let mut monologue_text = String::new();

        for element in monologue.elements.as_deref().unwrap_or_default() {
            if let Some(value) = element.value.as_deref() {
                current_segment_text.push_str(value);
                monologue_text.push_str(value);
            }

            if element.r#type.as_deref() == Some("text") {
                if element.end_ts.is_some_and(|end| end > duration) {
                    duration = element.end_ts.unwrap_or(duration);
                }

                if !has_started_segment {
                    if let Some(start) = element.ts {
                        segment_start_second = start;
                        has_started_segment = true;
                    }
                }

                if let Some(end_second) = element.end_ts {
                    if has_started_segment {
                        let text = current_segment_text.trim();

                        if !text.is_empty() {
                            segments.push(TranscriptionModelSegment::new(
                                text,
                                segment_start_second,
                                end_second,
                            ));
                        }

                        current_segment_text.clear();
                        has_started_segment = false;
                    }
                }
            }
        }

        if has_started_segment {
            let text = current_segment_text.trim();

            if !text.is_empty() {
                let end_second = if duration > segment_start_second {
                    duration
                } else {
                    segment_start_second + 1.0
                };
                segments.push(TranscriptionModelSegment::new(
                    text,
                    segment_start_second,
                    end_second,
                ));
            }
        }

        monologue_texts.push(monologue_text);
    }

    (monologue_texts.join(" "), segments, duration)
}

fn revai_transcription_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    raw_body: Option<JsonValue>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> TranscriptionModelResult {
    let response_body = raw_body.unwrap_or_else(|| JsonValue::Object(JsonObject::new()));
    let mut response =
        TranscriptionModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = TranscriptionModelResult::new("", Vec::new(), response)
        .with_provider_metadata(revai_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn revai_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("revai".to_string(), provider);
    metadata
}

fn default_revai_date_provider() -> RevaiDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_revai_transport() -> RevaiTransport {
    Arc::new(|request| Box::pin(ready(execute_revai_request(request))))
}

fn execute_revai_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_revai_get_request(request),
        ProviderApiRequestMethod::Post => execute_revai_post_request(request),
    }
}

fn execute_revai_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    revai_provider_api_response(response)
}

fn execute_revai_post_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let form_body = request.body.as_ref().and_then(|body| match body {
        ProviderApiRequestBody::FormData { content } => Some(revai_multipart_body(content)),
        _ => None,
    });
    let mut builder = ureq::post(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    if let Some((content_type, _)) = form_body.as_ref() {
        builder = builder.header("content-type", content_type.as_str());
    }

    let builder = builder.config().http_status_as_error(false).build();
    let response = match request.body {
        Some(ProviderApiRequestBody::Text { content }) => builder.send(content),
        Some(ProviderApiRequestBody::Bytes { content }) => builder.send(content),
        Some(ProviderApiRequestBody::FormData { .. }) => {
            builder.send(form_body.expect("form body was prepared").1)
        }
        None => builder.send_empty(),
    };

    revai_provider_api_response(response)
}

fn revai_multipart_body(form_data: &FormData) -> (String, Vec<u8>) {
    let boundary = "----ai-sdk-rust-revai-boundary";
    let mut body = Vec::new();

    for entry in &form_data.entries {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());

        match &entry.value {
            FormDataValue::Text { value } => {
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
            FormDataValue::Bytes { value } => {
                body.extend_from_slice(
                    format!(
                        "content-disposition: form-data; name=\"{}\"; filename=\"audio\"\r\ncontent-type: application/octet-stream\r\n\r\n",
                        entry.name
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

fn revai_provider_api_response(
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
        DEFAULT_REVAI_BASE_URL, RevaiProvider, RevaiProviderSettings, RevaiTransport,
        RevaiTransportFuture, create_revai, revai,
    };
    use ai_sdk_rust::{
        FileDataContent, FormDataValue, ModelType, Provider, ProviderApiRequest,
        ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions,
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

    fn revai_transcript_fixture() -> serde_json::Value {
        json!({
            "monologues": [
                {
                    "speaker": 0,
                    "elements": [
                        {"type": "text", "value": "Hello", "ts": 0.075, "end_ts": 0.425, "confidence": 0.96},
                        {"type": "punct", "value": " "},
                        {"type": "text", "value": "from", "ts": 0.425, "end_ts": 0.665, "confidence": 0.98},
                        {"type": "punct", "value": " "},
                        {"type": "text", "value": "the", "ts": 0.665, "end_ts": 0.785, "confidence": 0.98},
                        {"type": "punct", "value": " "},
                        {"type": "text", "value": "Sal", "ts": 0.945, "end_ts": 1.105, "confidence": 0.64},
                        {"type": "punct", "value": ","},
                        {"type": "punct", "value": " "},
                        {"type": "text", "value": "A-I-S-D-K", "ts": 1.185, "end_ts": 2.145, "confidence": 0.96},
                        {"type": "punct", "value": "."}
                    ]
                }
            ]
        })
    }

    fn revai_success_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, RevaiTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: RevaiTransport = Arc::new(move |request| -> RevaiTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.rev.ai/speechtotext/v1/jobs") => {
                    json_response(json!({
                        "id": "test-id",
                        "status": "in_progress",
                        "language": "en"
                    }))
                }
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.rev.ai/speechtotext/v1/jobs/test-id",
                ) => json_response(json!({
                    "id": "test-id",
                    "status": "transcribed",
                    "language": "en"
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.rev.ai/speechtotext/v1/jobs/test-id/transcript",
                ) => json_response(revai_transcript_fixture()).with_headers(
                    [("x-request-id".to_string(), "req-123".to_string())]
                        .into_iter()
                        .collect(),
                ),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"error": {"message": "unexpected request", "code": 404}}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });

        (requests, transport)
    }

    fn form_config_json(request: &ProviderApiRequest) -> serde_json::Value {
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("request body is form data");
        let Some(FormDataValue::Text { value }) = form_data.get("config") else {
            panic!("expected config form field");
        };

        serde_json::from_str(value).expect("config form field is JSON")
    }

    #[test]
    fn revai_transcription_model_transcribes_audio_with_headers_options_and_response() {
        let (requests, transport) = revai_success_transport();
        let provider = create_revai(
            RevaiProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "revai".to_string(),
            serde_json::from_value(json!({
                "metadata": "job-1",
                "rush": true,
                "test_mode": false,
                "language": "en",
                "speaker_channels_count": 2,
                "notification_config": {
                    "url": "https://example.com/webhook"
                }
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.transcription("machine").do_generate(
                TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3, 4]),
                    "audio/wav",
                )
                .with_header("Custom-Request-Header", "request")
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.text, "Hello from the Sal, A-I-S-D-K.");
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(result.duration_in_seconds, Some(2.145));
        assert_eq!(
            result.segments,
            vec![
                TranscriptionModelSegment::new("Hello", 0.075, 0.425),
                TranscriptionModelSegment::new("from", 0.425, 0.665),
                TranscriptionModelSegment::new("the", 0.665, 0.785),
                TranscriptionModelSegment::new("Sal", 0.945, 1.105),
                TranscriptionModelSegment::new(", A-I-S-D-K", 1.185, 2.145),
            ]
        );
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(result.response.model_id, "machine");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req-123".to_string())
        );
        assert_eq!(
            result.response.body.as_ref(),
            Some(&revai_transcript_fixture())
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("custom-provider-header"),
            Some(&"provider".to_string())
        );
        assert_eq!(
            requests[0].headers.get("custom-request-header"),
            Some(&"request".to_string())
        );
        assert!(
            requests[0]
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/revai/"))
        );
        let form_data = requests[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("request body is form data");
        assert_eq!(
            form_data.get("media"),
            Some(&FormDataValue::bytes(vec![1, 2, 3, 4]))
        );
        assert_eq!(
            form_config_json(&requests[0]),
            json!({
                "transcriber": "machine",
                "metadata": "job-1",
                "rush": true,
                "test_mode": false,
                "language": "en",
                "speaker_channels_count": 2,
                "notification_config": {
                    "url": "https://example.com/webhook"
                }
            })
        );
    }

    #[test]
    fn revai_transcription_duration_falls_back_for_open_segment() {
        let transport: RevaiTransport = Arc::new(move |request| -> RevaiTransportFuture {
            let response = match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.rev.ai/speechtotext/v1/jobs") => {
                    json_response(json!({"id": "open-segment", "status": "transcribed"}))
                }
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.rev.ai/speechtotext/v1/jobs/open-segment/transcript",
                ) => json_response(json!({
                    "monologues": [
                        {
                            "elements": [
                                {"type": "text", "value": "unfinished", "ts": 1.0}
                            ]
                        }
                    ]
                })),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"error": {"message": "unexpected request", "code": 404}}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });
        let provider = create_revai(RevaiProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport)
            .with_current_date(fixed_timestamp);

        let result = poll_ready(provider.transcription("machine").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
        ));

        assert_eq!(result.text, "unfinished");
        assert_eq!(result.duration_in_seconds, Some(0.0));
        assert_eq!(
            result.segments,
            vec![TranscriptionModelSegment::new("unfinished", 1.0, 2.0)]
        );
    }

    #[test]
    fn revai_transcription_model_maps_api_and_status_errors_to_metadata() {
        let transport: RevaiTransport = Arc::new(move |request| -> RevaiTransportFuture {
            let response = match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.rev.ai/speechtotext/v1/jobs") => {
                    json_response(json!({"id": "failed-job", "status": "in_progress"}))
                }
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.rev.ai/speechtotext/v1/jobs/failed-job",
                ) => json_response(json!({"id": "failed-job", "status": "failed"})),
                _ => ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({"error": {"message": "Invalid media", "code": 400}}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });
        let provider = create_revai(RevaiProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport)
            .with_current_date(fixed_timestamp);

        let result = poll_ready(provider.transcription("machine").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1]), "audio/wav"),
        ));

        assert!(result.text.is_empty());
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("revai"))
                .and_then(|provider| provider.get("errorMessage"))
                .and_then(|message| message.as_str())
                .is_some_and(|message| message.contains("Transcription job failed"))
        );
    }

    #[test]
    fn revai_provider_reports_unsupported_model_families_and_trait_transcription() {
        let provider = RevaiProvider::new();
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
        assert!(language_error.to_string().contains("language models"));
        assert!(
            embedding_error
                .to_string()
                .contains("text embedding models")
        );
        assert!(image_error.to_string().contains("image models"));
        assert_eq!(provider.specification_version().as_str(), "v4");
        assert_eq!(revai("machine").provider(), "revai.transcription");

        let trait_transcription =
            ProviderWithTranscriptionModel::transcription_model(&provider, "machine")
                .expect("ProviderWithTranscriptionModel creates transcription model");
        assert_eq!(trait_transcription.model_id(), "machine");
    }

    #[test]
    fn revai_provider_settings_serde_accepts_upstream_shape() {
        let settings: RevaiProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "headers": {
                "x-extra": "1"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(settings.headers.get("x-extra"), Some(&"1".to_string()));
        assert_eq!(DEFAULT_REVAI_BASE_URL, "https://api.rev.ai");
    }
}
