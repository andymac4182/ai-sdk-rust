use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::file_data::FileData;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue, NonNullJsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFilePart, LanguageModelFinishReason, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelProviderTool, LanguageModelRawStreamPart,
    LanguageModelReasoning, LanguageModelReasoningDelta, LanguageModelReasoningEnd,
    LanguageModelReasoningStart, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelStreamFinish, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice,
    LanguageModelToolContentPart, LanguageModelToolResult, LanguageModelToolResultContentPart,
    LanguageModelToolResultOutput, LanguageModelUsage, LanguageModelUserContentPart,
    OutputTokenUsage,
};
use crate::openai_compatible::{OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel};
use crate::provider::{
    ApiCallError, ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    SpecificationVersion,
};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, ParseJsonResult, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers, convert_to_base64,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, create_tool_name_mapping, get_top_level_media_type,
    post_json_to_api, resolve_full_media_type, with_user_agent_suffix,
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
                    OpenResponsesErrorContext::from_message(message),
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
                &options.tools,
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
        let (mut request_body, warnings) = match open_responses_request_body(
            &self.model_id,
            &self.config.provider_options_name,
            &options,
        ) {
            Ok(result) => result,
            Err(message) => {
                return open_responses_error_stream_result(
                    OpenResponsesErrorContext::from_message(message),
                    json!({ "model": self.model_id }),
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
            PostJsonToApiOptions::new(self.config.settings.url.clone(), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown());
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
                    open_responses_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => open_responses_stream_result_from_response(
                &self.config.provider_options_name,
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
        tools: &Option<Vec<LanguageModelTool>>,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let (content, has_tool_calls) = open_responses_content(&response, tools);
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
        open_responses_error_generate_result(
            &self.config.provider_options_name,
            OpenResponsesErrorContext::from_fetch_error(error),
            request_body,
        )
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        open_responses_error_stream_result(
            OpenResponsesErrorContext::from_fetch_error(error),
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
    provider_options_name: &str,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut warnings = Vec::new();
    let (input, instructions) = open_responses_input(&options.prompt, &mut warnings)?;
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

    let (tools, tool_choice) =
        open_responses_prepare_tools(&options.tools, &options.tool_choice, &mut warnings);
    if let Some(tools) = tools {
        body.insert("tools".to_string(), JsonValue::Array(tools));
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".to_string(), tool_choice);
    }

    if let Some(text_format) = open_responses_text_format(&options.response_format) {
        body.insert(
            "text".to_string(),
            json!({
                "format": text_format
            }),
        );
    }

    merge_open_responses_provider_options(
        provider_options_name,
        options.provider_options.as_ref(),
        &mut body,
        &mut warnings,
    );

    Ok((JsonValue::Object(body), warnings))
}

fn merge_open_responses_provider_options(
    provider_options_name: &str,
    provider_options: Option<&ProviderOptions>,
    body: &mut JsonObject,
    warnings: &mut Vec<Warning>,
) {
    let Some(provider_options) = provider_options else {
        return;
    };

    let raw_provider_options_name = provider_options_name
        .split('.')
        .next()
        .unwrap_or(provider_options_name)
        .trim();
    let camel_provider_options_name =
        open_responses_camel_case_provider_options_key(raw_provider_options_name);

    if let Some(options) = provider_options.get(raw_provider_options_name) {
        if camel_provider_options_name != raw_provider_options_name {
            warnings.push(Warning::Deprecated {
                setting: format!("providerOptions key '{raw_provider_options_name}'"),
                message: format!("Use '{camel_provider_options_name}' instead."),
            });
        }

        body.extend(options.clone());
    }

    if camel_provider_options_name != raw_provider_options_name
        && let Some(options) = provider_options.get(&camel_provider_options_name)
    {
        body.extend(options.clone());
    }

    merge_vercel_ai_gateway_open_responses_provider_options(
        raw_provider_options_name,
        provider_options,
        body,
    );
}

fn merge_vercel_ai_gateway_open_responses_provider_options(
    provider_options_name: &str,
    provider_options: &ProviderOptions,
    body: &mut JsonObject,
) {
    if provider_options_name != "vercel-ai-gateway" {
        return;
    }

    let Some(gateway_options) = provider_options.get("gateway") else {
        return;
    };

    let request_provider_options = body
        .entry("providerOptions".to_string())
        .or_insert_with(|| JsonValue::Object(JsonObject::new()));

    if let JsonValue::Object(request_provider_options) = request_provider_options {
        request_provider_options
            .entry("gateway".to_string())
            .or_insert_with(|| JsonValue::Object(gateway_options.clone()));
    }
}

fn open_responses_camel_case_provider_options_key(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = false;

    for character in value.chars() {
        if matches!(character, '-' | '_') {
            uppercase_next = true;
            continue;
        }

        if uppercase_next {
            output.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(character);
        }
    }

    output
}

fn open_responses_input(
    prompt: &[LanguageModelMessage],
    warnings: &mut Vec<Warning>,
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
                        LanguageModelUserContentPart::File(file) => {
                            content.push(open_responses_file_part(file)?);
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
            LanguageModelMessage::Tool(message) => {
                for part in &message.content {
                    match part {
                        LanguageModelToolContentPart::ToolResult(part) => {
                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": part.tool_call_id,
                                "output": open_responses_tool_result_output(
                                    &part.output,
                                    warnings
                                )
                            }));
                        }
                        LanguageModelToolContentPart::ToolApprovalResponse(_) => {
                            warnings.push(Warning::Unsupported {
                                feature: "toolApprovalResponse".to_string(),
                                details: Some(
                                    "Open Responses tool approval responses are not implemented yet."
                                        .to_string(),
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    let instructions = (!system_messages.is_empty()).then(|| system_messages.join("\n"));

    Ok((input, instructions))
}

fn open_responses_file_part(file: &LanguageModelFilePart) -> Result<JsonValue, String> {
    let top_level_media_type = get_top_level_media_type(&file.media_type);

    match &file.data {
        FileData::Reference { .. } => Err(
            "Open Responses file parts with provider references are not implemented yet."
                .to_string(),
        ),
        FileData::Text { .. } => {
            Err("Open Responses text file parts are not implemented yet.".to_string())
        }
        FileData::Url { url } => {
            if top_level_media_type == "image" {
                Ok(json!({
                    "type": "input_image",
                    "image_url": url.as_str()
                }))
            } else {
                Ok(json!({
                    "type": "input_file",
                    "file_url": url.as_str()
                }))
            }
        }
        FileData::Data { data } => {
            let full_media_type =
                resolve_full_media_type(file).map_err(|error| error.message().to_string())?;
            let data_uri = format!("data:{full_media_type};base64,{}", convert_to_base64(data));

            if top_level_media_type == "image" {
                Ok(json!({
                    "type": "input_image",
                    "image_url": data_uri
                }))
            } else {
                Ok(json!({
                    "type": "input_file",
                    "filename": file.filename.as_deref().unwrap_or("data"),
                    "file_data": data_uri
                }))
            }
        }
    }
}

fn open_responses_tool_result_output(
    output: &LanguageModelToolResultOutput,
    warnings: &mut Vec<Warning>,
) -> JsonValue {
    match output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => {
            JsonValue::String(value.clone())
        }
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => {
            JsonValue::String(value.to_string())
        }
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => JsonValue::String(
            reason
                .clone()
                .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        ),
        LanguageModelToolResultOutput::Content { value } => {
            let mut content = Vec::new();

            for part in value {
                match part {
                    LanguageModelToolResultContentPart::Text(text) => {
                        content.push(json!({
                            "type": "input_text",
                            "text": text.text
                        }));
                    }
                    LanguageModelToolResultContentPart::File(_) => {
                        warnings.push(Warning::Unsupported {
                            feature: "toolResultFileContent".to_string(),
                            details: Some(
                                "Open Responses tool result file content is not implemented yet."
                                    .to_string(),
                            ),
                        });
                    }
                    LanguageModelToolResultContentPart::Custom(_) => {
                        warnings.push(Warning::Unsupported {
                            feature: "toolResultCustomContent".to_string(),
                            details: Some(
                                "Open Responses tool result custom content is not implemented yet."
                                    .to_string(),
                            ),
                        });
                    }
                }
            }

            JsonValue::Array(content)
        }
    }
}

fn open_responses_prepare_tools(
    tools: &Option<Vec<LanguageModelTool>>,
    tool_choice: &Option<LanguageModelToolChoice>,
    warnings: &mut Vec<Warning>,
) -> (Option<Vec<JsonValue>>, Option<JsonValue>) {
    let provider_tool_names = open_responses_provider_tool_names();
    let tool_name_mapping = create_tool_name_mapping(tools.iter().flatten(), &provider_tool_names);
    let mut custom_provider_tool_names = BTreeSet::new();

    let prepared_tools = tools.as_ref().and_then(|tools| {
        let prepared_tools = tools
            .iter()
            .filter_map(|tool| match tool {
                LanguageModelTool::Function(tool) => {
                    let mut function = JsonObject::new();
                    function.insert(
                        "type".to_string(),
                        JsonValue::String("function".to_string()),
                    );
                    function.insert("name".to_string(), JsonValue::String(tool.name.clone()));

                    if let Some(description) = &tool.description {
                        function.insert(
                            "description".to_string(),
                            JsonValue::String(description.clone()),
                        );
                    }

                    function.insert(
                        "parameters".to_string(),
                        JsonValue::Object(tool.input_schema.clone()),
                    );

                    if let Some(strict) = tool.strict {
                        function.insert("strict".to_string(), JsonValue::Bool(strict));
                    }

                    if let Some(defer_loading) =
                        open_responses_function_tool_defer_loading(tool.provider_options.as_ref())
                    {
                        function.insert("defer_loading".to_string(), defer_loading);
                    }

                    Some(JsonValue::Object(function))
                }
                LanguageModelTool::Provider(tool) => open_responses_prepare_provider_tool(
                    tool,
                    warnings,
                    &mut custom_provider_tool_names,
                ),
            })
            .collect::<Vec<_>>();

        (!prepared_tools.is_empty()).then_some(prepared_tools)
    });

    let prepared_tool_choice = tool_choice.as_ref().map(|choice| match choice {
        LanguageModelToolChoice::Auto => JsonValue::String("auto".to_string()),
        LanguageModelToolChoice::None => JsonValue::String("none".to_string()),
        LanguageModelToolChoice::Required => JsonValue::String("required".to_string()),
        LanguageModelToolChoice::Tool { tool_name } => {
            let resolved_tool_name = tool_name_mapping.to_provider_tool_name(tool_name);

            if open_responses_hosted_tool_choice_type(&resolved_tool_name) {
                json!({ "type": resolved_tool_name })
            } else if custom_provider_tool_names.contains(&resolved_tool_name) {
                json!({
                    "type": "custom",
                    "name": resolved_tool_name
                })
            } else {
                json!({
                    "type": "function",
                    "name": resolved_tool_name
                })
            }
        }
    });

    (prepared_tools, prepared_tool_choice)
}

fn open_responses_provider_tool_names() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "openai.code_interpreter".to_string(),
            "code_interpreter".to_string(),
        ),
        ("openai.file_search".to_string(), "file_search".to_string()),
        (
            "openai.image_generation".to_string(),
            "image_generation".to_string(),
        ),
        ("openai.local_shell".to_string(), "local_shell".to_string()),
        ("openai.shell".to_string(), "shell".to_string()),
        ("openai.web_search".to_string(), "web_search".to_string()),
        (
            "openai.web_search_preview".to_string(),
            "web_search_preview".to_string(),
        ),
        ("openai.mcp".to_string(), "mcp".to_string()),
        ("openai.apply_patch".to_string(), "apply_patch".to_string()),
        ("openai.tool_search".to_string(), "tool_search".to_string()),
    ])
}

fn open_responses_function_tool_defer_loading(
    provider_options: Option<&ProviderOptions>,
) -> Option<JsonValue> {
    provider_options
        .and_then(|provider_options| provider_options.get("openai"))
        .and_then(|openai_options| openai_options.get("deferLoading"))
        .filter(|value| value.is_boolean())
        .cloned()
}

fn open_responses_prepare_provider_tool(
    tool: &LanguageModelProviderTool,
    warnings: &mut Vec<Warning>,
    custom_provider_tool_names: &mut BTreeSet<String>,
) -> Option<JsonValue> {
    let prepared = match tool.id.as_str() {
        "openai.file_search" => open_responses_file_search_tool(&tool.args),
        "openai.local_shell" => open_responses_tool_with_type("local_shell"),
        "openai.shell" => open_responses_shell_tool(&tool.args),
        "openai.apply_patch" => open_responses_tool_with_type("apply_patch"),
        "openai.web_search_preview" => open_responses_web_search_preview_tool(&tool.args),
        "openai.web_search" => open_responses_web_search_tool(&tool.args),
        "openai.code_interpreter" => open_responses_code_interpreter_tool(&tool.args),
        "openai.image_generation" => open_responses_image_generation_tool(&tool.args),
        "openai.mcp" => open_responses_mcp_tool(&tool.args),
        "openai.custom" => {
            custom_provider_tool_names.insert(tool.name.clone());
            open_responses_custom_tool(tool)
        }
        "openai.tool_search" => open_responses_tool_search_tool(&tool.args),
        _ => {
            warnings.push(Warning::Unsupported {
                feature: format!("provider-defined tool {}", tool.id),
                details: None,
            });
            return None;
        }
    };

    Some(JsonValue::Object(prepared))
}

fn open_responses_tool_with_type(tool_type: &str) -> JsonObject {
    let mut tool = JsonObject::new();
    tool.insert("type".to_string(), JsonValue::String(tool_type.to_string()));
    tool
}

fn open_responses_arg(args: &JsonObject, key: &str) -> Option<JsonValue> {
    args.get(key).filter(|value| !value.is_null()).cloned()
}

fn open_responses_insert_arg(
    target: &mut JsonObject,
    target_key: &str,
    args: &JsonObject,
    source_key: &str,
) {
    if let Some(value) = open_responses_arg(args, source_key) {
        target.insert(target_key.to_string(), value);
    }
}

fn open_responses_file_search_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("file_search");
    open_responses_insert_arg(&mut tool, "vector_store_ids", args, "vectorStoreIds");
    open_responses_insert_arg(&mut tool, "max_num_results", args, "maxNumResults");

    if let Some(ranking) = args.get("ranking").and_then(JsonValue::as_object) {
        let mut ranking_options = JsonObject::new();
        open_responses_insert_arg(&mut ranking_options, "ranker", ranking, "ranker");
        open_responses_insert_arg(
            &mut ranking_options,
            "score_threshold",
            ranking,
            "scoreThreshold",
        );

        if !ranking_options.is_empty() {
            tool.insert(
                "ranking_options".to_string(),
                JsonValue::Object(ranking_options),
            );
        }
    }

    open_responses_insert_arg(&mut tool, "filters", args, "filters");
    tool
}

fn open_responses_web_search_preview_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("web_search_preview");
    open_responses_insert_arg(&mut tool, "search_context_size", args, "searchContextSize");
    open_responses_insert_arg(&mut tool, "user_location", args, "userLocation");
    tool
}

fn open_responses_web_search_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("web_search");
    open_responses_insert_arg(&mut tool, "external_web_access", args, "externalWebAccess");

    if let Some(filters) = args.get("filters").and_then(JsonValue::as_object) {
        let mut mapped_filters = JsonObject::new();
        open_responses_insert_arg(
            &mut mapped_filters,
            "allowed_domains",
            filters,
            "allowedDomains",
        );

        if !mapped_filters.is_empty() {
            tool.insert("filters".to_string(), JsonValue::Object(mapped_filters));
        }
    }

    open_responses_insert_arg(&mut tool, "search_context_size", args, "searchContextSize");
    open_responses_insert_arg(&mut tool, "user_location", args, "userLocation");
    tool
}

fn open_responses_code_interpreter_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("code_interpreter");
    let container = match open_responses_arg(args, "container") {
        Some(JsonValue::String(container_id)) => JsonValue::String(container_id),
        Some(JsonValue::Object(container)) => {
            let mut mapped_container = JsonObject::new();
            mapped_container.insert("type".to_string(), JsonValue::String("auto".to_string()));
            open_responses_insert_arg(&mut mapped_container, "file_ids", &container, "fileIds");
            JsonValue::Object(mapped_container)
        }
        _ => json!({ "type": "auto" }),
    };

    tool.insert("container".to_string(), container);
    tool
}

