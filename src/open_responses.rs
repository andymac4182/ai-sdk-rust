use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelReasoning, LanguageModelRequest, LanguageModelResponse, LanguageModelStreamPart,
    LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelText,
    LanguageModelToolCall, LanguageModelUsage, LanguageModelUserContentPart, OutputTokenUsage,
};
use crate::openai_compatible::{OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, SpecificationVersion,
};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers,
    create_json_error_response_handler, create_json_response_handler, post_json_to_api,
    with_user_agent_suffix,
};
use crate::warning::Warning;

/// Future returned by an injected Open Responses HTTP transport.
pub type OpenResponsesTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Open Responses provider models.
pub type OpenResponsesTransport =
    Arc<dyn Fn(ProviderApiRequest) -> OpenResponsesTransportFuture + Send + Sync>;

/// Settings for an Open Responses provider instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResponsesProviderSettings {
    /// URL for the Open Responses API POST endpoint.
    pub url: String,

    /// Provider name used as provider id prefix and provider-options key.
    pub name: String,

    /// API key used to build a `Bearer` authorization header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom headers included in model requests.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// User-agent suffix for wrappers built on the Open Responses transport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent_suffix: Option<String>,
}

impl OpenResponsesProviderSettings {
    /// Creates Open Responses provider settings.
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            api_key: None,
            headers: Headers::new(),
            user_agent_suffix: None,
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

    /// Sets the request user-agent suffix for wrappers built on this provider.
    pub fn with_user_agent_suffix(mut self, user_agent_suffix: impl Into<String>) -> Self {
        self.user_agent_suffix = Some(user_agent_suffix.into());
        self
    }
}

/// Open Responses provider.
#[derive(Clone)]
pub struct OpenResponsesProvider {
    settings: OpenResponsesProviderSettings,
    transport: OpenResponsesTransport,
}

impl OpenResponsesProvider {
    /// Creates a provider from explicit Open Responses settings.
    pub fn from_settings(settings: OpenResponsesProviderSettings) -> Self {
        Self {
            settings,
            transport: default_open_responses_transport(),
        }
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenResponsesTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Creates an Open Responses language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenResponsesLanguageModel {
        OpenResponsesLanguageModel::new(
            model_id,
            OpenResponsesModelConfig {
                provider: format!("{}.responses", self.settings.name),
                provider_options_name: self.settings.name.clone(),
                settings: self.settings.clone(),
                transport: Arc::clone(&self.transport),
            },
        )
    }

    /// Reports that Open Responses does not expose embedding models.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Reports that Open Responses does not expose image models.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Provider for OpenResponsesProvider {
    type LanguageModel = OpenResponsesLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(OpenResponsesProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        OpenResponsesProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        OpenResponsesProvider::image_model(self, model_id)
    }
}

/// Creates an Open Responses provider.
pub fn create_open_responses(settings: OpenResponsesProviderSettings) -> OpenResponsesProvider {
    OpenResponsesProvider::from_settings(settings)
}

#[derive(Clone)]
struct OpenResponsesModelConfig {
    provider: String,
    provider_options_name: String,
    settings: OpenResponsesProviderSettings,
    transport: OpenResponsesTransport,
}

/// Open Responses language model.
#[derive(Clone)]
pub struct OpenResponsesLanguageModel {
    model_id: String,
    config: OpenResponsesModelConfig,
}

impl OpenResponsesLanguageModel {
    fn new(model_id: impl Into<String>, config: OpenResponsesModelConfig) -> Self {
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

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) = match open_responses_request_body(
            &self.model_id,
            &self.config.provider_options_name,
            &options,
        ) {
            Ok(result) => result,
            Err(message) => {
                return open_responses_error_generate_result(
                    &self.config.provider_options_name,
                    message,
                    json!({ "model": self.model_id }),
                );
            }
        };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(self.config.settings.url.clone(), request_body)
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
                    open_responses_error_message,
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
        let request_body = open_responses_request_body(
            &self.model_id,
            &self.config.provider_options_name,
            &options,
        )
        .map(|(body, _)| body)
        .unwrap_or_else(|message| {
            json!({
                "model": self.model_id,
                "error": message
            })
        });

        open_responses_error_stream_result(
            "Open Responses streaming is not implemented yet.",
            request_body,
        )
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                open_responses_provider_headers(&self.config.settings)
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
        let (content, has_tool_calls) = open_responses_content(&response);
        let usage = open_responses_usage(response.get("usage"));
        let finish_reason = map_open_responses_finish_reason(
            response
                .get("incomplete_details")
                .and_then(|details| details.get("reason"))
                .and_then(JsonValue::as_str),
            has_tool_calls,
        );
        let raw_body = raw_response.unwrap_or_else(|| response.clone());
        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_request(LanguageModelRequest::new().with_body(request_body));
        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

