use std::collections::BTreeMap;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelRawStreamPart, LanguageModelReasoning, LanguageModelReasoningDelta,
    LanguageModelReasoningEnd, LanguageModelReasoningStart, LanguageModelRequest,
    LanguageModelResponse, LanguageModelResponseFormat, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelUsage, OutputTokenUsage,
};
use crate::provider::{ProviderMetadata, SpecificationVersion};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, ParseJsonResult, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, post_json_to_api, with_user_agent_suffix, without_trailing_slash,
};
use crate::warning::Warning;

/// Future returned by an injected OpenAI-compatible HTTP transport.
pub type OpenAICompatibleTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by OpenAI-compatible provider models.
pub type OpenAICompatibleTransport =
    Arc<dyn Fn(ProviderApiRequest) -> OpenAICompatibleTransportFuture + Send + Sync>;

/// Settings for an OpenAI-compatible provider instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompatibleProviderSettings {
    /// Base URL for API calls, without the endpoint path.
    pub base_url: String,

    /// Provider name used as the provider id prefix.
    pub name: String,

    /// API key used to build a `Bearer` authorization header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom headers included in model requests.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Custom query parameters appended to model request URLs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query_params: BTreeMap<String, String>,

    /// Include usage information in streaming responses when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_usage: Option<bool>,

    /// Whether chat models support structured JSON schema outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_structured_outputs: Option<bool>,
}

impl OpenAICompatibleProviderSettings {
    /// Creates OpenAI-compatible provider settings.
    pub fn new(name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            api_key: None,
            headers: Headers::new(),
            query_params: BTreeMap::new(),
            include_usage: None,
            supports_structured_outputs: None,
        }
    }

    /// Sets the API key used for bearer authentication.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a custom request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Adds a custom URL query parameter.
    pub fn with_query_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.query_params.insert(name.into(), value.into());
        self
    }

    /// Sets whether streamed requests should include usage when supported.
    pub fn with_include_usage(mut self, include_usage: bool) -> Self {
        self.include_usage = Some(include_usage);
        self
    }

    /// Sets whether chat models support structured JSON schema outputs.
    pub fn with_supports_structured_outputs(mut self, supports_structured_outputs: bool) -> Self {
        self.supports_structured_outputs = Some(supports_structured_outputs);
        self
    }
}

/// OpenAI-compatible provider.
#[derive(Clone)]
pub struct OpenAICompatibleProvider {
    settings: OpenAICompatibleProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl OpenAICompatibleProvider {
    /// Creates a provider from explicit OpenAI-compatible settings.
    pub fn from_settings(settings: OpenAICompatibleProviderSettings) -> Self {
        Self {
            settings,
            transport: default_openai_compatible_transport(),
        }
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Creates the default OpenAI-compatible chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates an OpenAI-compatible chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        OpenAICompatibleChatLanguageModel::new(
            model_id,
            openai_compatible_model_config("chat", &self.settings, &self.transport),
        )
    }

    /// Creates an OpenAI-compatible completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        OpenAICompatibleCompletionLanguageModel::new(
            model_id,
            openai_compatible_model_config("completion", &self.settings, &self.transport),
        )
    }

    /// Creates an OpenAI-compatible embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        OpenAICompatibleEmbeddingModel::new(
            model_id,
            openai_compatible_model_config("embedding", &self.settings, &self.transport),
        )
    }

    /// Deprecated upstream alias for [`OpenAICompatibleProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates an OpenAI-compatible image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        OpenAICompatibleImageModel::new(
            model_id,
            openai_compatible_model_config("image", &self.settings, &self.transport),
        )
    }
}

/// Creates an OpenAI-compatible provider.
pub fn create_openai_compatible(
    settings: OpenAICompatibleProviderSettings,
) -> OpenAICompatibleProvider {
    OpenAICompatibleProvider::from_settings(settings)
}

#[derive(Clone)]
struct OpenAICompatibleModelConfig {
    provider: String,
    settings: OpenAICompatibleProviderSettings,
    transport: OpenAICompatibleTransport,
}