fn open_responses_image_generation_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("image_generation");
    open_responses_insert_arg(&mut tool, "background", args, "background");
    open_responses_insert_arg(&mut tool, "input_fidelity", args, "inputFidelity");

    if let Some(mask) = args.get("inputImageMask").and_then(JsonValue::as_object) {
        let mut mapped_mask = JsonObject::new();
        open_responses_insert_arg(&mut mapped_mask, "file_id", mask, "fileId");
        open_responses_insert_arg(&mut mapped_mask, "image_url", mask, "imageUrl");

        if !mapped_mask.is_empty() {
            tool.insert(
                "input_image_mask".to_string(),
                JsonValue::Object(mapped_mask),
            );
        }
    }

    open_responses_insert_arg(&mut tool, "model", args, "model");
    open_responses_insert_arg(&mut tool, "moderation", args, "moderation");
    open_responses_insert_arg(&mut tool, "partial_images", args, "partialImages");
    open_responses_insert_arg(&mut tool, "quality", args, "quality");
    open_responses_insert_arg(&mut tool, "output_compression", args, "outputCompression");
    open_responses_insert_arg(&mut tool, "output_format", args, "outputFormat");
    open_responses_insert_arg(&mut tool, "size", args, "size");
    tool
}

fn open_responses_mcp_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("mcp");
    open_responses_insert_arg(&mut tool, "server_label", args, "serverLabel");

    if let Some(allowed_tools) = args.get("allowedTools") {
        let mapped_allowed_tools = if let Some(filter) = allowed_tools.as_object() {
            let mut mapped_filter = JsonObject::new();
            open_responses_insert_arg(&mut mapped_filter, "read_only", filter, "readOnly");
            open_responses_insert_arg(&mut mapped_filter, "tool_names", filter, "toolNames");
            JsonValue::Object(mapped_filter)
        } else {
            allowed_tools.clone()
        };
        tool.insert("allowed_tools".to_string(), mapped_allowed_tools);
    }

    open_responses_insert_arg(&mut tool, "authorization", args, "authorization");
    open_responses_insert_arg(&mut tool, "connector_id", args, "connectorId");
    open_responses_insert_arg(&mut tool, "headers", args, "headers");

    let require_approval = args
        .get("requireApproval")
        .and_then(open_responses_mcp_require_approval)
        .unwrap_or_else(|| JsonValue::String("never".to_string()));
    tool.insert("require_approval".to_string(), require_approval);

    open_responses_insert_arg(&mut tool, "server_description", args, "serverDescription");
    open_responses_insert_arg(&mut tool, "server_url", args, "serverUrl");
    tool
}