        if let Some(id) = response.get("id").and_then(JsonValue::as_str) {
            response_metadata = response_metadata.with_id(id);
            result = result.with_provider_metadata(open_responses_provider_metadata(
                &self.config.provider_options_name,
                id,
            ));
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
    ) -> LanguageModelGenerateResult {
        let message = match error {
            HandledFetchError::Original { error } => error.message().to_string(),
            HandledFetchError::ApiCall { error } => error.message().to_string(),
        };

        open_responses_error_generate_result(
            &self.config.provider_options_name,
            message,
            request_body,
        )
    }
}

impl LanguageModel for OpenResponsesLanguageModel {
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

fn open_responses_request_body(
    model_id: &str,
    _provider_options_name: &str,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut warnings = Vec::new();
    let (input, instructions) = open_responses_input(&options.prompt)?;
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("input".to_string(), JsonValue::Array(input));

    if let Some(instructions) = instructions {
        body.insert("instructions".to_string(), JsonValue::String(instructions));
    }

    if let Some(max_output_tokens) = options.max_output_tokens {
        body.insert("max_output_tokens".to_string(), json!(max_output_tokens));
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

    if options.stop_sequences.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "stopSequences".to_string(),
            details: None,
        });
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

    Ok((JsonValue::Object(body), warnings))
}

fn open_responses_input(
    prompt: &[LanguageModelMessage],
) -> Result<(Vec<JsonValue>, Option<String>), String> {
    let mut input = Vec::new();
    let mut system_messages = Vec::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                system_messages.push(message.content.clone());
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
                        LanguageModelUserContentPart::File(_) => {
                            return Err(
                                "Open Responses file prompt parts are not implemented yet."
                                    .to_string(),
                            );
                        }
                    }
                }

                input.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": content
                }));
            }
            LanguageModelMessage::Assistant(message) => {
                let mut content = Vec::new();
                let mut tool_calls = Vec::new();

                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text) => {
                            content.push(json!({
                                "type": "output_text",
                                "text": text.text
                            }));
                        }
                        LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                            tool_calls.push(json!({
                                "type": "function_call",
                                "call_id": tool_call.tool_call_id,
                                "name": tool_call.tool_name,
                                "arguments": tool_call.input
                            }));
                        }
                        _ => {
                            return Err(
                                "Open Responses assistant prompt part is not implemented yet."
                                    .to_string(),
                            );
                        }
                    }
                }

                if !content.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": content
                    }));
                }

                input.extend(tool_calls);
            }
            LanguageModelMessage::Tool(_) => {
                return Err(
                    "Open Responses tool prompt messages are not implemented yet.".to_string(),
                );
            }
        }
    }

    let instructions = (!system_messages.is_empty()).then(|| system_messages.join("\n"));

    Ok((input, instructions))
}

fn open_responses_content(response: &JsonValue) -> (Vec<LanguageModelContent>, bool) {
    let mut content = Vec::new();
    let mut has_tool_calls = false;

    for part in response
        .get("output")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        match part.get("type").and_then(JsonValue::as_str) {
            Some("message") => {
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
                        content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
                    }
                }
            }
            Some("reasoning") => {
                for content_part in part
                    .get("content")
                    .or_else(|| part.get("summary"))
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = content_part.get("text").and_then(JsonValue::as_str) {
                        content.push(LanguageModelContent::Reasoning(
                            LanguageModelReasoning::new(text),
                        ));
                    }
                }
            }
            Some("function_call") => {
                has_tool_calls = true;
                let tool_call_id = part
                    .get("call_id")
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
                content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name,
                    input,
                )));
            }
            _ => {}
        }
    }

    (content, has_tool_calls)
}

fn map_open_responses_finish_reason(
    finish_reason: Option<&str>,
    has_tool_calls: bool,
) -> LanguageModelFinishReason {
    let unified = match finish_reason {
        None => {
            if has_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        Some("max_output_tokens") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        Some(_) => {
            if has_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Other
            }
        }
    };

    LanguageModelFinishReason {
        unified,
        raw: finish_reason.map(ToString::to_string),
    }
}

fn open_responses_usage(usage: Option<&JsonValue>) -> LanguageModelUsage {
    let input_tokens = usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(JsonValue::as_u64);
    let cached_input_tokens = usage
        .and_then(|usage| usage.get("input_tokens_details"))
        .and_then(|details| details.get("cached_tokens"))
        .and_then(JsonValue::as_u64);
    let output_tokens = usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(JsonValue::as_u64);
    let reasoning_tokens = usage
        .and_then(|usage| usage.get("output_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(JsonValue::as_u64);

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_tokens,
            no_cache: Some(
                input_tokens
                    .unwrap_or(0)
                    .saturating_sub(cached_input_tokens.unwrap_or(0)),
            ),
            cache_read: cached_input_tokens,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: output_tokens,
            text: Some(
                output_tokens
                    .unwrap_or(0)
                    .saturating_sub(reasoning_tokens.unwrap_or(0)),
            ),
            reasoning: reasoning_tokens,
        },
        raw: usage.and_then(JsonValue::as_object).cloned(),
    }
}

fn open_responses_provider_headers(settings: &OpenResponsesProviderSettings) -> Headers {
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

    let user_agent_suffix = settings
        .user_agent_suffix
        .clone()
        .unwrap_or_else(|| format!("ai-sdk/open-responses/{}", crate::VERSION));

    with_user_agent_suffix(Some(headers), [user_agent_suffix])
}