fn openai_compatible_model_config(
    model_type: &str,
    settings: &OpenAICompatibleProviderSettings,
    transport: &OpenAICompatibleTransport,
) -> OpenAICompatibleModelConfig {
    OpenAICompatibleModelConfig {
        provider: format!("{}.{}", settings.name, model_type),
        settings: settings.clone(),
        transport: Arc::clone(transport),
    }
}

/// OpenAI-compatible chat language model.
#[derive(Clone)]
pub struct OpenAICompatibleChatLanguageModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleChatLanguageModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    /// Returns whether structured outputs are enabled for this chat model.
    pub fn supports_structured_outputs(&self) -> bool {
        self.config
            .settings
            .supports_structured_outputs
            .unwrap_or(false)
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) =
            openai_compatible_chat_request_body(&self.model_id, &self.config.settings, &options);
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/chat/completions") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_error_generate_result(
                    &self.config.settings.name,
                    message,
                    request_body_for_error,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    clone_json_value,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => self.generate_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
                warnings,
            ),
            Err(error) => self.generate_result_from_error(error, request_body_for_error),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (request_body, warnings) = openai_compatible_chat_stream_request_body(
            &self.model_id,
            &self.config.settings,
            &options,
        );
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/chat/completions") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_error_stream_result(
                    message,
                    request_body_for_error,
                    None,
                    None,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    clone_json_value,
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => openai_compatible_stream_result_from_response(
                &self.config.settings.name,
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
            ),
            Err(error) => self.stream_result_from_error(error, request_body_for_error),
        }
    }

    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                openai_compatible_provider_headers(&self.config.settings)
                    .into_iter()
                    .map(|(name, value)| (name, Some(value)))
                    .collect::<Vec<_>>(),
            ),
            optional_headers(call_headers),
        ])
    }

    fn generate_result_from_response(
        &self,
        response: JsonValue,
        raw_response: Option<JsonValue>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let choice = response
            .get("choices")
            .and_then(JsonValue::as_array)
            .and_then(|choices| choices.first());
        let message = choice.and_then(|choice| choice.get("message"));
        let content = openai_compatible_response_content(message);
        let finish_reason = openai_compatible_finish_reason(
            choice
                .and_then(|choice| choice.get("finish_reason"))
                .or_else(|| choice.and_then(|choice| choice.get("finishReason"))),
        );
        let usage = openai_compatible_usage(response.get("usage"));
        let raw_body = raw_response.unwrap_or_else(|| response.clone());

        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_request(LanguageModelRequest::new().with_body(request_body));
        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

        if let Some(id) = json_string(response.get("id")) {
            response_metadata = response_metadata.with_id(id);
        }

        if let Some(timestamp) = openai_compatible_response_timestamp(response.get("created")) {
            response_metadata = response_metadata.with_timestamp(timestamp);
        }

        if let Some(model_id) = json_string(response.get("model")) {
            response_metadata = response_metadata.with_model_id(model_id);
        }

        if let Some(headers) = response_headers {
            response_metadata = with_response_headers(response_metadata, headers);
        }

        let metadata = openai_compatible_provider_metadata(&self.config.settings.name, &response);
        if !metadata.is_empty() {
            result = result.with_provider_metadata(metadata);
        }

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result.with_response(response_metadata)
    }

    fn generate_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelGenerateResult {
        let message = match error {
            HandledFetchError::Original { error } => error.message().to_string(),
            HandledFetchError::ApiCall { error } => error.message().to_string(),
        };

        openai_compatible_error_generate_result(&self.config.settings.name, message, request_body)
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (message, headers, body) = match error {
            HandledFetchError::Original { error } => (error.message().to_string(), None, None),
            HandledFetchError::ApiCall { error } => (
                error.message().to_string(),
                error.response_headers().cloned(),
                error.response_body().map(String::from),
            ),
        };

        openai_compatible_error_stream_result(message, request_body, headers, body.as_deref())
    }
}

