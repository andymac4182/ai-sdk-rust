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

/// Default base URL for upstream `@ai-sdk/lmnt` API calls.
pub const DEFAULT_LMNT_BASE_URL: &str = "https://api.lmnt.com";

/// Settings for the upstream LMNT provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LMNTProviderSettings {
    /// LMNT API key. When omitted, `LMNT_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl LMNTProviderSettings {
    /// Creates empty LMNT provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the LMNT API key.
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

/// Upstream LMNT provider foundation.
#[derive(Clone)]
pub struct LMNTProvider {
    settings: LMNTProviderSettings,
    transport: LMNTTransport,
    current_date: LMNTDateProvider,
}

/// LMNT speech model for `/v1/ai/speech/bytes` calls.
#[derive(Clone)]
pub struct LMNTSpeechModel {
    model_id: String,
    settings: LMNTProviderSettings,
    transport: LMNTTransport,
    current_date: LMNTDateProvider,
}

/// Future returned by an injected LMNT HTTP transport.
pub type LMNTTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by LMNT provider models.
pub type LMNTTransport = Arc<dyn Fn(ProviderApiRequest) -> LMNTTransportFuture + Send + Sync>;

type LMNTDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type LMNTSpeechGenerateFuture<'a> = Pin<Box<dyn Future<Output = SpeechModelResult> + Send + 'a>>;

impl LMNTProvider {
    /// Creates an LMNT provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(LMNTProviderSettings::new())
    }

    /// Creates a provider from explicit LMNT settings.
    pub fn from_settings(settings: LMNTProviderSettings) -> Self {
        Self {
            settings,
            transport: default_lmnt_transport(),
            current_date: default_lmnt_date_provider(),
        }
    }

    /// Sets the LMNT API key for this provider.
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
    pub fn with_transport(mut self, transport: LMNTTransport) -> Self {
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

    /// Creates an LMNT speech model.
    pub fn speech(&self, model_id: impl Into<String>) -> LMNTSpeechModel {
        self.speech_model(model_id)
    }

    /// Creates an LMNT speech model.
    pub fn speech_model(&self, model_id: impl Into<String>) -> LMNTSpeechModel {
        LMNTSpeechModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        )
    }

    /// Reports that LMNT does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "LMNT does not provide language models",
        ))
    }

    /// Reports that LMNT does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            "LMNT does not provide embedding models",
        ))
    }

    /// Reports that LMNT does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            "LMNT does not provide image models",
        ))
    }
}

impl Default for LMNTProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for LMNTProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        LMNTProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        LMNTProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        LMNTProvider::image_model(self, model_id)
    }
}

impl ProviderWithSpeechModel for LMNTProvider {
    type SpeechModel = LMNTSpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        Ok(LMNTProvider::speech_model(self, model_id))
    }
}

impl LMNTSpeechModel {
    fn new(
        model_id: impl Into<String>,
        settings: LMNTProviderSettings,
        transport: LMNTTransport,
        current_date: LMNTDateProvider,
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
        "lmnt.speech"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: LMNTTransport) -> Self {
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
        let (request_body, warnings) = match lmnt_speech_request_body(&self.model_id, &options) {
            Ok(args) => args,
            Err(error) => {
                return lmnt_speech_result_from_error(
                    error.to_string(),
                    JsonValue::Object(JsonObject::new()),
                    None,
                    None,
                    Vec::new(),
                    timestamp,
                    self.model_id.clone(),
                );
            }
        };

        let request_body_for_error = request_body.clone();
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return lmnt_speech_result_from_error(
                    error.to_string(),
                    request_body_for_error,
                    None,
                    None,
                    warnings,
                    timestamp,
                    self.model_id.clone(),
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
                    lmnt_error_data,
                    |data| data.error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => lmnt_speech_result_from_response(
                response.value,
                response.response_headers,
                request_body_for_error,
                warnings,
                timestamp,
                self.model_id.clone(),
            ),
            Err(error) => lmnt_speech_result_from_handled_error(
                error,
                request_body_for_error,
                warnings,
                timestamp,
                self.model_id.clone(),
            ),
        }
    }

