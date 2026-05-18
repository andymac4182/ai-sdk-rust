use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, HandledFetchError, Headers, JsonObject, JsonValue,
    LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    PostJsonToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderWithSpeechModel, RuntimeEnvironment, SpeechModel,
    SpeechModelCallOptions, SpeechModelRequest, SpeechModelResponse, SpeechModelResult, Warning,
    combine_headers, create_binary_response_handler, create_json_error_response_handler,
    load_api_key, parse_provider_options, post_json_to_api, with_user_agent_suffix,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/hume` API calls.
pub const DEFAULT_HUME_BASE_URL: &str = "https://api.hume.ai";

/// Default Hume voice id used by upstream `@ai-sdk/hume`.
pub const DEFAULT_HUME_VOICE_ID: &str = "d8ab67c6-953d-4bd8-9370-8fa53a0f1453";

/// Settings for the upstream Hume provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumeProviderSettings {
    /// Hume API key. When omitted, `HUME_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl HumeProviderSettings {
    /// Creates empty Hume provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Hume API key.
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

/// Upstream Hume provider foundation.
#[derive(Clone)]
pub struct HumeProvider {
    settings: HumeProviderSettings,
    transport: HumeTransport,
    current_date: HumeDateProvider,
}

/// Hume speech model for `/v0/tts/file` calls.
#[derive(Clone)]
pub struct HumeSpeechModel {
    settings: HumeProviderSettings,
    transport: HumeTransport,
    current_date: HumeDateProvider,
}

/// Future returned by an injected Hume HTTP transport.
pub type HumeTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Hume provider models.
pub type HumeTransport = Arc<dyn Fn(ProviderApiRequest) -> HumeTransportFuture + Send + Sync>;

type HumeDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type HumeSpeechGenerateFuture<'a> = Pin<Box<dyn Future<Output = SpeechModelResult> + Send + 'a>>;

impl HumeProvider {
    /// Creates a Hume provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(HumeProviderSettings::new())
    }

    /// Creates a provider from explicit Hume settings.
    pub fn from_settings(settings: HumeProviderSettings) -> Self {
        Self {
            settings,
            transport: default_hume_transport(),
            current_date: default_hume_date_provider(),
        }
    }

    /// Sets the Hume API key for this provider.
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
    pub fn with_transport(mut self, transport: HumeTransport) -> Self {
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

    /// Creates the Hume speech model. Upstream Hume uses an empty model id.
    pub fn speech(&self) -> HumeSpeechModel {
        HumeSpeechModel::new(
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        )
    }

    /// Reports that Hume does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "Hume does not provide language models",
        ))
    }

    /// Reports that Hume does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            "Hume does not provide embedding models",
        ))
    }

    /// Reports that Hume does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            "Hume does not provide image models",
        ))
    }
}

impl Default for HumeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for HumeProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        HumeProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        HumeProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        HumeProvider::image_model(self, model_id)
    }
}

impl ProviderWithSpeechModel for HumeProvider {
    type SpeechModel = HumeSpeechModel;

    fn speech_model(&self, _model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        Ok(self.speech())
    }
}

impl HumeSpeechModel {
    fn new(
        settings: HumeProviderSettings,
        transport: HumeTransport,
        current_date: HumeDateProvider,
    ) -> Self {
        Self {
            settings,
            transport,
            current_date,
        }
    }