impl LanguageModel for OpenAICompatibleChatLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

/// OpenAI-compatible completion language model.
#[derive(Clone)]
pub struct OpenAICompatibleCompletionLanguageModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleCompletionLanguageModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    #[cfg(test)]
    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    #[cfg(test)]
    fn request_headers(&self) -> Headers {
        openai_compatible_provider_headers(&self.config.settings)
    }
}

/// OpenAI-compatible embedding model.
#[derive(Clone)]
pub struct OpenAICompatibleEmbeddingModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleEmbeddingModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    #[cfg(test)]
    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    #[cfg(test)]
    fn request_headers(&self) -> Headers {
        openai_compatible_provider_headers(&self.config.settings)
    }
}

/// OpenAI-compatible image model.
#[derive(Clone)]
pub struct OpenAICompatibleImageModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleImageModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    #[cfg(test)]
    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    #[cfg(test)]
    fn request_headers(&self) -> Headers {
        openai_compatible_provider_headers(&self.config.settings)
    }
}

fn openai_compatible_url(
    settings: &OpenAICompatibleProviderSettings,
    path: &str,
) -> Result<String, String> {
    let base_url = without_trailing_slash(Some(settings.base_url.as_str()))
        .unwrap_or(settings.base_url.as_str());
    let mut url = Url::parse(&format!("{base_url}{path}"))
        .map_err(|error| format!("invalid OpenAI-compatible base URL: {error}"))?;

    if !settings.query_params.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in &settings.query_params {
            pairs.append_pair(name, value);
        }
    }

    Ok(url.to_string())
}

fn openai_compatible_provider_headers(settings: &OpenAICompatibleProviderSettings) -> Headers {
    let mut headers = Vec::new();

    if let Some(api_key) = settings
        .api_key
        .as_ref()
        .filter(|api_key| !api_key.is_empty())
    {
        headers.push((
            "authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        ));
    }

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/openai-compatible/{}", crate::VERSION)],
    )
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn openai_compatible_chat_request_body(
    model_id: &str,
    settings: &OpenAICompatibleProviderSettings,
    options: &LanguageModelCallOptions,
) -> (JsonValue, Vec<Warning>) {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert(
        "messages".to_string(),
        JsonValue::Array(openai_compatible_messages(&options.prompt)),
    );

    if let Some(max_output_tokens) = options.max_output_tokens {
        body.insert("max_tokens".to_string(), json!(max_output_tokens));
    }

    if let Some(temperature) = options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }

    if let Some(top_p) = options.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }

    if let Some(presence_penalty) = options.presence_penalty {
        body.insert("presence_penalty".to_string(), json!(presence_penalty));
    }

    if let Some(frequency_penalty) = options.frequency_penalty {
        body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
    }

    if let Some(stop_sequences) = &options.stop_sequences {
        body.insert("stop".to_string(), json!(stop_sequences));
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), json!(seed));
    }

    if options.top_k.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "topK".to_string(),
            details: None,
        });
    }

    if let Some(response_format) = &options.response_format {
        if let Some(value) =
            openai_compatible_response_format(response_format, settings, &mut warnings)
        {
            body.insert("response_format".to_string(), value);
        }
    }

    (JsonValue::Object(body), warnings)
}

fn openai_compatible_chat_stream_request_body(
    model_id: &str,
    settings: &OpenAICompatibleProviderSettings,
    options: &LanguageModelCallOptions,
) -> (JsonValue, Vec<Warning>) {
    let (mut body, warnings) = openai_compatible_chat_request_body(model_id, settings, options);

    if let Some(body) = body.as_object_mut() {
        body.insert("stream".to_string(), JsonValue::Bool(true));

        if settings.include_usage == Some(true) {
            body.insert(
                "stream_options".to_string(),
                json!({
                    "include_usage": true
                }),
            );
        }
    }

    (body, warnings)
}