    fn speech_model_url(&self) -> String {
        format!("{DEFAULT_LMNT_BASE_URL}/v1/ai/speech/bytes")
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(lmnt_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl SpeechModel for LMNTSpeechModel {
    type GenerateFuture<'a>
        = LMNTSpeechGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        LMNTSpeechModel::provider(self)
    }

    fn model_id(&self) -> &str {
        LMNTSpeechModel::model_id(self)
    }

    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates an LMNT provider with explicit settings.
pub fn create_lmnt(settings: LMNTProviderSettings) -> LMNTProvider {
    LMNTProvider::from_settings(settings)
}

/// Creates an LMNT speech model using the default provider settings.
pub fn lmnt(model_id: impl Into<String>) -> LMNTSpeechModel {
    LMNTProvider::new().speech(model_id)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LMNTSpeechModelOptions {
    /// Provider-specific model hint accepted by upstream but not sent separately.
    #[serde(default)]
    pub model: Option<String>,

    /// Provider-specific format option accepted by upstream validation.
    #[serde(default)]
    pub format: Option<String>,

    /// Output sample rate in Hz.
    #[serde(default, rename = "sampleRate", alias = "sample_rate")]
    pub sample_rate: Option<u64>,

    /// Speech speed.
    #[serde(default)]
    pub speed: Option<f64>,

    /// Deterministic generation seed.
    #[serde(default)]
    pub seed: Option<i64>,

    /// Whether to use a conversational style.
    #[serde(default)]
    pub conversational: Option<bool>,

    /// Maximum output length in seconds.
    #[serde(default)]
    pub length: Option<f64>,

    /// Top-p sampling value.
    #[serde(default, rename = "topP", alias = "top_p")]
    pub top_p: Option<f64>,

    /// Temperature sampling value.
    #[serde(default)]
    pub temperature: Option<f64>,
}

impl LMNTSpeechModelOptions {
    fn with_upstream_defaults(mut self) -> Self {
        self.model.get_or_insert_with(|| "aurora".to_string());
        self.format.get_or_insert_with(|| "mp3".to_string());
        self.sample_rate.get_or_insert(24_000);
        self.speed.get_or_insert(1.0);
        self.conversational.get_or_insert(false);
        self.top_p.get_or_insert(1.0);
        self.temperature.get_or_insert(1.0);
        self
    }

    fn validate(&self) -> Result<(), &'static str> {
        if let Some(format) = self.format.as_deref() {
            if !LMNT_SUPPORTED_OUTPUT_FORMATS.contains(&format) {
                return Err("format must be one of aac, mp3, mulaw, raw, wav");
            }
        }

        if let Some(sample_rate) = self.sample_rate {
            if !matches!(sample_rate, 8_000 | 16_000 | 24_000) {
                return Err("sampleRate must be 8000, 16000, or 24000");
            }
        }

        if let Some(speed) = self.speed {
            if !(0.25..=2.0).contains(&speed) {
                return Err("speed must be between 0.25 and 2");
            }
        }

        if self.length.is_some_and(|length| length > 300.0) {
            return Err("length must be at most 300");
        }

        if let Some(top_p) = self.top_p {
            if !(0.0..=1.0).contains(&top_p) {
                return Err("topP must be between 0 and 1");
            }
        }

        if self
            .temperature
            .is_some_and(|temperature| temperature < 0.0)
        {
            return Err("temperature must be greater than or equal to 0");
        }

        Ok(())
    }
}

const LMNT_SUPPORTED_OUTPUT_FORMATS: &[&str] = &["aac", "mp3", "mulaw", "raw", "wav"];

fn lmnt_speech_request_body(
    model_id: &str,
    options: &SpeechModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), ai_sdk_rust::InvalidArgumentError> {
    let mut warnings = Vec::new();
    let lmnt_options = parse_provider_options(
        "lmnt",
        options.provider_options.as_ref(),
        lmnt_speech_model_options,
    )?;
    let mut body = JsonObject::new();

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("text".to_string(), JsonValue::String(options.text.clone()));
    body.insert(
        "voice".to_string(),
        JsonValue::String(options.voice.clone().unwrap_or_else(|| "ava".to_string())),
    );
    body.insert(
        "response_format".to_string(),
        JsonValue::String("mp3".to_string()),
    );

    if let Some(speed) = options.speed {
        body.insert("speed".to_string(), JsonValue::from(speed));
    }

    if let Some(output_format) = options.output_format.as_deref() {
        if LMNT_SUPPORTED_OUTPUT_FORMATS.contains(&output_format) {
            body.insert(
                "response_format".to_string(),
                JsonValue::String(output_format.to_string()),
            );
        } else {
            warnings.push(Warning::Unsupported {
                feature: "outputFormat".to_string(),
                details: Some(format!(
                    "Unsupported output format: {output_format}. Using mp3 instead."
                )),
            });
        }
    }

    if let Some(lmnt_options) = lmnt_options {
        insert_option_bool(&mut body, "conversational", lmnt_options.conversational);
        insert_option_f64(&mut body, "length", lmnt_options.length);
        insert_option_i64(&mut body, "seed", lmnt_options.seed);
        insert_option_f64(&mut body, "speed", lmnt_options.speed);
        insert_option_f64(&mut body, "temperature", lmnt_options.temperature);
        insert_option_f64(&mut body, "top_p", lmnt_options.top_p);
        insert_option_u64(&mut body, "sample_rate", lmnt_options.sample_rate);
    }

    if let Some(language) = options.language.as_ref() {
        body.insert("language".to_string(), JsonValue::String(language.clone()));
    }

    Ok((JsonValue::Object(body), warnings))
}

fn lmnt_speech_model_options(value: &JsonValue) -> Result<LMNTSpeechModelOptions, String> {
    let options = serde_json::from_value::<LMNTSpeechModelOptions>(value.clone())
        .map_err(|error| error.to_string())?
        .with_upstream_defaults();
    options.validate().map_err(str::to_string)?;
    Ok(options)
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

fn insert_option_u64(body: &mut JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn lmnt_provider_header_entries(
    settings: &LMNTProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "x-api-key".to_string(),
        Some(lmnt_api_key(settings.api_key.as_ref())?),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/lmnt/{}", ai_sdk_rust::VERSION)],
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

fn lmnt_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("LMNT_API_KEY", "LMNT");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LMNTErrorData {
    error: LMNTErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LMNTErrorBody {
    message: String,
    code: i64,
}

fn lmnt_error_data(value: &JsonValue) -> Result<LMNTErrorData, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

fn lmnt_speech_result_from_response(
    audio: Vec<u8>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
    model_id: String,
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

fn lmnt_speech_result_from_handled_error(
    error: HandledFetchError,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
    model_id: String,
) -> SpeechModelResult {
    let (message, headers, body) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    };

    lmnt_speech_result_from_error(
        message,
        request_body,
        headers,
        body.as_deref(),
        warnings,
        timestamp,
        model_id,
    )
}

fn lmnt_speech_result_from_error(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
    model_id: String,
) -> SpeechModelResult {
    let response_body = raw_body
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.map(|body| JsonValue::String(body.to_string())))
        .unwrap_or_else(|| request_body.clone());
    let mut response = SpeechModelResponse::new(timestamp, model_id).with_body(response_body);

    if let Some(headers) = response_headers {
        response = with_speech_response_headers(response, headers);
    }

    let mut result = SpeechModelResult::new(FileDataContent::Bytes(Vec::new()), response)
        .with_request(SpeechModelRequest::new().with_body(request_body))
        .with_provider_metadata(lmnt_error_metadata(message));

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

fn lmnt_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("lmnt".to_string(), provider);
    metadata
}

fn default_lmnt_date_provider() -> LMNTDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_lmnt_transport() -> LMNTTransport {
    Arc::new(|request| Box::pin(ready(execute_lmnt_request(request))))
}

fn execute_lmnt_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_lmnt_get_request(request),
        ProviderApiRequestMethod::Post => execute_lmnt_post_request(request),
    }
}

fn execute_lmnt_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    lmnt_provider_api_response(response)
}

fn execute_lmnt_post_request(
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
                "multipart form data is not supported by the LMNT transport",
            ));
        }
        None => builder.send_empty(),
    };

    lmnt_provider_api_response(response)
}