fn open_responses_mcp_require_approval(require_approval: &JsonValue) -> Option<JsonValue> {
    if matches!(require_approval.as_str(), Some("always") | Some("never")) {
        return Some(require_approval.clone());
    }

    let never = require_approval
        .as_object()
        .and_then(|approval| approval.get("never"))
        .and_then(JsonValue::as_object)?;
    let mut never_filter = JsonObject::new();
    open_responses_insert_arg(&mut never_filter, "tool_names", never, "toolNames");
    Some(json!({ "never": never_filter }))
}

fn open_responses_custom_tool(tool: &LanguageModelProviderTool) -> JsonObject {
    let mut prepared = open_responses_tool_with_type("custom");
    prepared.insert("name".to_string(), JsonValue::String(tool.name.clone()));
    open_responses_insert_arg(&mut prepared, "description", &tool.args, "description");
    open_responses_insert_arg(&mut prepared, "format", &tool.args, "format");
    prepared
}

fn open_responses_shell_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("shell");

    if let Some(environment) = args.get("environment").and_then(JsonValue::as_object) {
        let mapped_environment = open_responses_shell_environment(environment);
        tool.insert(
            "environment".to_string(),
            JsonValue::Object(mapped_environment),
        );
    }

    tool
}

fn open_responses_shell_environment(environment: &JsonObject) -> JsonObject {
    match environment.get("type").and_then(JsonValue::as_str) {
        Some("containerReference") => {
            let mut mapped_environment = open_responses_tool_with_type("container_reference");
            open_responses_insert_arg(
                &mut mapped_environment,
                "container_id",
                environment,
                "containerId",
            );
            mapped_environment
        }
        Some("containerAuto") => {
            let mut mapped_environment = open_responses_tool_with_type("container_auto");
            open_responses_insert_arg(&mut mapped_environment, "file_ids", environment, "fileIds");
            open_responses_insert_arg(
                &mut mapped_environment,
                "memory_limit",
                environment,
                "memoryLimit",
            );

            if let Some(network_policy) = environment
                .get("networkPolicy")
                .and_then(JsonValue::as_object)
            {
                mapped_environment.insert(
                    "network_policy".to_string(),
                    JsonValue::Object(open_responses_shell_network_policy(network_policy)),
                );
            }

            if let Some(skills) = environment.get("skills").and_then(JsonValue::as_array) {
                mapped_environment.insert(
                    "skills".to_string(),
                    JsonValue::Array(
                        skills
                            .iter()
                            .filter_map(open_responses_shell_skill)
                            .map(JsonValue::Object)
                            .collect(),
                    ),
                );
            }

            mapped_environment
        }
        _ => {
            let mut mapped_environment = open_responses_tool_with_type("local");
            open_responses_insert_arg(&mut mapped_environment, "skills", environment, "skills");
            mapped_environment
        }
    }
}

fn open_responses_shell_network_policy(network_policy: &JsonObject) -> JsonObject {
    if matches!(
        network_policy.get("type").and_then(JsonValue::as_str),
        Some("disabled")
    ) {
        return open_responses_tool_with_type("disabled");
    }

    let mut mapped_policy = open_responses_tool_with_type("allowlist");
    open_responses_insert_arg(
        &mut mapped_policy,
        "allowed_domains",
        network_policy,
        "allowedDomains",
    );
    open_responses_insert_arg(
        &mut mapped_policy,
        "domain_secrets",
        network_policy,
        "domainSecrets",
    );
    mapped_policy
}

fn open_responses_shell_skill(skill: &JsonValue) -> Option<JsonObject> {
    let skill = skill.as_object()?;

    if matches!(
        skill.get("type").and_then(JsonValue::as_str),
        Some("skillReference")
    ) {
        let mut mapped_skill = open_responses_tool_with_type("skill_reference");
        let skill_id = skill
            .get("providerReference")
            .and_then(JsonValue::as_object)
            .and_then(|reference| reference.get("openai"))
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        mapped_skill.insert(
            "skill_id".to_string(),
            JsonValue::String(skill_id.to_string()),
        );
        mapped_skill.insert(
            "version".to_string(),
            open_responses_arg(skill, "version")
                .unwrap_or_else(|| JsonValue::String("latest".to_string())),
        );
        return Some(mapped_skill);
    }

    let mut mapped_skill = open_responses_tool_with_type("inline");
    open_responses_insert_arg(&mut mapped_skill, "name", skill, "name");
    open_responses_insert_arg(&mut mapped_skill, "description", skill, "description");

    if let Some(source) = skill.get("source").and_then(JsonValue::as_object) {
        let mut mapped_source = open_responses_tool_with_type("base64");
        open_responses_insert_arg(&mut mapped_source, "media_type", source, "mediaType");
        open_responses_insert_arg(&mut mapped_source, "data", source, "data");
        mapped_skill.insert("source".to_string(), JsonValue::Object(mapped_source));
    }

    Some(mapped_skill)
}

fn open_responses_tool_search_tool(args: &JsonObject) -> JsonObject {
    let mut tool = open_responses_tool_with_type("tool_search");
    open_responses_insert_arg(&mut tool, "execution", args, "execution");
    open_responses_insert_arg(&mut tool, "description", args, "description");
    open_responses_insert_arg(&mut tool, "parameters", args, "parameters");
    tool
}

fn open_responses_hosted_tool_choice_type(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "code_interpreter"
            | "file_search"
            | "image_generation"
            | "web_search_preview"
            | "web_search"
            | "mcp"
            | "apply_patch"
    )
}

fn open_responses_text_format(
    response_format: &Option<LanguageModelResponseFormat>,
) -> Option<JsonValue> {
    let Some(LanguageModelResponseFormat::Json {
        schema,
        name,
        description,
    }) = response_format
    else {
        return None;
    };

    let mut format = JsonObject::new();
    format.insert(
        "type".to_string(),
        JsonValue::String("json_schema".to_string()),
    );

    if let Some(schema) = schema {
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
        format.insert("strict".to_string(), JsonValue::Bool(true));
    }

    Some(JsonValue::Object(format))
}