fn openai_compatible_response_format(
    response_format: &LanguageModelResponseFormat,
    settings: &OpenAICompatibleProviderSettings,
    warnings: &mut Vec<Warning>,
) -> Option<JsonValue> {
    match response_format {
        LanguageModelResponseFormat::Text => None,
        LanguageModelResponseFormat::Json {
            schema,
            name,
            description,
        } => {
            if let Some(schema) = schema
                && settings.supports_structured_outputs == Some(true)
            {
                let mut json_schema = JsonObject::new();
                json_schema.insert("schema".to_string(), JsonValue::Object(schema.clone()));
                json_schema.insert("strict".to_string(), JsonValue::Bool(true));
                json_schema.insert(
                    "name".to_string(),
                    JsonValue::String(name.clone().unwrap_or_else(|| "response".to_string())),
                );

                if let Some(description) = description {
                    json_schema.insert(
                        "description".to_string(),
                        JsonValue::String(description.clone()),
                    );
                }

                return Some(json!({
                    "type": "json_schema",
                    "json_schema": json_schema
                }));
            }

            if schema.is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "responseFormat".to_string(),
                    details: Some(
                        "JSON response format schema is only supported with structuredOutputs"
                            .to_string(),
                    ),
                });
            }

            Some(json!({
                "type": "json_object"
            }))
        }
    }
}

fn openai_compatible_messages(prompt: &[LanguageModelMessage]) -> Vec<JsonValue> {
    prompt
        .iter()
        .filter_map(|message| match message {
            LanguageModelMessage::System(message) => Some(json!({
                "role": "system",
                "content": message.content
            })),
            LanguageModelMessage::User(message) => Some(json!({
                "role": "user",
                "content": message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        crate::language_model::LanguageModelUserContentPart::Text(text) => {
                            Some(text.text.as_str())
                        }
                        crate::language_model::LanguageModelUserContentPart::File(_) => None,
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })),
            LanguageModelMessage::Assistant(message) => Some(json!({
                "role": "assistant",
                "content": message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        LanguageModelAssistantContentPart::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })),
            LanguageModelMessage::Tool(_) => None,
        })
        .collect()
}

fn openai_compatible_response_content(message: Option<&JsonValue>) -> Vec<LanguageModelContent> {
    let mut content = Vec::new();
    let Some(message) = message else {
        return content;
    };

    if let Some(text) = message.get("content").and_then(JsonValue::as_str)
        && !text.is_empty()
    {
        content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
    }

    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(JsonValue::as_str)
        && !reasoning.is_empty()
    {
        content.push(LanguageModelContent::Reasoning(
            LanguageModelReasoning::new(reasoning),
        ));
    }

    content
}

fn openai_compatible_finish_reason(value: Option<&JsonValue>) -> LanguageModelFinishReason {
    let raw = json_string(value).unwrap_or_else(|| "unknown".to_string());
    let unified = match raw.as_str() {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        "tool_calls" => FinishReason::ToolCalls,
        "error" => FinishReason::Error,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason {
        unified,
        raw: Some(raw),
    }
}

fn openai_compatible_usage(value: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(value) = value else {
        return LanguageModelUsage::default();
    };

    let input_total = json_u64(
        value
            .get("prompt_tokens")
            .or_else(|| value.get("promptTokens"))
            .or_else(|| value.get("input_tokens"))
            .or_else(|| value.get("inputTokens")),
    );
    let output_total = json_u64(
        value
            .get("completion_tokens")
            .or_else(|| value.get("completionTokens"))
            .or_else(|| value.get("output_tokens"))
            .or_else(|| value.get("outputTokens")),
    );
    let cache_read = json_u64(value.get("prompt_tokens_details").and_then(|details| {
        details
            .get("cached_tokens")
            .or_else(|| details.get("cachedTokens"))
    }));
    let reasoning_tokens = json_u64(
        value
            .get("completion_tokens_details")
            .and_then(|details| {
                details
                    .get("reasoning_tokens")
                    .or_else(|| details.get("reasoningTokens"))
            })
            .or_else(|| value.get("reasoning_tokens"))
            .or_else(|| value.get("reasoningTokens")),
    );
    let raw = value.as_object().cloned();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_total,
            no_cache: input_total
                .zip(cache_read)
                .map(|(total, cached)| total.saturating_sub(cached)),
            cache_read,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: output_total,
            text: output_total
                .map(|total| total.saturating_sub(reasoning_tokens.unwrap_or_default())),
            reasoning: reasoning_tokens,
        },
        raw,
    }
}

