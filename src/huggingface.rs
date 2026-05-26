use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::file_data::{FileData, FileDataContent};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue, NonNullJsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelRawStreamPart, LanguageModelReasoning, LanguageModelReasoningDelta,
    LanguageModelReasoningEnd, LanguageModelReasoningStart, LanguageModelRequest,
    LanguageModelResponse, LanguageModelSource, LanguageModelStreamFinish, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelToolCall, LanguageModelToolInputDelta, LanguageModelToolInputEnd,
    LanguageModelToolInputStart, LanguageModelToolResult, LanguageModelUrlSource,
    LanguageModelUsage, LanguageModelUserContentPart, OutputTokenUsage,
};
use crate::openai_compatible::{OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions, SpecificationVersion,
};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, ParseJsonResult, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers, convert_to_base64,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, get_top_level_media_type, post_json_to_api,
    resolve_full_media_type, with_user_agent_suffix, without_trailing_slash,
};
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/huggingface` Responses API calls.
pub const DEFAULT_HUGGINGFACE_BASE_URL: &str = "https://router.huggingface.co/v1";

const HUGGINGFACE_PROVIDER_OPTIONS_NAME: &str = "huggingface";
const HUGGINGFACE_PROVIDER_ID: &str = "huggingface.responses";
const HUGGINGFACE_UNSUPPORTED_EMBEDDING_MESSAGE: &str = "Hugging Face Responses API does not support text embeddings. Use the Hugging Face Inference API directly for embeddings.";
const HUGGINGFACE_UNSUPPORTED_IMAGE_MESSAGE: &str = "Hugging Face Responses API does not support image generation. Use the Hugging Face Inference API directly for image models.";

/// Future returned by an injected Hugging Face HTTP transport.
pub type HuggingFaceTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Hugging Face Responses models.
pub type HuggingFaceTransport =
    Arc<dyn Fn(ProviderApiRequest) -> HuggingFaceTransportFuture + Send + Sync>;

/// Settings for the upstream Hugging Face provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceProviderSettings {
    /// Base URL for Hugging Face API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Hugging Face API key. When omitted, `HUGGINGFACE_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl HuggingFaceProviderSettings {
    /// Creates empty Hugging Face provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Hugging Face API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Hugging Face API key.
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

/// Upstream Hugging Face provider foundation.
#[derive(Clone)]
pub struct HuggingFaceProvider {
    settings: HuggingFaceProviderSettings,
    transport: Option<HuggingFaceTransport>,
}

impl HuggingFaceProvider {
    /// Creates a Hugging Face provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(HuggingFaceProviderSettings::new())
    }

    /// Creates a provider from explicit Hugging Face settings.
    pub fn from_settings(settings: HuggingFaceProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Hugging Face API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Hugging Face API base URL for this provider.
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
    pub fn with_transport(mut self, transport: HuggingFaceTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates a Hugging Face Responses API language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> HuggingFaceResponsesLanguageModel {
        self.responses(model_id)
    }

    /// Creates a Hugging Face Responses API language model.
    pub fn responses(&self, model_id: impl Into<String>) -> HuggingFaceResponsesLanguageModel {
        HuggingFaceResponsesLanguageModel::new(
            model_id,
            HuggingFaceModelConfig {
                base_url: huggingface_base_url(&self.settings),
                settings: self.settings.clone(),
                transport: self
                    .transport
                    .clone()
                    .unwrap_or_else(default_huggingface_transport),
            },
        )
    }

    /// Reports that Hugging Face Responses does not expose embedding models.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::EmbeddingModel,
            HUGGINGFACE_UNSUPPORTED_EMBEDDING_MESSAGE,
        ))
    }

    /// Deprecated upstream alias for [`HuggingFaceProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Hugging Face Responses does not expose image models.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::ImageModel,
            HUGGINGFACE_UNSUPPORTED_IMAGE_MESSAGE,
        ))
    }
}

impl Default for HuggingFaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for HuggingFaceProvider {
    type LanguageModel = HuggingFaceResponsesLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(HuggingFaceProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        HuggingFaceProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        HuggingFaceProvider::image_model(self, model_id)
    }
}

/// Creates a Hugging Face provider with explicit settings.
pub fn create_huggingface(settings: HuggingFaceProviderSettings) -> HuggingFaceProvider {
    HuggingFaceProvider::from_settings(settings)
}

/// Creates a Hugging Face Responses API language model using default provider settings.
pub fn huggingface(model_id: impl Into<String>) -> HuggingFaceResponsesLanguageModel {
    HuggingFaceProvider::new().language_model(model_id)
}

#[derive(Clone)]
struct HuggingFaceModelConfig {
    base_url: String,
    settings: HuggingFaceProviderSettings,
    transport: HuggingFaceTransport,
}

/// Hugging Face Responses API language model.
#[derive(Clone)]
pub struct HuggingFaceResponsesLanguageModel {
    model_id: String,
    config: HuggingFaceModelConfig,
}

impl HuggingFaceResponsesLanguageModel {
    fn new(model_id: impl Into<String>, config: HuggingFaceModelConfig) -> Self {
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
        HUGGINGFACE_PROVIDER_ID
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) =
            match huggingface_responses_request_body(&self.model_id, &options, false) {
                Ok(result) => result,
                Err(message) => {
                    return huggingface_error_generate_result(
                        message,
                        json!({ "model": self.model_id, "stream": false }),
                    );
                }
            };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/responses", self.config.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, Infallible>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    |value| Ok::<JsonValue, Infallible>(value.clone()),
                    huggingface_error_message,
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
            Err(error) => self.generate_result_from_error(error, request_body_for_error, warnings),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (mut request_body, warnings) =
            match huggingface_responses_request_body(&self.model_id, &options, true) {
                Ok(result) => result,
                Err(message) => {
                    return huggingface_error_stream_result(
                        message,
                        json!({
                            "model": self.model_id,
                            "stream": true
                        }),
                    );
                }
            };
        if let JsonValue::Object(body) = &mut request_body {
            body.insert("stream".to_string(), JsonValue::Bool(true));
        }

        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/responses", self.config.base_url), request_body)
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
                    |value| Ok::<JsonValue, Infallible>(value.clone()),
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    |value| Ok::<JsonValue, Infallible>(value.clone()),
                    huggingface_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => huggingface_stream_result_from_response(
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
                huggingface_provider_headers(&self.config.settings)
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

    fn generate_result_from_response(
        &self,
        response: JsonValue,
        raw_response: Option<JsonValue>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        if let Some(message) = response
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(JsonValue::as_str)
        {
            let mut result = huggingface_error_generate_result(message.to_string(), request_body);
            for warning in warnings {
                result = result.with_warning(warning);
            }
            return result;
        }

        let content = huggingface_responses_content(&response);
        let usage = huggingface_responses_usage(response.get("usage"));
        let finish_reason = map_huggingface_responses_finish_reason(
            response
                .get("incomplete_details")
                .and_then(|details| details.get("reason"))
                .and_then(JsonValue::as_str),
        );
        let raw_body = raw_response.unwrap_or_else(|| response.clone());
        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_request(LanguageModelRequest::new().with_body(request_body));
        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

        if let Some(id) = response.get("id").and_then(JsonValue::as_str) {
            response_metadata = response_metadata.with_id(id);
            result = result.with_provider_metadata(huggingface_response_metadata(id));
        }

        if let Some(timestamp) = response
            .get("created_at")
            .and_then(JsonValue::as_i64)
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
        {
            response_metadata = response_metadata.with_timestamp(timestamp);
        }

        if let Some(model_id) = response.get("model").and_then(JsonValue::as_str) {
            response_metadata = response_metadata.with_model_id(model_id);
        }

        if let Some(headers) = response_headers {
            response_metadata = response_metadata_with_headers(response_metadata, headers);
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
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let message = match error {
            HandledFetchError::Original { error } => error.message().to_string(),
            HandledFetchError::ApiCall { error } => error.message().to_string(),
        };

        let mut result = huggingface_error_generate_result(message, request_body);
        for warning in warnings {
            result = result.with_warning(warning);
        }

        result
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let message = match error {
            HandledFetchError::Original { error } => error.message().to_string(),
            HandledFetchError::ApiCall { error } => error.message().to_string(),
        };

        huggingface_error_stream_result(message, request_body)
    }
}

impl LanguageModel for HuggingFaceResponsesLanguageModel {
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
        HUGGINGFACE_PROVIDER_ID
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::from([(
            "image/*".to_string(),
            vec!["^https?://.*$".to_string()],
        )]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

fn huggingface_responses_request_body(
    model_id: &str,
    options: &LanguageModelCallOptions,
    stream: bool,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut warnings = Vec::new();
    let input = huggingface_responses_input(&options.prompt, &mut warnings)?;
    let provider_options = huggingface_provider_options(options.provider_options.as_ref());
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("input".to_string(), JsonValue::Array(input));

    if let Some(temperature) = options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }

    if let Some(top_p) = options.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }

    if let Some(max_output_tokens) = options.max_output_tokens {
        body.insert("max_output_tokens".to_string(), json!(max_output_tokens));
    }

    if let Some(response_format) = &options.response_format
        && let Some(value) = huggingface_response_format(response_format, provider_options)
    {
        body.insert("text".to_string(), value);
    }

    if let Some(metadata) = provider_options.and_then(|options| options.get("metadata")) {
        body.insert("metadata".to_string(), metadata.clone());
    }

    if let Some(instructions) = provider_options
        .and_then(|options| options.get("instructions"))
        .and_then(JsonValue::as_str)
    {
        body.insert(
            "instructions".to_string(),
            JsonValue::String(instructions.to_string()),
        );
    }

    if let Some(reasoning_effort) = provider_options
        .and_then(|options| {
            options
                .get("reasoningEffort")
                .or_else(|| options.get("reasoning_effort"))
        })
        .and_then(JsonValue::as_str)
    {
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": reasoning_effort }),
        );
    }

    if options.top_k.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "topK".to_string(),
            details: None,
        });
    }

    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: None,
        });
    }

    if options.presence_penalty.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "presencePenalty".to_string(),
            details: None,
        });
    }

    if options.frequency_penalty.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "frequencyPenalty".to_string(),
            details: None,
        });
    }

    if options.stop_sequences.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "stopSequences".to_string(),
            details: None,
        });
    }

    body.insert("stream".to_string(), JsonValue::Bool(stream));

    Ok((JsonValue::Object(body), warnings))
}