    /// Returns the provider-specific model id. Hume speech uses an empty id upstream.
    pub fn model_id(&self) -> &str {
        ""
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "hume.speech"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: HumeTransport) -> Self {
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
        let (request_body, warnings) = match hume_speech_request_body(&options) {
            Ok(args) => args,
            Err(error) => {
                return hume_speech_result_from_error(
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
                return hume_speech_result_from_error(
                    error.to_string(),
                    request_body_for_error,
                    None,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(self.speech_model_url(), request_body)
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
                    hume_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => hume_speech_result_from_response(
                response.value,
                response.response_headers,
                request_body_for_error,
                warnings,
                timestamp,
            ),
            Err(error) => hume_speech_result_from_handled_error(
                error,
                request_body_for_error,
                warnings,
                timestamp,
            ),
        }
    }

    fn speech_model_url(&self) -> String {
        format!("{DEFAULT_HUME_BASE_URL}/v0/tts/file")
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(hume_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl SpeechModel for HumeSpeechModel {
    type GenerateFuture<'a>
        = HumeSpeechGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        HumeSpeechModel::provider(self)
    }

    fn model_id(&self) -> &str {
        HumeSpeechModel::model_id(self)
    }

    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Hume provider with explicit settings.
pub fn create_hume(settings: HumeProviderSettings) -> HumeProvider {
    HumeProvider::from_settings(settings)
}

/// Creates a Hume speech model using the default provider settings.
pub fn hume() -> HumeSpeechModel {
    HumeProvider::new().speech()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumeSpeechModelOptions {
    /// Context for the speech synthesis request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<HumeSpeechContext>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum HumeSpeechContext {
    GenerationId {
        #[serde(rename = "generationId", alias = "generation_id")]
        generation_id: String,
    },
    Utterances {
        utterances: Vec<HumeSpeechContextUtterance>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumeSpeechContextUtterance {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(
        default,
        rename = "trailingSilence",
        alias = "trailing_silence",
        skip_serializing_if = "Option::is_none"
    )]
    pub trailing_silence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<HumeVoice>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum HumeVoice {
    Id {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<HumeVoiceProvider>,
    },
    Name {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<HumeVoiceProvider>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum HumeVoiceProvider {
    #[serde(rename = "HUME_AI")]
    HumeAi,
    #[serde(rename = "CUSTOM_VOICE")]
    CustomVoice,
}

const HUME_SUPPORTED_OUTPUT_FORMATS: &[&str] = &["mp3", "pcm", "wav"];

fn hume_speech_request_body(
    options: &SpeechModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), ai_sdk_rust::InvalidArgumentError> {
    let mut warnings = Vec::new();
    let hume_options = parse_provider_options(
        "hume",
        options.provider_options.as_ref(),
        hume_speech_model_options,
    )?;

    let mut utterance = JsonObject::new();
    utterance.insert("text".to_string(), JsonValue::String(options.text.clone()));
    if let Some(speed) = options.speed {
        utterance.insert("speed".to_string(), JsonValue::from(speed));
    }
    if let Some(instructions) = options.instructions.as_ref() {
        utterance.insert(
            "description".to_string(),
            JsonValue::String(instructions.clone()),
        );
    }
    let mut voice = JsonObject::new();
    voice.insert(
        "id".to_string(),
        JsonValue::String(
            options
                .voice
                .clone()
                .unwrap_or_else(|| DEFAULT_HUME_VOICE_ID.to_string()),
        ),
    );
    voice.insert(
        "provider".to_string(),
        JsonValue::String("HUME_AI".to_string()),
    );
    utterance.insert("voice".to_string(), JsonValue::Object(voice));

    let mut format = JsonObject::new();
    let output_format = options.output_format.as_deref().unwrap_or("mp3");
    if HUME_SUPPORTED_OUTPUT_FORMATS.contains(&output_format) {
        format.insert(
            "type".to_string(),
            JsonValue::String(output_format.to_string()),
        );
    } else {
        warnings.push(Warning::Unsupported {
            feature: "outputFormat".to_string(),
            details: Some(format!(
                "Unsupported output format: {output_format}. Using mp3 instead."
            )),
        });
        format.insert("type".to_string(), JsonValue::String("mp3".to_string()));
    }

    let mut body = JsonObject::new();
    body.insert(
        "utterances".to_string(),
        JsonValue::Array(vec![JsonValue::Object(utterance)]),
    );
    body.insert("format".to_string(), JsonValue::Object(format));

    if let Some(hume_options) = hume_options {
        if let Some(context) = hume_options.context {
            body.insert("context".to_string(), hume_context_request_value(context));
        }
    }

    if let Some(language) = options.language.as_ref() {
        warnings.push(Warning::Unsupported {
            feature: "language".to_string(),
            details: Some(format!(
                "Hume speech models do not support language selection. Language parameter \"{language}\" was ignored."
            )),
        });
    }

    Ok((JsonValue::Object(body), warnings))
}

fn hume_speech_model_options(value: &JsonValue) -> Result<HumeSpeechModelOptions, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

fn hume_context_request_value(context: HumeSpeechContext) -> JsonValue {
    let mut object = JsonObject::new();

    match context {
        HumeSpeechContext::GenerationId { generation_id } => {
            object.insert(
                "generation_id".to_string(),
                JsonValue::String(generation_id),
            );
        }
        HumeSpeechContext::Utterances { utterances } => {
            object.insert(
                "utterances".to_string(),
                JsonValue::Array(
                    utterances
                        .into_iter()
                        .map(hume_context_utterance_request_value)
                        .collect(),
                ),
            );
        }
    }

    JsonValue::Object(object)
}

fn hume_context_utterance_request_value(utterance: HumeSpeechContextUtterance) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert("text".to_string(), JsonValue::String(utterance.text));
    if let Some(description) = utterance.description {
        object.insert("description".to_string(), JsonValue::String(description));
    }
    if let Some(speed) = utterance.speed {
        object.insert("speed".to_string(), JsonValue::from(speed));
    }
    if let Some(trailing_silence) = utterance.trailing_silence {
        object.insert(
            "trailing_silence".to_string(),
            JsonValue::from(trailing_silence),
        );
    }
    if let Some(voice) = utterance.voice {
        object.insert("voice".to_string(), hume_voice_request_value(voice));
    }

    JsonValue::Object(object)
}

fn hume_voice_request_value(voice: HumeVoice) -> JsonValue {
    let mut object = JsonObject::new();

    match voice {
        HumeVoice::Id { id, provider } => {
            object.insert("id".to_string(), JsonValue::String(id));
            if let Some(provider) = provider {
                object.insert(
                    "provider".to_string(),
                    JsonValue::String(hume_voice_provider_name(provider).to_string()),
                );
            }
        }
        HumeVoice::Name { name, provider } => {
            object.insert("name".to_string(), JsonValue::String(name));
            if let Some(provider) = provider {
                object.insert(
                    "provider".to_string(),
                    JsonValue::String(hume_voice_provider_name(provider).to_string()),
                );
            }
        }
    }

    JsonValue::Object(object)
}

fn hume_voice_provider_name(provider: HumeVoiceProvider) -> &'static str {
    match provider {
        HumeVoiceProvider::HumeAi => "HUME_AI",
        HumeVoiceProvider::CustomVoice => "CUSTOM_VOICE",
    }
}

fn hume_provider_header_entries(
    settings: &HumeProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "X-Hume-Api-Key".to_string(),
        Some(hume_api_key(settings.api_key.as_ref())?),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/hume/{}", ai_sdk_rust::VERSION)],
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

fn hume_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("HUME_API_KEY", "Hume");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HumeErrorData {
    error: HumeErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HumeErrorBody {
    message: String,
    code: i64,
}

fn hume_error_data(value: &JsonValue) -> Result<HumeErrorData, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

fn hume_speech_result_from_response(
    audio: Vec<u8>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let mut response = SpeechModelResponse::new(timestamp, "");

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

fn hume_speech_result_from_handled_error(
    error: HandledFetchError,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let (message, headers, body) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    };

    hume_speech_result_from_error(
        message,
        request_body,
        headers,
        body.as_deref(),
        warnings,
        timestamp,
    )
}

fn hume_speech_result_from_error(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> SpeechModelResult {
    let response_body = raw_body
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.map(|body| JsonValue::String(body.to_string())))
        .unwrap_or_else(|| request_body.clone());
    let mut response = SpeechModelResponse::new(timestamp, "").with_body(response_body);

    if let Some(headers) = response_headers {
        response = with_speech_response_headers(response, headers);
    }

    let mut result = SpeechModelResult::new(FileDataContent::Bytes(Vec::new()), response)
        .with_request(SpeechModelRequest::new().with_body(request_body))
        .with_provider_metadata(hume_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
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

fn hume_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("hume".to_string(), provider);
    metadata
}

fn default_hume_date_provider() -> HumeDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_hume_transport() -> HumeTransport {
    Arc::new(|request| Box::pin(ready(execute_hume_request(request))))
}

fn execute_hume_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_hume_get_request(request),
        ProviderApiRequestMethod::Post => execute_hume_post_request(request),
    }
}

fn execute_hume_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    hume_provider_api_response(response)
}

fn execute_hume_post_request(
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
                "multipart form data is not supported by the Hume transport",
            ));
        }
        None => builder.send_empty(),
    };

    hume_provider_api_response(response)
}

fn hume_provider_api_response(
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
        DEFAULT_HUME_BASE_URL, DEFAULT_HUME_VOICE_ID, HumeProvider, HumeProviderSettings,
        HumeTransport, HumeTransportFuture, create_hume, hume,
    };
    use ai_sdk_rust::{
        FileDataContent, ModelType, Provider, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions, ProviderWithSpeechModel,
        SpeechModel, SpeechModelCallOptions, Warning,
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
        response: ProviderApiResponse,
    ) -> (Arc<Mutex<Option<ProviderApiRequest>>>, HumeTransport) {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HumeTransport = Arc::new(move |request| -> HumeTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(response.clone())))
        });

        (captured_request, transport)
    }

    fn fixed_timestamp() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(0).expect("unix epoch is valid")
    }

