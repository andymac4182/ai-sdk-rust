use std::collections::BTreeMap;
use std::env;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, FileData, FileDataContent, FinishReason, HandledFetchError, Headers,
    InputTokenUsage, JsonObject, JsonValue, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelRequest, LanguageModelResponse, LanguageModelResponseFormat,
    LanguageModelSource, LanguageModelStreamFinish, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelText,
    LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart, LanguageModelUsage,
    LanguageModelUserContentPart, ModelType, NoSuchModelError, OpenAICompatibleEmbeddingModel,
    OpenAICompatibleImageModel, OpenAICompatibleTransport, OpenAICompatibleTransportFuture,
    OutputTokenUsage, ParseJsonResult, PostJsonToApiOptions, Provider, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, RuntimeEnvironment,
    UnsupportedFunctionalityError, Warning, combine_headers, convert_to_base64,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, generate_id, get_top_level_media_type, post_json_to_api,
    resolve_full_media_type, with_user_agent_suffix, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/perplexity` API calls.
pub const DEFAULT_PERPLEXITY_BASE_URL: &str = "https://api.perplexity.ai";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerplexityProviderSettings {
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl PerplexityProviderSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

#[derive(Clone)]
pub struct PerplexityProvider {
    settings: PerplexityProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

#[derive(Clone)]
pub struct PerplexityLanguageModel {
    model_id: String,
    config: PerplexityModelConfig,
}

#[derive(Clone)]
struct PerplexityModelConfig {
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
}

impl PerplexityProvider {
    pub fn new() -> Self {
        Self::from_settings(PerplexityProviderSettings::new())
    }

    pub fn from_settings(settings: PerplexityProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn language_model(&self, model_id: impl Into<String>) -> PerplexityLanguageModel {
        PerplexityLanguageModel {
            model_id: model_id.into(),
            config: PerplexityModelConfig {
                base_url: perplexity_base_url(&self.settings),
                headers: perplexity_request_headers(&self.settings),
                transport: self
                    .transport
                    .clone()
                    .unwrap_or_else(default_perplexity_transport),
            },
        }
    }

    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for PerplexityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for PerplexityProvider {
    type LanguageModel = PerplexityLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(PerplexityProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        PerplexityProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        PerplexityProvider::image_model(self, model_id)
    }
}

impl PerplexityLanguageModel {
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn provider(&self) -> &str {
        "perplexity"
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let provider_metadata_key = "perplexity".to_string();
        let (request_body, warnings) = match perplexity_request_body(&self.model_id, &options) {
            Ok(result) => result,
            Err(message) => {
                return perplexity_error_generate_result(
                    &provider_metadata_key,
                    message.to_string(),
                    json!({ "model": self.model_id }),
                    None,
                    None,
                );
            }
        };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = format!("{}{}", self.config.base_url, "/chat/completions");
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    perplexity_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    perplexity_error_response,
                    perplexity_error_to_message,
                    |_status_code, _error| None,
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
        let provider_metadata_key = "perplexity".to_string();
        let (request_body, warnings) = match perplexity_request_body(&self.model_id, &options) {
            Ok(result) => result,
            Err(message) => {
                return perplexity_error_stream_result(
                    message.to_string(),
                    json!({ "model": self.model_id }),
                    None,
                    None,
                );
            }
        };
        let request_body = {
            let mut body = request_body;
            if let Some(map) = body.as_object_mut() {
                map.insert("stream".to_string(), JsonValue::Bool(true));
            }
            body
        };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = format!("{}{}", self.config.base_url, "/chat/completions");
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    perplexity_chunk,
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    perplexity_error_response,
                    perplexity_error_to_message,
                    |_status_code, _error| None,
                ))
            },
        )
        .await
        {
            Ok(response) => self.stream_result_from_response(
                &provider_metadata_key,
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
            ),
            Err(error) => self.stream_result_from_error(error, request_body_for_error),
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                self.config
                    .headers
                    .iter()
                    .map(|(name, value)| (name.clone(), Some(value.clone())))
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

    fn generate_result_from_response(
        &self,
        response: PerplexityResponse,
        raw_response: Option<JsonValue>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let choice = response.choices.first();
        let mut content = Vec::new();

        if let Some(text) = choice
            .and_then(|choice| {
                if choice.message.content.is_empty() {
                    None
                } else {
                    Some(choice.message.content.as_str())
                }
            })
            .filter(|text| !text.is_empty())
        {
            content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
        }

        if let Some(citations) = response.citations.as_ref() {
            for citation in citations {
                content.push(LanguageModelContent::Source(LanguageModelSource::url(
                    generate_id(),
                    citation,
                )));
            }
        }

        let finish_reason =
            perplexity_finish_reason(choice.and_then(|choice| choice.finish_reason.as_deref()));
        let usage = perplexity_usage(response.usage.as_ref());
        let provider_metadata =
            perplexity_provider_metadata(response.images.as_ref(), response.usage.as_ref());
        let raw_body = raw_response.unwrap_or_else(|| {
            serde_json::to_value(&response).expect("perplexity response serializes")
        });

        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_provider_metadata(provider_metadata)
            .with_request(LanguageModelRequest::new().with_body(request_body));

        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

        if let Some(id) = response.id {
            response_metadata = response_metadata.with_id(id);
        }

        if let Some(created) = response.created
            && let Ok(timestamp) = OffsetDateTime::from_unix_timestamp(created as i64)
        {
            response_metadata = response_metadata.with_timestamp(timestamp);
        }

        if let Some(model_id) = response.model {
            response_metadata = response_metadata.with_model_id(model_id);
        }

        if let Some(headers) = response_headers {
            for (name, value) in headers {
                response_metadata = response_metadata.with_header(name, value);
            }
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
        let (message, headers, raw_body) = match error {
            HandledFetchError::Original { error } => (error.message().to_string(), None, None),
            HandledFetchError::ApiCall { error } => (
                error.message().to_string(),
                error.response_headers().cloned(),
                error.response_body().map(str::to_string),
            ),
        };

        perplexity_error_generate_result("perplexity", message, request_body, headers, raw_body)
    }

    fn stream_result_from_response(
        &self,
        provider_metadata_key: &str,
        events: Vec<ParseJsonResult<PerplexityChunk>>,
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
        let mut usage = None::<PerplexityUsage>;
        let mut images = None::<Vec<PerplexityImage>>;
        let mut is_first_chunk = true;
        let mut is_active_text = false;

        for event in events {
            match event {
                ParseJsonResult::Success { value, raw_value } => {
                    if include_raw_chunks {
                        stream.push(LanguageModelStreamPart::Raw(
                            ai_sdk_rust::LanguageModelRawStreamPart::new(raw_value.clone()),
                        ));
                    }

                    if is_first_chunk {
                        is_first_chunk = false;
                        stream.push(LanguageModelStreamPart::ResponseMetadata(
                            perplexity_stream_response_metadata(&value),
                        ));

                        if let Some(citations) = value.citations.as_ref() {
                            for citation in citations {
                                stream.push(LanguageModelStreamPart::Source(
                                    LanguageModelSource::url(generate_id(), citation),
                                ));
                            }
                        }
                    }

                    if let Some(error) = value.error.as_ref() {
                        finish_reason = LanguageModelFinishReason {
                            unified: FinishReason::Error,
                            raw: None,
                        };
                        stream.push(LanguageModelStreamPart::Error(
                            LanguageModelErrorStreamPart::new(json!({
                                "message": perplexity_error_message(error),
                            })),
                        ));
                        continue;
                    }

                    if let Some(event_usage) = value.usage.as_ref() {
                        usage = Some(event_usage.clone());
                    }

                    if let Some(event_images) = value.images.as_ref() {
                        images = Some(event_images.clone());
                    }

                    let Some(choice) = value.choices.first() else {
                        continue;
                    };

                    if let Some(raw_finish_reason) = choice.finish_reason.as_deref() {
                        finish_reason = perplexity_finish_reason(Some(raw_finish_reason));
                    }

                    let Some(delta) = choice.delta.as_ref() else {
                        continue;
                    };

                    if let Some(text) = delta.content.as_deref().filter(|text| !text.is_empty()) {
                        if !is_active_text {
                            stream.push(LanguageModelStreamPart::TextStart(
                                LanguageModelTextStart::new("0"),
                            ));
                            is_active_text = true;
                        }

                        stream.push(LanguageModelStreamPart::TextDelta(
                            LanguageModelTextDelta::new("0", text),
                        ));
                    }
                }
                ParseJsonResult::Failure { error, raw_value } => {
                    finish_reason = LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: None,
                    };
                    stream.push(LanguageModelStreamPart::Error(
                        LanguageModelErrorStreamPart::new(json!({
                            "message": error.to_string(),
                            "body": raw_value.as_ref().map(JsonValue::to_string),
                        })),
                    ));
                }
            }
        }

        if is_active_text {
            stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                "0",
            )));
        }

        let mut result = LanguageModelStreamResult::new(stream)
            .with_request(LanguageModelRequest::new().with_body(request_body));

        if let Some(headers) = response_headers {
            result = result.with_response(LanguageModelStreamResultResponse {
                headers: Some(headers),
            });
        }

        let provider_metadata = perplexity_stream_provider_metadata(
            provider_metadata_key,
            images.as_ref(),
            usage.as_ref(),
        );
        let finish =
            LanguageModelStreamFinish::new(perplexity_usage(usage.as_ref()), finish_reason)
                .with_provider_metadata(provider_metadata);

        result.stream.push(LanguageModelStreamPart::Finish(finish));
        result
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (message, headers, raw_body) = match error {
            HandledFetchError::Original { error } => (error.message().to_string(), None, None),
            HandledFetchError::ApiCall { error } => (
                error.message().to_string(),
                error.response_headers().cloned(),
                error.response_body().map(str::to_string),
            ),
        };

        perplexity_error_stream_result(message, request_body, headers, raw_body.as_deref())
    }
}

impl LanguageModel for PerplexityLanguageModel {
    type SupportedUrlsFuture<'a>
        = Pin<Box<dyn Future<Output = ai_sdk_rust::LanguageModelSupportedUrls> + Send + 'a>>
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

    fn provider(&self) -> &str {
        PerplexityLanguageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        PerplexityLanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        Box::pin(async { ai_sdk_rust::LanguageModelSupportedUrls::new() })
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

pub fn create_perplexity(settings: PerplexityProviderSettings) -> PerplexityProvider {
    PerplexityProvider::from_settings(settings)
}

pub fn perplexity(model_id: impl Into<String>) -> PerplexityLanguageModel {
    PerplexityProvider::new().language_model(model_id)
}

fn perplexity_base_url(settings: &PerplexityProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_PERPLEXITY_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn perplexity_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("PERPLEXITY_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn perplexity_request_headers(settings: &PerplexityProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = perplexity_api_key(settings.api_key.as_ref()) {
        headers.insert("authorization".to_string(), format!("Bearer {api_key}"));
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), value.clone());
    }

    with_user_agent_suffix(
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        [format!("ai-sdk/perplexity/{}", ai_sdk_rust::VERSION)],
    )
}

fn perplexity_request_body(
    model_id: &str,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), UnsupportedFunctionalityError> {
    let mut warnings = Vec::new();

    if options.reasoning.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: None,
        });
    }

    if options.top_k.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "topK".to_string(),
            details: None,
        });
    }

    if options.stop_sequences.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "stopSequences".to_string(),
            details: None,
        });
    }

    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: None,
        });
    }

    let messages = convert_to_perplexity_messages(&options.prompt)?;
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    insert_number_field(&mut body, "frequency_penalty", options.frequency_penalty);
    insert_number_field(
        &mut body,
        "max_tokens",
        options.max_output_tokens.map(|value| value as f64),
    );
    insert_number_field(&mut body, "presence_penalty", options.presence_penalty);
    insert_number_field(&mut body, "temperature", options.temperature);
    insert_number_field(&mut body, "top_k", options.top_k.map(|value| value as f64));
    insert_number_field(&mut body, "top_p", options.top_p);
    body.insert("messages".to_string(), JsonValue::Array(messages));

    if let Some(response_format) =
        response_format_to_perplexity_value(options.response_format.as_ref())
    {
        body.insert("response_format".to_string(), response_format);
    }

    if let Some(provider_options) = options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("perplexity"))
    {
        body.extend(provider_options.clone());
    }

    Ok((JsonValue::Object(body), warnings))
}

fn response_format_to_perplexity_value(
    response_format: Option<&LanguageModelResponseFormat>,
) -> Option<JsonValue> {
    let LanguageModelResponseFormat::Json { schema, .. } = response_format? else {
        return None;
    };

    let mut json_schema = JsonObject::new();
    if let Some(schema) = schema {
        json_schema.insert("schema".to_string(), JsonValue::Object(schema.clone()));
    }

    Some(json!({
        "type": "json_schema",
        "json_schema": JsonValue::Object(json_schema)
    }))
}

fn insert_number_field(body: &mut JsonObject, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        if let Some(number) = serde_json::Number::from_f64(value) {
            body.insert(key.to_string(), JsonValue::Number(number));
        }
    }
}

fn convert_to_perplexity_messages(
    prompt: &LanguageModelPrompt,
) -> Result<Vec<JsonValue>, UnsupportedFunctionalityError> {
    let mut messages = Vec::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                messages.push(json!({
                    "role": "system",
                    "content": message.content,
                }));
            }
            LanguageModelMessage::User(message) => {
                messages.push(convert_user_or_assistant_message(
                    "user",
                    message
                        .content
                        .iter()
                        .enumerate()
                        .map(|(index, part)| convert_user_part(index, part)),
                )?);
            }
            LanguageModelMessage::Assistant(message) => {
                messages.push(convert_user_or_assistant_message(
                    "assistant",
                    message
                        .content
                        .iter()
                        .enumerate()
                        .map(|(index, part)| convert_assistant_part(index, part)),
                )?);
            }
            LanguageModelMessage::Tool(_) => {
                return Err(UnsupportedFunctionalityError::new("Tool messages"));
            }
        }
    }

    Ok(messages)
}

#[derive(Debug)]
struct PerplexityConvertedPart {
    value: Option<JsonValue>,
    is_multipart: bool,
}

fn convert_user_or_assistant_message<I>(
    role: &'static str,
    parts: I,
) -> Result<JsonValue, UnsupportedFunctionalityError>
where
    I: IntoIterator<Item = Result<PerplexityConvertedPart, UnsupportedFunctionalityError>>,
{
    let mut message_content = Vec::new();
    let mut text_content = Vec::new();
    let mut has_multipart_content = false;

    for part in parts {
        let part = part?;
        has_multipart_content |= part.is_multipart;

        if let Some(part) = part.value {
            if part.get("type").and_then(JsonValue::as_str) == Some("text") {
                if let Some(text) = part.get("text").and_then(JsonValue::as_str) {
                    text_content.push(text.to_string());
                }
            }

            message_content.push(part);
        }
    }

    Ok(json!({
        "role": role,
        "content": if has_multipart_content {
            JsonValue::Array(message_content)
        } else {
            JsonValue::String(text_content.join(""))
        }
    }))
}

fn convert_user_part(
    index: usize,
    part: &LanguageModelUserContentPart,
) -> Result<PerplexityConvertedPart, UnsupportedFunctionalityError> {
    match part {
        LanguageModelUserContentPart::Text(text) => Ok(PerplexityConvertedPart {
            value: Some(json!({
                "type": "text",
                "text": text.text,
            })),
            is_multipart: false,
        }),
        LanguageModelUserContentPart::File(file) => convert_file_part(index, file),
    }
}

fn convert_assistant_part(
    index: usize,
    part: &LanguageModelAssistantContentPart,
) -> Result<PerplexityConvertedPart, UnsupportedFunctionalityError> {
    match part {
        LanguageModelAssistantContentPart::Text(text) => Ok(PerplexityConvertedPart {
            value: Some(json!({
                "type": "text",
                "text": text.text,
            })),
            is_multipart: false,
        }),
        LanguageModelAssistantContentPart::File(file) => convert_file_part(index, file),
        _ => Ok(PerplexityConvertedPart {
            value: None,
            is_multipart: false,
        }),
    }
}

fn convert_file_part(
    index: usize,
    part: &ai_sdk_rust::LanguageModelFilePart,
) -> Result<PerplexityConvertedPart, UnsupportedFunctionalityError> {
    let top_level_media_type = get_top_level_media_type(&part.media_type);
    let is_pdf = part.media_type == "application/pdf";
    let is_image = top_level_media_type == "image";

    if !is_pdf && !is_image {
        return Ok(PerplexityConvertedPart {
            value: None,
            is_multipart: top_level_media_type == "application",
        });
    }

    match &part.data {
        FileData::Url { url } => {
            if is_pdf {
                let mut file_url = JsonObject::new();
                file_url.insert("url".to_string(), JsonValue::String(url.to_string()));

                let mut file = JsonObject::new();
                file.insert(
                    "type".to_string(),
                    JsonValue::String("file_url".to_string()),
                );
                file.insert("file_url".to_string(), JsonValue::Object(file_url));
                if let Some(filename) = &part.filename {
                    file.insert("file_name".to_string(), JsonValue::String(filename.clone()));
                }

                Ok(PerplexityConvertedPart {
                    value: Some(JsonValue::Object(file)),
                    is_multipart: true,
                })
            } else {
                let mut image_url = JsonObject::new();
                image_url.insert("url".to_string(), JsonValue::String(url.to_string()));

                let mut image = JsonObject::new();
                image.insert(
                    "type".to_string(),
                    JsonValue::String("image_url".to_string()),
                );
                image.insert("image_url".to_string(), JsonValue::Object(image_url));

                Ok(PerplexityConvertedPart {
                    value: Some(JsonValue::Object(image)),
                    is_multipart: true,
                })
            }
        }
        FileData::Data { data } => {
            let base64 = match data {
                FileDataContent::Bytes(bytes) => {
                    convert_to_base64(&FileDataContent::Bytes(bytes.clone()))
                }
                FileDataContent::Base64(base64) => base64.clone(),
            };

            if is_pdf {
                let mut file_url = JsonObject::new();
                file_url.insert("url".to_string(), JsonValue::String(base64));

                let mut file = JsonObject::new();
                file.insert(
                    "type".to_string(),
                    JsonValue::String("file_url".to_string()),
                );
                file.insert("file_url".to_string(), JsonValue::Object(file_url));
                file.insert(
                    "file_name".to_string(),
                    JsonValue::String(
                        part.filename
                            .clone()
                            .unwrap_or_else(|| format!("document-{index}.pdf")),
                    ),
                );

                Ok(PerplexityConvertedPart {
                    value: Some(JsonValue::Object(file)),
                    is_multipart: true,
                })
            } else {
                let media_type = resolve_full_media_type(part)?;
                let mut image_url = JsonObject::new();
                image_url.insert(
                    "url".to_string(),
                    JsonValue::String(format!("data:{};base64,{}", media_type, base64)),
                );

                let mut image = JsonObject::new();
                image.insert(
                    "type".to_string(),
                    JsonValue::String("image_url".to_string()),
                );
                image.insert("image_url".to_string(), JsonValue::Object(image_url));

                Ok(PerplexityConvertedPart {
                    value: Some(JsonValue::Object(image)),
                    is_multipart: true,
                })
            }
        }
        FileData::Reference { .. } => Err(UnsupportedFunctionalityError::new(
            "file parts with provider references",
        )),
        FileData::Text { .. } => Err(UnsupportedFunctionalityError::new("text file parts")),
    }
}

fn perplexity_usage(usage: Option<&PerplexityUsage>) -> LanguageModelUsage {
    let Some(usage) = usage else {
        return LanguageModelUsage::default();
    };

    let prompt_tokens = usage.prompt_tokens;
    let completion_tokens = usage.completion_tokens;
    let reasoning_tokens = usage.reasoning_tokens.unwrap_or_default();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(prompt_tokens),
            no_cache: Some(prompt_tokens),
            cache_read: None,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(completion_tokens),
            text: Some(completion_tokens.saturating_sub(reasoning_tokens)),
            reasoning: Some(reasoning_tokens),
        },
        raw: Some(usage.as_object()),
    }
}

fn perplexity_provider_metadata(
    images: Option<&Vec<PerplexityImage>>,
    usage: Option<&PerplexityUsage>,
) -> ProviderMetadata {
    let mut perplexity = JsonObject::new();

    perplexity.insert(
        "images".to_string(),
        images
            .map(|images| {
                JsonValue::Array(
                    images
                        .iter()
                        .map(|image| {
                            json!({
                                "imageUrl": image.image_url,
                                "originUrl": image.origin_url,
                                "height": image.height,
                                "width": image.width,
                            })
                        })
                        .collect(),
                )
            })
            .unwrap_or(JsonValue::Null),
    );

    perplexity.insert("usage".to_string(), perplexity_usage_metadata(usage));

    let mut metadata = ProviderMetadata::new();
    metadata.insert("perplexity".to_string(), perplexity);
    metadata
}

fn perplexity_stream_provider_metadata(
    provider_metadata_key: &str,
    images: Option<&Vec<PerplexityImage>>,
    usage: Option<&PerplexityUsage>,
) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();

    provider.insert(
        "images".to_string(),
        images
            .map(|images| {
                JsonValue::Array(
                    images
                        .iter()
                        .map(|image| {
                            json!({
                                "imageUrl": image.image_url,
                                "originUrl": image.origin_url,
                                "height": image.height,
                                "width": image.width,
                            })
                        })
                        .collect(),
                )
            })
            .unwrap_or(JsonValue::Null),
    );
    provider.insert("usage".to_string(), perplexity_usage_metadata(usage));

    metadata.insert(provider_metadata_key.to_string(), provider);
    metadata
}

fn perplexity_usage_metadata(usage: Option<&PerplexityUsage>) -> JsonValue {
    json!({
        "citationTokens": usage.and_then(|usage| usage.citation_tokens),
        "numSearchQueries": usage.and_then(|usage| usage.num_search_queries),
    })
}

fn perplexity_finish_reason(finish_reason: Option<&str>) -> LanguageModelFinishReason {
    let raw = finish_reason.map(str::to_string);
    let unified = match finish_reason {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason { unified, raw }
}

fn perplexity_stream_response_metadata(
    response: &PerplexityChunk,
) -> LanguageModelStreamResponseMetadata {
    let mut metadata = LanguageModelStreamResponseMetadata::new();

    if let Some(id) = &response.id {
        metadata = metadata.with_id(id.clone());
    }

    if let Some(created) = response.created
        && let Ok(timestamp) = OffsetDateTime::from_unix_timestamp(created as i64)
    {
        metadata = metadata.with_timestamp(timestamp);
    }

    if let Some(model) = &response.model {
        metadata = metadata.with_model_id(model.clone());
    }

    metadata
}

fn perplexity_error_message(error: &PerplexityError) -> String {
    error
        .message
        .as_deref()
        .or(error.error_type.as_deref())
        .unwrap_or("unknown error")
        .to_string()
}

fn perplexity_error_to_message(error: &PerplexityErrorResponse) -> String {
    perplexity_error_message(&error.error)
}

fn perplexity_error_generate_result(
    provider_name: &str,
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<String>,
) -> LanguageModelGenerateResult {
    let response_body = raw_body
        .as_ref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.clone().map(JsonValue::String))
        .unwrap_or_else(|| request_body.clone());
    let mut response = LanguageModelResponse::new().with_body(response_body);

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("perplexity-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_response(response)
    .with_provider_metadata(perplexity_error_metadata(provider_name, message))
}

fn perplexity_error_stream_result(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut result = LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
        LanguageModelErrorStreamPart::new(json!({
            "message": message,
            "body": raw_body.map(str::to_string),
        })),
    )])
    .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(LanguageModelStreamResultResponse {
            headers: Some(headers),
        });
    }

    result
}

fn perplexity_error_metadata(provider_name: &str, message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    metadata.insert(
        provider_name.to_string(),
        json!({
            "errorMessage": message,
        })
        .as_object()
        .expect("metadata is an object")
        .clone(),
    );
    metadata
}

fn perplexity_response(value: &JsonValue) -> Result<PerplexityResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn perplexity_chunk(value: &JsonValue) -> Result<PerplexityChunk, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn perplexity_error_response(
    value: &JsonValue,
) -> Result<PerplexityErrorResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn default_perplexity_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_perplexity_request(request))))
}

fn execute_perplexity_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_perplexity_get_request(request),
        ProviderApiRequestMethod::Post => execute_perplexity_post_request(request),
    }
}

fn execute_perplexity_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    perplexity_provider_api_response(response)
}

fn execute_perplexity_post_request(
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
                "multipart form data is not supported by the Perplexity transport",
            ));
        }
        None => builder.send_empty(),
    };

    perplexity_provider_api_response(response)
}

fn perplexity_provider_api_response(
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityResponseMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityChoice {
    message: PerplexityResponseMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityImage {
    image_url: String,
    origin_url: String,
    height: u64,
    width: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    citation_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    num_search_queries: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u64>,
}

impl PerplexityUsage {
    fn as_object(&self) -> JsonObject {
        serde_json::to_value(self)
            .expect("perplexity usage serializes")
            .as_object()
            .expect("perplexity usage is an object")
            .clone()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    choices: Vec<PerplexityChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    citations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    images: Option<Vec<PerplexityImage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<PerplexityUsage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityChunkChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delta: Option<PerplexityDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityChunk {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    choices: Vec<PerplexityChunkChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    citations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    images: Option<Vec<PerplexityImage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<PerplexityUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<PerplexityError>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityErrorResponse {
    error: PerplexityError,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PerplexityError {
    code: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    error_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_PERPLEXITY_BASE_URL, PerplexityProvider, PerplexityProviderSettings,
        convert_to_perplexity_messages, create_perplexity, perplexity,
    };
    use ai_sdk_rust::{
        FileData, FileDataContent, Headers, JsonObject, JsonValue, LanguageModel,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFilePart, LanguageModelMessage, LanguageModelResponseFormat,
        LanguageModelStreamPart, LanguageModelStreamStart, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelToolMessage, LanguageModelUserContentPart,
        LanguageModelUserMessage, ModelType, Provider, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderReference, Warning,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

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

    fn sse_body(values: impl IntoIterator<Item = JsonValue>) -> String {
        let mut body = String::new();
        for value in values {
            body.push_str("data: ");
            body.push_str(&value.to_string());
            body.push_str("\n\n");
        }
        body
    }

    fn recording_json_transport(
        response_body: JsonValue,
        response_headers: Option<Headers>,
    ) -> (
        super::OpenAICompatibleTransport,
        Arc<Mutex<Option<ProviderApiRequest>>>,
    ) {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let response_body = response_body.to_string();
        let transport: super::OpenAICompatibleTransport =
            Arc::new(move |request| -> super::OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                let mut response = ProviderApiResponse::text(200, "OK", response_body.clone());
                if let Some(headers) = response_headers.clone() {
                    response = response.with_headers(headers);
                }

                Box::pin(ready(Ok(response)))
            });

        (transport, captured_request)
    }

    fn recording_stream_transport(
        stream_body: String,
        response_headers: Option<Headers>,
    ) -> (
        super::OpenAICompatibleTransport,
        Arc<Mutex<Option<ProviderApiRequest>>>,
    ) {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: super::OpenAICompatibleTransport =
            Arc::new(move |request| -> super::OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                let mut response = ProviderApiResponse::text(200, "OK", stream_body.clone());
                if let Some(headers) = response_headers.clone() {
                    response = response.with_headers(headers);
                }

                Box::pin(ready(Ok(response)))
            });

        (transport, captured_request)
    }

    fn request_body_json(captured_request: &Arc<Mutex<Option<ProviderApiRequest>>>) -> JsonValue {
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        serde_json::from_str(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .expect("request body is text"),
        )
        .expect("request body is JSON")
    }

    #[test]
    fn convert_to_perplexity_messages_converts_system_user_assistant_messages() {
        let prompt = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("System initialization")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello ")),
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("World")),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                ai_sdk_rust::LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                    "Assistant reply",
                )),
            ])),
        ];

        assert_eq!(
            convert_to_perplexity_messages(&prompt).expect("messages convert"),
            vec![
                json!({
                    "role": "system",
                    "content": "System initialization"
                }),
                json!({
                    "role": "user",
                    "content": "Hello World"
                }),
                json!({
                    "role": "assistant",
                    "content": "Assistant reply"
                }),
            ]
        );
    }

    #[test]
    fn convert_to_perplexity_messages_handles_tool_messages_and_provider_references() {
        let tool_message = vec![LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            vec![],
        ))];
        assert!(convert_to_perplexity_messages(&tool_message).is_err());

        let provider_reference = ProviderReference::try_from(BTreeMap::from([(
            "perplexity".to_string(),
            "file-ref-123".to_string(),
        )]))
        .expect("provider reference is valid");

        let file_reference_prompt = vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Reference {
                        reference: provider_reference,
                    },
                    "image/png",
                ),
            )]),
        )];
        assert!(convert_to_perplexity_messages(&file_reference_prompt).is_err());
    }

    #[test]
    fn convert_to_perplexity_messages_handles_top_level_media_type_resolution() {
        let png_base64 = "iVBORw0KGgo=";
        let png_bytes = vec![0x89, 0x50, 0x4e, 0x47, 0xff, 0xff];
        let png_bytes_base64 = super::convert_to_base64(&FileDataContent::Bytes(png_bytes.clone()));

        let full_image = convert_to_perplexity_messages(&vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image/png",
                ),
            )]),
        )])
        .expect("messages convert");
        assert_eq!(
            full_image,
            vec![json!({
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": format!("data:image/png;base64,{png_base64}") }
                    }
                ]
            })]
        );

        let top_level_image = convert_to_perplexity_messages(&vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(png_bytes.clone()),
                    },
                    "image/*",
                ),
            )]),
        )])
        .expect("messages convert");
        assert_eq!(
            top_level_image,
            vec![json!({
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": format!("data:image/png;base64,{png_bytes_base64}") }
                    }
                ]
            })]
        );

        let top_level_image_url =
            convert_to_perplexity_messages(&vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/x.png").expect("url parses"),
                        },
                        "image",
                    ),
                )]),
            )])
            .expect("messages convert");
        assert_eq!(
            top_level_image_url,
            vec![json!({
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": "https://example.com/x.png" }
                    }
                ]
            })]
        );

        let pdf_data = convert_to_perplexity_messages(&vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("JVBERi0xLjQ=".to_string()),
                    },
                    "application/pdf",
                ),
            )]),
        )])
        .expect("messages convert");
        assert_eq!(
            pdf_data,
            vec![json!({
                "role": "user",
                "content": [
                    {
                        "type": "file_url",
                        "file_url": { "url": "JVBERi0xLjQ=" },
                        "file_name": "document-0.pdf"
                    }
                ]
            })]
        );

        let top_level_application =
            convert_to_perplexity_messages(&vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Base64("JVBERi0xLjQ=".to_string()),
                        },
                        "application",
                    )
                    .with_filename("doc.pdf"),
                )]),
            )])
            .expect("messages convert");
        assert_eq!(
            top_level_application,
            vec![json!({
                "role": "user",
                "content": []
            })]
        );
    }

    #[test]
    fn convert_to_perplexity_messages_passes_full_image_and_url_cases_through_unchanged() {
        let png_base64 = "iVBORw0KGgo=";
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image/png",
                ),
            )],
        ))];

        let result = convert_to_perplexity_messages(&prompt).expect("messages convert");

        assert_eq!(
            result[0],
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": format!("data:image/png;base64,{png_base64}") }
                    }
                ]
            })
        );
    }

    #[test]
    fn perplexity_provider_passes_through_perplexity_provider_options() {
        let (transport, captured_request) = recording_json_transport(
            json!({
                "id": "pplx-124",
                "created": 1711115038,
                "model": "sonar",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "{\"ok\":true}"
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 2
                }
            }),
            None,
        );
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let provider_options: ai_sdk_rust::ProviderOptions = serde_json::from_value(json!({
            "perplexity": {
                "search_recency_filter": "month",
                "return_images": true
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return JSON"),
                    )]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.finish_reason.unified,
            ai_sdk_rust::FinishReason::Stop
        );
        assert_eq!(
            request_body_json(&captured_request),
            json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Return JSON"
                    }
                ],
                "return_images": true,
                "search_recency_filter": "month"
            })
        );
    }

    #[test]
    fn perplexity_provider_supports_json_response_format() {
        let (transport, _captured_request) = recording_json_transport(
            json!({
                "id": "pplx-124",
                "created": 1711115038,
                "model": "sonar",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "{\"ok\":true}"
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 2
                }
            }),
            None,
        );
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let response_format = LanguageModelResponseFormat::json().with_schema(
            JsonObject::from_iter([("type".to_string(), JsonValue::String("object".to_string()))]),
        );

        let _result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return JSON"),
                    )]),
                )])
                .with_response_format(response_format),
            ),
        );

        assert_eq!(
            request_body_json(&_captured_request),
            json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Return JSON"
                    }
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "schema": {
                            "type": "object"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn perplexity_provider_warns_about_unsupported_reasoning() {
        let (transport, _captured_request) = recording_json_transport(
            json!({
                "id": "pplx-124",
                "created": 1711115038,
                "model": "sonar",
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "{\"ok\":true}"
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 2
                }
            }),
            None,
        );
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return JSON"),
                    )]),
                )])
                .with_reasoning(ai_sdk_rust::LanguageModelReasoningEffort::High),
            ),
        );

        assert!(result.warnings.iter().any(|warning| matches!(
            warning,
            Warning::Unsupported { feature, .. } if feature == "reasoning"
        )));
    }

    #[test]
    fn perplexity_provider_creates_language_model_with_headers_and_base_url() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: super::OpenAICompatibleTransport =
            Arc::new(move |request| -> super::OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "pplx-123",
                        "created": 1711115037,
                        "model": "sonar",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Perplexity"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "citations": [
                            "https://example.com/source-a"
                        ],
                        "images": [
                            {
                                "image_url": "https://example.com/image.png",
                                "origin_url": "https://example.com/original.png",
                                "height": 100,
                                "width": 200
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 3,
                            "total_tokens": 7,
                            "citation_tokens": 2,
                            "num_search_queries": 1
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_perplexity".to_string(),
                )])))))
            });

        let provider = create_perplexity(
            PerplexityProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.perplexity.test/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("sonar");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Say hello")),
            ])),
        ])));

        assert_eq!(model.provider(), "perplexity");
        assert_eq!(model.model_id(), "sonar");
        assert_eq!(
            result
                .content
                .iter()
                .filter_map(|part| match part {
                    LanguageModelContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Hello from Perplexity"]
        );
        assert_eq!(
            result
                .content
                .iter()
                .filter(|part| matches!(part, LanguageModelContent::Source(_)))
                .count(),
            1
        );
        assert_eq!(
            result.finish_reason.unified,
            ai_sdk_rust::FinishReason::Stop
        );
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(3));
        assert_eq!(result.usage.output_tokens.reasoning, Some(0));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("perplexity"))
                .and_then(|metadata| metadata.get("usage"))
                .and_then(|usage| usage.get("citationTokens"))
                .and_then(JsonValue::as_u64),
            Some(2)
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_perplexity")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.perplexity.test/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/perplexity/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ]
            }))
        );
    }

    #[test]
    fn perplexity_provider_supports_json_response_format_and_provider_options() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: super::OpenAICompatibleTransport =
            Arc::new(move |request| -> super::OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "pplx-124",
                        "created": 1711115038,
                        "model": "sonar",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "{\"ok\":true}"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 2
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let provider_options: ai_sdk_rust::ProviderOptions = serde_json::from_value(json!({
            "perplexity": {
                "search_mode": "web"
            }
        }))
        .expect("provider options deserialize");
        let response_format = LanguageModelResponseFormat::json().with_schema(
            JsonObject::from_iter([("type".to_string(), JsonValue::String("object".to_string()))]),
        );
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return JSON"),
                    )]),
                )])
                .with_provider_options(provider_options)
                .with_response_format(response_format)
                .with_top_k(40)
                .with_stop_sequence("DONE")
                .with_reasoning(ai_sdk_rust::LanguageModelReasoningEffort::High),
            ),
        );

        assert_eq!(
            result
                .warnings
                .iter()
                .filter_map(|warning| match warning {
                    Warning::Unsupported { feature, .. } => Some(feature.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["reasoning", "topK", "stopSequences"]
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Return JSON"
                    }
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "schema": {
                            "type": "object"
                        }
                    }
                },
                "search_mode": "web",
                "top_k": 40.0
            }))
        );
    }

    #[test]
    fn perplexity_provider_handles_pdf_and_image_files_in_request_messages() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: super::OpenAICompatibleTransport =
            Arc::new(move |request| -> super::OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "pplx-125",
                        "created": 1711115039,
                        "model": "sonar",
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": "done"
                                },
                                "finish_reason": "stop"
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Here")),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/report.pdf")
                                .expect("pdf url parses"),
                        },
                        "application/pdf",
                    )
                    .with_filename("report.pdf"),
                ),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![1, 2, 3]),
                    },
                    "image/png",
                )),
            ],
        ))];
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt)
                    .with_max_output_tokens(8)
                    .with_temperature(0.1),
            ),
        );

        assert_eq!(
            result
                .content
                .iter()
                .filter_map(|part| match part {
                    LanguageModelContent::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["done"]
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
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "model": "sonar",
                "max_tokens": 8.0,
                "temperature": 0.1,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Here"
                            },
                            {
                                "type": "file_url",
                                "file_url": {
                                    "url": "https://example.com/report.pdf"
                                },
                                "file_name": "report.pdf"
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": "data:image/png;base64,AQID"
                                }
                            }
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn perplexity_provider_streams_sources_and_usage() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: super::OpenAICompatibleTransport =
            Arc::new(move |request| -> super::OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "pplx-stream",
                            "created": 1711115040,
                            "model": "sonar",
                            "choices": [
                                {
                                    "delta": {
                                        "content": "Hello "
                                    },
                                    "finish_reason": null
                                }
                            ],
                            "citations": [
                                "https://example.com/source-a"
                            ]
                        }),
                        json!({
                            "id": "pplx-stream",
                            "created": 1711115040,
                            "model": "sonar",
                            "choices": [
                                {
                                    "delta": {
                                        "content": "world"
                                    },
                                    "finish_reason": "stop"
                                }
                            ],
                            "images": [
                                {
                                    "image_url": "https://example.com/image.png",
                                    "origin_url": "https://example.com/original.png",
                                    "height": 100,
                                    "width": 200
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 5,
                                "completion_tokens": 2,
                                "citation_tokens": 1,
                                "num_search_queries": 3
                            }
                        }),
                    ]),
                ))))
            });
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Say hi")),
            ])),
        ])));

        assert_eq!(
            result.stream.first(),
            Some(&LanguageModelStreamPart::StreamStart(
                LanguageModelStreamStart::new(Vec::new())
            ))
        );
        let mut text = String::new();
        let mut source_count = 0usize;
        let mut finish = None;
        for part in &result.stream {
            match part {
                LanguageModelStreamPart::TextDelta(delta) => text.push_str(&delta.delta),
                LanguageModelStreamPart::Source(_) => source_count += 1,
                LanguageModelStreamPart::Finish(value) => finish = Some(value),
                _ => {}
            }
        }
        assert_eq!(text, "Hello world");
        assert_eq!(source_count, 1);
        let finish = finish.expect("finish part is present");
        assert_eq!(
            finish.finish_reason.unified,
            ai_sdk_rust::FinishReason::Stop
        );
        assert_eq!(finish.usage.input_tokens.total, Some(5));
        assert_eq!(finish.usage.output_tokens.total, Some(2));
        assert_eq!(
            finish
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("perplexity"))
                .and_then(|metadata| metadata.get("usage"))
                .and_then(|usage| usage.get("numSearchQueries"))
                .and_then(JsonValue::as_u64),
            Some(3)
        );
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hi"
                    }
                ],
                "stream": true
            }))
        );
    }

    #[test]
    fn perplexity_provider_streams_raw_chunks() {
        let (transport, captured_request) = recording_stream_transport(
            sse_body([
                json!({
                    "id":"ppl-123",
                    "object":"chat.completion.chunk",
                    "created":1234567890,
                    "model":"sonar",
                    "choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}],
                    "citations":["https://example.com"]
                }),
                json!({
                    "id":"ppl-456",
                    "object":"chat.completion.chunk",
                    "created":1234567890,
                    "model":"sonar",
                    "choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]
                }),
                json!({
                    "id":"ppl-789",
                    "object":"chat.completion.chunk",
                    "created":1234567890,
                    "model":"sonar",
                    "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"citation_tokens":2,"num_search_queries":1}
                }),
            ]),
            None,
        );
        let provider =
            create_perplexity(PerplexityProviderSettings::new()).with_transport(transport);
        let model = provider.language_model("sonar");
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Say hi"),
                    )]),
                )])
                .with_include_raw_chunks(true),
            ),
        );

        let raw_chunks = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::Raw(raw) => Some(&raw.raw_value),
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut text = String::new();
        let mut source_count = 0usize;
        for part in &result.stream {
            match part {
                LanguageModelStreamPart::TextDelta(delta) => text.push_str(&delta.delta),
                LanguageModelStreamPart::Source(_) => source_count += 1,
                _ => {}
            }
        }

        assert_eq!(raw_chunks.len(), 3);
        assert_eq!(text, "Hello world");
        assert_eq!(source_count, 1);
        assert_eq!(
            request_body_json(&captured_request),
            json!({
                "model": "sonar",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hi"
                    }
                ],
                "stream": true
            })
        );
    }

    #[test]
    fn perplexity_provider_reports_unsupported_model_families() {
        let provider = PerplexityProvider::new();

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);

        let text_embedding_error = provider
            .text_embedding_model("embed")
            .err()
            .expect("text embedding alias is unsupported");
        assert_eq!(text_embedding_error.model_type(), ModelType::EmbeddingModel);

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn perplexity_provider_implements_provider_trait() {
        let provider = PerplexityProvider::new();
        let model = Provider::language_model(&provider, "sonar").expect("language model resolves");

        assert_eq!(model.provider(), "perplexity");
        assert_eq!(model.model_id(), "sonar");
    }

    #[test]
    fn perplexity_provider_uses_default_base_url_and_function_alias() {
        let model = perplexity("sonar");

        assert_eq!(model.provider(), "perplexity");
        assert_eq!(model.model_id(), "sonar");
        assert_eq!(
            super::perplexity_base_url(&PerplexityProviderSettings::new()),
            DEFAULT_PERPLEXITY_BASE_URL
        );
    }

    #[test]
    fn perplexity_provider_settings_serde_accepts_upstream_base_url() {
        let settings: PerplexityProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.perplexity.test/",
            "apiKey": "key",
            "headers": {
                "x-provider": "perplexity"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            PerplexityProviderSettings::new()
                .with_base_url("https://api.perplexity.test/")
                .with_api_key("key")
                .with_header("x-provider", "perplexity")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.perplexity.test/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "perplexity"
                }
            })
        );
    }
}