fn openai_compatible_provider_metadata(
    provider_name: &str,
    response: &JsonValue,
) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider_metadata = JsonObject::new();

    add_openai_compatible_prediction_metadata(&mut provider_metadata, response.get("usage"));

    if !provider_metadata.is_empty() {
        metadata.insert(provider_name.to_string(), provider_metadata);
    }

    metadata
}

fn openai_compatible_stream_result_from_response(
    provider_name: &str,
    events: Vec<ParseJsonResult<JsonValue>>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    include_raw_chunks: bool,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut stream = vec![LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    )];
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage = None::<JsonValue>;
    let mut is_first_chunk = true;
    let mut is_active_reasoning = false;
    let mut is_active_text = false;

    for event in events {
        match event {
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        LanguageModelRawStreamPart::new(raw_value.clone()),
                    ));
                }

                if value.get("error").is_some() {
                    finish_reason = LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: Some("openai-compatible-error".to_string()),
                    };
                    stream.push(openai_compatible_stream_error(
                        openai_compatible_error_message(&value),
                        Some(&raw_value.to_string()),
                    ));
                    continue;
                }

                if is_first_chunk {
                    is_first_chunk = false;
                    stream.push(LanguageModelStreamPart::ResponseMetadata(
                        openai_compatible_stream_response_metadata(&value),
                    ));
                }

                if let Some(event_usage) = value.get("usage") {
                    usage = Some(event_usage.clone());
                }

                let Some(choice) = value
                    .get("choices")
                    .and_then(JsonValue::as_array)
                    .and_then(|choices| choices.first())
                else {
                    continue;
                };

                if let Some(raw_finish_reason) = choice.get("finish_reason") {
                    finish_reason = openai_compatible_finish_reason(Some(raw_finish_reason));
                }

                let Some(delta) = choice.get("delta") else {
                    continue;
                };

                let reasoning = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .and_then(JsonValue::as_str)
                    .filter(|reasoning| !reasoning.is_empty());
                if let Some(reasoning) = reasoning {
                    if !is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningStart(
                            LanguageModelReasoningStart::new("reasoning-0"),
                        ));
                        is_active_reasoning = true;
                    }

                    stream.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new("reasoning-0", reasoning),
                    ));
                }

                let text = delta
                    .get("content")
                    .and_then(JsonValue::as_str)
                    .filter(|text| !text.is_empty());
                if let Some(text) = text {
                    if is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        is_active_reasoning = false;
                    }

                    if !is_active_text {
                        stream.push(LanguageModelStreamPart::TextStart(
                            LanguageModelTextStart::new("txt-0"),
                        ));
                        is_active_text = true;
                    }

                    stream.push(LanguageModelStreamPart::TextDelta(
                        LanguageModelTextDelta::new("txt-0", text),
                    ));
                }

                if delta.get("tool_calls").is_some() {
                    if is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        is_active_reasoning = false;
                    }

                    finish_reason = LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: Some("openai-compatible-unported-tool-calls".to_string()),
                    };
                    stream.push(openai_compatible_stream_error(
                        "OpenAI-compatible streamed tool calls are not implemented yet",
                        Some(&raw_value.to_string()),
                    ));
                }
            }
            ParseJsonResult::Failure { error, raw_value } => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("openai-compatible-parse-error".to_string()),
                };
                stream.push(openai_compatible_stream_error(
                    error.to_string(),
                    raw_value.as_ref().map(JsonValue::to_string).as_deref(),
                ));
            }
        }
    }

    if is_active_reasoning {
        stream.push(LanguageModelStreamPart::ReasoningEnd(
            LanguageModelReasoningEnd::new("reasoning-0"),
        ));
    }

    if is_active_text {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            "txt-0",
        )));
    }

    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(openai_compatible_usage(usage.as_ref()), finish_reason)
            .with_provider_metadata(openai_compatible_stream_provider_metadata(
                provider_name,
                usage.as_ref(),
            )),
    ));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(with_stream_response_headers(
            LanguageModelStreamResultResponse::new(),
            headers,
        ));
    }

    result
}