fn huggingface_responses_input(
    prompt: &[LanguageModelMessage],
    warnings: &mut Vec<Warning>,
) -> Result<Vec<JsonValue>, String> {
    let mut input = Vec::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                input.push(json!({
                    "role": "system",
                    "content": message.content
                }));
            }
            LanguageModelMessage::User(message) => {
                let mut content = Vec::new();

                for part in &message.content {
                    match part {
                        LanguageModelUserContentPart::Text(text) => {
                            content.push(json!({
                                "type": "input_text",
                                "text": text.text
                            }));
                        }
                        LanguageModelUserContentPart::File(file) => {
                            content.push(huggingface_file_part(file)?);
                        }
                    }
                }

                input.push(json!({
                    "role": "user",
                    "content": content
                }));
            }
            LanguageModelMessage::Assistant(message) => {
                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text) => {
                            input.push(json!({
                                "role": "assistant",
                                "content": [{
                                    "type": "output_text",
                                    "text": text.text
                                }]
                            }));
                        }
                        LanguageModelAssistantContentPart::Reasoning(reasoning) => {
                            input.push(json!({
                                "role": "assistant",
                                "content": [{
                                    "type": "output_text",
                                    "text": reasoning.text
                                }]
                            }));
                        }
                        LanguageModelAssistantContentPart::ToolCall(_)
                        | LanguageModelAssistantContentPart::ToolResult(_) => {}
                        _ => {
                            return Err(
                                "Hugging Face Responses assistant prompt part is not implemented yet."
                                    .to_string(),
                            );
                        }
                    }
                }
            }
            LanguageModelMessage::Tool(_) => {
                warnings.push(Warning::Unsupported {
                    feature: "tool messages".to_string(),
                    details: None,
                });
            }
        }
    }

    Ok(input)
}

fn huggingface_file_part(
    file: &crate::language_model::LanguageModelFilePart,
) -> Result<JsonValue, String> {
    if get_top_level_media_type(&file.media_type) != "image" {
        return Err(format!(
            "Hugging Face Responses file part media type {} is not implemented yet.",
            file.media_type
        ));
    }

    match &file.data {
        FileData::Url { url } => Ok(json!({
            "type": "input_image",
            "image_url": url.as_str()
        })),
        FileData::Data { data } => {
            let media_type = match data {
                FileDataContent::Bytes(_) => {
                    resolve_full_media_type(file).map_err(|error| error.message().to_string())?
                }
                FileDataContent::Base64(_) => {
                    if crate::provider_utils::is_full_media_type(&file.media_type) {
                        file.media_type.clone()
                    } else {
                        resolve_full_media_type(file)
                            .map_err(|error| error.message().to_string())?
                    }
                }
            };
            Ok(json!({
                "type": "input_image",
                "image_url": format!("data:{media_type};base64,{}", convert_to_base64(data))
            }))
        }
        FileData::Reference { .. } => Err(
            "Hugging Face Responses file parts with provider references are not implemented yet."
                .to_string(),
        ),
        FileData::Text { .. } => {
            Err("Hugging Face Responses text file parts are not implemented yet.".to_string())
        }
    }
}

fn huggingface_response_format(
    response_format: &crate::language_model::LanguageModelResponseFormat,
    provider_options: Option<&JsonObject>,
) -> Option<JsonValue> {
    match response_format {
        crate::language_model::LanguageModelResponseFormat::Json {
            schema: Some(schema),
            name,
            description,
        } => {
            let strict = provider_options
                .and_then(|options| options.get("strictJsonSchema"))
                .or_else(|| provider_options.and_then(|options| options.get("strict_json_schema")))
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            let mut format = JsonObject::new();
            format.insert(
                "type".to_string(),
                JsonValue::String("json_schema".to_string()),
            );
            format.insert("strict".to_string(), JsonValue::Bool(strict));
            format.insert(
                "name".to_string(),
                JsonValue::String(name.clone().unwrap_or_else(|| "response".to_string())),
            );
            if let Some(description) = description {
                format.insert(
                    "description".to_string(),
                    JsonValue::String(description.clone()),
                );
            }
            format.insert("schema".to_string(), JsonValue::Object(schema.clone()));

            Some(json!({ "format": JsonValue::Object(format) }))
        }
        _ => None,
    }
}

fn huggingface_provider_options(provider_options: Option<&ProviderOptions>) -> Option<&JsonObject> {
    provider_options.and_then(|options| options.get(HUGGINGFACE_PROVIDER_OPTIONS_NAME))
}

fn huggingface_responses_content(response: &JsonValue) -> Vec<LanguageModelContent> {
    let mut content = Vec::new();
    let mut source_index = 0usize;

    for part in response
        .get("output")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        match part.get("type").and_then(JsonValue::as_str) {
            Some("message") => {
                let item_id = part.get("id").and_then(JsonValue::as_str);
                for content_part in part
                    .get("content")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
                {
                    if matches!(
                        content_part.get("type").and_then(JsonValue::as_str),
                        Some("output_text")
                    ) && let Some(text) = content_part.get("text").and_then(JsonValue::as_str)
                    {
                        let mut text_part = LanguageModelText::new(text);
                        if let Some(item_id) = item_id {
                            text_part = text_part
                                .with_provider_metadata(huggingface_item_metadata(item_id));
                        }
                        content.push(LanguageModelContent::Text(text_part));

                        for annotation in content_part
                            .get("annotations")
                            .and_then(JsonValue::as_array)
                            .into_iter()
                            .flatten()
                        {
                            if let Some(url) = annotation.get("url").and_then(JsonValue::as_str) {
                                let source_id = format!("source-{source_index}");
                                source_index += 1;
                                let mut source = LanguageModelUrlSource::new(source_id, url);
                                if let Some(title) =
                                    annotation.get("title").and_then(JsonValue::as_str)
                                {
                                    source = source.with_title(title);
                                }
                                content.push(LanguageModelContent::Source(
                                    LanguageModelSource::Url(source),
                                ));
                            }
                        }
                    }
                }
            }
            Some("reasoning") => {
                let item_id = part.get("id").and_then(JsonValue::as_str);
                for content_part in part
                    .get("content")
                    .or_else(|| part.get("summary"))
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = content_part.get("text").and_then(JsonValue::as_str) {
                        let mut reasoning = LanguageModelReasoning::new(text);
                        if let Some(item_id) = item_id {
                            reasoning = reasoning
                                .with_provider_metadata(huggingface_item_metadata(item_id));
                        }
                        content.push(LanguageModelContent::Reasoning(reasoning));
                    }
                }
            }
            Some("function_call") => {
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .or_else(|| part.get("id").and_then(JsonValue::as_str))
                    .unwrap_or_default();
                let tool_name = part
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let input = part
                    .get("arguments")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("{}");
                content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name,
                    input,
                )));

                if let Some(output) = part.get("output") {
                    push_tool_result(&mut content, tool_call_id, tool_name, output.clone(), false);
                }
            }
            Some("mcp_call") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = part
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let input = part
                    .get("arguments")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("{}");
                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(tool_call_id, tool_name, input)
                        .with_provider_executed(true),
                ));

                if let Some(output) = part.get("output") {
                    push_tool_result(&mut content, tool_call_id, tool_name, output.clone(), true);
                }
            }
            Some("mcp_list_tools") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let server_label = part
                    .get("server_label")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(
                        tool_call_id,
                        "list_tools",
                        json!({ "server_label": server_label }).to_string(),
                    )
                    .with_provider_executed(true),
                ));

                if let Some(tools) = part.get("tools") {
                    push_tool_result(
                        &mut content,
                        tool_call_id,
                        "list_tools",
                        json!({ "tools": tools }),
                        true,
                    );
                }
            }
            _ => {}
        }
    }

    content
}