fn open_responses_error_message(error: &JsonValue) -> String {
    error
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| error.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Open Responses API error")
        .to_string()
}

fn open_responses_error_generate_result(
    provider_name: &str,
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    let message = message.into();
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("open-responses-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(open_responses_error_metadata(provider_name, message))
}

fn open_responses_error_stream_result(
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

fn open_responses_provider_metadata(provider_name: &str, response_id: &str) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert(
        "responseId".to_string(),
        JsonValue::String(response_id.to_string()),
    );
    metadata.insert(provider_name.to_string(), provider);
    metadata
}

fn open_responses_error_metadata(provider_name: &str, message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(provider_name.to_string(), provider);
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

fn default_open_responses_transport() -> OpenResponsesTransport {
    Arc::new(|request| Box::pin(ready(execute_open_responses_request(request))))
}

fn execute_open_responses_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Post => execute_open_responses_post_request(request),
        ProviderApiRequestMethod::Get => Err(FetchErrorInfo::new(
            "GET requests are not supported by the Open Responses transport",
        )),
    }
}

fn execute_open_responses_post_request(
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
                "multipart form data is not supported by the Open Responses transport",
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
        OpenResponsesProvider, OpenResponsesProviderSettings, OpenResponsesTransport,
        OpenResponsesTransportFuture, create_open_responses, map_open_responses_finish_reason,
    };
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{FinishReason, LanguageModel, LanguageModelStreamPart};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderMetadata};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[test]
    fn open_responses_provider_generates_text_with_request_and_response_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_open",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from Responses"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "input_tokens_details": {
                                "cached_tokens": 2
                            },
                            "output_tokens": 4,
                            "output_tokens_details": {
                                "reasoning_tokens": 1
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_open_responses".to_string(),
                )])))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "openai.responses");
        assert_eq!(model.model_id(), "gpt-4.1-mini");
        assert_eq!(result.text, "Hello from Responses");
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.input_tokens.no_cache, Some(3));
        assert_eq!(result.usage.input_tokens.cache_read, Some(2));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.text, Some(3));
        assert_eq!(result.usage.output_tokens.reasoning, Some(1));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("resp_open")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("openai")
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_open")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/responses");
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
                .is_some_and(|value| value.contains("ai-sdk/open-responses/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "gpt-4.1-mini",
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Say hello"
                            }
                        ]
                    }
                ],
                "max_output_tokens": 16,
                "temperature": 0.0
            }))
        );
    }

    #[test]
    fn open_responses_provider_reports_unsupported_embedding_and_image() {
        let provider = OpenResponsesProvider::from_settings(OpenResponsesProviderSettings::new(
            "openai",
            "https://api.openai.test/v1/responses",
        ));
        let embedding = match provider.embedding_model("embedding-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(embedding.model_type(), ModelType::EmbeddingModel);
        let image = match provider.image_model("image-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);

        let trait_model =
            Provider::language_model(&provider, "gpt-4.1-mini").expect("language model exists");
        assert_eq!(trait_model.provider(), "openai.responses");
    }

    #[test]
    fn open_responses_finish_reason_mapping_matches_upstream() {
        assert_eq!(
            map_open_responses_finish_reason(None, false).unified,
            FinishReason::Stop
        );
        assert_eq!(
            map_open_responses_finish_reason(None, true).unified,
            FinishReason::ToolCalls
        );
        assert_eq!(
            map_open_responses_finish_reason(Some("max_output_tokens"), false).unified,
            FinishReason::Length
        );
        assert_eq!(
            map_open_responses_finish_reason(Some("content_filter"), false).unified,
            FinishReason::ContentFilter
        );
        assert_eq!(
            map_open_responses_finish_reason(Some("unknown"), false).unified,
            FinishReason::Other
        );
        assert_eq!(
            map_open_responses_finish_reason(Some("unknown"), true).unified,
            FinishReason::ToolCalls
        );
    }

    #[test]
    fn open_responses_stream_returns_error_until_sse_is_ported() {
        let provider = OpenResponsesProvider::from_settings(OpenResponsesProviderSettings::new(
            "openai",
            "https://api.openai.test/v1/responses",
        ));
        let model = provider.language_model("gpt-4.1-mini");
        let stream_result = poll_ready(model.do_stream(
            crate::language_model::LanguageModelCallOptions::new(vec![
                crate::language_model::LanguageModelMessage::User(
                    crate::language_model::LanguageModelUserMessage::new(vec![
                        crate::language_model::LanguageModelUserContentPart::Text(
                            crate::language_model::LanguageModelTextPart::new("Say hello"),
                        ),
                    ]),
                ),
            ]),
        ));

        match stream_result.stream.as_slice() {
            [LanguageModelStreamPart::Error(error)] => {
                assert_eq!(
                    error.error.get("message").and_then(JsonValue::as_str),
                    Some("Open Responses streaming is not implemented yet.")
                );
            }
            parts => panic!("expected one error stream part, got {parts:?}"),
        }
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