fn open_responses_content(
    response: &JsonValue,
    tools: &Option<Vec<LanguageModelTool>>,
) -> (Vec<LanguageModelContent>, bool) {
    let mut content = Vec::new();
    let mut has_tool_calls = false;
    let provider_tool_names = open_responses_provider_tool_names();
    let tool_name_mapping = create_tool_name_mapping(tools.iter().flatten(), &provider_tool_names);
    let web_search_tool_name = open_responses_web_search_response_tool_name(tools);

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
            Some("web_search_call") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = tool_name_mapping.to_custom_tool_name(&web_search_tool_name);

                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(tool_call_id, tool_name.clone(), "{}")
                        .with_provider_executed(true),
                ));
                open_responses_push_tool_result(
                    &mut content,
                    tool_call_id,
                    &tool_name,
                    open_responses_web_search_output(part.get("action")),
                );
            }
            Some("file_search_call") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = tool_name_mapping.to_custom_tool_name("file_search");

                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(tool_call_id, tool_name.clone(), "{}")
                        .with_provider_executed(true),
                ));
                open_responses_push_tool_result(
                    &mut content,
                    tool_call_id,
                    &tool_name,
                    open_responses_file_search_output(part),
                );
            }
            Some("code_interpreter_call") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = tool_name_mapping.to_custom_tool_name("code_interpreter");
                let input = json!({
                    "code": part.get("code").cloned().unwrap_or(JsonValue::Null),
                    "containerId": part.get("container_id").cloned().unwrap_or(JsonValue::Null)
                })
                .to_string();

                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(tool_call_id, tool_name.clone(), input)
                        .with_provider_executed(true),
                ));
                open_responses_push_tool_result(
                    &mut content,
                    tool_call_id,
                    &tool_name,
                    json!({
                        "outputs": part.get("outputs").cloned().unwrap_or(JsonValue::Null)
                    }),
                );
            }
            Some("image_generation_call") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = tool_name_mapping.to_custom_tool_name("image_generation");

                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(tool_call_id, tool_name.clone(), "{}")
                        .with_provider_executed(true),
                ));
                open_responses_push_tool_result(
                    &mut content,
                    tool_call_id,
                    &tool_name,
                    json!({
                        "result": part.get("result").cloned().unwrap_or(JsonValue::Null)
                    }),
                );
            }
            _ => {}
        }
    }

    (content, has_tool_calls)
}

fn open_responses_web_search_response_tool_name(tools: &Option<Vec<LanguageModelTool>>) -> String {
    tools
        .iter()
        .flatten()
        .find_map(|tool| {
            let LanguageModelTool::Provider(tool) = tool else {
                return None;
            };

            match tool.id.as_str() {
                "openai.web_search" => Some("web_search".to_string()),
                "openai.web_search_preview" => Some("web_search_preview".to_string()),
                _ => None,
            }
        })
        .unwrap_or_else(|| "web_search".to_string())
}

fn open_responses_web_search_output(action: Option<&JsonValue>) -> JsonValue {
    let Some(action) = action.and_then(JsonValue::as_object) else {
        return json!({});
    };

    match action.get("type").and_then(JsonValue::as_str) {
        Some("search") => {
            let mut mapped_action = open_responses_tool_with_type("search");
            open_responses_insert_arg(&mut mapped_action, "query", action, "query");

            let mut output = JsonObject::new();
            output.insert("action".to_string(), JsonValue::Object(mapped_action));
            open_responses_insert_arg(&mut output, "sources", action, "sources");
            JsonValue::Object(output)
        }
        Some("open_page") => json!({
            "action": {
                "type": "openPage",
                "url": action.get("url").cloned().unwrap_or(JsonValue::Null)
            }
        }),
        Some("find_in_page") => json!({
            "action": {
                "type": "findInPage",
                "url": action.get("url").cloned().unwrap_or(JsonValue::Null),
                "pattern": action.get("pattern").cloned().unwrap_or(JsonValue::Null)
            }
        }),
        _ => json!({}),
    }
}

fn open_responses_file_search_output(part: &JsonValue) -> JsonValue {
    json!({
        "queries": part.get("queries").cloned().unwrap_or_else(|| JsonValue::Array(Vec::new())),
        "results": part
            .get("results")
            .and_then(JsonValue::as_array)
            .map(|results| {
                JsonValue::Array(
                    results
                        .iter()
                        .map(open_responses_file_search_result)
                        .collect(),
                )
            })
            .unwrap_or(JsonValue::Null)
    })
}

fn open_responses_file_search_result(result: &JsonValue) -> JsonValue {
    json!({
        "attributes": result.get("attributes").cloned().unwrap_or_else(|| json!({})),
        "fileId": result.get("file_id").cloned().unwrap_or(JsonValue::Null),
        "filename": result.get("filename").cloned().unwrap_or(JsonValue::Null),
        "score": result.get("score").cloned().unwrap_or(JsonValue::Null),
        "text": result.get("text").cloned().unwrap_or(JsonValue::Null)
    })
}

fn open_responses_push_tool_result(
    content: &mut Vec<LanguageModelContent>,
    tool_call_id: &str,
    tool_name: &str,
    result: JsonValue,
) {
    if let Ok(result) = NonNullJsonValue::new(result) {
        content.push(LanguageModelContent::ToolResult(
            LanguageModelToolResult::new(tool_call_id, tool_name, result),
        ));
    }
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
        Some("max_output_tokens" | "max_tokens") => FinishReason::Length,
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

fn open_responses_stream_result_from_response(
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
    let mut usage = LanguageModelUsage::default();
    let mut response_id = None::<String>;
    let mut emitted_response_metadata = false;
    let mut has_tool_calls = false;
    let mut emitted_tool_calls = BTreeSet::<String>::new();
    let mut text_buffers = BTreeMap::<String, String>::new();
    let mut active_text = BTreeSet::<String>::new();
    let mut ended_text = BTreeSet::<String>::new();
    let mut reasoning_buffers = BTreeMap::<String, String>::new();
    let mut active_reasoning = BTreeSet::<String>::new();
    let mut ended_reasoning = BTreeSet::<String>::new();
    let mut pending_tool_calls = BTreeMap::<String, PendingOpenResponsesToolCall>::new();

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
                        raw: Some("open-responses-error".to_string()),
                    };
                    stream.push(open_responses_stream_event_error(
                        &value,
                        Some(&raw_value.to_string()),
                    ));
                    continue;
                }

                if let Some(response) = open_responses_event_response(&value) {
                    open_responses_push_response_metadata(
                        &mut stream,
                        &mut response_id,
                        &mut emitted_response_metadata,
                        response,
                    );
                }

                match event_type {
                    Some("response.output_text.delta") => {
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                            && !delta.is_empty()
                        {
                            let id = open_responses_stream_block_id("txt", &value);
                            open_responses_push_text_delta(
                                &mut stream,
                                &mut text_buffers,
                                &mut active_text,
                                &ended_text,
                                &id,
                                delta,
                            );
                        }
                    }
                    Some("response.output_text.done") => {
                        let id = open_responses_stream_block_id("txt", &value);
                        let text = value.get("text").and_then(JsonValue::as_str);
                        open_responses_finish_text_block(
                            &mut stream,
                            &mut text_buffers,
                            &mut active_text,
                            &mut ended_text,
                            &id,
                            text,
                        );
                    }
                    Some("response.reasoning_summary_text.delta")
                    | Some("response.reasoning_text.delta") => {
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                            && !delta.is_empty()
                        {
                            let id = open_responses_stream_block_id("reasoning", &value);
                            open_responses_push_reasoning_delta(
                                &mut stream,
                                &mut reasoning_buffers,
                                &mut active_reasoning,
                                &ended_reasoning,
                                &id,
                                delta,
                            );
                        }
                    }
                    Some("response.reasoning_summary_text.done")
                    | Some("response.reasoning_text.done") => {
                        let id = open_responses_stream_block_id("reasoning", &value);
                        let text = value.get("text").and_then(JsonValue::as_str);
                        open_responses_finish_reasoning_block(
                            &mut stream,
                            &mut reasoning_buffers,
                            &mut active_reasoning,
                            &mut ended_reasoning,
                            &id,
                            text,
                        );
                    }
                    Some("response.content_part.done") => {
                        let part = value.get("part");
                        let part_type = part
                            .and_then(|part| part.get("type"))
                            .and_then(JsonValue::as_str);
                        let text = part
                            .and_then(|part| part.get("text"))
                            .and_then(JsonValue::as_str);

                        if matches!(part_type, Some("output_text")) {
                            let id = open_responses_stream_block_id("txt", &value);
                            open_responses_finish_text_block(
                                &mut stream,
                                &mut text_buffers,
                                &mut active_text,
                                &mut ended_text,
                                &id,
                                text,
                            );
                        } else if open_responses_is_reasoning_text_part(part_type) {
                            let id = open_responses_stream_block_id("reasoning", &value);
                            open_responses_finish_reasoning_block(
                                &mut stream,
                                &mut reasoning_buffers,
                                &mut active_reasoning,
                                &mut ended_reasoning,
                                &id,
                                text,
                            );
                        }
                    }
                    Some("response.output_item.added") => {
                        if let Some(item) = value.get("item") {
                            open_responses_record_pending_tool_call(&mut pending_tool_calls, item);
                        }
                    }
                    Some("response.function_call_arguments.delta") => {
                        open_responses_append_pending_tool_call_arguments(
                            &mut pending_tool_calls,
                            &value,
                        );
                    }
                    Some("response.function_call_arguments.done") => {
                        open_responses_finish_pending_tool_call_arguments(
                            &mut pending_tool_calls,
                            &value,
                        );
                    }
                    Some("response.output_item.done") => {
                        if let Some(item) = value.get("item")
                            && open_responses_push_tool_call_from_item(
                                &mut stream,
                                &mut emitted_tool_calls,
                                &mut pending_tool_calls,
                                item,
                            )
                        {
                            has_tool_calls = true;
                        }
                    }
                    Some("response.completed") => {
                        if let Some(response) = open_responses_event_response(&value) {
                            usage = open_responses_usage(response.get("usage"));
                            has_tool_calls |= open_responses_push_tool_calls_from_response(
                                &mut stream,
                                &mut emitted_tool_calls,
                                &mut pending_tool_calls,
                                response,
                            );
                            finish_reason = map_open_responses_finish_reason(
                                response
                                    .get("incomplete_details")
                                    .and_then(|details| details.get("reason"))
                                    .and_then(JsonValue::as_str),
                                has_tool_calls,
                            );
                        }
                    }
                    Some("response.incomplete") => {
                        if let Some(response) = open_responses_event_response(&value) {
                            usage = open_responses_usage(response.get("usage"));
                            has_tool_calls |= open_responses_response_has_tool_calls(response);
                            finish_reason = map_open_responses_finish_reason(
                                response
                                    .get("incomplete_details")
                                    .and_then(|details| details.get("reason"))
                                    .and_then(JsonValue::as_str),
                                has_tool_calls,
                            );
                        }
                    }
                    Some("response.failed") => {
                        finish_reason = LanguageModelFinishReason {
                            unified: FinishReason::Error,
                            raw: Some("open-responses-error".to_string()),
                        };
                        stream.push(open_responses_stream_event_error(
                            &value,
                            Some(&raw_value.to_string()),
                        ));
                    }
                    _ => {}
                }
            }
            ParseJsonResult::Failure { error, raw_value } => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("open-responses-parse-error".to_string()),
                };
                stream.push(open_responses_stream_error(
                    error.to_string(),
                    raw_value.as_ref().map(JsonValue::to_string).as_deref(),
                ));
            }
        }
    }

    for id in active_reasoning.clone() {
        stream.push(LanguageModelStreamPart::ReasoningEnd(
            LanguageModelReasoningEnd::new(id),
        ));
    }

    for id in active_text.clone() {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            id,
        )));
    }

    let mut finish = LanguageModelStreamFinish::new(usage, finish_reason);
    if let Some(response_id) = response_id {
        finish = finish.with_provider_metadata(open_responses_provider_metadata(
            provider_name,
            &response_id,
        ));
    }
    stream.push(LanguageModelStreamPart::Finish(finish));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(open_responses_stream_response_with_headers(headers));
    }

    result
}