fn openai_compatible_stream_response_metadata(
    value: &JsonValue,
) -> LanguageModelStreamResponseMetadata {
    let mut metadata = LanguageModelStreamResponseMetadata::new();

    if let Some(id) = json_string(value.get("id")) {
        metadata = metadata.with_id(id);
    }

    if let Some(timestamp) = openai_compatible_response_timestamp(value.get("created")) {
        metadata = metadata.with_timestamp(timestamp);
    }

    if let Some(model_id) = json_string(value.get("model")) {
        metadata = metadata.with_model_id(model_id);
    }

    metadata
}

fn openai_compatible_stream_provider_metadata(
    provider_name: &str,
    usage: Option<&JsonValue>,
) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider_metadata = JsonObject::new();
    add_openai_compatible_prediction_metadata(&mut provider_metadata, usage);
    metadata.insert(provider_name.to_string(), provider_metadata);
    metadata
}

fn add_openai_compatible_prediction_metadata(
    provider_metadata: &mut JsonObject,
    usage: Option<&JsonValue>,
) {
    if let Some(completion_token_details) =
        usage.and_then(|usage| usage.get("completion_tokens_details"))
    {
        if let Some(accepted_prediction_tokens) = json_u64(
            completion_token_details
                .get("accepted_prediction_tokens")
                .or_else(|| completion_token_details.get("acceptedPredictionTokens")),
        ) {
            provider_metadata.insert(
                "acceptedPredictionTokens".to_string(),
                json!(accepted_prediction_tokens),
            );
        }

        if let Some(rejected_prediction_tokens) = json_u64(
            completion_token_details
                .get("rejected_prediction_tokens")
                .or_else(|| completion_token_details.get("rejectedPredictionTokens")),
        ) {
            provider_metadata.insert(
                "rejectedPredictionTokens".to_string(),
                json!(rejected_prediction_tokens),
            );
        }
    }
}

fn openai_compatible_error_generate_result(
    provider_name: &str,
    message: String,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("openai-compatible-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(openai_compatible_error_metadata(provider_name, message))
}

fn openai_compatible_error_stream_result(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut result =
        LanguageModelStreamResult::new(vec![openai_compatible_stream_error(message, raw_body)])
            .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(with_stream_response_headers(
            LanguageModelStreamResultResponse::new(),
            headers,
        ));
    }

    result
}

fn openai_compatible_stream_error(
    message: impl Into<String>,
    raw_body: Option<&str>,
) -> LanguageModelStreamPart {
    let mut error = JsonObject::new();
    error.insert("message".to_string(), JsonValue::String(message.into()));

    if let Some(raw_body) = raw_body {
        error.insert("body".to_string(), JsonValue::String(raw_body.to_string()));
    }

    LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(JsonValue::Object(error)))
}

fn openai_compatible_error_metadata(provider_name: &str, message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(provider_name.to_string(), provider);
    metadata
}

fn openai_compatible_error_message(error: &JsonValue) -> String {
    error
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .or_else(|| error.get("message").and_then(JsonValue::as_str))
        .map_or_else(|| error.to_string(), String::from)
}

fn clone_json_value(value: &JsonValue) -> Result<JsonValue, &'static str> {
    Ok(value.clone())
}

fn json_string(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    match value {
        Some(JsonValue::Number(value)) => value.as_u64(),
        Some(JsonValue::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
}

fn openai_compatible_response_timestamp(value: Option<&JsonValue>) -> Option<OffsetDateTime> {
    match value {
        Some(JsonValue::Number(value)) => value
            .as_i64()
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok()),
        Some(JsonValue::String(value)) => value
            .parse::<i64>()
            .ok()
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok()),
        _ => None,
    }
}

