use std::collections::BTreeMap;
use std::env;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderWithTranscriptionModel,
};
use crate::provider_utils::without_trailing_slash;
use crate::provider_utils::{
    FetchErrorInfo, FormData, FormDataValue, HandledFetchError, PostFormDataToApiOptions,
    ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers, convert_base64_to_bytes,
    create_json_error_response_handler, create_json_response_handler, media_type_to_extension,
    post_form_data_to_api, with_user_agent_suffix,
};
use crate::transcription_model::{
    TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
    TranscriptionModelResult, TranscriptionModelSegment,
};

/// Default base URL for upstream `@ai-sdk/groq` API calls.
pub const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Settings for the upstream Groq provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroqProviderSettings {
    /// Base URL for Groq API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Groq API key. When omitted, `GROQ_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl GroqProviderSettings {
    /// Creates empty Groq provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Groq API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Groq API key.
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

/// Upstream Groq provider foundation.
#[derive(Clone)]
pub struct GroqProvider {
    settings: GroqProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// Groq transcription model for `/audio/transcriptions` calls.
#[derive(Clone)]
pub struct GroqTranscriptionModel {
    model_id: String,
    base_url: String,
    settings: GroqProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl GroqProvider {
    /// Creates a Groq provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(GroqProviderSettings::new())
    }

    /// Creates a provider from explicit Groq settings.
    pub fn from_settings(settings: GroqProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Groq API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Groq API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates a Groq chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Groq chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Reports that Groq does not expose embedding models through this Rust slice.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`GroqProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Groq does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    /// Creates a Groq transcription model.
    pub fn transcription(&self, model_id: impl Into<String>) -> GroqTranscriptionModel {
        GroqTranscriptionModel::new(
            model_id,
            groq_base_url(&self.settings),
            self.settings.clone(),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_groq_transport),
        )
    }

    /// Creates a Groq transcription model.
    pub fn transcription_model(&self, model_id: impl Into<String>) -> GroqTranscriptionModel {
        self.transcription(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("groq", groq_base_url(&self.settings))
                .with_supports_structured_outputs(true)
                .with_user_agent_suffix(format!("ai-sdk/groq/{}", crate::VERSION));

        if let Some(api_key) = groq_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for GroqProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GroqProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(GroqProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        GroqProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        GroqProvider::image_model(self, model_id)
    }
}

impl ProviderWithTranscriptionModel for GroqProvider {
    type TranscriptionModel = GroqTranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        Ok(GroqProvider::transcription_model(self, model_id))
    }
}

impl GroqTranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: GroqProviderSettings,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
            settings,
            transport,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "groq.transcription"
    }

    async fn do_generate_result(
        &self,
        options: TranscriptionModelCallOptions,
    ) -> TranscriptionModelResult {
        let timestamp = OffsetDateTime::now_utc();
        let form_data = groq_transcription_form_data(&self.model_id, &options);
        let request_headers = groq_transcription_headers(&self.settings, options.headers.as_ref());
        let post_options = PostFormDataToApiOptions::new(
            format!("{}/audio/transcriptions", self.base_url),
            form_data,
        )
        .with_headers(request_headers)
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal);
        let transport = Arc::clone(&self.transport);

        match post_form_data_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    groq_transcription_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    groq_error_response,
                    groq_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => groq_transcription_result_from_response(
                &self.model_id,
                timestamp,
                response.value,
                response.raw_value,
                response.response_headers,
            ),
            Err(error) => groq_transcription_result_from_error(&self.model_id, timestamp, error),
        }
    }
}

impl TranscriptionModel for GroqTranscriptionModel {
    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        GroqTranscriptionModel::provider(self)
    }

    fn model_id(&self) -> &str {
        GroqTranscriptionModel::model_id(self)
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Groq provider with explicit settings.
pub fn create_groq(settings: GroqProviderSettings) -> GroqProvider {
    GroqProvider::from_settings(settings)
}

/// Creates a Groq chat language model using default provider settings.
pub fn groq(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    GroqProvider::new().language_model(model_id)
}

fn groq_base_url(settings: &GroqProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_GROQ_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn groq_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("GROQ_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn groq_transcription_headers(
    settings: &GroqProviderSettings,
    call_headers: Option<&Headers>,
) -> BTreeMap<String, Option<String>> {
    let mut headers = Headers::new();

    if let Some(api_key) = groq_api_key(settings.api_key.as_ref()) {
        headers.insert("authorization".to_string(), format!("Bearer {api_key}"));
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), value.clone());
    }

    let headers = with_user_agent_suffix(
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        [format!("ai-sdk/groq/{}", crate::VERSION)],
    );

    combine_headers([
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        call_headers.map(|headers| {
            headers
                .iter()
                .map(|(name, value)| (name.clone(), Some(value.clone())))
                .collect::<Vec<_>>()
        }),
    ])
}

fn groq_transcription_form_data(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> FormData {
    let mut form_data = FormData::new();
    form_data.append("model", FormDataValue::text(model_id));
    form_data.append(
        "file",
        FormDataValue::bytes(groq_audio_bytes(&options.audio)),
    );

    if let Some(groq_options) = options
        .provider_options
        .as_ref()
        .and_then(|options| options.get("groq"))
    {
        groq_transcription_append_option(&mut form_data, groq_options, "language", "language");
        groq_transcription_append_option(&mut form_data, groq_options, "prompt", "prompt");
        groq_transcription_append_option(
            &mut form_data,
            groq_options,
            "responseFormat",
            "response_format",
        );
        groq_transcription_append_option(
            &mut form_data,
            groq_options,
            "temperature",
            "temperature",
        );

        if let Some(JsonValue::Array(values)) = groq_options.get("timestampGranularities") {
            for value in values.iter().filter_map(JsonValue::as_str) {
                form_data.append("timestamp_granularities[]", FormDataValue::text(value));
            }
        }
    }

    let _filename = format!("audio.{}", media_type_to_extension(&options.media_type));
    form_data
}

fn groq_transcription_append_option(
    form_data: &mut FormData,
    options: &JsonObject,
    source: &str,
    target: &str,
) {
    if let Some(value) = options.get(source) {
        match value {
            JsonValue::Null => {}
            JsonValue::String(value) => form_data.append(target, FormDataValue::text(value)),
            JsonValue::Number(value) => {
                form_data.append(target, FormDataValue::text(value.to_string()))
            }
            JsonValue::Bool(value) => {
                form_data.append(target, FormDataValue::text(value.to_string()))
            }
            JsonValue::Array(_) | JsonValue::Object(_) => {
                form_data.append(target, FormDataValue::text(value.to_string()))
            }
        }
    }
}

fn groq_audio_bytes(data: &FileDataContent) -> Vec<u8> {
    match data {
        FileDataContent::Bytes(bytes) => bytes.clone(),
        FileDataContent::Base64(base64) => {
            convert_base64_to_bytes(base64).unwrap_or_else(|_| base64.as_bytes().to_vec())
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct GroqTranscriptionResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    segments: Option<Vec<GroqTranscriptionSegment>>,
}

#[derive(Clone, Debug, Deserialize)]
struct GroqTranscriptionSegment {
    start: f64,
    end: f64,
    text: String,
}

fn groq_transcription_response(
    value: &JsonValue,
) -> Result<GroqTranscriptionResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn groq_error_response(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn groq_error_message(value: &JsonValue) -> String {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| value.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Unknown error")
        .to_string()
}

fn groq_transcription_result_from_response(
    model_id: &str,
    timestamp: OffsetDateTime,
    response: GroqTranscriptionResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> TranscriptionModelResult {
    let mut model_response = TranscriptionModelResponse::new(timestamp, model_id);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            model_response = model_response.with_header(name, value);
        }
    }

    if let Some(raw_response) = raw_response {
        model_response = model_response.with_body(raw_response);
    }

    let mut result = TranscriptionModelResult::new(
        response.text,
        response
            .segments
            .unwrap_or_default()
            .into_iter()
            .map(|segment| TranscriptionModelSegment::new(segment.text, segment.start, segment.end))
            .collect(),
        model_response,
    );

    if let Some(language) = response.language {
        result = result.with_language(language);
    }

    if let Some(duration) = response.duration {
        result = result.with_duration_in_seconds(duration);
    }

    result
}

fn groq_transcription_result_from_error(
    model_id: &str,
    timestamp: OffsetDateTime,
    error: HandledFetchError,
) -> TranscriptionModelResult {
    let (message, headers, body) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    };
    let mut response = TranscriptionModelResponse::new(timestamp, model_id);

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    if let Some(body) = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
    {
        response = response.with_body(body);
    }

    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));

    TranscriptionModelResult::new("", Vec::new(), response)
        .with_provider_metadata(ProviderMetadata::from([("groq".to_string(), extra)]))
}

fn default_groq_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_groq_request(request))))
}

fn execute_groq_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => Err(FetchErrorInfo::new(
            "GET requests require an injected Groq transport",
        )),
        ProviderApiRequestMethod::Post => match request.body {
            Some(ProviderApiRequestBody::FormData { .. }) => Err(FetchErrorInfo::new(
                "multipart form data requires an injected Groq transport",
            )),
            _ => Err(FetchErrorInfo::new(
                "Groq transcription requests require an injected Groq transport",
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_GROQ_BASE_URL, GroqProvider, GroqProviderSettings, create_groq, groq};
    use crate::file_data::FileDataContent;
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderOptions, ProviderWithTranscriptionModel};
    use crate::provider_utils::{
        FormDataValue, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse,
    };
    use crate::transcription_model::{TranscriptionModel, TranscriptionModelCallOptions};
    use serde_json::json;
    use std::future::{Future, ready};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

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

    #[test]
    fn groq_provider_creates_chat_model_with_headers_base_url_and_provider_options() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-groq",
                        "created": 1711115037,
                        "model": "llama-3.3-70b-versatile",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Groq"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 4,
                            "total_tokens": 8
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_groq".to_string(),
                )])))))
            });
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "groq": {
                "serviceTier": "flex"
            }
        }))
        .expect("provider options deserialize");
        let provider = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("llama-3.3-70b-versatile");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "groq.chat");
        assert_eq!(result.text, "Hello from Groq");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.groq.test/openai/v1/chat/completions"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/groq/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "llama-3.3-70b-versatile",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "service_tier": "flex"
            }))
        );
    }

    #[test]
    fn groq_provider_uses_default_base_url_and_function_alias() {
        let model = groq("llama-3.1-8b-instant");

        assert_eq!(model.provider(), "groq.chat");
        assert_eq!(model.model_id(), "llama-3.1-8b-instant");
        assert_eq!(DEFAULT_GROQ_BASE_URL, "https://api.groq.com/openai/v1");
    }

    #[test]
    fn groq_provider_reports_unsupported_model_families() {
        let provider = GroqProvider::new();

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn groq_provider_creates_transcription_model_with_headers_options_and_response() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "task": "transcribe",
                        "language": "English",
                        "duration": 2.5,
                        "text": "Hello world!",
                        "segments": [
                            {
                                "id": 0,
                                "seek": 0,
                                "start": 0.0,
                                "end": 2.48,
                                "text": "Hello world!",
                                "tokens": [50365, 2425, 490, 264],
                                "temperature": 0,
                                "avg_logprob": -0.29010406,
                                "compression_ratio": 0.7777778,
                                "no_speech_prob": 0.032802984
                            }
                        ],
                        "x_groq": {
                            "id": "req_01jrh9nn61f24rydqq1r4b3yg5"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_groq_transcription".to_string(),
                )])))))
            });
        let provider = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "groq": {
                "language": "en",
                "prompt": "Meeting notes",
                "responseFormat": "verbose_json",
                "temperature": 0,
                "timestampGranularities": ["segment"]
            }
        }))
        .expect("provider options deserialize");
        let model = provider.transcription("whisper-large-v3-turbo");
        let result = poll_ready(
            model.do_generate(
                TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3]),
                    "audio/wav",
                )
                .with_provider_options(provider_options)
                .with_header("x-call", "transcribe"),
            ),
        );

        assert_eq!(model.provider(), "groq.transcription");
        assert_eq!(model.model_id(), "whisper-large-v3-turbo");
        assert_eq!(result.text, "Hello world!");
        assert_eq!(result.language.as_deref(), Some("English"));
        assert_eq!(result.duration_in_seconds, Some(2.5));
        assert_eq!(result.segments[0].start_second, 0.0);
        assert_eq!(result.segments[0].end_second, 2.48);
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_groq_transcription")
        );
        assert!(
            result
                .response
                .body
                .as_ref()
                .and_then(|body| body.get("x_groq"))
                .is_some()
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.groq.test/openai/v1/audio/transcriptions"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("transcribe")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/groq/0.1.0"))
        );

        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("transcription request uses form data");
        assert_eq!(
            form_text(form_data, "model"),
            Some("whisper-large-v3-turbo")
        );
        assert_eq!(form_text(form_data, "language"), Some("en"));
        assert_eq!(form_text(form_data, "prompt"), Some("Meeting notes"));
        assert_eq!(
            form_text(form_data, "response_format"),
            Some("verbose_json")
        );
        assert_eq!(form_text(form_data, "temperature"), Some("0"));
        assert_eq!(
            form_text(form_data, "timestamp_granularities[]"),
            Some("segment")
        );
        assert_eq!(form_bytes(form_data, "file"), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn groq_provider_implements_transcription_trait() {
        let provider = GroqProvider::new();
        let model =
            ProviderWithTranscriptionModel::transcription_model(&provider, "whisper-large-v3")
                .expect("transcription model resolves");

        assert_eq!(model.provider(), "groq.transcription");
        assert_eq!(model.model_id(), "whisper-large-v3");
    }

    #[test]
    fn groq_provider_implements_provider_trait() {
        let provider = GroqProvider::new();
        let model =
            Provider::language_model(&provider, "llama-3.3-70b-versatile").expect("model resolves");

        assert_eq!(model.provider(), "groq.chat");
        assert_eq!(model.model_id(), "llama-3.3-70b-versatile");
    }

    #[test]
    fn groq_provider_settings_serde_accepts_upstream_base_url() {
        let settings: GroqProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.groq.test/openai/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "groq"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            GroqProviderSettings::new()
                .with_base_url("https://api.groq.test/openai/v1/")
                .with_api_key("key")
                .with_header("x-provider", "groq")
        );
    }

    fn form_text<'a>(
        form_data: &'a crate::provider_utils::FormData,
        name: &str,
    ) -> Option<&'a str> {
        form_data.get(name).and_then(|value| match value {
            FormDataValue::Text { value } => Some(value.as_str()),
            FormDataValue::Bytes { .. } => None,
        })
    }

    fn form_bytes<'a>(
        form_data: &'a crate::provider_utils::FormData,
        name: &str,
    ) -> Option<&'a [u8]> {
        form_data.get(name).and_then(|value| match value {
            FormDataValue::Bytes { value } => Some(value.as_slice()),
            FormDataValue::Text { .. } => None,
        })
    }
}