fn open_responses_event_response(value: &JsonValue) -> Option<&JsonValue> {
    value
        .get("response")
        .or_else(|| value.get("id").and_then(JsonValue::as_str).map(|_| value))
}

fn open_responses_push_response_metadata(
    stream: &mut Vec<LanguageModelStreamPart>,
    response_id: &mut Option<String>,
    emitted_response_metadata: &mut bool,
    response: &JsonValue,
) {
    if let Some(id) = response.get("id").and_then(JsonValue::as_str) {
        *response_id = Some(id.to_string());
    }

    if *emitted_response_metadata {
        return;
    }

    if let Some(metadata) = open_responses_stream_response_metadata(response) {
        stream.push(LanguageModelStreamPart::ResponseMetadata(metadata));
        *emitted_response_metadata = true;
    }
}

fn open_responses_stream_response_metadata(
    response: &JsonValue,
) -> Option<LanguageModelStreamResponseMetadata> {
    let mut metadata = LanguageModelStreamResponseMetadata::new();
    let mut has_metadata = false;

    if let Some(id) = response.get("id").and_then(JsonValue::as_str) {
        metadata = metadata.with_id(id);
        has_metadata = true;
    }

    if let Some(timestamp) = response
        .get("created_at")
        .and_then(JsonValue::as_i64)
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
    {
        metadata = metadata.with_timestamp(timestamp);
        has_metadata = true;
    }

    if let Some(model_id) = response.get("model").and_then(JsonValue::as_str) {
        metadata = metadata.with_model_id(model_id);
        has_metadata = true;
    }

    has_metadata.then_some(metadata)
}

fn open_responses_stream_block_id(prefix: &str, value: &JsonValue) -> String {
    let mut parts = vec![prefix.to_string()];

    if let Some(item_id) = value
        .get("item_id")
        .or_else(|| value.get("item").and_then(|item| item.get("id")))
        .and_then(JsonValue::as_str)
    {
        parts.push(item_id.to_string());
    }

    if let Some(output_index) = open_responses_json_index(value.get("output_index")) {
        parts.push(output_index);
    }

    if let Some(content_index) = open_responses_json_index(value.get("content_index")) {
        parts.push(content_index);
    }

    if parts.len() == 1 {
        parts.push("0".to_string());
    }

    parts.join("-")
}

fn open_responses_json_index(value: Option<&JsonValue>) -> Option<String> {
    value
        .and_then(JsonValue::as_u64)
        .map(|value| value.to_string())
        .or_else(|| {
            value
                .and_then(JsonValue::as_i64)
                .map(|value| value.to_string())
        })
}

fn open_responses_push_text_delta(
    stream: &mut Vec<LanguageModelStreamPart>,
    text_buffers: &mut BTreeMap<String, String>,
    active_text: &mut BTreeSet<String>,
    ended_text: &BTreeSet<String>,
    id: &str,
    delta: &str,
) {
    if ended_text.contains(id) {
        return;
    }

    if active_text.insert(id.to_string()) {
        stream.push(LanguageModelStreamPart::TextStart(
            LanguageModelTextStart::new(id),
        ));
    }

    text_buffers
        .entry(id.to_string())
        .or_default()
        .push_str(delta);
    stream.push(LanguageModelStreamPart::TextDelta(
        LanguageModelTextDelta::new(id, delta),
    ));
}

fn open_responses_finish_text_block(
    stream: &mut Vec<LanguageModelStreamPart>,
    text_buffers: &mut BTreeMap<String, String>,
    active_text: &mut BTreeSet<String>,
    ended_text: &mut BTreeSet<String>,
    id: &str,
    final_text: Option<&str>,
) {
    if ended_text.contains(id) {
        return;
    }

    let buffered = text_buffers.remove(id).unwrap_or_default();
    let emitted_final_text = buffered.is_empty() && final_text.is_some_and(|text| !text.is_empty());
    if emitted_final_text && let Some(text) = final_text {
        open_responses_push_text_delta(stream, text_buffers, active_text, ended_text, id, text);
        text_buffers.remove(id);
    }

    if active_text.remove(id) || !buffered.is_empty() || emitted_final_text {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            id,
        )));
        ended_text.insert(id.to_string());
    }
}

fn open_responses_push_reasoning_delta(
    stream: &mut Vec<LanguageModelStreamPart>,
    reasoning_buffers: &mut BTreeMap<String, String>,
    active_reasoning: &mut BTreeSet<String>,
    ended_reasoning: &BTreeSet<String>,
    id: &str,
    delta: &str,
) {
    if ended_reasoning.contains(id) {
        return;
    }

    if active_reasoning.insert(id.to_string()) {
        stream.push(LanguageModelStreamPart::ReasoningStart(
            LanguageModelReasoningStart::new(id),
        ));
    }

    reasoning_buffers
        .entry(id.to_string())
        .or_default()
        .push_str(delta);
    stream.push(LanguageModelStreamPart::ReasoningDelta(
        LanguageModelReasoningDelta::new(id, delta),
    ));
}

fn open_responses_finish_reasoning_block(
    stream: &mut Vec<LanguageModelStreamPart>,
    reasoning_buffers: &mut BTreeMap<String, String>,
    active_reasoning: &mut BTreeSet<String>,
    ended_reasoning: &mut BTreeSet<String>,
    id: &str,
    final_text: Option<&str>,
) {
    if ended_reasoning.contains(id) {
        return;
    }

    let buffered = reasoning_buffers.remove(id).unwrap_or_default();
    let emitted_final_text = buffered.is_empty() && final_text.is_some_and(|text| !text.is_empty());
    if emitted_final_text && let Some(text) = final_text {
        open_responses_push_reasoning_delta(
            stream,
            reasoning_buffers,
            active_reasoning,
            ended_reasoning,
            id,
            text,
        );
        reasoning_buffers.remove(id);
    }

    if active_reasoning.remove(id) || !buffered.is_empty() || emitted_final_text {
        stream.push(LanguageModelStreamPart::ReasoningEnd(
            LanguageModelReasoningEnd::new(id),
        ));
        ended_reasoning.insert(id.to_string());
    }
}