fn push_tool_result(
    content: &mut Vec<LanguageModelContent>,
    tool_call_id: &str,
    tool_name: &str,
    value: JsonValue,
    provider_executed: bool,
) {
    if let Ok(result) = NonNullJsonValue::new(value) {
        let mut tool_result = LanguageModelToolResult::new(tool_call_id, tool_name, result);
        if provider_executed {
            tool_result = tool_result.with_provider_metadata(ProviderMetadata::new());
        }
        content.push(LanguageModelContent::ToolResult(tool_result));
    }
}

fn map_huggingface_responses_finish_reason(
    finish_reason: Option<&str>,
) -> LanguageModelFinishReason {
    let unified = match finish_reason.unwrap_or("stop") {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        "tool_calls" => FinishReason::ToolCalls,
        "error" => FinishReason::Error,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason {
        unified,
        raw: finish_reason.map(ToString::to_string),
    }
}

fn huggingface_responses_usage(usage: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(usage) = usage.filter(|usage| !usage.is_null()) else {
        return LanguageModelUsage::default();
    };

    let input_tokens = usage.get("input_tokens").and_then(JsonValue::as_u64);
    let cached_input_tokens = usage
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(JsonValue::as_u64);
    let output_tokens = usage.get("output_tokens").and_then(JsonValue::as_u64);
    let reasoning_tokens = usage
        .get("output_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(JsonValue::as_u64);

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_tokens,
            no_cache: input_tokens
                .map(|tokens| tokens.saturating_sub(cached_input_tokens.unwrap_or(0))),
            cache_read: Some(cached_input_tokens.unwrap_or(0)),
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: output_tokens,
            text: output_tokens.map(|tokens| tokens.saturating_sub(reasoning_tokens.unwrap_or(0))),
            reasoning: Some(reasoning_tokens.unwrap_or(0)),
        },
        raw: usage.as_object().cloned(),
    }
}

fn huggingface_provider_headers(settings: &HuggingFaceProviderSettings) -> Headers {
    let mut headers = Vec::new();

    if let Some(api_key) = huggingface_api_key(settings.api_key.as_ref()) {
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
        [format!("ai-sdk/huggingface/{}", crate::VERSION)],
    )
}

fn huggingface_error_message(error: &JsonValue) -> String {
    error
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| error.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Hugging Face API error")
        .to_string()
}

fn huggingface_error_generate_result(
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    let message = message.into();
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("huggingface-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(huggingface_error_metadata(message))
}

fn huggingface_error_stream_result(
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut error = JsonObject::new();
    error.insert("message".to_string(), JsonValue::String(message.into()));
    LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
        LanguageModelErrorStreamPart::new(JsonValue::Object(error)),
    )])
    .with_request(LanguageModelRequest::new().with_body(request_body))
}