fn with_response_headers(
    mut response: LanguageModelResponse,
    headers: Headers,
) -> LanguageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_stream_response_headers(
    mut response: LanguageModelStreamResultResponse,
    headers: Headers,
) -> LanguageModelStreamResultResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn default_openai_compatible_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_openai_compatible_request(request))))
}

fn execute_openai_compatible_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_openai_compatible_get_request(request),
        ProviderApiRequestMethod::Post => execute_openai_compatible_post_request(request),
    }
}

fn execute_openai_compatible_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    provider_api_response(response)
}

fn execute_openai_compatible_post_request(
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
                "multipart form data is not supported by the OpenAI-compatible transport",
            ));
        }
        None => builder.send_empty(),
    };

    provider_api_response(response)
}

fn provider_api_response(
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
    let body = response.body_mut().read_to_string().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;

    Ok(ProviderApiResponse::text(status.as_u16(), status_text, body).with_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::{
        OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
        OpenAICompatibleTransportFuture, create_openai_compatible,
    };
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelMessage,
        LanguageModelResponseFormat, LanguageModelStreamPart, LanguageModelSystemMessage,
    };
    use crate::prompt::Prompt;
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::stream_text::{StreamTextOptions, stream_text};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[test]
    fn openai_compatible_provider_configures_headers_urls_and_model_aliases() {
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_query_param("Custom-Param", "value")
                .with_include_usage(true)
                .with_supports_structured_outputs(true),
        );

        let chat = provider.chat_model("chat-model");
        let language = provider.language_model("language-model");
        let completion = provider.completion_model("completion-model");
        let embedding = provider.embedding_model("embedding-model");
        let text_embedding = provider.text_embedding_model("embedding-model");
        let image = provider.image_model("image-model");

        assert_eq!(chat.provider(), "test-provider.chat");
        assert_eq!(language.model_id(), "language-model");
        assert_eq!(completion.provider(), "test-provider.completion");
        assert_eq!(embedding.provider(), "test-provider.embedding");
        assert_eq!(text_embedding.model_id(), "embedding-model");
        assert_eq!(image.provider(), "test-provider.image");
        assert!(chat.supports_structured_outputs());
        assert_eq!(
            chat.model_url("/v1/chat").expect("url is valid"),
            "https://api.example.com/v1/chat?Custom-Param=value"
        );
        assert_eq!(
            completion
                .model_url("/v1/completions")
                .expect("url is valid"),
            "https://api.example.com/v1/completions?Custom-Param=value"
        );
        assert_eq!(
            embedding.model_url("/v1/embeddings").expect("url is valid"),
            "https://api.example.com/v1/embeddings?Custom-Param=value"
        );
        assert_eq!(
            image.model_url("/v1/images").expect("url is valid"),
            "https://api.example.com/v1/images?Custom-Param=value"
        );

        let headers = chat.request_headers(None);
        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers.get("user-agent").and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
        assert_eq!(
            completion
                .request_headers()
                .get("user-agent")
                .map(String::as_str),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
        assert_eq!(
            embedding
                .request_headers()
                .get("user-agent")
                .map(String::as_str),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
        assert_eq!(
            image
                .request_headers()
                .get("user-agent")
                .map(String::as_str),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
    }

    #[test]
    fn openai_compatible_chat_generates_text_through_generate_text() {
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
                        "id": "chatcmpl-test",
                        "created": 1711115037,
                        "model": "test-chat-model",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from OpenAI-compatible"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 3,
                            "total_tokens": 7,
                            "prompt_tokens_details": {
                                "cached_tokens": 1
                            },
                            "completion_tokens_details": {
                                "reasoning_tokens": 2,
                                "accepted_prediction_tokens": 5,
                                "rejected_prediction_tokens": 1
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_openai_compatible".to_string(),
                )])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_query_param("api-version", "2026-01-01"),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello from OpenAI-compatible");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.input_tokens.cache_read, Some(1));
        assert_eq!(result.usage.input_tokens.no_cache, Some(3));
        assert_eq!(result.usage.output_tokens.total, Some(3));
        assert_eq!(result.usage.output_tokens.reasoning, Some(2));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("acceptedPredictionTokens"))
                .and_then(JsonValue::as_u64),
            Some(5)
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/chat/completions?api-version=2026-01-01"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai-compatible/0.1.0"))
        );

        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.0
            })
        );
    }

    #[test]
    fn openai_compatible_chat_streams_text_through_stream_text() {
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
                    openai_compatible_chat_stream_body(),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    (
                        "x-request-id".to_string(),
                        "req_openai_compatible_stream".to_string(),
                    ),
                ])))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_api_key("test-api-key")
                .with_query_param("api-version", "2026-01-01")
                .with_include_usage(true),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(12)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello stream");
        assert_eq!(result.text_stream, vec!["Hello ", "stream"]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.text, Some(4));
        assert_eq!(result.usage.output_tokens.reasoning, Some(1));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("acceptedPredictionTokens"))
                .and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_openai_compatible_stream")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.example.com/chat/completions?api-version=2026-01-01"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );

        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 12,
                "temperature": 0.0,
                "stream": true,
                "stream_options": {
                    "include_usage": true
                }
            })
        );
    }

    #[test]
    fn openai_compatible_chat_streams_reasoning_raw_chunks_and_parse_errors() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "reasoning_content": "Let me think"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "reasoning": " about this"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "Here's my response"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 2,
                                "completion_tokens": 3
                            }
                        }),
                    ]),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Think first"),
                )])
                .with_include_raw_chunks(true),
            ),
        );

        assert!(matches!(
            result.stream.first(),
            Some(LanguageModelStreamPart::StreamStart(_))
        ));
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ReasoningDelta(part) => Some(part.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Let me think", " about this"]
        );
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::TextDelta(part) => Some(part.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Here's my response"]
        );
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::Stop
                    && finish.usage.input_tokens.total == Some(2)
        ));

        let parse_error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    "data: {not json}\n\ndata: [DONE]\n\n",
                ))))
            });
        let parse_error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(parse_error_transport)
        .chat_model("test-chat-model");
        let parse_error_result =
            poll_ready(parse_error_model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert!(
            parse_error_result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Error(_)))
        );
        assert!(matches!(
            parse_error_result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::Error
        ));
    }

    #[test]
    fn openai_compatible_chat_maps_response_formats_and_warnings() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "{}",
                                    "reasoning_content": "reasoning"
                                },
                                "finish_reason": "length"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("JSON only"),
                )])
                .with_top_k(4)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(
                        serde_json::from_value(json!({
                            "type": "object",
                            "properties": {}
                        }))
                        .expect("schema deserializes"),
                    ),
                ),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Length);
        assert_eq!(result.content.len(), 2);
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
    }

    fn openai_compatible_chat_stream_body() -> String {
        sse_body([
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "test-chat-model",
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "role": "assistant",
                            "content": ""
                        },
                        "finish_reason": null
                    }
                ]
            }),
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "test-chat-model",
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "content": "Hello "
                        },
                        "finish_reason": null
                    }
                ]
            }),
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "test-chat-model",
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "content": "stream"
                        },
                        "finish_reason": null
                    }
                ]
            }),
            json!({
                "id": "chatcmpl-stream-test",
                "created": 1711115037,
                "model": "test-chat-model",
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 4,
                    "completion_tokens": 5,
                    "completion_tokens_details": {
                        "reasoning_tokens": 1,
                        "accepted_prediction_tokens": 2,
                        "rejected_prediction_tokens": 3
                    }
                }
            }),
        ])
    }

    fn sse_body(events: impl IntoIterator<Item = JsonValue>) -> String {
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .chain(["data: [DONE]\n\n".to_string()])
            .collect()
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending in test"),
        }
    }
}