fn open_responses_is_reasoning_text_part(part_type: Option<&str>) -> bool {
    part_type.is_some_and(|part_type| {
        matches!(
            part_type,
            "reasoning_text" | "reasoning_summary_text" | "summary_text"
        ) || (part_type.contains("reasoning") && part_type.contains("text"))
    })
}

fn open_responses_push_tool_calls_from_response(
    stream: &mut Vec<LanguageModelStreamPart>,
    emitted_tool_calls: &mut BTreeSet<String>,
    pending_tool_calls: &mut BTreeMap<String, PendingOpenResponsesToolCall>,
    response: &JsonValue,
) -> bool {
    let mut has_tool_calls = false;

    for item in response
        .get("output")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        has_tool_calls |= open_responses_push_tool_call_from_item(
            stream,
            emitted_tool_calls,
            pending_tool_calls,
            item,
        );
    }

    has_tool_calls
}

fn open_responses_response_has_tool_calls(response: &JsonValue) -> bool {
    response
        .get("output")
        .and_then(JsonValue::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                matches!(
                    item.get("type").and_then(JsonValue::as_str),
                    Some("function_call")
                )
            })
        })
}

fn open_responses_push_tool_call_from_item(
    stream: &mut Vec<LanguageModelStreamPart>,
    emitted_tool_calls: &mut BTreeSet<String>,
    pending_tool_calls: &mut BTreeMap<String, PendingOpenResponsesToolCall>,
    item: &JsonValue,
) -> bool {
    if !matches!(
        item.get("type").and_then(JsonValue::as_str),
        Some("function_call")
    ) {
        return false;
    }

    let item_id = item.get("id").and_then(JsonValue::as_str);
    let pending = item_id.and_then(|item_id| pending_tool_calls.remove(item_id));
    let tool_call_id = item
        .get("call_id")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            pending
                .as_ref()
                .and_then(|pending| pending.tool_call_id.clone())
        })
        .or_else(|| item_id.map(ToString::to_string))
        .unwrap_or_default();
    let tool_name = item
        .get("name")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            pending
                .as_ref()
                .and_then(|pending| pending.tool_name.clone())
        })
        .unwrap_or_default();
    let input = item
        .get("arguments")
        .and_then(JsonValue::as_str)
        .filter(|arguments| !arguments.is_empty())
        .map(ToString::to_string)
        .or_else(|| pending.and_then(|pending| pending.arguments))
        .unwrap_or_else(|| "{}".to_string());
    let dedupe_key = if tool_call_id.is_empty() {
        format!("{}:{input}", tool_name)
    } else {
        tool_call_id.clone()
    };

    if !emitted_tool_calls.insert(dedupe_key) {
        return true;
    }

    stream.push(LanguageModelStreamPart::ToolCall(
        LanguageModelToolCall::new(tool_call_id, tool_name, input),
    ));
    true
}

#[derive(Clone, Debug, Default)]
struct PendingOpenResponsesToolCall {
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    arguments: Option<String>,
}

fn open_responses_record_pending_tool_call(
    pending_tool_calls: &mut BTreeMap<String, PendingOpenResponsesToolCall>,
    item: &JsonValue,
) {
    if !matches!(
        item.get("type").and_then(JsonValue::as_str),
        Some("function_call")
    ) {
        return;
    }

    let Some(item_id) = item.get("id").and_then(JsonValue::as_str) else {
        return;
    };

    let pending = pending_tool_calls.entry(item_id.to_string()).or_default();
    if let Some(tool_name) = item.get("name").and_then(JsonValue::as_str) {
        pending.tool_name = Some(tool_name.to_string());
    }
    if let Some(tool_call_id) = item.get("call_id").and_then(JsonValue::as_str) {
        pending.tool_call_id = Some(tool_call_id.to_string());
    }
    if let Some(arguments) = item.get("arguments").and_then(JsonValue::as_str)
        && !arguments.is_empty()
    {
        pending.arguments = Some(arguments.to_string());
    }
}

fn open_responses_append_pending_tool_call_arguments(
    pending_tool_calls: &mut BTreeMap<String, PendingOpenResponsesToolCall>,
    value: &JsonValue,
) {
    let Some(item_id) = value.get("item_id").and_then(JsonValue::as_str) else {
        return;
    };
    let Some(delta) = value.get("delta").and_then(JsonValue::as_str) else {
        return;
    };

    pending_tool_calls
        .entry(item_id.to_string())
        .or_default()
        .arguments
        .get_or_insert_with(String::new)
        .push_str(delta);
}

fn open_responses_finish_pending_tool_call_arguments(
    pending_tool_calls: &mut BTreeMap<String, PendingOpenResponsesToolCall>,
    value: &JsonValue,
) {
    let Some(item_id) = value.get("item_id").and_then(JsonValue::as_str) else {
        return;
    };
    let Some(arguments) = value.get("arguments").and_then(JsonValue::as_str) else {
        return;
    };

    pending_tool_calls
        .entry(item_id.to_string())
        .or_default()
        .arguments = Some(arguments.to_string());
}

#[derive(Clone, Debug)]
struct OpenResponsesErrorContext {
    message: String,
    response_headers: Option<Headers>,
    raw_body: Option<String>,
    status_code: Option<u16>,
    is_retryable: Option<bool>,
    data: Option<JsonValue>,
}

impl OpenResponsesErrorContext {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            response_headers: None,
            raw_body: None,
            status_code: None,
            is_retryable: None,
            data: None,
        }
    }

    fn from_fetch_error(error: HandledFetchError) -> Self {
        match error {
            HandledFetchError::Original { error } => {
                Self::from_message(error.message().to_string())
            }
            HandledFetchError::ApiCall { error } => Self::from_api_call(error.as_ref()),
        }
    }

    fn from_api_call(error: &ApiCallError) -> Self {
        let raw_body = error.response_body().map(String::from);
        let data = error
            .data()
            .cloned()
            .or_else(|| raw_body.as_deref().and_then(open_responses_parse_json_body));

        Self {
            message: error.message().to_string(),
            response_headers: error.response_headers().cloned(),
            raw_body,
            status_code: error.status_code(),
            is_retryable: Some(error.is_retryable()),
            data,
        }
    }
}

fn open_responses_parse_json_body(body: &str) -> Option<JsonValue> {
    serde_json::from_str::<JsonValue>(body).ok()
}

fn open_responses_response_body(raw_body: Option<&str>, fallback: &JsonValue) -> JsonValue {
    raw_body
        .and_then(open_responses_parse_json_body)
        .or_else(|| raw_body.map(|body| JsonValue::String(body.to_string())))
        .unwrap_or_else(|| fallback.clone())
}

fn open_responses_stream_error(
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

fn open_responses_stream_error_from_context(
    context: &OpenResponsesErrorContext,
) -> LanguageModelStreamPart {
    let mut error = open_responses_error_object(context);

    if let Some(raw_body) = &context.raw_body {
        error.insert("body".to_string(), JsonValue::String(raw_body.clone()));
    }

    LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(JsonValue::Object(error)))
}

fn open_responses_stream_event_error(
    value: &JsonValue,
    raw_body: Option<&str>,
) -> LanguageModelStreamPart {
    let mut error = value.as_object().cloned().unwrap_or_default();

    error
        .entry("message".to_string())
        .or_insert_with(|| JsonValue::String(open_responses_error_message(value)));

    if let Some(raw_body) = raw_body {
        error
            .entry("body".to_string())
            .or_insert_with(|| JsonValue::String(raw_body.to_string()));
    }

    LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(JsonValue::Object(error)))
}