fn huggingface_stream_result_from_response(
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
    let mut usage = LanguageModelUsage::default();
    let mut response_id = None::<String>;
    let mut saw_error_event = false;
    let mut saw_tool_calls = false;

    let mut text_buffers = BTreeMap::<String, String>::new();
    let mut active_text = BTreeSet::<String>::new();
    let mut ended_text = BTreeSet::<String>::new();

    let mut reasoning_buffers = BTreeMap::<String, String>::new();
    let mut active_reasoning = BTreeSet::<String>::new();
    let mut ended_reasoning = BTreeSet::<String>::new();

    let mut pending_tool_calls = BTreeMap::<String, HuggingFacePendingToolCall>::new();
    let mut active_tool_inputs = BTreeSet::<String>::new();
    let mut ended_tool_inputs = BTreeSet::<String>::new();
    let mut emitted_tool_calls = BTreeSet::<String>::new();
    let mut emitted_tool_results = BTreeSet::<String>::new();

    for event in events {
        match event {
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        LanguageModelRawStreamPart::new(raw_value.clone()),
                    ));
                }

                let event_type = value.get("type").and_then(JsonValue::as_str);
                let has_error = value.get("error").is_some_and(|error| !error.is_null())
                    || matches!(event_type, Some("error"));
                if has_error {
                    finish_reason = LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: Some(event_type.unwrap_or("huggingface-error").to_string()),
                    };
                    saw_error_event = matches!(event_type, Some("error"));
                    stream.push(huggingface_stream_event_error(
                        &value,
                        Some(&raw_value.to_string()),
                    ));
                    continue;
                }

                match event_type {
                    Some("response.created") => {
                        if let Some(response) = value.get("response") {
                            huggingface_emit_response_metadata(&mut stream, response);
                            if response_id.is_none() {
                                response_id = response
                                    .get("id")
                                    .and_then(JsonValue::as_str)
                                    .map(ToString::to_string);
                            }
                        }
                    }
                    Some("response.output_item.added") => {
                        if let Some(item) = value.get("item") {
                            match item.get("type").and_then(JsonValue::as_str) {
                                Some("message") => {
                                    let id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .map(ToString::to_string)
                                        .unwrap_or_else(|| {
                                            huggingface_stream_block_id("txt", &value)
                                        });
                                    huggingface_start_text_block(
                                        &mut stream,
                                        &mut active_text,
                                        &ended_text,
                                        &id,
                                        Some(huggingface_item_metadata(&id)),
                                    );
                                }
                                Some("reasoning") => {
                                    let id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .map(ToString::to_string)
                                        .unwrap_or_else(|| {
                                            huggingface_stream_block_id("reasoning", &value)
                                        });
                                    huggingface_start_reasoning_block(
                                        &mut stream,
                                        &mut active_reasoning,
                                        &ended_reasoning,
                                        &id,
                                        Some(huggingface_item_metadata(&id)),
                                    );
                                }
                                Some("function_call") => {
                                    let item_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .or_else(|| item.get("id").and_then(JsonValue::as_str))
                                        .unwrap_or_default();
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let arguments = item
                                        .get("arguments")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    pending_tool_calls.insert(
                                        item_id.to_string(),
                                        HuggingFacePendingToolCall::new(
                                            tool_call_id,
                                            tool_name,
                                            arguments,
                                        ),
                                    );
                                    huggingface_start_tool_input(
                                        &mut stream,
                                        &mut active_tool_inputs,
                                        &ended_tool_inputs,
                                        tool_call_id,
                                        tool_name,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    Some("response.output_text.delta") => {
                        let id = huggingface_stream_text_id(&value);
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                            && !delta.is_empty()
                        {
                            huggingface_push_text_delta(
                                &mut stream,
                                &mut text_buffers,
                                &mut active_text,
                                &ended_text,
                                &id,
                                delta,
                                Some(huggingface_item_metadata(&id)),
                            );
                        }
                    }
                    Some("response.output_text.done") => {
                        let id = huggingface_stream_text_id(&value);
                        let text = value.get("text").and_then(JsonValue::as_str);
                        huggingface_finish_text_block(
                            &mut stream,
                            &mut text_buffers,
                            &mut active_text,
                            &mut ended_text,
                            &id,
                            text,
                            Some(huggingface_item_metadata(&id)),
                        );
                    }
                    Some("response.reasoning_text.delta") => {
                        let id = huggingface_stream_reasoning_id(&value);
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                            && !delta.is_empty()
                        {
                            huggingface_push_reasoning_delta(
                                &mut stream,
                                &mut reasoning_buffers,
                                &mut active_reasoning,
                                &ended_reasoning,
                                &id,
                                delta,
                                Some(huggingface_item_metadata(&id)),
                            );
                        }
                    }
                    Some("response.reasoning_text.done") => {
                        let id = huggingface_stream_reasoning_id(&value);
                        let text = value.get("text").and_then(JsonValue::as_str);
                        huggingface_finish_reasoning_block(
                            &mut stream,
                            &mut reasoning_buffers,
                            &mut active_reasoning,
                            &mut ended_reasoning,
                            &id,
                            text,
                            Some(huggingface_item_metadata(&id)),
                        );
                    }
                    Some("response.function_call_arguments.delta") => {
                        if let Some(item_id) = value.get("item_id").and_then(JsonValue::as_str) {
                            let delta = value.get("delta").and_then(JsonValue::as_str);
                            if let Some(delta) = delta.filter(|delta| !delta.is_empty()) {
                                huggingface_append_tool_call_arguments(
                                    &mut pending_tool_calls,
                                    item_id,
                                    delta,
                                );
                                if let Some(tool_call) = pending_tool_calls.get(item_id) {
                                    huggingface_start_tool_input(
                                        &mut stream,
                                        &mut active_tool_inputs,
                                        &ended_tool_inputs,
                                        &tool_call.tool_call_id,
                                        &tool_call.tool_name,
                                    );
                                    stream.push(LanguageModelStreamPart::ToolInputDelta(
                                        LanguageModelToolInputDelta::new(
                                            &tool_call.tool_call_id,
                                            delta,
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    Some("response.function_call_arguments.done") => {
                        if let Some(item_id) = value.get("item_id").and_then(JsonValue::as_str) {
                            let arguments = value.get("arguments").and_then(JsonValue::as_str);
                            huggingface_finalize_tool_call(
                                &mut stream,
                                &mut active_tool_inputs,
                                &mut ended_tool_inputs,
                                &mut pending_tool_calls,
                                &mut emitted_tool_calls,
                                &mut emitted_tool_results,
                                item_id,
                                arguments,
                                None,
                            );
                            saw_tool_calls = true;
                        }
                    }
                    Some("response.output_item.done") => {
                        if let Some(item) = value.get("item") {
                            match item.get("type").and_then(JsonValue::as_str) {
                                Some("message") => {
                                    let id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let text = item
                                        .get("content")
                                        .and_then(JsonValue::as_array)
                                        .and_then(|content| {
                                            content.iter().find_map(|part| {
                                                (part.get("type").and_then(JsonValue::as_str)
                                                    == Some("output_text"))
                                                .then(|| {
                                                    part.get("text").and_then(JsonValue::as_str)
                                                })
                                                .flatten()
                                            })
                                        });
                                    huggingface_finish_text_block(
                                        &mut stream,
                                        &mut text_buffers,
                                        &mut active_text,
                                        &mut ended_text,
                                        id,
                                        text,
                                        Some(huggingface_item_metadata(id)),
                                    );
                                }
                                Some("reasoning") => {
                                    let id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let text = item
                                        .get("content")
                                        .or_else(|| item.get("summary"))
                                        .and_then(JsonValue::as_array)
                                        .and_then(|content| {
                                            content.iter().find_map(|part| {
                                                part.get("text").and_then(JsonValue::as_str)
                                            })
                                        });
                                    huggingface_finish_reasoning_block(
                                        &mut stream,
                                        &mut reasoning_buffers,
                                        &mut active_reasoning,
                                        &mut ended_reasoning,
                                        id,
                                        text,
                                        Some(huggingface_item_metadata(id)),
                                    );
                                }
                                Some("function_call") => {
                                    let item_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let arguments =
                                        item.get("arguments").and_then(JsonValue::as_str);
                                    let output = item.get("output");
                                    huggingface_finalize_tool_call(
                                        &mut stream,
                                        &mut active_tool_inputs,
                                        &mut ended_tool_inputs,
                                        &mut pending_tool_calls,
                                        &mut emitted_tool_calls,
                                        &mut emitted_tool_results,
                                        item_id,
                                        arguments,
                                        output,
                                    );
                                    saw_tool_calls = true;
                                }
                                Some("mcp_call") => {
                                    let item_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let arguments =
                                        item.get("arguments").and_then(JsonValue::as_str);
                                    pending_tool_calls.insert(
                                        item_id.to_string(),
                                        HuggingFacePendingToolCall::new(
                                            tool_call_id,
                                            format!("mcp.{tool_name}"),
                                            arguments.unwrap_or("{}"),
                                        ),
                                    );
                                    if let Some(output) = item.get("output") {
                                        huggingface_finalize_tool_call(
                                            &mut stream,
                                            &mut active_tool_inputs,
                                            &mut ended_tool_inputs,
                                            &mut pending_tool_calls,
                                            &mut emitted_tool_calls,
                                            &mut emitted_tool_results,
                                            item_id,
                                            arguments,
                                            Some(output),
                                        );
                                    }
                                    saw_tool_calls = true;
                                }
                                Some("mcp_list_tools") => {
                                    let item_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name = "list_tools";
                                    let tool_call_id = item_id;
                                    let arguments = item
                                        .get("server_label")
                                        .and_then(JsonValue::as_str)
                                        .map(|server_label| {
                                            json!({ "server_label": server_label }).to_string()
                                        });
                                    pending_tool_calls.insert(
                                        item_id.to_string(),
                                        HuggingFacePendingToolCall::new(
                                            tool_call_id,
                                            tool_name,
                                            arguments.as_deref().unwrap_or("{}"),
                                        ),
                                    );
                                    if let Some(tools) = item.get("tools") {
                                        huggingface_finalize_tool_call(
                                            &mut stream,
                                            &mut active_tool_inputs,
                                            &mut ended_tool_inputs,
                                            &mut pending_tool_calls,
                                            &mut emitted_tool_calls,
                                            &mut emitted_tool_results,
                                            item_id,
                                            arguments.as_deref(),
                                            Some(&json!({ "tools": tools })),
                                        );
                                    }
                                    saw_tool_calls = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some("response.completed") => {
                        if let Some(response) = value.get("response") {
                            if response_id.is_none() {
                                response_id = response
                                    .get("id")
                                    .and_then(JsonValue::as_str)
                                    .map(ToString::to_string);
                            }
                            huggingface_emit_response_metadata(&mut stream, response);
                            usage = huggingface_responses_usage(response.get("usage"));
                            if let Some(reason) = response
                                .get("incomplete_details")
                                .and_then(|details| details.get("reason"))
                                .and_then(JsonValue::as_str)
                            {
                                finish_reason =
                                    map_huggingface_responses_finish_reason(Some(reason));
                            } else {
                                let response_has_tool_calls = response
                                    .get("output")
                                    .and_then(JsonValue::as_array)
                                    .is_some_and(|items| {
                                        items.iter().any(|item| {
                                            matches!(
                                                item.get("type").and_then(JsonValue::as_str),
                                                Some(
                                                    "function_call" | "mcp_call" | "mcp_list_tools"
                                                )
                                            )
                                        })
                                    });
                                finish_reason = map_huggingface_responses_finish_reason(Some(
                                    if response_has_tool_calls || saw_tool_calls {
                                        "tool_calls"
                                    } else {
                                        "stop"
                                    },
                                ));
                            }

                            if let Some(output) =
                                response.get("output").and_then(JsonValue::as_array)
                            {
                                for item in output {
                                    match item.get("type").and_then(JsonValue::as_str) {
                                        Some("message") => {
                                            let id = item
                                                .get("id")
                                                .and_then(JsonValue::as_str)
                                                .unwrap_or_default();
                                            let text = item
                                                .get("content")
                                                .and_then(JsonValue::as_array)
                                                .and_then(|content| {
                                                    content.iter().find_map(|part| {
                                                        (part
                                                            .get("type")
                                                            .and_then(JsonValue::as_str)
                                                            == Some("output_text"))
                                                        .then(|| {
                                                            part.get("text")
                                                                .and_then(JsonValue::as_str)
                                                        })
                                                        .flatten()
                                                    })
                                                });
                                            huggingface_finish_text_block(
                                                &mut stream,
                                                &mut text_buffers,
                                                &mut active_text,
                                                &mut ended_text,
                                                id,
                                                text,
                                                Some(huggingface_item_metadata(id)),
                                            );
                                        }
                                        Some("reasoning") => {
                                            let id = item
                                                .get("id")
                                                .and_then(JsonValue::as_str)
                                                .unwrap_or_default();
                                            let text = item
                                                .get("content")
                                                .or_else(|| item.get("summary"))
                                                .and_then(JsonValue::as_array)
                                                .and_then(|content| {
                                                    content.iter().find_map(|part| {
                                                        part.get("text").and_then(JsonValue::as_str)
                                                    })
                                                });
                                            huggingface_finish_reasoning_block(
                                                &mut stream,
                                                &mut reasoning_buffers,
                                                &mut active_reasoning,
                                                &mut ended_reasoning,
                                                id,
                                                text,
                                                Some(huggingface_item_metadata(id)),
                                            );
                                        }
                                        Some("function_call") => {
                                            let item_id = item
                                                .get("id")
                                                .and_then(JsonValue::as_str)
                                                .unwrap_or_default();
                                            huggingface_finalize_tool_call(
                                                &mut stream,
                                                &mut active_tool_inputs,
                                                &mut ended_tool_inputs,
                                                &mut pending_tool_calls,
                                                &mut emitted_tool_calls,
                                                &mut emitted_tool_results,
                                                item_id,
                                                item.get("arguments").and_then(JsonValue::as_str),
                                                item.get("output"),
                                            );
                                            saw_tool_calls = true;
                                        }
                                        Some("mcp_call") => {
                                            let item_id = item
                                                .get("id")
                                                .and_then(JsonValue::as_str)
                                                .unwrap_or_default();
                                            huggingface_finalize_tool_call(
                                                &mut stream,
                                                &mut active_tool_inputs,
                                                &mut ended_tool_inputs,
                                                &mut pending_tool_calls,
                                                &mut emitted_tool_calls,
                                                &mut emitted_tool_results,
                                                item_id,
                                                item.get("arguments").and_then(JsonValue::as_str),
                                                item.get("output"),
                                            );
                                            saw_tool_calls = true;
                                        }
                                        Some("mcp_list_tools") => {
                                            let item_id = item
                                                .get("id")
                                                .and_then(JsonValue::as_str)
                                                .unwrap_or_default();
                                            huggingface_finalize_tool_call(
                                                &mut stream,
                                                &mut active_tool_inputs,
                                                &mut ended_tool_inputs,
                                                &mut pending_tool_calls,
                                                &mut emitted_tool_calls,
                                                &mut emitted_tool_results,
                                                item_id,
                                                item.get("server_label")
                                                    .and_then(JsonValue::as_str)
                                                    .map(|server_label| {
                                                        json!({ "server_label": server_label })
                                                            .to_string()
                                                    })
                                                    .as_deref(),
                                                item.get("tools")
                                                    .map(|tools| json!({ "tools": tools }))
                                                    .as_ref(),
                                            );
                                            saw_tool_calls = true;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Some("response.incomplete") => {
                        if let Some(response) = value.get("response") {
                            usage = huggingface_responses_usage(response.get("usage"));
                            finish_reason = map_huggingface_responses_finish_reason(
                                response
                                    .get("incomplete_details")
                                    .and_then(|details| details.get("reason"))
                                    .and_then(JsonValue::as_str),
                            );
                        }
                    }
                    Some("response.failed") => {
                        if let Some(response) = value.get("response") {
                            usage = huggingface_responses_usage(response.get("usage"));
                            if let Some(incomplete_reason) = response
                                .get("incomplete_details")
                                .and_then(|details| details.get("reason"))
                                .and_then(JsonValue::as_str)
                            {
                                finish_reason = map_huggingface_responses_finish_reason(Some(
                                    incomplete_reason,
                                ));
                            } else if !saw_error_event {
                                finish_reason = LanguageModelFinishReason {
                                    unified: FinishReason::Error,
                                    raw: Some("huggingface-error".to_string()),
                                };
                            }
                        }
                    }
                    _ => {}
                }
            }
            ParseJsonResult::Failure { error, raw_value } => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("huggingface-parse-error".to_string()),
                };
                stream.push(huggingface_stream_error(
                    error.to_string(),
                    raw_value.as_ref().map(JsonValue::to_string).as_deref(),
                ));
            }
        }
    }

    for id in active_reasoning.clone() {
        huggingface_finish_reasoning_block(
            &mut stream,
            &mut reasoning_buffers,
            &mut active_reasoning,
            &mut ended_reasoning,
            &id,
            None,
            Some(huggingface_item_metadata(&id)),
        );
    }

    for id in active_text.clone() {
        huggingface_finish_text_block(
            &mut stream,
            &mut text_buffers,
            &mut active_text,
            &mut ended_text,
            &id,
            None,
            Some(huggingface_item_metadata(&id)),
        );
    }

    for id in active_tool_inputs.clone() {
        huggingface_finalize_tool_call(
            &mut stream,
            &mut active_tool_inputs,
            &mut ended_tool_inputs,
            &mut pending_tool_calls,
            &mut emitted_tool_calls,
            &mut emitted_tool_results,
            &id,
            None,
            None,
        );
    }

    let mut finish = LanguageModelStreamFinish::new(usage, finish_reason);
    if let Some(response_id) = response_id.as_deref() {
        finish = finish.with_provider_metadata(huggingface_response_metadata(response_id));
    }
    stream.push(LanguageModelStreamPart::Finish(finish));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));
    if let Some(headers) = response_headers {
        result = result.with_response(huggingface_stream_response_with_headers(headers));
    }
    result
}

fn huggingface_stream_error(
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

fn huggingface_stream_event_error(
    value: &JsonValue,
    raw_body: Option<&str>,
) -> LanguageModelStreamPart {
    let mut error = value.as_object().cloned().unwrap_or_default();
    if !error.contains_key("error") {
        error
            .entry("message".to_string())
            .or_insert_with(|| JsonValue::String(huggingface_error_message(value)));
    }
    if let Some(raw_body) = raw_body {
        error
            .entry("body".to_string())
            .or_insert_with(|| JsonValue::String(raw_body.to_string()));
    }

    LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(JsonValue::Object(error)))
}

fn huggingface_stream_response_with_headers(headers: Headers) -> LanguageModelStreamResultResponse {
    let mut response = LanguageModelStreamResultResponse::new();
    for (name, value) in headers {
        response = response.with_header(name, value);
    }
    response
}

fn huggingface_emit_response_metadata(
    stream: &mut Vec<LanguageModelStreamPart>,
    response: &JsonValue,
) -> bool {
    let mut metadata = LanguageModelStreamResponseMetadata::new();
    let mut emitted = false;
    if let Some(id) = response.get("id").and_then(JsonValue::as_str) {
        metadata = metadata.with_id(id);
        emitted = true;
    }
    if let Some(timestamp) = response
        .get("created_at")
        .and_then(JsonValue::as_i64)
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
    {
        metadata = metadata.with_timestamp(timestamp);
        emitted = true;
    }
    if let Some(model_id) = response.get("model").and_then(JsonValue::as_str) {
        metadata = metadata.with_model_id(model_id);
        emitted = true;
    }
    if emitted {
        stream.push(LanguageModelStreamPart::ResponseMetadata(metadata));
    }
    emitted
}

fn huggingface_stream_block_id(prefix: &str, value: &JsonValue) -> String {
    let mut parts = vec![prefix.to_string()];
    if let Some(item_id) = huggingface_stream_item_id(value) {
        parts.push(item_id);
    }
    if let Some(output_index) = value
        .get("output_index")
        .and_then(JsonValue::as_u64)
        .map(|index| index.to_string())
    {
        parts.push(output_index);
    }
    if parts.len() == 1 {
        parts.push("0".to_string());
    }
    parts.join("-")
}

fn huggingface_stream_item_id(value: &JsonValue) -> Option<String> {
    value
        .get("item_id")
        .or_else(|| value.get("item").and_then(|item| item.get("id")))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
}

fn huggingface_stream_text_id(value: &JsonValue) -> String {
    huggingface_stream_item_id(value).unwrap_or_else(|| huggingface_stream_block_id("txt", value))
}

fn huggingface_stream_reasoning_id(value: &JsonValue) -> String {
    huggingface_stream_item_id(value)
        .unwrap_or_else(|| huggingface_stream_block_id("reasoning", value))
}

fn huggingface_start_text_block(
    stream: &mut Vec<LanguageModelStreamPart>,
    active_text: &mut BTreeSet<String>,
    ended_text: &BTreeSet<String>,
    id: &str,
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_text.contains(id) {
        return;
    }

    if active_text.insert(id.to_string()) {
        let mut start = LanguageModelTextStart::new(id);
        if let Some(provider_metadata) = provider_metadata {
            start = start.with_provider_metadata(provider_metadata);
        }
        stream.push(LanguageModelStreamPart::TextStart(start));
    }
}

fn huggingface_push_text_delta(
    stream: &mut Vec<LanguageModelStreamPart>,
    text_buffers: &mut BTreeMap<String, String>,
    active_text: &mut BTreeSet<String>,
    ended_text: &BTreeSet<String>,
    id: &str,
    delta: &str,
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_text.contains(id) {
        return;
    }

    huggingface_start_text_block(stream, active_text, ended_text, id, provider_metadata);
    text_buffers
        .entry(id.to_string())
        .or_default()
        .push_str(delta);
    stream.push(LanguageModelStreamPart::TextDelta(
        LanguageModelTextDelta::new(id, delta),
    ));
}

fn huggingface_finish_text_block(
    stream: &mut Vec<LanguageModelStreamPart>,
    text_buffers: &mut BTreeMap<String, String>,
    active_text: &mut BTreeSet<String>,
    ended_text: &mut BTreeSet<String>,
    id: &str,
    final_text: Option<&str>,
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_text.contains(id) {
        return;
    }

    let buffered = text_buffers.remove(id).unwrap_or_default();
    let emitted_final_text = buffered.is_empty() && final_text.is_some_and(|text| !text.is_empty());
    if emitted_final_text && let Some(text) = final_text {
        huggingface_push_text_delta(
            stream,
            text_buffers,
            active_text,
            ended_text,
            id,
            text,
            provider_metadata.clone(),
        );
        text_buffers.remove(id);
    }

    if active_text.remove(id) || !buffered.is_empty() || emitted_final_text {
        let mut end = LanguageModelTextEnd::new(id);
        if let Some(provider_metadata) = provider_metadata {
            end = end.with_provider_metadata(provider_metadata);
        }
        stream.push(LanguageModelStreamPart::TextEnd(end));
        ended_text.insert(id.to_string());
    }
}

fn huggingface_start_reasoning_block(
    stream: &mut Vec<LanguageModelStreamPart>,
    active_reasoning: &mut BTreeSet<String>,
    ended_reasoning: &BTreeSet<String>,
    id: &str,
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_reasoning.contains(id) {
        return;
    }

    if active_reasoning.insert(id.to_string()) {
        let mut start = LanguageModelReasoningStart::new(id);
        if let Some(provider_metadata) = provider_metadata {
            start = start.with_provider_metadata(provider_metadata);
        }
        stream.push(LanguageModelStreamPart::ReasoningStart(start));
    }
}

fn huggingface_push_reasoning_delta(
    stream: &mut Vec<LanguageModelStreamPart>,
    reasoning_buffers: &mut BTreeMap<String, String>,
    active_reasoning: &mut BTreeSet<String>,
    ended_reasoning: &BTreeSet<String>,
    id: &str,
    delta: &str,
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_reasoning.contains(id) {
        return;
    }

    huggingface_start_reasoning_block(
        stream,
        active_reasoning,
        ended_reasoning,
        id,
        provider_metadata.clone(),
    );
    reasoning_buffers
        .entry(id.to_string())
        .or_default()
        .push_str(delta);
    let mut reasoning_delta = LanguageModelReasoningDelta::new(id, delta);
    if let Some(provider_metadata) = provider_metadata {
        reasoning_delta = reasoning_delta.with_provider_metadata(provider_metadata);
    }
    stream.push(LanguageModelStreamPart::ReasoningDelta(reasoning_delta));
}

fn huggingface_finish_reasoning_block(
    stream: &mut Vec<LanguageModelStreamPart>,
    reasoning_buffers: &mut BTreeMap<String, String>,
    active_reasoning: &mut BTreeSet<String>,
    ended_reasoning: &mut BTreeSet<String>,
    id: &str,
    final_text: Option<&str>,
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_reasoning.contains(id) {
        return;
    }

    let buffered = reasoning_buffers.remove(id).unwrap_or_default();
    let emitted_final_text = buffered.is_empty() && final_text.is_some_and(|text| !text.is_empty());
    if emitted_final_text && let Some(text) = final_text {
        huggingface_push_reasoning_delta(
            stream,
            reasoning_buffers,
            active_reasoning,
            ended_reasoning,
            id,
            text,
            provider_metadata.clone(),
        );
        reasoning_buffers.remove(id);
    }

    if active_reasoning.remove(id) || !buffered.is_empty() || emitted_final_text {
        let mut end = LanguageModelReasoningEnd::new(id);
        if let Some(provider_metadata) = provider_metadata {
            end = end.with_provider_metadata(provider_metadata);
        }
        stream.push(LanguageModelStreamPart::ReasoningEnd(end));
        ended_reasoning.insert(id.to_string());
    }
}

#[derive(Clone, Debug, Default)]
struct HuggingFacePendingToolCall {
    tool_call_id: String,
    tool_name: String,
    arguments: String,
}

impl HuggingFacePendingToolCall {
    fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            arguments: arguments.into(),
        }
    }
}

fn huggingface_start_tool_input(
    stream: &mut Vec<LanguageModelStreamPart>,
    active_tool_inputs: &mut BTreeSet<String>,
    ended_tool_inputs: &BTreeSet<String>,
    tool_call_id: &str,
    tool_name: &str,
) {
    if ended_tool_inputs.contains(tool_call_id) {
        return;
    }

    if active_tool_inputs.insert(tool_call_id.to_string()) {
        stream.push(LanguageModelStreamPart::ToolInputStart(
            LanguageModelToolInputStart::new(tool_call_id, tool_name),
        ));
    }
}

fn huggingface_append_tool_call_arguments(
    pending_tool_calls: &mut BTreeMap<String, HuggingFacePendingToolCall>,
    item_id: &str,
    delta: &str,
) {
    pending_tool_calls
        .entry(item_id.to_string())
        .or_default()
        .arguments
        .push_str(delta);
}

#[allow(clippy::too_many_arguments)]
fn huggingface_finalize_tool_call(
    stream: &mut Vec<LanguageModelStreamPart>,
    active_tool_inputs: &mut BTreeSet<String>,
    ended_tool_inputs: &mut BTreeSet<String>,
    pending_tool_calls: &mut BTreeMap<String, HuggingFacePendingToolCall>,
    emitted_tool_calls: &mut BTreeSet<String>,
    emitted_tool_results: &mut BTreeSet<String>,
    item_id: &str,
    final_arguments: Option<&str>,
    output: Option<&JsonValue>,
) {
    let pending = pending_tool_calls
        .entry(item_id.to_string())
        .or_insert_with(|| HuggingFacePendingToolCall::new(item_id, item_id, ""));

    if pending.tool_call_id.is_empty() {
        pending.tool_call_id = item_id.to_string();
    }
    if pending.tool_name.is_empty() {
        pending.tool_name = item_id.to_string();
    }

    if pending.arguments.is_empty()
        && !ended_tool_inputs.contains(&pending.tool_call_id)
        && let Some(arguments) = final_arguments
        && !arguments.is_empty()
    {
        huggingface_start_tool_input(
            stream,
            active_tool_inputs,
            ended_tool_inputs,
            &pending.tool_call_id,
            &pending.tool_name,
        );
        stream.push(LanguageModelStreamPart::ToolInputDelta(
            LanguageModelToolInputDelta::new(&pending.tool_call_id, arguments),
        ));
        pending.arguments.push_str(arguments);
    }

    if (active_tool_inputs.remove(&pending.tool_call_id) || !pending.arguments.is_empty())
        && !ended_tool_inputs.contains(&pending.tool_call_id)
    {
        stream.push(LanguageModelStreamPart::ToolInputEnd(
            LanguageModelToolInputEnd::new(&pending.tool_call_id),
        ));
        ended_tool_inputs.insert(pending.tool_call_id.clone());
    }

    let tool_call_key = format!("call:{}", pending.tool_call_id);
    if emitted_tool_calls.insert(tool_call_key) {
        let mut tool_call = LanguageModelToolCall::new(
            &pending.tool_call_id,
            &pending.tool_name,
            if pending.arguments.is_empty() {
                final_arguments.unwrap_or("{}").to_string()
            } else {
                pending.arguments.clone()
            },
        );
        let metadata = huggingface_item_metadata(item_id);
        tool_call = tool_call.with_provider_metadata(metadata);
        stream.push(LanguageModelStreamPart::ToolCall(tool_call));
    }

    if let Some(output) = output.filter(|value| !value.is_null()) {
        let result_key = format!("result:{}", pending.tool_call_id);
        if emitted_tool_results.insert(result_key)
            && let Ok(result) = NonNullJsonValue::new(output.clone())
        {
            let mut tool_result =
                LanguageModelToolResult::new(&pending.tool_call_id, &pending.tool_name, result);
            if output.is_object() && pending.tool_name.starts_with("mcp.") {
                tool_result = tool_result.with_provider_metadata(ProviderMetadata::new());
            }
            stream.push(LanguageModelStreamPart::ToolResult(tool_result));
        }
    }
}

fn huggingface_response_metadata(response_id: &str) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert(
        "responseId".to_string(),
        JsonValue::String(response_id.to_string()),
    );
    metadata.insert(HUGGINGFACE_PROVIDER_OPTIONS_NAME.to_string(), provider);
    metadata
}

fn huggingface_item_metadata(item_id: &str) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));
    metadata.insert(HUGGINGFACE_PROVIDER_OPTIONS_NAME.to_string(), provider);
    metadata
}

fn huggingface_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(HUGGINGFACE_PROVIDER_OPTIONS_NAME.to_string(), provider);
    metadata
}

fn response_metadata_with_headers(
    mut response: LanguageModelResponse,
    headers: Headers,
) -> LanguageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }
    response
}

fn huggingface_base_url(settings: &HuggingFaceProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_HUGGINGFACE_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn huggingface_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("HUGGINGFACE_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn default_huggingface_transport() -> HuggingFaceTransport {
    Arc::new(|request| Box::pin(ready(execute_huggingface_request(request))))
}

fn execute_huggingface_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Post => execute_huggingface_post_request(request),
        ProviderApiRequestMethod::Get => Err(FetchErrorInfo::new(
            "GET requests are not supported by the Hugging Face transport",
        )),
    }
}

fn execute_huggingface_post_request(
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
                "multipart form data is not supported by the Hugging Face transport",
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
        DEFAULT_HUGGINGFACE_BASE_URL, HuggingFaceProvider, HuggingFaceProviderSettings,
        HuggingFaceTransport, HuggingFaceTransportFuture, create_huggingface, huggingface,
    };
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelMessage,
        LanguageModelResponseFormat, LanguageModelStreamPart, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelToolMessage, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderMetadata, ProviderOptions};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    #[test]
    fn huggingface_provider_generates_text_with_request_and_response_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HuggingFaceTransport =
            Arc::new(move |request| -> HuggingFaceTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_hf",
                        "model": "deepseek-ai/DeepSeek-V3-0324",
                        "object": "response",
                        "created_at": 1711115037,
                        "status": "completed",
                        "error": null,
                        "incomplete_details": null,
                        "usage": {
                            "input_tokens": 12,
                            "input_tokens_details": {
                                "cached_tokens": 2
                            },
                            "output_tokens": 25,
                            "output_tokens_details": {
                                "reasoning_tokens": 3
                            },
                            "total_tokens": 37
                        },
                        "output": [
                            {
                                "id": "msg_hf",
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from Hugging Face"
                                    }
                                ]
                            }
                        ],
                        "output_text": "Hello from Hugging Face"
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_huggingface".to_string(),
                )])))))
            });
        let provider = create_huggingface(
            HuggingFaceProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://router.huggingface.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("deepseek-ai/DeepSeek-V3-0324");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_top_p(0.9),
        ));

        assert_eq!(model.provider(), "huggingface.responses");
        assert_eq!(model.model_id(), "deepseek-ai/DeepSeek-V3-0324");
        assert_eq!(result.text, "Hello from Hugging Face");
        assert_eq!(result.usage.input_tokens.total, Some(12));
        assert_eq!(result.usage.input_tokens.no_cache, Some(10));
        assert_eq!(result.usage.input_tokens.cache_read, Some(2));
        assert_eq!(result.usage.output_tokens.total, Some(25));
        assert_eq!(result.usage.output_tokens.text, Some(22));
        assert_eq!(result.usage.output_tokens.reasoning, Some(3));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("resp_hf")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("huggingface")
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_hf")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://router.huggingface.test/v1/responses");
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
                .is_some_and(|value| value.contains("ai-sdk/huggingface/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "deepseek-ai/DeepSeek-V3-0324",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Hello"
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
                "top_p": 0.9,
                "max_output_tokens": 16,
                "stream": false
            }))
        );
    }

    #[test]
    fn huggingface_responses_maps_system_provider_options_and_structured_output() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HuggingFaceTransport =
            Arc::new(move |request| -> HuggingFaceTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_hf_structured",
                        "model": "moonshotai/Kimi-K2-Instruct",
                        "object": "response",
                        "created_at": 1711115037,
                        "status": "completed",
                        "error": null,
                        "incomplete_details": { "reason": "length" },
                        "usage": null,
                        "output": [],
                        "output_text": null
                    })
                    .to_string(),
                ))))
            });
        let provider = HuggingFaceProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://router.huggingface.test/v1")
            .with_transport(transport);
        let model = provider.responses("moonshotai/Kimi-K2-Instruct");
        let mut schema = JsonObject::new();
        schema.insert("type".to_string(), JsonValue::String("object".to_string()));
        let mut huggingface_options = JsonObject::new();
        huggingface_options.insert("metadata".to_string(), json!({ "trace": "abc" }));
        huggingface_options.insert(
            "instructions".to_string(),
            JsonValue::String("Be terse.".to_string()),
        );
        huggingface_options.insert("strictJsonSchema".to_string(), JsonValue::Bool(true));
        huggingface_options.insert(
            "reasoningEffort".to_string(),
            JsonValue::String("low".to_string()),
        );
        let provider_options =
            ProviderOptions::from([("huggingface".to_string(), huggingface_options)]);
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::System(LanguageModelSystemMessage::new(
                        "You are a helpful assistant.",
                    )),
                    LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                    ])),
                ])
                .with_response_format(
                    LanguageModelResponseFormat::json()
                        .with_schema(schema)
                        .with_name("answer")
                        .with_description("Short answer."),
                )
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Length);
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
                "model": "moonshotai/Kimi-K2-Instruct",
                "input": [
                    {
                        "role": "system",
                        "content": "You are a helpful assistant."
                    },
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Hello"
                            }
                        ]
                    }
                ],
                "text": {
                    "format": {
                        "type": "json_schema",
                        "strict": true,
                        "name": "answer",
                        "description": "Short answer.",
                        "schema": {
                            "type": "object"
                        }
                    }
                },
                "metadata": {
                    "trace": "abc"
                },
                "instructions": "Be terse.",
                "reasoning": {
                    "effort": "low"
                },
                "stream": false
            }))
        );
    }

    #[test]
    fn huggingface_responses_converts_images_tool_messages_and_content_parts() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HuggingFaceTransport =
            Arc::new(move |request| -> HuggingFaceTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_hf_content",
                        "model": "deepseek-ai/DeepSeek-V3-0324",
                        "object": "response",
                        "created_at": 1711115037,
                        "status": "completed",
                        "error": null,
                        "incomplete_details": null,
                        "usage": null,
                        "output": [
                            {
                                "id": "reasoning_hf",
                                "type": "reasoning",
                                "content": [
                                    {
                                        "type": "reasoning_text",
                                        "text": "Thinking"
                                    }
                                ]
                            },
                            {
                                "id": "msg_hf",
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "See this source",
                                        "annotations": [
                                            {
                                                "type": "url_citation",
                                                "url": "https://example.com/article",
                                                "title": "Article"
                                            }
                                        ]
                                    }
                                ]
                            },
                            {
                                "id": "mcp_hf",
                                "type": "mcp_call",
                                "name": "search",
                                "arguments": "{\"query\":\"rust\"}",
                                "output": "found"
                            }
                        ],
                        "output_text": null
                    })
                    .to_string(),
                ))))
            });
        let provider = HuggingFaceProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://router.huggingface.test/v1")
            .with_transport(transport);
        let model = provider.responses("deepseek-ai/DeepSeek-V3-0324");
        let result =
            poll_ready(
                model.do_generate(LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                            "What do you see?",
                        )),
                        LanguageModelUserContentPart::File(
                            crate::language_model::LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Bytes(vec![1, 2, 3, 4]),
                                },
                                "image/jpeg",
                            ),
                        ),
                        LanguageModelUserContentPart::File(
                            crate::language_model::LanguageModelFilePart::new(
                                FileData::Url {
                                    url: Url::parse("https://example.com/image.png")
                                        .expect("url parses"),
                                },
                                "image/png",
                            ),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(Vec::new())),
                ])),
            );

        assert!(
            result
                .warnings
                .iter()
                .any(|warning| matches!(warning, Warning::Unsupported { feature, .. } if feature == "tool messages"))
        );
        assert_eq!(
            serde_json::to_value(&result.content).expect("content serializes"),
            json!([
                {
                    "type": "reasoning",
                    "text": "Thinking",
                    "providerMetadata": {
                        "huggingface": {
                            "itemId": "reasoning_hf"
                        }
                    }
                },
                {
                    "type": "text",
                    "text": "See this source",
                    "providerMetadata": {
                        "huggingface": {
                            "itemId": "msg_hf"
                        }
                    }
                },
                {
                    "type": "source",
                    "sourceType": "url",
                    "id": "source-0",
                    "url": "https://example.com/article",
                    "title": "Article"
                },
                {
                    "type": "tool-call",
                    "toolCallId": "mcp_hf",
                    "toolName": "search",
                    "input": "{\"query\":\"rust\"}",
                    "providerExecuted": true
                },
                {
                    "type": "tool-result",
                    "toolCallId": "mcp_hf",
                    "toolName": "search",
                    "result": "found",
                    "providerMetadata": {}
                }
            ])
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body parses");
        assert_eq!(
            body.get("input")
                .and_then(JsonValue::as_array)
                .and_then(|input| input.first())
                .and_then(|message| message.get("content")),
            Some(&json!([
                {
                    "type": "input_text",
                    "text": "What do you see?"
                },
                {
                    "type": "input_image",
                    "image_url": "data:image/jpeg;base64,AQIDBA=="
                },
                {
                    "type": "input_image",
                    "image_url": "https://example.com/image.png"
                }
            ]))
        );
    }

    #[test]
    fn huggingface_responses_reports_unsupported_provider_references() {
        let provider = HuggingFaceProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://router.huggingface.test/v1");
        let model = provider.responses("deepseek-ai/DeepSeek-V3-0324");
        let mut references = BTreeMap::new();
        references.insert("huggingface".to_string(), "file_123".to_string());
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::File(
                        crate::language_model::LanguageModelFilePart::new(
                            FileData::Reference {
                                reference: ProviderReference::from_map(references)
                                    .expect("provider reference is valid"),
                            },
                            "image/png",
                        ),
                    ),
                ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("huggingface")
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some(
                "Hugging Face Responses file parts with provider references are not implemented yet."
            )
        );
    }

    #[test]
    fn huggingface_responses_maps_warnings_and_stream_errors() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HuggingFaceTransport =
            Arc::new(move |request| -> HuggingFaceTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "Hugging Face rejected the request"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "content-type".to_string(),
                    "application/json".to_string(),
                )])))))
            });
        let provider = HuggingFaceProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://router.huggingface.test/v1")
            .with_transport(transport);
        let model = provider.responses("deepseek-ai/DeepSeek-V3-0324");
        let options = LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Hello"),
            )]),
        )])
        .with_top_k(10)
        .with_seed(123)
        .with_presence_penalty(0.5)
        .with_frequency_penalty(0.3)
        .with_stop_sequence("stop");
        let result = poll_ready(model.do_generate(options.clone()));

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("huggingface")
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Hugging Face rejected the request")
        );
        for expected in [
            "topK",
            "seed",
            "presencePenalty",
            "frequencyPenalty",
            "stopSequences",
        ] {
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| matches!(warning, Warning::Unsupported { feature, .. } if feature == expected)),
                "missing warning for {expected}"
            );
        }

        let stream = poll_ready(model.do_stream(options));
        match stream.stream.as_slice() {
            [LanguageModelStreamPart::Error(error)] => {
                assert_eq!(
                    error.error.get("message").and_then(JsonValue::as_str),
                    Some("Hugging Face rejected the request")
                );
            }
            parts => panic!("expected one error stream part, got {parts:?}"),
        }

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
                "model": "deepseek-ai/DeepSeek-V3-0324",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Hello"
                            }
                        ]
                    }
                ],
                "stream": true
            }))
        );
    }

    #[test]
    fn huggingface_responses_streams_text_with_request_and_response_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HuggingFaceTransport = Arc::new(
            move |request| -> HuggingFaceTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_hf_stream","created_at":1711115037,"model":"deepseek-ai/DeepSeek-V3-0324"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_hf_stream","output_index":0,"content_index":0,"delta":"Hello"}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_hf_stream","output_index":0,"content_index":0,"delta":" from Hugging Face"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"msg_hf_stream","output_index":0,"content_index":0,"text":"Hello from Hugging Face"}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_hf_stream","created_at":1711115037,"model":"deepseek-ai/DeepSeek-V3-0324","usage":{"input_tokens":5,"output_tokens":4}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse)
                    .with_headers(Headers::from([(
                        "content-type".to_string(),
                        "text/event-stream".to_string(),
                    )])))))
            },
        );
        let provider = HuggingFaceProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://router.huggingface.test/v1")
            .with_transport(transport);
        let model = provider.responses("deepseek-ai/DeepSeek-V3-0324");
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Hello"),
                    )]),
                )])
                .with_max_output_tokens(16)
                .with_temperature(0.0),
            ),
        );

        assert!(matches!(
            result.stream.first(),
            Some(LanguageModelStreamPart::StreamStart(start)) if start.warnings.is_empty()
        ));
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::ResponseMetadata(metadata) => Some(metadata),
                _ => None,
            }),
            Some(metadata)
                if metadata.id.as_deref() == Some("resp_hf_stream")
                    && metadata.model_id.as_deref() == Some("deepseek-ai/DeepSeek-V3-0324")
        ));
        let text_deltas = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::TextDelta(delta) => Some(delta.delta.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_deltas, vec!["Hello", " from Hugging Face"]);
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::TextEnd(end) => Some(end),
                _ => None,
            }),
            Some(end) if end.id == "msg_hf_stream"
        ));
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            }),
            Some(finish)
                if finish.finish_reason.unified == FinishReason::Stop
                    && finish.usage.input_tokens.total == Some(5)
                    && finish.usage.output_tokens.total == Some(4)
        ));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("content-type"))
                .map(String::as_str),
            Some("text/event-stream")
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
                "model": "deepseek-ai/DeepSeek-V3-0324",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Hello"
                            }
                        ]
                    }
                ],
                "max_output_tokens": 16,
                "temperature": 0.0,
                "stream": true
            }))
        );
    }

    #[test]
    fn huggingface_responses_streams_reasoning_text_and_tool_calls() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: HuggingFaceTransport = Arc::new(
            move |request| -> HuggingFaceTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_hf_tool_stream","created_at":1711115037,"model":"deepseek-ai/DeepSeek-V3-0324"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"reasoning_hf","type":"reasoning","content":[],"summary":[]}}"#,
                    "",
                    r#"data: {"type":"response.reasoning_text.delta","item_id":"reasoning_hf","output_index":0,"content_index":0,"delta":"Thinking"}"#,
                    "",
                    r#"data: {"type":"response.reasoning_text.done","item_id":"reasoning_hf","output_index":0,"content_index":0,"text":"Thinking"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"reasoning_hf","type":"reasoning","content":[{"type":"reasoning_text","text":"Thinking"}],"summary":[]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"msg_hf_tool_stream","type":"message","role":"assistant","content":[]}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_hf_tool_stream","output_index":1,"content_index":0,"delta":"I"}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_hf_tool_stream","output_index":1,"content_index":0,"delta":"'ll get"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"msg_hf_tool_stream","output_index":1,"content_index":0,"text":"I'll get"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"msg_hf_tool_stream","type":"message","role":"assistant","content":[{"type":"output_text","text":"I'll get"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":2,"item":{"id":"fc_hf_tool_stream","type":"function_call","call_id":"call_weather","name":"weather","arguments":""}}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_hf_tool_stream","output_index":2,"delta":"{\"location\""}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_hf_tool_stream","output_index":2,"delta":":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_hf_tool_stream","output_index":2,"arguments":"{\"location\":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"fc_hf_tool_stream","type":"function_call","call_id":"call_weather","name":"weather","arguments":"{\"location\":\"Brisbane\"}","output":"sunny"}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_hf_tool_stream","created_at":1711115037,"model":"deepseek-ai/DeepSeek-V3-0324","output":[{"id":"reasoning_hf","type":"reasoning","content":[{"type":"reasoning_text","text":"Thinking"}],"summary":[]},{"id":"msg_hf_tool_stream","type":"message","role":"assistant","content":[{"type":"output_text","text":"I'll get"}]},{"id":"fc_hf_tool_stream","type":"function_call","call_id":"call_weather","name":"weather","arguments":"{\"location\":\"Brisbane\"}","output":"sunny"}],"usage":{"input_tokens":6,"output_tokens":3}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse)
                    .with_headers(Headers::from([(
                        "content-type".to_string(),
                        "text/event-stream".to_string(),
                    )])))))
            },
        );
        let provider = HuggingFaceProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://router.huggingface.test/v1")
            .with_transport(transport);
        let model = provider.responses("deepseek-ai/DeepSeek-V3-0324");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                    "Weather in Brisbane?",
                )),
            ])),
        ])));

        let reasoning_deltas = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ReasoningDelta(delta) => Some(delta.delta.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(reasoning_deltas, vec!["Thinking"]);
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::ReasoningStart(start) => Some(start),
                _ => None,
            }),
            Some(start) if start.id == "reasoning_hf"
        ));
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::ReasoningEnd(end) => Some(end),
                _ => None,
            }),
            Some(end) if end.id == "reasoning_hf"
        ));

        let text_deltas = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::TextDelta(delta) => Some(delta.delta.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_deltas, vec!["I", "'ll get"]);
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::TextStart(start) => Some(start),
                _ => None,
            }),
            Some(start) if start.id == "msg_hf_tool_stream"
        ));
        assert!(matches!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::TextEnd(end) => Some(end),
                _ => None,
            }),
            Some(end) if end.id == "msg_hf_tool_stream"
        ));

        let tool_call = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .expect("stream includes a tool call");
        assert_eq!(tool_call.tool_call_id, "call_weather");
        assert_eq!(tool_call.tool_name, "weather");
        assert_eq!(tool_call.input, r#"{"location":"Brisbane"}"#);

        let tool_result = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .expect("stream includes a tool result");
        assert_eq!(tool_result.tool_call_id, "call_weather");
        assert_eq!(tool_result.tool_name, "weather");
        assert_eq!(tool_result.result.as_value(), &json!("sunny"));

        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream includes finish part");
        assert_eq!(finish.finish_reason.unified, FinishReason::ToolCalls);
        assert_eq!(finish.usage.input_tokens.total, Some(6));
        assert_eq!(finish.usage.output_tokens.total, Some(3));

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
                "model": "deepseek-ai/DeepSeek-V3-0324",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Weather in Brisbane?"
                            }
                        ]
                    }
                ],
                "stream": true
            }))
        );
    }

    #[test]
    fn huggingface_provider_reports_unsupported_embedding_and_image() {
        let provider = HuggingFaceProvider::new();
        let embedding = match provider.embedding_model("embedding-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        assert_eq!(
            embedding.message(),
            "Hugging Face Responses API does not support text embeddings. Use the Hugging Face Inference API directly for embeddings."
        );
        let text_embedding = match provider.text_embedding_model("embedding-model") {
            Ok(_) => panic!("text embedding models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(text_embedding.model_type(), ModelType::EmbeddingModel);
        let image = match provider.image_model("image-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(
            image.message(),
            "Hugging Face Responses API does not support image generation. Use the Hugging Face Inference API directly for image models."
        );
    }

    #[test]
    fn huggingface_provider_uses_default_base_url_and_function_alias() {
        let model = huggingface("deepseek-ai/DeepSeek-V3-0324");

        assert_eq!(model.provider(), "huggingface.responses");
        assert_eq!(model.model_id(), "deepseek-ai/DeepSeek-V3-0324");
        assert_eq!(
            super::huggingface_base_url(&HuggingFaceProviderSettings::new()),
            DEFAULT_HUGGINGFACE_BASE_URL
        );
    }

    #[test]
    fn huggingface_provider_implements_provider_trait() {
        let provider = HuggingFaceProvider::new();
        let model = Provider::language_model(&provider, "deepseek-ai/DeepSeek-V3-0324")
            .expect("language model exists");

        assert_eq!(model.provider(), "huggingface.responses");
        assert!(Provider::embedding_model(&provider, "embedding-model").is_err());
        assert!(Provider::image_model(&provider, "image-model").is_err());
    }

    #[test]
    fn huggingface_provider_settings_serde_accepts_upstream_base_url() {
        let settings: HuggingFaceProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://router.huggingface.test/v1/",
            "apiKey": "test-api-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            HuggingFaceProviderSettings::new()
                .with_base_url("https://router.huggingface.test/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://router.huggingface.test/v1/",
                "apiKey": "test-api-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => {
                struct NoopWake;

                impl Wake for NoopWake {
                    fn wake(self: Arc<Self>) {}
                }

                let waker = Waker::from(Arc::new(NoopWake));
                let mut context = Context::from_waker(&waker);
                loop {
                    match Pin::new(&mut future).poll(&mut context) {
                        Poll::Ready(value) => break value,
                        Poll::Pending => std::thread::yield_now(),
                    }
                }
            }
        }
    }
}