fn lmnt_provider_api_response(
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
        DEFAULT_LMNT_BASE_URL, LMNTProvider, LMNTProviderSettings, LMNTTransport,
        LMNTTransportFuture, create_lmnt, lmnt,
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
    ) -> (Arc<Mutex<Option<ProviderApiRequest>>>, LMNTTransport) {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: LMNTTransport = Arc::new(move |request| -> LMNTTransportFuture {
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
    fn lmnt_provider_creates_speech_model_with_headers_options_and_body() {
        let (captured_request, transport) = capture_transport(
            ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3]).with_headers(
                [("content-type".to_string(), "audio/wav".to_string())]
                    .into_iter()
                    .collect(),
            ),
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "lmnt": {
                "conversational": true,
                "length": 12,
                "seed": 7,
                "speed": 1.25,
                "temperature": 0.7,
                "topP": 0.9,
                "sampleRate": 16000
            }
        }))
        .expect("provider options deserialize");
        let provider = create_lmnt(
            LMNTProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-header-value"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.speech("aurora").do_generate(
                SpeechModelCallOptions::new("Hello from the AI SDK!")
                    .with_voice("nova")
                    .with_output_format("wav")
                    .with_speed(1.5)
                    .with_language("en")
                    .with_provider_options(provider_options)
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        assert_eq!(result.audio, FileDataContent::Bytes(vec![1, 2, 3]));
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(result.response.model_id, "aurora");
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
        assert_eq!(
            request.url,
            format!("{DEFAULT_LMNT_BASE_URL}/v1/ai/speech/bytes")
        );
        assert_eq!(
            request.headers.get("x-api-key"),
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
                .contains("ai-sdk/lmnt/")
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
                "model": "aurora",
                "text": "Hello from the AI SDK!",
                "voice": "nova",
                "response_format": "wav",
                "speed": 1.25,
                "language": "en",
                "conversational": true,
                "length": 12.0,
                "seed": 7,
                "temperature": 0.7,
                "top_p": 0.9,
                "sample_rate": 16000
            })
        );
    }

    #[test]
    fn lmnt_speech_model_defaults_voice_format_and_warns_for_unsupported_format() {
        let (captured_request, transport) =
            capture_transport(ProviderApiResponse::bytes(200, "OK", vec![9]));
        let model = LMNTProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("blizzard");

        let result = poll_ready(
            model.do_generate(SpeechModelCallOptions::new("Hello.").with_output_format("flac")),
        );

        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "outputFormat".to_string(),
                details: Some("Unsupported output format: flac. Using mp3 instead.".to_string())
            }]
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
                "model": "blizzard",
                "text": "Hello.",
                "voice": "ava",
                "response_format": "mp3"
            })
        );
    }

    #[test]
    fn lmnt_speech_model_maps_error_response_to_metadata() {
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
        let model = LMNTProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .with_current_date(fixed_timestamp)
            .speech("aurora");

        let result = poll_ready(model.do_generate(SpeechModelCallOptions::new("Hello.")));

        assert_eq!(result.audio, FileDataContent::Bytes(Vec::new()));
        assert_eq!(
            result.provider_metadata,
            Some(
                serde_json::from_value(json!({
                    "lmnt": {
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

        assert!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .is_some()
        );
    }

    #[test]
    fn lmnt_provider_reports_unsupported_model_families_and_trait_speech() {
        let provider = LMNTProvider::new().with_api_key("test-api-key");

        let language_error = Provider::language_model(&provider, "gpt")
            .err()
            .expect("language models are unsupported");
        assert_eq!(language_error.model_type(), ModelType::LanguageModel);
        assert_eq!(
            language_error.message(),
            "LMNT does not provide language models"
        );

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            embedding_error.message(),
            "LMNT does not provide embedding models"
        );

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(image_error.message(), "LMNT does not provide image models");

        let speech_model = ProviderWithSpeechModel::speech_model(&provider, "aurora")
            .expect("speech model resolves");
        assert_eq!(speech_model.provider(), "lmnt.speech");
        assert_eq!(speech_model.model_id(), "aurora");
    }

    #[test]
    fn lmnt_default_factory_creates_speech_model() {
        let model = lmnt("aurora");

        assert_eq!(model.provider(), "lmnt.speech");
        assert_eq!(model.model_id(), "aurora");
    }
}