fn open_responses_stream_response_with_headers(
    headers: Headers,
) -> LanguageModelStreamResultResponse {
    let mut response = LanguageModelStreamResultResponse::new();
    for (name, value) in headers {
        response = response.with_header(name, value);
    }
    response
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
    context: OpenResponsesErrorContext,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    let mut result = LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("open-responses-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(open_responses_error_metadata(provider_name, &context));

    if context.response_headers.is_some() || context.raw_body.is_some() {
        let mut response = LanguageModelResponse::new();

        if let Some(body) = context
            .raw_body
            .as_deref()
            .map(|body| open_responses_response_body(Some(body), &JsonValue::Null))
        {
            response = response.with_body(body);
        }

        if let Some(headers) = context.response_headers {
            response = response_metadata_with_headers(response, headers);
        }

        result = result.with_response(response);
    }

    result
}

fn open_responses_error_stream_result(
    context: OpenResponsesErrorContext,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut result =
        LanguageModelStreamResult::new(vec![open_responses_stream_error_from_context(&context)])
            .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = context.response_headers {
        result = result.with_response(open_responses_stream_response_with_headers(headers));
    }

    result
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

fn open_responses_error_metadata(
    provider_name: &str,
    context: &OpenResponsesErrorContext,
) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    metadata.insert(
        provider_name.to_string(),
        open_responses_error_object(context),
    );
    metadata
}

fn open_responses_error_object(context: &OpenResponsesErrorContext) -> JsonObject {
    let mut provider = JsonObject::new();
    provider.insert(
        "errorMessage".to_string(),
        JsonValue::String(context.message.clone()),
    );

    if let Some(status_code) = context.status_code {
        provider.insert("statusCode".to_string(), json!(status_code));
    }

    if let Some(is_retryable) = context.is_retryable {
        provider.insert("isRetryable".to_string(), json!(is_retryable));
    }

    if let Some(error) = context.data.as_ref().and_then(|data| data.get("error")) {
        open_responses_insert_error_detail(&mut provider, error, "type", "errorType");
        open_responses_insert_error_detail(&mut provider, error, "param", "errorParam");
        open_responses_insert_error_detail(&mut provider, error, "code", "errorCode");
    }

    provider
}

fn open_responses_insert_error_detail(
    provider: &mut JsonObject,
    error: &JsonValue,
    source_key: &str,
    target_key: &str,
) {
    let Some(value) = error.get(source_key).filter(|value| !value.is_null()) else {
        return;
    };

    provider.insert(target_key.to_string(), value.clone());
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
    use crate::file_data::{FileData, FileDataContent};
    use crate::generate_object::{GenerateObjectOptions, generate_object};
    use crate::generate_text::{GenerateTextInclude, GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelFilePart,
        LanguageModelMessage, LanguageModelProviderTool, LanguageModelStreamPart,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolChoice,
        LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderMetadata, ProviderOptions};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        Tool, json_schema,
    };
    use crate::stream_text::{StreamTextOptions, TextStreamPart, stream_text};
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

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
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false,
                "metadata": {
                    "trace": "responses-test"
                }
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_provider_options(provider_options),
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
                "temperature": 0.0,
                "store": false,
                "metadata": {
                    "trace": "responses-test"
                }
            }))
        );
    }

    #[test]
    fn open_responses_provider_converts_user_file_prompt_parts() {
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
                        "id": "resp_files",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "File prompt accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
                            "output_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_messages(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                            "Summarize these inputs",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/photo.jpg")
                                    .expect("url parses"),
                            },
                            "image/jpeg",
                        )),
                        LanguageModelUserContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Base64("JVBERi0=".to_string()),
                                },
                                "application/pdf",
                            )
                            .with_filename("report.pdf"),
                        ),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/report.pdf")
                                    .expect("url parses"),
                            },
                            "application/pdf",
                        )),
                    ]),
                )]),
            )
            .expect("prompt is valid"),
        ));

        assert_eq!(result.text, "File prompt accepted");

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
                "model": "gpt-4.1-mini",
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Summarize these inputs"
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,AAECAw=="
                            },
                            {
                                "type": "input_image",
                                "image_url": "https://example.com/photo.jpg"
                            },
                            {
                                "type": "input_file",
                                "filename": "report.pdf",
                                "file_data": "data:application/pdf;base64,JVBERi0="
                            },
                            {
                                "type": "input_file",
                                "file_url": "https://example.com/report.pdf"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_generates_object_with_json_schema_response_format() {
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
                        "id": "resp_object",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "{\"answer\":\"Open Responses object\",\"count\":3}"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 6
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let object_schema: JsonObject = serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                },
                "count": {
                    "type": "integer"
                }
            },
            "required": ["answer", "count"],
            "additionalProperties": false
        }))
        .expect("schema deserializes");

        let result = poll_ready(generate_object(
            GenerateObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("Return a JSON object with answer and count."),
            )
            .expect("prompt is valid")
            .with_schema(json_schema(object_schema.clone()))
            .with_schema_name("answer_object")
            .with_schema_description("An answer object.")
            .with_max_output_tokens(32)
            .with_temperature(0.0),
        ))
        .expect("object is generated");

        assert_eq!(
            result.object,
            json!({
                "answer": "Open Responses object",
                "count": 3
            })
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.total, Some(6));
        assert!(result.warnings.as_ref().is_none_or(Vec::is_empty));

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
        assert_eq!(request_body["model"], "gpt-4.1-mini");
        assert_eq!(request_body["max_output_tokens"], 32);
        assert_eq!(request_body["temperature"], 0.0);
        assert_eq!(
            request_body["text"]["format"],
            json!({
                "type": "json_schema",
                "name": "answer_object",
                "description": "An answer object.",
                "schema": object_schema,
                "strict": true
            })
        );
    }

    #[test]
    fn open_responses_provider_prepares_openai_hosted_tools() {
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
                        "id": "resp_tools",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 7,
                            "output_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use hosted tools"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "liveSearch",
                    json_object(json!({
                        "externalWebAccess": true,
                        "filters": {
                            "allowedDomains": ["example.com", "docs.rs"]
                        },
                        "searchContextSize": "high",
                        "userLocation": {
                            "type": "approximate",
                            "country": "US",
                            "city": "San Francisco",
                            "region": "California",
                            "timezone": "America/Los_Angeles"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.file_search",
                    "fileSearch",
                    json_object(json!({
                        "vectorStoreIds": ["vs_123"],
                        "maxNumResults": 5,
                        "ranking": {
                            "ranker": "auto",
                            "scoreThreshold": 0.25
                        },
                        "filters": {
                            "type": "eq",
                            "key": "kind",
                            "value": "docs"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.code_interpreter",
                    "codeRunner",
                    json_object(json!({
                        "container": {
                            "fileIds": ["file_123", "file_456"]
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.custom",
                    "write_sql",
                    json_object(json!({
                        "description": "Write SQL statements.",
                        "format": {
                            "type": "grammar",
                            "syntax": "lark",
                            "definition": "start: SELECT"
                        }
                    })),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "liveSearch".to_string(),
                }),
        ));

        assert_eq!(result.text, "Hosted tools prepared");
        assert!(result.warnings.is_empty());

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "web_search",
                    "external_web_access": true,
                    "filters": {
                        "allowed_domains": ["example.com", "docs.rs"]
                    },
                    "search_context_size": "high",
                    "user_location": {
                        "type": "approximate",
                        "country": "US",
                        "city": "San Francisco",
                        "region": "California",
                        "timezone": "America/Los_Angeles"
                    }
                },
                {
                    "type": "file_search",
                    "vector_store_ids": ["vs_123"],
                    "max_num_results": 5,
                    "ranking_options": {
                        "ranker": "auto",
                        "score_threshold": 0.25
                    },
                    "filters": {
                        "type": "eq",
                        "key": "kind",
                        "value": "docs"
                    }
                },
                {
                    "type": "code_interpreter",
                    "container": {
                        "type": "auto",
                        "file_ids": ["file_123", "file_456"]
                    }
                },
                {
                    "type": "custom",
                    "name": "write_sql",
                    "description": "Write SQL statements.",
                    "format": {
                        "type": "grammar",
                        "syntax": "lark",
                        "definition": "start: SELECT"
                    }
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "web_search"
            })
        );
    }

    #[test]
    fn open_responses_provider_maps_openai_hosted_tool_outputs() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_hosted_tool_outputs",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "ws_123",
                                "type": "web_search_call",
                                "status": "completed",
                                "action": {
                                    "type": "search",
                                    "query": "AI SDK Rust",
                                    "sources": [
                                        {
                                            "type": "url",
                                            "url": "https://example.com"
                                        }
                                    ]
                                }
                            },
                            {
                                "id": "fs_123",
                                "type": "file_search_call",
                                "status": "completed",
                                "queries": ["rust sdk"],
                                "results": [
                                    {
                                        "attributes": {
                                            "kind": "docs"
                                        },
                                        "file_id": "file_123",
                                        "filename": "guide.md",
                                        "score": 0.91,
                                        "text": "Guide text"
                                    }
                                ]
                            },
                            {
                                "id": "ci_123",
                                "type": "code_interpreter_call",
                                "status": "completed",
                                "code": "print(1)",
                                "container_id": "container_123",
                                "outputs": [
                                    {
                                        "type": "logs",
                                        "logs": "1"
                                    }
                                ]
                            },
                            {
                                "id": "ig_123",
                                "type": "image_generation_call",
                                "status": "completed",
                                "result": "base64-image"
                            },
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted tools completed"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 11,
                            "output_tokens": 7
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use hosted tools"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "liveSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.file_search",
                    "docSearch",
                    json_object(json!({
                        "vectorStoreIds": ["vs_123"]
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.code_interpreter",
                    "codeRunner",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.image_generation",
                    "imageMaker",
                    JsonObject::new(),
                ))),
        ));

        assert_eq!(result.text, "Hosted tools completed");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_calls.len(), 4);
        assert_eq!(result.tool_results.len(), 4);
        assert!(
            result
                .tool_calls
                .iter()
                .all(|tool_call| tool_call.provider_executed == Some(true))
        );
        assert!(
            result
                .tool_results
                .iter()
                .all(|tool_result| tool_result.provider_executed == Some(true))
        );
        assert_eq!(result.tool_calls[0].tool_name, "liveSearch");
        assert_eq!(result.tool_calls[0].provider_executed, Some(true));
        assert_eq!(result.tool_results[0].tool_name, "liveSearch");
        assert_eq!(
            result.tool_results[0].output,
            json!({
                "action": {
                    "type": "search",
                    "query": "AI SDK Rust"
                },
                "sources": [
                    {
                        "type": "url",
                        "url": "https://example.com"
                    }
                ]
            })
        );
        assert_eq!(result.tool_calls[1].tool_name, "docSearch");
        assert_eq!(
            result.tool_results[1].output,
            json!({
                "queries": ["rust sdk"],
                "results": [
                    {
                        "attributes": {
                            "kind": "docs"
                        },
                        "fileId": "file_123",
                        "filename": "guide.md",
                        "score": 0.91,
                        "text": "Guide text"
                    }
                ]
            })
        );
        assert_eq!(result.tool_calls[2].tool_name, "codeRunner");
        assert_eq!(
            result.tool_calls[2].input,
            json!({
                "code": "print(1)",
                "containerId": "container_123"
            })
        );
        assert_eq!(
            result.tool_results[2].output,
            json!({
                "outputs": [
                    {
                        "type": "logs",
                        "logs": "1"
                    }
                ]
            })
        );
        assert_eq!(result.tool_calls[3].tool_name, "imageMaker");
        assert_eq!(
            result.tool_results[3].output,
            json!({
                "result": "base64-image"
            })
        );
    }

    #[test]
    fn open_responses_provider_maps_api_error_data_to_metadata_and_response() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    429,
                    "Too Many Requests",
                    json!({
                        "error": {
                            "message": "Quota exceeded",
                            "type": "insufficient_quota",
                            "param": "model",
                            "code": "quota_exceeded"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_open_responses_error".to_string(),
                )])))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_include(GenerateTextInclude::new().with_response_body(true)),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.text, "");
        let metadata = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("openai"))
            .expect("Open Responses error metadata is present");
        assert_eq!(
            metadata.get("errorMessage").and_then(JsonValue::as_str),
            Some("Quota exceeded")
        );
        assert_eq!(
            metadata.get("errorType").and_then(JsonValue::as_str),
            Some("insufficient_quota")
        );
        assert_eq!(
            metadata.get("errorParam").and_then(JsonValue::as_str),
            Some("model")
        );
        assert_eq!(
            metadata.get("errorCode").and_then(JsonValue::as_str),
            Some("quota_exceeded")
        );
        assert_eq!(
            metadata.get("statusCode").and_then(JsonValue::as_u64),
            Some(429)
        );
        assert_eq!(
            metadata.get("isRetryable").and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_open_responses_error")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref()),
            Some(&json!({
                "error": {
                    "message": "Quota exceeded",
                    "type": "insufficient_quota",
                    "param": "model",
                    "code": "quota_exceeded"
                }
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
            map_open_responses_finish_reason(Some("max_tokens"), false).unified,
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
    fn open_responses_provider_streams_text_with_request_and_response_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenResponsesTransport = Arc::new(
            move |request| -> OpenResponsesTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"Hello"}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":" from Responses"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":"Hello from Responses"}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":5,"output_tokens":4}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse)
                    .with_headers(Headers::from([
                        ("content-type".to_string(), "text/event-stream".to_string()),
                        (
                            "x-request-id".to_string(),
                            "req_open_responses_stream".to_string(),
                        ),
                    ])))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0)
                .with_include_raw_chunks(true),
        ));

        assert_eq!(result.text, "Hello from Responses");
        assert_eq!(result.text_stream, vec!["Hello", " from Responses"]);
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.total, Some(4));
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.response.id.as_deref(), Some("resp_stream"));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_open_responses_stream")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("openai")
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_stream")
        );
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, TextStreamPart::Raw(_)))
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
                "temperature": 0.0,
                "stream": true
            }))
        );
    }

    #[test]
    fn open_responses_provider_preserves_stream_error_event_data() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_error","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"error","sequence_number":1,"error":{"type":"server_error","code":"server_error","message":"response failed","param":null}}"#,
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
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_stream_error")
        );
        let error = result.errors.first().expect("stream error is captured");
        assert_eq!(error.get("type").and_then(JsonValue::as_str), Some("error"));
        assert_eq!(
            error
                .get("error")
                .and_then(|error| error.get("type"))
                .and_then(JsonValue::as_str),
            Some("server_error")
        );
        assert_eq!(
            error
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(JsonValue::as_str),
            Some("server_error")
        );
        assert_eq!(
            error
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(JsonValue::as_str),
            Some("response failed")
        );
    }

    #[test]
    fn open_responses_streams_function_call_argument_deltas() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_tool","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":""}}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"location\""}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"location\":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":""}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_tool","created_at":1711115037,"model":"gpt-4.1-mini","output":[{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":"{\"location\":\"Brisbane\"}"}],"usage":{"input_tokens":6,"output_tokens":3}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let stream_result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Weather?")),
            ])),
        ])));

        let tool_call = stream_result
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
        assert_eq!(
            stream_result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => {
                    Some(finish.finish_reason.unified.clone())
                }
                _ => None,
            }),
            Some(FinishReason::ToolCalls)
        );
    }

    #[test]
    fn open_responses_provider_runs_generate_text_tool_loop_end_to_end() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                let call_number = {
                    let mut requests = captured_requests_for_transport
                        .lock()
                        .expect("captured requests mutex is not poisoned");
                    requests.push(request.clone());
                    requests.len()
                };

                let body = if call_number == 1 {
                    json!({
                        "id": "resp_tool_call",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "fc_weather",
                                "type": "function_call",
                                "call_id": "call_weather",
                                "name": "weather",
                                "arguments": "{\"location\":\"Brisbane\"}"
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
                            "output_tokens": 3
                        }
                    })
                } else {
                    json!({
                        "id": "resp_tool_final",
                        "created_at": 1711115038,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Brisbane is sunny."
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 4
                        }
                    })
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    body.to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new("openai", "https://api.openai.test/v1/responses")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string"
                }
            },
            "required": ["location"]
        }))
        .expect("schema deserializes");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Weather in Brisbane?"))
                .expect("prompt is valid")
                .with_tool(
                    Tool::new("weather", input_schema.clone())
                        .with_description("Get weather for a location")
                        .with_execute(|input, options| async move {
                            Ok(json!({
                                "location": input
                                    .get("location")
                                    .and_then(JsonValue::as_str)
                                    .unwrap_or("Brisbane"),
                                "forecast": "sunny",
                                "toolCallId": options.tool_call_id
                            }))
                        }),
                )
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "weather".to_string(),
                })
                .with_max_steps(2),
        ));

        assert_eq!(result.text, "Brisbane is sunny.");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_results.len(), 1);

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .clone();
        assert_eq!(requests.len(), 2);

        let first_body = requests[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("first request body is JSON");
        assert_eq!(
            first_body.get("tools"),
            Some(&json!([
                {
                    "type": "function",
                    "name": "weather",
                    "description": "Get weather for a location",
                    "parameters": input_schema
                }
            ]))
        );
        assert_eq!(
            first_body.get("tool_choice"),
            Some(&json!({
                "type": "function",
                "name": "weather"
            }))
        );

        let second_body = requests[1]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("second request body is JSON");
        let second_input = second_body
            .get("input")
            .and_then(JsonValue::as_array)
            .expect("second request input is an array");
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("function_call")
                && item.get("call_id").and_then(JsonValue::as_str) == Some("call_weather")
        }));
        assert!(second_input.iter().any(|item| {
            item.get("type").and_then(JsonValue::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(JsonValue::as_str) == Some("call_weather")
                && item
                    .get("output")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|output| output.contains("\"forecast\":\"sunny\""))
        }));
    }

    fn json_object(value: JsonValue) -> JsonObject {
        serde_json::from_value(value).expect("value is a JSON object")
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