    #[test]
    fn hume_provider_creates_speech_model_with_headers_options_and_body() {
        let (captured_request, transport) = capture_transport(
            ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3]).with_headers(
                [("content-type".to_string(), "audio/wav".to_string())]
                    .into_iter()
                    .collect(),
            ),
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "hume": {
                "context": {
                    "utterances": [
                        {
                            "text": "Earlier line",
                            "description": "calmly",
                            "speed": 0.8,
                            "trailingSilence": 0.25,
                            "voice": {
                                "name": "Narrator",
                                "provider": "CUSTOM_VOICE"
                            }
                        }
                    ]
                }
            }
        }))
        .expect("provider options deserialize");
        let provider = create_hume(
            HumeProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-header-value"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.speech().do_generate(
                SpeechModelCallOptions::new("Hello from the AI SDK!")
                    .with_voice("voice-123")
                    .with_output_format("wav")
                    .with_speed(1.5)
                    .with_instructions("speak warmly")
                    .with_provider_options(provider_options)
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        assert_eq!(result.audio, FileDataContent::Bytes(vec![1, 2, 3]));
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(result.response.model_id, "");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("content-type")),
            Some(&"audio/wav".to_string())
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, format!("{DEFAULT_HUME_BASE_URL}/v0/tts/file"));
        assert_eq!(
            request.headers.get("x-hume-api-key"),
            Some(&"test-api-key".to_string())
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
                .contains("ai-sdk/hume/")
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
                "utterances": [
                    {
                        "text": "Hello from the AI SDK!",
                        "speed": 1.5,
                        "description": "speak warmly",
                        "voice": {
                            "id": "voice-123",
                            "provider": "HUME_AI"
                        }
                    }
                ],
                "format": {
                    "type": "wav"
                },
                "context": {
                    "utterances": [
                        {
                            "text": "Earlier line",
                            "description": "calmly",
                            "speed": 0.8,
                            "trailing_silence": 0.25,
                            "voice": {
                                "name": "Narrator",
                                "provider": "CUSTOM_VOICE"
                            }
                        }
                    ]
                }
            })
        );
    }

    #[test]
    fn hume_speech_model_defaults_voice_format_and_warns_for_unsupported_inputs() {
        let (captured_request, transport) =
            capture_transport(ProviderApiResponse::bytes(200, "OK", vec![9]));
        let model = HumeProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech();

        let result = poll_ready(
            model.do_generate(
                SpeechModelCallOptions::new("Hello.")
                    .with_output_format("flac")
                    .with_language("en"),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "outputFormat".to_string(),
                    details: Some(
                        "Unsupported output format: flac. Using mp3 instead.".to_string()
                    )
                },
                Warning::Unsupported {
                    feature: "language".to_string(),
                    details: Some("Hume speech models do not support language selection. Language parameter \"en\" was ignored.".to_string())
                }
            ]
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body,
            json!({
                "utterances": [
                    {
                        "text": "Hello.",
                        "voice": {
                            "id": DEFAULT_HUME_VOICE_ID,
                            "provider": "HUME_AI"
                        }
                    }
                ],
                "format": {
                    "type": "mp3"
                }
            })
        );
    }

    #[test]
    fn hume_speech_model_maps_generation_context_and_api_errors_to_metadata() {
        let (captured_request, transport) = capture_transport(
            ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({
                    "error": {
                        "message": "bad voice",
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
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "hume": {
                "context": {
                    "generationId": "gen_123"
                }
            }
        }))
        .expect("provider options deserialize");
        let model = HumeProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech();

        let result = poll_ready(model.do_generate(
            SpeechModelCallOptions::new("Hello.").with_provider_options(provider_options),
        ));

        assert_eq!(result.audio, FileDataContent::Bytes(Vec::new()));
        assert_eq!(
            result.provider_metadata,
            Some(
                serde_json::from_value(json!({
                    "hume": {
                        "errorMessage": "bad voice"
                    }
                }))
                .expect("metadata deserializes")
            )
        );
        assert_eq!(
            result.response.body,
            Some(json!({
                "error": {
                    "message": "bad voice",
                    "code": 123
                }
            }))
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req_123".to_string())
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body["context"],
            json!({ "generation_id": "gen_123" })
        );
    }

    #[test]
    fn hume_provider_reports_unsupported_model_families_and_trait_speech() {
        let provider = HumeProvider::new().with_api_key("test-api-key");

        let language_error = Provider::language_model(&provider, "gpt")
            .err()
            .expect("language models are unsupported");
        assert_eq!(language_error.model_type(), ModelType::LanguageModel);
        assert_eq!(
            language_error.message(),
            "Hume does not provide language models"
        );

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            embedding_error.message(),
            "Hume does not provide embedding models"
        );

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(image_error.message(), "Hume does not provide image models");

        let speech_model =
            ProviderWithSpeechModel::speech_model(&provider, "").expect("speech model resolves");
        assert_eq!(speech_model.provider(), "hume.speech");
        assert_eq!(speech_model.model_id(), "");
    }

    #[test]
    fn hume_provider_settings_serde_accepts_upstream_shape() {
        let settings: HumeProviderSettings = serde_json::from_value(json!({
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
    }

    #[test]
    fn hume_default_factory_creates_speech_model() {
        let model = hume();

        assert_eq!(model.provider(), "hume.speech");
        assert_eq!(model.model_id(), "");
    }
}
